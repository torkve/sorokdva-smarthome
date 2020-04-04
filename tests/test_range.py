import pytest

from dialogs.devices.device import Other
from dialogs.devices.capability import Range


pytestmark = pytest.mark.asyncio


@pytest.fixture(scope='function')
def retrievable_device() -> Other:
    result = Other(
        device_id='device1',
        device_name='Test',
        capabilities=[Range(instance=Range.Instance.Humidity,
                            unit=Range.Unit.Percent,
                            min_value=50.,
                            max_value=100.,
                            retrievable=True
                            )]
    )
    return result


async def test_description(retrievable_device: Other):
    spec = await retrievable_device.specification()
    expected = {
        'type': 'devices.types.other',
        'id': 'device1',
        'name': 'Test',
        'capabilities': [
            {
                'type': 'devices.capabilities.range',
                'retrievable': True,
                'parameters': {
                    'instance': 'humidity',
                    'random_access': True,
                    'range': {
                        'max': 100.,
                        'min': 50.,
                        'precision': 1.,
                    },
                    'unit': 'unit.percent',
                },
            },
        ],
        'properties': [],
    }
    assert spec == expected


async def test_state(retrievable_device: Other):
    next(iter(retrievable_device.capabilities())).value = 60.

    state = await retrievable_device.state()
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.range',
                'state': {
                    'instance': 'humidity',
                    'value': 60.,
                },
            }
        ],
        'properties': [],
    }
    assert state == expected


async def test_action(retrievable_device: Other):
    result = await retrievable_device.action([{
        'type': 'devices.capabilities.range',
        'state': {
            'instance': 'unknown',
            'value': 'americano',
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.range',
                'state': {
                    'instance': 'unknown',
                    'action_result': {
                        'status': 'ERROR',
                        'error_code': 'INVALID_ACTION',
                        'error_message': 'Unknown capability for this device',
                    }
                }
            }
        ]
    }
    assert result == expected

    result = await retrievable_device.action([{
        'type': 'devices.capabilities.range',
        'state': {
            'instance': 'humidity',
            'value': 70.,
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.range',
                'state': {
                    'instance': 'humidity',
                    'action_result': {
                        'status': 'ERROR',
                        'error_code': 'NOT_SUPPORTED_IN_CURRENT_MODE',
                        'error_message': 'NOT_SUPPORTED_IN_CURRENT_MODE',
                    }
                }
            }
        ]
    }
    assert result == expected

    async def change_value(device, capability, instance, value):
        capability.value = value
        return capability.type_id, instance

    next(iter(retrievable_device.capabilities())).change_value = change_value
    result = await retrievable_device.action([{
        'type': 'devices.capabilities.range',
        'state': {
            'instance': 'humidity',
            'value': 70.,
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.range',
                'state': {
                    'instance': 'humidity',
                    'action_result': {
                        'status': 'DONE',
                    }
                }
            }
        ]
    }
    assert result == expected
    assert next(iter(retrievable_device.capabilities())).value == 70.
