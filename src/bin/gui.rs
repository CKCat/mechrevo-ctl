//! Native GUI for MECHREVO Control Center.
//! Ultra-lightweight, high-performance, native Rust GUI using eframe/egui.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use ksni::blocking::{Handle as TrayHandle, TrayMethods};
use mechrevo::daemon::{IpcRequest, IpcResponse, SystemInfoReport, DEFAULT_SOCKET_PATH};

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum TrayCommand {
    ToggleWindow,
    ShowWindow,
    HideWindow,
    SetProfile(String),
    SetFanBoost(bool),
    SetBattery(u32),
    Quit,
}

#[derive(Clone)]
struct MechrevoTray {
    active_profile: String,
    fan_boost: bool,
    cpu_temp: Option<i32>,
    gpu_temp: Option<i32>,
    gpu_d3cold: bool,
    battery_th: Option<u32>,
    ac_online: bool,
    tray_tx: Sender<TrayCommand>,
    ctx: egui::Context,
}

/// Formats the system tray tooltip strings.
/// Explicitly reports PCIe RTD3 D3cold deep sleep state (0W) instead of misleading "--".
pub fn format_tray_tooltip(
    active_profile: &str,
    cpu_temp: Option<i32>,
    gpu_temp: Option<i32>,
    gpu_d3cold: bool,
    ac_online: bool,
    fan_boost: bool,
) -> (String, String) {
    let prof_name = match active_profile {
        "turbo" => "狂暴满血 (105W/140W)",
        "office" => "办公静音 (45W/0W)",
        _ => "日常平衡 (75W/115W)",
    };
    let cpu_str = cpu_temp
        .map(|t| format!("{}°C", t))
        .unwrap_or_else(|| "--".to_string());
    let gpu_str = if gpu_d3cold {
        "深度休眠 (0W)".to_string()
    } else if let Some(t) = gpu_temp {
        format!("{}°C", t)
    } else {
        "离线".to_string()
    };
    let pwr_str = if ac_online { "AC 适配器供电" } else { "电池供电" };
    let boost_str = if fan_boost { " | 强冷开启" } else { "" };

    (
        format!("机械革命控制中心 - {}", prof_name),
        format!("CPU: {} | GPU: {} | {}{}", cpu_str, gpu_str, pwr_str, boost_str),
    )
}

