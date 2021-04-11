from dialogs.protocol.base import Device


class Light(Device):
    type_id = 'devices.types.light'


class Socket(Device):
    type_id = 'devices.types.socket'


class Switch(Device):
    type_id = 'devices.types.switch'


class Thermostat(Device):
    type_id = 'devices.types.thermostat'


class AirConditioner(Device):
    type_id = 'devices.types.thermostat.ac'


class MediaDevice(Device):
    type_id = 'devices.types.media_device'


class TV(Device):
    type_id = 'devices.types.media_device.tv'


class TVBox(Device):
    type_id = 'devices.types.media_device.tv_box'


class Receiver(Device):
    type_id = 'devices.types.media_device.receiver'


class Cooking(Device):
    type_id = 'devices.types.cooking'


class CoffeeMaker(Device):
    type_id = 'devices.types.cooking.coffee_maker'


class Kettle(Device):
    type_id = 'devices.types.cooking.kettle'


class Multicooker(Device):
    type_id = 'devices.types.cooking.multicooker'


class Openable(Device):
    type_id = 'devices.types.openable'


class Curtain(Device):
    type_id = 'devices.types.openable.curtain'


class Humidifier(Device):
    type_id = 'devices.types.humidifier'


class Purifier(Device):
    type_id = 'devices.types.purifier'


class VacuumCleaner(Device):
    type_id = 'devices.types.vacuum_cleaner'


class WashingMachine(Device):
    type_id = 'devices.types.washing_machine'


class Dishwasher(Device):
    type_id = 'devices.types.dishwasher'


class Iron(Device):
    type_id = 'devices.types.iron'


class Sensor(Device):
    type_id = 'devices.types.sensor'


class Other(Device):
    type_id = 'devices.types.other'
