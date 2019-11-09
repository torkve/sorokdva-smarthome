import enum


class QueryError(enum.Enum):
    DeviceUnreachable = 'DEVICE_UNREACHABLE'
    DeviceBusy = 'DEVICE_BUSY'
    DeviceNotFound = 'DEVICE_NOT_FOUND'
    InternalError = 'INTERNAL_ERROR'


class ActionError(enum.Enum):
    DeviceUnreachable = 'DEVICE_UNREACHABLE'
    DeviceBusy = 'DEVICE_BUSY'
    DeviceNotFound = 'DEVICE_NOT_FOUND'
    InternalError = 'INTERNAL_ERROR'
    InvalidAction = 'INVALID_ACTION'
    InvalidValue = 'INVALID_VALUE'
    NotSupportedInCurrentMode = 'NOT_SUPPORTED_IN_CURRENT_MODE'


class ActionStatus(enum.Enum):
    Done = 'DONE'
    Error = 'ERROR'
