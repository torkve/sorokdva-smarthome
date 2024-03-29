"""
Dimmable light. It has no on-off switch, and its range depends on dim-contoller.
"""

import typing
import logging

from dialogs.mqtt_client import MqttClient

from dialogs.protocol.consts import ActionError
from dialogs.protocol.exceptions import ActionException
from dialogs.protocol.device import Light
from dialogs.protocol.capability import Range, OnOff


class WbDimmableLight(Light):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        status_path: str,
        control_path: str,
        range_off: int,
        range_low: int,
        range_high: int,
        description: typing.Optional[str] = None,
        room=None,
    ):
        self.client = mqtt_client
        self.onoff = OnOff(
            change_value=self.change_onoff,
            retrievable=True,
            reportable=True,
        )
        self.level = Range(
            change_value=self.change_level,
            retrievable=True,
            reportable=True,
            instance=Range.Instance.Brightness,
            unit=Range.Unit.Percent,
            min_value=0.,
            max_value=100.,
            precision=1. if (range_high - range_low) < 500 else 0.1,
        )

        self.last_val = 100.

        self.range_off = range_off
        self.range_low = range_low
        self.range_high = range_high
        self.status_path = status_path
        self.control_path = control_path
        self.client.subscribe(self.status_path, self.on_level_changed)

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
        self.onoff.value = percent_value > 0
        if percent_value > 0:
            self.last_val = percent_value

    def _get_level_value(self, level: float) -> int:
        real_value = int(level / 100 * (self.range_high - self.range_low) + self.range_low)
        if real_value <= self.range_low:
            # this fixes on/off button logic: when dimmer has some off range
            # (e.g. 0..200 from total 0..1000) and you set range_low (200),
            # the button will consider device as on and thus flip state between 0 and 200.
            return self.range_off

        return real_value

    async def change_level(
        self,
        capability: Range,
        instance: str,
        value: float,
        /,
        relative: bool = False,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        if relative:
            if self.level.value is None:
                raise ActionException(capability.type_id, instance, ActionError.DeviceBusy)
            value += self.level.value

        real_value = self._get_level_value(value)
        logging.getLogger('wb.dimlight').info("Switching light to %s (real value %s)", value, real_value)
        self.client.send(self.control_path, str(real_value))
        return (capability.type_id, instance)

    async def change_onoff(
        self,
        capability: OnOff,
        instance: str,
        value: bool,
        /,
        **kwargs,
    ):
        target = self.last_val if value else 0.
        real_value = self._get_level_value(target)
        logging.getLogger('wb.dimlight').info("Switching light to %s (real value %s)", target, real_value)
        self.client.send(self.control_path, str(real_value))
        return (capability.type_id, instance)