impl ksni::Tray for MechrevoTray {
    fn id(&self) -> String {
        "mechrevo-control-center".to_string()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn title(&self) -> String {
        "机械革命控制中心".to_string()
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn icon_name(&self) -> String {
        "mechrevo-control-center".to_string()
    }

    fn icon_theme_path(&self) -> String {
        if let Ok(home) = std::env::var("HOME") {
            let local_dir = format!("{}/.local/share/icons/hicolor/scalable/apps", home);
            if std::path::Path::new(&local_dir).exists() {
                return local_dir;
            }
        }
        "/usr/share/icons/hicolor/scalable/apps".to_string()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let (title, description) = format_tray_tooltip(
            &self.active_profile,
            self.cpu_temp,
            self.gpu_temp,
            self.gpu_d3cold,
            self.ac_online,
            self.fan_boost,
        );

        ksni::ToolTip {
            title,
            description,
            icon_name: "mechrevo-control-center".to_string(),
            icon_pixmap: Vec::new(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tray_tx.send(TrayCommand::ToggleWindow);
        self.ctx.request_repaint();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let tx = self.tray_tx.clone();
        let ctx = self.ctx.clone();

        let tx_prof = tx.clone();
        let ctx_prof = ctx.clone();
        let current_prof = self.active_profile.clone();

        let tx_bat = tx.clone();
        let ctx_bat = ctx.clone();
        let current_th = self.battery_th.unwrap_or(80);

        let make_cb = |cmd: TrayCommand| -> Box<dyn Fn(&mut Self) + Send> {
            let tx = tx.clone();
            let ctx = ctx.clone();
            Box::new(move |_: &mut Self| {
                let _ = tx.send(cmd.clone());
                ctx.request_repaint();
            })
        };

        let current_boost = self.fan_boost;

        let selected_prof = match current_prof.as_str() {
            "turbo" => 0,
            "balanced" => 1,
            "office" => 2,
            _ => 1,
        };

        let selected_bat = match current_th {
            th if th <= 65 => 2, // 60%
            th if th <= 85 => 0, // 80%
            _ => 1,              // 100%
        };

        vec![
            RadioGroup {
                selected: selected_prof,
                select: Box::new(move |tray: &mut Self, index: usize| {
                    let (prof, cmd) = match index {
                        0 => ("turbo", TrayCommand::SetProfile("turbo".to_string())),
                        1 => ("balanced", TrayCommand::SetProfile("balanced".to_string())),
                        2 => ("office", TrayCommand::SetProfile("office".to_string())),
                        _ => ("balanced", TrayCommand::SetProfile("balanced".to_string())),
                    };
                    tray.active_profile = prof.to_string();
                    let _ = tx_prof.send(cmd);
                    ctx_prof.request_repaint();
                }),
                options: vec![
                    RadioItem {
                        label: "狂暴满血 (Turbo 105W/140W)".to_string(),
                        ..Default::default()
                    },
                    RadioItem {
                        label: "日常平衡 (Balanced 75W/115W)".to_string(),
                        ..Default::default()
                    },
                    RadioItem {
                        label: "办公静音 (Office 45W/0W)".to_string(),
                        ..Default::default()
                    },
                ],
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "双风扇 100% 极限强冷 (Fan Boost)".to_string(),
                enabled: true,
                visible: true,
                checked: current_boost,
                activate: make_cb(TrayCommand::SetFanBoost(!current_boost)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "电池健康与充电养护".to_string(),
                enabled: true,
                visible: true,
                submenu: vec![
                    RadioGroup {
                        selected: selected_bat,
                        select: Box::new(move |tray: &mut Self, index: usize| {
                            let th = match index {
                                0 => 80,
                                1 => 100,
                                2 => 60,
                                _ => 80,
                            };
                            tray.battery_th = Some(th);
                            let _ = tx_bat.send(TrayCommand::SetBattery(th));
                            ctx_bat.request_repaint();
                        }),
                        options: vec![
                            RadioItem {
                                label: "80% 常年插电养护 (推荐)".to_string(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: "100% 满电出行备战".to_string(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: "60% 深度存放养护".to_string(),
                                ..Default::default()
                            },
                        ],
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "打开控制中心主界面".to_string(),
                enabled: true,
                visible: true,
                activate: make_cb(TrayCommand::ShowWindow),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "彻底退出".to_string(),
                enabled: true,
                visible: true,
                activate: make_cb(TrayCommand::Quit),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub mod theme {
    use eframe::egui::Color32;

    // Surface Hierarchy Palette (Modern Dark Theme - VS Code / Linear style)
    pub const BG_BASE: Color32 = Color32::from_rgb(15, 23, 42);       // #0f172a Main container background
    pub const BG_PANEL: Color32 = Color32::from_rgb(24, 24, 30);      // #18181e Top title bar & bottom status bar
    pub const BG_CARD: Color32 = Color32::from_rgb(30, 41, 59);       // #1e293b Elevated feature cards
    pub const BG_CARD_SUBTLE: Color32 = Color32::from_rgb(20, 28, 42);// Nested tiles
    pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(51, 65, 85); // #334155 Subtle border stroke (1.0px)
    pub const BORDER_CARD: Color32 = Color32::from_rgb(42, 55, 82);

    // Interactive Controls
    pub const BTN_INACTIVE_BG: Color32 = Color32::from_rgb(40, 44, 56);  // #282c38 Inactive buttons & chips
    pub const BTN_HOVERED_BG: Color32 = Color32::from_rgb(55, 65, 81);   // #374151 Hover feedback
    pub const BTN_ACTIVE_BG: Color32 = Color32::from_rgb(71, 85, 105);   // #475569 Pressed state

    // Accent Colors
    pub const ACCENT_CYAN: Color32 = Color32::from_rgb(6, 182, 212);    // #06b6d4 Primary brand highlights
    pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(249, 115, 22);  // #f97316 Fan boost & heavy alerts
    pub const ACCENT_GREEN: Color32 = Color32::from_rgb(34, 197, 94);   // #22c55e AC online & success badges
    pub const ACCENT_RED: Color32 = Color32::from_rgb(239, 68, 68);     // #ef4444 Critical warnings & failures

    // Typography
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(241, 245, 249);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(148, 163, 184);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(100, 116, 139);
}

enum IpcAction {
    SetProfile(String),
    SetBattery(u32),
    SetKeyboard(u32),
    SetFanBoost(bool),
    SetAutostart(bool),
    Refresh,
}

enum IpcResult {
    Info(Box<SystemInfoReport>),
    ActionResponse { success: bool, message: String },
    Error(String),
}

/// Classifies CPU temperature into localized status and alert color.
pub fn classify_cpu_temp(temp: Option<i32>) -> (String, Color32) {
    match temp {
        Some(t) if t >= 80 => (format!("{}°C (高温)", t), theme::ACCENT_RED),
        Some(t) if t >= 65 => (format!("{}°C (温热)", t), theme::ACCENT_ORANGE),
        Some(t) => (format!("{}°C (正常)", t), theme::ACCENT_CYAN),
        None => ("--°C".to_string(), theme::TEXT_MUTED),
    }
}

/// Classifies GPU temperature into localized status and alert color.
pub fn classify_gpu_temp(temp: Option<i32>) -> (String, Color32) {
    match temp {
        Some(t) if t >= 80 => (format!("{}°C (高温)", t), theme::ACCENT_RED),
        Some(t) if t >= 65 => (format!("{}°C (温热)", t), theme::ACCENT_ORANGE),
        Some(t) => (format!("{}°C (清凉)", t), theme::ACCENT_CYAN),
        None => ("--°C".to_string(), theme::TEXT_MUTED),
    }
}

/// Formats CPU PL1/PL2 power limit telemetry. Avoids misleading 0W when readings are unavailable.
pub fn format_cpu_pl(pl1: Option<f64>, pl2: Option<f64>) -> String {
    match (pl1, pl2) {
        (Some(p1), Some(p2)) => format!("PL1: {:.0}W | PL2: {:.0}W", p1, p2),
        (Some(p1), None) => format!("PL1: {:.0}W | PL2: --W", p1),
        (None, Some(p2)) => format!("PL1: --W | PL2: {:.0}W", p2),
        (None, None) => "PL1: --W | PL2: --W".to_string(),
    }
}

/// Formats dual cooling fan RPM and boost telemetry description.
pub fn format_fan_status(fan1: Option<u32>, fan2: Option<u32>, boost: Option<bool>) -> String {
    match (fan1, fan2) {
        (Some(f1), Some(f2)) => format!("F1: {} | F2: {} RPM", f1, f2),
        (Some(f1), None) => format!("F1: {} RPM", f1),
        (None, Some(f2)) => format!("F2: {} RPM", f2),
        _ => if boost == Some(true) {
            "双风扇 100% 满速强冷".to_string()
        } else {
            "EC 固件智能调速".to_string()
        },
    }
}

/// Formats GPU power limit telemetry.
pub fn format_gpu_power(gpu_power_w: Option<f64>) -> String {
    match gpu_power_w {
        Some(p) if p > 0.0 => format!("活跃 · 功耗上限 {:.0}W", p),
        _ => "活跃 · 基础功耗释放".to_string(),
    }
}

/// Renders a modern rounded badge pill (Rounding::same(12.0)).
pub fn render_badge(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(fg).strong().size(10.5));
        });
}

fn send_ipc_payload(req: &IpcRequest) -> Result<IpcResponse, String> {
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

fn parse_status_text(text: &str) -> SystemInfoReport {
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

    let ac_online = std::fs::read_to_string("/sys/class/power_supply/AC/online")
        .or_else(|_| std::fs::read_to_string("/sys/class/power_supply/AC0/online"))
        .or_else(|_| std::fs::read_to_string("/sys/class/power_supply/ADP1/online"))
        .or_else(|_| std::fs::read_to_string("/sys/class/power_supply/ACAD/online"))
        .map(|s| s.trim() == "1")
        .unwrap_or(true);

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
                    battery_status = Some(status_str.trim().to_string());
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

fn fetch_system_info() -> Result<SystemInfoReport, String> {
    if let Ok(resp) = send_ipc_payload(&IpcRequest::GetInfo) {
        if resp.success {
            if let Some(msg) = resp.message {
                if let Ok(report) = serde_json::from_str::<SystemInfoReport>(&msg) {
                    return Ok(report);
                }
            }
        }
    }

    // Fallback: Query Status for backward compatibility with running daemon
    let resp = send_ipc_payload(&IpcRequest::Status)?;
    if resp.success {
        if let Some(msg) = resp.message {
            return Ok(parse_status_text(&msg));
        }
    }
    Err(resp.error.unwrap_or_else(|| "获取硬件数据失败".to_string()))
}

fn format_status_message(raw: &str, is_err: bool) -> (String, Option<String>) {
    if is_err {
        if raw.contains("charge_control") || raw.contains("Battery") || raw.contains("battery") {
            (
                "电池充电养护阈值配置失败: 硬件节点不可用".to_string(),
                Some("当前 Linux 内核未挂载 Uniwill/Tuxedo 电池扩展驱动，可运行 sudo ./scripts/setup-tuxedo-drivers.sh 激活".to_string()),
            )
        } else if raw.contains("fan_boost") || raw.contains("Fan boost") || raw.contains("fan-boost") {
            (
                "散热强冷配置失败: 硬件节点不可用".to_string(),
                Some("请在终端运行 sudo ./scripts/setup-tuxedo-drivers.sh 激活 EC 强冷控制".to_string()),
            )
        } else if raw.contains("Permission denied") {
            (
                "操作权限不足: 请求被系统拒绝".to_string(),
                Some("请确保 mechrevo-ctld 守护进程正在以 root 身份运行".to_string()),
            )
        } else {
            (
                "操作执行失败".to_string(),
                Some(raw.to_string()),
            )
        }
    } else if raw.contains("Successfully applied profile 'turbo'") {
        (
            "已成功切换至【狂暴满血模式】(CPU 105W / GPU 140W)".to_string(),
            Some("散热由 EC 固件智能接管，如需极速降温可随时按键 [B] 开启 100% 强冷".to_string()),
        )
    } else if raw.contains("Successfully applied profile 'balanced'") {
        (
            "已成功切换至【日常平衡模式】(CPU 75W / GPU 115W)".to_string(),
            Some("独显空载自然沉入 PCIe RTD3 D3cold 深度休眠，兼顾性能与整机能效".to_string()),
        )
    } else if raw.contains("Successfully applied profile 'office'") {
        (
            "已成功切换至【办公静音模式】(CPU 45W / 独显 D3cold 断电 0W)".to_string(),
            Some("核心调频锁定节能 EPP，最大限度减少机身发热并延长电池续航".to_string()),
        )
    } else if raw.contains("Battery charge threshold set to") || raw.contains("Battery threshold") {
        let th = raw
            .split("set to ")
            .nth(1)
            .and_then(|s| s.split('%').next())
            .unwrap_or("80");
        (
            format!("已成功配置电池充电养护阈值: {}%", th),
            Some("已持久化存储至 state.json，关机断电后自动恢复".to_string()),
        )
    } else if raw.contains("fan-boost") || raw.contains("Fan boost") || raw.contains("FanBoost") {
        (
            "散热强冷状态已更新".to_string(),
            Some(raw.to_string()),
        )
    } else if raw.contains("Keyboard brightness set to") {
        let br = raw
            .split("set to ")
            .nth(1)
            .and_then(|s| s.split('%').next())
            .unwrap_or("50");
        (
            format!("已成功设置键盘背光亮度: {}%", br),
            None,
        )
    } else {
        (raw.to_string(), None)
    }
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut font_candidates = vec![
        // 1. WenQuanYi Micro Hei / Zen Hei (Top priority for Linux desktop CJK glyph rendering)
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc".to_string(),
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc".to_string(),
        // 2. Noto Sans CJK SC
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc".to_string(),
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf".to_string(),
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/truetype/arphic/uming.ttc".to_string(),
        "/usr/share/fonts/truetype/arphic/ukai.ttc".to_string(),
        // 3. Microsoft YaHei / SimHei (Dual-boot Windows & Wine paths)
        "/mnt/c/Windows/Fonts/msyh.ttc".to_string(),
        "/mnt/c/Windows/Fonts/msyhbd.ttc".to_string(),
        "/mnt/c/Windows/Fonts/simhei.ttf".to_string(),
    ];

    if let Ok(home) = std::env::var("HOME") {
        font_candidates.push(format!("{}/.local/share/fonts/NotoSansCJK-Regular.ttc", home));
        font_candidates.push(format!("{}/.local/share/fonts/DroidSansFallbackFull.ttf", home));
        font_candidates.push(format!("{}/.wine/drive_c/windows/Fonts/msyh.ttc", home));
        font_candidates.push(format!("{}/.wine/drive_c/windows/Fonts/simhei.ttf", home));
    }

    for path in &font_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "cjk_fallback".to_owned(),
                egui::FontData::from_owned(bytes),
            );

            if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                vec.insert(0, "cjk_fallback".to_owned());
            }
            if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                vec.push("cjk_fallback".to_owned());
            }

            ctx.set_fonts(fonts);
            break;
        }
    }
}

/// Renders a clickable preset pill button with active indicator and hover feedback
fn render_pill_button(
    ui: &mut egui::Ui,
    label: &str,
    is_active: bool,
    accent_color: Color32,
) -> bool {
    let mut clicked = false;
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        if is_active {
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(26, 44, 66);
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.5_f32, accent_color);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(34, 58, 88);
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.5_f32, accent_color);
        } else {
            visuals.widgets.inactive.bg_fill = theme::BTN_INACTIVE_BG;
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, theme::BORDER_SUBTLE);
            visuals.widgets.hovered.bg_fill = theme::BTN_HOVERED_BG;
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, theme::BORDER_SUBTLE);
        }

        let text_color = if is_active {
            Color32::WHITE
        } else {
            theme::TEXT_SECONDARY
        };

        let display_text = if is_active {
            format!("✔ {}", label)
        } else {
            label.to_string()
        };

        let button = egui::Button::new(
            RichText::new(display_text)
                .color(text_color)
                .strong()
                .size(11.0),
        )
        .rounding(Rounding::same(6.0_f32));

        let response = ui.add(button);
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        clicked = response.clicked();
    });
    clicked
}

/// Returns the standard XDG autostart path for the user desktop entry.
fn get_autostart_path() -> Option<PathBuf> {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !config_home.trim().is_empty() {
            return Some(PathBuf::from(config_home).join("autostart").join("mechrevo-control-center.desktop"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Some(PathBuf::from(home).join(".config").join("autostart").join("mechrevo-control-center.desktop"));
        }
    }
    None
}

/// Parses an autostart desktop entry content to determine if it is actively enabled.
fn is_autostart_content_enabled(content: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("hidden=true")
            || trimmed.eq_ignore_ascii_case("x-gnome-autostart-enabled=false")
        {
            return false;
        }
    }
    true
}

/// Checks whether mechrevo-gui is configured to autostart upon user login.
fn is_autostart_enabled() -> bool {
    // 1. Check user local autostart file first
    if let Some(user_path) = get_autostart_path() {
        if user_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&user_path) {
                return is_autostart_content_enabled(&content);
            }
        }
    }

    // 2. Check system-wide autostart file
    let sys_path = Path::new("/etc/xdg/autostart/mechrevo-control-center.desktop");
    if sys_path.exists() {
        if let Ok(content) = std::fs::read_to_string(sys_path) {
            return is_autostart_content_enabled(&content);
        }
    }

    false
}

