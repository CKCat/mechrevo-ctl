//! Background daemon for hardware monitoring, uevents, and IPC.

use serde::{Deserialize, Serialize};

use crate::core::battery::BatteryDriver;
use crate::core::cpu::IntelRaplDriver;
use crate::core::gpu::NvidiaSmiDriver;
use crate::core::keyboard::KeyboardDriver;
use crate::core::platform::{AcpiPlatformDriver, FanDriver, ProfileManager};
use crate::hal::SysfsContext;

pub const DEFAULT_SOCKET_PATH: &str = "/run/mechrevo-ctl.sock";
pub const AC_ONLINE_NODE: &str = "/sys/class/power_supply/AC/online";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum IpcRequest {
    Profile { mode: String },
    Battery { threshold: u32 },
    Keyboard { brightness: u32 },
    FanBoost { enable: bool },
    ReloadProfile,
    Status,
}

pub const DEFAULT_STATE_FILE_PATH: &str = "/var/lib/mechrevo/state.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct DaemonState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charge_threshold: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_profile: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct MechrevoDaemon {
    ctx: SysfsContext,
    socket_path: String,
    state_file_path: Option<String>,
    profile_mgr: ProfileManager,
    battery_drv: BatteryDriver,
    keyboard_drv: KeyboardDriver,
    fan_drv: FanDriver,
    pub last_ac_state: Option<bool>,
    pub manual_profile_override: Option<String>,
    pub last_active_profile: String,
    pub persisted_charge_threshold: Option<u32>,
}

impl MechrevoDaemon {
    pub fn new(ctx: SysfsContext, socket_path: Option<String>) -> Self {
        let cpu = IntelRaplDriver::new(ctx.clone());
        let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
        let platform = AcpiPlatformDriver::new(ctx.clone());
        let fan = FanDriver::new(ctx.clone());
        let profile_mgr = ProfileManager::new(cpu, gpu, platform, fan.clone());
        let battery_drv = BatteryDriver::new(ctx.clone());
        let keyboard_drv = KeyboardDriver::new(ctx.clone());

        let mut daemon = Self {
            ctx,
            socket_path: socket_path.unwrap_or_else(|| DEFAULT_SOCKET_PATH.to_string()),
            state_file_path: Some(DEFAULT_STATE_FILE_PATH.to_string()),
            profile_mgr,
            battery_drv,
            keyboard_drv,
            fan_drv: fan,
            last_ac_state: None,
            manual_profile_override: None,
            last_active_profile: "balanced".to_string(),
            persisted_charge_threshold: None,
        };

        // Automatically restore persisted state on startup (fixes volatile EC reset on cold boot)
        daemon.load_and_restore_state();
        daemon
    }

    pub fn state_file_path(&self) -> Option<&str> {
        self.state_file_path.as_deref()
    }

    pub fn set_state_file_path(&mut self, path: Option<String>) {
        self.state_file_path = path;
    }

    /// Restores persisted charge threshold and manual profile lock from disk.
    pub fn load_and_restore_state(&mut self) {
        if let Some(ref path_str) = self.state_file_path {
            let target = self.ctx.resolve(path_str);
            if target.exists() {
                if let Ok(content) = std::fs::read_to_string(&target) {
                    if let Ok(state) = serde_json::from_str::<DaemonState>(&content) {
                        if let Some(threshold) = state.charge_threshold {
                            println!("[+] Restored battery threshold from disk: {}%", threshold);
                            self.persisted_charge_threshold = Some(threshold);
                            let _ = self.battery_drv.set_charge_threshold(threshold);
                        }
                        if let Some(ref profile) = state.manual_profile {
                            println!("[+] Restored manual profile lock from disk: {}", profile);
                            self.manual_profile_override = Some(profile.clone());
                            self.last_active_profile = profile.clone();
                        }
                    }
                }
            }
        }
    }

