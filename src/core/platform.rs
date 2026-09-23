//! ACPI platform_profile driver and integrated ProfileManager.

use crate::config::{get_profile_by_name, PowerProfile};
use crate::core::cpu::IntelRaplDriver;
use crate::core::gpu::NvidiaSmiDriver;
use crate::error::{MechrevoError, Result};
use crate::hal::SysfsContext;

pub const PROFILE_NODE: &str = "/sys/firmware/acpi/platform_profile";
pub const CHOICES_NODE: &str = "/sys/firmware/acpi/platform_profile_choices";
pub const DMI_PRODUCT_NAME_NODE: &str = "/sys/class/dmi/id/product_name";

pub struct AcpiPlatformDriver {
    ctx: SysfsContext,
}

impl AcpiPlatformDriver {
    pub fn new(ctx: SysfsContext) -> Self {
        Self { ctx }
    }

    /// Non-intrusively retrieves the platform DMI product/chassis name (e.g. GM6IQ0C).
    pub fn get_dmi_product_name(&self) -> Option<String> {
        if self.ctx.exists(DMI_PRODUCT_NAME_NODE) {
            self.ctx.read_text(DMI_PRODUCT_NAME_NODE).ok()
        } else {
            None
        }
    }

    pub fn is_available(&self) -> bool {
        self.ctx.exists(PROFILE_NODE)
    }

    pub fn get_current_profile(&self) -> Result<String> {
        if !self.ctx.exists(PROFILE_NODE) {
            return Err(MechrevoError::HardwareInterface(
                "ACPI platform_profile node not found".to_string(),
            ));
        }
        self.ctx.read_text(PROFILE_NODE)
    }

    pub fn get_available_profiles(&self) -> Vec<String> {
        if !self.ctx.exists(CHOICES_NODE) {
            return vec![
                "low-power".to_string(),
                "balanced".to_string(),
                "performance".to_string(),
            ];
        }

        match self.ctx.read_text(CHOICES_NODE) {
            Ok(content) => content.split_whitespace().map(|s| s.to_string()).collect(),
            Err(_) => vec![
                "low-power".to_string(),
                "balanced".to_string(),
                "performance".to_string(),
            ],
        }
    }

    pub fn set_profile(&self, profile: &str) -> Result<()> {
        let available = self.get_available_profiles();
        if !available.iter().any(|p| p == profile) {
            return Err(MechrevoError::InvalidParameter(format!(
                "Unsupported ACPI profile '{}'. Supported: {}",
                profile,
                available.join(", ")
            )));
        }

        if !self.ctx.exists(PROFILE_NODE) {
            return Err(MechrevoError::HardwareInterface(
                "ACPI platform_profile node not found".to_string(),
            ));
        }

        self.ctx.write_text(PROFILE_NODE, &format!("{}\n", profile))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileApplyResult {
    pub is_success: bool,
    pub applied_profile: PowerProfile,
    pub message: String,
}

pub const HWMON_BASE_PATH: &str = "/sys/class/hwmon";

pub const FAN_BOOST_CANDIDATES: &[&str] = &[
    "/sys/devices/platform/tuxedo_keyboard/fan_boost",
    "/sys/devices/platform/uniwill_laptop/fan_boost",
    "/sys/devices/platform/uniwill_wmi/fan_boost",
    "/sys/devices/platform/INOU0000:00/fan_boost",
    "/sys/bus/platform/devices/INOU0000:00/fan_boost",
    "/sys/devices/platform/mech_forza/fan_boost",
    "/sys/devices/platform/bitland-mifs-wmi/fan_boost",
    "/sys/devices/platform/qc71_laptop/fan_boost",
    "/sys/devices/platform/mechrevo/fan_boost",
];

/// Snapshot of thermal and cooling sensors from Linux hwmon subsystem.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HwmonSnapshot {
    pub cpu_temp_c: Option<i32>,
    pub fan1_rpm: Option<u32>,
    pub fan2_rpm: Option<u32>,
}

#[derive(Clone)]
pub struct FanDriver {
    ctx: SysfsContext,
}

impl FanDriver {
    pub fn new(ctx: SysfsContext) -> Self {
        Self { ctx }
    }

