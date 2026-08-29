//! Coverage for the newer Yandex Smart Home API surface: recent device
//! types, float/event property instances, color scenes and the
//! video_stream capability.

use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};

use sorokdva_smarthome::devices::{self, Device, DeviceLogic};
use sorokdva_smarthome::protocol::{
    ActionException, Capability, ColorSetting, ColorValue, DeviceCore, DeviceType, EventKind,
    EventProp, FloatKind, FloatProp, Property, VideoStreamCap, COLOR_SCENES,
};

#[test]
fn new_device_type_ids() {
    for (device_type, id) in [
        (DeviceType::LightLamp, "devices.types.light.lamp"),
        (DeviceType::LightCeiling, "devices.types.light.ceiling"),
        (DeviceType::LightStrip, "devices.types.light.strip"),
        (DeviceType::SwitchRelay, "devices.types.switch.relay"),
        (DeviceType::OpenableValve, "devices.types.openable.valve"),
        (DeviceType::Ventilation, "devices.types.ventilation"),
        (DeviceType::VentilationFan, "devices.types.ventilation.fan"),
        (
            DeviceType::PetDrinkingFountain,
            "devices.types.pet_drinking_fountain",
        ),
        (DeviceType::PetFeeder, "devices.types.pet_feeder"),
        (DeviceType::Camera, "devices.types.camera"),
        (DeviceType::SensorButton, "devices.types.sensor.button"),
        (DeviceType::SensorClimate, "devices.types.sensor.climate"),
        (DeviceType::SensorGas, "devices.types.sensor.gas"),
        (
            DeviceType::SensorIllumination,
            "devices.types.sensor.illumination",
        ),
        (DeviceType::SensorMotion, "devices.types.sensor.motion"),
        (DeviceType::SensorOpen, "devices.types.sensor.open"),
        (DeviceType::SensorSmoke, "devices.types.sensor.smoke"),
        (
            DeviceType::SensorVibration,
            "devices.types.sensor.vibration",
        ),
        (
            DeviceType::SensorWaterLeak,
            "devices.types.sensor.water_leak",
        ),
        (DeviceType::SmartMeter, "devices.types.smart_meter"),
        (
            DeviceType::SmartMeterColdWater,
            "devices.types.smart_meter.cold_water",
        ),
        (
            DeviceType::SmartMeterElectricity,
            "devices.types.smart_meter.electricity",
        ),
        (DeviceType::SmartMeterGas, "devices.types.smart_meter.gas"),
        (DeviceType::SmartMeterHeat, "devices.types.smart_meter.heat"),
        (
            DeviceType::SmartMeterHotWater,
            "devices.types.smart_meter.hot_water",
        ),
    ] {
        assert_eq!(device_type.type_id(), id);
        let core = DeviceCore::new("d", device_type);
        assert_eq!(core.specification()["type"], json!(id));
    }
}

#[test]
fn new_float_property_instances() {
    for (kind, instance, unit) in [
        (
            FloatKind::BatteryLevel,
            "battery_level",
            Some("unit.percent"),
        ),
        (
            FloatKind::ElectricityMeter,
            "electricity_meter",
            Some("unit.kilowatt_hour"),
        ),
        (FloatKind::FoodLevel, "food_level", Some("unit.percent")),
        (FloatKind::GasMeter, "gas_meter", Some("unit.cubic_meter")),
        (FloatKind::HeatMeter, "heat_meter", Some("unit.gigacalorie")),
        (
            FloatKind::Illumination,
            "illumination",
            Some("unit.illumination.lux"),
        ),
        (FloatKind::Meter, "meter", None),
        (
            FloatKind::Pm1Density,
            "pm1_density",
            Some("unit.density.mcg_m3"),
        ),
        (
            FloatKind::Pm25Density,
            "pm2.5_density",
            Some("unit.density.mcg_m3"),
        ),
        (
            FloatKind::Pm10Density,
            "pm10_density",
            Some("unit.density.mcg_m3"),
        ),
        (
            FloatKind::PressureAtm,
            "pressure",
            Some("unit.pressure.atm"),
        ),
        (
            FloatKind::PressurePascal,
            "pressure",
            Some("unit.pressure.pascal"),
        ),
        (
            FloatKind::PressureBar,
            "pressure",
            Some("unit.pressure.bar"),
        ),
        (
            FloatKind::PressureMmHg,
            "pressure",
            Some("unit.pressure.mmhg"),
        ),
        (FloatKind::Tvoc, "tvoc", Some("unit.density.mcg_m3")),
        (
            FloatKind::WaterMeter,
            "water_meter",
            Some("unit.cubic_meter"),
        ),
    ] {
        let prop = Property::Float(FloatProp::new(kind, true, false));
        let spec = prop.specification();
        assert_eq!(spec["retrievable"], json!(true), "{kind:?}");
        assert_eq!(spec["reportable"], json!(false), "{kind:?}");
        assert_eq!(spec["parameters"]["instance"], json!(instance), "{kind:?}");
        match unit {
            Some(unit) => assert_eq!(spec["parameters"]["unit"], json!(unit), "{kind:?}"),
            // the generic meter instance has no unit parameter at all
            None => assert!(
                spec["parameters"]
                    .as_object()
                    .unwrap()
                    .get("unit")
                    .is_none(),
                "{kind:?}"
            ),
        }
    }

    // validation: percentages bounded, meters non-negative
    let mut battery = FloatProp::new(FloatKind::BatteryLevel, true, false);
    assert!(battery.assign_f64(146.).is_err());
    assert!(battery.assign_f64(42.).is_ok());
    let mut meter = FloatProp::new(FloatKind::WaterMeter, true, false);
    assert!(meter.assign_f64(-1.).is_err());
    assert!(meter.assign_f64(12345.678).is_ok());
}

