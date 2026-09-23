//! Intel RAPL CPU power limits driver.

use std::fs;

use crate::error::{MechrevoError, Result};
use crate::hal::SysfsContext;

pub const RAPL_BASE_PATH: &str = "/sys/class/powercap/intel-rapl/intel-rapl:0";

pub const RAPL_MSR_PATHS: &[&str] = &[
    "/sys/class/powercap/intel-rapl/intel-rapl:0",
    "/sys/class/powercap/intel-rapl:0",
];

pub const RAPL_MMIO_PATHS: &[&str] = &[
    "/sys/class/powercap/intel-rapl-mmio/intel-rapl-mmio:0",
    "/sys/class/powercap/intel-rapl-mmio:0",
];

pub const CPUFREQ_BASE_PATH: &str = "/sys/devices/system/cpu/cpufreq";
pub const CPU_SYS_BASE_PATH: &str = "/sys/devices/system/cpu";

pub const MIN_PL_W: f64 = 15.0;
pub const MAX_PL1_W: f64 = 150.0;
pub const MAX_PL2_W: f64 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpuPowerLimits {
    pub pl1_w: f64,
    pub pl2_w: f64,
}

pub struct IntelRaplDriver {
    ctx: SysfsContext,
}

impl IntelRaplDriver {
    pub fn new(ctx: SysfsContext) -> Self {
        Self { ctx }
    }

    /// Discovers all active RAPL domain bases across MSR and MMIO interfaces.
    pub fn find_active_domain_bases(&self) -> Vec<String> {
        let mut bases = Vec::new();
        for path in RAPL_MSR_PATHS {
            if self.ctx.exists(path) {
                bases.push(path.to_string());
                break;
            }
        }
        for path in RAPL_MMIO_PATHS {
            if self.ctx.exists(path) {
                bases.push(path.to_string());
                break;
            }
        }
        if bases.is_empty() && self.ctx.exists(RAPL_BASE_PATH) {
            bases.push(RAPL_BASE_PATH.to_string());
        }
        bases
    }

