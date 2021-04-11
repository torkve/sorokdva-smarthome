import pytest

from dialogs.protocol.device import Other
from dialogs.protocol.capability import OnOff


pytestmark = pytest.mark.asyncio


@pytest.fixture(scope='function')
def device() -> Other:
    result = Other(
        device_id='device1',
        device_name='Test',
        capabilities=[OnOff(initial_value=False, retrievable=False)]
    )

    return result


@pytest.fixture(scope='function')
def retrievable_device() -> Other:
    result = Other(
        device_id='device2',
        device_name='Test',
        capabilities=[OnOff(initial_value=False, retrievable=True)]
    )

    return result


async def test_description(device: Other, retrievable_device: Other):
    spec = await device.specification()
    expected = {
        'type': 'devices.types.other',
        'id': 'device1',
        'name': 'Test',
        'capabilities': [
            {
                'type': 'devices.capabilities.on_off',
                'retrievable': False,
                'parameters': {
                    'split': False,
                },
            },
        ],
        'properties': [],
    }
    assert spec == expected

    spec = await retrievable_device.specification()
    expected = {
        'type': 'devices.types.other',
        'id': 'device2',
        'name': 'Test',
        'capabilities': [
            {
                'type': 'devices.capabilities.on_off',
                'retrievable': True,
                'parameters': {
                    'split': False,
                },
            },
        ],
        'properties': [],
    }
    assert spec == expected


async def test_state(device: Other, retrievable_device: Other):
    next(iter(device.capabilities())).value = True
    next(iter(retrievable_device.capabilities())).value = True

    state = await device.state()
    expected: dict = {
        'id': 'device1',
        'capabilities': [],
        'properties': [],
    }
    assert state == expected

    state = await retrievable_device.state()
    expected = {
        'id': 'device2',
        'capabilities': [
            {
                'type': 'devices.capabilities.on_off',
                'state': {
                    'instance': 'on',
                    'value': True,
                },
            }
        ],
        'properties': [],
    }
    assert state == expected


async def test_action(device: Other, retrievable_device: Other):
    result = await device.action([{
        'type': 'devices.capabilities.on_off',
        'state': {
            'instance': 'unknown',
            'value': True,
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.on_off',
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

    result = await device.action([{
        'type': 'devices.capabilities.on_off',
        'state': {
            'instance': 'on',
            'value': True,
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.on_off',
                'state': {
                    'instance': 'on',
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
        'type': 'devices.capabilities.on_off',
        'state': {
            'instance': 'on',
            'value': True,
        }
    }], None)
    expected = {
        'id': 'device2',
        'capabilities': [
            {
                'type': 'devices.capabilities.on_off',
                'state': {
                    'instance': 'on',
                    'action_result': {
                        'status': 'DONE',
                    }
                }
            }
        ]
    }
    assert result == expected
    assert next(iter(retrievable_device.capabilities())).value is True
