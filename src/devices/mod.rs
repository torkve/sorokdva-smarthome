#[cfg(not(any(feature = "wirenboard", feature = "influx")))]
compile_error!("at least one device family feature (wirenboard, influx) must be enabled");

#[cfg(feature = "influx")]
pub mod influx;
#[cfg(feature = "wirenboard")]
pub mod wirenboard;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};

use crate::mqtt::MqttHandle;
use crate::protocol::consts::{ActionError, ActionStatus};
use crate::protocol::device::{ActionException, DeviceCore};
use crate::tasks::TaskSpawner;

/// Behaviour of a concrete device: reacting to MQTT updates and API actions.
/// All methods are synchronous and run under the device mutex; long effects
/// must be spawned through TaskSpawner / MqttHandle.
pub trait DeviceLogic: Send {
    /// Handle a capability change. Ok(None) reports a plain DONE result;
    /// Ok(Some(value)) additionally returns a value payload in the action
    /// response state (the video_stream capability answers get_stream with
    /// {"stream_url": ..., "protocol": ...} this way).
    fn on_action(
        &mut self,
        core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        value: &Value,
        opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException>;

    /// Handle a status message. Returning Some(delay) asks the dispatcher to
    /// call on_settle after that quiet period — for devices whose state is
    /// derived from several topics that do not update atomically.
    fn on_mqtt(
        &mut self,
        _core: &mut DeviceCore,
        _topic: &str,
        _payload: &str,
    ) -> Option<std::time::Duration> {
        None
    }

    /// Deferred recomputation requested by on_mqtt. Must be idempotent: the
    /// dispatcher may call it after the state already settled via a
    /// faster path.
    fn on_settle(&mut self, _core: &mut DeviceCore) {}
}

/// A no-behaviour logic for devices whose capabilities cannot be changed.
pub struct NoLogic;

impl DeviceLogic for NoLogic {
    fn on_action(
        &mut self,
        _core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        _value: &Value,
        _opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        Err(not_supported(type_id, instance))
    }
}

/// The default "change_value is not supported" failure.
pub fn not_supported(type_id: &str, instance: &str) -> ActionException {
    ActionException {
        capability_id: leak_type_id(type_id),
        instance: instance.to_string(),
        code: ActionError::NotSupportedInCurrentMode,
        message: None,
    }
}

/// Capability type ids are a small closed set; map to the static strings.
fn leak_type_id(type_id: &str) -> &'static str {
    use crate::protocol::capability as cap;
    match type_id {
        cap::ON_OFF => cap::ON_OFF,
        cap::COLOR_SETTING => cap::COLOR_SETTING,
        cap::MODE => cap::MODE,
        cap::RANGE => cap::RANGE,
        cap::TOGGLE => cap::TOGGLE,
        cap::VIDEO_STREAM => cap::VIDEO_STREAM,
        _ => "devices.capabilities.unknown",
    }
}

pub struct Device {
    pub core: DeviceCore,
    pub logic: Box<dyn DeviceLogic>,
}

/// A reportable state transition observed on a device: the values captured
/// at the moment of the change, not re-sampled later (a short PIR pulse
/// must deliver its rising edge even if the level is already gone).
#[derive(Debug)]
pub struct StateChange {
    pub device_id: String,
    /// Property/capability state objects, exactly as report() emits them.
    pub entries: Vec<Value>,
}

/// Where observed state transitions are sent (the notifications loop).
#[derive(Clone)]
pub struct StateSink(tokio::sync::mpsc::UnboundedSender<StateChange>);

impl StateSink {
    pub fn channel() -> (StateSink, tokio::sync::mpsc::UnboundedReceiver<StateChange>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (StateSink(tx), rx)
    }

    pub fn send(&self, change: StateChange) {
        let _ = self.0.send(change);
    }
}

/// Which states qualify for an immediate push: event properties are edge
/// phenomena (motion, leak) that a periodic sampler can miss entirely.
/// Capabilities and float properties stay on the periodic pass.
fn push_worthy_states(core: &DeviceCore) -> Vec<Option<Value>> {
    core.properties
        .iter()
        .map(|p| {
            if matches!(p, crate::protocol::Property::Event(_)) && p.retrievable() && p.reportable()
            {
                p.state()
            } else {
                None
            }
        })
        .collect()
}

