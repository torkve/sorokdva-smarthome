from .arduino.freezer import Freezer
from .arduino.freezer2 import FreezerWatcher
from .wirenboard.curtain import WbCurtain
from .wirenboard.sensor import WbSensor
from .wirenboard.rtd_ra import WbRtdRa


device_classes = {
    klass.__name__: klass
    for klass in (
        Freezer,
        FreezerWatcher,
        WbCurtain,
        WbSensor,
        WbRtdRa,
    )
}
