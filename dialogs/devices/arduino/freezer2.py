"""
Simple autonomous device, that is attached to my fridge and measures its
parameters: temperature, humidity, and if the cooler is enabled. Freezer
pushes all the metrics into influxdb, and we read them from there.

This implementation uses newer Dialogs API, where support for sensors is
available. For older implmenetation see freezer module.
"""

import typing
import logging
import asyncio

import aiohttp

from dialogs.protocol.device import Other
from dialogs.protocol.float_property import Humidity, Temperature, Power


class FreezerWatcher(Other):
    def __init__(
        self,
        device_id: str,
        name: str,
        description: typing.Optional[str] = None,
        room: typing.Optional[str] = None,
    ):
        self.temperature = Temperature(unit=Temperature.Unit.Celsius)
        self.humidity = Humidity()
        self.cooler = Power()  # no toggle-like sensors yet
        super().__init__(
            device_id=device_id,
            capabilities=[],
            properties=[self.temperature, self.humidity, self.cooler],
            device_name=name,
            description=description,
            room=room,
            manufacturer='torkve',
            model='FRDG2',
            hw_version='2.0',
            sw_version='7.0',
        )

    async def _fetch(self, client: aiohttp.ClientSession) -> dict:
        async with client.get("http://localhost:8086/query", params={
            "db": "freezer",
            "q": "select * from freezer order by time desc limit 1",
        }) as resp:
            data = await resp.json()
            data = data['results'][0]['series'][0]
            columns = data['columns']
            values = data['values'][0]
            result = dict(zip(columns, values))
            logging.getLogger('freezer').info("fetched %s", result)
            return result

    async def updater_loop(self) -> None:
        async with aiohttp.ClientSession() as client:
            while True:
                try:
                    response = await self._fetch(client)
                    if response['temperature_bme'] is not None:
                        self.temperature.assign(response['temperature_bme'])
                    if response['humidity_bme'] is not None:
                        self.humidity.assign(response['humidity_bme'])
                    if response['cooler'] is not None:
                        self.cooler.assign(response['cooler'])
                except Exception:
                    logging.getLogger('freezer').exception("fetch failed")

                await asyncio.sleep(10)
