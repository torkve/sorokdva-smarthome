import pytest

from dialogs.protocol.device import Other
from dialogs.protocol.capability import ColorSetting


pytestmark = pytest.mark.asyncio


@pytest.fixture
def device() -> Other:
    result = Other(
        device_id='device1',
        device_name='Test',
        capabilities=[ColorSetting(
            color_model=ColorSetting.HSV(h=1, s=2, v=3),
            temperature=ColorSetting.Temperature(),
            retrievable=True,
        )],
    )

    return result


@pytest.fixture
def no_temperature_device() -> Other:
    result = Other(
        device_id='device2',
        device_name='Test',
        capabilities=[ColorSetting(
            color_model=ColorSetting.HSV(h=1, s=2, v=3),
            retrievable=True,
        )],
    )

    return result


@pytest.fixture
def no_color_device() -> Other:
    result = Other(
        device_id='device3',
        device_name='Test',
        capabilities=[ColorSetting(
            temperature=ColorSetting.Temperature(value=5000),
            retrievable=True,
        )],
    )

    return result


async def test_description(device: Other, no_temperature_device: Other, no_color_device: Other):
    spec = await device.specification()
    expected = {
        'type': 'devices.types.other',
        'id': 'device1',
        'name': 'Test',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'reportable': False,
                'retrievable': True,
                'parameters': {
                    'color_model': 'hsv',
                    'temperature_k': {},
                }
            },
        ],
        'properties': [],
    }
    assert spec == expected

    spec = await no_temperature_device.specification()
    expected = {
        'type': 'devices.types.other',
        'id': 'device2',
        'name': 'Test',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'reportable': False,
                'retrievable': True,
                'parameters': {
                    'color_model': 'hsv',
                }
            },
        ],
        'properties': [],
    }
    assert spec == expected

    spec = await no_color_device.specification()
    expected = {
        'type': 'devices.types.other',
        'id': 'device3',
        'name': 'Test',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'reportable': False,
                'retrievable': True,
                'parameters': {
                    'temperature_k': {},
                }
            },
        ],
        'properties': [],
    }
    assert spec == expected


async def test_state(device: Other, no_temperature_device: Other, no_color_device: Other):
    state = await device.state()
    expected: dict = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'state': {
                    'instance': 'hsv',
                    'value': {
                        'h': 1,
                        's': 2,
                        'v': 3,
                    },
                },
            },
        ],
        'properties': [],
    }
    assert state == expected

    state = await no_temperature_device.state()
    expected = {
        'id': 'device2',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'state': {
                    'instance': 'hsv',
                    'value': {
                        'h': 1,
                        's': 2,
                        'v': 3,
                    },
                },
            },
        ],
        'properties': [],
    }
    assert state == expected

    state = await no_color_device.state()
    expected = {
        'id': 'device3',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'state': {
                    'instance': 'temperature_k',
                    'value': 5000,
                },
            },
        ],
        'properties': [],
    }
    assert state == expected


async def test_action(device: Other):
    result = await device.action([{
        'type': 'devices.capabilities.color_setting',
        'state': {
            'instance': 'unknown',
            'value': False,
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'state': {
                    'instance': 'unknown',
                    'action_result': {
                        'status': 'ERROR',
                        'error_code': 'INVALID_ACTION',
                        'error_message': 'Unknown capability for this device',
                    }
                }
            }
        ],
    }
    assert result == expected

    result = await device.action([{
        'type': 'devices.capabilities.color_setting',
        'state': {
            'instance': 'temperature_k',
            'value': False,
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'state': {
                    'instance': 'temperature_k',
                    'action_result': {
                        'status': 'ERROR',
                        'error_code': 'NOT_SUPPORTED_IN_CURRENT_MODE',
                        'error_message': 'NOT_SUPPORTED_IN_CURRENT_MODE',
                    }
                }
            }
        ],
    }
    assert result == expected

    async def change_value(capability, instance, value):
        if capability.value is not None and instance == capability.value.name:
            capability.value.assign(value)
        elif instance not in capability.instances:
            raise TypeError(f"Instance type {instance} is not supported for capability")
        else:
            if instance == capability.temperature.name:
                newval = capability.temperature
            else:
                newval = capability.color_model
            newval.assign(value)
            capability.value = newval

        return capability.type_id, instance

    next(iter(device.capabilities())).change_value = change_value
    result = await device.action([{
        'type': 'devices.capabilities.color_setting',
        'state': {
            'instance': 'temperature_k',
            'value': 6000,
        }
    }], None)
    expected = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'state': {
                    'instance': 'temperature_k',
                    'action_result': {
                        'status': 'DONE',
                    }
                }
            }
        ]
    }
    assert result == expected

    expected_state = {
        'id': 'device1',
        'capabilities': [
            {
                'type': 'devices.capabilities.color_setting',
                'state': {
                    'instance': 'temperature_k',
                    'value': 6000,
                }
            }
        ],
        'properties': [],
    }

    assert await device.state() == expected_state
