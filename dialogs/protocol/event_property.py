import enum
import typing

from .base import Property


__all__ = [
    'Vibration',
    'Open',
    'Button',
    'Motion',
    'Smoke',
    'Gas',
    'BatteryLevel',
    'WaterLevel',
    'WaterLeak',
]

E = typing.TypeVar('E', bound=enum.Enum)


class Event(typing.Generic[E], Property):
    type_id = 'devices.properties.event'

    class Instance(enum.Enum):
        Vibration = 'vibration'
        Open = 'open'
        Button = 'button'
        Motion = 'motion'
        Smoke = 'smoke'
        Gas = 'gas'
        BatteryLevel = 'battery_level'
        WaterLevel = 'water_level'
        WaterLeak = 'water_leak'

    def __init__(
        self,
        instance: Instance,
        events: typing.Type[E],
        initial_value: typing.Optional[E] = None,
    ):
        super().__init__(
            instance=instance.value,
            initial_value=initial_value.value if initial_value is not None else None,
        )
        self.events = events

    async def specification(self) -> dict:
        response = {
            'type': self.type_id,
            'retrievable': self.retrievable,
            'parameters': {
                'instance': self.instance,
                'events': [{'value': event.value} for event in self.events.__members__.values()],
            }
        }

        return response

    def assign(self, value: E) -> None:
        self.validate(value)
        self.value = value.value

    def validate(self, value: E) -> None:
        """
        Validate value to meet restrictions
        """


class Vibration(Event["Vibration.Value"]):
    class Value(enum.Enum):
        Tilt = "tilt"
        Fall = "fall"
        Vibration = "vibration"

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.Vibration,
            events=self.Value,
            initial_value=initial_value,
        )


class Open(Event["Open.Value"]):
    class Value(enum.Enum):
        Opened = "opened"
        Closed = "closed"

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.Open,
            events=self.Value,
            initial_value=initial_value,
        )


class Button(Event["Button.Value"]):
    class Value(enum.Enum):
        Click = "click"
        DoubleClick = "double_click"
        LongPress = "long_press"

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.Button,
            events=self.Value,
            initial_value=initial_value,
        )


class Motion(Event["Motion.Value"]):
    class Value(enum.Enum):
        Detected = "detected"
        NotDetected = "not_detected"

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.Motion,
            events=self.Value,
            initial_value=initial_value,
        )


class Smoke(Event["Smoke.Value"]):
    class Value(enum.Enum):
        Detected = "detected"
        NotDetected = "not_detected"
        High = 'high'

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.Smoke,
            events=self.Value,
            initial_value=initial_value,
        )


class Gas(Event["Gas.Value"]):
    class Value(enum.Enum):
        Detected = "detected"
        NotDetected = "not_detected"
        High = 'high'

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.Gas,
            events=self.Value,
            initial_value=initial_value,
        )


class BatteryLevel(Event["BatteryLevel.Value"]):
    class Value(enum.Enum):
        Low = "low"
        Normal = "normal"

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.BatteryLevel,
            events=self.Value,
            initial_value=initial_value,
        )


class WaterLevel(Event["WaterLevel.Value"]):
    class Value(enum.Enum):
        Low = "low"
        Normal = "normal"

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.WaterLevel,
            events=self.Value,
            initial_value=initial_value,
        )


class WaterLeak(Event["WaterLeak.Value"]):
    class Value(enum.Enum):
        Dry = "dry"
        Leak = "leak"

    def __init__(
        self,
        initial_value: typing.Optional[Value] = None,
    ):
        super().__init__(
            instance=Event.Instance.WaterLeak,
            events=self.Value,
            initial_value=initial_value,
        )
