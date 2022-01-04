from .arduino.freezer import Freezer
from .arduino.freezer2 import FreezerWatcher
from .wirenboard.curtain import WbCurtain
from .wirenboard.sensor import WbSensor
from .wirenboard.rtd_ra import WbRtdRa
from .wirenboard.cooler import WbCooler
from .wirenboard.socket import WbSocket
from .wirenboard.light import WbLight
from .wirenboard.dimmable_light import WbDimmableLight
from .wirenboard.dimmable_onoff_light import WbDimmableOnoffLight
from .wirenboard.mixwhite_light import WbMixwhiteLight
from .wirenboard.water_valve import WbWaterValve
from .wirenboard.leak_sensor import WbLeakSensor
from .wirenboard.pir_sensor import WbPirSensor


device_classes = {
    klass.__name__: klass
    for klass in (
        Freezer,
        FreezerWatcher,
        WbCurtain,
        WbSensor,
        WbRtdRa,
        WbCooler,
        WbSocket,
        WbLight,
        WbDimmableLight,
        WbDimmableOnoffLight,
        WbMixwhiteLight,
        WbWaterValve,
        WbLeakSensor,
        WbPirSensor,
    )
}