/// Enables or disables autostart for the GUI desktop application.
fn set_autostart(enabled: bool) -> Result<(), std::io::Error> {
    let path = get_autostart_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "未找到有效的用户配置目录 (~/.config)")
    })?;

    let sys_path = Path::new("/etc/xdg/autostart/mechrevo-control-center.desktop");
    let sys_exists = sys_path.exists();

    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = "\
[Desktop Entry]
Type=Application
Name=MECHREVO Control Center
Name[zh_CN]=机械革命控制中心
Comment=Hardware power, thermal, battery and performance management for MECHREVO laptops
Comment[zh_CN]=机械革命笔记本功耗释放、性能模式、风扇强冷与电池养护控制中心
Exec=mechrevo-gui --minimized
Icon=mechrevo-control-center
Terminal=false
Categories=Settings;HardwareSettings;System;Utility;
StartupNotify=false
X-GNOME-Autostart-enabled=true
";
        std::fs::write(&path, content)?;
    } else if sys_exists {
        // Mask the system-wide autostart entry with Hidden=true in user directory
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = "\
[Desktop Entry]
Type=Application
Name=MECHREVO Control Center
Exec=mechrevo-gui --minimized
Hidden=true
X-GNOME-Autostart-enabled=false
";
        std::fs::write(&path, content)?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }

    Ok(())
}

struct MechrevoGuiApp {
    info: Option<SystemInfoReport>,
    action_tx: Sender<IpcAction>,
    result_rx: Receiver<IpcResult>,
    status_message: Option<(String, bool, Instant)>, // (msg, is_error, timestamp)
    auto_refresh: bool,
    last_refresh: Instant,
    custom_battery_th: u32,
    custom_kbd_br: u32,
    is_loading: bool,
    // Tray and window lifecycle
    tray_handle: Option<TrayHandle<MechrevoTray>>,
    tray_rx: Receiver<TrayCommand>,
    window_visible: bool,
    minimize_to_tray: bool,
    autostart: bool,
}

