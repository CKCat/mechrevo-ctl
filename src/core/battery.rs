//! Battery charging threshold and health status driver.

use std::fs;

use crate::error::{MechrevoError, Result};
use crate::hal::SysfsContext;

pub const POWER_SUPPLY_BASE: &str = "/sys/class/power_supply";
pub const MIN_THRESHOLD: u32 = 20;
pub const MAX_THRESHOLD: u32 = 100;

pub const THRESHOLD_CANDIDATES: &[&str] = &[
    "charge_control_end_threshold",
    "charge_stop_threshold",
    "charge_control_threshold",
    "charge_control_limit_max",
];

pub const START_THRESHOLD_CANDIDATES: &[&str] = &[
    "charge_control_start_threshold",
    "charge_start_threshold",
];

pub const TUXEDO_CHARGING_PROFILE_CANDIDATES: &[&str] = &[
    "/sys/devices/platform/tuxedo_keyboard/charging_profile/charging_profile",
    "/sys/devices/platform/tuxedo_keyboard/charging_profile",
    "/sys/devices/platform/uniwill_laptop/charging_profile",
];

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryStatus {
    pub capacity_percent: u32,
    pub status: String,
    pub charge_threshold: u32,
}

pub struct BatteryDriver {
    ctx: SysfsContext,
}

impl BatteryDriver {
    pub fn new(ctx: SysfsContext) -> Self {
        Self { ctx }
    }

    /// Dynamically locates the active battery device node (BAT0, BAT1, etc.).
    pub fn find_battery_path(&self) -> Result<String> {
        let base = self.ctx.resolve(POWER_SUPPLY_BASE);
        if base.exists() {
            if let Ok(entries) = fs::read_dir(&base) {
                let mut candidates: Vec<_> = entries.flatten().collect();
                candidates.sort_by_key(|e| e.file_name());

                for entry in candidates {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    if name_str.to_uppercase().starts_with("BAT")
                        && entry.path().join("capacity").exists()
                    {
                        return Ok(format!("{}/{}", POWER_SUPPLY_BASE, name_str));
                    }
                }
            }
        }
        Err(MechrevoError::HardwareInterface(
            "Battery sysfs node not found".to_string(),
        ))
    }

    /// Finds the active end/stop threshold control sysfs node.
    pub fn find_threshold_node(&self, bat_path: &str) -> Option<String> {
        for candidate in THRESHOLD_CANDIDATES {
            let node = format!("{}/{}", bat_path, candidate);
            if self.ctx.exists(&node) {
                return Some(node);
            }
        }
        None
    }

    /// Finds the active Tuxedo/DKMS charging profile node if present.
    pub fn find_tuxedo_charging_profile_node(&self) -> Option<String> {
        for candidate in TUXEDO_CHARGING_PROFILE_CANDIDATES {
            if self.ctx.exists(candidate) {
                return Some(candidate.to_string());
            }
        }
        None
    }

    /// Finds the active start threshold control sysfs node, if present.
    pub fn find_start_threshold_node(&self, bat_path: &str) -> Option<String> {
        for candidate in START_THRESHOLD_CANDIDATES {
            let node = format!("{}/{}", bat_path, candidate);
            if self.ctx.exists(&node) {
                return Some(node);
            }
        }
        None
    }

    /// Reads current battery charge percentage, state, and conservation threshold.
    pub fn get_battery_status(&self) -> Result<BatteryStatus> {
        let bat_path = self.find_battery_path()?;
        let capacity_node = format!("{}/capacity", bat_path);
        let status_node = format!("{}/status", bat_path);

        let capacity: u32 = self
            .ctx
            .read_text(&capacity_node)?
            .parse()
            .map_err(|e| MechrevoError::HardwareInterface(format!("Invalid battery capacity: {}", e)))?;
        let status = self.ctx.read_text(&status_node)?;

        let mut threshold = 100;
        if let Some(threshold_node) = self.find_threshold_node(&bat_path) {
            if let Ok(val_str) = self.ctx.read_text(&threshold_node) {
                if let Ok(val) = val_str.parse::<u32>() {
                    threshold = val;
                }
            }
        } else if let Some(tuxedo_node) = self.find_tuxedo_charging_profile_node() {
            if let Ok(val_str) = self.ctx.read_text(&tuxedo_node) {
                let trimmed = val_str.trim().to_lowercase();
                if trimmed == "stationary" {
                    threshold = 80;
                } else if trimmed == "balanced" {
                    threshold = 90;
                } else if trimmed == "high_capacity" {
                    threshold = 100;
                }
            }
        }

        Ok(BatteryStatus {
            capacity_percent: capacity,
            status,
            charge_threshold: threshold,
        })
    }

