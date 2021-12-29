"""
Implementation of the sensor WB-MSW v.3 connected to Wirenboard.
"""

import typing

from dialogs.protocol.base import Property
from dialogs.protocol.device import Sensor
from dialogs.protocol.float_property import Humidity, Temperature
from dialogs.mqtt_client import MqttClient


class WbSensor(Sensor):
    def __init__(
        self,
        mqtt_client: MqttClient,
        device_id: str,
        name: str,
        description: typing.Optional[str] = None,
        room: typing.Optional[str] = None,
        temperature_path: typing.Optional[str] = None,
        humidity_path: typing.Optional[str] = None,
        sound_level_path: typing.Optional[str] = None,
        illuminance_path: typing.Optional[str] = None,
    ):
        assert (
            temperature_path
            or humidity_path
            or sound_level_path
            or illuminance_path
        ), "At least one property path must be specified"

        self.client = mqtt_client
        self.temperature = None
        self.humidity = None
        self.sound_level = None
        self.illuminance = None

        properties: typing.List[Property] = []

        if temperature_path is not None:
            self.temperature = Temperature(unit=Temperature.Unit.Celsius, reportable=True)
            self.client.subscribe(temperature_path, self.on_temperature_changed)
            properties.append(self.temperature)

        if humidity_path is not None:
            self.humidity = Humidity(reportable=True)
            self.client.subscribe(humidity_path, self.on_humidity_changed)
            properties.append(self.humidity)

        # sound_level and illuminance are currently not supported by Yandex

        super().__init__(
            device_id=device_id,
            capabilities=[],
            properties=properties,
            device_name=name,
            description=description,
            room=room,
            manufacturer='Wirenboard',
            model='WB-MSW v.3',
        )

    async def on_temperature_changed(self, topic: str, payload: str) -> None:
        assert self.temperature is not None
        self.temperature.assign(float(payload))

    async def on_humidity_changed(self, topic: str, payload: str) -> None:
        assert self.humidity is not None
        self.humidity.assign(float(payload))
