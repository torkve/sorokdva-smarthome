import enum
import typing
from dataclasses import dataclass, asdict

from .base import Capability, SingleInstanceCapability, ChangeValue


__all__ = [
    'OnOff',
    'ColorSetting',
    'Mode',
    'Range',
    'Toggle',
]


class OnOff(SingleInstanceCapability):
    type_id = "devices.capabilities.on_off"

    def __init__(
        self,
        change_value: ChangeValue[bool] = None,
        initial_value: typing.Optional[bool] = None,
        retrievable: bool = False,
        split: bool = False,
    ):
        super().__init__(
            instance='on',
            initial_value=initial_value,
            change_value=change_value,
            retrievable=retrievable,
        )
        self.split = split

    @property
    def parameters(self) -> dict:
        return {'split': self.split}


class ColorSetting(Capability):
    type_id = "devices.capabilities.color_setting"

    @dataclass
    class HSV:
        h: typing.Optional[int] = None
        s: typing.Optional[int] = None
        v: typing.Optional[int] = None

        def __post_init__(self):
            self.validate(self.h, self.s, self.v)

        def validate(self, h, s, v):
            if h is None and s is None and v is None:
                return

            if not isinstance(h, int):
                raise TypeError(f"Hue must be int: got {type(h).__name__}")
            if not isinstance(s, int):
                raise TypeError(f"Saturation must be int: got {type(s).__name__}")
            if not isinstance(v, int):
                raise TypeError(f"Value must be int: got {type(v).__name__}")

            if not (0 <= h <= 360):
                raise ValueError(f"Hue must be in range [0; 360]: got {h}")
            if not (0 <= s <= 100):
                raise ValueError(f"Saturation must be in range [0; 100]: got {s}")
            if not (0 <= v <= 100):
                raise ValueError(f"Value must be in range [0; 100]: got {v}")

        def serialize(self) -> dict:
            return asdict(self)

        def assign(self, value: dict) -> None:
            h = value['h']
            s = value['s']
            v = value['v']

            assert h is not None and s is not None and v is not None
            self.validate(h, s, v)

            self.h = h
            self.s = s
            self.v = v

        @property
        def name(self):
            return 'hsv'

    @dataclass
    class RGB:
        value: typing.Optional[int] = None

        def __post_init__(self):
            self.validate(self.value)

        def validate(self, value):
            if value is None:
                return
            if not isinstance(value, int):
                raise TypeError(f"Color must be int: got {type(value).__name__}")
            if not (0 <= value <= 0xffffff):
                raise ValueError(f"Color must be within range [000000; FFFFFF]: got {value:06X}")

        def serialize(self) -> typing.Optional[int]:
            return self.value

        def assign(self, value: int) -> None:
            self.validate(value)
            self.value = value

        @property
        def name(self):
            return 'rgb'

    @dataclass
    class Temperature:
        min: typing.Optional[int] = None
        max: typing.Optional[int] = None

        value: typing.Optional[int] = None

        def __post_init__(self):
            self.validate(self.value)

        def validate(self, value):
            if value is None:
                return
            if not isinstance(value, int):
                raise TypeError(f"Color temperature must be int: got {type(value).__name__}")

            if self.min is not None and value < self.min:
                raise ValueError(f"Color temperature must be ≥{self.min}: got {value}")

            if self.max is not None and value > self.max:
                raise ValueError(f"Color temperature must be ≤{self.max}: got {value}")

        def serialize(self) -> int:
            assert self.value is not None
            return self.value

        def assign(self, value: int) -> None:
            self.validate(value)
            self.value = value

        @property
        def name(self):
            return 'temperature_k'

    ValueType = typing.Union[HSV, RGB, Temperature]

    def __init__(
        self,
        change_value: ChangeValue[ValueType] = None,
        color_model: typing.Optional[typing.Union[RGB, HSV]] = None,
        temperature: typing.Optional[Temperature] = None,
        retrievable: bool = False,
    ):
        if color_model is None and temperature is None:
            raise TypeError("Either color_model or temperature_range must be specified")

        if (
            color_model is not None
            and temperature is not None
            and color_model.serialize() is not None
            and temperature.value is not None
        ):
            raise ValueError("Both color and color temperature cannot be set simuntaneously")

        instances = []
        if color_model is not None:
            instances.append(color_model.name)
        if temperature is not None:
            instances.append(temperature.name)

        self.color_model = color_model
        self.temperature = temperature

        value: typing.Optional[ColorSetting.ValueType]
        if color_model is not None and color_model.serialize() is not None:
            value = color_model
        elif temperature is not None and temperature.value is not None:
            value = temperature
        else:
            value = None

        super().__init__(
            instances=instances,
            initial_value=value,
            change_value=change_value,
            retrievable=retrievable,
        )

    @property
    def parameters(self) -> dict:
        result: dict = {
        }

        if self.color_model is not None:
            result['color_model'] = self.color_model.name

        if self.temperature is not None:
            result['temperature_k'] = {}
            if self.temperature.min is not None:
                result['temperature_k']['min'] = self.temperature.min
            if self.temperature.max is not None:
                result['temperature_k']['max'] = self.temperature.max

        return result

    async def state(self) -> typing.AsyncIterator[dict]:
        if self.value is None:
            return

        value = self.value.serialize()
        if value is None:
            return

        yield {
            'type': self.type_id,
            'state': {
                'instance': self.value.name,
                'value': value,
            }
        }


