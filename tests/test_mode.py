import pytest

from dialogs.protocol.device import Other
from dialogs.protocol.capability import Mode


pytestmark = pytest.mark.asyncio


@pytest.fixture(scope='function')
def retrievable_device() -> Other:
    result = Other(
        device_id='device1',
        device_name='Test',
        capabilities=[Mode(instance=Mode.Instance.CleanupMode,
                           modes=(Mode.WorkMode.Americano, Mode.WorkMode.FanOnly),
                           initial_value=Mode.WorkMode.Americano,
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
                'type': 'devices.capabilities.mode',
                'retrievable': True,
                'parameters': {
                    'instance': 'cleanup_mode',
                    'modes': [
                        {'value': 'americano'},
                        {'value': 'fan_only'},
                    ],
                },
            },
        ],
        'properties': [],
    }
    assert spec == expected


async def test_state(retrievable_device: Other):
    next(iter(retrievable_device.capabilities())).value = 'fan_only'

    state = await retrievable_device.state()
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.mode',
                'state': {
                    'instance': 'cleanup_mode',
                    'value': 'fan_only',
                },
            }
        ],
        'properties': [],
    }
    assert state == expected


async def test_action(retrievable_device: Other):
    result = await retrievable_device.action([{
        'type': 'devices.capabilities.mode',
        'state': {
            'instance': 'unknown',
            'value': 'americano',
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.mode',
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
        'type': 'devices.capabilities.mode',
        'state': {
            'instance': 'cleanup_mode',
            'value': 'americano',
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.mode',
                'state': {
                    'instance': 'cleanup_mode',
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

    async def change_value(capability, instance, value):
        capability.value = value
        return capability.type_id, instance

    next(iter(retrievable_device.capabilities())).change_value = change_value
    result = await retrievable_device.action([{
        'type': 'devices.capabilities.mode',
        'state': {
            'instance': 'cleanup_mode',
            'value': 'fan_only',
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.mode',
                'state': {
                    'instance': 'cleanup_mode',
                    'action_result': {
                        'status': 'DONE',
                    }
                }
            }
        ]
    }
    assert result == expected
    assert next(iter(retrievable_device.capabilities())).value == Mode.WorkMode.FanOnly.value
