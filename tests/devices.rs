//! Protocol and device behaviour tests: specifications, state
//! serialization, action dispatch and MQTT round-trips, with golden
//! assertions pinning the exact JSON shapes of the API.

use serde_json::json;
#[cfg(feature = "wirenboard")]
use serde_json::Value;

use sorokdva_smarthome::devices::{self, lock, BuildContext, BuiltDevices};
use sorokdva_smarthome::mqtt::{self, MqttHandle};
use sorokdva_smarthome::tasks::TaskSpawner;

type Built = (
    BuiltDevices,
    MqttHandle,
    mqtt::OutgoingReceiver,
    TaskSpawner,
);

fn build(cfg: &str) -> Built {
    let table: toml::Table = toml::from_str(cfg).unwrap();
    let (handle, rx) = mqtt::channel();
    let tasks = TaskSpawner::new();
    let built = devices::build_all(
        &table,
        &BuildContext {
            mqtt: Some(handle.clone()),
            tasks: tasks.clone(),
        },
    )
    .unwrap();
    (built, handle, rx, tasks)
}

/// Deliver a status message through the real dispatcher (settle included).
#[cfg(feature = "wirenboard")]
fn feed(built: &BuiltDevices, tasks: &TaskSpawner, topic: &str, payload: &str) {
    assert!(
        built.subscriptions.contains_key(topic),
        "topic {topic} not subscribed"
    );
    mqtt::dispatch(built, tasks, None, topic, payload);
}

#[cfg(feature = "wirenboard")]
fn drain(rx: &mut mqtt::OutgoingReceiver) -> Vec<(String, String)> {
    let mut result = Vec::new();
    while let Some(message) = rx.try_recv() {
        result.push(message);
    }
    result
}

#[cfg(feature = "wirenboard")]
const LIGHT_CFG: &str = r#"
[light1]
_class = "WbLight"
_mqtt_used = true
name = "Свет"
description = "Лампа"
room = "Кухня"
status_path = "/devices/x/controls/K1"
control_path = "/devices/x/controls/K1/on"
"#;

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn light_spec_state_action() {
    let (built, _handle, mut rx, tasks) = build(LIGHT_CFG);
    let (_, device) = &built.devices[0];

    let spec = lock(device).core.specification();
    assert_eq!(
        spec,
        json!({
            "id": "light1",
            "type": "devices.types.light",
            "capabilities": [
                {
                    "type": "devices.capabilities.on_off",
                    "retrievable": true,
                    "reportable": true,
                    "parameters": {"split": false},
                }
            ],
            "properties": [],
            "name": "Свет",
            "description": "Лампа",
            "room": "Кухня",
            "device_info": {"manufacturer": "torkve", "model": "WB"},
        })
    );

    // no value yet -> empty state
    assert_eq!(
        lock(device).core.state(),
        json!({"id": "light1", "capabilities": [], "properties": []})
    );

    feed(&built, &tasks, "/devices/x/controls/K1", "1");
    assert_eq!(
        lock(device).core.state(),
        json!({
            "id": "light1",
            "capabilities": [
                {
                    "type": "devices.capabilities.on_off",
                    "state": {"instance": "on", "value": true},
                }
            ],
            "properties": [],
        })
    );

    // action: switch off
    let result = devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": false},
        })],
    );
    assert_eq!(
        result,
        json!({
            "id": "light1",
            "capabilities": [
                {
                    "type": "devices.capabilities.on_off",
                    "state": {
                        "instance": "on",
                        "action_result": {"status": "DONE"},
                    },
                }
            ],
        })
    );
    assert_eq!(
        drain(&mut rx),
        vec![("/devices/x/controls/K1/on".to_string(), "0".to_string())]
    );

    // unknown capability -> INVALID_ACTION with this exact message
    let result = devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.mode",
            "state": {"instance": "fan_speed", "value": "auto"},
        })],
    );
    assert_eq!(
        result["capabilities"][0]["state"]["action_result"],
        json!({
            "status": "ERROR",
            "error_code": "INVALID_ACTION",
            "error_message": "Unknown capability for this device",
        })
    );
}

