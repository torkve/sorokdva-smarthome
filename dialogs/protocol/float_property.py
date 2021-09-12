import abc
import enum
import typing

from .base import Property


__all__ = [
    'Amperage',
    'CO2Level',
    'Humidity',
    'Power',
    'Temperature',
    'Voltage',
    'WaterLevel',
]


class Float(Property):
    type_id = 'devices.properties.float'

    class Instance(enum.Enum):
        Amperage = 'amperage'
        CO2Level = 'co2_level'
        Humidity = 'humidity'
        Power = 'power'
        Temperature = 'temperature'
        Voltage = 'voltage'
        WaterLevel = 'water_level'

    class Unit(enum.Enum):
        Ampere = 'unit.ampere'
        Ppm = 'unit.ppm'
        Percent = 'unit.percent'
        Watt = 'unit.watt'
        Celsius = 'unit.temperature.celsius'
        Kelvin = 'unit.temperature.kelvin'
        Volt = 'unit.volt'

    def __init__(
        self,
        instance: Instance,
        unit: Unit,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        super().__init__(
            instance=instance.value,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )
        self.unit = unit

    def assign(self, value: float) -> None:
        self.validate(value)
        self.value = value

    @abc.abstractmethod
    def validate(self, value: float) -> None:
        """
        Validate value to meet restrictions
        """

    async def specification(self) -> dict:
        response = {
            'type': self.type_id,
            'retrievable': self.retrievable,
            'parameters': {
                'instance': self.instance,
                'unit': self.unit.value,
            }
        }

        return response


class Amperage(Float):
    def __init__(
        self,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        super().__init__(
            instance=Float.Instance.Amperage,
            unit=Float.Unit.Ampere,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    def validate(self, value: float) -> None:
        if value <= 0:
            raise ValueError(f"Amperage cannot be ≤0: got {value}")


class CO2Level(Float):
    def __init__(
        self,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        super().__init__(
            instance=Float.Instance.CO2Level,
            unit=Float.Unit.Ppm,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    def validate(self, value: float) -> None:
        if value <= 0:
            raise ValueError(f"CO₂ level cannot be ≤0: got {value}")


class Humidity(Float):
    def __init__(
        self,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        super().__init__(
            instance=Float.Instance.Humidity,
            unit=Float.Unit.Percent,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    def validate(self, value: float) -> None:
        if value < 0:
            raise ValueError(f"Humidity cannot be <0%: got {value}")
        if value > 100:
            raise ValueError(f"Humidity cannot be >100%: got {value}")


class Power(Float):
    def __init__(
        self,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        super().__init__(
            instance=Float.Instance.Power,
            unit=Float.Unit.Watt,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    def validate(self, value: float) -> None:
        if value < 0:
            raise ValueError(f"Power consumption cannot be <0: got {value}")


class Temperature(Float):
    def __init__(
        self,
        unit: Float.Unit,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        if unit not in (Float.Unit.Celsius, Float.Unit.Kelvin):
            raise TypeError(f"Not supported temperature mode: {unit}")

        super().__init__(
            instance=Float.Instance.Temperature,
            unit=unit,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    def validate(self, value: float) -> None:
        if self.unit == Float.Unit.Celsius.value and value < -273.15:
            raise ValueError(f"Temperature cannot be below absolute zero: got {value}°C")
        if self.unit == Float.Unit.Kelvin.value and value < 0:
            raise ValueError(f"Temperature cannot be below absolute zero: got {value} K")


class Voltage(Float):
    def __init__(
        self,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        super().__init__(
            instance=Float.Instance.Voltage,
            unit=Float.Unit.Volt,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    def validate(self, value: float) -> None:
        if value <= 0:
            raise ValueError(f"Voltage cannot be ≤0: got {value}")


class WaterLevel(Float):
    def __init__(
        self,
        initial_value: typing.Optional[float] = None,
        retrievable: bool = True,
        reportable: bool = False,
    ):
        super().__init__(
            instance=Float.Instance.WaterLevel,
            unit=Float.Unit.Percent,
            initial_value=initial_value,
            retrievable=retrievable,
            reportable=reportable,
        )

    def validate(self, value: float) -> None:
        if value < 0:
            raise ValueError(f"Water level cannot be <0%: got {value}")
        if value > 100:
            raise ValueError(f"Water level cannot be >100%: got {value}")
