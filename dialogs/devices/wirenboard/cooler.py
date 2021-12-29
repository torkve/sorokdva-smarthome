"""
Simple on-off switch for ceiling fan, connected to the Wirenboard.
"""

import typing
import logging

from dialogs.mqtt_client import MqttClient

from dialogs.protocol.device import Switch
from dialogs.protocol.capability import OnOff


class WbCooler(Switch):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        status_path: str,
        control_path: str,
        description: typing.Optional[str] = None,
        room=None,
    ):
        self.client = mqtt_client
        self.onoff = OnOff(
            change_value=self.change_onoff,
            retrievable=True,
            reportable=True,
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
        self.onoff.value = bool(int(payload))

    async def change_onoff(
        self,
        capability: OnOff,
        instance: str,
        value: bool,
        /,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        logging.getLogger('wb.cooler').info("Switching cooler to %s", value)
        self.client.send(self.control_path, str(int(value)))
        return (capability.type_id, instance)
