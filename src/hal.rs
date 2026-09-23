//! Hardware Abstraction Layer (HAL) and SysfsContext.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{MechrevoError, Result};

/// Manages interactions with the Linux `/sys` filesystem.
/// Supports path redirection (via `root_path`) for automated testing and mocking.
#[derive(Clone, Debug, Default)]
pub struct SysfsContext {
    root_path: Option<PathBuf>,
}

impl SysfsContext {
    /// Creates a new default SysfsContext targeting the host system.
    pub fn new() -> Self {
        Self { root_path: None }
    }

    /// Creates a SysfsContext redirected to a custom base directory (useful for testing).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root_path: Some(root.into()),
        }
    }

    /// Returns the configured mock root path, if any.
    pub fn root_path(&self) -> Option<&Path> {
        self.root_path.as_deref()
    }

    /// Resolves a sysfs absolute path string against the configured root directory.
    pub fn resolve(&self, path_str: &str) -> PathBuf {
        let clean_path = path_str.trim_start_matches(['/', '\\']);
        if let Some(ref base) = self.root_path {
            if cfg!(windows) {
                let sanitized = clean_path.replace(':', "_");
                base.join(sanitized)
            } else {
                base.join(clean_path)
            }
        } else {
            PathBuf::from("/").join(clean_path)
        }
    }

    /// Reads UTF-8 text from a sysfs path, returning the trimmed string.
    pub fn read_text(&self, path_str: &str) -> Result<String> {
        let target = self.resolve(path_str);
        if !target.exists() {
            return Err(MechrevoError::HardwareInterface(format!(
                "Sysfs node not found: {}",
                path_str
            )));
        }

        match fs::read_to_string(&target) {
            Ok(content) => Ok(content.trim().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                Err(MechrevoError::PermissionDenied)
            }
            Err(e) => Err(MechrevoError::HardwareInterface(format!(
                "Failed to read sysfs node {}: {}",
                path_str, e
            ))),
        }
    }

    /// Writes text content to a sysfs path.
    pub fn write_text(&self, path_str: &str, value: &str) -> Result<()> {
        let target = self.resolve(path_str);
        if !target.exists() {
            return Err(MechrevoError::HardwareInterface(format!(
                "Sysfs node not found: {}",
                path_str
            )));
        }

        match fs::write(&target, value) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                Err(MechrevoError::PermissionDenied)
            }
            Err(e) => Err(MechrevoError::HardwareInterface(format!(
                "Failed to write to sysfs node {}: {}",
                path_str, e
            ))),
        }
    }

    /// Checks if a sysfs node exists.
    pub fn exists(&self, path_str: &str) -> bool {
        self.resolve(path_str).exists()
    }
}
