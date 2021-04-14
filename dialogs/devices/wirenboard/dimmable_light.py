"""
Dimmable light. It has no on-off switch, and its range depends on dim-contoller.
"""

import typing
import logging

from dialogs.mqtt_client import MqttClient

from dialogs.protocol.device import Light
from dialogs.protocol.capability import Range


class WbDimmableLight(Light):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        status_path: str,
        control_path: str,
        range_low: int,
        range_high: int,
        description: typing.Optional[str] = None,
        room=None,
    ):
        self.client = mqtt_client
        self.level = Range(
            change_value=self.change_level,
            retrievable=True,
            instance=Range.Instance.Brightness,
            unit=Range.Unit.Percent,
            min_value=0.,
            max_value=100.,
            precision=1. if range_high - range_low < 500 else 0.1,
        )

        self.range_low = range_low
        self.range_high = range_high
        self.status_path = status_path
        self.control_path = control_path
        self.client.subscribe(self.status_path, self.on_level_changed)

        super().__init__(
            device_id=device_id,
            capabilities=[self.level],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    async def on_level_changed(self, topic: str, payload: str) -> None:
        self.level.value = (int(payload) - self.range_low) / (self.range_high - self.range_low)

    async def change_level(
        self,
        device: "WbDimmableLight",
        capability: Range,
        instance: str,
        value: float,
    ) -> typing.Tuple[str, str]:
        real_value = str(int(value * (self.range_high - self.range_low) + self.range_low))
        logging.getLogger('wb.cooler').info("Switching light to %s (real value %s)", value, real_value)
        self.client.send(self.control_path, real_value)
        return (capability.type_id, instance)