#[cfg(feature = "wirenboard")]
const SENSOR_CFG: &str = r#"
[sensor1]
_class = "WbSensor"
_mqtt_used = true
name = "Датчик"
temperature_path = "/devices/msw/controls/Temperature"
humidity_path = "/devices/msw/controls/Humidity"
sound_level_path = "/devices/msw/controls/Sound Level"
illuminance_path = "/devices/msw/controls/Illuminance"
"#;

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn sensor_properties() {
    let (built, _handle, _rx, tasks) = build(SENSOR_CFG);
    let (_, device) = &built.devices[0];

    let spec = lock(device).core.specification();
    assert_eq!(spec["capabilities"], json!([]),);
    // float property specifications include "reportable" per the
    // current API docs
    assert_eq!(
        spec["properties"],
        json!([
            {
                "type": "devices.properties.float",
                "retrievable": true,
                "reportable": true,
                "parameters": {"instance": "temperature", "unit": "unit.temperature.celsius"},
            },
            {
                "type": "devices.properties.float",
                "retrievable": true,
                "reportable": true,
                "parameters": {"instance": "humidity", "unit": "unit.percent"},
            },
        ])
    );
    assert_eq!(spec["device_info"]["manufacturer"], "Wirenboard");
    assert_eq!(spec["device_info"]["model"], "WB-MSW v.3");

    feed(&built, &tasks, "/devices/msw/controls/Temperature", "23.5");
    feed(&built, &tasks, "/devices/msw/controls/Humidity", "48");
    // out-of-range humidity is rejected (validation), keeping the old value
    feed(&built, &tasks, "/devices/msw/controls/Humidity", "146");

    assert_eq!(
        lock(device).core.state(),
        json!({
            "id": "sensor1",
            "capabilities": [],
            "properties": [
                {
                    "type": "devices.properties.float",
                    "state": {"instance": "temperature", "value": 23.5},
                },
                {
                    "type": "devices.properties.float",
                    "state": {"instance": "humidity", "value": 48.0},
                },
            ],
        })
    );
}

#[cfg(feature = "wirenboard")]
const MIXWHITE_CFG: &str = r#"
[mix1]
_class = "WbMixwhiteLight"
_mqtt_used = true
name = "n"
warm_status_path = "w"
warm_control_path = "w/on"
cold_status_path = "c"
cold_control_path = "c/on"
warm_temperature = 2700
cold_temperature = 6500
range_low = 20
range_high = 1000
"#;

/// Golden values for the warm/cold channel math: channel mixes for
/// given brightness/temperature actions and the states derived back
/// from channel updates. The constants are fixed reference outputs --
/// never recompute them with this implementation.
#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn mixwhite_math_golden() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");

    {
        let guard = lock(device);
        let state = guard.core.state();
        let caps = state["capabilities"].as_array().unwrap();
        let find =
            |type_id: &str| -> &Value { caps.iter().find(|c| c["type"] == type_id).unwrap() };
        assert_eq!(
            find("devices.capabilities.range")["state"]["value"]
                .as_f64()
                .unwrap(),
            69.38775510204081
        );
        assert_eq!(
            find("devices.capabilities.on_off")["state"]["value"],
            json!(true)
        );
        assert_eq!(
            find("devices.capabilities.color_setting")["state"],
            json!({"instance": "temperature_k", "value": 3808})
        );
    }
    drain(&mut rx);

    // change temperature to 4000K
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.color_setting",
            "state": {"instance": "temperature_k", "value": 4000},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "373".to_string()),
            ("w/on".to_string(), "700".to_string()),
        ]
    );

    // change brightness to 50%: composes with the still-unconfirmed
    // 4000K intended by the previous action, not with the stale settled
    // temperature, so the requested color survives
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 50.0},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "274".to_string()),
            ("w/on".to_string(), "510".to_string()),
        ]
    );

    // off then on
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": false},
        })],
    );
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "0".to_string()),
            ("w/on".to_string(), "0".to_string()),
            ("c/on".to_string(), "299".to_string()),
            ("w/on".to_string(), "700".to_string()),
        ]
    );

    // the full specification shape (capability order aside)
    let spec = lock(device).core.specification();
    let caps = spec["capabilities"].as_array().unwrap();
    let color = caps
        .iter()
        .find(|c| c["type"] == "devices.capabilities.color_setting")
        .unwrap();
    assert_eq!(
        color["parameters"],
        json!({"temperature_k": {"min": 2700, "max": 6500}})
    );
    let range = caps
        .iter()
        .find(|c| c["type"] == "devices.capabilities.range")
        .unwrap();
    assert_eq!(
        range["parameters"],
        json!({
            "instance": "brightness",
            "random_access": true,
            "unit": "unit.percent",
            "range": {"min": 0.0, "max": 100.0, "precision": 0.1},
        })
    );

    // invalid temperature value type
    let result = devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.color_setting",
            "state": {"instance": "temperature_k", "value": {"h": 1, "s": 2, "v": 3}},
        })],
    );
    assert_eq!(
        result["capabilities"][0]["state"]["action_result"]["error_code"],
        "INVALID_VALUE"
    );
}