    pub fn find_fan_boost_node(&self) -> Option<String> {
        for node in FAN_BOOST_CANDIDATES {
            if self.ctx.exists(node) {
                return Some(node.to_string());
            }
        }
        None
    }

    pub fn get_fan_boost(&self) -> Option<bool> {
        if let Some(node) = self.find_fan_boost_node() {
            if let Ok(content) = self.ctx.read_text(&node) {
                return Some(content.trim() == "1");
            }
        }
        None
    }

    pub fn set_fan_boost(&self, enable: bool) -> Result<()> {
        let node = self.find_fan_boost_node().ok_or_else(|| {
            MechrevoError::HardwareInterface(
                "No supported fan boost interface found in sysfs (requires tuxedo-drivers or supported OEM module)".to_string(),
            )
        })?;
        let val = if enable { "1\n" } else { "0\n" };
        self.ctx.write_text(&node, val)
    }

    /// Universally reads CPU Package temperature and dual fan speeds from /sys/class/hwmon.
    pub fn get_hwmon_snapshot(&self) -> HwmonSnapshot {
        let mut snapshot = HwmonSnapshot::default();
        let base = self.ctx.resolve(HWMON_BASE_PATH);
        if !base.exists() {
            return snapshot;
        }

        let Ok(entries) = std::fs::read_dir(&base) else {
            return snapshot;
        };

        let mut sorted_entries: Vec<_> = entries.flatten().collect();
        sorted_entries.sort_by_key(|e| e.file_name());

        for entry in sorted_entries {
            let dir_name = entry.file_name();
            let dir_str = dir_name.to_string_lossy();
            if !dir_str.starts_with("hwmon") {
                continue;
            }
            let hwmon_path = format!("{}/{}", HWMON_BASE_PATH, dir_str);
            let name_path = format!("{}/name", hwmon_path);

            let driver_name = self
                .ctx
                .read_text(&name_path)
                .map(|s| s.trim().to_lowercase())
                .unwrap_or_default();

            // 1. CPU Package Temperature (coretemp or k10temp, with platform driver fallback)
            let is_cpu_sensor = driver_name.contains("coretemp")
                || driver_name.contains("k10temp")
                || driver_name.contains("uniwill")
                || driver_name.contains("tuxedo")
                || driver_name.is_empty();

            if snapshot.cpu_temp_c.is_none() && is_cpu_sensor {
                let temp_path = format!("{}/temp1_input", hwmon_path);
                if let Ok(raw_str) = self.ctx.read_text(&temp_path) {
                    if let Ok(millidegrees) = raw_str.trim().parse::<i32>() {
                        snapshot.cpu_temp_c = Some(millidegrees / 1000);
                    }
                }
            }

            // 2. Dual Fan RPM readings (uniwill_laptop, tuxedo_keyboard, etc.)
            if snapshot.fan1_rpm.is_none() {
                let fan1_path = format!("{}/fan1_input", hwmon_path);
                if let Ok(raw_str) = self.ctx.read_text(&fan1_path) {
                    if let Ok(rpm) = raw_str.trim().parse::<u32>() {
                        snapshot.fan1_rpm = Some(rpm);
                    }
                }
            }
            if snapshot.fan2_rpm.is_none() {
                let fan2_path = format!("{}/fan2_input", hwmon_path);
                if let Ok(raw_str) = self.ctx.read_text(&fan2_path) {
                    if let Ok(rpm) = raw_str.trim().parse::<u32>() {
                        snapshot.fan2_rpm = Some(rpm);
                    }
                }
            }
        }

        snapshot
    }
}

pub struct ProfileManager {
    cpu: IntelRaplDriver,
    gpu: NvidiaSmiDriver,
    platform: AcpiPlatformDriver,
    fan: FanDriver,
}

impl ProfileManager {
    pub fn new(
        cpu_driver: IntelRaplDriver,
        gpu_driver: NvidiaSmiDriver,
        platform_driver: AcpiPlatformDriver,
        fan_driver: FanDriver,
    ) -> Self {
        Self {
            cpu: cpu_driver,
            gpu: gpu_driver,
            platform: platform_driver,
            fan: fan_driver,
        }
    }