/// Run a mutation under the device lock and emit any event-property
/// transitions to the sink. The values are captured inside the lock, and
/// the send happens after the guard is dropped.
pub fn with_notify<R>(
    device_id: &str,
    device: &SharedDevice,
    sink: Option<&StateSink>,
    f: impl FnOnce(&mut Device) -> R,
) -> R {
    let (result, entries) = {
        let mut guard = lock(device);
        let before = sink.map(|_| push_worthy_states(&guard.core));
        let result = f(&mut guard);
        let entries: Vec<Value> = match before {
            Some(before) => push_worthy_states(&guard.core)
                .into_iter()
                .zip(before)
                .filter_map(|(after, before)| match after {
                    Some(after) if Some(&after) != before.as_ref() => Some(after),
                    _ => None,
                })
                .collect(),
            None => Vec::new(),
        };
        (result, entries)
    };
    if !entries.is_empty() {
        if let Some(sink) = sink {
            sink.send(StateChange {
                device_id: device_id.to_string(),
                entries,
            });
        }
    }
    result
}

pub type SharedDevice = Arc<Mutex<Device>>;

pub fn lock(device: &SharedDevice) -> MutexGuard<'_, Device> {
    device.lock().unwrap_or_else(|e| e.into_inner())
}

/// Handle the devices/action API call for one device.
pub fn action(device: &SharedDevice, capabilities: &[Value]) -> Value {
    let mut guard = lock(device);
    let Device { core, logic } = &mut *guard;

    let changes = DeviceCore::parse_changes(capabilities);
    let mut results: Vec<Value> = Vec::new();

    for change in &changes {
        if core
            .find_capability(&change.type_id, &change.instance)
            .is_none()
        {
            results.push(serde_json::json!({
                "type": change.type_id,
                "state": {
                    "instance": change.instance,
                    "action_result": {
                        "status": ActionStatus::Error.as_str(),
                        "error_code": ActionError::InvalidAction.as_str(),
                        "error_message": "Unknown capability for this device",
                    }
                }
            }));
        }
    }

    for change in &changes {
        if core
            .find_capability(&change.type_id, &change.instance)
            .is_none()
        {
            continue;
        }
        match logic.on_action(
            core,
            &change.type_id,
            &change.instance,
            &change.value,
            &change.opts,
        ) {
            Ok(value) => {
                results.push(DeviceCore::action_result_done(
                    &change.type_id,
                    &change.instance,
                    value,
                ));
            }
            Err(e) => {
                results.push(DeviceCore::action_result_error(
                    e.capability_id,
                    &e.instance,
                    e.code,
                    &e.message(),
                ));
            }
        }
    }

    serde_json::json!({
        "id": lockless_id(core),
        "capabilities": results,
    })
}

fn lockless_id(core: &DeviceCore) -> String {
    core.device_id.clone()
}

/// Everything a device build may need.
pub struct BuildContext {
    pub mqtt: Option<MqttHandle>,
    pub tasks: TaskSpawner,
}

pub struct BuiltDevices {
    /// Devices keyed by id, in the config table's iteration order
    /// (alphabetical by id).
    pub devices: Vec<(String, SharedDevice)>,
    /// MQTT topic -> indices into `devices`.
    pub subscriptions: HashMap<String, Vec<usize>>,
    /// Devices with a background poll loop: (index into `devices`,
    /// spawner from the class table).
    pub pollers: Vec<(usize, PollerFn)>,
}

impl BuiltDevices {
    pub fn get(&self, device_id: &str) -> Option<&SharedDevice> {
        self.devices
            .iter()
            .find(|(id, _)| id == device_id)
            .map(|(_, device)| device)
    }
}

/// Reads device spec fields out of the config table through one shared
/// implementation with deny-unknown-fields semantics. Replaces a serde
/// derive per spec struct: those monomorphized into ~33KiB of binary for
/// what is a dozen string/int lookups per device class.
pub(crate) struct SpecReader {
    table: toml::Table,
}

impl SpecReader {
    pub fn new(spec: toml::Value) -> Result<Self> {
        match spec {
            toml::Value::Table(table) => Ok(SpecReader { table }),
            _ => bail!("invalid device spec: expected a table"),
        }
    }

    fn take(&mut self, key: &'static str) -> Option<toml::Value> {
        self.table.remove(key)
    }

    pub fn req_str(&mut self, key: &'static str) -> Result<String> {
        match self.take(key) {
            Some(toml::Value::String(value)) => Ok(value),
            Some(_) => bail!("invalid type for `{key}`: expected a string"),
            None => bail!("missing field `{key}`"),
        }
    }

    pub fn opt_str(&mut self, key: &'static str) -> Result<Option<String>> {
        match self.take(key) {
            Some(toml::Value::String(value)) => Ok(Some(value)),
            Some(_) => bail!("invalid type for `{key}`: expected a string"),
            None => Ok(None),
        }
    }

    #[cfg_attr(not(feature = "wirenboard"), allow(dead_code))]
    pub fn req_i64(&mut self, key: &'static str) -> Result<i64> {
        match self.take(key) {
            Some(toml::Value::Integer(value)) => Ok(value),
            Some(_) => bail!("invalid type for `{key}`: expected an integer"),
            None => bail!("missing field `{key}`"),
        }
    }