#[cfg(feature = "wirenboard")]
const DIMMABLE_CFG: &str = r#"
[dim1]
_class = "WbDimmableLight"
_mqtt_used = true
name = "n"
status_path = "s"
control_path = "s/on"
range_off = 0
range_low = 200
range_high = 1000
"#;

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn dimmable_light_levels() {
    let (built, _handle, mut rx, tasks) = build(DIMMABLE_CFG);
    let (_, device) = &built.devices[0];

    // brightness 60% -> real value int(0.6*800+200) = 680
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 60.0},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![("s/on".to_string(), "680".to_string())]
    );

    // brightness 0% -> real <= range_low -> range_off
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 0.0},
        })],
    );
    assert_eq!(drain(&mut rx), vec![("s/on".to_string(), "0".to_string())]);

    // relative change without a known level -> DEVICE_BUSY
    let result = devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 10.0, "relative": true},
        })],
    );
    assert_eq!(
        result["capabilities"][0]["state"]["action_result"]["error_code"],
        "DEVICE_BUSY"
    );

    // status at range_low -> percent is the *int* 0, not 0.0
    feed(&built, &tasks, "s", "200");
    {
        let guard = lock(device);
        let state = guard.core.state();
        let level = state["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["state"]["instance"] == "brightness")
            .unwrap();
        assert_eq!(level["state"]["value"], json!(0));
        assert!(level["state"]["value"].is_i64());
    }

    // status 600 -> percent (600-200)/800*100 = 50
    feed(&built, &tasks, "s", "600");
    // relative +10 -> 60% -> 680
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 10.0, "relative": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![("s/on".to_string(), "680".to_string())]
    );

    // off remembers last percent; on restores it
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": false},
        })],
    );
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("s/on".to_string(), "0".to_string()),
            ("s/on".to_string(), "600".to_string()),
        ]
    );
}

#[cfg(feature = "wirenboard")]
const CURTAIN_CFG: &str = r#"
[curtain1]
_class = "WbCurtain"
_mqtt_used = true
name = "Штора"
direction_status_path = "dir"
motor_status_path = "mot"
direction_control_path = "dir/on"
motor_control_path = "mot/on"
action_time_seconds = 0
"#;

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn curtain_actions() {
    let (built, _handle, mut rx, tasks) = build(CURTAIN_CFG);
    let (_, device) = &built.devices[0];

    // spec: 4 capabilities, updown is split and not retrievable
    let spec = lock(device).core.specification();
    let caps = spec["capabilities"].as_array().unwrap();
    assert_eq!(caps.len(), 4);
    let onoff = caps
        .iter()
        .find(|c| c["type"] == "devices.capabilities.on_off")
        .unwrap();
    assert_eq!(onoff["retrievable"], json!(false));
    assert_eq!(onoff["parameters"], json!({"split": true}));
    let range = caps
        .iter()
        .find(|c| c["type"] == "devices.capabilities.range")
        .unwrap();
    assert_eq!(range["parameters"]["random_access"], json!(false));

    // open (value=true): motor stop, direction, motor go, [sleep], motor stop
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        drain(&mut rx),
        vec![
            ("mot/on".to_string(), "0".to_string()),
            ("dir/on".to_string(), "1".to_string()),
            ("mot/on".to_string(), "1".to_string()),
            ("mot/on".to_string(), "0".to_string()),
        ]
    );

    // direction mode change
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.mode",
            "state": {"instance": "swing", "value": "low"},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![("dir/on".to_string(), "0".to_string())]
    );

    // invalid mode value
    let result = devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.mode",
            "state": {"instance": "swing", "value": "auto"},
        })],
    );
    assert_eq!(
        result["capabilities"][0]["state"]["action_result"]["error_code"],
        "INVALID_VALUE"
    );

    // motor toggle + status updates
    feed(&built, &tasks, "dir", "1");
    feed(&built, &tasks, "mot", "1");
    let state = lock(device).core.state();
    let caps = state["capabilities"].as_array().unwrap();
    assert_eq!(caps.len(), 2); // only direction + motor are retrievable
    assert!(caps
        .iter()
        .any(|c| c["state"] == json!({"instance": "swing", "value": "high"})));
    assert!(caps
        .iter()
        .any(|c| c["state"] == json!({"instance": "oscillation", "value": true})));
}

