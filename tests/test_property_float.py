import pytest

from dialogs.protocol.device import Other
from dialogs.protocol.property import Amperage, CO2Level, Humidity, Power, Temperature, Voltage, WaterLevel


pytestmark = pytest.mark.asyncio


@pytest.fixture(scope='function')
def device() -> Other:
    return Other(
        device_id='device1',
        device_name='Test',
        capabilities=[],
        properties=[
            Amperage(initial_value=10.),
            CO2Level(initial_value=800.),
            Humidity(initial_value=85.),
            Power(initial_value=0.),
            Temperature(unit=Temperature.Unit.Celsius, initial_value=42),
            Voltage(initial_value=12.),
            WaterLevel(initial_value=100),
        ]
    )


async def test_description(device):
    spec = await device.specification()
    expected = [
        {
            'type': 'devices.properties.float',
            'retrievable': True,
            'parameters': {
                'instance': 'amperage',
                'unit': 'unit.ampere',
            }
        },
        {
            'type': 'devices.properties.float',
            'retrievable': True,
            'parameters': {
                'instance': 'co2_level',
                'unit': 'unit.ppm',
            }
        },
        {
            'type': 'devices.properties.float',
            'retrievable': True,
            'parameters': {
                'instance': 'humidity',
                'unit': 'unit.percent',
            }
        },
        {
            'type': 'devices.properties.float',
            'retrievable': True,
            'parameters': {
                'instance': 'power',
                'unit': 'unit.watt',
            }
        },
        {
            'type': 'devices.properties.float',
            'retrievable': True,
            'parameters': {
                'instance': 'temperature',
                'unit': 'unit.temperature.celsius',
            }
        },
        {
            'type': 'devices.properties.float',
            'retrievable': True,
            'parameters': {
                'instance': 'voltage',
                'unit': 'unit.volt',
            }
        },
        {
            'type': 'devices.properties.float',
            'retrievable': True,
            'parameters': {
                'instance': 'water_level',
                'unit': 'unit.percent',
            }
        },
    ]
    assert len(spec['properties']) == len(expected)
    for item in expected:
        assert item in spec['properties']


async def test_state(device):
    state = await device.state()
    expected = [
        {
            'type': 'devices.properties.float',
            'state': {
                'instance': 'amperage',
                'value': 10.,
            }
        },
        {
            'type': 'devices.properties.float',
            'state': {
                'instance': 'co2_level',
                'value': 800.,
            }
        },
        {
            'type': 'devices.properties.float',
            'state': {
                'instance': 'humidity',
                'value': 85.,
            }
        },
        {
            'type': 'devices.properties.float',
            'state': {
                'instance': 'power',
                'value': 0.,
            }
        },
        {
            'type': 'devices.properties.float',
            'state': {
                'instance': 'temperature',
                'value': 42,
            }
        },
        {
            'type': 'devices.properties.float',
            'state': {
                'instance': 'voltage',
                'value': 12.,
            }
        },
        {
            'type': 'devices.properties.float',
            'state': {
                'instance': 'water_level',
                'value': 100,
            }
        },
    ]
    assert len(state['properties']) == len(expected)
    for item in expected:
        assert item in state['properties']
