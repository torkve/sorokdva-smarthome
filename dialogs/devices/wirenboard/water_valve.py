"""
Implementation of the water valve with motor connected to the WB-MWAC Wirenboard extension.
"""

import typing
import logging

from dialogs.protocol.device import Switch
from dialogs.protocol.capability import OnOff
from dialogs.mqtt_client import MqttClient


class WbWaterValve(Switch):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        status_path: str,
        control_path: str,
        description: typing.Optional[str] = None,
        room: typing.Optional[str] = None,
    ):
        self.client = mqtt_client
        self.onoff = OnOff(
            change_value=self.change_onoff,
            retrievable=True,
        )

        self.status_path = status_path
        self.control_path = control_path
        self.client.subscribe(self.status_path, self.on_onoff_changed)

        super().__init__(
            device_id=device_id,
            capabilities=[self.onoff],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    async def on_onoff_changed(self, topic: str, payload: str) -> None:
        self.onoff.value = not bool(int(payload))

    async def change_onoff(
        self,
        device: "WbWaterValve",
        capability: OnOff,
        instance: str,
        value: bool,
    ) -> typing.Tuple[str, str]:
        logging.getLogger('wb.water_valve').info("Switching water to %s", value)
        self.client.send(self.control_path, str(int(not value)))
        return (capability.type_id, instance)
