use serde_json::{json, Map, Number, Value};

pub const ON_OFF: &str = "devices.capabilities.on_off";
pub const COLOR_SETTING: &str = "devices.capabilities.color_setting";
pub const MODE: &str = "devices.capabilities.mode";
pub const RANGE: &str = "devices.capabilities.range";
pub const TOGGLE: &str = "devices.capabilities.toggle";
pub const VIDEO_STREAM: &str = "devices.capabilities.video_stream";

/// The color scene ids Yandex currently recognizes.
pub const COLOR_SCENES: &[&str] = &[
    "alarm", "alice", "candle", "dinner", "fantasy", "garland", "jungle", "movie", "neon", "night",
    "ocean", "party", "reading", "rest", "romance", "siren",
];

/// devices.capabilities.on_off, single instance "on".
#[derive(Debug)]
pub struct OnOff {
    pub value: Option<bool>,
    pub retrievable: bool,
    pub reportable: bool,
    pub split: bool,
}

/// A color value currently held by a color_setting capability.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorValue {
    Hsv { h: i64, s: i64, v: i64 },
    Rgb(i64),
    TemperatureK(i64),
    Scene(String),
}

impl ColorValue {
    pub fn instance(&self) -> &'static str {
        match self {
            ColorValue::Hsv { .. } => "hsv",
            ColorValue::Rgb(_) => "rgb",
            ColorValue::TemperatureK(_) => "temperature_k",
            ColorValue::Scene(_) => "scene",
        }
    }

    pub fn serialize(&self) -> Value {
        match self {
            ColorValue::Hsv { h, s, v } => json!({"h": h, "s": s, "v": v}),
            ColorValue::Rgb(v) => json!(v),
            ColorValue::TemperatureK(v) => json!(v),
            ColorValue::Scene(scene) => json!(scene),
        }
    }
}

/// Which color models a color_setting capability supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorModel {
    Rgb,
    Hsv,
}

impl ColorModel {
    pub fn name(self) -> &'static str {
        match self {
            ColorModel::Rgb => "rgb",
            ColorModel::Hsv => "hsv",
        }
    }
}

/// devices.capabilities.color_setting.
#[derive(Debug)]
pub struct ColorSetting {
    pub color_model: Option<ColorModel>,
    /// (min, max) bounds for the temperature_k instance; present iff supported.
    pub temperature: Option<(Option<i64>, Option<i64>)>,
    /// Supported lighting scene ids (the "scene" instance); see COLOR_SCENES.
    pub scenes: Vec<&'static str>,
    pub value: Option<ColorValue>,
    pub retrievable: bool,
    pub reportable: bool,
}

impl ColorSetting {
    pub fn instances(&self) -> Vec<&'static str> {
        let mut result = Vec::new();
        if let Some(model) = self.color_model {
            result.push(model.name());
        }
        if self.temperature.is_some() {
            result.push("temperature_k");
        }
        if !self.scenes.is_empty() {
            result.push("scene");
        }
        result
    }

    pub fn parameters(&self) -> Value {
        let mut result = Map::new();
        if let Some(model) = self.color_model {
            result.insert("color_model".into(), json!(model.name()));
        }
        if let Some((min, max)) = self.temperature {
            let mut range = Map::new();
            if let Some(min) = min {
                range.insert("min".into(), json!(min));
            }
            if let Some(max) = max {
                range.insert("max".into(), json!(max));
            }
            result.insert("temperature_k".into(), Value::Object(range));
        }
        if !self.scenes.is_empty() {
            result.insert(
                "color_scene".into(),
                json!({
                    "scenes": self.scenes.iter().map(|id| json!({"id": id})).collect::<Vec<_>>(),
                }),
            );
        }
        Value::Object(result)
    }
}

/// devices.capabilities.mode, single instance.
#[derive(Debug)]
pub struct ModeCap {
    pub instance: &'static str,
    pub modes: Vec<&'static str>,
    pub value: Option<&'static str>,
    pub retrievable: bool,
    pub reportable: bool,
}

/// devices.capabilities.range, single instance.
#[derive(Debug)]
pub struct RangeCap {
    pub instance: &'static str,
    pub unit: Option<&'static str>,
    pub random_access: Option<bool>,
    pub min_value: Option<Number>,
    pub max_value: Option<Number>,
    pub precision: Option<Number>,
    /// Kept as a raw JSON number so the serialized state preserves the
    /// int/float distinction (a switched-off dimmer reports the int 0).
    pub value: Option<Number>,
    pub retrievable: bool,
    pub reportable: bool,
}

impl RangeCap {
    pub fn parameters(&self) -> Value {
        let mut result = Map::new();
        result.insert("instance".into(), json!(self.instance));
        if let Some(random_access) = self.random_access {
            result.insert("random_access".into(), json!(random_access));
        }
        if let Some(unit) = self.unit {
            result.insert("unit".into(), json!(unit));
        }
        let mut range = Map::new();
        if let Some(min) = &self.min_value {
            range.insert("min".into(), Value::Number(min.clone()));
        }
        if let Some(max) = &self.max_value {
            range.insert("max".into(), Value::Number(max.clone()));
        }
        if let Some(precision) = &self.precision {
            range.insert("precision".into(), Value::Number(precision.clone()));
        }
        if !range.is_empty() {
            result.insert("range".into(), Value::Object(range));
        }
        Value::Object(result)
    }
}