    /// Sets upper charging threshold percentage (e.g. 60, 80, 100).
    /// Adheres to Linux kernel dual-threshold timing axioms (start <= end invariant) to prevent EINVAL.
    /// Also supports Tuxedo DKMS platform charging profiles (stationary, balanced, high_capacity).
    pub fn set_charge_threshold(&self, threshold_percent: u32) -> Result<()> {
        if !(MIN_THRESHOLD..=MAX_THRESHOLD).contains(&threshold_percent) {
            return Err(MechrevoError::InvalidParameter(format!(
                "Battery threshold must be between {} and {} (got {})",
                MIN_THRESHOLD, MAX_THRESHOLD, threshold_percent
            )));
        }

        let bat_path = self.find_battery_path()?;

        let map_write_err = |e: MechrevoError| -> MechrevoError {
            match e {
                MechrevoError::PermissionDenied => MechrevoError::HardwareInterface(
                    "Permission denied when writing battery threshold. Note: Linux mainline uniwill-laptop driver under 'force=1' locks charge threshold writes due to CVE-2026-64143 safety policy. Consider installing tuxedo-drivers-dkms or running with appropriate root permissions.".to_string(),
                ),
                MechrevoError::HardwareInterface(msg) => MechrevoError::HardwareInterface(format!(
                    "{} (Note: Mainline uniwill-laptop with 'force=1' locks battery writes due to CVE-2026-64143; consider tuxedo-drivers-dkms)",
                    msg
                )),
                other => other,
            }
        };

        if let Some(threshold_node) = self.find_threshold_node(&bat_path) {
            let start_node_opt = self.find_start_threshold_node(&bat_path);
            let target_start = threshold_percent
                .saturating_sub(5)
                .max(10)
                .min(threshold_percent.saturating_sub(1));

            if let Some(start_node) = start_node_opt {
                let current_end_opt: Option<u32> = self
                    .ctx
                    .read_text(&threshold_node)
                    .ok()
                    .and_then(|s| s.trim().parse().ok());

                let write_end_first = current_end_opt
                    .map(|curr| threshold_percent >= curr)
                    .unwrap_or(false);

                if write_end_first {
                    self.ctx
                        .write_text(&threshold_node, &format!("{}\n", threshold_percent))
                        .map_err(&map_write_err)?;
                    let _ = self
                        .ctx
                        .write_text(&start_node, &format!("{}\n", target_start));
                } else {
                    let _ = self
                        .ctx
                        .write_text(&start_node, &format!("{}\n", target_start));
                    self.ctx
                        .write_text(&threshold_node, &format!("{}\n", threshold_percent))
                        .map_err(&map_write_err)?;
                }
            } else {
                self.ctx
                    .write_text(&threshold_node, &format!("{}\n", threshold_percent))
                    .map_err(&map_write_err)?;
            }
        } else if let Some(tuxedo_node) = self.find_tuxedo_charging_profile_node() {
            let mode_str = if threshold_percent <= 80 {
                "stationary"
            } else if threshold_percent <= 90 {
                "balanced"
            } else {
                "high_capacity"
            };
            self.ctx
                .write_text(&tuxedo_node, &format!("{}\n", mode_str))
                .map_err(&map_write_err)?;
        } else {
            let fallback_node = format!("{}/charge_control_end_threshold", bat_path);
            self.ctx
                .write_text(&fallback_node, &format!("{}\n", threshold_percent))
                .map_err(&map_write_err)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_battery_status_and_threshold() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let bat_dir = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_dir).unwrap();
        std::fs::write(bat_dir.join("capacity"), "85\n").unwrap();
        std::fs::write(bat_dir.join("status"), "Discharging\n").unwrap();
        std::fs::write(bat_dir.join("charge_control_end_threshold"), "100\n").unwrap();

        let driver = BatteryDriver::new(ctx.clone());
        let status = driver.get_battery_status().unwrap();
        assert_eq!(status.capacity_percent, 85);
        assert_eq!(status.status, "Discharging");
        assert_eq!(status.charge_threshold, 100);

        driver.set_charge_threshold(80).unwrap();
        let updated = driver.get_battery_status().unwrap();
        assert_eq!(updated.charge_threshold, 80);
    }