impl MechrevoGuiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Setup CJK fallback font for Chinese character rendering
        setup_custom_fonts(&cc.egui_ctx);

        // Customize egui theme according to rust-egui-design surface elevation
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(theme::TEXT_PRIMARY);
        visuals.panel_fill = theme::BG_PANEL;
        visuals.window_fill = theme::BG_BASE;

        // Configure widget styling tokens according to rust-egui-design
        visuals.widgets.noninteractive.bg_fill = theme::BG_CARD;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, theme::BORDER_SUBTLE);
        visuals.widgets.noninteractive.rounding = Rounding::same(6.0_f32);

        visuals.widgets.inactive.bg_fill = theme::BTN_INACTIVE_BG;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, theme::BORDER_SUBTLE);
        visuals.widgets.inactive.rounding = Rounding::same(6.0_f32);

        visuals.widgets.hovered.bg_fill = theme::BTN_HOVERED_BG;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, theme::ACCENT_CYAN);
        visuals.widgets.hovered.rounding = Rounding::same(6.0_f32);

        visuals.widgets.active.bg_fill = theme::BTN_ACTIVE_BG;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, theme::BORDER_SUBTLE);
        visuals.widgets.active.rounding = Rounding::same(6.0_f32);

        visuals.selection.bg_fill = theme::ACCENT_CYAN;
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

        cc.egui_ctx.set_visuals(visuals);

        // Standard comfortable button padding and item spacing
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        cc.egui_ctx.set_style(style);

        let (action_tx, action_rx) = channel::<IpcAction>();
        let (result_tx, result_rx) = channel::<IpcResult>();

        // Background worker thread for zero UI lag
        let res_tx_clone = result_tx.clone();
        thread::spawn(move || {
            // Initial fetch
            if let Ok(info) = fetch_system_info() {
                let _ = res_tx_clone.send(IpcResult::Info(Box::new(info)));
            }

            while let Ok(action) = action_rx.recv() {
                match action {
                    IpcAction::Refresh => match fetch_system_info() {
                        Ok(info) => {
                            let _ = res_tx_clone.send(IpcResult::Info(Box::new(info)));
                        }
                        Err(e) => {
                            let _ = res_tx_clone.send(IpcResult::Error(e));
                        }
                    },
                    IpcAction::SetProfile(profile) => {
                        let resp = send_ipc_payload(&IpcRequest::Profile { mode: profile });
                        match resp {
                            Ok(r) => {
                                let _ = res_tx_clone.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    let _ = res_tx_clone.send(IpcResult::Info(Box::new(info)));
                                }
                            }
                            Err(e) => {
                                let _ = res_tx_clone.send(IpcResult::Error(e));
                            }
                        }
                    }
                    IpcAction::SetBattery(threshold) => {
                        let resp = send_ipc_payload(&IpcRequest::Battery { threshold });
                        match resp {
                            Ok(r) => {
                                let _ = res_tx_clone.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    let _ = res_tx_clone.send(IpcResult::Info(Box::new(info)));
                                }
                            }
                            Err(e) => {
                                let _ = res_tx_clone.send(IpcResult::Error(e));
                            }
                        }
                    }
                    IpcAction::SetKeyboard(brightness) => {
                        let resp = send_ipc_payload(&IpcRequest::Keyboard { brightness });
                        match resp {
                            Ok(r) => {
                                let _ = res_tx_clone.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    let _ = res_tx_clone.send(IpcResult::Info(Box::new(info)));
                                }
                            }
                            Err(e) => {
                                let _ = res_tx_clone.send(IpcResult::Error(e));
                            }
                        }
                    }
                    IpcAction::SetFanBoost(enabled) => {
                        let resp = send_ipc_payload(&IpcRequest::FanBoost { enable: enabled });
                        match resp {
                            Ok(r) => {
                                let _ = res_tx_clone.send(IpcResult::ActionResponse {
                                    success: r.success,
                                    message: r.message.or(r.error).unwrap_or_default(),
                                });
                                if let Ok(info) = fetch_system_info() {
                                    let _ = res_tx_clone.send(IpcResult::Info(Box::new(info)));
                                }
                            }
                            Err(e) => {
                                let _ = res_tx_clone.send(IpcResult::Error(e));
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
                                let _ = res_tx_clone.send(IpcResult::ActionResponse {
                                    success: true,
                                    message: msg,
                                });
                            }
                            Err(e) => {
                                let _ = res_tx_clone.send(IpcResult::Error(format!(
                                    "配置开机自启失败: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
            }
        });

        // Initialize system tray integration
        let (tray_tx, tray_rx) = channel::<TrayCommand>();
        let tray = MechrevoTray {
            active_profile: "balanced".to_string(),
            fan_boost: false,
            cpu_temp: None,
            gpu_temp: None,
            gpu_d3cold: false,
            battery_th: Some(80),
            ac_online: true,
            tray_tx,
            ctx: cc.egui_ctx.clone(),
        };

        let tray_handle: Option<TrayHandle<MechrevoTray>> = match tray.spawn() {
            Ok(handle) => Some(handle),
            Err(e) => {
                eprintln!("[!] 启动系统托盘服务失败 (非致命警告): {}", e);
                None
            }
        };

        Self {
            info: None,
            action_tx,
            result_rx,
            status_message: None,
            auto_refresh: true,
            last_refresh: Instant::now(),
            custom_battery_th: 80,
            custom_kbd_br: 50,
            is_loading: false,
            tray_handle,
            tray_rx,
            window_visible: true,
            minimize_to_tray: true,
            autostart: is_autostart_enabled(),
        }
    }
}

impl eframe::App for MechrevoGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process incoming commands from system tray
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCommand::ToggleWindow => {
                    self.window_visible = !self.window_visible;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.window_visible));
                    if self.window_visible {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
                TrayCommand::ShowWindow => {
                    self.window_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayCommand::HideWindow => {
                    self.window_visible = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                TrayCommand::SetProfile(profile) => {
                    let _ = self.action_tx.send(IpcAction::SetProfile(profile));
                }
                TrayCommand::SetFanBoost(enabled) => {
                    let _ = self.action_tx.send(IpcAction::SetFanBoost(enabled));
                }
                TrayCommand::SetBattery(threshold) => {
                    let _ = self.action_tx.send(IpcAction::SetBattery(threshold));
                }
                TrayCommand::Quit => {
                    if let Some(ref handle) = self.tray_handle {
                        let _ = handle.shutdown();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Intercept window close request to minimize to tray if enabled
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.minimize_to_tray && self.tray_handle.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.window_visible = false;
            } else if let Some(ref handle) = self.tray_handle {
                let _ = handle.shutdown();
            }
        }

        // Global Ctrl+Q shortcut to cleanly exit application
        if ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(egui::Key::Q)) {
            if let Some(ref handle) = self.tray_handle {
                let _ = handle.shutdown();
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Poll incoming responses from background worker
        while let Ok(res) = self.result_rx.try_recv() {
            self.is_loading = false;
            match res {
                IpcResult::Info(info) => {
                    self.custom_battery_th = info.battery_threshold.unwrap_or(self.custom_battery_th);
                    self.custom_kbd_br = info.keyboard_brightness.unwrap_or(self.custom_kbd_br);
                    if let Some(ref handle) = self.tray_handle {
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
                    self.info = Some(*info);
                }
                IpcResult::ActionResponse { success, message } => {
                    self.status_message = Some((message, !success, Instant::now()));
                }
                IpcResult::Error(err) => {
                    self.status_message = Some((err, true, Instant::now()));
                }
            }
        }

        // Global ergonomic keyboard shortcuts
        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
                let _ = self.action_tx.send(IpcAction::SetProfile("turbo".to_string()));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num2)) {
                let _ = self.action_tx.send(IpcAction::SetProfile("balanced".to_string()));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num3)) {
                let _ = self.action_tx.send(IpcAction::SetProfile("office".to_string()));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::B)) {
                let current_boost = self.info.as_ref().and_then(|i| i.fan_boost).unwrap_or(false);
                let _ = self.action_tx.send(IpcAction::SetFanBoost(!current_boost));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F5) || (i.modifiers.ctrl && i.key_pressed(egui::Key::R))) {
                self.is_loading = true;
                let _ = self.action_tx.send(IpcAction::Refresh);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.status_message = None;
            }
        }

        // Auto-refresh hardware sensors every 2 seconds
        if self.auto_refresh && self.last_refresh.elapsed() >= Duration::from_secs(2) {
            let _ = self.action_tx.send(IpcAction::Refresh);
            self.last_refresh = Instant::now();
        }

        // Auto-dismiss status messages after 5 seconds
        if let Some((_, _, time)) = self.status_message {
            if time.elapsed() > Duration::from_secs(5) {
                self.status_message = None;
            }
        }

        // Request continuous repaint if window is visible and auto_refresh is active,
        // or deep idle sleep when hidden in tray (Rule B: Zero-CPU Idle Scheduling)
        if !self.window_visible {
            ctx.request_repaint_after(Duration::from_secs(2));
        } else if self.auto_refresh {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        // 1. Bottom Shortcut & Status Bar (0-CLS Stabilized)
        let (has_status, is_err) = match self.status_message {
            Some((_, err, _)) => (true, err),
            None => (false, false),
        };

        let bottom_bg = if has_status {
            if is_err {
                Color32::from_rgb(46, 20, 24)
            } else {
                Color32::from_rgb(14, 34, 44)
            }
        } else {
            theme::BG_PANEL
        };

        let bottom_stroke = if has_status {
            if is_err {
                Stroke::new(1.0_f32, theme::ACCENT_RED)
            } else {
                Stroke::new(1.0_f32, theme::ACCENT_CYAN)
            }
        } else {
            Stroke::new(1.0_f32, theme::BORDER_SUBTLE)
        };

        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(
                egui::Frame::none()
                    .fill(bottom_bg)
                    .stroke(bottom_stroke)
                    .inner_margin(egui::Margin::symmetric(16.0, 7.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some((ref msg, is_err, _)) = self.status_message {
                        let (title, detail) = format_status_message(msg, is_err);
                        let (badge_bg, icon, title_color, detail_color) = if is_err {
                            (
                                Color32::from_rgb(185, 28, 28),
                                "⚠ 执行失败",
                                Color32::from_rgb(254, 202, 202),
                                Color32::from_rgb(252, 165, 165),
                            )
                        } else {
                            (
                                Color32::from_rgb(8, 145, 178),
                                "✔ 生效成功",
                                Color32::WHITE,
                                Color32::from_rgb(153, 246, 228),
                            )
                        };

                        render_badge(ui, icon, badge_bg, Color32::WHITE);
                        ui.add_space(4.0);

                        ui.add(
                            egui::Label::new(
                                RichText::new(&title)
                                    .color(title_color)
                                    .strong()
                                    .size(12.0),
                            )
                            .truncate(true),
                        );

                        if let Some(ref det) = detail {
                            ui.add_space(4.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("• {}", det))
                                        .color(detail_color)
                                        .size(11.0),
                                )
                                .truncate(true),
                            );
                        }

                        ui.add_space(6.0);
                        let close_btn = ui.small_button(
                            RichText::new("✕ 关闭")
                                .color(Color32::from_rgb(203, 213, 225))
                                .size(10.5),
                        );
                        if close_btn.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if close_btn.clicked() {
                            self.status_message = None;
                        }
                    } else {
                        ui.label(
                            RichText::new("快捷键: [1] 狂暴  [2] 平衡  [3] 办公  [B] 强冷  [R/F5] 刷新  [Ctrl+Q] 退出")
                                .color(theme::TEXT_SECONDARY)
                                .size(11.5),
                        );
                    }

                    // Right anchored controls (Permanent, zero layout shift)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let chassis_str = self
                            .info
                            .as_ref()
                            .map(|i| i.chassis.as_str())
                            .unwrap_or("MECHREVO Laptop");
                        ui.label(
                            RichText::new(chassis_str)
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.add_space(8.0);
                        ui.checkbox(&mut self.minimize_to_tray, "关闭最小化到托盘")
                            .on_hover_text("点击关闭窗口按钮时最小化到系统托盘，而非退出程序");
                        ui.add_space(8.0);

                        let autostart_resp = ui.checkbox(&mut self.autostart, "开机静默自启");
                        if autostart_resp.changed() {
                            let _ = self.action_tx.send(IpcAction::SetAutostart(self.autostart));
                        }
                        autostart_resp.on_hover_text("开机登录桌面后自动静默启动并常驻系统托盘");
                    });
                });
            });

        // 2. Top Navigation Bar
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_PANEL)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                    .inner_margin(egui::Margin::symmetric(16.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("MECHREVO")
                            .color(theme::ACCENT_CYAN)
                            .strong()
                            .size(16.5),
                    );
                    ui.label(
                        RichText::new("控制中心")
                            .color(Color32::WHITE)
                            .strong()
                            .size(16.5),
                    );
                    ui.label(
                        RichText::new("v0.2.0")
                            .color(theme::TEXT_MUTED)
                            .size(11.0),
                    );

                    if let Some(ref info) = self.info {
                        ui.label(
                            RichText::new(format!("• {}", info.chassis))
                                .color(theme::TEXT_SECONDARY)
                                .size(11.5),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = ui.button(if self.is_loading { "刷新中..." } else { "刷新" });
                        if btn.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if btn.clicked() {
                            self.is_loading = true;
                            let _ = self.action_tx.send(IpcAction::Refresh);
                        }

                        ui.checkbox(&mut self.auto_refresh, "自动遥测 (2s)");

                        if let Some(ref info) = self.info {
                            if info.ac_online {
                                ui.label(
                                    RichText::new("[AC 适配器供电]")
                                        .color(theme::ACCENT_GREEN)
                                        .size(11.5),
                                );
                            } else {
                                ui.label(
                                    RichText::new("[正在使用电池]")
                                        .color(theme::ACCENT_ORANGE)
                                        .size(11.5),
                                );
                            }
                        }
                    });
                });
            });

        // 3. Central Scrollable Content
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_BASE)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Module 1: Performance Release Profiles
                        self.render_profile_section(ui);
                        ui.add_space(10.0);

                        // Module 2: Cooling & Fan Boost
                        self.render_cooling_section(ui);
                        ui.add_space(10.0);

                        // Module 3: Battery Conservation
                        self.render_battery_section(ui);
                        ui.add_space(10.0);

                        // Module 4: Keyboard Backlight
                        self.render_keyboard_section(ui);
                        ui.add_space(10.0);

                        // Module 5: Live Telemetry Dashboard
                        self.render_telemetry_section(ui);
                        ui.add_space(12.0);
                    });
            });
    }
}

