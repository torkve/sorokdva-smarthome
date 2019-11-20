import enum
import typing

from .base import Capability, ChangeValue


__all__ = [
    'OnOff',
    'ColorSetting',
    'Mode',
    'Range',
    'Toggle',
]


class OnOff(Capability):
    type_id = "devices.capabilities.on_off"
    parameters = None

    def __init__(
        self,
        change_value: ChangeValue[bool] = None,
        initial_value: typing.Optional[bool] = None,
        retrievable: bool = False,
    ):
        super().__init__(
            instance='on',
            initial_value=initial_value,
            change_value=change_value,
            retrievable=retrievable,
        )


class ColorSetting(Capability):
    type_id = "devices.capabilities.color_setting"

    # TODO implement the rest


class Mode(Capability):
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
            raise TypeError(f'Device must have at least one available working mode')

    @property
    def parameters(self) -> dict:
        return {
            'instance': self.instance,
            'modes': [mode.value for mode in self.modes],
        }


class Range(Capability):
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


class Toggle(Capability):
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
