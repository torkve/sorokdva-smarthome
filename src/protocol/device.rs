use serde_json::{json, Map, Value};

use super::capability::Capability;
use super::consts::{ActionError, ActionStatus};
use super::property::Property;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Light,
    LightLamp,
    LightCeiling,
    LightStrip,
    Socket,
    Switch,
    SwitchRelay,
    Thermostat,
    AirConditioner,
    MediaDevice,
    TV,
    TVBox,
    Receiver,
    Cooking,
    CoffeeMaker,
    Kettle,
    Multicooker,
    Openable,
    Curtain,
    OpenableValve,
    Humidifier,
    Purifier,
    VacuumCleaner,
    WashingMachine,
    Dishwasher,
    Iron,
    Ventilation,
    VentilationFan,
    PetDrinkingFountain,
    PetFeeder,
    Camera,
    Sensor,
    SensorButton,
    SensorClimate,
    SensorGas,
    SensorIllumination,
    SensorMotion,
    SensorOpen,
    SensorSmoke,
    SensorVibration,
    SensorWaterLeak,
    SmartMeter,
    SmartMeterColdWater,
    SmartMeterElectricity,
    SmartMeterGas,
    SmartMeterHeat,
    SmartMeterHotWater,
    Other,
}

impl DeviceType {
    pub fn type_id(self) -> &'static str {
        match self {
            DeviceType::Light => "devices.types.light",
            DeviceType::LightLamp => "devices.types.light.lamp",
            DeviceType::LightCeiling => "devices.types.light.ceiling",
            DeviceType::LightStrip => "devices.types.light.strip",
            DeviceType::Socket => "devices.types.socket",
            DeviceType::Switch => "devices.types.switch",
            DeviceType::SwitchRelay => "devices.types.switch.relay",
            DeviceType::Thermostat => "devices.types.thermostat",
            DeviceType::AirConditioner => "devices.types.thermostat.ac",
            DeviceType::MediaDevice => "devices.types.media_device",
            DeviceType::TV => "devices.types.media_device.tv",
            DeviceType::TVBox => "devices.types.media_device.tv_box",
            DeviceType::Receiver => "devices.types.media_device.receiver",
            DeviceType::Cooking => "devices.types.cooking",
            DeviceType::CoffeeMaker => "devices.types.cooking.coffee_maker",
            DeviceType::Kettle => "devices.types.cooking.kettle",
            DeviceType::Multicooker => "devices.types.cooking.multicooker",
            DeviceType::Openable => "devices.types.openable",
            DeviceType::Curtain => "devices.types.openable.curtain",
            DeviceType::OpenableValve => "devices.types.openable.valve",
            DeviceType::Humidifier => "devices.types.humidifier",
            DeviceType::Purifier => "devices.types.purifier",
            DeviceType::VacuumCleaner => "devices.types.vacuum_cleaner",
            DeviceType::WashingMachine => "devices.types.washing_machine",
            DeviceType::Dishwasher => "devices.types.dishwasher",
            DeviceType::Iron => "devices.types.iron",
            DeviceType::Ventilation => "devices.types.ventilation",
            DeviceType::VentilationFan => "devices.types.ventilation.fan",
            DeviceType::PetDrinkingFountain => "devices.types.pet_drinking_fountain",
            DeviceType::PetFeeder => "devices.types.pet_feeder",
            DeviceType::Camera => "devices.types.camera",
            DeviceType::Sensor => "devices.types.sensor",
            DeviceType::SensorButton => "devices.types.sensor.button",
            DeviceType::SensorClimate => "devices.types.sensor.climate",
            DeviceType::SensorGas => "devices.types.sensor.gas",
            DeviceType::SensorIllumination => "devices.types.sensor.illumination",
            DeviceType::SensorMotion => "devices.types.sensor.motion",
            DeviceType::SensorOpen => "devices.types.sensor.open",
            DeviceType::SensorSmoke => "devices.types.sensor.smoke",
            DeviceType::SensorVibration => "devices.types.sensor.vibration",
            DeviceType::SensorWaterLeak => "devices.types.sensor.water_leak",
            DeviceType::SmartMeter => "devices.types.smart_meter",
            DeviceType::SmartMeterColdWater => "devices.types.smart_meter.cold_water",
            DeviceType::SmartMeterElectricity => "devices.types.smart_meter.electricity",
            DeviceType::SmartMeterGas => "devices.types.smart_meter.gas",
            DeviceType::SmartMeterHeat => "devices.types.smart_meter.heat",
            DeviceType::SmartMeterHotWater => "devices.types.smart_meter.hot_water",
            DeviceType::Other => "devices.types.other",
        }
    }
}

/// The Rust analogue of protocol.exceptions.ActionException.
#[derive(Debug)]
pub struct ActionException {
    pub capability_id: &'static str,
    pub instance: String,
    pub code: ActionError,
    pub message: Option<String>,
}

impl ActionException {
    pub fn new(capability_id: &'static str, instance: &str, code: ActionError) -> Self {
        ActionException {
            capability_id,
            instance: instance.to_string(),
            code,
            message: None,
        }
    }

    pub fn message(&self) -> String {
        self.message
            .clone()
            .unwrap_or_else(|| self.code.as_str().to_string())
    }
}

/// One capability change requested by the action API call.
#[derive(Debug)]
pub struct ActionChange {
    pub type_id: String,
    pub instance: String,
    pub value: Value,
    /// The extra keys of the requested state, e.g. {"relative": true}.
    pub opts: Map<String, Value>,
}