#[cfg(feature = "wirenboard")]
const RTD_CFG: &str = r#"
[ac1]
_class = "WbRtdRa"
_mqtt_used = true
name = "Кондиционер"
device_path = "/devices/RTD-NET_10/controls"
"#;

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn rtd_ra_modes() {
    let (built, _handle, mut rx, tasks) = build(RTD_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "/devices/RTD-NET_10/controls/Mode", "3");
    feed(&built, &tasks, "/devices/RTD-NET_10/controls/Fanspeed", "2");
    feed(&built, &tasks, "/devices/RTD-NET_10/controls/Louvre", "1");
    feed(
        &built,
        &tasks,
        "/devices/RTD-NET_10/controls/Setpoint",
        "25",
    );
    feed(&built, &tasks, "/devices/RTD-NET_10/controls/OnOff", "1");
    feed(
        &built,
        &tasks,
        "/devices/RTD-NET_10/controls/Return Air Temperature",
        "26.5",
    );

    let state = lock(device).core.state();
    let caps = state["capabilities"].as_array().unwrap();
    let find = |instance: &str| -> &Value {
        caps.iter()
            .find(|c| c["state"]["instance"] == instance)
            .unwrap()
    };
    assert_eq!(find("thermostat")["state"]["value"], "cool");
    assert_eq!(find("fan_speed")["state"]["value"], "two");
    assert_eq!(find("swing")["state"]["value"], "vertical");
    assert_eq!(find("temperature")["state"]["value"], json!(25.0));
    assert_eq!(find("on")["state"]["value"], json!(true));
    assert_eq!(
        state["properties"][0]["state"],
        json!({"instance": "temperature", "value": 26.5})
    );

    // actions: mode mapping + setpoint rounding (str(round(x, 1)))
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.mode",
            "state": {"instance": "thermostat", "value": "heat"},
        })],
    );
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "temperature", "value": 24},
        })],
    );
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "temperature", "value": 1.0, "relative": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            (
                "/devices/RTD-NET_10/controls/Mode/on".to_string(),
                "1".to_string()
            ),
            (
                "/devices/RTD-NET_10/controls/Setpoint/on".to_string(),
                "24.0".to_string()
            ),
            (
                "/devices/RTD-NET_10/controls/Setpoint/on".to_string(),
                "26.0".to_string()
            ),
        ]
    );
}

#[cfg(feature = "wirenboard")]
const WATER_CFG: &str = r#"
[valve1]
_class = "WbWaterValve"
_mqtt_used = true
name = "Вода"
status_path = "v"
control_path = "v/on"
alarm_control_path = "alarm/on"

[leak1]
_class = "WbLeakSensor"
_mqtt_used = true
name = "Протечка"
status_path = "leak"

[pir1]
_class = "WbPirSensor"
_mqtt_used = true
name = "Движение"
status_path = "pir"
"#;

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn water_and_sensors() {
    let (built, _handle, mut rx, tasks) = build(WATER_CFG);
    let valve = built.get("valve1").expect("valve1");
    let leak = built.get("leak1").expect("leak1");
    let pir = built.get("pir1").expect("pir1");

    // valve state is inverted
    feed(&built, &tasks, "v", "0");
    assert_eq!(
        lock(valve).core.state()["capabilities"][0]["state"]["value"],
        json!(true)
    );

    // switching on: control gets inverted value + alarm reset
    devices::action(
        valve,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("v/on".to_string(), "0".to_string()),
            ("alarm/on".to_string(), "0".to_string()),
        ]
    );
    devices::action(
        valve,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": false},
        })],
    );
    assert_eq!(drain(&mut rx), vec![("v/on".to_string(), "1".to_string())]);

    // leak/pir events
    feed(&built, &tasks, "leak", "1");
    assert_eq!(
        lock(leak).core.state()["properties"][0]["state"],
        json!({"instance": "water_leak", "value": "leak"})
    );
    feed(&built, &tasks, "pir", "0");
    assert_eq!(
        lock(pir).core.state()["properties"][0]["state"],
        json!({"instance": "motion", "value": "detected"})
    );

    // event property specification includes reportable + events list
    assert_eq!(
        lock(leak).core.specification()["properties"][0],
        json!({
            "type": "devices.properties.event",
            "retrievable": true,
            "reportable": true,
            "parameters": {
                "instance": "water_leak",
                "events": [{"value": "dry"}, {"value": "leak"}],
            },
        })
    );
}

