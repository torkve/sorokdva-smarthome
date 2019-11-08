import typing
import asyncio
import aiohttp


class Freezer:
    def __init__(self, device_id: str, name: str):
        self.device_id: str = device_id
        self.name: str = name
        self.temperature: typing.Optional[float] = None
        self.humidity: typing.Optional[float] = None
        self.cooler_active: typing.Optional[bool] = None

    async def features(self) -> dict:
        return {
            'id': self.device_id,
            'name': self.name,
            'type': 'devices.types.other',
            'capabilities': [
                {
                    'type': 'devices.capabilities.range',
                    'retrievable': True,
                    'parameters': {
                        'instance': 'temperature',
                        'unit': 'unit.temperature.celsius',
                        'range': {
                            'min': -100,
                            'max': 100,
                            'precision': 1,
                        },
                    },
                },
                {
                    'type': 'devices.capabilities.range',
                    'retrievable': True,
                    'parameters': {
                        'instance': 'humidity',
                        'unit': 'unit.percent',
                        'range': {
                            'min': 0,
                            'max': 100,
                            'precision': 1,
                        },
                    },
                },
                {
                    'type': 'devices.capabilities.toggle',
                    'retrievable': True,
                    'parameters': {
                        'instance': 'oscillation',
                    }
                }
            ],
        }

    async def query(self) -> dict:
        result: dict = {
            'id': self.device_id,
        }

        if self.temperature is None or self.humidity is None or self.cooler_active is None:
            result['error_code'] = 'DEVICE_BUSY'
            result['error_message'] = 'Холодильник отдыхает, подождите, пожалуйста'
            return result

        result['capabilities'] = [
            {
                'type': 'devices.capabilities.range',
                'state': {
                    'instance': 'temperature',
                    'value': self.temperature,
                }
            },
            {
                'type': 'devices.capabilities.range',
                'state': {
                    'instance': 'humidity',
                    'value': self.humidity,
                }
            },
            {
                'type': 'device.capabilities.toggle',
                'state': {
                    'instance': 'oscillation',
                    'value': self.cooler_active,
                }
            }
        ]
        return result

    async def _fetch(self, client: aiohttp.ClientSession) -> dict:
        async with client.get("http://localhost:8086/query", params={
            "db": "freezer",
            "q": "select * from freezer order by time desc limit 1",
        }) as resp:
            data = await resp.json()
            data = data['results'][0]['series']
            columns = data['columns']
            values = data['values']['0']
            return dict(zip(columns, values))

    async def updater(self) -> None:
        async with aiohttp.ClientSession() as client:
            while True:
                try:
                    response = await self._fetch(client)
                    if response['temperature_bme'] is not None:
                        self.temperature = response['temperature_bme']
                    if response['humidity_bme'] is not None:
                        self.humidity = response['humidity_bme']
                    if response['cooler'] is not None:
                        self.cooler_active = bool(response['cooler'])
                except Exception:
                    # FIXME
                    pass

                await asyncio.sleep(10)