    /// Accesses the underlying FanDriver instance.
    pub fn fan(&self) -> &FanDriver {
        &self.fan
    }

    /// Applies a named profile across ACPI platform, CPU limits, GPU limits, and fan policy.
    /// Gracefully degrades on platforms without writable RAPL (e.g. AMD Zen / locked UEFI).
    pub fn apply_profile(&self, profile_name: &str) -> Result<ProfileApplyResult> {
        let profile = get_profile_by_name(profile_name)?;

        // 1. ACPI platform profile (First-class firmware control via EC)
        if self.platform.is_available() {
            let available = self.platform.get_available_profiles();
            let mut target_acpi = profile.platform_profile.clone();
            if !available.contains(&target_acpi) {
                if target_acpi == "low-power" && available.contains(&"quiet".to_string()) {
                    target_acpi = "quiet".to_string();
                } else if target_acpi == "quiet" && available.contains(&"low-power".to_string()) {
                    target_acpi = "low-power".to_string();
                } else if target_acpi == "performance"
                    && available.contains(&"balanced-performance".to_string())
                {
                    target_acpi = "balanced-performance".to_string();
                }
            }

            if available.contains(&target_acpi) {
                let _ = self.platform.set_profile(&target_acpi);
            }
        }

        // 2. CPU PL1/PL2 limits (Enhanced tuning: graceful degradation if RAPL is missing/read-only)
        let cpu_status_msg = match self.cpu.set_power_limits(profile.cpu_pl1_w, profile.cpu_pl2_w) {
            Ok(()) => None,
            Err(MechrevoError::HardwareInterface(reason)) => {
                // AMD 平台或无 RAPL 权限：降级由 EC ACPI profile 统领
                Some(format!("CPU RAPL unavailable ({}), managed by ACPI", reason))
            }
            Err(err @ MechrevoError::InvalidParameter(_)) => {
                // 逻辑校验失败不可降级，必须阻断
                return Err(err);
            }
            Err(e) => Some(format!("CPU RAPL warning: {}", e)),
        };

        // 2.1 CPU Energy Performance Preference (EPP) HWP 调频策略联动
        let epp_pref = match profile.name.to_lowercase().as_str() {
            "turbo" => "performance",
            "office" => "power",
            _ => "balance_performance",
        };
        let _ = self.cpu.set_energy_performance_preference(epp_pref);

        // 3. GPU TGP limits & persistence mode toggle
        if self.gpu.is_available() {
            let enable_pm = profile.name.to_lowercase() != "office";
            let _ = self
                .gpu
                .set_power_limit(profile.gpu_tgp_w, enable_pm);
        }

        let mut msg = match cpu_status_msg {
            Some(detail) => format!(
                "Successfully applied profile '{}' ({}) [{}]",
                profile.name, profile.display_name, detail
            ),
            None => format!(
                "Successfully applied profile '{}' ({})",
                profile.name, profile.display_name
            ),
        };

        // 4. 硬件散热曲线失步预警 (Thermal Desynchronization Guard)
        // 若内核未加载 ACPI platform_profile 模块，EC 硬件仍运行在默认温控表 (M2T1 保守曲线)。
        // 在 Turbo 满血模式下，向用户如实警示风扇曲线未同步，避免重负载撞温控墙。
        if !self.platform.is_available() && profile.name.eq_ignore_ascii_case("turbo") {
            msg.push_str(" [Note: ACPI platform_profile absent in kernel; EC fan curve not synchronized to Turbo. Consider 'mechrevo-ctl fan-boost on' or tuxedo-drivers-dkms under heavy load]");
        }

        Ok(ProfileApplyResult {
            is_success: true,
            message: msg,
            applied_profile: profile,
        })
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_acpi_platform_profile() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let acpi_dir = ctx.resolve("/sys/firmware/acpi");
        std::fs::create_dir_all(&acpi_dir).unwrap();
        std::fs::write(acpi_dir.join("platform_profile"), "balanced\n").unwrap();
        std::fs::write(
            acpi_dir.join("platform_profile_choices"),
            "low-power balanced performance\n",
        )
        .unwrap();

        let driver = AcpiPlatformDriver::new(ctx.clone());
        assert!(driver.is_available());
        assert_eq!(driver.get_current_profile().unwrap(), "balanced");
        assert_eq!(
            driver.get_available_profiles(),
            vec!["low-power", "balanced", "performance"]
        );

        driver.set_profile("performance").unwrap();
        let updated = std::fs::read_to_string(acpi_dir.join("platform_profile")).unwrap();
        assert_eq!(updated.trim(), "performance");
    }

