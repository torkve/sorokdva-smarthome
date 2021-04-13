"""
Implementation of the curtain, connected to the Wirenboard.
This curtain doesn't provide its current position,
but it reports the state of its two available controls:
    - motor (on/off)
    - direction (up/down)
"""

import typing
import asyncio
import logging

from dialogs.mqtt_client import MqttClient

from dialogs.protocol.device import Curtain
from dialogs.protocol.consts import ActionError
from dialogs.protocol.exceptions import ActionException
from dialogs.protocol.capability import Toggle, Mode, OnOff, Range


class WbCurtain(Curtain):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        direction_status_path: str,
        motor_status_path: str,
        direction_control_path: str,
        motor_control_path: str,
        action_time_seconds: int,
        description: typing.Optional[str] = None,
        room=None,
    ):
        self.client = mqtt_client
        self.updown = OnOff(
            change_value=self.change_updown,
            retrievable=False,
            split=True,
        )
        self.direction = Mode(
            instance=Mode.Instance.Swing,
            modes=[Mode.WorkMode.High, Mode.WorkMode.Low],
            retrievable=True,
            change_value=self.change_direction,
        )
        self.motor = Toggle(
            instance=Toggle.Instance.Oscillation,
            retrievable=True,
            change_value=self.change_motor,
        )
        self.partial_open = Range(
            instance=Range.Instance.Open,
            unit=Range.Unit.Percent,
            random_access=False,
            retrievable=False,
            min_value=0.,
            max_value=100.,
            precision=5.,
            change_value=self.change_partial_open,
        )
        self.direction_status_path = direction_status_path
        self.motor_status_path = motor_status_path
        self.direction_control_path = direction_control_path
        self.motor_control_path = motor_control_path
        self.action_times_seconds = action_time_seconds
        self.client.subscribe(self.direction_status_path, self.on_direction_changed)
        self.client.subscribe(self.motor_status_path, self.on_motor_changed)
        self.task: typing.Optional[asyncio.Task] = None
        super().__init__(
            device_id=device_id,
            capabilities=[self.updown, self.partial_open, self.direction, self.motor],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    def cancel_task(self):
        if self.task is not None:
            self.task.cancel()
            self.task = None

    async def on_direction_changed(self, topic: str, payload: str) -> None:
        value = int(payload)
        if value:
            self.direction.value = Mode.WorkMode.High
        else:
            self.direction.value = Mode.WorkMode.Low

    async def on_motor_changed(self, topic: str, payload: str) -> None:
        self.motor.value = payload == "1"

    async def change_updown(
        self,
        device: "WbCurtain",
        capability: OnOff,
        instance: str,
        value: bool,
    ) -> typing.Tuple[str, str]:
        async def task():
            self.client.send(self.motor_control_path, "0")
            self.client.send(self.direction_control_path, str(int(value)))
            self.client.send(self.motor_control_path, "1")
            await asyncio.sleep(self.action_times_seconds)
            self.client.send(self.motor_control_path, "0")

        self.cancel_task()
        logging.getLogger('wb').info("Switching curtain to %s", value)
        self.task = asyncio.create_task(task())
        return (capability.type_id, instance)

    async def change_partial_open(
        self,
        device: "WbCurtain",
        capability: Range,
        instance: str,
        value: float,
    ) -> typing.Tuple[str, str]:
        async def task():
            self.client.send(self.motor_control_path, "0")
            self.client.send(self.direction_control_path, '1' if value < 0 else '0')
            self.client.send(self.motor_control_path, "1")
            await asyncio.sleep(2)
            self.client.send(self.motor_control_path, "0")

        self.cancel_task()
        logging.getLogger('wb.curtain').info("Shifting curtain to %s", value)
        self.task = asyncio.create_task(task())
        return (capability.type_id, instance)

    async def change_direction(
        self,
        device: "WbCurtain",
        capability: Mode,
        instance: str,
        value: str,
    ) -> typing.Tuple[str, str]:
        # FIXME make enum-typed capabilities to pass it as enum, not raw string
        if value not in (Mode.WorkMode.High.value, Mode.WorkMode.Low.value):
            raise ActionException(capability.type_id, instance, ActionError.InvalidValue)

        value = "1" if value == Mode.WorkMode.High.value else "0"
        self.client.send(self.direction_control_path, value)
        return (capability.type_id, instance)

    async def change_motor(
        self,
        device: "WbCurtain",
        capability: Toggle,
        instance: str,
        value: bool,
    ) -> typing.Tuple[str, str]:
        self.client.send(self.motor_control_path, str(int(value)))
        return (capability.type_id, instance)