#[cfg(feature = "influx")]
const FREEZER_CFG: &str = r#"
[freezer]
_class = "FreezerWatcher"
name = "Холодильник"
description = "Устройство"
"#;

#[cfg(feature = "influx")]
#[tokio::test]
async fn freezer_watcher_spec() {
    let (built, _handle, _rx, _tasks) = build(FREEZER_CFG);
    let (_, device) = &built.devices[0];
    assert!(built.pollers.iter().any(|(index, _)| *index == 0));

    let spec = lock(device).core.specification();
    assert_eq!(spec["type"], "devices.types.other");
    assert_eq!(spec["capabilities"], json!([]));
    assert_eq!(
        spec["properties"],
        json!([
            {
                "type": "devices.properties.float",
                "retrievable": true,
                "reportable": false,
                "parameters": {"instance": "temperature", "unit": "unit.temperature.celsius"},
            },
            {
                "type": "devices.properties.float",
                "retrievable": true,
                "reportable": false,
                "parameters": {"instance": "humidity", "unit": "unit.percent"},
            },
            {
                "type": "devices.properties.float",
                "retrievable": true,
                "reportable": false,
                "parameters": {"instance": "power", "unit": "unit.watt"},
            },
        ])
    );
    assert_eq!(
        spec["device_info"],
        json!({
            "manufacturer": "torkve",
            "model": "FRDG2",
            "hw_version": "2.0",
            "sw_version": "7.0",
        })
    );

    // actions on a device without capabilities -> INVALID_ACTION
    let result = devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    assert_eq!(
        result["capabilities"][0]["state"]["action_result"]["error_code"],
        "INVALID_ACTION"
    );
}

/// The report diffing used by the notifications loop.
#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn report_diffing() {
    let (built, _handle, _rx, tasks) = build(LIGHT_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "/devices/x/controls/K1", "1");

    let report = lock(device).core.report(&json!({}));
    assert_eq!(
        report["capabilities"],
        json!([[
            {"type": "devices.capabilities.on_off", "state": {"instance": "on", "value": true}},
            true
        ]])
    );

    let previous = json!({
        "id": "light1",
        "capabilities": [
            {"type": "devices.capabilities.on_off", "state": {"instance": "on", "value": true}}
        ],
        "properties": [],
    });
    let report = lock(device).core.report(&previous);
    assert_eq!(report["capabilities"][0][1], json!(false));

    feed(&built, &tasks, "/devices/x/controls/K1", "0");
    let report = lock(device).core.report(&previous);
    assert_eq!(report["capabilities"][0][1], json!(true));
}

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn mqtt_requirement_enforced() {
    let table: toml::Table = toml::from_str(
        r#"
[light1]
_class = "WbLight"
_mqtt_used = true
name = "x"
status_path = "s"
control_path = "c"
"#,
    )
    .unwrap();
    let err = devices::build_all(
        &table,
        &BuildContext {
            mqtt: None,
            tasks: TaskSpawner::new(),
        },
    )
    .map(|_| ())
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("Cannot initialize MQTT-based device: no MQTT config available"),
        "{err}"
    );
}

#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn config_typo_rejected() {
    let (_, handle, _rx, _tasks) = build(LIGHT_CFG);
    let table: toml::Table = toml::from_str(
        r#"
[light1]
_class = "WbLight"
_mqtt_used = true
name = "x"
status_path = "s"
control_path = "c"
unknown_field = 1
"#,
    )
    .unwrap();
    let err = devices::build_all(
        &table,
        &BuildContext {
            mqtt: Some(handle),
            tasks: TaskSpawner::new(),
        },
    )
    .map(|_| ())
    .unwrap_err();
    assert!(format!("{err:#}").contains("unknown field"), "{err:#}");
}