    #[test]
    fn test_fan_boost_decoupled_from_profile_manager() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup RAPL
        let rapl_base = ctx.resolve(crate::core::cpu::RAPL_BASE_PATH);
        std::fs::create_dir_all(&rapl_base).unwrap();
        std::fs::write(rapl_base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(rapl_base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();

        // Setup Fan Boost sysfs node
        let fan_node = ctx.resolve("/sys/devices/platform/tuxedo_keyboard/fan_boost");
        std::fs::create_dir_all(fan_node.parent().unwrap()).unwrap();
        std::fs::write(&fan_node, "0\n").unwrap();

        let cpu = IntelRaplDriver::new(ctx.clone());
        let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
        let platform = AcpiPlatformDriver::new(ctx.clone());
        let fan = FanDriver::new(ctx.clone());
        let mgr = ProfileManager::new(cpu, gpu, platform, fan);

        // Core Requirement: Switching to turbo MUST NOT force fan_boost to 1.
        // It must rely on EC firmware's dynamic thermal table (M3T1).
        mgr.apply_profile("turbo").unwrap();
        let fan_state = std::fs::read_to_string(&fan_node).unwrap();
        assert_eq!(
            fan_state.trim(),
            "0",
            "Turbo profile must NOT lock fan_boost to 1; EC dynamic thermal table manages fans"
        );

        // Switching to office also must not alter fan_boost state
        mgr.apply_profile("office").unwrap();
        let fan_state = std::fs::read_to_string(&fan_node).unwrap();
        assert_eq!(fan_state.trim(), "0");
    }

    #[test]
    fn test_fan_driver_set_fan_boost_missing_node_errors() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        // No fan boost node created
        let fan = FanDriver::new(ctx);
        let res = fan.set_fan_boost(true);
        assert!(
            res.is_err(),
            "set_fan_boost must return Err(HardwareInterface) if no candidate node exists"
        );
    }

    #[test]
    fn test_acpi_platform_dmi_product_name() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let dmi_node = ctx.resolve("/sys/class/dmi/id/product_name");
        std::fs::create_dir_all(dmi_node.parent().unwrap()).unwrap();
        std::fs::write(&dmi_node, "GM6IQ0C\n").unwrap();

        let driver = AcpiPlatformDriver::new(ctx);
        assert_eq!(driver.get_dmi_product_name().as_deref(), Some("GM6IQ0C"));
    }

    #[test]
    fn test_profile_manager_graceful_degradation_without_rapl() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup ACPI platform_profile
        let acpi_dir = ctx.resolve("/sys/firmware/acpi");
        std::fs::create_dir_all(&acpi_dir).unwrap();
        std::fs::write(acpi_dir.join("platform_profile"), "balanced\n").unwrap();
        std::fs::write(
            acpi_dir.join("platform_profile_choices"),
            "low-power balanced performance\n",
        )
        .unwrap();

