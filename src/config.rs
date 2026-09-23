//! Power profile definitions and hardware presets.

use serde::{Deserialize, Serialize};

use crate::error::{MechrevoError, Result};

/// Represents a hardware performance and thermal release profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PowerProfile {
    pub name: String,
    pub display_name: String,
    pub cpu_pl1_w: f64,
    pub cpu_pl2_w: f64,
    pub gpu_tgp_w: f64,
    pub platform_profile: String,
    pub fan_policy: String,
}

/// Default hardware profiles grounded in reverse-engineered Uniwill/Tongfang GM6 SMAPCTable
/// and Intel Turbo Boost 2.0 (Tau) dual-step power release physics:
///
/// - Office (Mode 2): Official OEM sets PL1 45W / PL2 45W, Fan M2T1 (max 50% duty), TccOffset 20 (85°C).
///   Here configured as PL1 45W / PL2 65W to provide burst responsiveness while keeping long-term thermals silent.
/// - Balanced (Mode 1): Official OEM sets PL1 75W / PL2 75W, Fan M1T1 (max 90% duty), TccOffset 5 (100°C), GPU 115W.
///   Here configured as PL1 75W / PL2 90W for optimal daily productivity.
/// - Turbo (Mode 3): Official OEM single-load PL1 130W / PL2 130W, combined load CPU 105W + GPU 140W (115W base + 25W Dynamic Boost),
///   Fan M3T1 (max 95% duty), TccOffset 5.
///   Here configured as CPU PL1 105W / PL2 135W, GPU 140W, providing balanced thermal headroom during dual-load stress.
pub fn get_default_profiles() -> Vec<PowerProfile> {
    vec![
        PowerProfile {
            name: "office".to_string(),
            display_name: "办公节能模式".to_string(),
            cpu_pl1_w: 45.0,
            cpu_pl2_w: 65.0,
            gpu_tgp_w: 60.0,
            platform_profile: "low-power".to_string(),
            fan_policy: "quiet".to_string(),
        },
        PowerProfile {
            name: "balanced".to_string(),
            display_name: "日常平衡模式".to_string(),
            cpu_pl1_w: 75.0,
            cpu_pl2_w: 90.0,
            gpu_tgp_w: 115.0,
            platform_profile: "balanced".to_string(),
            fan_policy: "balanced".to_string(),
        },
        PowerProfile {
            name: "turbo".to_string(),
            display_name: "满血狂暴模式".to_string(),
            cpu_pl1_w: 105.0,
            cpu_pl2_w: 135.0,
            gpu_tgp_w: 140.0,
            platform_profile: "performance".to_string(),
            fan_policy: "turbo".to_string(),
        },
    ]
}

/// Retrieves a power profile by its identifier (office, balanced, turbo).
pub fn get_profile_by_name(name: &str) -> Result<PowerProfile> {
    let normalized = name.trim().to_lowercase();
    get_default_profiles()
        .into_iter()
        .find(|p| p.name == normalized)
        .ok_or_else(|| MechrevoError::UnsupportedProfile(name.to_string()))
}