/// Pure device state: metadata plus capability and property values.
/// Behaviour (MQTT parsing, action side effects) lives in devices::DeviceLogic.
#[derive(Debug)]
pub struct DeviceCore {
    pub device_id: String,
    pub device_type: DeviceType,
    pub name: Option<String>,
    pub description: Option<String>,
    pub room: Option<String>,
    pub custom_data: Option<Value>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub hw_version: Option<String>,
    pub sw_version: Option<String>,
    pub capabilities: Vec<Capability>,
    pub properties: Vec<Property>,
}

impl DeviceCore {
    pub fn new(device_id: &str, device_type: DeviceType) -> Self {
        DeviceCore {
            device_id: device_id.to_string(),
            device_type,
            name: None,
            description: None,
            room: None,
            custom_data: None,
            manufacturer: None,
            model: None,
            hw_version: None,
            sw_version: None,
            capabilities: Vec::new(),
            properties: Vec::new(),
        }
    }

    pub fn find_capability(&self, type_id: &str, instance: &str) -> Option<usize> {
        self.capabilities
            .iter()
            .position(|cap| cap.type_id() == type_id && cap.instances().contains(&instance))
    }

    /// Device specification in the format required by the device list API call.
    pub fn specification(&self) -> Value {
        let mut result = Map::new();
        result.insert("id".into(), json!(self.device_id));
        result.insert("type".into(), json!(self.device_type.type_id()));
        result.insert(
            "capabilities".into(),
            Value::Array(
                self.capabilities
                    .iter()
                    .map(|c| c.specification())
                    .collect(),
            ),
        );
        result.insert(
            "properties".into(),
            Value::Array(self.properties.iter().map(|p| p.specification()).collect()),
        );

        for (field, value) in [
            ("name", &self.name),
            ("description", &self.description),
            ("room", &self.room),
        ] {
            if let Some(value) = value {
                result.insert(field.into(), json!(value));
            }
        }
        if let Some(custom_data) = &self.custom_data {
            result.insert("custom_data".into(), custom_data.clone());
        }

        let mut device_info = Map::new();
        for (field, value) in [
            ("manufacturer", &self.manufacturer),
            ("model", &self.model),
            ("hw_version", &self.hw_version),
            ("sw_version", &self.sw_version),
        ] {
            if let Some(value) = value {
                device_info.insert(field.into(), json!(value));
            }
        }
        if !device_info.is_empty() {
            result.insert("device_info".into(), Value::Object(device_info));
        }

        Value::Object(result)
    }

    /// State of all retrievable capabilities and properties.
    pub fn state(&self) -> Value {
        let capabilities: Vec<Value> = self
            .capabilities
            .iter()
            .filter(|c| c.retrievable())
            .filter_map(|c| c.state())
            .collect();
        let properties: Vec<Value> = self
            .properties
            .iter()
            .filter(|p| p.retrievable())
            .filter_map(|p| p.state())
            .collect();
        json!({
            "id": self.device_id,
            "capabilities": capabilities,
            "properties": properties,
        })
    }

    /// State of all retrievable+reportable capabilities and properties, each
    /// paired with a flag showing whether it differs from previous_state
    /// (the value previously returned in the corresponding list).
    pub fn report(&self, previous_state: &Value) -> Value {
        let previous_caps = previous_state.get("capabilities").and_then(Value::as_array);
        let previous_props = previous_state.get("properties").and_then(Value::as_array);
        let contains =
            |prev: Option<&Vec<Value>>, state: &Value| prev.is_some_and(|p| p.contains(state));

        let capabilities: Vec<Value> = self
            .capabilities
            .iter()
            .filter(|c| c.retrievable() && c.reportable())
            .filter_map(|c| c.state())
            .map(|state| {
                let changed = !contains(previous_caps, &state);
                json!([state, changed])
            })
            .collect();
        let properties: Vec<Value> = self
            .properties
            .iter()
            .filter(|p| p.retrievable() && p.reportable())
            .filter_map(|p| p.state())
            .map(|state| {
                let changed = !contains(previous_props, &state);
                json!([state, changed])
            })
            .collect();

        json!({
            "id": self.device_id,
            "capabilities": capabilities,
            "properties": properties,
        })
    }

    /// Parse the capability changes of an action API call.
    /// Mirrors Device.split_value: "value" and "instance" are removed from the
    /// state dict, everything remaining is passed through as opts.
    pub fn parse_changes(capabilities: &[Value]) -> Vec<ActionChange> {
        capabilities
            .iter()
            .filter_map(|cap| {
                let type_id = cap.get("type")?.as_str()?.to_string();
                let mut state = cap.get("state")?.as_object()?.clone();
                let value = state.remove("value")?;
                let instance = state.remove("instance")?.as_str()?.to_string();
                Some(ActionChange {
                    type_id,
                    instance,
                    value,
                    opts: state,
                })
            })
            .collect()
    }

    pub fn action_result_done(type_id: &str, instance: &str, value: Option<Value>) -> Value {
        let mut state = Map::new();
        state.insert("instance".into(), json!(instance));
        if let Some(value) = value {
            state.insert("value".into(), value);
        }
        state.insert(
            "action_result".into(),
            json!({"status": ActionStatus::Done.as_str()}),
        );
        json!({
            "type": type_id,
            "state": state,
        })
    }

    pub fn action_result_error(
        type_id: &str,
        instance: &str,
        code: ActionError,
        message: &str,
    ) -> Value {
        json!({
            "type": type_id,
            "state": {
                "instance": instance,
                "action_result": {
                    "status": ActionStatus::Error.as_str(),
                    "error_code": code.as_str(),
                    "error_message": message,
                }
            }
        })
    }
}