        // Note: We DO NOT setup RAPL (simulating AMD CPU or locked BIOS/lockdown)
        let cpu = IntelRaplDriver::new(ctx.clone());
        let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
        let platform = AcpiPlatformDriver::new(ctx.clone());
        let fan = FanDriver::new(ctx.clone());
        let mgr = ProfileManager::new(cpu, gpu, platform, fan);

        // Applying profile should succeed via graceful degradation of RAPL
        let res = mgr.apply_profile("turbo").unwrap();
        assert!(res.is_success);
        let updated = std::fs::read_to_string(acpi_dir.join("platform_profile")).unwrap();
        assert_eq!(updated.trim(), "performance");
    }

    #[test]
    fn test_profile_manager_links_cpu_epp() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock cpufreq policy0
        let policy0 = ctx.resolve("/sys/devices/system/cpu/cpufreq/policy0");
        std::fs::create_dir_all(&policy0).unwrap();
        std::fs::write(policy0.join("energy_performance_preference"), "balance_power\n").unwrap();

        // Setup ACPI platform_profile
        let acpi_dir = ctx.resolve("/sys/firmware/acpi");
        std::fs::create_dir_all(&acpi_dir).unwrap();
        std::fs::write(acpi_dir.join("platform_profile"), "balanced\n").unwrap();
        std::fs::write(
            acpi_dir.join("platform_profile_choices"),
            "low-power balanced performance\n",
        )
        .unwrap();

        let cpu = IntelRaplDriver::new(ctx.clone());
        let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
        let platform = AcpiPlatformDriver::new(ctx.clone());
        let fan = FanDriver::new(ctx.clone());
        let mgr = ProfileManager::new(cpu, gpu, platform, fan);

        // Apply turbo -> EPP must be "performance"
        mgr.apply_profile("turbo").unwrap();
        let epp_turbo = std::fs::read_to_string(policy0.join("energy_performance_preference")).unwrap();
        assert_eq!(epp_turbo.trim(), "performance");

        // Apply office -> EPP must be "power"
        mgr.apply_profile("office").unwrap();
        let epp_office = std::fs::read_to_string(policy0.join("energy_performance_preference")).unwrap();
        assert_eq!(epp_office.trim(), "power");
    }

    #[test]
    fn test_fan_driver_hwmon_snapshot_cpu_and_fans() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock coretemp hwmon0
        let hwmon0 = ctx.resolve("/sys/class/hwmon/hwmon0");
        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::write(hwmon0.join("name"), "coretemp\n").unwrap();
        std::fs::write(hwmon0.join("temp1_input"), "58000\n").unwrap();

        // Setup mock uniwill_laptop hwmon1 with fans
        let hwmon1 = ctx.resolve("/sys/class/hwmon/hwmon1");
        std::fs::create_dir_all(&hwmon1).unwrap();
        std::fs::write(hwmon1.join("name"), "uniwill_laptop\n").unwrap();
        std::fs::write(hwmon1.join("fan1_input"), "2800\n").unwrap();
        std::fs::write(hwmon1.join("fan2_input"), "3100\n").unwrap();

        let fan = FanDriver::new(ctx);
        let snapshot = fan.get_hwmon_snapshot();
        assert_eq!(snapshot.cpu_temp_c, Some(58));
        assert_eq!(snapshot.fan1_rpm, Some(2800));
        assert_eq!(snapshot.fan2_rpm, Some(3100));
    }

    #[test]
    fn test_fan_driver_get_and_set_fan_boost() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        let fan_node = ctx.resolve("/sys/devices/platform/tuxedo_keyboard/fan_boost");
        std::fs::create_dir_all(fan_node.parent().unwrap()).unwrap();
        std::fs::write(&fan_node, "0\n").unwrap();

        let fan = FanDriver::new(ctx);
        assert_eq!(fan.get_fan_boost(), Some(false));

        fan.set_fan_boost(true).unwrap();
        assert_eq!(fan.get_fan_boost(), Some(true));

        let updated = std::fs::read_to_string(&fan_node).unwrap();
        assert_eq!(updated.trim(), "1");
    }

    #[test]
    fn test_fan_driver_detects_inou0000_candidate() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        let fan_node = ctx.resolve("/sys/devices/platform/INOU0000:00/fan_boost");
        std::fs::create_dir_all(fan_node.parent().unwrap()).unwrap();
        std::fs::write(&fan_node, "1\n").unwrap();

        let fan = FanDriver::new(ctx);
        assert_eq!(
            fan.find_fan_boost_node(),
            Some("/sys/devices/platform/INOU0000:00/fan_boost".to_string())
        );
        assert_eq!(fan.get_fan_boost(), Some(true));
    }

    #[test]
    fn test_fan_driver_hwmon_fallback_to_uniwill_cpu_temp() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock uniwill_laptop hwmon WITHOUT coretemp
        let hwmon0 = ctx.resolve("/sys/class/hwmon/hwmon0");
        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::write(hwmon0.join("name"), "uniwill_laptop\n").unwrap();
        std::fs::write(hwmon0.join("temp1_input"), "62000\n").unwrap();
        std::fs::write(hwmon0.join("fan1_input"), "2500\n").unwrap();

        let fan = FanDriver::new(ctx);
        let snapshot = fan.get_hwmon_snapshot();
        assert_eq!(
            snapshot.cpu_temp_c,
            Some(62),
            "Should fallback to platform driver temp1_input when coretemp is absent"
        );
        assert_eq!(snapshot.fan1_rpm, Some(2500));
    }

    #[test]
    fn test_profile_manager_bidirectional_acpi_profile_fallback() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup ACPI platform_profile choices: quiet balanced balanced-performance (no "low-power" and no "performance")
        let acpi_dir = ctx.resolve("/sys/firmware/acpi");
        std::fs::create_dir_all(&acpi_dir).unwrap();
        std::fs::write(acpi_dir.join("platform_profile"), "balanced\n").unwrap();
        std::fs::write(
            acpi_dir.join("platform_profile_choices"),
            "quiet balanced balanced-performance\n",
        )
        .unwrap();

        let cpu = IntelRaplDriver::new(ctx.clone());
        let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
        let platform = AcpiPlatformDriver::new(ctx.clone());
        let fan = FanDriver::new(ctx.clone());
        let mgr = ProfileManager::new(cpu, gpu, platform, fan);

        // office has platform_profile: "low-power" -> should map to "quiet"
        mgr.apply_profile("office").unwrap();
        let curr = std::fs::read_to_string(acpi_dir.join("platform_profile")).unwrap();
        assert_eq!(curr.trim(), "quiet");

        // turbo has platform_profile: "performance" -> should map to "balanced-performance"
        mgr.apply_profile("turbo").unwrap();
        let curr = std::fs::read_to_string(acpi_dir.join("platform_profile")).unwrap();
        assert_eq!(curr.trim(), "balanced-performance");
    }

    #[test]
    fn test_profile_manager_warns_thermal_desync_when_acpi_absent_in_turbo() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup RAPL only, NO ACPI platform_profile
        let rapl_base = ctx.resolve(crate::core::cpu::RAPL_BASE_PATH);
        std::fs::create_dir_all(&rapl_base).unwrap();
        std::fs::write(rapl_base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(rapl_base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();

        let cpu = IntelRaplDriver::new(ctx.clone());
        let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
        let platform = AcpiPlatformDriver::new(ctx.clone());
        let fan = FanDriver::new(ctx.clone());
        let mgr = ProfileManager::new(cpu, gpu, platform, fan);

        let res = mgr.apply_profile("turbo").unwrap();
        assert!(res.is_success);
        assert!(
            res.message.contains("EC fan curve not synchronized"),
            "Message must alert user that EC fan curve is not synchronized when ACPI node is absent: got '{}'",
            res.message
        );
    }
}