    /// Saves the current charge threshold and profile configuration to disk.
    pub fn save_state(&self) {
        if let Some(ref path_str) = self.state_file_path {
            let target = self.ctx.resolve(path_str);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let current_threshold = self
                .persisted_charge_threshold
                .or_else(|| self.battery_drv.get_battery_status().map(|s| s.charge_threshold).ok());
            let state = DaemonState {
                charge_threshold: current_threshold,
                manual_profile: self.manual_profile_override.clone(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&state) {
                let _ = std::fs::write(&target, json);
            }
        }
    }

    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    /// Checks if AC power adapter is currently connected.
    /// Universally probes /sys/class/power_supply for all external power supplies
    /// (Mains barrel jack, USB Type-C PD charger, etc.), preventing Type-C PD charger
    /// from being falsely treated as battery discharging mode.
    pub fn is_ac_online(&self) -> bool {
        let base = self.ctx.resolve(crate::core::battery::POWER_SUPPLY_BASE);
        let mut found_external_supply = false;

        if base.exists() {
            if let Ok(entries) = std::fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    let dev_path = format!("{}/{}", crate::core::battery::POWER_SUPPLY_BASE, name_str);
                    let type_node = format!("{}/type", dev_path);
                    let online_node = format!("{}/online", dev_path);

                    if let Ok(type_val) = self.ctx.read_text(&type_node) {
                        let clean_type = type_val.trim().to_uppercase();
                        // 任何非 Battery 的供电设备（Mains, USB, USB_TYPE_C, USB_PD, UPS, Wireless）均为外接电源
                        if !clean_type.contains("BATTERY") {
                            found_external_supply = true;
                            if let Ok(online_val) = self.ctx.read_text(&online_node) {
                                if online_val.trim() == "1" {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 仅当明确探测到外接电源设备且全部为离线 (0) 时，才确信处于纯电池模式
        if found_external_supply {
            return false;
        }

        // Fallback 1: Legacy hardcoded node
        if self.ctx.exists(AC_ONLINE_NODE) {
            if let Ok(val) = self.ctx.read_text(AC_ONLINE_NODE) {
                return val.trim() == "1";
            }
        }

        // Fallback 2: Battery status heuristic
        if let Ok(status) = self.battery_drv.get_battery_status() {
            let lower = status.status.to_lowercase();
            if lower.contains("charging") || lower.contains("full") {
                return true;
            }
            if lower.contains("discharging") {
                return false;
            }
            // If status is "not charging", check whether battery capacity reached charge threshold
            if lower.contains("not charging") && status.capacity_percent + 2 >= status.charge_threshold {
                return true;
            }
        }

        true
    }

    /// Generates a complete, human-readable hardware status summary as root.
    /// Provides zero-privilege desktop clients with transparent access despite PLATYPUS permissions.
    pub fn get_system_status_summary(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(out, "==================================================");
        let _ = writeln!(out, "      MECHREVO Control Center - System Status");
        let _ = writeln!(out, "==================================================");

        // 0. Hardware Chassis
        let platform_drv = AcpiPlatformDriver::new(self.ctx.clone());
        if let Some(chassis) = platform_drv.get_dmi_product_name() {
            let _ = writeln!(out, "[*] Hardware Chassis: {}", chassis);
        }

        // 1. CPU
        let cpu_drv = IntelRaplDriver::new(self.ctx.clone());
        let hwmon = self.fan_drv.get_hwmon_snapshot();
        match cpu_drv.get_power_limits() {
            Ok(limits) => {
                let temp_str = hwmon
                    .cpu_temp_c
                    .map(|t| format!(" | Temp: {}°C", t))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "[*] Intel RAPL (CPU): PL1 = {:.0}W | PL2 = {:.0}W{}",
                    limits.pl1_w, limits.pl2_w, temp_str
                );
            }
            Err(e) => {
                let temp_str = hwmon
                    .cpu_temp_c
                    .map(|t| format!(" (Temp: {}°C)", t))
                    .unwrap_or_default();
                let _ = writeln!(out, "[-] Intel RAPL (CPU): Unavailable ({}){}", e, temp_str);
            }
        }
        let epp_nodes = cpu_drv.find_epp_nodes();
        if let Some(first_epp) = epp_nodes.first() {
            if let Ok(curr_epp) = self.ctx.read_text(first_epp) {
                let _ = writeln!(out, "    Energy Perf Preference (EPP): {}", curr_epp);
            }
        }

        // 2. GPU
        let gpu_drv = NvidiaSmiDriver::default().with_ctx(self.ctx.clone());
        if gpu_drv.is_available() {
            match gpu_drv.get_gpu_info() {
                Ok(info) if info.is_suspended => {
                    let _ = writeln!(out, "[*] NVIDIA dGPU: {}", info.name);
                    let db_str = match info.dynamic_boost_active {
                        Some(true) => " | Dynamic Boost: [nvidia-powerd active]",
                        Some(false) => " | Dynamic Boost: [nvidia-powerd inactive]",
                        None => "",
                    };
                    let _ = writeln!(
                        out,
                        "    Power State: Suspended (PCIe RTD3 D3cold - Zero Wakeup){}",
                        db_str
                    );
                }
                Ok(info) => {
                    let _ = writeln!(out, "[*] NVIDIA dGPU: {}", info.name);
                    let tgp_str = match info.dynamic_boost_active {
                        Some(true) => format!(
                            "    TGP Limit: {:.0}W (115W Base + 25W Dynamic Boost [nvidia-powerd active])",
                            info.current_power_limit_w
                        ),
                        Some(false) => format!(
                            "    TGP Limit: {:.0}W (Base TGP; 25W Dynamic Boost inactive, run 'systemctl start nvidia-powerd')",
                            info.current_power_limit_w
                        ),
                        None => format!(
                            "    TGP Limit: {:.0}W (Max: {:.0}W)",
                            info.current_power_limit_w, info.max_power_limit_w
                        ),
                    };
                    let _ = writeln!(out, "{}", tgp_str);
                    let _ = writeln!(out, "    Temperature: {}°C", info.temperature_c);
                }
                Err(e) => {
                    let _ = writeln!(out, "[-] NVIDIA dGPU: Query error ({})", e);
                }
            }
        } else {
            let _ = writeln!(out, "[-] NVIDIA dGPU: Not available or driver not loaded");
        }

        // 3. ACPI Platform Profile
        match platform_drv.get_current_profile() {
            Ok(profile) => {
                let _ = writeln!(out, "[*] ACPI Platform Profile: {}", profile);
            }
            Err(_) => {
                let _ = writeln!(
                    out,
                    "[-] ACPI Platform Profile: Not available in kernel (Active software profile: '{}', managed via native Intel RAPL & EPP; Note: EC fan curve not synchronized, consider fan-boost or tuxedo-drivers-dkms under heavy load)",
                    self.last_active_profile
                );
            }
        }

        // 4. Battery
        match self.battery_drv.get_battery_status() {
            Ok(status) => {
                let _ = writeln!(
                    out,
                    "[*] Battery: {}% ({}) | Threshold Limit: {}%",
                    status.capacity_percent, status.status, status.charge_threshold
                );
            }
            Err(e) => {
                let _ = writeln!(out, "[-] Battery: Unavailable ({})", e);
            }
        }

        // 5. Keyboard
        match self.keyboard_drv.get_brightness_percent() {
            Ok(brightness) => {
                let _ = writeln!(out, "[*] Keyboard Backlight: {}%", brightness);
            }
            Err(e) => {
                let _ = writeln!(out, "[-] Keyboard Backlight: Unavailable ({})", e);
            }
        }

        // 6. Cooling & Fans (hwmon + Fan Boost status)
        let boost_str = match self.fan_drv.get_fan_boost() {
            Some(true) => "Boost: ON",
            Some(false) => "Boost: OFF",
            None => "Boost: N/A",
        };
        let fan_rpm_str = match (hwmon.fan1_rpm, hwmon.fan2_rpm) {
            (Some(f1), Some(f2)) => format!("Fan 1: {} RPM | Fan 2: {} RPM", f1, f2),
            (Some(f1), None) => format!("Fan 1: {} RPM", f1),
            (None, Some(f2)) => format!("Fan 2: {} RPM", f2),
            (None, None) => "Managed by EC firmware".to_string(),
        };
        let _ = writeln!(out, "[*] Cooling (Fans): {} ({})", fan_rpm_str, boost_str);

        let _ = write!(out, "==================================================");
        out
    }


    /// Evaluates AC state transition and applies power profile safely.
    pub fn evaluate_ac_state(&mut self) {
        let ac_online = self.is_ac_online();
        if self.last_ac_state != Some(ac_online) {
            self.last_ac_state = Some(ac_online);
            if ac_online {
                let target = self
                    .manual_profile_override
                    .clone()
                    .unwrap_or_else(|| "balanced".to_string());
                println!("[*] AC adapter connected -> Applying {} profile", target);
                let _ = self.profile_mgr.apply_profile(&target);
                self.last_active_profile = target;
            } else {
                println!("[*] On battery power -> Applying office power saving profile");
                let _ = self.profile_mgr.apply_profile("office");
                self.last_active_profile = "office".to_string();
            }
        }
    }

    /// Processes an incoming JSON payload string and produces an IPC response.
    pub fn handle_ipc_request(&mut self, payload: &str) -> String {
        let req: Result<IpcRequest, serde_json::Error> = serde_json::from_str(payload);
        let response = match req {
            Ok(IpcRequest::Profile { mode }) => match self.profile_mgr.apply_profile(&mode) {
                Ok(res) => {
                    self.manual_profile_override = Some(mode.clone());
                    self.last_active_profile = mode;
                    self.save_state();
                    IpcResponse {
                        success: true,
                        message: Some(res.message),
                        error: None,
                    }
                }
                Err(e) => IpcResponse {
                    success: false,
                    message: None,
                    error: Some(e.to_string()),
                },
            },
            Ok(IpcRequest::Battery { threshold }) => {
                match self.battery_drv.set_charge_threshold(threshold) {
                    Ok(()) => {
                        self.persisted_charge_threshold = Some(threshold);
                        self.save_state();
                        IpcResponse {
                            success: true,
                            message: Some(format!(
                                "Battery charge threshold set to {}% (Note: If battery continues charging past limit, toggle conservation mode once in BIOS/Windows to initialize EC register 0x087F)",
                                threshold
                            )),
                            error: None,
                        }
                    }
                    Err(e) => IpcResponse {
                        success: false,
                        message: None,
                        error: Some(e.to_string()),
                    },
                }
            }
            Ok(IpcRequest::Keyboard { brightness }) => {
                match self.keyboard_drv.set_brightness_percent(brightness) {
                    Ok(()) => IpcResponse {
                        success: true,
                        message: Some(format!("Keyboard brightness set to {}%", brightness)),
                        error: None,
                    },
                    Err(e) => IpcResponse {
                        success: false,
                        message: None,
                        error: Some(e.to_string()),
                    },
                }
            }
            Ok(IpcRequest::FanBoost { enable }) => {
                match self.fan_drv.set_fan_boost(enable) {
                    Ok(()) => {
                        let state_str = if enable {
                            "enabled (maximum cooling)"
                        } else {
                            "disabled (automatic thermal curve)"
                        };
                        IpcResponse {
                            success: true,
                            message: Some(format!("Fan boost {}", state_str)),
                            error: None,
                        }
                    }
                    Err(e) => IpcResponse {
                        success: false,
                        message: None,
                        error: Some(e.to_string()),
                    },
                }
            }
            Ok(IpcRequest::ReloadProfile) => {
                // 强制重新探测当前真实的 AC 供电状态 (解决合盖休眠期间拔除或插入电源问题)
                let ac_online = self.is_ac_online();
                self.last_ac_state = Some(ac_online);
                let target = if ac_online {
                    self.manual_profile_override
                        .clone()
                        .unwrap_or_else(|| self.last_active_profile.clone())
                } else {
                    // 电池供电下强制降级为 office 节能模式，保护电池避免过流脱扣与快速耗尽
                    "office".to_string()
                };

                match self.profile_mgr.apply_profile(&target) {
                    Ok(_) => {
                        self.last_active_profile = target.clone();
                        // 工业级自愈：休眠唤醒后 EC 易丢失寄存器配置，强制重放电池阈值
                        let bat_rearm_msg = if let Some(thresh) = self.persisted_charge_threshold {
                            match self.battery_drv.set_charge_threshold(thresh) {
                                Ok(()) => format!(" (Battery threshold re-armed: {}%)", thresh),
                                Err(e) => format!(" (Battery re-arm failed: {})", e),
                            }
                        } else {
                            String::new()
                        };
                        self.save_state();
                        IpcResponse {
                            success: true,
                            message: Some(format!("Re-armed profile: {}{}", target, bat_rearm_msg)),
                            error: None,
                        }
                    }
                    Err(e) => IpcResponse {
                        success: false,
                        message: None,
                        error: Some(e.to_string()),
                    },
                }
            }
            Ok(IpcRequest::Status) => {
                let report = self.get_system_status_summary();
                IpcResponse {
                    success: true,
                    message: Some(report),
                    error: None,
                }
            }

            Err(e) => IpcResponse {
                success: false,
                message: None,
                error: Some(format!("Invalid IPC JSON request: {}", e)),
            },
        };

        serde_json::to_string(&response).unwrap_or_else(|_| {
            r#"{"success":false,"error":"Serialization error"}"#.to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_daemon_ipc_profile_and_battery() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock environment
        let rapl_base = ctx.resolve(crate::core::cpu::RAPL_BASE_PATH);
        std::fs::create_dir_all(&rapl_base).unwrap();
        std::fs::write(rapl_base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(rapl_base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();

        let bat_base = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_base).unwrap();
        std::fs::write(bat_base.join("capacity"), "80\n").unwrap();
        std::fs::write(bat_base.join("status"), "Discharging\n").unwrap();
        std::fs::write(bat_base.join("charge_control_end_threshold"), "100\n").unwrap();

        let mut daemon = MechrevoDaemon::new(ctx, None);

        // 1. Profile command
        let res_str = daemon.handle_ipc_request(r#"{"action":"profile","mode":"turbo"}"#);
        let resp: IpcResponse = serde_json::from_str(&res_str).unwrap();
        assert!(resp.success);
        assert_eq!(daemon.last_active_profile, "turbo");
        assert_eq!(daemon.manual_profile_override.as_deref(), Some("turbo"));

        // 2. Battery command
        let res_str = daemon.handle_ipc_request(r#"{"action":"battery","threshold":80}"#);
        let resp: IpcResponse = serde_json::from_str(&res_str).unwrap();
        assert!(resp.success);
        assert!(
            resp.message.as_ref().unwrap().contains("0x087F"),
            "Battery response must provide EC register 0x087F firmware guidance"
        );

        // 3. Invalid command
        let res_str = daemon.handle_ipc_request(r#"{"action":"unknown"}"#);
        let resp: IpcResponse = serde_json::from_str(&res_str).unwrap();
        assert!(!resp.success);
    }

    #[test]
    fn test_daemon_user_profile_lock() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        let ac_node = ctx.resolve(AC_ONLINE_NODE);
        std::fs::create_dir_all(ac_node.parent().unwrap()).unwrap();
        std::fs::write(&ac_node, "1\n").unwrap();

        let rapl_base = ctx.resolve(crate::core::cpu::RAPL_BASE_PATH);
        std::fs::create_dir_all(&rapl_base).unwrap();
        std::fs::write(rapl_base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(rapl_base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();

        let mut daemon = MechrevoDaemon::new(ctx.clone(), None);
        daemon.evaluate_ac_state();
        assert_eq!(daemon.last_active_profile, "balanced");

        // User locks turbo
        daemon.handle_ipc_request(r#"{"action":"profile","mode":"turbo"}"#);
        assert_eq!(daemon.last_active_profile, "turbo");

        // Disconnect AC
        std::fs::write(&ac_node, "0\n").unwrap();
        daemon.evaluate_ac_state();
        assert_eq!(daemon.last_active_profile, "office");

        // Reconnect AC -> Must restore locked turbo profile!
        std::fs::write(&ac_node, "1\n").unwrap();
        daemon.evaluate_ac_state();
        assert_eq!(daemon.last_active_profile, "turbo");
    }

    #[test]
    fn test_daemon_dynamic_ac_detection_mains() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // AC adapter named ACAD with type=Mains instead of AC
        let acad_dir = ctx.resolve("/sys/class/power_supply/ACAD");
        std::fs::create_dir_all(&acad_dir).unwrap();
        std::fs::write(acad_dir.join("type"), "Mains\n").unwrap();
        std::fs::write(acad_dir.join("online"), "1\n").unwrap();

        let daemon = MechrevoDaemon::new(ctx.clone(), None);
        assert!(daemon.is_ac_online(), "ACAD with type=Mains and online=1 should be detected as online");

        std::fs::write(acad_dir.join("online"), "0\n").unwrap();
        assert!(!daemon.is_ac_online(), "ACAD with online=0 should be detected as offline");
    }

    #[test]
    fn test_daemon_battery_not_charging_heuristic() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // No AC node present, only battery with conservation mode active
        let bat_base = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_base).unwrap();
        std::fs::write(bat_base.join("capacity"), "80\n").unwrap();
        std::fs::write(bat_base.join("status"), "Not charging\n").unwrap();
        std::fs::write(bat_base.join("charge_control_end_threshold"), "80\n").unwrap();

        let daemon = MechrevoDaemon::new(ctx.clone(), None);
        assert!(
            daemon.is_ac_online(),
            "Battery 'Not charging' at configured threshold (80%) must be detected as AC connected"
        );
    }

    #[test]
    fn test_daemon_cold_boot_persistence_recovery() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // 1. Setup mock battery with 100% threshold simulating volatile EC reset upon power cycle
        let bat_base = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_base).unwrap();
        std::fs::write(bat_base.join("capacity"), "50\n").unwrap();
        std::fs::write(bat_base.join("status"), "Charging\n").unwrap();
        std::fs::write(bat_base.join("charge_control_end_threshold"), "100\n").unwrap();

        // 2. Setup mock AC adapter
        let ac_node = ctx.resolve(AC_ONLINE_NODE);
        std::fs::create_dir_all(ac_node.parent().unwrap()).unwrap();
        std::fs::write(&ac_node, "1\n").unwrap();

        // 3. Write persisted state file
        let state_file = ctx.resolve(DEFAULT_STATE_FILE_PATH);
        std::fs::create_dir_all(state_file.parent().unwrap()).unwrap();
        std::fs::write(&state_file, r#"{"charge_threshold": 80, "manual_profile": "turbo"}"#).unwrap();

        // 4. Instantiate daemon with state recovery
        let mut daemon = MechrevoDaemon::new(ctx.clone(), None);
        daemon.load_and_restore_state();

        // Assert: Battery threshold must be re-applied to EC sysfs node!
        let end_threshold = std::fs::read_to_string(bat_base.join("charge_control_end_threshold")).unwrap();
        assert_eq!(end_threshold.trim(), "80", "Cold boot must re-arm battery threshold to 80%");

        // Assert: Manual profile must be restored
        assert_eq!(daemon.manual_profile_override.as_deref(), Some("turbo"));
    }

    #[test]
    fn test_daemon_reload_profile_rearms_battery_threshold() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock battery
        let bat_base = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_base).unwrap();
        std::fs::write(bat_base.join("capacity"), "50\n").unwrap();
        std::fs::write(bat_base.join("status"), "Charging\n").unwrap();
        std::fs::write(bat_base.join("charge_control_end_threshold"), "100\n").unwrap();

        // Setup mock AC
        let ac_node = ctx.resolve(AC_ONLINE_NODE);
        std::fs::create_dir_all(ac_node.parent().unwrap()).unwrap();
        std::fs::write(&ac_node, "1\n").unwrap();

        let mut daemon = MechrevoDaemon::new(ctx.clone(), None);

        // 1. User configures 80% threshold via IPC
        let resp_str = daemon.handle_ipc_request(r#"{"action":"battery","threshold":80}"#);
        assert!(resp_str.contains("\"success\":true"));
        assert_eq!(
            std::fs::read_to_string(bat_base.join("charge_control_end_threshold")).unwrap().trim(),
            "80"
        );

        // 2. Simulate suspend/resume: hardware EC volatile reset restores register to 100%
        std::fs::write(bat_base.join("charge_control_end_threshold"), "100\n").unwrap();

        // 3. Systemd sleep resume hook sends reload_profile IPC request
        let reload_resp = daemon.handle_ipc_request(r#"{"action":"reload_profile"}"#);
        assert!(reload_resp.contains("\"success\":true"));

        // 4. Verification: The reload_profile action MUST re-arm the battery threshold back to 80%!
        let end_threshold_after_resume = std::fs::read_to_string(bat_base.join("charge_control_end_threshold")).unwrap();
        assert_eq!(
            end_threshold_after_resume.trim(),
            "80",
            "Resume reload_profile MUST re-arm battery threshold after hardware EC volatile reset!"
        );
    }

    #[test]
    fn test_daemon_reload_profile_on_battery_forces_office_profile() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock RAPL
        let rapl_base = ctx.resolve(crate::core::cpu::RAPL_BASE_PATH);
        std::fs::create_dir_all(&rapl_base).unwrap();
        std::fs::write(rapl_base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(rapl_base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();

        // Setup AC node: offline (0) simulating unplugged while suspended
        let ac_node = ctx.resolve(AC_ONLINE_NODE);
        std::fs::create_dir_all(ac_node.parent().unwrap()).unwrap();
        std::fs::write(&ac_node, "0\n").unwrap();

        let mut daemon = MechrevoDaemon::new(ctx.clone(), None);
        // User had previously locked turbo mode while on AC
        daemon.manual_profile_override = Some("turbo".to_string());
        daemon.last_active_profile = "turbo".to_string();

        // ReloadProfile received upon resume in a coffee shop on battery
        let resp_str = daemon.handle_ipc_request(r#"{"action":"reload_profile"}"#);
        let resp: IpcResponse = serde_json::from_str(&resp_str).unwrap();
        assert!(resp.success);

        // Crucial safety guarantee: MUST be forced to office mode on battery!
        assert_eq!(
            daemon.last_active_profile, "office",
            "Waking up on battery MUST force office power-saving mode, even if turbo was set on AC"
        );
    }

    #[test]
    fn test_daemon_type_c_pd_charging_detected_as_online_when_mains_is_offline() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // 1. Primary DC Mains adapter is unplugged (e.g. ACAD with online=0)
        let acad_dir = ctx.resolve("/sys/class/power_supply/ACAD");
        std::fs::create_dir_all(&acad_dir).unwrap();
        std::fs::write(acad_dir.join("type"), "Mains\n").unwrap();
        std::fs::write(acad_dir.join("online"), "0\n").unwrap();

        // 2. USB Type-C PD charger is plugged in (e.g. tcpm-source-psy-0-0022 with type=USB_PD and online=1)
        let pd_dir = ctx.resolve("/sys/class/power_supply/tcpm-source-psy-0-0022");
        std::fs::create_dir_all(&pd_dir).unwrap();
        std::fs::write(pd_dir.join("type"), "USB_PD\n").unwrap();
        std::fs::write(pd_dir.join("online"), "1\n").unwrap();

        let daemon = MechrevoDaemon::new(ctx.clone(), None);

        // Core requirement: USB Type-C PD charger must be recognized as AC online!
        assert!(
            daemon.is_ac_online(),
            "USB_PD power supply with online=1 MUST be detected as online, even if DC Mains is offline (0)"
        );
    }

    #[test]
    fn test_daemon_ipc_status_request() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock environment
        let rapl_base = ctx.resolve(crate::core::cpu::RAPL_BASE_PATH);
        std::fs::create_dir_all(&rapl_base).unwrap();
        std::fs::write(rapl_base.join("constraint_0_power_limit_uw"), "75000000\n").unwrap();
        std::fs::write(rapl_base.join("constraint_1_power_limit_uw"), "90000000\n").unwrap();

        let bat_base = ctx.resolve("/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat_base).unwrap();
        std::fs::write(bat_base.join("capacity"), "80\n").unwrap();
        std::fs::write(bat_base.join("status"), "Charging\n").unwrap();
        std::fs::write(bat_base.join("charge_control_end_threshold"), "80\n").unwrap();

        let mut daemon = MechrevoDaemon::new(ctx, None);

        let res_str = daemon.handle_ipc_request(r#"{"action":"status"}"#);
        let resp: IpcResponse = serde_json::from_str(&res_str).unwrap();
        assert!(resp.success, "IPC status request must succeed");
        let msg = resp.message.expect("IPC status response must contain status report");
        assert!(msg.contains("PL1"), "Status message must report CPU PL1 power limit");
        assert!(msg.contains("Battery"), "Status message must report battery status");
    }

    #[test]
    fn test_daemon_ipc_fan_boost_and_status_hwmon() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock fan_boost node
        let fan_node = ctx.resolve("/sys/devices/platform/tuxedo_keyboard/fan_boost");
        std::fs::create_dir_all(fan_node.parent().unwrap()).unwrap();
        std::fs::write(&fan_node, "0\n").unwrap();

        // Setup mock hwmon with coretemp (CPU 54C) and fans (2400 / 2700 RPM)
        let hwmon0 = ctx.resolve("/sys/class/hwmon/hwmon0");
        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::write(hwmon0.join("name"), "coretemp\n").unwrap();
        std::fs::write(hwmon0.join("temp1_input"), "54000\n").unwrap();

        let hwmon1 = ctx.resolve("/sys/class/hwmon/hwmon1");
        std::fs::create_dir_all(&hwmon1).unwrap();
        std::fs::write(hwmon1.join("name"), "uniwill_laptop\n").unwrap();
        std::fs::write(hwmon1.join("fan1_input"), "2400\n").unwrap();
        std::fs::write(hwmon1.join("fan2_input"), "2700\n").unwrap();

        let mut daemon = MechrevoDaemon::new(ctx, None);

        // 1. Send FanBoost IPC enable
        let boost_on = daemon.handle_ipc_request(r#"{"action":"fan_boost","enable":true}"#);
        assert!(boost_on.contains("\"success\":true"));
        assert_eq!(std::fs::read_to_string(&fan_node).unwrap().trim(), "1");

        // 2. Query status: must contain CPU temperature and Fan RPMs and Boost status
        let status_res = daemon.handle_ipc_request(r#"{"action":"status"}"#);
        let resp: IpcResponse = serde_json::from_str(&status_res).unwrap();
        assert!(resp.success);
        let msg = resp.message.unwrap();
        assert!(msg.contains("54°C"), "Status must report CPU temperature (54°C)");
        assert!(msg.contains("2400 RPM"), "Status must report Fan 1 RPM");
        assert!(msg.contains("2700 RPM"), "Status must report Fan 2 RPM");
        assert!(msg.contains("Boost: ON"), "Status must report Fan Boost: ON");
    }

    #[test]
    fn test_daemon_status_reports_software_fallback_profile_when_acpi_node_absent() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Note: ACPI platform_profile node is NOT created in this mock environment
        let mut daemon = MechrevoDaemon::new(ctx, None);
        daemon.last_active_profile = "turbo".to_string();

        let res_str = daemon.handle_ipc_request(r#"{"action":"status"}"#);
        let resp: IpcResponse = serde_json::from_str(&res_str).unwrap();
        assert!(resp.success);
        let msg = resp.message.unwrap();

        // Must report transparent software fallback profile and RAPL management
        assert!(
            msg.contains("Active software profile: 'turbo'"),
            "Status report must transparently display active software profile when ACPI node is absent"
        );
        assert!(
            msg.contains("managed via native Intel RAPL & EPP"),
            "Status report must indicate RAPL & EPP management mode"
        );
        assert!(
            msg.contains("EC fan curve not synchronized"),
            "Status report must indicate EC fan curve is not synchronized when ACPI is absent"
        );
    }

    #[test]
    fn test_daemon_status_reports_gpu_dynamic_boost_status() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Setup mock PCI NVIDIA GPU
        let pci_dev = ctx.resolve("/sys/bus/pci/devices/0000:01:00.0");
        std::fs::create_dir_all(pci_dev.join("power")).unwrap();
        std::fs::write(pci_dev.join("vendor"), "0x10de\n").unwrap();
        std::fs::write(pci_dev.join("class"), "0x030000\n").unwrap();
        std::fs::write(pci_dev.join("power").join("runtime_status"), "suspended\n").unwrap();

        // 1. Without dynamic boost mock node -> inactive
        let daemon1 = MechrevoDaemon::new(ctx.clone(), None);
        let summary1 = daemon1.get_system_status_summary();
        assert!(summary1.contains("RTD3 D3cold"));

        // 2. With dynamic boost mock node -> active
        let mock_active = ctx.resolve(crate::core::gpu::DYNAMIC_BOOST_MOCK_NODE);
        std::fs::create_dir_all(mock_active.parent().unwrap()).unwrap();
        std::fs::write(&mock_active, "active\n").unwrap();

        let daemon2 = MechrevoDaemon::new(ctx.clone(), None);
        let summary2 = daemon2.get_system_status_summary();
        assert!(summary2.contains("nvidia-powerd active") || summary2.contains("Dynamic Boost"));
    }
}