impl MechrevoGuiApp {
    fn render_profile_card(
        ui: &mut egui::Ui,
        title: &str,
        shortcut: &str,
        specs: &[&str],
        is_active: bool,
        accent_color: Color32,
        active_bg: Color32,
    ) -> bool {
        let mut clicked = false;
        let stroke = if is_active {
            Stroke::new(2.0_f32, accent_color)
        } else {
            Stroke::new(1.0_f32, theme::BORDER_CARD)
        };
        let bg = if is_active {
            active_bg
        } else {
            theme::BG_CARD_SUBTLE
        };

        let frame = egui::Frame::none()
            .fill(bg)
            .stroke(stroke)
            .rounding(Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0));

        let card = frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title)
                        .color(if is_active { accent_color } else { Color32::WHITE })
                        .strong()
                        .size(13.5),
                );
                ui.label(
                    RichText::new(shortcut)
                        .color(theme::TEXT_MUTED)
                        .size(11.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_active {
                        render_badge(ui, "● 生效中", accent_color, Color32::WHITE);
                    } else {
                        ui.label(
                            RichText::new("切换")
                                .color(theme::TEXT_SECONDARY)
                                .size(11.0),
                        );
                    }
                });
            });
            ui.add_space(5.0);
            for spec in specs {
                ui.label(
                    RichText::new(format!("• {}", spec))
                        .color(Color32::from_rgb(160, 175, 195))
                        .size(11.0),
                );
            }
            ui.add_space(6.0);

            let btn_label = if is_active { "当前模式已生效" } else { "点击切换此模式" };
            let btn = egui::Button::new(
                RichText::new(btn_label)
                    .color(if is_active { Color32::WHITE } else { Color32::from_rgb(203, 213, 225) })
                    .size(11.5),
            )
            .fill(if is_active { accent_color } else { theme::BTN_INACTIVE_BG })
            .rounding(Rounding::same(6.0_f32));

            if ui.add_sized([ui.available_width(), 24.0], btn).clicked() && !is_active {
                clicked = true;
            }
        });

        // Make the entire card area clickable
        let card_interact = ui.interact(card.response.rect, card.response.id.with("card_click"), egui::Sense::click());
        if card_interact.hovered() && !is_active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if card_interact.clicked() && !is_active {
            clicked = true;
        }

        clicked
    }

    fn render_profile_section(&mut self, ui: &mut egui::Ui) {
        let current_profile = self
            .info
            .as_ref()
            .map(|i| i.active_profile.as_str())
            .unwrap_or("balanced");

        egui::Frame::none()
            .fill(theme::BG_CARD)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("性能释放模式 (PERFORMANCE PROFILES)")
                            .color(theme::TEXT_SECONDARY)
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("点击卡片或按键 [1-3] 极速切换")
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                    });
                });
                ui.add_space(8.0);

                ui.columns(3, |columns| {
                    // 1. Turbo
                    if Self::render_profile_card(
                        &mut columns[0],
                        "狂暴满血 (Turbo)",
                        "[1]",
                        &[
                            "CPU PL1 105W / PL2 135W",
                            "GPU 140W 满血释放",
                            "EPP: Performance 电竞策略",
                            "EC M3T1 温控曲线接管",
                        ],
                        current_profile == "turbo",
                        theme::ACCENT_RED,
                        Color32::from_rgb(45, 22, 25),
                    ) {
                        let _ = self.action_tx.send(IpcAction::SetProfile("turbo".to_string()));
                    }

                    // 2. Balanced
                    if Self::render_profile_card(
                        &mut columns[1],
                        "日常平衡 (Balanced)",
                        "[2]",
                        &[
                            "CPU PL1 75W / PL2 90W",
                            "GPU 115W 基准释放",
                            "EPP: Balance 均衡发热",
                            "独显空载自然沉入 D3cold",
                        ],
                        current_profile == "balanced",
                        theme::ACCENT_CYAN,
                        Color32::from_rgb(18, 38, 48),
                    ) {
                        let _ = self.action_tx.send(IpcAction::SetProfile("balanced".to_string()));
                    }

                    // 3. Office
                    if Self::render_profile_card(
                        &mut columns[2],
                        "办公静音 (Office)",
                        "[3]",
                        &[
                            "CPU PL1 45W / PL2 60W",
                            "独显 PCIe RTD3 深度断电 0W",
                            "EPP: Power 极致省电",
                            "超长电池待机续航",
                        ],
                        current_profile == "office",
                        theme::ACCENT_GREEN,
                        Color32::from_rgb(16, 38, 30),
                    ) {
                        let _ = self.action_tx.send(IpcAction::SetProfile("office".to_string()));
                    }
                });
            });
    }

    fn render_cooling_section(&mut self, ui: &mut egui::Ui) {
        let is_boost = self.info.as_ref().and_then(|i| i.fan_boost).unwrap_or(false);

        egui::Frame::none()
            .fill(theme::BG_CARD)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("散热与极限强冷 (COOLING & FAN BOOST)")
                            .color(theme::TEXT_SECONDARY)
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let status_text = if is_boost {
                            RichText::new("[ 100% 满血强冷: ON ]")
                                .color(theme::ACCENT_RED)
                                .strong()
                        } else {
                            RichText::new("[ EC 智能温控: OFF ]")
                                .color(theme::TEXT_SECONDARY)
                        };
                        ui.label(status_text);
                    });
                });
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (btn_label, btn_color, target) = if is_boost {
                            ("关闭强冷 (恢复自动)", Color32::from_rgb(14, 116, 144), false)
                        } else {
                            ("开启 100% 极限强冷", Color32::from_rgb(220, 38, 38), true)
                        };

                        let btn = egui::Button::new(
                            RichText::new(btn_label)
                                .color(Color32::WHITE)
                                .strong()
                                .size(12.0),
                        )
                        .fill(btn_color)
                        .rounding(Rounding::same(6.0_f32));

                        let resp = ui.add_sized([160.0, 30.0], btn);
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            let _ = self.action_tx.send(IpcAction::SetFanBoost(target));
                        }

                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("双风扇 100% 极限飓风强冷 (快捷键: B)")
                                        .color(Color32::WHITE)
                                        .strong()
                                        .size(13.0),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new("接管 Uniwill EC RAM 0x0751 强冷寄存器，拉满双风扇 100% 转速；适合 3A 游戏与高压拷机。")
                                            .color(theme::TEXT_SECONDARY)
                                            .size(11.0),
                                    )
                                    .wrap(true),
                                );
                            });
                        });
                    });
                });
            });
    }

    fn render_battery_section(&mut self, ui: &mut egui::Ui) {
        let (percent, status, current_th) = self
            .info
            .as_ref()
            .map(|i| {
                (
                    i.battery_percent.unwrap_or(100),
                    i.battery_status.as_deref().unwrap_or("Full"),
                    i.battery_threshold.unwrap_or(80),
                )
            })
            .unwrap_or((100, "Full", 80));

        egui::Frame::none()
            .fill(theme::BG_CARD)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("电池健康与充电养护 (BATTERY CONSERVATION)")
                            .color(theme::TEXT_SECONDARY)
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("当前电量: {}% ({}) | 当前养护上限: {}%", percent, status, current_th))
                                .color(theme::TEXT_SECONDARY)
                                .size(11.5),
                        );
                    });
                });
                ui.add_space(8.0);

                // Progress Bar
                let frac = (percent as f32 / 100.0).clamp(0.0, 1.0);
                ui.add(
                    egui::ProgressBar::new(frac)
                        .show_percentage()
                        .animate(false),
                );
                ui.add_space(8.0);

                // Row with Presets on Left and Custom Slider on Right
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Right side: Custom slider & Apply button
                        let btn = ui.button(RichText::new("应用阈值").size(11.5));
                        if btn.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if btn.clicked() {
                            let _ = self.action_tx.send(IpcAction::SetBattery(self.custom_battery_th));
                        }
                        ui.add_sized([90.0, 20.0], egui::Slider::new(&mut self.custom_battery_th, 50..=100).suffix("%"));

                        // Left side: Presets
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(RichText::new("预设:").size(12.0).color(theme::TEXT_SECONDARY));

                            if render_pill_button(
                                ui,
                                "80% 常年插电",
                                current_th == 80,
                                theme::ACCENT_CYAN,
                            ) {
                                self.custom_battery_th = 80;
                                let _ = self.action_tx.send(IpcAction::SetBattery(80));
                            }

                            if render_pill_button(
                                ui,
                                "100% 满电备战",
                                current_th == 100,
                                theme::ACCENT_GREEN,
                            ) {
                                self.custom_battery_th = 100;
                                let _ = self.action_tx.send(IpcAction::SetBattery(100));
                            }

                            if render_pill_button(
                                ui,
                                "60% 深度养护",
                                current_th == 60,
                                theme::ACCENT_ORANGE,
                            ) {
                                self.custom_battery_th = 60;
                                let _ = self.action_tx.send(IpcAction::SetBattery(60));
                            }
                        });
                    });
                });
            });
    }

    fn render_keyboard_section(&mut self, ui: &mut egui::Ui) {
        let current_br = self
            .info
            .as_ref()
            .and_then(|i| i.keyboard_brightness)
            .unwrap_or(50);

        egui::Frame::none()
            .fill(theme::BG_CARD)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("键盘多色背光调节 (KEYBOARD BACKLIGHT)")
                            .color(theme::TEXT_SECONDARY)
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("当前背光: {}%", current_br))
                                .color(theme::TEXT_SECONDARY)
                                .size(11.5),
                        );
                    });
                });
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Right side: Custom slider
                        let slider = ui.add_sized([100.0, 20.0], egui::Slider::new(&mut self.custom_kbd_br, 0..=100).suffix("%"));
                        if slider.drag_stopped() || (slider.changed() && !slider.has_focus()) {
                            let _ = self.action_tx.send(IpcAction::SetKeyboard(self.custom_kbd_br));
                        }

                        // Left side: Preset pills
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let levels: [(u32, &str); 5] = [
                                (0, "关闭 0%"),
                                (25, "弱 25%"),
                                (50, "中 50%"),
                                (75, "亮 75%"),
                                (100, "极亮 100%"),
                            ];

                            for (val, label) in levels {
                                let is_active = match val {
                                    0 => current_br == 0,
                                    25 => current_br > 0 && current_br <= 35,
                                    50 => current_br > 35 && current_br <= 65,
                                    75 => current_br > 65 && current_br <= 85,
                                    100 => current_br > 85,
                                    _ => false,
                                };

                                if render_pill_button(ui, label, is_active, theme::ACCENT_CYAN) {
                                    self.custom_kbd_br = val;
                                    let _ = self.action_tx.send(IpcAction::SetKeyboard(val));
                                }
                            }
                        });
                    });
                });
            });
    }

    fn render_telemetry_section(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(theme::BG_CARD)
            .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("实时硬件传感器与状态遥测 (LIVE TELEMETRY)")
                        .color(theme::TEXT_SECONDARY)
                        .strong()
                        .size(13.0),
                );
                ui.add_space(8.0);

                ui.columns(3, |columns| {
                    // CPU Tile
                    egui::Frame::none()
                        .fill(theme::BG_CARD_SUBTLE)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_CARD))
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                        .show(&mut columns[0], |ui| {
                            ui.label(RichText::new("中央处理器 (CPU)").color(theme::TEXT_MUTED).size(11.0));
                            ui.add_space(2.0);
                            if let Some(ref info) = self.info {
                                let pl_str = format_cpu_pl(info.cpu_pl1_w, info.cpu_pl2_w);
                                ui.label(RichText::new(pl_str).color(Color32::WHITE).strong().monospace().size(12.5));
                                
                                let (temp_str, temp_color) = classify_cpu_temp(info.cpu_temp_c);
                                let epp = info.cpu_epp.as_deref().unwrap_or("balance");
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("温度: {}", temp_str)).color(temp_color).monospace().size(11.0));
                                    ui.label(RichText::new(format!("EPP: {}", epp)).color(theme::TEXT_SECONDARY).size(11.0));
                                });
                            } else {
                                ui.label(RichText::new("读取中...").color(theme::TEXT_MUTED));
                            }
                        });

                    // GPU Tile
                    egui::Frame::none()
                        .fill(theme::BG_CARD_SUBTLE)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_CARD))
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                        .show(&mut columns[1], |ui| {
                            let gpu_title = self.info.as_ref()
                                .and_then(|i| i.gpu_name.as_deref())
                                .unwrap_or("独立显卡 (NVIDIA dGPU)");
                            ui.add(
                                egui::Label::new(
                                    RichText::new(gpu_title)
                                        .color(theme::TEXT_MUTED)
                                        .size(11.0),
                                )
                                .truncate(true),
                            );
                            ui.add_space(2.0);
                            if let Some(ref info) = self.info {
                                if info.gpu_d3cold {
                                    ui.label(RichText::new("● PCIe RTD3 深度休眠").color(theme::ACCENT_GREEN).strong().size(12.5));
                                    ui.label(RichText::new("0W 功耗 · 零发热零耗电待机").color(theme::TEXT_SECONDARY).size(11.0));
                                } else {
                                    let p_str = format_gpu_power(info.gpu_power_w);
                                    ui.label(RichText::new(p_str).color(Color32::WHITE).strong().monospace().size(12.5));

                                    let (temp_str, temp_color) = classify_gpu_temp(info.gpu_temp_c);
                                    ui.label(RichText::new(format!("核心温度: {}", temp_str)).color(temp_color).monospace().size(11.0));
                                }
                            } else {
                                ui.label(RichText::new("读取中...").color(theme::TEXT_MUTED));
                            }
                        });

                    // Power / Fans Tile
                    egui::Frame::none()
                        .fill(theme::BG_CARD_SUBTLE)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_CARD))
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                        .show(&mut columns[2], |ui| {
                            ui.label(RichText::new("供电输入与双风扇").color(theme::TEXT_MUTED).size(11.0));
                            ui.add_space(2.0);
                            if let Some(ref info) = self.info {
                                if info.ac_online {
                                    ui.label(RichText::new("● 原装适配器已连接").color(theme::ACCENT_GREEN).strong().size(12.5));
                                } else {
                                    ui.label(RichText::new("● 正在使用电池供电").color(theme::ACCENT_ORANGE).strong().size(12.5));
                                }

                                let fan_str = format_fan_status(info.fan1_rpm, info.fan2_rpm, info.fan_boost);
                                ui.label(RichText::new(fan_str).color(theme::ACCENT_CYAN).monospace().size(11.0));
                            } else {
                                ui.label(RichText::new("读取中...").color(theme::TEXT_MUTED));
                            }
                        });
                });
            });
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let start_minimized = args.iter().any(|a| a == "--minimized" || a == "--tray");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(880.0, 720.0))
            .with_min_inner_size(Vec2::new(800.0, 600.0))
            .with_title("机械革命控制中心 - MECHREVO Control Center")
            .with_visible(!start_minimized),
        ..Default::default()
    };

    eframe::run_native(
        "mechrevo-gui",
        native_options,
        Box::new(move |cc| {
            let mut app = MechrevoGuiApp::new(cc);
            app.window_visible = !start_minimized;
            Box::new(app)
        }),
    )
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

    #[test]
    fn test_telemetry_pure_functions() {
        // CPU PL formatting
        assert_eq!(format_cpu_pl(Some(75.0), Some(90.0)), "PL1: 75W | PL2: 90W");
        assert_eq!(format_cpu_pl(Some(105.0), None), "PL1: 105W | PL2: --W");
        assert_eq!(format_cpu_pl(None, None), "PL1: --W | PL2: --W");

        // CPU & GPU Temperature Classification
        let (cpu_str, cpu_color) = classify_cpu_temp(Some(85));
        assert_eq!(cpu_str, "85°C (高温)");
        assert_eq!(cpu_color, theme::ACCENT_RED);

        let (cpu_str_warn, cpu_color_warn) = classify_cpu_temp(Some(70));
        assert_eq!(cpu_str_warn, "70°C (温热)");
        assert_eq!(cpu_color_warn, theme::ACCENT_ORANGE);

        let (cpu_str_norm, cpu_color_norm) = classify_cpu_temp(Some(55));
        assert_eq!(cpu_str_norm, "55°C (正常)");
        assert_eq!(cpu_color_norm, theme::ACCENT_CYAN);

        let (gpu_str_cool, gpu_color_cool) = classify_gpu_temp(Some(45));
        assert_eq!(gpu_str_cool, "45°C (清凉)");
        assert_eq!(gpu_color_cool, theme::ACCENT_CYAN);

        let (none_str, none_color) = classify_cpu_temp(None);
        assert_eq!(none_str, "--°C");
        assert_eq!(none_color, theme::TEXT_MUTED);

        // Fan RPM and Boost formatting
        assert_eq!(format_fan_status(Some(2400), Some(2600), Some(false)), "F1: 2400 | F2: 2600 RPM");
        assert_eq!(format_fan_status(Some(3000), None, Some(false)), "F1: 3000 RPM");
        assert_eq!(format_fan_status(None, None, Some(true)), "双风扇 100% 满速强冷");
        assert_eq!(format_fan_status(None, None, Some(false)), "EC 固件智能调速");

        // GPU Power formatting
        assert_eq!(format_gpu_power(Some(140.0)), "活跃 · 功耗上限 140W");
        assert_eq!(format_gpu_power(Some(0.0)), "活跃 · 基础功耗释放");
        assert_eq!(format_gpu_power(None), "活跃 · 基础功耗释放");
    }

    #[test]
    fn test_font_loading() {
        for path in &[
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if std::path::Path::new(path).exists() {
                let bytes = std::fs::read(path).unwrap();
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    "cjk_fallback".to_owned(),
                    egui::FontData::from_owned(bytes),
                );
                if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    vec.push("cjk_fallback".to_owned());
                }
                let ctx = egui::Context::default();
                ctx.set_fonts(fonts);

                let _ = ctx.run(Default::default(), |ctx| {
                    let galley = ctx.fonts(|f| {
                        f.layout_no_wrap(
                            "机械革命 控制中心 狂暴模式 75W".to_owned(),
                            egui::FontId::proportional(14.0),
                            egui::Color32::WHITE,
                        )
                    });
                    assert!(!galley.rows.is_empty());
                    assert!(galley.size().x > 10.0);
                });
            }
        }
    }

    #[test]
    fn test_format_status_message() {
        // Success scenarios
        let (title, detail) = format_status_message("Successfully applied profile 'turbo'", false);
        assert!(title.contains("狂暴满血模式"));
        assert!(detail.is_some());

        let (title, detail) = format_status_message("Battery charge threshold set to 80%", false);
        assert_eq!(title, "已成功配置电池充电养护阈值: 80%");
        assert!(detail.unwrap().contains("state.json"));

        // Error scenarios
        let (title, detail) = format_status_message("Hardware sysfs node or interface not accessible: Sysfs node not found: /sys/class/power_supply/BAT0/charge_control_end_threshold", true);
        assert_eq!(title, "电池充电养护阈值配置失败: 硬件节点不可用");
        assert!(detail.unwrap().contains("setup-tuxedo-drivers.sh"));

        let (title, detail) = format_status_message("Permission denied when writing battery threshold", true);
        assert!(title.contains("电池充电养护阈值配置失败") || title.contains("权限不足"));
        assert!(detail.is_some());
    }

    #[test]
    fn test_autostart_content_parsing() {
        let normal = "[Desktop Entry]\nType=Application\nExec=mechrevo-gui --minimized\n";
        assert!(is_autostart_content_enabled(normal));

        let hidden = "[Desktop Entry]\nType=Application\nHidden=true\nExec=mechrevo-gui --minimized\n";
        assert!(!is_autostart_content_enabled(hidden));

        let gnome_disabled = "[Desktop Entry]\nType=Application\nX-GNOME-Autostart-enabled=false\n";
        assert!(!is_autostart_content_enabled(gnome_disabled));

        let gnome_enabled = "[Desktop Entry]\nType=Application\nX-GNOME-Autostart-enabled=true\n";
        assert!(is_autostart_content_enabled(gnome_enabled));
    }

    #[test]
    fn test_format_tray_tooltip() {
        // Office mode with GPU suspended in D3cold (User scenario from screenshot)
        let (title, desc) = format_tray_tooltip("office", Some(56), None, true, true, false);
        assert_eq!(title, "机械革命控制中心 - 办公静音 (45W/0W)");
        assert_eq!(desc, "CPU: 56°C | GPU: 深度休眠 (0W) | AC 适配器供电");

        // Turbo mode under active load with fan boost
        let (title, desc) = format_tray_tooltip("turbo", Some(78), Some(65), false, true, true);
        assert_eq!(title, "机械革命控制中心 - 狂暴满血 (105W/140W)");
        assert_eq!(desc, "CPU: 78°C | GPU: 65°C | AC 适配器供电 | 强冷开启");

        // Battery power without dGPU
        let (title, desc) = format_tray_tooltip("balanced", Some(48), None, false, false, false);
        assert_eq!(title, "机械革命控制中心 - 日常平衡 (75W/115W)");
        assert_eq!(desc, "CPU: 48°C | GPU: 离线 | 电池供电");
    }
}
