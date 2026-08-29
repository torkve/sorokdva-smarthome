//! Wirenboard-connected devices, driven over MQTT topics: lights
//! (on/off, dimmable, dual-channel mixwhite), sockets, PIR/leak/climate
//! sensors, curtains, water valves and coolers.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use log::{info, warn};
use serde_json::{Map, Number, Value};

use crate::mqtt::MqttHandle;
use crate::protocol::capability::{
    Capability, ColorSetting, ColorValue, ModeCap, OnOff, RangeCap, ToggleCap,
};
use crate::protocol::consts::ActionError;
use crate::protocol::device::{ActionException, DeviceCore, DeviceType};
use crate::protocol::property::{EventKind, EventProp, FloatKind, FloatProp, Property};
use crate::tasks::TaskSpawner;

use super::{BuildContext, Device, DeviceClass, DeviceLogic, SharedDevice, SpecReader};

/// The wirenboard device classes, registered into the class table.
pub static CLASSES: &[DeviceClass] = &[
    DeviceClass::new("WbCurtain", build_curtain),
    DeviceClass::new("WbSensor", build_sensor),
    DeviceClass::new("WbRtdRa", build_rtd_ra),
    DeviceClass::new("WbCooler", build_cooler),
    DeviceClass::new("WbSocket", build_socket),
    DeviceClass::new("WbLight", build_light),
    DeviceClass::new("WbDimmableLight", build_dimmable_light),
    DeviceClass::new("WbDimmableOnoffLight", build_dimmable_onoff_light),
    DeviceClass::new("WbMixwhiteLight", build_mixwhite_light),
    DeviceClass::new("WbWaterValve", build_water_valve),
    DeviceClass::new("WbLeakSensor", build_leak_sensor),
    DeviceClass::new("WbPirSensor", build_pir_sensor),
];

fn require_mqtt(ctx: &BuildContext, class: &str) -> Result<MqttHandle> {
    ctx.mqtt
        .clone()
        .ok_or_else(|| anyhow::anyhow!("device class {class} requires MQTT (_mqtt_used = true)"))
}

pub(crate) fn nf(value: f64) -> Number {
    Number::from_f64(value).unwrap_or_else(|| Number::from(0))
}

/// Percent as a JSON number: the *int* 0 whenever percent <= 0, the
/// float otherwise (the int/float distinction survives serialization).
fn percent_number(percent: f64) -> Number {
    if percent <= 0. {
        Number::from(0)
    } else {
        nf(percent)
    }
}

/// Truthiness of the JSON bool/number an action passes (any non-zero
/// number counts as true).
fn value_as_bool(value: &Value) -> Option<bool> {
    value
        .as_bool()
        .or_else(|| value.as_i64().map(|n| n != 0))
        .or_else(|| value.as_f64().map(|n| n != 0.))
}

fn invalid_value(type_id: &'static str, instance: &str) -> ActionException {
    ActionException::new(type_id, instance, ActionError::InvalidValue)
}

fn device_busy(type_id: &'static str, instance: &str) -> ActionException {
    ActionException::new(type_id, instance, ActionError::DeviceBusy)
}

fn relative_flag(opts: &Map<String, Value>) -> bool {
    opts.get("relative")
        .and_then(value_as_bool)
        .unwrap_or(false)
}