#[test]
fn new_event_property_values() {
    let food = Property::Event(EventProp::new(EventKind::FoodLevel, true, true));
    assert_eq!(
        food.specification()["parameters"],
        json!({
            "instance": "food_level",
            "events": [{"value": "empty"}, {"value": "low"}, {"value": "normal"}],
        })
    );
    // water_level gained the "empty" event
    let water = Property::Event(EventProp::new(EventKind::WaterLevel, true, false));
    assert_eq!(
        water.specification()["parameters"]["events"],
        json!([{"value": "empty"}, {"value": "low"}, {"value": "normal"}])
    );
}

#[test]
fn color_scenes() {
    assert!(COLOR_SCENES.contains(&"party") && COLOR_SCENES.len() == 16);

    let color = Capability::Color(ColorSetting {
        color_model: None,
        temperature: Some((Some(2700), Some(6500))),
        scenes: vec!["party", "candle"],
        value: Some(ColorValue::Scene("party".to_string())),
        retrievable: true,
        reportable: false,
    });
    assert_eq!(color.instances(), vec!["temperature_k", "scene"]);
    assert_eq!(
        color.parameters().unwrap(),
        json!({
            "temperature_k": {"min": 2700, "max": 6500},
            "color_scene": {"scenes": [{"id": "party"}, {"id": "candle"}]},
        })
    );
    assert_eq!(
        color.state().unwrap(),
        json!({
            "type": "devices.capabilities.color_setting",
            "state": {"instance": "scene", "value": "party"},
        })
    );
}

/// A camera logic answering get_stream with a stream URL payload.
struct CameraLogic;

impl DeviceLogic for CameraLogic {
    fn on_action(
        &mut self,
        _core: &mut DeviceCore,
        _type_id: &str,
        instance: &str,
        value: &Value,
        _opts: &Map<String, Value>,
    ) -> Result<Option<Value>, ActionException> {
        assert_eq!(instance, "get_stream");
        assert_eq!(value["protocols"], json!(["hls"]));
        Ok(Some(json!({
            "stream_url": "https://example.com/stream.m3u8",
            "protocol": "hls",
        })))
    }
}

#[tokio::test]
async fn video_stream_capability() {
    let mut core = DeviceCore::new("cam1", DeviceType::Camera);
    core.capabilities
        .push(Capability::VideoStream(VideoStreamCap {
            protocols: vec!["hls"],
        }));

    // specification: never retrievable or reportable, protocols parameter
    assert_eq!(
        core.specification()["capabilities"],
        json!([{
            "type": "devices.capabilities.video_stream",
            "retrievable": false,
            "reportable": false,
            "parameters": {"protocols": ["hls"]},
        }])
    );

    // no state at all
    assert_eq!(core.state()["capabilities"], json!([]));

    // action returns the stream url inside the response state
    let device = Arc::new(Mutex::new(Device {
        core,
        logic: Box::new(CameraLogic),
    }));
    let result = devices::action(
        &device,
        &[json!({
            "type": "devices.capabilities.video_stream",
            "state": {
                "instance": "get_stream",
                "value": {"protocols": ["hls"]},
            },
        })],
    );
    assert_eq!(
        result,
        json!({
            "id": "cam1",
            "capabilities": [{
                "type": "devices.capabilities.video_stream",
                "state": {
                    "instance": "get_stream",
                    "value": {
                        "stream_url": "https://example.com/stream.m3u8",
                        "protocol": "hls",
                    },
                    "action_result": {"status": "DONE"},
                },
            }],
        })
    );
}

#[test]
fn capability_validation() {
    let good = Capability::Color(ColorSetting {
        color_model: None,
        temperature: None,
        scenes: vec!["party", "candle"],
        value: None,
        retrievable: true,
        reportable: false,
    });
    assert!(good.validate().is_ok());

    let bad = Capability::Color(ColorSetting {
        color_model: None,
        temperature: None,
        scenes: vec!["party", "disco"],
        value: None,
        retrievable: true,
        reportable: false,
    });
    assert!(bad.validate().unwrap_err().contains("disco"));

    let empty = Capability::VideoStream(VideoStreamCap {
        protocols: Vec::new(),
    });
    assert!(empty.validate().is_err());
    let hls = Capability::VideoStream(VideoStreamCap {
        protocols: vec!["hls"],
    });
    assert!(hls.validate().is_ok());
}