    /// Dynamically resolves PL1 and PL2 sysfs paths for a specific base by probing constraint names.
    pub fn resolve_nodes_for_base(&self, base_str: &str) -> (String, String) {
        let base = self.ctx.resolve(base_str);
        let mut pl1_node = format!("{}/constraint_0_power_limit_uw", base_str);
        let mut pl2_node = format!("{}/constraint_1_power_limit_uw", base_str);

        if base.exists() {
            if let Ok(entries) = fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    if name_str.starts_with("constraint_") && name_str.ends_with("_name") {
                        if let Ok(content) = fs::read_to_string(entry.path()) {
                            let label = content.trim().to_lowercase();
                            let parts: Vec<&str> = name_str.split('_').collect();
                            if parts.len() >= 3 {
                                let idx = parts[1];
                                let limit_node = format!(
                                    "{}/constraint_{}_power_limit_uw",
                                    base_str, idx
                                );
                                if label == "long_term" {
                                    pl1_node = limit_node;
                                } else if label == "short_term" {
                                    pl2_node = limit_node;
                                }
                            }
                        }
                    }
                }
            }
        }

        (pl1_node, pl2_node)
    }

    /// Dynamically resolves PL1 and PL2 sysfs paths on primary domain.
    pub fn resolve_nodes(&self) -> (String, String) {
        let active = self.find_active_domain_bases();
        if let Some(first_base) = active.first() {
            self.resolve_nodes_for_base(first_base)
        } else {
            self.resolve_nodes_for_base(RAPL_BASE_PATH)
        }
    }

    /// Reads current PL1 and PL2 limits in Watts.
    pub fn get_power_limits(&self) -> Result<CpuPowerLimits> {
        let active_bases = self.find_active_domain_bases();
        if active_bases.is_empty() {
            return Err(MechrevoError::HardwareInterface(
                "Intel RAPL sysfs node not found".to_string(),
            ));
        }

        for base_str in &active_bases {
            let (pl1_node, pl2_node) = self.resolve_nodes_for_base(base_str);
            if self.ctx.exists(&pl1_node) && self.ctx.exists(&pl2_node) {
                if let (Ok(raw_pl1_str), Ok(raw_pl2_str)) = (
                    self.ctx.read_text(&pl1_node),
                    self.ctx.read_text(&pl2_node),
                ) {
                    if let (Ok(raw_pl1), Ok(raw_pl2)) = (
                        raw_pl1_str.parse::<u64>(),
                        raw_pl2_str.parse::<u64>(),
                    ) {
                        return Ok(CpuPowerLimits {
                            pl1_w: raw_pl1 as f64 / 1_000_000.0,
                            pl2_w: raw_pl2 as f64 / 1_000_000.0,
                        });
                    }
                }
            }
        }

        Err(MechrevoError::HardwareInterface(
            "Intel RAPL sysfs node not found or unreadable".to_string(),
        ))
    }

    /// Sets new PL1 and PL2 limits in Watts across ALL active domains (MSR + MMIO).
    /// Enforces anti-inversion timing axioms ($PL2_{new} \ge PL2_{old}$) and synchronizes
    /// both MSR and MMIO interfaces to bypass Intel 14th Gen HX P-Unit MIN(MSR, MMIO) clamps.
    pub fn set_power_limits(&self, pl1_w: f64, pl2_w: f64) -> Result<()> {
        if !(MIN_PL_W..=MAX_PL1_W).contains(&pl1_w) {
            return Err(MechrevoError::InvalidParameter(format!(
                "PL1 must be between {}W and {}W (got {}W)",
                MIN_PL_W, MAX_PL1_W, pl1_w
            )));
        }
        if !(MIN_PL_W..=MAX_PL2_W).contains(&pl2_w) {
            return Err(MechrevoError::InvalidParameter(format!(
                "PL2 must be between {}W and {}W (got {}W)",
                MIN_PL_W, MAX_PL2_W, pl2_w
            )));
        }
        if pl1_w > pl2_w {
            return Err(MechrevoError::InvalidParameter(format!(
                "PL1 ({}W) cannot exceed PL2 ({}W)",
                pl1_w, pl2_w
            )));
        }

        let active_bases = self.find_active_domain_bases();
        if active_bases.is_empty() {
            return Err(MechrevoError::HardwareInterface(
                "Intel RAPL sysfs node not found".to_string(),
            ));
        }

        let pl1_uw = (pl1_w * 1_000_000.0) as u64;
        let pl2_uw = (pl2_w * 1_000_000.0) as u64;

        let current_pl2 = self.get_power_limits().map(|c| c.pl2_w).ok();
        let write_pl2_first = current_pl2.map(|curr| pl2_w >= curr).unwrap_or(true);

        let mut write_success = false;
        let mut last_err = None;

        for base_str in &active_bases {
            let (pl1_node, pl2_node) = self.resolve_nodes_for_base(base_str);
            if !self.ctx.exists(&pl1_node) || !self.ctx.exists(&pl2_node) {
                continue;
            }

            let res = if write_pl2_first {
                self.ctx
                    .write_text(&pl2_node, &format!("{}\n", pl2_uw))
                    .and_then(|_| self.ctx.write_text(&pl1_node, &format!("{}\n", pl1_uw)))
            } else {
                self.ctx
                    .write_text(&pl1_node, &format!("{}\n", pl1_uw))
                    .and_then(|_| self.ctx.write_text(&pl2_node, &format!("{}\n", pl2_uw)))
            };

            match res {
                Ok(()) => write_success = true,
                Err(e) => last_err = Some(e),
            }
        }

        if write_success {
            Ok(())
        } else if let Some(err) = last_err {
            Err(err)
        } else {
            Err(MechrevoError::HardwareInterface(
                "Failed to write to any Intel RAPL domain".to_string(),
            ))
        }
    }

    /// Discovers all available CPU Energy Performance Preference (EPP) sysfs nodes.
    /// Traverses /sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference,
    /// and falls back to /sys/devices/system/cpu/cpu*/power/energy_perf_preference.
    pub fn find_epp_nodes(&self) -> Vec<String> {
        let mut nodes = Vec::new();

        // 1. Primary path: cpufreq policy nodes (Intel HWP active mode)
        let cpufreq_base = self.ctx.resolve(CPUFREQ_BASE_PATH);
        if cpufreq_base.exists() {
            if let Ok(entries) = fs::read_dir(&cpufreq_base) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    if name_str.starts_with("policy") {
                        let epp_path = format!("{}/{}/energy_performance_preference", CPUFREQ_BASE_PATH, name_str);
                        if self.ctx.exists(&epp_path) {
                            nodes.push(epp_path);
                        }
                    }
                }
            }
        }

        // 2. Fallback path: cpuX/power/energy_perf_preference
        if nodes.is_empty() {
            let cpu_base = self.ctx.resolve(CPU_SYS_BASE_PATH);
            if cpu_base.exists() {
                if let Ok(entries) = fs::read_dir(&cpu_base) {
                    for entry in entries.flatten() {
                        let file_name = entry.file_name();
                        let name_str = file_name.to_string_lossy();
                        if name_str.starts_with("cpu") && name_str[3..].chars().all(|c| c.is_ascii_digit()) {
                            let epp_path = format!("{}/{}/power/energy_perf_preference", CPU_SYS_BASE_PATH, name_str);
                            if self.ctx.exists(&epp_path) {
                                nodes.push(epp_path);
                            }
                        }
                    }
                }
            }
        }

        nodes.sort();
        nodes
    }

    /// Sets Intel Energy Performance Preference (EPP) across all CPU policies.
    /// Values typically include: "performance", "balance_performance", "balance_power", "power".
    /// Synchronizes HWP frequency scaling with RAPL power limits to ensure full 105W utilization or deep C-state sleep.
    pub fn set_energy_performance_preference(&self, preference: &str) -> Result<usize> {
        let normalized = preference.trim().to_lowercase();
        let valid_prefs = ["performance", "balance_performance", "balance_power", "power", "default"];
        if !valid_prefs.contains(&normalized.as_str()) {
            return Err(MechrevoError::InvalidParameter(format!(
                "Invalid EPP preference '{}'. Valid: {}",
                preference,
                valid_prefs.join(", ")
            )));
        }

        let nodes = self.find_epp_nodes();
        if nodes.is_empty() {
            return Ok(0);
        }

        let mut updated = 0;
        let mut last_err = None;
        for node in &nodes {
            match self.ctx.write_text(node, &format!("{}\n", normalized)) {
                Ok(()) => updated += 1,
                Err(e) => last_err = Some(e),
            }
        }

        if updated > 0 {
            Ok(updated)
        } else if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_mock_rapl(dir: &std::path::Path) -> SysfsContext {
        let ctx = SysfsContext::with_root(dir);
        let base = ctx.resolve(RAPL_BASE_PATH);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();
        ctx
    }

    #[test]
    fn test_read_cpu_power_limits() {
        let dir = tempdir().unwrap();
        let ctx = setup_mock_rapl(dir.path());
        let driver = IntelRaplDriver::new(ctx);

        let limits = driver.get_power_limits().unwrap();
        assert_eq!(limits.pl1_w, 75.0);
        assert_eq!(limits.pl2_w, 90.0);
    }

    #[test]
    fn test_set_cpu_power_limits_success() {
        let dir = tempdir().unwrap();
        let ctx = setup_mock_rapl(dir.path());
        let driver = IntelRaplDriver::new(ctx.clone());

        driver.set_power_limits(105.0, 135.0).unwrap();

        let limits = driver.get_power_limits().unwrap();
        assert_eq!(limits.pl1_w, 105.0);
        assert_eq!(limits.pl2_w, 135.0);
    }

    #[test]
    fn test_set_cpu_power_limits_validation() {
        let dir = tempdir().unwrap();
        let ctx = setup_mock_rapl(dir.path());
        let driver = IntelRaplDriver::new(ctx);

        assert!(driver.set_power_limits(10.0, 90.0).is_err());
        assert!(driver.set_power_limits(160.0, 180.0).is_err());
        assert!(driver.set_power_limits(90.0, 80.0).is_err());
    }

    #[test]
    fn test_set_cpu_power_limits_mathematical_order() {
        let dir = tempdir().unwrap();
        let ctx = setup_mock_rapl(dir.path());
        let driver = IntelRaplDriver::new(ctx.clone());

        // Target: 80W / 100W (PL2 expands from 90W to 100W, but 80W < 90W)
        // Must write PL2 first
        driver.set_power_limits(80.0, 100.0).unwrap();
        let limits = driver.get_power_limits().unwrap();
        assert_eq!(limits.pl1_w, 80.0);
        assert_eq!(limits.pl2_w, 100.0);
    }

    #[test]
    fn test_rapl_dynamic_constraint_name_matching() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let base = ctx.resolve(RAPL_BASE_PATH);
        std::fs::create_dir_all(&base).unwrap();

        // Inverted names: 0 is short_term, 1 is long_term
        std::fs::write(base.join("constraint_0_name"), "short_term\n").unwrap();
        std::fs::write(base.join("constraint_0_power_limit_uw"), "90000000\n").unwrap();
        std::fs::write(base.join("constraint_1_name"), "long_term\n").unwrap();
        std::fs::write(base.join("constraint_1_power_limit_uw"), "75000000\n").unwrap();

        let driver = IntelRaplDriver::new(ctx);
        let limits = driver.get_power_limits().unwrap();
        assert_eq!(limits.pl1_w, 75.0);
        assert_eq!(limits.pl2_w, 90.0);

        driver.set_power_limits(105.0, 135.0).unwrap();
        let pl1_content = std::fs::read_to_string(base.join("constraint_1_power_limit_uw")).unwrap();
        let pl2_content = std::fs::read_to_string(base.join("constraint_0_power_limit_uw")).unwrap();
        assert_eq!(pl1_content.trim(), "105000000");
        assert_eq!(pl2_content.trim(), "135000000");
    }

    #[test]
    fn test_set_cpu_power_limits_fallback_when_read_fails() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let base = ctx.resolve(RAPL_BASE_PATH);
        std::fs::create_dir_all(&base).unwrap();

        // Nodes exist but contain corrupted / empty content so get_power_limits() fails
        std::fs::write(base.join("constraint_0_power_limit_uw"), "").unwrap();
        std::fs::write(base.join("constraint_1_power_limit_uw"), "").unwrap();

        let driver = IntelRaplDriver::new(ctx);
        assert!(driver.get_power_limits().is_err());

        // But set_power_limits must successfully recover and write with safety order!
        assert!(driver.set_power_limits(45.0, 65.0).is_ok());
        let pl1 = std::fs::read_to_string(base.join("constraint_0_power_limit_uw")).unwrap();
        let pl2 = std::fs::read_to_string(base.join("constraint_1_power_limit_uw")).unwrap();
        assert_eq!(pl1.trim(), "45000000");
        assert_eq!(pl2.trim(), "65000000");
    }

    #[test]
    fn test_rapl_dual_domain_msr_and_mmio_synchronization() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // 1. Setup MSR domain (intel-rapl)
        let msr_base = ctx.resolve("/sys/class/powercap/intel-rapl/intel-rapl:0");
        std::fs::create_dir_all(&msr_base).unwrap();
        std::fs::write(msr_base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(msr_base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();

        // 2. Setup MMIO domain (intel-rapl-mmio) with lower default clamp (45W/55W)
        let mmio_base = ctx.resolve("/sys/class/powercap/intel-rapl-mmio/intel-rapl-mmio:0");
        std::fs::create_dir_all(&mmio_base).unwrap();
        std::fs::write(mmio_base.join("constraint_0_power_limit_uw"), "45000000\n").unwrap();
        std::fs::write(mmio_base.join("constraint_1_power_limit_uw"), "55000000\n").unwrap();

        let driver = IntelRaplDriver::new(ctx);
        // Setting limits to 105W / 135W MUST write to BOTH MSR and MMIO domains!
        driver.set_power_limits(105.0, 135.0).unwrap();

        // Verify MSR domain was updated
        let msr_pl1 = std::fs::read_to_string(msr_base.join("constraint_0_power_limit_uw")).unwrap();
        let msr_pl2 = std::fs::read_to_string(msr_base.join("constraint_1_power_limit_uw")).unwrap();
        assert_eq!(msr_pl1.trim(), "105000000");
        assert_eq!(msr_pl2.trim(), "135000000");

        // Verify MMIO domain was ALSO updated (preventing hardware P-Unit MIN(MSR, MMIO) clamp)
        let mmio_pl1 = std::fs::read_to_string(mmio_base.join("constraint_0_power_limit_uw")).unwrap();
        let mmio_pl2 = std::fs::read_to_string(mmio_base.join("constraint_1_power_limit_uw")).unwrap();
        assert_eq!(
            mmio_pl1.trim(), "105000000",
            "MMIO PL1 must be updated to 105W to prevent hardware throttle"
        );
        assert_eq!(
            mmio_pl2.trim(), "135000000",
            "MMIO PL2 must be updated to 135W to prevent hardware throttle"
        );
    }

    #[test]
    fn test_rapl_mmio_only_fallback() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup ONLY MMIO domain (simulating systems with locked MSR or MMIO only)
        let mmio_base = ctx.resolve("/sys/class/powercap/intel-rapl-mmio/intel-rapl-mmio:0");
        std::fs::create_dir_all(&mmio_base).unwrap();
        std::fs::write(mmio_base.join("constraint_0_power_limit_uw"), "55000000\n").unwrap();
        std::fs::write(mmio_base.join("constraint_1_power_limit_uw"), "70000000\n").unwrap();

        let driver = IntelRaplDriver::new(ctx);
        let limits = driver.get_power_limits().unwrap();
        assert_eq!(limits.pl1_w, 55.0);
        assert_eq!(limits.pl2_w, 70.0);

        driver.set_power_limits(80.0, 100.0).unwrap();
        let mmio_pl1 = std::fs::read_to_string(mmio_base.join("constraint_0_power_limit_uw")).unwrap();
        assert_eq!(mmio_pl1.trim(), "80000000");
    }

    #[test]
    fn test_cpu_epp_policy_configuration() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock cpufreq policy0 and policy1
        let policy0 = ctx.resolve("/sys/devices/system/cpu/cpufreq/policy0");
        let policy1 = ctx.resolve("/sys/devices/system/cpu/cpufreq/policy1");
        std::fs::create_dir_all(&policy0).unwrap();
        std::fs::create_dir_all(&policy1).unwrap();

        std::fs::write(policy0.join("energy_performance_preference"), "balance_power\n").unwrap();
        std::fs::write(policy1.join("energy_performance_preference"), "balance_power\n").unwrap();

        let driver = IntelRaplDriver::new(ctx);
        let updated_count = driver.set_energy_performance_preference("performance").unwrap();
        assert_eq!(updated_count, 2, "Must update both policy0 and policy1 EPP nodes");

        let p0_epp = std::fs::read_to_string(policy0.join("energy_performance_preference")).unwrap();
        let p1_epp = std::fs::read_to_string(policy1.join("energy_performance_preference")).unwrap();
        assert_eq!(p0_epp.trim(), "performance");
        assert_eq!(p1_epp.trim(), "performance");
    }
}


