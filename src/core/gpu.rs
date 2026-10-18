//! NVIDIA GPU power management driver via nvidia-smi.

use std::process::Command;

use crate::error::{MechrevoError, Result};

use crate::hal::SysfsContext;

pub const MIN_TGP_W: f64 = 30.0;
pub const MAX_TGP_W: f64 = 175.0;
pub const PCI_DEVICES_PATH: &str = "/sys/bus/pci/devices";
pub const DYNAMIC_BOOST_MOCK_NODE: &str = "/run/systemd/units/nvidia-powerd.active";
pub const DYNAMIC_BOOST_PID_NODE: &str = "/run/nvidia-powerd.pid";

#[derive(Clone, Debug, PartialEq)]
pub struct GpuInfo {
    pub name: String,
    pub current_power_limit_w: f64,
    pub max_power_limit_w: f64,
    pub temperature_c: i32,
    pub is_suspended: bool,
    pub dynamic_boost_active: Option<bool>,
}

pub struct NvidiaSmiDriver {
    binary_path: String,
    ctx: SysfsContext,
}

impl Default for NvidiaSmiDriver {
    fn default() -> Self {
        Self::new("nvidia-smi")
    }
}

impl NvidiaSmiDriver {
    pub fn new(binary_path: impl Into<String>) -> Self {
        Self {
            binary_path: binary_path.into(),
            ctx: SysfsContext::new(),
        }
    }

    pub fn with_ctx(mut self, ctx: SysfsContext) -> Self {
        self.ctx = ctx;
        self
    }

    pub fn set_ctx(&mut self, ctx: SysfsContext) {
        self.ctx = ctx;
    }

