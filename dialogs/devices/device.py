from .base import Device


_mapping = (
    ('devices.types.light', 'Light'),
    ('devices.types.socket', 'Socket'),
    ('devices.types.switch', 'Switch'),
    ('devices.types.thermostat', 'Thermostat'),
    ('devices.types.thermostat.ac', 'AirConditioner'),
    ('devices.types.media_device', 'MediaDevice'),
    ('devices.types.media_device.tv', 'TV'),
    ('devices.types.cooking', 'Cooking'),
    ('devices.types.cooking.coffee_maker', 'CoffeeMaker'),
    ('devices.types.cooking.kettle', 'Kettle'),
    ('devices.types.openable', 'Openable'),
    ('devices.types.openable.curtain', 'Curtain'),
    ('devices.types.humidifier', 'Humidifier'),
    ('devices.types.purifier', 'Purifier'),
    ('devices.types.vacuum_cleaner', 'VacuumCleaner'),
    ('devices.types.other', 'Other'),
)

__all__ = [item[1] for item in _mapping]

# FIXME mypy is not happy here
for type_id, typename in _mapping:
    globals()[typename] = type(typename, (Device,), {'type_id': type_id})
