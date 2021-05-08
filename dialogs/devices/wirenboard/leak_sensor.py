"""
Implementation of the water leak sensor, connected to WB-MWAC Wirenboard extension.
"""

import typing

from dialogs.protocol.device import Sensor
from dialogs.protocol.event_property import WaterLeak
from dialogs.mqtt_client import MqttClient


class WbLeakSensor(Sensor):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        status_path: str,
        description: typing.Optional[str] = None,
        room: typing.Optional[str] = None,
    ):
        self.client = mqtt_client
        self.leak = WaterLeak()

        self.client.subscribe(self.status_path, self.on_leak_changed)

        super().__init__(
            device_id=device_id,
            capabilities=[],
            properties=[self.leak],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    async def on_leak_changed(self, topic: str, payload: str) -> None:
        value = WaterLeak.Value.Leak if payload == '1' else WaterLeak.Value.Dry
        self.leak.assign(value)
