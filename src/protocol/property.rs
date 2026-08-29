use serde_json::{json, Value};

pub const FLOAT: &str = "devices.properties.float";
pub const EVENT: &str = "devices.properties.event";

/// devices.properties.float kinds, fixing instance, unit and validation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatKind {
    Amperage,
    BatteryLevel,
    CO2Level,
    ElectricityMeter,
    FoodLevel,
    GasMeter,
    HeatMeter,
    Humidity,
    Illumination,
    /// A generic meter reading in abstract units (no unit parameter).
    Meter,
    Pm1Density,
    Pm25Density,
    Pm10Density,
    Power,
    PressureAtm,
    PressurePascal,
    PressureBar,
    PressureMmHg,
    TemperatureCelsius,
    TemperatureKelvin,
    Tvoc,
    Voltage,
    WaterLevel,
    WaterMeter,
}

impl FloatKind {
    pub fn instance(self) -> &'static str {
        match self {
            FloatKind::Amperage => "amperage",
            FloatKind::BatteryLevel => "battery_level",
            FloatKind::CO2Level => "co2_level",
            FloatKind::ElectricityMeter => "electricity_meter",
            FloatKind::FoodLevel => "food_level",
            FloatKind::GasMeter => "gas_meter",
            FloatKind::HeatMeter => "heat_meter",
            FloatKind::Humidity => "humidity",
            FloatKind::Illumination => "illumination",
            FloatKind::Meter => "meter",
            FloatKind::Pm1Density => "pm1_density",
            FloatKind::Pm25Density => "pm2.5_density",
            FloatKind::Pm10Density => "pm10_density",
            FloatKind::Power => "power",
            FloatKind::PressureAtm
            | FloatKind::PressurePascal
            | FloatKind::PressureBar
            | FloatKind::PressureMmHg => "pressure",
            FloatKind::TemperatureCelsius | FloatKind::TemperatureKelvin => "temperature",
            FloatKind::Tvoc => "tvoc",
            FloatKind::Voltage => "voltage",
            FloatKind::WaterLevel => "water_level",
            FloatKind::WaterMeter => "water_meter",
        }
    }

    /// The unit parameter; None for the unitless "meter" instance.
    pub fn unit(self) -> Option<&'static str> {
        match self {
            FloatKind::Amperage => Some("unit.ampere"),
            FloatKind::CO2Level => Some("unit.ppm"),
            FloatKind::BatteryLevel
            | FloatKind::FoodLevel
            | FloatKind::Humidity
            | FloatKind::WaterLevel => Some("unit.percent"),
            FloatKind::ElectricityMeter => Some("unit.kilowatt_hour"),
            FloatKind::GasMeter | FloatKind::WaterMeter => Some("unit.cubic_meter"),
            FloatKind::HeatMeter => Some("unit.gigacalorie"),
            FloatKind::Illumination => Some("unit.illumination.lux"),
            FloatKind::Meter => None,
            FloatKind::Pm1Density
            | FloatKind::Pm25Density
            | FloatKind::Pm10Density
            | FloatKind::Tvoc => Some("unit.density.mcg_m3"),
            FloatKind::Power => Some("unit.watt"),
            FloatKind::PressureAtm => Some("unit.pressure.atm"),
            FloatKind::PressurePascal => Some("unit.pressure.pascal"),
            FloatKind::PressureBar => Some("unit.pressure.bar"),
            FloatKind::PressureMmHg => Some("unit.pressure.mmhg"),
            FloatKind::TemperatureCelsius => Some("unit.temperature.celsius"),
            FloatKind::TemperatureKelvin => Some("unit.temperature.kelvin"),
            FloatKind::Voltage => Some("unit.volt"),
        }
    }

    pub fn validate(self, value: f64) -> Result<(), String> {
        match self {
            FloatKind::Amperage if value <= 0. => {
                Err(format!("Amperage cannot be ≤0: got {value}"))
            }
            FloatKind::CO2Level if value <= 0. => {
                Err(format!("CO₂ level cannot be ≤0: got {value}"))
            }
            FloatKind::Humidity if value < 0. => {
                Err(format!("Humidity cannot be <0%: got {value}"))
            }
            FloatKind::Humidity if value > 100. => {
                Err(format!("Humidity cannot be >100%: got {value}"))
            }
            FloatKind::Power if value < 0. => {
                Err(format!("Power consumption cannot be <0: got {value}"))
            }
            FloatKind::TemperatureCelsius if value < -273.15 => Err(format!(
                "Temperature cannot be below absolute zero: got {value}°C"
            )),
            FloatKind::TemperatureKelvin if value < 0. => Err(format!(
                "Temperature cannot be below absolute zero: got {value} K"
            )),
            FloatKind::Voltage if value <= 0. => Err(format!("Voltage cannot be ≤0: got {value}")),
            FloatKind::WaterLevel if value < 0. => {
                Err(format!("Water level cannot be <0%: got {value}"))
            }
            FloatKind::WaterLevel if value > 100. => {
                Err(format!("Water level cannot be >100%: got {value}"))
            }
            FloatKind::BatteryLevel | FloatKind::FoodLevel if !(0. ..=100.).contains(&value) => {
                Err(format!("Percentage must be in [0; 100]: got {value}"))
            }
            FloatKind::ElectricityMeter
            | FloatKind::GasMeter
            | FloatKind::HeatMeter
            | FloatKind::WaterMeter
            | FloatKind::Meter
            | FloatKind::Illumination
            | FloatKind::Pm1Density
            | FloatKind::Pm25Density
            | FloatKind::Pm10Density
            | FloatKind::Tvoc
            | FloatKind::PressureAtm
            | FloatKind::PressurePascal
            | FloatKind::PressureBar
            | FloatKind::PressureMmHg
                if value < 0. =>
            {
                Err(format!("Value cannot be negative: got {value}"))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug)]
pub struct FloatProp {
    pub kind: FloatKind,
    /// Kept as a raw JSON number so the serialized state preserves the
    /// int/float distinction.
    pub value: Option<serde_json::Number>,
    pub retrievable: bool,
    pub reportable: bool,
}

impl FloatProp {
    pub fn new(kind: FloatKind, retrievable: bool, reportable: bool) -> Self {
        FloatProp {
            kind,
            value: None,
            retrievable,
            reportable,
        }
    }

    pub fn assign(&mut self, value: serde_json::Number) -> Result<(), String> {
        self.kind.validate(value.as_f64().unwrap_or(0.))?;
        self.value = Some(value);
        Ok(())
    }

    pub fn assign_f64(&mut self, value: f64) -> Result<(), String> {
        let number = serde_json::Number::from_f64(value)
            .ok_or_else(|| format!("not a finite number: {value}"))?;
        self.assign(number)
    }
}

/// devices.properties.event kinds used by the devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Vibration,
    Open,
    Button,
    Motion,
    Smoke,
    Gas,
    BatteryLevel,
    FoodLevel,
    WaterLevel,
    WaterLeak,
}

