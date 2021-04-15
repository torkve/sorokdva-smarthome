"""
Dimmable light with on-off switch, and its range depends on dim-contoller.
"""

import typing
import logging

from dialogs.mqtt_client import MqttClient

from dialogs.protocol.device import Light
from dialogs.protocol.capability import Range, OnOff


class WbDimmableOnoffLight(Light):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        brightness_status_path: str,
        brightness_control_path: str,
        onoff_status_path: str,
        onoff_control_path: str,
        range_low: int,
        range_high: int,
        description: typing.Optional[str] = None,
        room=None,
    ):
        self.client = mqtt_client
        self.onoff = OnOff(
            change_value=self.change_onoff,
            retrievable=True,
        )
        self.level = Range(
            change_value=self.change_level,
            retrievable=True,
            instance=Range.Instance.Brightness,
            unit=Range.Unit.Percent,
            min_value=0.,
            max_value=100.,
            precision=1. if (range_high - range_low) < 500 else 0.1,
        )

        self.range_low = range_low
        self.range_high = range_high
        self.brightness_status_path = brightness_status_path
        self.brightness_control_path = brightness_control_path
        self.onoff_status_path = onoff_status_path
        self.onoff_control_path = onoff_control_path
        self.client.subscribe(self.brightness_status_path, self.on_level_changed)
        self.client.subscribe(self.onoff_status_path, self.on_onoff_changed)

        super().__init__(
            device_id=device_id,
            capabilities=[self.onoff, self.level],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    async def on_level_changed(self, topic: str, payload: str) -> None:
        percent_value = max(0, (int(payload) - self.range_low) / (self.range_high - self.range_low) * 100.)
        self.level.value = percent_value

    async def on_onoff_changed(self, topic: str, payload: str) -> None:
        self.onoff.value = payload == '1'

    async def change_level(
        self,
        device: "WbDimmableOnoffLight",
        capability: Range,
        instance: str,
        value: float,
    ) -> typing.Tuple[str, str]:
        real_value = str(int(value / 100 * (self.range_high - self.range_low) + self.range_low))
        logging.getLogger('wb.dimlight').info("Switching light to %s (real value %s)", value, real_value)
        self.client.send(self.brightness_control_path, real_value)
        return (capability.type_id, instance)

    async def change_onoff(
        self,
        device: "WbDimmableOnoffLight",
        capability: OnOff,
        instance: str,
        value: bool,
    ):
        logging.getLogger('wb.dimlight').info("Switching light to %s", value)
        self.client.send(self.onoff_control_path, str(int(value)))
        return (capability.type_id, instance)
