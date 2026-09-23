//! Error types for the mechrevo control center.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MechrevoError {
    #[error("Hardware sysfs node or interface not accessible: {0}")]
    HardwareInterface(String),

    #[error("Permission denied: root privileges required to modify hardware state")]
    PermissionDenied,

    #[error("Unsupported performance profile: {0}")]
    UnsupportedProfile(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("IPC communication error: {0}")]
    Ipc(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MechrevoError>;

