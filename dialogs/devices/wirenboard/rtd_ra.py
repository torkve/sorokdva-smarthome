"""
Implementation of the Daikin FTXM-M/ATXM-M air conditioner
connected to the Wirenboard using RTD-RA interface module.

In MQTT tree it appears like this:
    /devices/RTD-NET_10/meta/name RTD-NET 10
    /devices/RTD-NET_10/controls/Setpoint 25
    /devices/RTD-NET_10/controls/Setpoint/meta/type range
    /devices/RTD-NET_10/controls/Setpoint/meta/max 32
    /devices/RTD-NET_10/controls/Setpoint/meta/order 1
    /devices/RTD-NET_10/controls/Fanspeed 0
    /devices/RTD-NET_10/controls/Fanspeed/meta/type range
    /devices/RTD-NET_10/controls/Fanspeed/meta/max 3
    /devices/RTD-NET_10/controls/Fanspeed/meta/order 2
    /devices/RTD-NET_10/controls/Mode 0
    /devices/RTD-NET_10/controls/Mode/meta/type range
    /devices/RTD-NET_10/controls/Mode/meta/max 4
    /devices/RTD-NET_10/controls/Mode/meta/order 3
    /devices/RTD-NET_10/controls/Louvre 0
    /devices/RTD-NET_10/controls/Louvre/meta/type range
    /devices/RTD-NET_10/controls/Louvre/meta/max 3
    /devices/RTD-NET_10/controls/Louvre/meta/order 4
    /devices/RTD-NET_10/controls/OnOff 0
    /devices/RTD-NET_10/controls/OnOff/meta/type switch
    /devices/RTD-NET_10/controls/OnOff/meta/order 5
    /devices/RTD-NET_10/controls/Return Air Temperature 26.5
    /devices/RTD-NET_10/controls/Return Air Temperature/meta/type temperature
    /devices/RTD-NET_10/controls/Return Air Temperature/meta/readonly 1
    /devices/RTD-NET_10/controls/Return Air Temperature/meta/order 6
    /devices/RTD-NET_10/controls/Coil Inlet Temperature 26
    /devices/RTD-NET_10/controls/Coil Inlet Temperature/meta/type temperature
    /devices/RTD-NET_10/controls/Coil Inlet Temperature/meta/readonly 1
    /devices/RTD-NET_10/controls/Coil Inlet Temperature/meta/order 7

So the constructor accepts path to /controls section and constructs
all the subsequent paths automatically.
"""

import typing

from dialogs.protocol.consts import ActionError
from dialogs.protocol.exceptions import ActionException
from dialogs.protocol.device import AirConditioner
from dialogs.protocol.capability import Mode, OnOff, Range
from dialogs.protocol.float_property import Temperature
from dialogs.mqtt_client import MqttClient


