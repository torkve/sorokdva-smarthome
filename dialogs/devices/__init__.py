from .arduino.freezer import Freezer
from .arduino.freezer2 import FreezerWatcher
from .wirenboard.curtain import WbCurtain
from .wirenboard.sensor import WbSensor
from .wirenboard.rtd_ra import WbRtdRa
from .wirenboard.cooler import WbCooler
from .wirenboard.light import WbLight
from .wirenboard.dimmable_light import WbDimmableLight
from .wirenboard.dimmable_onoff_light import WbDimmableOnoffLight
from .wirenboard.mixwhite_light import WbMixwhiteLight


device_classes = {
    klass.__name__: klass
    for klass in (
        Freezer,
        FreezerWatcher,
        WbCurtain,
        WbSensor,
        WbRtdRa,
        WbCooler,
        WbLight,
        WbDimmableLight,
        WbDimmableOnoffLight,
        WbMixwhiteLight,
    )
}