impl EventKind {
    pub fn instance(self) -> &'static str {
        match self {
            EventKind::Vibration => "vibration",
            EventKind::Open => "open",
            EventKind::Button => "button",
            EventKind::Motion => "motion",
            EventKind::Smoke => "smoke",
            EventKind::Gas => "gas",
            EventKind::BatteryLevel => "battery_level",
            EventKind::FoodLevel => "food_level",
            EventKind::WaterLevel => "water_level",
            EventKind::WaterLeak => "water_leak",
        }
    }

    pub fn events(self) -> &'static [&'static str] {
        match self {
            EventKind::Vibration => &["tilt", "fall", "vibration"],
            EventKind::Open => &["opened", "closed"],
            EventKind::Button => &["click", "double_click", "long_press"],
            EventKind::Motion => &["detected", "not_detected"],
            EventKind::Smoke | EventKind::Gas => &["detected", "not_detected", "high"],
            EventKind::BatteryLevel => &["low", "normal"],
            EventKind::FoodLevel | EventKind::WaterLevel => &["empty", "low", "normal"],
            EventKind::WaterLeak => &["dry", "leak"],
        }
    }
}

#[derive(Debug)]
pub struct EventProp {
    pub kind: EventKind,
    pub value: Option<&'static str>,
    pub retrievable: bool,
    pub reportable: bool,
}

impl EventProp {
    pub fn new(kind: EventKind, retrievable: bool, reportable: bool) -> Self {
        EventProp {
            kind,
            value: None,
            retrievable,
            reportable,
        }
    }
}

#[derive(Debug)]
pub enum Property {
    Float(FloatProp),
    Event(EventProp),
}

impl Property {
    pub fn type_id(&self) -> &'static str {
        match self {
            Property::Float(_) => FLOAT,
            Property::Event(_) => EVENT,
        }
    }

    pub fn instance(&self) -> &'static str {
        match self {
            Property::Float(p) => p.kind.instance(),
            Property::Event(p) => p.kind.instance(),
        }
    }

    pub fn retrievable(&self) -> bool {
        match self {
            Property::Float(p) => p.retrievable,
            Property::Event(p) => p.retrievable,
        }
    }

    pub fn reportable(&self) -> bool {
        match self {
            Property::Float(p) => p.reportable,
            Property::Event(p) => p.reportable,
        }
    }

    /// Current state, or None when the value is not known yet.
    pub fn state(&self) -> Option<Value> {
        let value = match self {
            Property::Float(p) => Value::Number(p.value.clone()?),
            Property::Event(p) => json!(p.value?),
        };
        Some(json!({
            "type": self.type_id(),
            "state": {
                "instance": self.instance(),
                "value": value,
            }
        }))
    }

    /// Property specification for the device list API call, including
    /// the "reportable" flag.
    pub fn specification(&self) -> Value {
        match self {
            Property::Float(p) => {
                let mut parameters = serde_json::Map::new();
                parameters.insert("instance".into(), json!(p.kind.instance()));
                if let Some(unit) = p.kind.unit() {
                    parameters.insert("unit".into(), json!(unit));
                }
                json!({
                    "type": FLOAT,
                    "retrievable": p.retrievable,
                    "reportable": p.reportable,
                    "parameters": parameters,
                })
            }
            Property::Event(p) => json!({
                "type": EVENT,
                "retrievable": p.retrievable,
                "reportable": p.reportable,
                "parameters": {
                    "instance": p.kind.instance(),
                    "events": p.kind.events().iter().map(|e| json!({"value": e})).collect::<Vec<_>>(),
                }
            }),
        }
    }
}