/// A single-channel status update must not produce a state derived
/// from one fresh and one stale channel. It settles after a quiet period
/// instead; paired updates settle immediately (as mixwhite_math_golden
/// shows).
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_single_channel_debounced() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];

    // consistent pair: settles immediately via the fast path
    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");
    let state = lock(device).core.state();
    let find = |state: &serde_json::Value, type_id: &str| -> serde_json::Value {
        state["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == type_id)
            .unwrap()["state"]["value"]
            .clone()
    };
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3808)
    );
    drain(&mut rx);

    // single-channel update: the derived state must stay consistent (old)
    // until the quiet period passes, not mix new warm with stale cold
    feed(&built, &tasks, "w", "20");
    let state = lock(device).core.state();
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3808)
    );
    assert_eq!(
        find(&state, "devices.capabilities.range").as_f64().unwrap(),
        69.38775510204081
    );

    // ... and the restore-on-turn-on values must not be corrupted by the
    // half-updated pair: off + on inside the window restores the settled
    // 3808K @ 69.4% channels, exactly as before the partial update
    devices::action(
        device,
        &[
            json!({"type": "devices.capabilities.on_off", "state": {"instance": "on", "value": false}}),
        ],
    );
    devices::action(
        device,
        &[
            json!({"type": "devices.capabilities.on_off", "state": {"instance": "on", "value": true}}),
        ],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "0".to_string()),
            ("w/on".to_string(), "0".to_string()),
            ("c/on".to_string(), "299".to_string()),
            ("w/on".to_string(), "700".to_string()),
        ]
    );

    // while the turn-on command still owes its echoes, even the elapsed
    // debounce does not derive from the stale external update: the command
    // echoes are the next truth
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let state = lock(device).core.state();
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3808)
    );

    // the turn-on echoes arrive and settle the pair (299, 700)
    feed(&built, &tasks, "c", "299");
    feed(&built, &tasks, "w", "700");
    let state = lock(device).core.state();
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3805)
    );

    // now a lone external update is applied after the quiet period,
    // derived from the new warm and the last-known cold value
    feed(&built, &tasks, "w", "20");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let state = lock(device).core.state();
    // warm=20 -> ratio 0, cold=299 -> ratio 0.2847: pure cold temperature
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(6500)
    );
    assert_eq!(
        find(&state, "devices.capabilities.range").as_f64().unwrap(),
        28.469387755102044
    );
}

/// No state is derived until both channels have published at least once:
/// half-known devices report nothing instead of fabricated values.
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_no_state_from_single_channel() {
    let (built, _handle, _rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "w", "700");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let state = lock(device).core.state();
    let caps = state["capabilities"].as_array().unwrap();
    // only the initial color temperature (set at construction) is
    // present; no on_off/brightness derived from a half-known pair
    assert!(
        !caps
            .iter()
            .any(|c| c["type"] == "devices.capabilities.on_off"),
        "{state}"
    );
    assert!(
        !caps
            .iter()
            .any(|c| c["type"] == "devices.capabilities.range"),
        "{state}"
    );
}

/// The command-side race: we publish the two channels, and the echo of the
/// first can arrive before the second publish even reaches the controller.
/// A lone echo matching the pending command must not trigger a derivation:
/// the partner echo gets a wider window than the plain debounce.
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_command_split_echo() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];
    let find = |state: &serde_json::Value, type_id: &str| -> serde_json::Value {
        state["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == type_id)
            .unwrap()["state"]["value"]
            .clone()
    };

    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");
    drain(&mut rx);

    // command: brightness 50% at the settled 3808K -> cold=221, warm=510
    // (both channels change)
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 50.0},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "221".to_string()),
            ("w/on".to_string(), "510".to_string()),
        ]
    );

    // the cold echo arrives alone; even past the plain debounce window the
    // state must stay at the old consistent pair, because the echo matches
    // a command still waiting for its partner
    feed(&built, &tasks, "c", "221");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let state = lock(device).core.state();
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3808)
    );
    assert_eq!(
        find(&state, "devices.capabilities.range").as_f64().unwrap(),
        69.38775510204081
    );

    // the partner echo completes the command pair: settle instantly
    feed(&built, &tasks, "w", "510");
    let state = lock(device).core.state();
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3805)
    );
    assert_eq!(
        find(&state, "devices.capabilities.range").as_f64().unwrap(),
        50.0
    );

    // a command whose channel already holds the target value gets no echo
    // for it: the lone changing-channel echo settles the pair immediately.
    // 4000K at the current 50% -> cold=274, warm=510 (warm pre-seen)
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.color_setting",
            "state": {"instance": "temperature_k", "value": 4000},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "274".to_string()),
            ("w/on".to_string(), "510".to_string()),
        ]
    );
    feed(&built, &tasks, "c", "274");
    let state = lock(device).core.state();
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3997)
    );
}