/// devices.capabilities.toggle, single instance.
#[derive(Debug)]
pub struct ToggleCap {
    pub instance: &'static str,
    pub value: Option<bool>,
    pub retrievable: bool,
    pub reportable: bool,
}

/// devices.capabilities.video_stream: cameras handing out an HLS stream URL
/// through the action call. Never retrievable or reportable, has no state.
#[derive(Debug)]
pub struct VideoStreamCap {
    /// Supported streaming protocols; Yandex currently accepts only "hls".
    pub protocols: Vec<&'static str>,
}

#[derive(Debug)]
pub enum Capability {
    OnOff(OnOff),
    Color(ColorSetting),
    Mode(ModeCap),
    Range(RangeCap),
    Toggle(ToggleCap),
    VideoStream(VideoStreamCap),
}

impl Capability {
    pub fn type_id(&self) -> &'static str {
        match self {
            Capability::OnOff(_) => ON_OFF,
            Capability::Color(_) => COLOR_SETTING,
            Capability::Mode(_) => MODE,
            Capability::Range(_) => RANGE,
            Capability::Toggle(_) => TOGGLE,
            Capability::VideoStream(_) => VIDEO_STREAM,
        }
    }

    pub fn instances(&self) -> Vec<&'static str> {
        match self {
            Capability::OnOff(_) => vec!["on"],
            Capability::Color(c) => c.instances(),
            Capability::Mode(c) => vec![c.instance],
            Capability::Range(c) => vec![c.instance],
            Capability::Toggle(c) => vec![c.instance],
            Capability::VideoStream(_) => vec!["get_stream"],
        }
    }

    pub fn retrievable(&self) -> bool {
        match self {
            Capability::OnOff(c) => c.retrievable,
            Capability::Color(c) => c.retrievable,
            Capability::Mode(c) => c.retrievable,
            Capability::Range(c) => c.retrievable,
            Capability::Toggle(c) => c.retrievable,
            Capability::VideoStream(_) => false,
        }
    }

    pub fn reportable(&self) -> bool {
        match self {
            Capability::OnOff(c) => c.reportable,
            Capability::Color(c) => c.reportable,
            Capability::Mode(c) => c.reportable,
            Capability::Range(c) => c.reportable,
            Capability::Toggle(c) => c.reportable,
            Capability::VideoStream(_) => false,
        }
    }

    pub fn parameters(&self) -> Option<Value> {
        match self {
            Capability::OnOff(c) => Some(json!({"split": c.split})),
            Capability::Color(c) => Some(c.parameters()),
            Capability::Mode(c) => Some(json!({
                "instance": c.instance,
                "modes": c.modes.iter().map(|m| json!({"value": m})).collect::<Vec<_>>(),
            })),
            Capability::Range(c) => Some(c.parameters()),
            Capability::Toggle(c) => Some(json!({"instance": c.instance})),
            Capability::VideoStream(c) => Some(json!({"protocols": c.protocols})),
        }
    }

    /// Current state, or None when the value is not known yet.
    pub fn state(&self) -> Option<Value> {
        let (instance, value) = match self {
            Capability::OnOff(c) => ("on", json!(c.value?)),
            Capability::Color(c) => {
                let value = c.value.as_ref()?;
                (value.instance(), value.serialize())
            }
            Capability::Mode(c) => (c.instance, json!(c.value?)),
            Capability::Range(c) => (c.instance, Value::Number(c.value.clone()?)),
            Capability::Toggle(c) => (c.instance, json!(c.value?)),
            // video_stream has no state at all
            Capability::VideoStream(_) => return None,
        };
        Some(json!({
            "type": self.type_id(),
            "state": {
                "instance": instance,
                "value": value,
            }
        }))
    }

    /// Validate the declared parameters against what Yandex accepts.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Capability::Color(c) => {
                for scene in &c.scenes {
                    if !COLOR_SCENES.contains(scene) {
                        return Err(format!("unknown color scene {scene:?}"));
                    }
                }
                Ok(())
            }
            Capability::VideoStream(c) => {
                if c.protocols.is_empty() {
                    return Err("video_stream must declare at least one protocol".to_string());
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Capability specification for the device list API call.
    pub fn specification(&self) -> Value {
        let mut result = Map::new();
        result.insert("type".into(), json!(self.type_id()));
        result.insert("retrievable".into(), json!(self.retrievable()));
        result.insert("reportable".into(), json!(self.reportable()));
        if let Some(parameters) = self.parameters() {
            result.insert("parameters".into(), parameters);
        }
        Value::Object(result)
    }
}
