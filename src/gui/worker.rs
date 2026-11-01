//! Non-blocking background worker thread for IPC hardware communication
//! and precise event-driven UI repaint scheduling.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;
use ksni::blocking::Handle as TrayHandle;
use crate::daemon::{IpcRequest, IpcResponse, SystemInfoReport, DEFAULT_SOCKET_PATH};

use super::autostart::set_autostart;
use super::tray::MechrevoTray;

#[derive(Clone, Debug)]
pub enum IpcAction {
    SetProfile(String),
    SetBattery(u32),
    SetKeyboard(u32),
    SetFanBoost(bool),
    SetAutostart(bool),
    Refresh,
}

#[derive(Clone, Debug)]
pub enum IpcResult {
    Info(Box<SystemInfoReport>),
    ActionResponse { success: bool, message: String },
    Error(String),
}

/// Pure parser converting daemon textual status output into a structured report.
/// 100% free of side-effects or synchronous sysfs I/O.
pub fn parse_status_text(text: &str) -> SystemInfoReport {
    let mut chassis = "MECHREVO Laptop".to_string();
    let mut active_profile = "balanced".to_string();
    let acpi_profile = None;
    let mut cpu_pl1_w = None;
    let mut cpu_pl2_w = None;
    let mut cpu_temp_c = None;
    let mut cpu_epp = None;
    let mut gpu_name = None;
    let mut gpu_power_w = None;
    let mut gpu_temp_c = None;
    let mut gpu_d3cold = false;
    let gpu_dynamic_boost = None;
    let mut battery_percent = None;
    let mut battery_status = None;
    let mut battery_threshold = None;
    let mut keyboard_brightness = None;
    let mut fan_boost = None;
    let fan1_rpm = None;
    let fan2_rpm = None;
    let mut ac_online = true;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("[*] Hardware Chassis:") {
            chassis = rest.trim().to_string();
        } else if trimmed.contains("Intel RAPL (CPU):") {
            for part in trimmed.split('|') {
                if let Some(pos) = part.find("PL1 = ") {
                    if let Some(num) = part[pos + 6..].split('W').next() {
                        cpu_pl1_w = num.trim().parse::<f64>().ok();
                    }
                }
                if let Some(pos) = part.find("PL2 = ") {
                    if let Some(num) = part[pos + 6..].split('W').next() {
                        cpu_pl2_w = num.trim().parse::<f64>().ok();
                    }
                }
                if let Some(pos) = part.find("Temp: ") {
                    if let Some(num) = part[pos + 6..].split("°C").next() {
                        cpu_temp_c = num.trim().parse::<i32>().ok();
                    }
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("Energy Perf Preference (EPP):") {
            cpu_epp = Some(rest.trim().to_string());
        } else if trimmed.contains("PCIe RTD3 D3cold") {
            gpu_d3cold = true;
            gpu_name = Some("NVIDIA RTX 4060 (D3cold)".to_string());
        } else if let Some(rest) = trimmed.strip_prefix("[*] NVIDIA dGPU:") {
            gpu_name = Some(rest.trim().to_string());
        } else if trimmed.contains("TGP Limit:") {
            if let Some(num) = trimmed.split("TGP Limit:").nth(1).and_then(|s| s.split('W').next()) {
                gpu_power_w = num.trim().parse::<f64>().ok();
            }
            if let Some(num) = trimmed.split("Temperature:").nth(1).and_then(|s| s.split("°C").next()) {
                gpu_temp_c = num.trim().parse::<i32>().ok();
            }
        } else if trimmed.contains("Temperature:") {
            if let Some(num) = trimmed.split("Temperature:").nth(1).and_then(|s| s.split("°C").next()) {
                gpu_temp_c = num.trim().parse::<i32>().ok();
            }
        } else if trimmed.contains("Active software profile: '") {
            if let Some(prof) = trimmed.split("Active software profile: '").nth(1).and_then(|s| s.split('\'').next()) {
                active_profile = prof.to_string();
            }
        } else if let Some(rest) = trimmed.strip_prefix("[*] Battery:") {
            let parts: Vec<&str> = rest.split('|').collect();
            if let Some(first) = parts.first() {
                if let Some(p_str) = first.split('%').next() {
                    battery_percent = p_str.trim().parse::<u32>().ok();
                }
                if let Some(status_str) = first.split('(').nth(1).and_then(|s| s.split(')').next()) {
                    let st = status_str.trim();
                    battery_status = Some(st.to_string());
                    if st.eq_ignore_ascii_case("discharging") {
                        ac_online = false;
                    }
                }
            }
            if let Some(second) = parts.get(1) {
                if let Some(th_str) = second.split("Threshold Limit:").nth(1).and_then(|s| s.split('%').next()) {
                    battery_threshold = th_str.trim().parse::<u32>().ok();
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("[*] Keyboard Backlight:") {
            if let Some(br_str) = rest.split('%').next() {
                keyboard_brightness = br_str.trim().parse::<u32>().ok();
            }
        } else if trimmed.contains("Boost: ON") {
            fan_boost = Some(true);
        } else if trimmed.contains("Boost: OFF") {
            fan_boost = Some(false);
        }
    }

    SystemInfoReport {
        chassis,
        active_profile,
        acpi_profile,
        cpu_pl1_w,
        cpu_pl2_w,
        cpu_temp_c,
        cpu_epp,
        gpu_name,
        gpu_power_w,
        gpu_temp_c,
        gpu_d3cold,
        gpu_dynamic_boost,
        battery_percent,
        battery_status,
        battery_threshold,
        keyboard_brightness,
        fan_boost,
        fan1_rpm,
        fan2_rpm,
        ac_online,
    }
}

pub fn send_ipc_payload(req: &IpcRequest) -> Result<IpcResponse, String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(DEFAULT_SOCKET_PATH)
        .map_err(|e| format!("无法连接后台守护进程 ({}): {}", DEFAULT_SOCKET_PATH, e))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;

    let payload = serde_json::to_string(req).map_err(|e| e.to_string())?;
    stream
        .write_all(payload.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.shutdown(std::net::Shutdown::Write).ok();

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;

    serde_json::from_slice::<IpcResponse>(&buf).map_err(|e| e.to_string())
}

pub fn fetch_system_info() -> Result<SystemInfoReport, String> {
    if let Ok(resp) = send_ipc_payload(&IpcRequest::GetInfo) {
        if resp.success {
            if let Some(ref msg) = resp.message {
                if let Ok(report) = serde_json::from_str::<SystemInfoReport>(msg) {
                    return Ok(report);
                }
            }
        }
    }

    // Fallback: Query Status for backward compatibility with running daemon
    let resp = send_ipc_payload(&IpcRequest::Status)?;
    if resp.success {
        if let Some(ref msg) = resp.message {
            return Ok(parse_status_text(msg));
        }
    }
    Err(resp.error.unwrap_or_else(|| "获取硬件数据失败".to_string()))
}

pub struct WorkerChannels {
    pub action_tx: Sender<IpcAction>,
    pub result_rx: Receiver<IpcResult>,
    pub is_window_visible: Arc<AtomicBool>,
    pub auto_refresh_active: Arc<AtomicBool>,
}

/// Spawns the background worker thread.
/// Takes `egui::Context` to provide precise wakeup calls without any busy-polling.
/// Shares `tray_handle` so the tray can be updated in real time even while the UI is deep sleeping.
pub fn spawn_worker(
    ctx: egui::Context,
    tray_handle: Arc<Mutex<Option<TrayHandle<MechrevoTray>>>>,
) -> WorkerChannels {
    let (action_tx, action_rx) = channel::<IpcAction>();
    let (result_tx, result_rx) = channel::<IpcResult>();
    let is_window_visible = Arc::new(AtomicBool::new(true));
    let auto_refresh_active = Arc::new(AtomicBool::new(true));

    let is_visible_clone = is_window_visible.clone();
    let auto_refresh_clone = auto_refresh_active.clone();

    thread::spawn(move || {
        let wake_ui = {
            let ctx = ctx.clone();
            let is_visible = is_visible_clone.clone();
            move || {
                if is_visible.load(Ordering::Relaxed) {
                    ctx.request_repaint();
                }
            }
        };

        let update_tray = {
            let tray_handle = tray_handle.clone();
            move |info: &SystemInfoReport| {
                if let Ok(guard) = tray_handle.lock() {
                    if let Some(ref handle) = *guard {
                        let prof = info.active_profile.clone();
                        let boost = info.fan_boost.unwrap_or(false);
                        let cpu_t = info.cpu_temp_c;
                        let gpu_t = info.gpu_temp_c;
                        let gpu_d3 = info.gpu_d3cold;
                        let ac = info.ac_online;
                        let bat_th = info.battery_threshold;
                        let _ = handle.update(|t| {
                            t.active_profile = prof;
                            t.fan_boost = boost;
                            t.cpu_temp = cpu_t;
                            t.gpu_temp = gpu_t;
                            t.gpu_d3cold = gpu_d3;
                            t.ac_online = ac;
                            t.battery_th = bat_th;
                        });
                    }
                }
            }
        };

        // Initial hardware snapshot
        if let Ok(info) = fetch_system_info() {
            update_tray(&info);
            let _ = result_tx.send(IpcResult::Info(Box::new(info)));
            wake_ui();
        }

        loop {
            // Wait for next action with a 2-second timeout for periodic sensor telemetry
            let action_opt = match action_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(action) => Some(action),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if auto_refresh_clone.load(Ordering::Relaxed) {
                        Some(IpcAction::Refresh)
                    } else {
                        None
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };

            if let Some(action) = action_opt {
                match action {
                    IpcAction::Refresh => match fetch_system_info() {
                        Ok(info) => {
                            update_tray(&info);
                            let _ = result_tx.send(IpcResult::Info(Box::new(info)));
                            wake_ui();
                        }
                        Err(e) => {
                            let _ = result_tx.send(IpcResult::Error(e));
                            wake_ui();
                        }
                    },
                    IpcAction::SetProfile(profile) => {
                        let resp = send_ipc_payload(&IpcRequest::Profile { mode: profile });
                        match resp {
                            Ok(r) => {
                                let _ = result_tx.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    update_tray(&info);
                                    let _ = result_tx.send(IpcResult::Info(Box::new(info)));
                                }
                                wake_ui();
                            }
                            Err(e) => {
                                let _ = result_tx.send(IpcResult::Error(e));
                                wake_ui();
                            }
                        }
                    }
                    IpcAction::SetBattery(threshold) => {
                        let resp = send_ipc_payload(&IpcRequest::Battery { threshold });
                        match resp {
                            Ok(r) => {
                                let _ = result_tx.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    update_tray(&info);
                                    let _ = result_tx.send(IpcResult::Info(Box::new(info)));
                                }
                                wake_ui();
                            }
                            Err(e) => {
                                let _ = result_tx.send(IpcResult::Error(e));
                                wake_ui();
                            }
                        }
                    }
                    IpcAction::SetKeyboard(brightness) => {
                        let resp = send_ipc_payload(&IpcRequest::Keyboard { brightness });
                        match resp {
                            Ok(r) => {
                                let _ = result_tx.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    update_tray(&info);
                                    let _ = result_tx.send(IpcResult::Info(Box::new(info)));
                                }
                                wake_ui();
                            }
                            Err(e) => {
                                let _ = result_tx.send(IpcResult::Error(e));
                                wake_ui();
                            }
                        }
                    }
                    IpcAction::SetFanBoost(enabled) => {
                        let resp = send_ipc_payload(&IpcRequest::FanBoost { enable: enabled });
                        match resp {
                            Ok(r) => {
                                let _ = result_tx.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    update_tray(&info);
                                    let _ = result_tx.send(IpcResult::Info(Box::new(info)));
                                }
                                wake_ui();
                            }
                            Err(e) => {
                                let _ = result_tx.send(IpcResult::Error(e));
                                wake_ui();
                            }
                        }
                    }
                    IpcAction::SetAutostart(enabled) => {
                        match set_autostart(enabled) {
                            Ok(()) => {
                                let msg = if enabled {
                                    "已开启开机静默自启 (登录后后台托盘启动)".to_string()
                                } else {
                                    "已关闭开机自启".to_string()
                                };
                                let _ = result_tx.send(IpcResult::ActionResponse {
                                    success: true,
                                    message: msg,
                                });
                            }
                            Err(e) => {
                                let _ = result_tx.send(IpcResult::Error(format!(
                                    "配置开机自启失败: {}",
                                    e
                                )));
                            }
                        }
                        wake_ui();
                    }
                }
            }
        }
    });

    WorkerChannels {
        action_tx,
        result_rx,
        is_window_visible,
        auto_refresh_active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_text_fallback() {
        let sample = "==================================================\n\
                      MECHREVO Control Center - System Status\n\
                      ==================================================\n\
                      [*] Hardware Chassis: JiguangX Series GM6IQ0C\n\
                      [*] Intel RAPL (CPU): PL1 = 75W | PL2 = 90W | Temp: 75°C\n\
                          Energy Perf Preference (EPP): balance_performance\n\
                      [*] NVIDIA dGPU: NVIDIA GeForce RTX 4060 Laptop GPU\n\
                          TGP Limit: 0W (Base TGP; 25W Dynamic Boost inactive, run 'systemctl start nvidia-powerd')\n\
                          Temperature: 51°C\n\
                      [-] ACPI Platform Profile: Not available in kernel (Active software profile: 'balanced', managed via native Intel RAPL & EPP; Note: EC fan curve not synchronized, consider fan-boost or tuxedo-drivers-dkms under heavy load)\n\
                      [*] Battery: 100% (Full) | Threshold Limit: 80%\n\
                      [*] Keyboard Backlight: 50%\n\
                      [*] Cooling (Fans): Managed by EC firmware (Boost: OFF)\n\
                      ==================================================";

        let info = parse_status_text(sample);
        assert_eq!(info.chassis, "JiguangX Series GM6IQ0C");
        assert_eq!(info.cpu_pl1_w, Some(75.0));
        assert_eq!(info.cpu_pl2_w, Some(90.0));
        assert_eq!(info.cpu_temp_c, Some(75));
        assert_eq!(info.cpu_epp.as_deref(), Some("balance_performance"));
        assert_eq!(info.active_profile, "balanced");
        assert_eq!(info.gpu_power_w, Some(0.0));
        assert_eq!(info.gpu_temp_c, Some(51));
        assert!(!info.gpu_d3cold);
        assert_eq!(info.battery_percent, Some(100));
        assert_eq!(info.battery_status.as_deref(), Some("Full"));
        assert_eq!(info.battery_threshold, Some(80));
        assert_eq!(info.keyboard_brightness, Some(50));
        assert_eq!(info.fan_boost, Some(false));
    }
}