class WbRtdRa(AirConditioner):
    FANSPEED_MODES_MAP = {
        Mode.WorkMode.Auto.value: '0',
        Mode.WorkMode.One.value: '1',
        Mode.WorkMode.Two.value: '2',
        Mode.WorkMode.Three.value: '3',
        Mode.WorkMode.Four.value: '4',
        Mode.WorkMode.Five.value: '5',
    }
    FANSPEED_MODES_REVMAP = dict((item[1], Mode.WorkMode(item[0])) for item in FANSPEED_MODES_MAP.items())

    HEAT_MODES_MAP = {
        Mode.WorkMode.Auto.value: '0',
        Mode.WorkMode.Heat.value: '1',
        Mode.WorkMode.FanOnly.value: '2',
        Mode.WorkMode.Cool.value: '3',
        Mode.WorkMode.Dry.value: '4',
    }
    HEAT_MODES_REVMAP = dict((item[1], Mode.WorkMode(item[0])) for item in HEAT_MODES_MAP.items())

    LOUVRE_MODES_MAP = {
        Mode.WorkMode.Stationary.value: '0',
        Mode.WorkMode.Vertical.value: '1',
    }
    LOUVRE_MODES_REVMAP = dict((item[1], Mode.WorkMode(item[0])) for item in LOUVRE_MODES_MAP.items())

    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        device_path: str,
        description: typing.Optional[str] = None,
        room=None,
    ):
        self.client = mqtt_client
        self.onoff_status_path = f'{device_path}/OnOff'
        self.onoff_control_path = f'{device_path}/OnOff/on'
        self.mode_status_path = f'{device_path}/Mode'
        self.mode_control_path = f'{device_path}/Mode/on'
        self.setpoint_status_path = f'{device_path}/Setpoint'
        self.setpoint_control_path = f'{device_path}/Setpoint/on'
        self.fanspeed_status_path = f'{device_path}/Fanspeed'
        self.fanspeed_control_path = f'{device_path}/Fanspeed/on'
        self.louvre_status_path = f'{device_path}/Louvre'
        self.louvre_control_path = f'{device_path}/Louvre/on'
        self.temperature_path = f'{device_path}/Return Air Temperature'

        self.onoff = OnOff(
            change_value=self.change_onoff,
            retrievable=True,
            reportable=True,
        )

        self.setpoint = Range(
            instance=Range.Instance.Temperature,
            unit=Range.Unit.TemperatureCelsius,
            min_value=18.,
            max_value=32.,
            precision=1.,
            retrievable=True,
            reportable=True,
            change_value=self.change_setpoint,
        )

        self.fanspeed = Mode(
            instance=Mode.Instance.FanSpeed,
            modes=[Mode.WorkMode.Auto, Mode.WorkMode.One, Mode.WorkMode.Two, Mode.WorkMode.Three, Mode.WorkMode.Four, Mode.WorkMode.Five],
            change_value=self.change_fanspeed,
            retrievable=True,
            reportable=True,
        )

        self.mode = Mode(
            instance=Mode.Instance.Thermostat,
            modes=[
                Mode.WorkMode.Auto,
                Mode.WorkMode.Heat,
                Mode.WorkMode.FanOnly,
                Mode.WorkMode.Cool,
                Mode.WorkMode.Dry,
            ],
            change_value=self.change_mode,
            retrievable=True,
            reportable=True,
        )

        self.louvre = Mode(
            instance=Mode.Instance.Swing,
            modes=[
                Mode.WorkMode.Stationary,
                Mode.WorkMode.Vertical,
            ],
            change_value=self.change_louvre,
            retrievable=True,
            reportable=True,
        )

        self.temperature = Temperature(unit=Temperature.Unit.Celsius, reportable=True)

        self.client.subscribe(self.onoff_status_path, self.on_onoff_changed)
        self.client.subscribe(self.setpoint_status_path, self.on_setpoint_changed)
        self.client.subscribe(self.mode_status_path, self.on_mode_changed)
        self.client.subscribe(self.louvre_status_path, self.on_louvre_changed)
        self.client.subscribe(self.fanspeed_status_path, self.on_fanspeed_changed)
        self.client.subscribe(self.temperature_path, self.on_temperature_changed)

        super().__init__(
            device_id=device_id,
            capabilities=[self.onoff, self.setpoint, self.fanspeed, self.mode, self.louvre],
            properties=[self.temperature],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    async def change_onoff(
        self,
        capability: OnOff,
        instance: str,
        value: bool,
        /,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        self.client.send(self.onoff_control_path, str(int(value)))
        return (capability.type_id, instance)

    async def change_setpoint(
        self,
        capability: Range,
        instance: str,
        value: float,
        /,
        relative: bool = False,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        if relative:
            if self.setpoint.value is None:
                raise ActionException(capability.type_id, instance, ActionError.DeviceBusy)
            value += self.setpoint.value
        self.client.send(self.setpoint_control_path, str(round(value, 1)))
        return (capability.type_id, instance)

    async def change_mode(
        self,
        capability: Mode,
        instance: str,
        value: str,
        /,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        # FIXME make enum-typed capabilities to pass it as enum, not raw string

        if value not in self.HEAT_MODES_MAP:
            raise ActionException(capability.type_id, instance, ActionError.InvalidValue)

        self.client.send(self.mode_control_path, self.HEAT_MODES_MAP[value])
        return (capability.type_id, instance)

    async def change_fanspeed(
        self,
        capability: Mode,
        instance: str,
        value: str,
        /,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        # FIXME make enum-typed capabilities to pass it as enum, not raw string

        if value not in self.FANSPEED_MODES_MAP:
            raise ActionException(capability.type_id, instance, ActionError.InvalidValue)

        self.client.send(self.fanspeed_control_path, self.FANSPEED_MODES_MAP[value])
        return (capability.type_id, instance)

    async def change_louvre(
        self,
        capability: Mode,
        instance: str,
        value: str,
        /,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        # FIXME make enum-typed capabilities to pass it as enum, not raw string

        if value not in self.LOUVRE_MODES_MAP:
            raise ActionException(capability.type_id, instance, ActionError.InvalidValue)

        self.client.send(self.louvre_control_path, self.LOUVRE_MODES_MAP[value])
        return (capability.type_id, instance)

    async def on_onoff_changed(self, topic: str, payload: str) -> None:
        self.onoff.value = payload == "1"

    async def on_setpoint_changed(self, topic: str, payload: str) -> None:
        self.setpoint.value = float(payload)

    async def on_mode_changed(self, topic: str, payload: str) -> None:
        value = self.HEAT_MODES_REVMAP.get(payload)
        if value is not None:
            self.mode.value = value

    async def on_fanspeed_changed(self, topic: str, payload: str) -> None:
        value = self.FANSPEED_MODES_REVMAP.get(payload)
        if value is not None:
            self.fanspeed.value = value

    async def on_louvre_changed(self, topic: str, payload: str) -> None:
        value = self.LOUVRE_MODES_REVMAP.get(payload)
        if value is not None:
            self.louvre.value = value

    async def on_temperature_changed(self, topic: str, payload: str) -> None:
        self.temperature.assign(float(payload))