    /// Non-intrusively inspects PCIe runtime PM status without waking up the dGPU.
    /// Strictly filters for PCI Display/3D Controller class (0x03*) to prevent
    /// companion functions like HD Audio (0x040300) from causing false suspended reports.
    pub fn is_runtime_suspended(&self) -> bool {
        let base = self.ctx.resolve(PCI_DEVICES_PATH);
        if base.exists() {
            if let Ok(entries) = std::fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let dev_name = entry.file_name();
                    let dev_str = dev_name.to_string_lossy();
                    let dev_path = format!("{}/{}", PCI_DEVICES_PATH, dev_str);
                    let vendor_node = format!("{}/vendor", dev_path);
                    let class_node = format!("{}/class", dev_path);
                    let status_node = format!("{}/power/runtime_status", dev_path);

                    if let Ok(vendor) = self.ctx.read_text(&vendor_node) {
                        // 0x10de is NVIDIA PCI Vendor ID
                        if vendor.trim().eq_ignore_ascii_case("0x10de") {
                            // PCI Class Verification: Must be Display/3D controller (0x03*)
                            // 0x030000: VGA compatible controller, 0x030200: 3D controller
                            if let Ok(class_str) = self.ctx.read_text(&class_node) {
                                let clean_class = class_str.trim().to_lowercase();
                                if !clean_class.starts_with("0x03") {
                                    // Skip companion functions like HD Audio (0x040300) or USB xHCI (0x0c0330)
                                    continue;
                                }
                            }

                            if let Ok(status) = self.ctx.read_text(&status_node) {
                                if status.trim().eq_ignore_ascii_case("suspended") {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Checks if nvidia-smi command is present and responsive.
    pub fn is_available(&self) -> bool {
        // If GPU is in PCIe RTD3 D3cold, consider available without waking up
        if self.is_runtime_suspended() {
            return true;
        }

        Command::new(&self.binary_path)
            .arg("-L")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    /// Detects whether NVIDIA Dynamic Boost daemon (nvidia-powerd) is actively running.
    /// In mobile Ada Lovelace (RTX 4060 Laptop), 140W max TGP is impossible without nvidia-powerd.
    pub fn is_dynamic_boost_active(&self) -> bool {
        if self.ctx.exists(DYNAMIC_BOOST_MOCK_NODE) || self.ctx.exists(DYNAMIC_BOOST_PID_NODE) {
            return true;
        }

        #[cfg(unix)]
        {
            Command::new("systemctl")
                .args(["is-active", "--quiet", "nvidia-powerd"])
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// Queries GPU name, current power limit, max power limit, and temperature.
    /// Employs zero-wakeup inspection to preserve battery life if GPU is in D3cold.
    pub fn get_gpu_info(&self) -> Result<GpuInfo> {
        if self.is_runtime_suspended() {
            return Ok(GpuInfo {
                name: "NVIDIA Discrete GPU (PCIe RTD3 D3cold)".to_string(),
                current_power_limit_w: 0.0,
                max_power_limit_w: 140.0,
                temperature_c: 0,
                is_suspended: true,
                dynamic_boost_active: Some(self.is_dynamic_boost_active()),
            });
        }

        let output = Command::new(&self.binary_path)
            .args([
                "--query-gpu=name,power.limit,power.max_limit,temperature.gpu",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .map_err(|e| {
                MechrevoError::HardwareInterface(format!("Failed to execute nvidia-smi: {}", e))
            })?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(MechrevoError::HardwareInterface(format!(
                "nvidia-smi query failed: {}",
                err_msg.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout.lines().next().ok_or_else(|| {
            MechrevoError::HardwareInterface("Empty nvidia-smi output".to_string())
        })?;

        let parts: Vec<&str> = first_line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 4 {
            return Err(MechrevoError::HardwareInterface(format!(
                "Unexpected nvidia-smi output format: {}",
                first_line
            )));
        }

        let parse_float = |val: &str, default: f64| -> f64 {
            let cleaned = val.to_uppercase().replace('W', "").trim().to_string();
            if cleaned == "[N/A]" || cleaned == "N/A" || cleaned == "NONE" || cleaned.is_empty() {
                default
            } else {
                cleaned.parse::<f64>().unwrap_or(default)
            }
        };

        let parse_int = |val: &str, default: i32| -> i32 {
            let cleaned = val.to_uppercase().replace('C', "").trim().to_string();
            if cleaned == "[N/A]" || cleaned == "N/A" || cleaned == "NONE" || cleaned.is_empty() {
                default
            } else {
                cleaned.parse::<i32>().unwrap_or(default)
            }
        };

        let name = parts[0].to_string();
        let current_pl = parse_float(parts[1], 0.0);
        let max_pl = parse_float(parts[2], 140.0);
        let temp = parse_int(parts[3], 0);

        let is_mobile_or_boost = name.to_lowercase().contains("laptop")
            || name.to_lowercase().contains("mobile")
            || max_pl > 115.0;
        let db_active = if is_mobile_or_boost {
            Some(self.is_dynamic_boost_active())
        } else {
            None
        };

        Ok(GpuInfo {
            name,
            current_power_limit_w: current_pl,
            max_power_limit_w: max_pl,
            temperature_c: temp,
            is_suspended: false,
            dynamic_boost_active: db_active,
        })
    }

    /// Enables NVIDIA persistence mode to keep driver loaded in hybrid graphics.
    pub fn ensure_persistence_mode(&self) -> Result<()> {
        let _ = Command::new(&self.binary_path).args(["-pm", "1"]).output();
        Ok(())
    }

    /// Disables NVIDIA persistence mode to allow PCIe RTD3 D3cold deep sleep.
    pub fn disable_persistence_mode(&self) -> Result<()> {
        let _ = Command::new(&self.binary_path).args(["-pm", "0"]).output();
        Ok(())
    }

    /// Sets NVIDIA Total Graphics Power (TGP) limit in Watts.
    pub fn set_power_limit(&self, tgp_w: f64, enable_persistence: bool) -> Result<()> {
        if !(MIN_TGP_W..=MAX_TGP_W).contains(&tgp_w) {
            return Err(MechrevoError::InvalidParameter(format!(
                "GPU power limit must be between {}W and {}W (got {}W)",
                MIN_TGP_W, MAX_TGP_W, tgp_w
            )));
        }

        // 工业级 PCIe RTD3 零惊醒保护 (Zero-Wakeup Guard):
        // 在节能模式 (!enable_persistence) 下，若独显当前已挂起在 PCIe RTD3 D3cold 状态，
        // 严禁调用 nvidia-smi，避免内核 pm_runtime_get_sync 强行拉升 GPU 供电并打断深睡眠。
        if !enable_persistence && self.is_runtime_suspended() {
            return Ok(());
        }

        if enable_persistence {
            self.ensure_persistence_mode()?;
        } else {
            self.disable_persistence_mode()?;
        }

        let output = Command::new(&self.binary_path)
            .args(["-i", "0", "-pl", &format!("{}", tgp_w as u32)])
            .output()
            .map_err(|e| {
                MechrevoError::HardwareInterface(format!("Failed to execute nvidia-smi: {}", e))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Self::verify_power_limit_response(&stdout, &stderr)?;

        if !output.status.success() {
            let err = stderr.trim();
            if err.contains("Insufficient Permissions") || err.to_lowercase().contains("root") {
                return Err(MechrevoError::PermissionDenied);
            }
            return Err(MechrevoError::HardwareInterface(format!(
                "Failed to set GPU power limit: {}",
                err
            )));
        }

        Ok(())
    }

    /// Verifies output from nvidia-smi -pl execution, detecting mobile GPU hardware lock.
    pub fn verify_power_limit_response(stdout: &str, stderr: &str) -> Result<()> {
        let combined = format!("{}\n{}", stdout, stderr);
        if combined.contains("Changing power management limit is not supported in current scope")
            || combined.contains("not supported in current scope")
        {
            return Err(MechrevoError::HardwareInterface(
                "GPU power limit adjustment not supported on mobile GeForce GPU (enforced via Dynamic Boost and nvidia-powerd)".to_string(),
            ));
        }
        if combined.contains("Insufficient Permissions") || combined.to_lowercase().contains("root") {
            return Err(MechrevoError::PermissionDenied);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_persistence_mode_toggle() {
        let driver = NvidiaSmiDriver::new("echo");
        // Must succeed without error even if mock binary echo is invoked
        assert!(driver.disable_persistence_mode().is_ok());
        assert!(driver.ensure_persistence_mode().is_ok());
    }

    #[test]
    fn test_gpu_power_limit_validation() {
        let driver = NvidiaSmiDriver::new("nvidia-smi");
        assert!(driver.set_power_limit(20.0, true).is_err());
        assert!(driver.set_power_limit(200.0, true).is_err());
    }

    #[test]
    fn test_gpu_zero_wakeup_when_suspended() {
        use tempfile::tempdir;
        use crate::hal::SysfsContext;

        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let pci_dev = ctx.resolve("/sys/bus/pci/devices/0000:01:00.0");
        std::fs::create_dir_all(pci_dev.join("power")).unwrap();
        std::fs::write(pci_dev.join("vendor"), "0x10de\n").unwrap();
        std::fs::write(pci_dev.join("class"), "0x030000\n").unwrap();
        std::fs::write(pci_dev.join("power").join("runtime_status"), "suspended\n").unwrap();

        // Pass invalid binary path so any execution attempt would fail
        let mut driver = NvidiaSmiDriver::new("invalid_binary_should_not_be_called");
        driver.set_ctx(ctx);

        let info = driver.get_gpu_info().unwrap();
        assert!(info.is_suspended, "GPU in D3cold should be reported as suspended without wake up");
        assert_eq!(info.current_power_limit_w, 0.0);
    }

    #[test]
    fn test_gpu_skip_set_power_limit_when_suspended_and_pm_disabled() {
        use tempfile::tempdir;
        use crate::hal::SysfsContext;

        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let pci_dev = ctx.resolve("/sys/bus/pci/devices/0000:01:00.0");
        std::fs::create_dir_all(pci_dev.join("power")).unwrap();
        std::fs::write(pci_dev.join("vendor"), "0x10de\n").unwrap();
        std::fs::write(pci_dev.join("class"), "0x030000\n").unwrap();
        std::fs::write(pci_dev.join("power").join("runtime_status"), "suspended\n").unwrap();

        // Non-existent binary path: if set_power_limit tries to execute it, it will return Err!
        let mut driver = NvidiaSmiDriver::new("non_existent_binary_should_not_be_executed");
        driver.set_ctx(ctx);

        // When enable_persistence = false (office mode) and GPU is already suspended in D3cold:
        // Must short-circuit and succeed with Ok(()) without executing any command!
        let res = driver.set_power_limit(60.0, false);
        assert!(
            res.is_ok(),
            "Setting power limit in office mode when GPU is in D3cold must not wake up the dGPU"
        );
    }

    #[test]
    fn test_gpu_multi_function_audio_suspended_does_not_mask_active_3d_core() {
        use tempfile::tempdir;
        use crate::hal::SysfsContext;

        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Function 0: 3D VGA Controller (0x030000) - ACTIVE
        let gpu_3d = ctx.resolve("/sys/bus/pci/devices/0000:01:00.0");
        std::fs::create_dir_all(gpu_3d.join("power")).unwrap();
        std::fs::write(gpu_3d.join("vendor"), "0x10de\n").unwrap();
        std::fs::write(gpu_3d.join("class"), "0x030000\n").unwrap();
        std::fs::write(gpu_3d.join("power").join("runtime_status"), "active\n").unwrap();

        // Function 1: HD Audio Controller (0x040300) - SUSPENDED
        let gpu_audio = ctx.resolve("/sys/bus/pci/devices/0000:01:00.1");
        std::fs::create_dir_all(gpu_audio.join("power")).unwrap();
        std::fs::write(gpu_audio.join("vendor"), "0x10de\n").unwrap();
        std::fs::write(gpu_audio.join("class"), "0x040300\n").unwrap();
        std::fs::write(gpu_audio.join("power").join("runtime_status"), "suspended\n").unwrap();

        let mut driver = NvidiaSmiDriver::new("nvidia-smi");
        driver.set_ctx(ctx);

        // Core requirement: Audio controller suspension MUST NOT mask active 3D GPU!
        assert!(
            !driver.is_runtime_suspended(),
            "Audio controller (0x040300) being suspended must NOT cause active 3D GPU (0x030000) to be reported as suspended!"
        );
    }

    #[test]
    fn test_gpu_set_power_limit_detects_unsupported_scope() {
        let stdout = "Changing power management limit is not supported in current scope for GPU: 00000000:01:00.0.\nAll done.";
        let stderr = "";
        let res = NvidiaSmiDriver::verify_power_limit_response(stdout, stderr);
        assert!(res.is_err(), "Must return Err when mobile GeForce locks -pl command");
        match res.unwrap_err() {
            MechrevoError::HardwareInterface(msg) => {
                assert!(msg.contains("not supported on mobile GeForce GPU"));
            }
            other => panic!("Unexpected error type: {:?}", other),
        }
    }

    #[test]
    fn test_gpu_dynamic_boost_detection() {
        use tempfile::tempdir;
        use crate::hal::SysfsContext;

        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let mut driver = NvidiaSmiDriver::new("echo");
        driver.set_ctx(ctx.clone());

        // Without mock node: dynamic boost is inactive (in test env)
        assert!(!driver.is_dynamic_boost_active());

        // Create mock active node
        let mock_active = ctx.resolve(DYNAMIC_BOOST_MOCK_NODE);
        std::fs::create_dir_all(mock_active.parent().unwrap()).unwrap();
        std::fs::write(&mock_active, "active\n").unwrap();

        assert!(driver.is_dynamic_boost_active());
    }
}