/// Decimal float representation: finite whole values below 1e16 render
/// with one fractional digit ("25.0"); everything else uses the default
/// shortest form.
fn decimal_float_str(value: f64) -> String {
    if value.is_finite() && value.fract() == 0. && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

fn get_onoff(core: &mut DeviceCore, index: usize) -> Option<&mut OnOff> {
    match core.capabilities.get_mut(index) {
        Some(Capability::OnOff(c)) => Some(c),
        _ => None,
    }
}

fn get_range(core: &mut DeviceCore, index: usize) -> Option<&mut RangeCap> {
    match core.capabilities.get_mut(index) {
        Some(Capability::Range(c)) => Some(c),
        _ => None,
    }
}

fn get_mode(core: &mut DeviceCore, index: usize) -> Option<&mut ModeCap> {
    match core.capabilities.get_mut(index) {
        Some(Capability::Mode(c)) => Some(c),
        _ => None,
    }
}

fn get_toggle(core: &mut DeviceCore, index: usize) -> Option<&mut ToggleCap> {
    match core.capabilities.get_mut(index) {
        Some(Capability::Toggle(c)) => Some(c),
        _ => None,
    }
}

fn get_color(core: &mut DeviceCore, index: usize) -> Option<&mut ColorSetting> {
    match core.capabilities.get_mut(index) {
        Some(Capability::Color(c)) => Some(c),
        _ => None,
    }
}

fn get_float(core: &mut DeviceCore, index: usize) -> Option<&mut FloatProp> {
    match core.properties.get_mut(index) {
        Some(Property::Float(p)) => Some(p),
        _ => None,
    }
}

fn get_event(core: &mut DeviceCore, index: usize) -> Option<&mut EventProp> {
    match core.properties.get_mut(index) {
        Some(Property::Event(p)) => Some(p),
        _ => None,
    }
}

fn assign_float(core: &mut DeviceCore, index: usize, payload: &str, log_target: &str) {
    let Ok(value) = payload.trim().parse::<f64>() else {
        warn!(target: "mqtt", "{log_target}: cannot parse float payload {payload:?}");
        return;
    };
    if let Some(prop) = get_float(core, index) {
        if let Err(e) = prop.assign_f64(value) {
            warn!(target: "mqtt", "{log_target}: {e}");
        }
    }
}

fn parse_int_bool(payload: &str) -> Option<bool> {
    payload.trim().parse::<i64>().ok().map(|n| n != 0)
}

fn base_core(
    device_id: &str,
    device_type: DeviceType,
    name: &str,
    description: &Option<String>,
    room: &Option<String>,
    model: &str,
    manufacturer: &str,
) -> DeviceCore {
    let mut core = DeviceCore::new(device_id, device_type);
    core.name = Some(name.to_string());
    core.description = description.clone();
    core.room = room.clone();
    core.manufacturer = Some(manufacturer.to_string());
    core.model = Some(model.to_string());
    core
}

fn onoff_cap() -> Capability {
    Capability::OnOff(OnOff {
        value: None,
        retrievable: true,
        reportable: true,
        split: false,
    })
}

fn shared(core: DeviceCore, logic: impl DeviceLogic + 'static) -> SharedDevice {
    Arc::new(Mutex::new(Device {
        core,
        logic: Box::new(logic),
    }))
}

// ---------------------------------------------------------------- on-off family

/// WbLight / WbSocket / WbCooler: a single on-off control.
struct OnOffLogic {
    mqtt: MqttHandle,
    control_path: String,
    log_target: &'static str,
    log_noun: &'static str,
}

impl DeviceLogic for OnOffLogic {
    fn on_action(
        &mut self,
        _core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        value: &Value,
        _opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        let value = value_as_bool(value)
            .ok_or_else(|| invalid_value(crate::protocol::capability::ON_OFF, instance))?;
        let _ = type_id;
        info!(target: self.log_target, "Switching {} to {}", self.log_noun, py_bool(value));
        self.mqtt
            .send(&self.control_path, if value { "1" } else { "0" });
        Ok(None)
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, _topic: &str, payload: &str) -> Option<Duration> {
        if let Some(value) = parse_int_bool(payload) {
            if let Some(onoff) = get_onoff(core, 0) {
                onoff.value = Some(value);
            }
        }
        None
    }
}

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn build_onoff_device(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
    class: &str,
    device_type: DeviceType,
    log_target: &'static str,
    log_noun: &'static str,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let status_path = spec.req_str("status_path")?;
    let control_path = spec.req_str("control_path")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let mqtt = require_mqtt(ctx, class)?;

    let mut core = base_core(
        device_id,
        device_type,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.capabilities.push(onoff_cap());

    let logic = OnOffLogic {
        mqtt,
        control_path,
        log_target,
        log_noun,
    };
    Ok((shared(core, logic), vec![status_path]))
}

fn build_light(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    build_onoff_device(
        device_id,
        spec,
        ctx,
        "WbLight",
        DeviceType::Light,
        "wb.light",
        "light",
    )
}

fn build_socket(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    build_onoff_device(
        device_id,
        spec,
        ctx,
        "WbSocket",
        DeviceType::Socket,
        "wb.socket",
        "socket",
    )
}

fn build_cooler(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    build_onoff_device(
        device_id,
        spec,
        ctx,
        "WbCooler",
        DeviceType::Switch,
        "wb.cooler",
        "cooler",
    )
}

// ---------------------------------------------------------------- water valve

struct WaterValveLogic {
    mqtt: MqttHandle,
    control_path: String,
    alarm_control_path: String,
}

impl DeviceLogic for WaterValveLogic {
    fn on_action(
        &mut self,
        _core: &mut DeviceCore,
        _type_id: &str,
        instance: &str,
        value: &Value,
        _opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        let value = value_as_bool(value)
            .ok_or_else(|| invalid_value(crate::protocol::capability::ON_OFF, instance))?;
        info!(target: "wb.water_valve", "Switching water to {}", py_bool(value));
        self.mqtt
            .send(&self.control_path, if value { "0" } else { "1" });
        if value {
            self.mqtt.send(&self.alarm_control_path, "0");
        }
        Ok(None)
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, _topic: &str, payload: &str) -> Option<Duration> {
        if let Some(value) = parse_int_bool(payload) {
            if let Some(onoff) = get_onoff(core, 0) {
                onoff.value = Some(!value);
            }
        }
        None
    }
}

fn build_water_valve(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let status_path = spec.req_str("status_path")?;
    let control_path = spec.req_str("control_path")?;
    let alarm_control_path = spec.req_str("alarm_control_path")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let mqtt = require_mqtt(ctx, "WbWaterValve")?;

    let mut core = base_core(
        device_id,
        DeviceType::Switch,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.capabilities.push(onoff_cap());

    let logic = WaterValveLogic {
        mqtt,
        control_path,
        alarm_control_path,
    };
    Ok((shared(core, logic), vec![status_path]))
}

// ---------------------------------------------------------------- event sensors

struct EventSensorLogic {
    kind: EventKind,
}

impl DeviceLogic for EventSensorLogic {
    fn on_action(
        &mut self,
        _core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        _value: &Value,
        _opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        Err(super::not_supported(type_id, instance))
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, _topic: &str, payload: &str) -> Option<Duration> {
        let value = match self.kind {
            EventKind::WaterLeak => {
                if payload == "1" {
                    "leak"
                } else {
                    "dry"
                }
            }
            EventKind::Motion => {
                if payload == "0" {
                    "detected"
                } else {
                    "not_detected"
                }
            }
            _ => return None,
        };
        if let Some(event) = get_event(core, 0) {
            event.value = Some(value);
        }
        None
    }
}

fn build_event_sensor(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
    class: &str,
    kind: EventKind,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let status_path = spec.req_str("status_path")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let _mqtt = require_mqtt(ctx, class)?;

    let mut core = base_core(
        device_id,
        DeviceType::Sensor,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.properties
        .push(Property::Event(EventProp::new(kind, true, true)));

    Ok((shared(core, EventSensorLogic { kind }), vec![status_path]))
}

fn build_leak_sensor(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    build_event_sensor(device_id, spec, ctx, "WbLeakSensor", EventKind::WaterLeak)
}

fn build_pir_sensor(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    build_event_sensor(device_id, spec, ctx, "WbPirSensor", EventKind::Motion)
}

// ---------------------------------------------------------------- WB-MSW sensor

struct SensorLogic {
    temperature_path: Option<String>,
    humidity_path: Option<String>,
    temperature_index: Option<usize>,
    humidity_index: Option<usize>,
}

impl DeviceLogic for SensorLogic {
    fn on_action(
        &mut self,
        _core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        _value: &Value,
        _opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        Err(super::not_supported(type_id, instance))
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, topic: &str, payload: &str) -> Option<Duration> {
        if self.temperature_path.as_deref() == Some(topic) {
            if let Some(index) = self.temperature_index {
                assign_float(core, index, payload, "wb.sensor.temperature");
            }
        } else if self.humidity_path.as_deref() == Some(topic) {
            if let Some(index) = self.humidity_index {
                assign_float(core, index, payload, "wb.sensor.humidity");
            }
        }
        None
    }
}

fn build_sensor(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    let temperature_path = spec.opt_str("temperature_path")?;
    let humidity_path = spec.opt_str("humidity_path")?;
    // accepted and counted as a configured property, but not exposed:
    // Yandex has no sound-level instance
    let sound_level_path = spec.opt_str("sound_level_path")?;
    let illuminance_path = spec.opt_str("illuminance_path")?;
    spec.finish()?;
    let _mqtt = require_mqtt(ctx, "WbSensor")?;

    if temperature_path.is_none()
        && humidity_path.is_none()
        && sound_level_path.is_none()
        && illuminance_path.is_none()
    {
        anyhow::bail!("At least one property path must be specified");
    }

    let mut core = base_core(
        device_id,
        DeviceType::Sensor,
        &name,
        &description,
        &room,
        "WB-MSW v.3",
        "Wirenboard",
    );

    let mut topics = Vec::new();
    let mut temperature_index = None;
    let mut humidity_index = None;
    if let Some(path) = &temperature_path {
        temperature_index = Some(core.properties.len());
        core.properties.push(Property::Float(FloatProp::new(
            FloatKind::TemperatureCelsius,
            true,
            true,
        )));
        topics.push(path.clone());
    }
    if let Some(path) = &humidity_path {
        humidity_index = Some(core.properties.len());
        core.properties.push(Property::Float(FloatProp::new(
            FloatKind::Humidity,
            true,
            true,
        )));
        topics.push(path.clone());
    }

    let logic = SensorLogic {
        temperature_path,
        humidity_path,
        temperature_index,
        humidity_index,
    };
    Ok((shared(core, logic), topics))
}

// ---------------------------------------------------------------- dimmable lights

const ONOFF_INDEX: usize = 0;
const LEVEL_INDEX: usize = 1;

fn brightness_range(range_low: i64, range_high: i64) -> Capability {
    Capability::Range(RangeCap {
        instance: "brightness",
        unit: Some("unit.percent"),
        random_access: Some(true),
        min_value: Some(nf(0.)),
        max_value: Some(nf(100.)),
        precision: Some(if range_high - range_low < 500 {
            nf(1.)
        } else {
            nf(0.1)
        }),
        value: None,
        retrievable: true,
        reportable: true,
    })
}

struct DimmableLightLogic {
    mqtt: MqttHandle,
    control_path: String,
    range_off: i64,
    range_low: i64,
    range_high: i64,
    last_val: f64,
}

impl DimmableLightLogic {
    fn level_value(&self, level: f64) -> i64 {
        let real = (level / 100. * (self.range_high - self.range_low) as f64
            + self.range_low as f64) as i64;
        if real <= self.range_low {
            // fixes on/off button logic when the dimmer has an off range
            self.range_off
        } else {
            real
        }
    }
}

impl DeviceLogic for DimmableLightLogic {
    fn on_action(
        &mut self,
        core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        value: &Value,
        opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        if type_id == crate::protocol::capability::RANGE {
            let mut value = value
                .as_f64()
                .ok_or_else(|| invalid_value(crate::protocol::capability::RANGE, instance))?;
            if relative_flag(opts) {
                let current = get_range(core, LEVEL_INDEX).and_then(|c| c.value.clone()?.as_f64());
                match current {
                    Some(current) => value += current,
                    None => {
                        return Err(device_busy(crate::protocol::capability::RANGE, instance));
                    }
                }
            }
            let real = self.level_value(value);
            info!(target: "wb.dimlight", "Switching light to {value} (real value {real})");
            self.mqtt.send(&self.control_path, real.to_string());
            Ok(None)
        } else {
            let value = value_as_bool(value)
                .ok_or_else(|| invalid_value(crate::protocol::capability::ON_OFF, instance))?;
            let target = if value { self.last_val } else { 0. };
            let real = self.level_value(target);
            info!(target: "wb.dimlight", "Switching light to {target} (real value {real})");
            self.mqtt.send(&self.control_path, real.to_string());
            Ok(None)
        }
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, _topic: &str, payload: &str) -> Option<Duration> {
        let Ok(raw) = payload.trim().parse::<i64>() else {
            warn!(target: "mqtt", "wb.dimlight: cannot parse payload {payload:?}");
            return None;
        };
        let percent =
            (raw - self.range_low) as f64 / (self.range_high - self.range_low) as f64 * 100.;
        if let Some(level) = get_range(core, LEVEL_INDEX) {
            level.value = Some(percent_number(percent));
        }
        if let Some(onoff) = get_onoff(core, ONOFF_INDEX) {
            onoff.value = Some(percent > 0.);
        }
        if percent > 0. {
            self.last_val = percent;
        }
        None
    }
}

fn build_dimmable_light(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let status_path = spec.req_str("status_path")?;
    let control_path = spec.req_str("control_path")?;
    let range_off = spec.req_i64("range_off")?;
    let range_low = spec.req_i64("range_low")?;
    let range_high = spec.req_i64("range_high")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let mqtt = require_mqtt(ctx, "WbDimmableLight")?;

    let mut core = base_core(
        device_id,
        DeviceType::Light,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.capabilities.push(onoff_cap());
    core.capabilities
        .push(brightness_range(range_low, range_high));

    let logic = DimmableLightLogic {
        mqtt,
        control_path,
        range_off,
        range_low,
        range_high,
        last_val: 100.,
    };
    Ok((shared(core, logic), vec![status_path]))
}

struct DimmableOnoffLightLogic {
    mqtt: MqttHandle,
    brightness_status_path: String,
    brightness_control_path: String,
    onoff_control_path: String,
    range_low: i64,
    range_high: i64,
}

impl DeviceLogic for DimmableOnoffLightLogic {
    fn on_action(
        &mut self,
        core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        value: &Value,
        opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        if type_id == crate::protocol::capability::RANGE {
            let mut value = value
                .as_f64()
                .ok_or_else(|| invalid_value(crate::protocol::capability::RANGE, instance))?;
            if relative_flag(opts) {
                let current = get_range(core, LEVEL_INDEX).and_then(|c| c.value.clone()?.as_f64());
                match current {
                    Some(current) => value += current,
                    None => {
                        return Err(device_busy(crate::protocol::capability::RANGE, instance));
                    }
                }
            }
            let real = (value / 100. * (self.range_high - self.range_low) as f64
                + self.range_low as f64) as i64;
            info!(target: "wb.dimlight", "Switching light to {value} (real value {real})");
            self.mqtt
                .send(&self.brightness_control_path, real.to_string());
            Ok(None)
        } else {
            let value = value_as_bool(value)
                .ok_or_else(|| invalid_value(crate::protocol::capability::ON_OFF, instance))?;
            info!(target: "wb.dimlight", "Switching light to {}", py_bool(value));
            self.mqtt
                .send(&self.onoff_control_path, if value { "1" } else { "0" });
            Ok(None)
        }
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, topic: &str, payload: &str) -> Option<Duration> {
        if topic == self.brightness_status_path {
            let Ok(raw) = payload.trim().parse::<i64>() else {
                warn!(target: "mqtt", "wb.dimlight: cannot parse payload {payload:?}");
                return None;
            };
            let percent =
                (raw - self.range_low) as f64 / (self.range_high - self.range_low) as f64 * 100.;
            if let Some(level) = get_range(core, LEVEL_INDEX) {
                level.value = Some(percent_number(percent));
            }
        } else if let Some(onoff) = get_onoff(core, ONOFF_INDEX) {
            onoff.value = Some(payload == "1");
        }
        None
    }
}

fn build_dimmable_onoff_light(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let brightness_status_path = spec.req_str("brightness_status_path")?;
    let brightness_control_path = spec.req_str("brightness_control_path")?;
    let onoff_status_path = spec.req_str("onoff_status_path")?;
    let onoff_control_path = spec.req_str("onoff_control_path")?;
    let range_low = spec.req_i64("range_low")?;
    let range_high = spec.req_i64("range_high")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let mqtt = require_mqtt(ctx, "WbDimmableOnoffLight")?;

    let mut core = base_core(
        device_id,
        DeviceType::Light,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.capabilities.push(onoff_cap());
    core.capabilities
        .push(brightness_range(range_low, range_high));

    let topics = vec![brightness_status_path.clone(), onoff_status_path];
    let logic = DimmableOnoffLightLogic {
        mqtt,
        brightness_status_path,
        brightness_control_path,
        onoff_control_path,
        range_low,
        range_high,
    };
    Ok((shared(core, logic), topics))
}

// ---------------------------------------------------------------- mixwhite light

const MIX_TEMPERATURE_INDEX: usize = 2;

struct MixwhiteLightLogic {
    mqtt: MqttHandle,
    warm_status_path: String,
    warm_control_path: String,
    cold_status_path: String,
    cold_control_path: String,
    warm_temperature: i64,
    cold_temperature: i64,
    range_low: i64,
    range_high: i64,
    /// Last raw channel values; None until the topic published at least once,
    /// so no state is derived from fabricated channels.
    warm_value: Option<i64>,
    cold_value: Option<i64>,
    /// Channels updated since the last derivation: when both are pending the
    /// pair is consistent and state settles immediately; a single pending
    /// channel settles after the quiet period instead.
    warm_pending: bool,
    cold_pending: bool,
    settle: Duration,
    /// Channel values we commanded and are waiting to see echoed back. While
    /// active (and not expired), a lone matching echo does not trigger a
    /// derivation: the partner echo of the same command is given up to
    /// EXPECTATION_WINDOW_FACTOR * settle to arrive.
    expectation: Option<CommandExpectation>,
    /// What the user last asked for, kept until the echoes confirm it (any
    /// settle clears it). Lets subsequent capability changes compose (set
    /// temperature + brightness together) instead of the later change
    /// reading the stale settled state.
    intended_temperature: Option<i64>,
    intended_brightness: Option<f64>,
    last_brightness_val: f64,
    last_temperature_val: i64,
}

/// The echoes a send_channels command still owes us.
struct CommandExpectation {
    cold: i64,
    warm: i64,
    cold_seen: bool,
    warm_seen: bool,
    /// false once any channel was resolved by a mismatching echo (external
    /// interference or controller clamping): the pair is still real, but it
    /// is not the commanded state and must not become the restore snapshot.
    clean: bool,
    created: tokio::time::Instant,
}

const EXPECTATION_WINDOW_FACTOR: u32 = 5;

impl MixwhiteLightLogic {
    fn value_to_ratio(&self, value: i64) -> f64 {
        ((value - self.range_low) as f64 / (self.range_high - self.range_low) as f64).clamp(0., 1.)
    }

    fn value_from_ratio(&self, ratio: f64) -> i64 {
        (ratio * (self.range_high - self.range_low) as f64 + self.range_low as f64) as i64
    }

    fn cold_and_warm_channels(&self, temperature: i64, brightness: f64) -> (i64, i64) {
        info!(
            target: "wb.mixwhiteight",
            "Calculating cold and warm channels for brightness {brightness} and {} <= T {temperature} <= {}",
            self.warm_temperature,
            self.cold_temperature,
        );
        if self.cold_temperature - temperature < temperature - self.warm_temperature {
            info!(target: "wb.mixwhiteight", "{temperature} is closer to cold temperature");
            let cold_value = self.value_from_ratio(brightness);
            let warm_ratio = brightness * (self.cold_temperature - temperature) as f64
                / (temperature - self.warm_temperature) as f64;
            (cold_value, self.value_from_ratio(warm_ratio))
        } else {
            info!(target: "wb.mixwhiteight", "{temperature} is closer to warm temperature");
            let warm_value = self.value_from_ratio(brightness);
            let cold_ratio = brightness * (temperature - self.warm_temperature) as f64
                / (self.cold_temperature - temperature) as f64;
            (self.value_from_ratio(cold_ratio), warm_value)
        }
    }

    fn send_channels(&self, cold_value: i64, warm_value: i64) {
        self.mqtt
            .send(&self.cold_control_path, cold_value.to_string());
        self.mqtt
            .send(&self.warm_control_path, warm_value.to_string());
    }

    fn expectation_window(&self) -> Duration {
        self.settle * EXPECTATION_WINDOW_FACTOR
    }

    /// Publish a channel pair and remember it as the expected echoes.
    /// A channel already holding the target value will produce no echo,
    /// so it counts as seen immediately.
    fn send_expecting(&mut self, cold_value: i64, warm_value: i64) {
        let cold_seen = self.cold_value == Some(cold_value);
        let warm_seen = self.warm_value == Some(warm_value);
        self.expectation = if cold_seen && warm_seen {
            None
        } else {
            Some(CommandExpectation {
                cold: cold_value,
                warm: warm_value,
                cold_seen,
                warm_seen,
                clean: true,
                created: tokio::time::Instant::now(),
            })
        };
        self.send_channels(cold_value, warm_value);
    }

    fn clear_expectation(&mut self) {
        self.expectation = None;
        self.intended_temperature = None;
        self.intended_brightness = None;
    }
}

impl DeviceLogic for MixwhiteLightLogic {
    fn on_action(
        &mut self,
        core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        value: &Value,
        opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        use crate::protocol::capability as cap;
        match type_id {
            cap::COLOR_SETTING => {
                let Some(temperature) = value.as_i64() else {
                    return Err(invalid_value(cap::COLOR_SETTING, instance));
                };
                let level_value = self
                    .intended_brightness
                    .or_else(|| {
                        get_range(core, LEVEL_INDEX).and_then(|c| c.value.clone()?.as_f64())
                    })
                    .unwrap_or(100.);
                let (cold_value, warm_value) =
                    self.cold_and_warm_channels(temperature, level_value / 100.);
                info!(
                    target: "wb.mixwhiteight",
                    "Switching temperature to {temperature} (cold {cold_value}, warm {warm_value})",
                );
                self.intended_temperature = Some(temperature);
                self.send_expecting(cold_value, warm_value);
                Ok(None)
            }
            cap::RANGE => {
                let temperature = self.intended_temperature.unwrap_or_else(|| {
                    match get_color(core, MIX_TEMPERATURE_INDEX).and_then(|c| c.value.clone()) {
                        Some(ColorValue::TemperatureK(t)) => t,
                        _ => self.last_temperature_val,
                    }
                });
                let mut value = value
                    .as_f64()
                    .ok_or_else(|| invalid_value(cap::RANGE, instance))?;
                if relative_flag(opts) {
                    let current = self.intended_brightness.or_else(|| {
                        get_range(core, LEVEL_INDEX).and_then(|c| c.value.clone()?.as_f64())
                    });
                    match current {
                        Some(current) => value += current,
                        None => return Err(device_busy(cap::RANGE, instance)),
                    }
                }
                let (cold_value, warm_value) =
                    self.cold_and_warm_channels(temperature, value / 100.);
                info!(
                    target: "wb.mixwhiteight",
                    "Switching brightness to {value} (cold {cold_value}, warm {warm_value})",
                );
                self.intended_brightness = Some(value);
                self.send_expecting(cold_value, warm_value);
                Ok(None)
            }
            _ => {
                let value =
                    value_as_bool(value).ok_or_else(|| invalid_value(cap::ON_OFF, instance))?;
                let (cold_value, warm_value) = if value {
                    self.cold_and_warm_channels(
                        self.last_temperature_val,
                        self.last_brightness_val / 100.,
                    )
                } else {
                    (0, 0)
                };
                info!(
                    target: "wb.mixwhitelight",
                    "Switching light to {} (cold {cold_value}, warm {warm_value})",
                    py_bool(value),
                );
                self.send_expecting(cold_value, warm_value);
                Ok(None)
            }
        }
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, topic: &str, payload: &str) -> Option<Duration> {
        let Ok(raw) = payload.trim().parse::<i64>() else {
            warn!(target: "mqtt", "wb.mixwhitelight: cannot parse payload {payload:?}");
            return None;
        };
        let is_warm = if topic == self.warm_status_path {
            self.warm_value = Some(raw);
            true
        } else if topic == self.cold_status_path {
            self.cold_value = Some(raw);
            false
        } else {
            return None;
        };

        // A command we sent may still owe us echoes: while the expectation
        // is fresh, a lone matching echo just waits for its partner (the
        // second control publish may not even have reached the controller
        // yet), so no state is derived from the half-applied pair.
        if self
            .expectation
            .as_ref()
            .is_some_and(|e| e.created.elapsed() > self.expectation_window())
        {
            self.clear_expectation();
        }
        if let Some(expectation) = &mut self.expectation {
            let (expected, seen) = if is_warm {
                (expectation.warm, &mut expectation.warm_seen)
            } else {
                (expectation.cold, &mut expectation.cold_seen)
            };
            *seen = true;
            if raw != expected {
                // external interference or controller clamping: this channel
                // is resolved by reality; keep waiting for its partner so no
                // half-applied pair is derived
                expectation.clean = false;
            }
            return if expectation.cold_seen && expectation.warm_seen {
                self.on_settle(core);
                None
            } else {
                Some(self.expectation_window())
            };
        }

        if is_warm {
            self.warm_pending = true;
        } else {
            self.cold_pending = true;
        }

        // The two channel values arrive as separate MQTT messages: deriving
        // state from one fresh and one stale channel produces a transient
        // wrong brightness/temperature (and corrupts the restore-on-turn-on
        // values). Settle immediately once both channels updated, otherwise
        // wait out a quiet period for the partner message.
        if self.warm_pending && self.cold_pending {
            self.on_settle(core);
            None
        } else {
            Some(self.settle)
        }
    }

    fn on_settle(&mut self, core: &mut DeviceCore) {
        // A stale timer armed before the current command must not wipe a
        // fresh expectation and derive a half-applied pair; the command's
        // own echoes (or its window timer) will settle it instead.
        if let Some(expectation) = &self.expectation {
            let fresh = expectation.created.elapsed() < self.expectation_window();
            let complete = expectation.cold_seen && expectation.warm_seen;
            if fresh && !complete {
                return;
            }
        }
        // Only a fully consistent snapshot may become the restore-on-turn-on
        // state: a cleanly confirmed command, or a paired external update.
        // Timer fallbacks and reality-resolved pairs still update the visible
        // state (they are what the light really shows) but may be transitional.
        let trusted = match &self.expectation {
            Some(expectation) => {
                expectation.cold_seen && expectation.warm_seen && expectation.clean
            }
            None => self.warm_pending && self.cold_pending,
        };
        self.warm_pending = false;
        self.cold_pending = false;
        self.clear_expectation();
        let (Some(warm_value), Some(cold_value)) = (self.warm_value, self.cold_value) else {
            return;
        };

        let warm_ratio = self.value_to_ratio(warm_value);
        let cold_ratio = self.value_to_ratio(cold_value);
        let percent = warm_ratio.max(cold_ratio) * 100.;

        if let Some(onoff) = get_onoff(core, ONOFF_INDEX) {
            onoff.value = Some(percent > 0.);
        }

        let level_unset = get_range(core, LEVEL_INDEX)
            .map(|c| c.value.is_none())
            .unwrap_or(true);
        if level_unset || percent > 0. {
            if let Some(level) = get_range(core, LEVEL_INDEX) {
                level.value = Some(nf(percent));
            }
        }

        if percent > 0. {
            let temperature = ((warm_ratio * self.warm_temperature as f64
                + cold_ratio * self.cold_temperature as f64)
                / (warm_ratio + cold_ratio)) as i64;
            // strange things occur sometimes
            let temperature = temperature
                .max(self.warm_temperature)
                .min(self.cold_temperature);
            if let Some(color) = get_color(core, MIX_TEMPERATURE_INDEX) {
                color.value = Some(ColorValue::TemperatureK(temperature));
            }
            if trusted {
                self.last_brightness_val = percent;
                self.last_temperature_val = temperature;
            }
        }
    }
}

fn build_mixwhite_light(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let warm_status_path = spec.req_str("warm_status_path")?;
    let warm_control_path = spec.req_str("warm_control_path")?;
    let cold_status_path = spec.req_str("cold_status_path")?;
    let cold_control_path = spec.req_str("cold_control_path")?;
    let warm_temperature = spec.req_i64("warm_temperature")?;
    let cold_temperature = spec.req_i64("cold_temperature")?;
    let range_low = spec.req_i64("range_low")?;
    let range_high = spec.req_i64("range_high")?;
    // quiet period before deriving state from a single-channel update;
    // paired warm+cold updates settle immediately
    let settle_ms = spec.opt_u64_or("settle_ms", 200)?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let mqtt = require_mqtt(ctx, "WbMixwhiteLight")?;

    let last_temperature_val = if warm_temperature <= 4500 && 4500 <= cold_temperature {
        4500
    } else {
        (warm_temperature + cold_temperature).div_euclid(2)
    };

    let mut core = base_core(
        device_id,
        DeviceType::Light,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.capabilities.push(onoff_cap());
    core.capabilities
        .push(brightness_range(range_low, range_high));
    core.capabilities.push(Capability::Color(ColorSetting {
        color_model: None,
        temperature: Some((Some(warm_temperature), Some(cold_temperature))),
        scenes: Vec::new(),
        value: Some(ColorValue::TemperatureK(last_temperature_val)),
        retrievable: true,
        reportable: true,
    }));

    let topics = vec![warm_status_path.clone(), cold_status_path.clone()];
    let logic = MixwhiteLightLogic {
        mqtt,
        warm_status_path,
        warm_control_path,
        cold_status_path,
        cold_control_path,
        warm_temperature,
        cold_temperature,
        range_low,
        range_high,
        warm_value: None,
        cold_value: None,
        warm_pending: false,
        cold_pending: false,
        settle: Duration::from_millis(settle_ms),
        expectation: None,
        intended_temperature: None,
        intended_brightness: None,
        last_brightness_val: 100.,
        last_temperature_val,
    };
    Ok((shared(core, logic), topics))
}

// ---------------------------------------------------------------- curtain

const CURTAIN_DIRECTION_INDEX: usize = 2;
const CURTAIN_MOTOR_INDEX: usize = 3;

struct CurtainLogic {
    mqtt: MqttHandle,
    tasks: TaskSpawner,
    task_key: String,
    direction_status_path: String,
    direction_control_path: String,
    motor_control_path: String,
    action_time_seconds: i64,
}

impl CurtainLogic {
    fn spawn_move(&self, direction_payload: String, hold: Duration) {
        let mqtt = self.mqtt.clone();
        let motor = self.motor_control_path.clone();
        let direction = self.direction_control_path.clone();
        self.tasks.spawn_replacing(&self.task_key, async move {
            mqtt.send(&motor, "0");
            mqtt.send(&direction, direction_payload);
            mqtt.send(&motor, "1");
            tokio::time::sleep(hold).await;
            mqtt.send(&motor, "0");
        });
    }
}

impl DeviceLogic for CurtainLogic {
    fn on_action(
        &mut self,
        _core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        value: &Value,
        _opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        use crate::protocol::capability as cap;
        match (type_id, instance) {
            (cap::ON_OFF, _) => {
                let value =
                    value_as_bool(value).ok_or_else(|| invalid_value(cap::ON_OFF, instance))?;
                info!(target: "wb", "Switching curtain to {}", py_bool(value));
                self.spawn_move(
                    if value { "1" } else { "0" }.to_string(),
                    Duration::from_secs(self.action_time_seconds.max(0) as u64),
                );
                Ok(None)
            }
            (cap::RANGE, _) => {
                let value = value
                    .as_f64()
                    .ok_or_else(|| invalid_value(cap::RANGE, instance))?;
                info!(target: "wb.curtain", "Shifting curtain to {value}");
                self.spawn_move(
                    if value < 0. { "1" } else { "0" }.to_string(),
                    Duration::from_secs(2),
                );
                Ok(None)
            }
            (cap::MODE, _) => {
                let value = value.as_str().unwrap_or("");
                let payload = match value {
                    "high" => "1",
                    "low" => "0",
                    _ => return Err(invalid_value(cap::MODE, instance)),
                };
                self.mqtt.send(&self.direction_control_path, payload);
                Ok(None)
            }
            (cap::TOGGLE, _) => {
                let value =
                    value_as_bool(value).ok_or_else(|| invalid_value(cap::TOGGLE, instance))?;
                self.mqtt
                    .send(&self.motor_control_path, if value { "1" } else { "0" });
                Ok(None)
            }
            _ => Err(super::not_supported(type_id, instance)),
        }
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, topic: &str, payload: &str) -> Option<Duration> {
        if topic == self.direction_status_path {
            let Ok(raw) = payload.trim().parse::<i64>() else {
                warn!(target: "mqtt", "wb.curtain: cannot parse payload {payload:?}");
                return None;
            };
            if let Some(direction) = get_mode(core, CURTAIN_DIRECTION_INDEX) {
                direction.value = Some(if raw != 0 { "high" } else { "low" });
            }
        } else if let Some(motor) = get_toggle(core, CURTAIN_MOTOR_INDEX) {
            motor.value = Some(payload == "1");
        }
        None
    }
}

fn build_curtain(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let direction_status_path = spec.req_str("direction_status_path")?;
    let motor_status_path = spec.req_str("motor_status_path")?;
    let direction_control_path = spec.req_str("direction_control_path")?;
    let motor_control_path = spec.req_str("motor_control_path")?;
    let action_time_seconds = spec.req_i64("action_time_seconds")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let mqtt = require_mqtt(ctx, "WbCurtain")?;

    let mut core = base_core(
        device_id,
        DeviceType::Curtain,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.capabilities.push(Capability::OnOff(OnOff {
        value: None,
        retrievable: false,
        reportable: false,
        split: true,
    }));
    core.capabilities.push(Capability::Range(RangeCap {
        instance: "open",
        unit: Some("unit.percent"),
        random_access: Some(false),
        min_value: Some(nf(0.)),
        max_value: Some(nf(100.)),
        precision: Some(nf(5.)),
        value: None,
        retrievable: false,
        reportable: false,
    }));
    core.capabilities.push(Capability::Mode(ModeCap {
        instance: "swing",
        modes: vec!["high", "low"],
        value: None,
        retrievable: true,
        reportable: true,
    }));
    core.capabilities.push(Capability::Toggle(ToggleCap {
        instance: "oscillation",
        value: None,
        retrievable: true,
        reportable: true,
    }));

    let topics = vec![direction_status_path.clone(), motor_status_path];
    let logic = CurtainLogic {
        mqtt,
        tasks: ctx.tasks.clone(),
        task_key: format!("curtain:{device_id}"),
        direction_status_path,
        direction_control_path,
        motor_control_path,
        action_time_seconds,
    };
    Ok((shared(core, logic), topics))
}

// ---------------------------------------------------------------- RTD-RA (Daikin AC)

const RTD_ONOFF_INDEX: usize = 0;
const RTD_SETPOINT_INDEX: usize = 1;
const RTD_FANSPEED_INDEX: usize = 2;
const RTD_MODE_INDEX: usize = 3;
const RTD_LOUVRE_INDEX: usize = 4;
const RTD_TEMPERATURE_INDEX: usize = 0;

const FANSPEED_MODES: &[(&str, &str)] = &[
    ("auto", "0"),
    ("one", "1"),
    ("two", "2"),
    ("three", "3"),
    ("four", "4"),
    ("five", "5"),
];
const HEAT_MODES: &[(&str, &str)] = &[
    ("auto", "0"),
    ("heat", "1"),
    ("fan_only", "2"),
    ("cool", "3"),
    ("dry", "4"),
];
const LOUVRE_MODES: &[(&str, &str)] = &[("stationary", "0"), ("vertical", "1")];

fn map_forward(map: &[(&'static str, &'static str)], mode: &str) -> Option<&'static str> {
    map.iter()
        .find(|(name, _)| *name == mode)
        .map(|(_, raw)| *raw)
}

fn map_reverse(map: &[(&'static str, &'static str)], raw: &str) -> Option<&'static str> {
    map.iter().find(|(_, r)| *r == raw).map(|(name, _)| *name)
}

struct RtdRaLogic {
    mqtt: MqttHandle,
    onoff_status_path: String,
    onoff_control_path: String,
    mode_status_path: String,
    mode_control_path: String,
    setpoint_status_path: String,
    setpoint_control_path: String,
    fanspeed_status_path: String,
    fanspeed_control_path: String,
    louvre_status_path: String,
    louvre_control_path: String,
    temperature_path: String,
}

impl DeviceLogic for RtdRaLogic {
    fn on_action(
        &mut self,
        core: &mut DeviceCore,
        type_id: &str,
        instance: &str,
        value: &Value,
        opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        use crate::protocol::capability as cap;
        match (type_id, instance) {
            (cap::ON_OFF, _) => {
                let value =
                    value_as_bool(value).ok_or_else(|| invalid_value(cap::ON_OFF, instance))?;
                self.mqtt
                    .send(&self.onoff_control_path, if value { "1" } else { "0" });
                Ok(None)
            }
            (cap::RANGE, _) => {
                let mut value = value
                    .as_f64()
                    .ok_or_else(|| invalid_value(cap::RANGE, instance))?;
                if relative_flag(opts) {
                    let current =
                        get_range(core, RTD_SETPOINT_INDEX).and_then(|c| c.value.clone()?.as_f64());
                    match current {
                        Some(current) => value += current,
                        None => return Err(device_busy(cap::RANGE, instance)),
                    }
                }
                let rounded = (value * 10.).round_ties_even() / 10.;
                self.mqtt
                    .send(&self.setpoint_control_path, decimal_float_str(rounded));
                Ok(None)
            }
            (cap::MODE, "thermostat") => {
                let value = value.as_str().unwrap_or("");
                match map_forward(HEAT_MODES, value) {
                    Some(raw) => {
                        self.mqtt.send(&self.mode_control_path, raw);
                        Ok(None)
                    }
                    None => Err(invalid_value(cap::MODE, instance)),
                }
            }
            (cap::MODE, "fan_speed") => {
                let value = value.as_str().unwrap_or("");
                match map_forward(FANSPEED_MODES, value) {
                    Some(raw) => {
                        self.mqtt.send(&self.fanspeed_control_path, raw);
                        Ok(None)
                    }
                    None => Err(invalid_value(cap::MODE, instance)),
                }
            }
            (cap::MODE, "swing") => {
                let value = value.as_str().unwrap_or("");
                match map_forward(LOUVRE_MODES, value) {
                    Some(raw) => {
                        self.mqtt.send(&self.louvre_control_path, raw);
                        Ok(None)
                    }
                    None => Err(invalid_value(cap::MODE, instance)),
                }
            }
            _ => Err(super::not_supported(type_id, instance)),
        }
    }

    fn on_mqtt(&mut self, core: &mut DeviceCore, topic: &str, payload: &str) -> Option<Duration> {
        if topic == self.onoff_status_path {
            if let Some(onoff) = get_onoff(core, RTD_ONOFF_INDEX) {
                onoff.value = Some(payload == "1");
            }
        } else if topic == self.setpoint_status_path {
            let Ok(value) = payload.trim().parse::<f64>() else {
                warn!(target: "mqtt", "wb.rtd_ra: cannot parse setpoint {payload:?}");
                return None;
            };
            if let Some(setpoint) = get_range(core, RTD_SETPOINT_INDEX) {
                setpoint.value = Some(nf(value));
            }
        } else if topic == self.mode_status_path {
            if let Some(mode) = map_reverse(HEAT_MODES, payload) {
                if let Some(cap) = get_mode(core, RTD_MODE_INDEX) {
                    cap.value = Some(mode);
                }
            }
        } else if topic == self.fanspeed_status_path {
            if let Some(mode) = map_reverse(FANSPEED_MODES, payload) {
                if let Some(cap) = get_mode(core, RTD_FANSPEED_INDEX) {
                    cap.value = Some(mode);
                }
            }
        } else if topic == self.louvre_status_path {
            if let Some(mode) = map_reverse(LOUVRE_MODES, payload) {
                if let Some(cap) = get_mode(core, RTD_LOUVRE_INDEX) {
                    cap.value = Some(mode);
                }
            }
        } else if topic == self.temperature_path {
            assign_float(
                core,
                RTD_TEMPERATURE_INDEX,
                payload,
                "wb.rtd_ra.temperature",
            );
        }
        None
    }
}

fn build_rtd_ra(
    device_id: &str,
    spec: toml::Value,
    ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let device_path = spec.req_str("device_path")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;
    let mqtt = require_mqtt(ctx, "WbRtdRa")?;
    let path = &device_path;

    let mut core = base_core(
        device_id,
        DeviceType::AirConditioner,
        &name,
        &description,
        &room,
        "WB",
        "torkve",
    );
    core.capabilities.push(onoff_cap());
    core.capabilities.push(Capability::Range(RangeCap {
        instance: "temperature",
        unit: Some("unit.temperature.celsius"),
        random_access: Some(true),
        min_value: Some(nf(18.)),
        max_value: Some(nf(32.)),
        precision: Some(nf(1.)),
        value: None,
        retrievable: true,
        reportable: true,
    }));
    core.capabilities.push(Capability::Mode(ModeCap {
        instance: "fan_speed",
        modes: vec!["auto", "one", "two", "three", "four", "five"],
        value: None,
        retrievable: true,
        reportable: true,
    }));
    core.capabilities.push(Capability::Mode(ModeCap {
        instance: "thermostat",
        modes: vec!["auto", "heat", "fan_only", "cool", "dry"],
        value: None,
        retrievable: true,
        reportable: true,
    }));
    core.capabilities.push(Capability::Mode(ModeCap {
        instance: "swing",
        modes: vec!["stationary", "vertical"],
        value: None,
        retrievable: true,
        reportable: true,
    }));
    core.properties.push(Property::Float(FloatProp::new(
        FloatKind::TemperatureCelsius,
        true,
        true,
    )));

    let logic = RtdRaLogic {
        mqtt,
        onoff_status_path: format!("{path}/OnOff"),
        onoff_control_path: format!("{path}/OnOff/on"),
        mode_status_path: format!("{path}/Mode"),
        mode_control_path: format!("{path}/Mode/on"),
        setpoint_status_path: format!("{path}/Setpoint"),
        setpoint_control_path: format!("{path}/Setpoint/on"),
        fanspeed_status_path: format!("{path}/Fanspeed"),
        fanspeed_control_path: format!("{path}/Fanspeed/on"),
        louvre_status_path: format!("{path}/Louvre"),
        louvre_control_path: format!("{path}/Louvre/on"),
        temperature_path: format!("{path}/Return Air Temperature"),
    };
    let topics = vec![
        logic.onoff_status_path.clone(),
        logic.setpoint_status_path.clone(),
        logic.mode_status_path.clone(),
        logic.louvre_status_path.clone(),
        logic.fanspeed_status_path.clone(),
        logic.temperature_path.clone(),
    ];
    Ok((shared(core, logic), topics))
}
