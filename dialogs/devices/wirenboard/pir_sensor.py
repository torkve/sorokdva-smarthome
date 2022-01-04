"""
Implementation of the PIR sensor, connected to some GPIO input.
"""

import typing

from dialogs.protocol.device import Sensor
from dialogs.protocol.event_property import Motion
from dialogs.mqtt_client import MqttClient


class WbPirSensor(Sensor):
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
        self.motion = Motion(reportable=True)
        self.status_path = status_path

        self.client.subscribe(self.status_path, self.on_motion_changed)

        super().__init__(
            device_id=device_id,
            capabilities=[],
            properties=[self.motion],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    async def on_motion_changed(self, topic: str, payload: str) -> None:
        value = Motion.Value.Detected if payload == '0' else Motion.Value.NotDetected
        self.motion.assign(value)
