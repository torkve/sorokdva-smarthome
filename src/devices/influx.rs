//! The freezer watcher device polling a local influxdb over HTTP every 10s.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use log::{debug, warn};
use serde_json::{Map, Value};

use crate::protocol::device::{ActionException, DeviceCore, DeviceType};
use crate::protocol::property::{FloatKind, FloatProp, Property};

use super::{lock, BuildContext, Device, DeviceClass, DeviceLogic, SharedDevice, SpecReader};

/// The influx-polled device classes, registered into the class table.
pub static CLASSES: &[DeviceClass] = &[DeviceClass::polled(
    "FreezerWatcher",
    build_freezer_watcher,
    spawn_updater,
)];

struct FreezerLogic;

impl DeviceLogic for FreezerLogic {
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
}

fn build_freezer_watcher(
    device_id: &str,
    spec: toml::Value,
    _ctx: &BuildContext,
) -> Result<(SharedDevice, Vec<String>)> {
    let mut spec = SpecReader::new(spec)?;
    let name = spec.req_str("name")?;
    let description = spec.opt_str("description")?;
    let room = spec.opt_str("room")?;
    spec.finish()?;

    let mut core = DeviceCore::new(device_id, DeviceType::Other);
    core.name = Some(name);
    core.description = description;
    core.room = room;
    core.manufacturer = Some("torkve".to_string());
    core.model = Some("FRDG2".to_string());
    core.hw_version = Some("2.0".to_string());
    core.sw_version = Some("7.0".to_string());

    core.properties.push(Property::Float(FloatProp::new(
        FloatKind::TemperatureCelsius,
        true,
        false,
    )));
    core.properties.push(Property::Float(FloatProp::new(
        FloatKind::Humidity,
        true,
        false,
    )));
    // no toggle-like sensors yet
    core.properties.push(Property::Float(FloatProp::new(
        FloatKind::Power,
        true,
        false,
    )));

    let device = Arc::new(Mutex::new(Device {
        core,
        logic: Box::new(FreezerLogic),
    }));
    Ok((device, Vec::new()))
}

async fn fetch(client: &crate::httpc::HttpClient) -> Option<Map<String, Value>> {
    let query: String = form_urlencoded::Serializer::new(String::new())
        .append_pair("db", "freezer")
        .append_pair("q", "select * from freezer order by time desc limit 1")
        .finish();
    let data: Value = client
        .get_json(&format!("http://localhost:8086/query?{query}"))
        .await
        .ok()?;
    let series = data.get("results")?.get(0)?.get("series")?.get(0)?;
    let columns = series.get("columns")?.as_array()?;
    let values = series.get("values")?.get(0)?.as_array()?;
    let mut result = Map::new();
    for (column, value) in columns.iter().zip(values) {
        result.insert(column.as_str()?.to_string(), value.clone());
    }
    debug!(target: "freezer", "fetched {}", Value::Object(result.clone()));
    Some(result)
}

fn apply(device: &SharedDevice, data: &Map<String, Value>) {
    let mut guard = lock(device);
    let Device { core, logic: _ } = &mut *guard;

    // keep the raw JSON numbers so the serialized state preserves the
    // int/float distinction of the influx values
    let number = |key: &str| match data.get(key) {
        Some(Value::Number(n)) => Some(n.clone()),
        _ => None,
    };

    for (value, index, name) in [
        (number("temperature_bme"), 0, "temperature"),
        (number("humidity_bme"), 1, "humidity"),
        (number("cooler"), 2, "cooler"),
    ] {
        if let (Some(v), Some(Property::Float(prop))) = (value, core.properties.get_mut(index)) {
            if let Err(e) = prop.assign(v) {
                warn!(target: "freezer", "{name}: {e}");
            }
        }
    }
}

/// Start the poll loop for one device. The HTTP client is shared across
/// all polled devices: constructing one builds a full rustls root store.
/// The 30s connect / 300s total timeouts keep a black-holed influx
/// connection from hanging the loop forever.
fn spawn_updater(device: SharedDevice) {
    static CLIENT: std::sync::OnceLock<crate::httpc::HttpClient> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        crate::httpc::HttpClient::new(
            Duration::from_secs(30),
            Duration::from_secs(300),
            Vec::new(),
        )
    });
    tokio::spawn(updater_loop(device, client.clone()));
}

/// Poll influx every 10s and fold the latest sample into the device state.
async fn updater_loop(device: SharedDevice, client: crate::httpc::HttpClient) {
    loop {
        match fetch(&client).await {
            Some(data) => apply(&device, &data),
            None => warn!(target: "freezer", "fetch failed"),
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
