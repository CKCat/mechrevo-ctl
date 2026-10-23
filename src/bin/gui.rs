//! Native GUI for MECHREVO Control Center.
//! Ultra-lightweight, high-performance, native Rust GUI using eframe/egui.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use mechrevo::daemon::{IpcRequest, IpcResponse, SystemInfoReport, DEFAULT_SOCKET_PATH};

enum IpcAction {
    SetProfile(String),
    SetBattery(u32),
    SetKeyboard(u32),
    SetFanBoost(bool),
    Refresh,
}

enum IpcResult {
    Info(SystemInfoReport),
    ActionResponse { success: bool, message: String },
    Error(String),
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

    let ac_online = std::fs::read_to_string("/sys/class/power_supply/AC0/online")
        .or_else(|_| std::fs::read_to_string("/sys/class/power_supply/AC/online"))
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

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut font_candidates = vec![
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf".to_string(),
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc".to_string(),
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc".to_string(),
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".to_string(),
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc".to_string(),
        "/usr/share/fonts/truetype/arphic/uming.ttc".to_string(),
        "/usr/share/fonts/truetype/arphic/ukai.ttc".to_string(),
    ];

    if let Ok(home) = std::env::var("HOME") {
        font_candidates.push(format!("{}/.local/share/fonts/NotoSansCJK-Regular.ttc", home));
        font_candidates.push(format!("{}/.local/share/fonts/DroidSansFallbackFull.ttf", home));
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

/// Renders a clickable preset pill button with active indicator
fn render_pill_button(
    ui: &mut egui::Ui,
    label: &str,
    is_active: bool,
    accent_color: Color32,
) -> bool {
    let (bg, stroke, text_color) = if is_active {
        (
            Color32::from_rgb(26, 44, 66),
            Stroke::new(1.5_f32, accent_color),
            Color32::WHITE,
        )
    } else {
        (
            Color32::from_rgb(24, 32, 48),
            Stroke::new(1.0_f32, Color32::from_rgb(46, 60, 84)),
            Color32::from_rgb(180, 195, 215),
        )
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
            .size(11.5),
    )
    .fill(bg)
    .stroke(stroke)
    .rounding(Rounding::same(6.0_f32));

    let response = ui.add(button);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
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
}

impl MechrevoGuiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Setup CJK fallback font for Chinese character rendering
        setup_custom_fonts(&cc.egui_ctx);

        // Customize egui theme
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(Color32::from_rgb(226, 232, 240));
        visuals.panel_fill = Color32::from_rgb(16, 20, 29);
        visuals.window_fill = Color32::from_rgb(16, 20, 29);
        cc.egui_ctx.set_visuals(visuals);

        let (action_tx, action_rx) = channel::<IpcAction>();
        let (result_tx, result_rx) = channel::<IpcResult>();

        // Background worker thread for zero UI lag
        let res_tx_clone = result_tx.clone();
        thread::spawn(move || {
            // Initial fetch
            if let Ok(info) = fetch_system_info() {
                let _ = res_tx_clone.send(IpcResult::Info(info));
            }

            while let Ok(action) = action_rx.recv() {
                match action {
                    IpcAction::Refresh => match fetch_system_info() {
                        Ok(info) => {
                            let _ = res_tx_clone.send(IpcResult::Info(info));
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
                                    let _ = res_tx_clone.send(IpcResult::Info(info));
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
                                    let _ = res_tx_clone.send(IpcResult::Info(info));
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
                                    let _ = res_tx_clone.send(IpcResult::Info(info));
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
                                    let _ = res_tx_clone.send(IpcResult::Info(info));
                                }
                            }
                            Err(e) => {
                                let _ = res_tx_clone.send(IpcResult::Error(e));
                            }
                        }
                    }
                }
            }
        });

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
        }
    }
}