    #[test]
    fn test_battery_fallback_charge_stop_threshold() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let bat_dir = ctx.resolve("/sys/class/power_supply/BAT1");
        std::fs::create_dir_all(&bat_dir).unwrap();
        std::fs::write(bat_dir.join("capacity"), "70\n").unwrap();
        std::fs::write(bat_dir.join("status"), "Charging\n").unwrap();
        std::fs::write(bat_dir.join("charge_stop_threshold"), "80\n").unwrap();

        let driver = BatteryDriver::new(ctx.clone());
        let status = driver.get_battery_status().unwrap();
        assert_eq!(status.charge_threshold, 80);

        driver.set_charge_threshold(60).unwrap();
        let content = std::fs::read_to_string(bat_dir.join("charge_stop_threshold")).unwrap();
        assert_eq!(content.trim(), "60");
    }

    #[test]
    fn test_battery_validation() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let driver = BatteryDriver::new(ctx);

        assert!(driver.set_charge_threshold(15).is_err());
        assert!(driver.set_charge_threshold(105).is_err());
    }

    #[test]
    fn test_battery_start_threshold_hysteresis() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let bat_dir = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_dir).unwrap();
        std::fs::write(bat_dir.join("capacity"), "50\n").unwrap();
        std::fs::write(bat_dir.join("status"), "Charging\n").unwrap();
        std::fs::write(bat_dir.join("charge_control_end_threshold"), "100\n").unwrap();
        std::fs::write(bat_dir.join("charge_control_start_threshold"), "95\n").unwrap();

        let driver = BatteryDriver::new(ctx.clone());
        driver.set_charge_threshold(80).unwrap();

        let end_content = std::fs::read_to_string(bat_dir.join("charge_control_end_threshold")).unwrap();
        let start_content = std::fs::read_to_string(bat_dir.join("charge_control_start_threshold")).unwrap();
        assert_eq!(end_content.trim(), "80");
        assert_eq!(start_content.trim(), "75", "Start threshold should automatically form a 5% hysteresis buffer");
    }

    #[test]
    fn test_battery_threshold_timing_order_safety() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let bat_dir = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_dir).unwrap();
        std::fs::write(bat_dir.join("capacity"), "90\n").unwrap();
        std::fs::write(bat_dir.join("status"), "Charging\n").unwrap();
        std::fs::write(bat_dir.join("charge_control_end_threshold"), "100\n").unwrap();
        std::fs::write(bat_dir.join("charge_control_start_threshold"), "95\n").unwrap();

        let driver = BatteryDriver::new(ctx.clone());

        // 1. Lowering threshold: 100% -> 60% (Must set start lower before end to avoid kernel EINVAL)
        driver.set_charge_threshold(60).unwrap();
        let end_content = std::fs::read_to_string(bat_dir.join("charge_control_end_threshold")).unwrap();
        let start_content = std::fs::read_to_string(bat_dir.join("charge_control_start_threshold")).unwrap();
        assert_eq!(end_content.trim(), "60");
        assert_eq!(start_content.trim(), "55");

        // 2. Raising threshold: 60% -> 100% (Must raise end before start to avoid kernel EINVAL)
        driver.set_charge_threshold(100).unwrap();
        let end_content = std::fs::read_to_string(bat_dir.join("charge_control_end_threshold")).unwrap();
        let start_content = std::fs::read_to_string(bat_dir.join("charge_control_start_threshold")).unwrap();
        assert_eq!(end_content.trim(), "100");
        assert_eq!(start_content.trim(), "95");
    }

    #[test]
    fn test_battery_min_threshold_start_strictly_less() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let bat_dir = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_dir).unwrap();
        std::fs::write(bat_dir.join("capacity"), "50\n").unwrap();
        std::fs::write(bat_dir.join("status"), "Charging\n").unwrap();
        std::fs::write(bat_dir.join("charge_control_end_threshold"), "100\n").unwrap();
        std::fs::write(bat_dir.join("charge_control_start_threshold"), "95\n").unwrap();

        let driver = BatteryDriver::new(ctx.clone());
        driver.set_charge_threshold(20).unwrap();

        let end_val: u32 = std::fs::read_to_string(bat_dir.join("charge_control_end_threshold"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let start_val: u32 = std::fs::read_to_string(bat_dir.join("charge_control_start_threshold"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        assert_eq!(end_val, 20);
        assert!(
            start_val < end_val,
            "Kernel ABI requirement: start_threshold ({}) must be strictly less than end_threshold ({})",
            start_val, end_val
        );
        assert_eq!(start_val, 15);
    }

    #[test]
    fn test_battery_tuxedo_charging_profile_read_and_write() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // 1. Setup mock battery with capacity/status but NO standard threshold node
        let bat_dir = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_dir).unwrap();
        std::fs::write(bat_dir.join("capacity"), "80\n").unwrap();
        std::fs::write(bat_dir.join("status"), "Charging\n").unwrap();

        // 2. Setup Tuxedo charging_profile sysfs path
        let tuxedo_node = ctx.resolve("/sys/devices/platform/tuxedo_keyboard/charging_profile/charging_profile");
        std::fs::create_dir_all(tuxedo_node.parent().unwrap()).unwrap();
        std::fs::write(&tuxedo_node, "stationary\n").unwrap();

        let driver = BatteryDriver::new(ctx.clone());

        // Reading status must translate "stationary" to 80%
        let status = driver.get_battery_status().unwrap();
        assert_eq!(status.charge_threshold, 80, "Tuxedo 'stationary' should map to 80%");

        // Writing 100% must translate to "high_capacity\n"
        driver.set_charge_threshold(100).unwrap();
        let t100 = std::fs::read_to_string(&tuxedo_node).unwrap();
        assert_eq!(t100.trim(), "high_capacity");

        // Writing 90% must translate to "balanced\n"
        driver.set_charge_threshold(90).unwrap();
        let t90 = std::fs::read_to_string(&tuxedo_node).unwrap();
        assert_eq!(t90.trim(), "balanced");

        // Writing 80% must translate to "stationary\n"
        driver.set_charge_threshold(80).unwrap();
        let t80 = std::fs::read_to_string(&tuxedo_node).unwrap();
        assert_eq!(t80.trim(), "stationary");
    }

    #[test]
    fn test_battery_threshold_write_failure_provides_cve_and_dkms_guidance() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        let bat_dir = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_dir).unwrap();
        std::fs::write(bat_dir.join("capacity"), "80\n").unwrap();
        std::fs::write(bat_dir.join("status"), "Discharging\n").unwrap();

        // Create threshold node and make it read-only on disk
        let thresh_node = bat_dir.join("charge_control_end_threshold");
        std::fs::write(&thresh_node, "100\n").unwrap();
        let mut perms = std::fs::metadata(&thresh_node).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&thresh_node, perms).unwrap();

        let driver = BatteryDriver::new(ctx);
        let res = driver.set_charge_threshold(80);
        assert!(res.is_err());
        let err_str = res.unwrap_err().to_string();
        assert!(
            err_str.contains("CVE-2026-64143") && err_str.contains("tuxedo-drivers-dkms"),
            "Error must guide user about CVE-2026-64143 and tuxedo-drivers-dkms: got '{}'",
            err_str
        );
    }
}


