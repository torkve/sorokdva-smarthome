#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionError {
    DeviceUnreachable,
    DeviceBusy,
    DeviceNotFound,
    InternalError,
    InvalidAction,
    InvalidValue,
    NotSupportedInCurrentMode,
}

impl ActionError {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionError::DeviceUnreachable => "DEVICE_UNREACHABLE",
            ActionError::DeviceBusy => "DEVICE_BUSY",
            ActionError::DeviceNotFound => "DEVICE_NOT_FOUND",
            ActionError::InternalError => "INTERNAL_ERROR",
            ActionError::InvalidAction => "INVALID_ACTION",
            ActionError::InvalidValue => "INVALID_VALUE",
            ActionError::NotSupportedInCurrentMode => "NOT_SUPPORTED_IN_CURRENT_MODE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionStatus {
    Done,
    Error,
}

impl ActionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionStatus::Done => "DONE",
            ActionStatus::Error => "ERROR",
        }
    }
}