/// A command whose second echo never arrives settles (best effort) when the
/// expectation window runs out, not at the shorter debounce timeout.
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_command_echo_timeout() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];
    let find = |state: &serde_json::Value, type_id: &str| -> serde_json::Value {
        state["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == type_id)
            .unwrap()["state"]["value"]
            .clone()
    };

    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");
    drain(&mut rx);

    // set brightness 50% at the settled 3808K -> cold=221, warm=510
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 50.0},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "221".to_string()),
            ("w/on".to_string(), "510".to_string()),
        ]
    );

    // only the cold echo ever arrives
    feed(&built, &tasks, "c", "221");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    // still held back inside the expectation window
    assert_eq!(
        lock(device).core.state()["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == "devices.capabilities.color_setting")
            .unwrap()["state"]["value"],
        json!(3808)
    );

    // 5 * settle_ms (1s) after the echo the timer settles with what we have
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
    let state = lock(device).core.state();
    assert_eq!(
        find(&state, "devices.capabilities.color_setting"),
        json!(3566)
    );
}

/// An echo that does not match the pending command (wall switch interference
/// or controller clamping) drops the expectation: reality wins, and the next
/// consistent external pair settles immediately.
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_command_interference() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");
    drain(&mut rx);

    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.color_setting",
            "state": {"instance": "temperature_k", "value": 4000},
        })],
    );
    drain(&mut rx);

    let color_of = |state: &serde_json::Value| -> serde_json::Value {
        state["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == "devices.capabilities.color_setting")
            .unwrap()["state"]["value"]
            .clone()
    };

    // the controller reports something else entirely: the 4000K command
    // expected (cold=373, warm=700) with warm pre-seen at 700, so the
    // mismatching cold echo resolves the pair by reality right away
    feed(&built, &tasks, "c", "999");
    let state = lock(device).core.state();
    // derived from the observed pair (warm=700, cold=999)
    assert_eq!(color_of(&state), json!(4942));

    // a further external change debounces like any lone update
    feed(&built, &tasks, "w", "555");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let state = lock(device).core.state();
    // warm ratio 0.5459, cold ratio 0.9990: mostly cold light
    assert_eq!(color_of(&state), json!(5157));
}

/// A stale settle timer (left armed by an inline fast-path settle) firing
/// inside a newer command's expectation window must not wipe that
/// expectation — otherwise the command's split echoes would again derive a
/// half-applied pair and latch corrupted restore values.
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_stale_timer_respects_new_command() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");
    drain(&mut rx);

    // command A: brightness 50% -> (221, 510); the first echo arms the
    // expectation-window timer, the second settles inline and leaves that
    // timer armed for ~1s
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 50.0},
        })],
    );
    drain(&mut rx);
    feed(&built, &tasks, "c", "221");
    feed(&built, &tasks, "w", "510");

    // command B within that window: turn off -> (0, 0)
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": false},
        })],
    );
    drain(&mut rx);

    // command A's stale timer fires here; command B's echoes then arrive
    // farther apart than the plain debounce window
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    feed(&built, &tasks, "c", "0");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    feed(&built, &tasks, "w", "0");

    // the settled off-state must not have touched the restore values:
    // turn-on restores 3805K @ 50%, not a mix of B's cold and A's warm
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "220".to_string()),
            ("w/on".to_string(), "510".to_string()),
        ]
    );
}

