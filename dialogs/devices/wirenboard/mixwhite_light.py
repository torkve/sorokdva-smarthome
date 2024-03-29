"""
Dimmable light with adjustable color temperature.
It's pretty similar to WbDimmableLight, but it has two light
channels: warm and cold, that can be mixed, giving required
temperature.

There's rather complex and interesting theory behind mixing
two color sources (in short: it's not linear), so one must
make some calculations to get real color temperature equal
to the requested one.
The theory is explained here: https://dsp.stackexchange.com/a/62018

But:
    1. All manufacturers lie about their LEDs properties, so
       you'd have to measure each channel real characteristics
       if you want precise values.
    2. Deviation from linear appears to be not so significant
       for typical in-house usage, so we'll just drop it all
       and stick to classic linear T = p*Tw + q*Tc,
       where p + q = 1 (p, q —  ratios of warm and cold channel)
         and Tw, Tc — temperatures of warm and cold channels.
"""

import typing
import logging

from dialogs.mqtt_client import MqttClient

from dialogs.protocol.device import Light
from dialogs.protocol.capability import Range, OnOff, ColorSetting
from dialogs.protocol.exceptions import ActionException, ActionError


class WbMixwhiteLight(Light):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        warm_status_path: str,
        warm_control_path: str,
        cold_status_path: str,
        cold_control_path: str,
        warm_temperature: int,
        cold_temperature: int,
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
        self.last_brightness_val = 100.
        self.last_temperature_val = (
            4500
            if warm_temperature <= 4500 <= cold_temperature
            else (warm_temperature + cold_temperature) // 2
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
        self.temperature = ColorSetting(
            temperature=ColorSetting.Temperature(
                min=warm_temperature,
                max=cold_temperature,
                value=self.last_temperature_val,
            ),
            retrievable=True,
            reportable=True,
            change_value=self.change_temperature,
        )

        self.warm_value = range_low
        self.cold_value = range_low

        self.range_low = range_low
        self.range_high = range_high
        self.warm_status_path = warm_status_path
        self.warm_control_path = warm_control_path
        self.warm_temperature = warm_temperature
        self.cold_status_path = cold_status_path
        self.cold_control_path = cold_control_path
        self.cold_temperature = cold_temperature

        self.client.subscribe(self.warm_status_path, self.on_data_changed)
        self.client.subscribe(self.cold_status_path, self.on_data_changed)

        super().__init__(
            device_id=device_id,
            capabilities=[self.onoff, self.level, self.temperature],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='WB',
        )

    def value_to_ratio(self, val: int) -> float:
        return min(1., max(0., (val - self.range_low) / (self.range_high - self.range_low)))

    def value_from_ratio(self, val: float) -> int:
        return int(val * (self.range_high - self.range_low) + self.range_low)

    async def on_data_changed(self, topic: str, payload: str) -> None:
        value = self.temperature.value
        assert isinstance(value, ColorSetting.Temperature)

        if topic == self.warm_status_path:
            self.warm_value = int(payload)
        elif topic == self.cold_status_path:
            self.cold_value = int(payload)
        else:
            return

        warm_ratio = self.value_to_ratio(self.warm_value)
        cold_ratio = self.value_to_ratio(self.cold_value)
        percent_value = max(warm_ratio, cold_ratio) * 100.

        self.onoff.value = percent_value > 0

        if self.level.value is None or percent_value > 0:
            self.level.value = percent_value

        if percent_value > 0:
            temperature_value = int(
                (warm_ratio * self.warm_temperature + cold_ratio * self.cold_temperature)
                / (warm_ratio + cold_ratio)
            )
            # strange things occur sometimes
            temperature_value = min(max(temperature_value, self.warm_temperature), self.cold_temperature)
            self.temperature.assign(temperature_value)

            self.last_brightness_val = percent_value
            self.last_temperature_val = temperature_value

    def get_cold_and_warm_channels(self, temperature: int, brightness: float) -> typing.Tuple[int, int]:
        logging.getLogger('wb.mixwhiteight').info(
            "Calculating cold and warm channels for brightness %r and %d <= T %d <= %d",
            brightness,
            self.warm_temperature,
            temperature,
            self.cold_temperature,
        )
        if self.cold_temperature - temperature < temperature - self.warm_temperature:
            logging.getLogger('wb.mixwhiteight').info("%d is closer to cold temperature", temperature)
            cold_value = self.value_from_ratio(brightness)
            warm_ratio = (
                brightness
                * (self.cold_temperature - temperature)
                / (temperature - self.warm_temperature)
            )
            warm_value = self.value_from_ratio(warm_ratio)
        else:
            logging.getLogger('wb.mixwhiteight').info("%d is closer to warm temperature", temperature)
            warm_value = self.value_from_ratio(brightness)
            cold_ratio = (
                brightness
                * (temperature - self.warm_temperature)
                / (self.cold_temperature - temperature)
            )
            cold_value = self.value_from_ratio(cold_ratio)

        return cold_value, warm_value

    async def change_temperature(
        self,
        capability: ColorSetting,
        instance: str,
        value: typing.Union[int, dict],
        /,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        if not isinstance(value, int):
            raise ActionException(capability.type_id, instance, ActionError.InvalidValue)

        level_value = self.level.value
        if level_value is None:
            level_value = 100.

        cold_value, warm_value = self.get_cold_and_warm_channels(value, level_value / 100)
        logging.getLogger('wb.mixwhiteight').info(
            "Switching temperature to %s (cold %s, warm %s)",
            value, cold_value, warm_value,
        )
        self.client.send(self.cold_control_path, str(cold_value))
        self.client.send(self.warm_control_path, str(warm_value))
        return (capability.type_id, instance)

    async def change_level(
        self,
        capability: Range,
        instance: str,
        value: float,
        /,
        relative: bool = False,
        **kwargs,
    ) -> typing.Tuple[str, str]:
        tvalue = self.temperature.value
        assert isinstance(tvalue, ColorSetting.Temperature)

        if relative:
            if self.level.value is None:
                raise ActionException(capability.type_id, instance, ActionError.DeviceBusy)
            value += self.level.value

        cold_value, warm_value = self.get_cold_and_warm_channels(tvalue.serialize(), value / 100)
        logging.getLogger('wb.mixwhiteight').info(
            "Switching brightness to %s (cold %s, warm %s)",
            value, cold_value, warm_value,
        )
        self.client.send(self.cold_control_path, str(cold_value))
        self.client.send(self.warm_control_path, str(warm_value))
        return (capability.type_id, instance)

    async def change_onoff(
        self,
        capability: OnOff,
        instance: str,
        value: bool,
        /,
        **kwargs,
    ):
        if value:
            cold_value, warm_value = self.get_cold_and_warm_channels(
                self.last_temperature_val,
                self.last_brightness_val / 100,
            )
        else:
            cold_value, warm_value = 0, 0

        logging.getLogger('wb.mixwhitelight').info(
            "Switching light to %s (cold %s, warm %s)",
            value, cold_value, warm_value,
        )
        self.client.send(self.cold_control_path, str(cold_value))
        self.client.send(self.warm_control_path, str(warm_value))
        return (capability.type_id, instance)
