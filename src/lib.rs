//! MECHREVO Linux Control Center library.

pub mod config;
pub mod core;
pub mod daemon;
pub mod error;
pub mod gui;
pub mod hal;

pub use config::{get_default_profiles, get_profile_by_name, PowerProfile};
pub use error::{MechrevoError, Result};
pub use hal::SysfsContext;