/// A late echo of a *previous* command arriving after a newer command was
/// issued resolves that channel by reality but must not corrupt the restore
/// values: only cleanly confirmed pairs become the turn-on snapshot.
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_late_previous_echo_keeps_restore() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");
    drain(&mut rx);

    // command A: brightness 50% -> (221, 510); only its cold echo arrives
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.range",
            "state": {"instance": "brightness", "value": 50.0},
        })],
    );
    drain(&mut rx);
    feed(&built, &tasks, "c", "221");

    // command B follows: turn off -> (0, 0)
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": false},
        })],
    );
    drain(&mut rx);

    // A's delayed warm echo mismatches B's expectation (reality-resolved),
    // then B's echoes arrive far apart
    feed(&built, &tasks, "w", "510");
    feed(&built, &tasks, "c", "0");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    feed(&built, &tasks, "w", "0");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // turn-on restores the last cleanly confirmed lit state (3808K @ 69.4%),
    // not a mix of B's cold and A's warm
    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "299".to_string()),
            ("w/on".to_string(), "700".to_string()),
        ]
    );
}

/// A controller reporting a fading intermediate value during a turn-off
/// (mismatching echo) must not latch that transitional pair as the restore
/// state.
#[cfg(feature = "wirenboard")]
#[tokio::test(start_paused = true)]
async fn mixwhite_fade_intermediate_keeps_restore() {
    let (built, _handle, mut rx, tasks) = build(MIXWHITE_CFG);
    let (_, device) = &built.devices[0];

    feed(&built, &tasks, "w", "700");
    feed(&built, &tasks, "c", "300");
    drain(&mut rx);

    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": false},
        })],
    );
    drain(&mut rx);

    // fading: an intermediate cold value first, then the real zeros
    feed(&built, &tasks, "c", "100");
    feed(&built, &tasks, "w", "0");
    feed(&built, &tasks, "c", "0");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    devices::action(
        device,
        &[json!({
            "type": "devices.capabilities.on_off",
            "state": {"instance": "on", "value": true},
        })],
    );
    assert_eq!(
        drain(&mut rx),
        vec![
            ("c/on".to_string(), "299".to_string()),
            ("w/on".to_string(), "700".to_string()),
        ]
    );
}

/// The class registry: every compiled-in class is present exactly once.
#[test]
#[allow(clippy::vec_init_then_push)]
fn class_registry_complete_and_unique() {
    let mut names = devices::class_names();
    names.sort_unstable();
    let mut expected: Vec<&str> = Vec::new();
    #[cfg(feature = "influx")]
    expected.push("FreezerWatcher");
    #[cfg(feature = "wirenboard")]
    expected.extend([
        "WbCooler",
        "WbCurtain",
        "WbDimmableLight",
        "WbDimmableOnoffLight",
        "WbLeakSensor",
        "WbLight",
        "WbMixwhiteLight",
        "WbPirSensor",
        "WbRtdRa",
        "WbSensor",
        "WbSocket",
        "WbWaterValve",
    ]);
    expected.sort_unstable();
    assert_eq!(names, expected);
}

/// Smoke coverage for the classes no other test builds.
#[cfg(feature = "wirenboard")]
#[tokio::test]
async fn remaining_classes_build() {
    let cfg = r#"
[socket1]
_class = "WbSocket"
_mqtt_used = true
name = "Розетка"
status_path = "sock"
control_path = "sock/on"

[cooler1]
_class = "WbCooler"
_mqtt_used = true
name = "Вентилятор"
status_path = "cool"
control_path = "cool/on"

[dol1]
_class = "WbDimmableOnoffLight"
_mqtt_used = true
name = "Свет"
brightness_status_path = "b"
brightness_control_path = "b/on"
onoff_status_path = "o"
onoff_control_path = "o/on"
range_low = 20
range_high = 1000
"#;
    let (built, _handle, _rx, _tasks) = build(cfg);
    assert_eq!(built.devices.len(), 3);
    assert!(built.pollers.is_empty());
    for (id, expected_type) in [
        ("socket1", "devices.types.socket"),
        ("cooler1", "devices.types.switch"),
        ("dol1", "devices.types.light"),
    ] {
        let device = built.get(id).expect(id);
        let spec = lock(device).core.specification();
        assert_eq!(spec["type"], json!(expected_type), "{id}: {spec}");
        assert!(
            !spec["capabilities"].as_array().unwrap().is_empty(),
            "{id} has no capabilities"
        );
    }
}
