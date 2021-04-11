from .arduino.freezer import Freezer
from .arduino.freezer2 import FreezerWatcher


device_classes = {
    klass.__name__: klass
    for klass in (
        Freezer,
        FreezerWatcher,
    )
}