    #[cfg_attr(not(feature = "wirenboard"), allow(dead_code))]
    pub fn opt_u64_or(&mut self, key: &'static str, default: u64) -> Result<u64> {
        match self.take(key) {
            Some(toml::Value::Integer(value)) => u64::try_from(value)
                .map_err(|_| anyhow!("invalid value for `{key}`: expected a non-negative integer")),
            Some(_) => bail!("invalid type for `{key}`: expected an integer"),
            None => Ok(default),
        }
    }

    /// deny_unknown_fields: everything must have been consumed.
    pub fn finish(self) -> Result<()> {
        if let Some(key) = self.table.keys().next() {
            bail!("unknown field `{key}`");
        }
        Ok(())
    }
}

/// Builds one device from its spec: returns the device and the MQTT
/// topics it subscribes to.
pub type BuildFn = fn(&str, toml::Value, &BuildContext) -> Result<(SharedDevice, Vec<String>)>;

/// Starts the background poll loop for one device of its class. Called
/// once per device from inside the tokio runtime; must spawn and return.
pub type PollerFn = fn(SharedDevice);

/// One registered device class. Each device family module exports its
/// classes as a `CLASSES` table; the compiled-in families are gathered
/// in [`TABLES`].
pub struct DeviceClass {
    pub name: &'static str,
    pub build: BuildFn,
    pub poller: Option<PollerFn>,
}

impl DeviceClass {
    pub const fn new(name: &'static str, build: BuildFn) -> Self {
        DeviceClass {
            name,
            build,
            poller: None,
        }
    }

    pub const fn polled(name: &'static str, build: BuildFn, poller: PollerFn) -> Self {
        DeviceClass {
            name,
            build,
            poller: Some(poller),
        }
    }
}

static TABLES: &[&[DeviceClass]] = &[
    #[cfg(feature = "wirenboard")]
    wirenboard::CLASSES,
    #[cfg(feature = "influx")]
    influx::CLASSES,
];

fn find_class(name: &str) -> Option<&'static DeviceClass> {
    TABLES.iter().copied().flatten().find(|c| c.name == name)
}

/// Names of all compiled-in device classes.
pub fn class_names() -> Vec<&'static str> {
    TABLES.iter().copied().flatten().map(|c| c.name).collect()
}

/// Start the poll loops of all poller devices. Must run inside the tokio
/// runtime.
pub fn spawn_pollers(built: &BuiltDevices) {
    for &(index, spawn) in &built.pollers {
        if let Some((_, device)) = built.devices.get(index) {
            spawn(device.clone());
        }
    }
}

/// Mirrors dialogs.app.make_app device instantiation.
pub fn build_all(devices_cfg: &toml::Table, ctx: &BuildContext) -> Result<BuiltDevices> {
    let mut result = BuiltDevices {
        devices: Vec::new(),
        subscriptions: HashMap::new(),
        pollers: Vec::new(),
    };

    for (device_id, spec) in devices_cfg {
        let mut table = spec
            .as_table()
            .cloned()
            .with_context(|| format!("devices.{device_id} must be a table"))?;
        let class = table
            .remove("_class")
            .and_then(|v| v.as_str().map(str::to_string))
            .with_context(|| format!("devices.{device_id} lacks _class"))?;
        let mqtt_used = table
            .remove("_mqtt_used")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if mqtt_used && ctx.mqtt.is_none() {
            bail!("Cannot initialize MQTT-based device: no MQTT config available");
        }
        let mqtt_ctx = BuildContext {
            mqtt: if mqtt_used { ctx.mqtt.clone() } else { None },
            tasks: ctx.tasks.clone(),
        };

        let index = result.devices.len();
        let (entry, device, topics) = (|| -> Result<_> {
            let entry = find_class(&class).ok_or_else(|| {
                anyhow!(
                    "unknown device class {class} (compiled in: {})",
                    class_names().join(", ")
                )
            })?;
            let (device, topics) = (entry.build)(device_id, toml::Value::Table(table), &mqtt_ctx)?;
            Ok((entry, device, topics))
        })()
        .with_context(|| format!("cannot initialize devices.{device_id}"))?;
        for capability in &lock(&device).core.capabilities {
            capability
                .validate()
                .map_err(|e| anyhow!("devices.{device_id}: {e}"))?;
        }
        for topic in topics {
            result.subscriptions.entry(topic).or_default().push(index);
        }
        if let Some(poller) = entry.poller {
            result.pollers.push((index, poller));
        }
        result.devices.push((device_id.clone(), device));
    }

    Ok(result)
}