impl eframe::App for MechrevoGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll incoming responses from background worker
        while let Ok(res) = self.result_rx.try_recv() {
            self.is_loading = false;
            match res {
                IpcResult::Info(info) => {
                    self.custom_battery_th = info.battery_threshold.unwrap_or(self.custom_battery_th);
                    self.custom_kbd_br = info.keyboard_brightness.unwrap_or(self.custom_kbd_br);
                    self.info = Some(info);
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

        // Auto-dismiss status messages after 4 seconds
        if let Some((_, _, time)) = self.status_message {
            if time.elapsed() > Duration::from_secs(4) {
                self.status_message = None;
            }
        }

        // Request continuous repaint if auto-refresh is active
        ctx.request_repaint_after(Duration::from_millis(500));

        // 1. Top Navigation Bar
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(20, 26, 38))
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("MECHREVO")
                            .color(Color32::from_rgb(0, 242, 254))
                            .strong()
                            .size(17.0),
                    );
                    ui.label(
                        RichText::new("控制中心")
                            .color(Color32::WHITE)
                            .strong()
                            .size(17.0),
                    );
                    ui.label(
                        RichText::new("v0.2.0")
                            .color(Color32::from_rgb(100, 116, 139))
                            .size(11.0),
                    );

                    if let Some(ref info) = self.info {
                        ui.label(
                            RichText::new(format!("• 机型: {}", info.chassis))
                                .color(Color32::from_rgb(148, 163, 184))
                                .size(12.0),
                        );
                    }

                    // Shortcut hints in header
                    ui.label(
                        RichText::new("快捷键: [1-3] 模式 | [B] 强冷 | [R] 刷新")
                            .color(Color32::from_rgb(100, 116, 139))
                            .size(11.0),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = ui.button(if self.is_loading { "刷新中..." } else { "刷新" });
                        if btn.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if btn.clicked() {
                            self.is_loading = true;
                            let _ = self.action_tx.send(IpcAction::Refresh);
                        }

                        ui.checkbox(&mut self.auto_refresh, "实时遥测 (2s)");

                        if let Some(ref info) = self.info {
                            if info.ac_online {
                                ui.label(
                                    RichText::new("[AC 适配器供电]")
                                        .color(Color32::from_rgb(52, 211, 153))
                                        .size(12.0),
                                );
                            } else {
                                ui.label(
                                    RichText::new("[正在使用电池]")
                                        .color(Color32::from_rgb(251, 191, 36))
                                        .size(12.0),
                                );
                            }
                        }
                    });
                });
            });

        // 2. Central Scrollable Content
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(16, 20, 29))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Toast banner
                        if let Some((ref msg, is_err, _)) = self.status_message {
                            let (bg, border) = if is_err {
                                (Color32::from_rgb(60, 20, 20), Color32::from_rgb(239, 68, 68))
                            } else {
                                (Color32::from_rgb(20, 45, 45), Color32::from_rgb(6, 182, 212))
                            };
                            let banner = egui::Frame::none()
                                .fill(bg)
                                .stroke(Stroke::new(1.0_f32, border))
                                .rounding(Rounding::same(6.0_f32))
                                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(msg).color(Color32::WHITE).size(13.0));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new("点击关闭 ✕").color(Color32::from_rgb(180, 195, 215)).size(11.0));
                                        });
                                    });
                                });
                            if banner.response.interact(egui::Sense::click()).clicked() {
                                self.status_message = None;
                            }
                            ui.add_space(8.0);
                        }

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
                        ui.add_space(14.0);
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
            Stroke::new(1.0_f32, Color32::from_rgb(42, 55, 82))
        };
        let bg = if is_active {
            active_bg
        } else {
            Color32::from_rgb(26, 35, 54)
        };

        let frame = egui::Frame::none()
            .fill(bg)
            .stroke(stroke)
            .rounding(Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::same(12.0));

        let card = frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title)
                        .color(if is_active { accent_color } else { Color32::WHITE })
                        .strong()
                        .size(14.0),
                );
                ui.label(
                    RichText::new(shortcut)
                        .color(Color32::from_rgb(100, 116, 139))
                        .size(11.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_active {
                        ui.label(
                            RichText::new("● 当前生效")
                                .color(Color32::WHITE)
                                .background_color(accent_color)
                                .strong()
                                .size(10.0),
                        );
                    } else {
                        ui.label(
                            RichText::new("点击切换")
                                .color(Color32::from_rgb(148, 163, 184))
                                .size(11.0),
                        );
                    }
                });
            });
            ui.add_space(6.0);
            for spec in specs {
                ui.label(
                    RichText::new(format!("• {}", spec))
                        .color(Color32::from_rgb(160, 175, 195))
                        .size(11.0),
                );
            }
            ui.add_space(8.0);

            let btn_label = if is_active { "当前已启用" } else { "立即切换" };
            let btn = egui::Button::new(
                RichText::new(btn_label)
                    .color(if is_active { Color32::WHITE } else { Color32::from_rgb(203, 213, 225) })
                    .size(11.5),
            )
            .fill(if is_active { accent_color } else { Color32::from_rgb(33, 44, 66) })
            .rounding(Rounding::same(4.0_f32));

            if ui.add_sized([ui.available_width(), 26.0], btn).clicked() && !is_active {
                clicked = true;
            }
        });

        // Make the entire card area clickable
        let card_interact = ui.interact(card.response.rect, card.response.id.with("click"), egui::Sense::click());
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
            .fill(Color32::from_rgb(23, 29, 43))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 50, 72)))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("性能释放模式 (PERFORMANCE PROFILES)")
                            .color(Color32::from_rgb(148, 163, 184))
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("提示: 点击任意卡片或按键 [1-3] 即可极速切换")
                                .color(Color32::from_rgb(100, 116, 139))
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
                        Color32::from_rgb(239, 68, 68),
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
                        Color32::from_rgb(6, 182, 212),
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
                        Color32::from_rgb(16, 185, 129),
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
            .fill(Color32::from_rgb(23, 29, 43))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 50, 72)))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("散热与极限强冷 (COOLING & FAN BOOST)")
                            .color(Color32::from_rgb(148, 163, 184))
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let status_text = if is_boost {
                            RichText::new("[ 100% 满血强冷运行中: ON ]")
                                .color(Color32::from_rgb(248, 113, 113))
                                .strong()
                        } else {
                            RichText::new("[ EC 智能温控曲线接管: OFF ]")
                                .color(Color32::from_rgb(148, 163, 184))
                        };
                        ui.label(status_text);
                    });
                });
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("双风扇 100% 极限飓风强冷")
                                    .color(Color32::WHITE)
                                    .strong()
                                    .size(13.0),
                            );
                            ui.label(
                                RichText::new("快捷键: [B]")
                                    .color(Color32::from_rgb(100, 116, 139))
                                    .size(11.0),
                            );
                        });
                        ui.label(
                            RichText::new(
                                "直接接管 Uniwill EC RAM 0x0751 寄存器强冷位，无视温控曲线拉满 100% 风扇转速；适合 3A 电竞与拷机。",
                            )
                            .color(Color32::from_rgb(148, 163, 184))
                            .size(11.0),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let (btn_label, btn_color, target) = if is_boost {
                            ("关闭强冷 (恢复自动温控)", Color32::from_rgb(14, 116, 144), false)
                        } else {
                            ("开启 100% 极限强冷", Color32::from_rgb(220, 38, 38), true)
                        };

                        let btn = egui::Button::new(
                            RichText::new(btn_label)
                                .color(Color32::WHITE)
                                .strong()
                                .size(12.5),
                        )
                        .fill(btn_color)
                        .rounding(Rounding::same(6.0_f32));

                        let resp = ui.add_sized([180.0, 32.0], btn);
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            let _ = self.action_tx.send(IpcAction::SetFanBoost(target));
                        }
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
            .fill(Color32::from_rgb(23, 29, 43))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 50, 72)))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("电池健康与充电养护 (BATTERY CONSERVATION)")
                            .color(Color32::from_rgb(148, 163, 184))
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("当前电量: {}% ({}) | 当前养护上限: {}%", percent, status, current_th))
                                .color(Color32::from_rgb(148, 163, 184))
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
                ui.add_space(10.0);

                // Preset Pills & Custom Slider
                ui.horizontal(|ui| {
                    ui.label(RichText::new("快速预设:").size(12.0).color(Color32::from_rgb(148, 163, 184)));

                    if render_pill_button(
                        ui,
                        "80% 长寿养护 (常年插电)",
                        current_th == 80,
                        Color32::from_rgb(6, 182, 212),
                    ) {
                        self.custom_battery_th = 80;
                        let _ = self.action_tx.send(IpcAction::SetBattery(80));
                    }

                    if render_pill_button(
                        ui,
                        "100% 满电出行 (备战外出)",
                        current_th == 100,
                        Color32::from_rgb(16, 185, 129),
                    ) {
                        self.custom_battery_th = 100;
                        let _ = self.action_tx.send(IpcAction::SetBattery(100));
                    }

                    if render_pill_button(
                        ui,
                        "60% 深度养护",
                        current_th == 60,
                        Color32::from_rgb(148, 163, 184),
                    ) {
                        self.custom_battery_th = 60;
                        let _ = self.action_tx.send(IpcAction::SetBattery(60));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = ui.button(RichText::new("应用阈值").size(11.5));
                        if btn.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if btn.clicked() {
                            let _ = self.action_tx.send(IpcAction::SetBattery(self.custom_battery_th));
                        }
                        ui.add(egui::Slider::new(&mut self.custom_battery_th, 50..=100).suffix("%"));
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
            .fill(Color32::from_rgb(23, 29, 43))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 50, 72)))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("键盘多色背光调节 (KEYBOARD BACKLIGHT)")
                            .color(Color32::from_rgb(148, 163, 184))
                            .strong()
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("当前背光亮度: {}%", current_br))
                                .color(Color32::from_rgb(148, 163, 184))
                                .size(11.5),
                        );
                    });
                });
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    let levels: [(u32, &str); 5] = [
                        (0, "关闭 (0%)"),
                        (25, "弱 (25%)"),
                        (50, "中 (50%)"),
                        (75, "亮 (75%)"),
                        (100, "极亮 (100%)"),
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

                        if render_pill_button(ui, label, is_active, Color32::from_rgb(0, 242, 254)) {
                            self.custom_kbd_br = val;
                            let _ = self.action_tx.send(IpcAction::SetKeyboard(val));
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let slider = ui.add(egui::Slider::new(&mut self.custom_kbd_br, 0..=100).suffix("%"));
                        if slider.drag_stopped() || (slider.changed() && !slider.has_focus()) {
                            let _ = self.action_tx.send(IpcAction::SetKeyboard(self.custom_kbd_br));
                        }
                    });
                });
            });
    }

    fn render_telemetry_section(&mut self, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(Color32::from_rgb(23, 29, 43))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 50, 72)))
            .rounding(Rounding::same(10.0_f32))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("实时硬件传感器与状态遥测 (LIVE TELEMETRY)")
                        .color(Color32::from_rgb(148, 163, 184))
                        .strong()
                        .size(13.0),
                );
                ui.add_space(8.0);

                ui.columns(3, |columns| {
                    // CPU Tile
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 24, 36))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 41, 60)))
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(&mut columns[0], |ui| {
                            ui.label(RichText::new("中央处理器 (CPU)").color(Color32::from_rgb(100, 116, 139)).size(11.0));
                            ui.add_space(2.0);
                            if let Some(ref info) = self.info {
                                let pl1 = info.cpu_pl1_w.unwrap_or(0.0);
                                let pl2 = info.cpu_pl2_w.unwrap_or(0.0);
                                ui.label(RichText::new(format!("PL1: {:.0}W | PL2: {:.0}W", pl1, pl2)).color(Color32::WHITE).strong().size(13.0));
                                
                                let (temp_str, temp_color) = match info.cpu_temp_c {
                                    Some(t) if t >= 80 => (format!("{}°C (高温)", t), Color32::from_rgb(248, 113, 113)),
                                    Some(t) if t >= 65 => (format!("{}°C (温热)", t), Color32::from_rgb(251, 191, 36)),
                                    Some(t) => (format!("{}°C (正常)", t), Color32::from_rgb(56, 189, 248)),
                                    None => ("--°C".to_string(), Color32::from_rgb(148, 163, 184)),
                                };
                                let epp = info.cpu_epp.as_deref().unwrap_or("balance");
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("温度: {}", temp_str)).color(temp_color).size(11.0));
                                    ui.label(RichText::new(format!("EPP: {}", epp)).color(Color32::from_rgb(148, 163, 184)).size(11.0));
                                });
                            } else {
                                ui.label(RichText::new("读取中...").color(Color32::from_rgb(148, 163, 184)));
                            }
                        });

                    // GPU Tile
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 24, 36))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 41, 60)))
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(&mut columns[1], |ui| {
                            ui.label(RichText::new("独立显卡 (RTX 4060)").color(Color32::from_rgb(100, 116, 139)).size(11.0));
                            ui.add_space(2.0);
                            if let Some(ref info) = self.info {
                                if info.gpu_d3cold {
                                    ui.label(RichText::new("● PCIe RTD3 深度休眠").color(Color32::from_rgb(52, 211, 153)).strong().size(13.0));
                                    ui.label(RichText::new("0W 功耗 · 零发热零耗电待机").color(Color32::from_rgb(148, 163, 184)).size(11.0));
                                } else {
                                    let p_str = match info.gpu_power_w {
                                        Some(p) if p > 0.0 => format!("活跃 · 功耗上限 {:.0}W", p),
                                        _ => "活跃 · 基础功耗释放".to_string(),
                                    };
                                    ui.label(RichText::new(p_str).color(Color32::WHITE).strong().size(13.0));

                                    let (temp_str, temp_color) = match info.gpu_temp_c {
                                        Some(t) if t >= 80 => (format!("{}°C (高温)", t), Color32::from_rgb(248, 113, 113)),
                                        Some(t) if t >= 65 => (format!("{}°C (温热)", t), Color32::from_rgb(251, 191, 36)),
                                        Some(t) => (format!("{}°C (清凉)", t), Color32::from_rgb(56, 189, 248)),
                                        None => ("--°C".to_string(), Color32::from_rgb(148, 163, 184)),
                                    };
                                    ui.label(RichText::new(format!("核心温度: {}", temp_str)).color(temp_color).size(11.0));
                                }
                            } else {
                                ui.label(RichText::new("读取中...").color(Color32::from_rgb(148, 163, 184)));
                            }
                        });

                    // Power / Fans Tile
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 24, 36))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 41, 60)))
                        .rounding(Rounding::same(6.0_f32))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(&mut columns[2], |ui| {
                            ui.label(RichText::new("供电输入与双风扇").color(Color32::from_rgb(100, 116, 139)).size(11.0));
                            ui.add_space(2.0);
                            if let Some(ref info) = self.info {
                                if info.ac_online {
                                    ui.label(RichText::new("● 原装适配器已连接").color(Color32::from_rgb(52, 211, 153)).strong().size(12.5));
                                } else {
                                    ui.label(RichText::new("● 正在使用电池供电").color(Color32::from_rgb(251, 191, 36)).strong().size(12.5));
                                }

                                let fan_str = match (info.fan1_rpm, info.fan2_rpm) {
                                    (Some(f1), Some(f2)) => format!("F1: {} | F2: {} RPM", f1, f2),
                                    (Some(f1), None) => format!("F1: {} RPM", f1),
                                    _ => if info.fan_boost == Some(true) {
                                        "双风扇 100% 满速强冷".to_string()
                                    } else {
                                        "EC 固件智能调速".to_string()
                                    },
                                };
                                ui.label(RichText::new(fan_str).color(Color32::from_rgb(56, 189, 248)).size(11.0));
                            } else {
                                ui.label(RichText::new("读取中...").color(Color32::from_rgb(148, 163, 184)));
                            }
                        });
                });
            });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(840.0, 660.0))
            .with_min_inner_size(Vec2::new(780.0, 580.0))
            .with_title("机械革命控制中心 - MECHREVO Control Center"),
        ..Default::default()
    };

    eframe::run_native(
        "mechrevo-gui",
        native_options,
        Box::new(|cc| Box::new(MechrevoGuiApp::new(cc))),
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
        assert_eq!(info.gpu_d3cold, false);
        assert_eq!(info.battery_percent, Some(100));
        assert_eq!(info.battery_status.as_deref(), Some("Full"));
        assert_eq!(info.battery_threshold, Some(80));
        assert_eq!(info.keyboard_brightness, Some(50));
        assert_eq!(info.fan_boost, Some(false));
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
}