class Mode(SingleInstanceCapability):
    type_id = "devices.capabilities.mode"

    class Instance(enum.Enum):
        CleanupMode = 'cleanup_mode'
        CoffeeMode = 'coffee_mode'
        FanSpeed = 'fan_speed'
        InputSource = 'input_source'
        Program = 'program'
        Swing = 'swing'
        Thermostat = 'thermostat'
        WorkSpeed = 'work_speed'

    class WorkMode(enum.Enum):
        Auto = 'auto'
        Eco = 'eco'
        Turbo = 'turbo'

        Cool = 'cool'
        Dry = 'dry'
        FanOnly = 'fan_only'
        Heat = 'heat'
        Preheat = 'preheat'

        High = 'high'
        Low = 'low'
        Medium = 'medium'

        Max = 'max'
        Min = 'min'

        Fast = 'fast'
        Slow = 'slow'

        Express = 'express'
        Normal = 'normal'
        Quiet = 'quiet'

        Horizontal = 'horizontal'
        Stationary = 'stationary'
        Vertical = 'vertical'

        One = 'one'
        Two = 'two'
        Three = 'three'
        Four = 'four'
        Five = 'five'
        Six = 'six'
        Seven = 'seven'
        Eight = 'eight'
        Nine = 'nine'
        Ten = 'ten'

        Americano = 'americano'
        Cappucino = 'cappucino'
        DoubleEspresso = 'double_espresso'
        Espresso = 'espresso'
        Latte = 'latte'

    def __init__(
        self,
        instance: Instance,
        modes: typing.Iterable[WorkMode],
        change_value: ChangeValue[WorkMode] = None,
        initial_value: typing.Optional[WorkMode] = None,
        retrievable: bool = False,
    ):
        super().__init__(
            instance=instance.value,
            initial_value=initial_value,
            change_value=change_value,
            retrievable=retrievable,
        )

        self.modes = list(modes)
        if not self.modes:
            raise TypeError('Device must have at least one available working mode')

        if initial_value is not None and initial_value not in self.modes:
            raise ValueError(f'Mode {initial_value} is not in supported list for this capability')

    @property
    def parameters(self) -> dict:
        return {
            'instance': self.instance,
            'modes': [{"value": mode.value} for mode in self.modes],
        }


class Range(SingleInstanceCapability):
    type_id = "devices.capabilities.range"

    class Instance(enum.Enum):
        Brightness = 'brightness'
        Temperature = 'temperature'
        Volume = 'volume'
        Channel = 'channel'
        Humidity = 'humidity'

    class Unit(enum.Enum):
        Percent = 'unit.percent'
        TemperatureCelsius = 'unit.temperature.celsius'
        TemperatureKelvin = 'unit.temperature.kelvin'

    def __init__(
        self,
        instance: Instance,
        change_value: ChangeValue[float] = None,
        unit: typing.Optional[Unit] = None,
        random_access: typing.Optional[bool] = True,
        min_value: typing.Optional[float] = None,
        max_value: typing.Optional[float] = None,
        precision: typing.Optional[float] = 1,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = False,
    ):
        super().__init__(
            instance=instance.value,
            initial_value=initial_value,
            change_value=change_value,
            retrievable=retrievable,
        )
        self.unit = unit
        self.random_access = random_access
        self.min_value = min_value
        self.max_value = max_value
        self.precision = precision

        if (
            (
                unit == self.Unit.Percent
                and instance not in (self.Instance.Brightness, self.Instance.Humidity)
            )
            or (
                unit in (self.Unit.TemperatureCelsius, self.Unit.TemperatureKelvin)
                and instance != self.Instance.Temperature
            )
        ):
            raise TypeError(f"Unit {unit} is not supported for instance {instance}")

        if (
            instance in (self.Instance.Brightness, self.Instance.Humidity)
            and min_value is not None
            and min_value < 0
        ):
            raise ValueError(f'Minimum value for {instance} cannot be less than 0 (got {min_value})')

        if (
            instance in (self.Instance.Brightness, self.Instance.Humidity)
            and max_value is not None
            and max_value > 100
        ):
            raise ValueError(f'Maximum value for {instance} cannot be greater than 100 (got {max_value})')

    @property
    def parameters(self) -> dict:
        result: dict = {
            'instance': self.instance,
        }

        if self.random_access is not None:
            result['random_access'] = self.random_access

        if self.unit is not None:
            result['unit'] = self.unit.value

        if self.min_value is not None:
            result.setdefault('range', {})['min'] = self.min_value

        if self.max_value is not None:
            result.setdefault('range', {})['max'] = self.max_value

        if self.precision is not None:
            result.setdefault('range', {})['precision'] = self.precision

        return result


class Toggle(SingleInstanceCapability):
    type_id = "devices.capabilities.toggle"

    class Instance(enum.Enum):
        Mute = 'mute'
        Backlight = 'backlight'
        ControlsLocked = 'controls_locked'
        Ionization = 'ionization'
        Oscillation = 'oscillation'
        KeepWarm = 'keep_warm'
        Pause = 'pause'

    def __init__(
        self,
        instance: Instance,
        change_value: ChangeValue[float] = None,
        initial_value: typing.Optional[bool] = None,
        retrievable: bool = False,
    ):
        super().__init__(
            instance=instance.value,
            initial_value=initial_value,
            change_value=change_value,
            retrievable=retrievable,
        )

    @property
    def parameters(self) -> dict:
        return {
            'instance': self.instance,
        }
