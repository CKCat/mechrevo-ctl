//! Application state and eframe::App implementation with event-driven Zero-CPU Idle scheduling.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Margin, RichText, Stroke, Vec2};
use ksni::blocking::{Handle as TrayHandle, TrayMethods};
use crate::daemon::SystemInfoReport;

use super::autostart::is_autostart_enabled;
use super::components::battery::render_battery_section;
use super::components::cooling::render_cooling_section;
use super::components::keyboard::render_keyboard_section;
use super::components::profile_cards::render_profile_section;
use super::components::status_bar::render_status_bar;
use super::components::telemetry::render_telemetry_section;
use super::fonts::setup_custom_fonts;
use super::icons;
use super::theme;
use super::tray::{MechrevoTray, TrayCommand};
use super::worker::{spawn_worker, IpcAction, IpcResult, WorkerChannels};

pub struct MechrevoGuiApp {
    info: Option<SystemInfoReport>,
    action_tx: Sender<IpcAction>,
    result_rx: Receiver<IpcResult>,
    is_window_visible: Arc<AtomicBool>,
    auto_refresh_active: Arc<AtomicBool>,
    status_message: Option<(String, bool, Instant)>,
    auto_refresh: bool,
    custom_battery_th: u32,
    custom_kbd_br: u32,
    is_loading: bool,
    tray_handle: Arc<Mutex<Option<TrayHandle<MechrevoTray>>>>,
    tray_rx: Receiver<TrayCommand>,
    pub window_visible: bool,
    minimize_to_tray: bool,
    autostart: bool,
}

impl MechrevoGuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Install egui_extras image and SVG loaders
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // Setup CJK fallback font
        setup_custom_fonts(&cc.egui_ctx);

        // Configure visual design tokens from rust-egui-design
        theme::configure_visuals(&cc.egui_ctx);

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

        let tray_handle_opt = match tray.spawn() {
            Ok(handle) => Some(handle),
            Err(e) => {
                tracing::warn!("启动系统托盘服务失败 (非致命警告): {}", e);
                None
            }
        };

        let shared_tray = Arc::new(Mutex::new(tray_handle_opt));

        // Spawn non-blocking background IPC worker with context and shared tray
        let WorkerChannels {
            action_tx,
            result_rx,
            is_window_visible,
            auto_refresh_active,
        } = spawn_worker(cc.egui_ctx.clone(), shared_tray.clone());

        Self {
            info: None,
            action_tx,
            result_rx,
            is_window_visible,
            auto_refresh_active,
            status_message: None,
            auto_refresh: true,
            custom_battery_th: 80,
            custom_kbd_br: 50,
            is_loading: false,
            tray_handle: shared_tray,
            tray_rx,
            window_visible: true,
            minimize_to_tray: true,
            autostart: is_autostart_enabled(),
        }
    }

    fn set_window_visibility(&mut self, ctx: &egui::Context, visible: bool) {
        self.window_visible = visible;
        self.is_window_visible.store(visible, Ordering::Relaxed);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(visible));
        if visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn shutdown_tray(&self) {
        if let Ok(mut guard) = self.tray_handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.shutdown();
            }
        }
    }

    fn has_tray(&self) -> bool {
        self.tray_handle.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

impl eframe::App for MechrevoGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process incoming commands from system tray
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCommand::ToggleWindow => {
                    let next = !self.window_visible;
                    self.set_window_visibility(ctx, next);
                }
                TrayCommand::ShowWindow => {
                    self.set_window_visibility(ctx, true);
                }
                TrayCommand::HideWindow => {
                    self.set_window_visibility(ctx, false);
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
                    self.shutdown_tray();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Intercept window close request to minimize to tray if enabled
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.minimize_to_tray && self.has_tray() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.set_window_visibility(ctx, false);
            } else {
                self.shutdown_tray();
            }
        }

        // Global Ctrl+Q shortcut to cleanly exit application
        if ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(egui::Key::Q)) {
            self.shutdown_tray();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Poll incoming responses from background worker
        while let Ok(res) = self.result_rx.try_recv() {
            self.is_loading = false;
            match res {
                IpcResult::Info(info) => {
                    self.custom_battery_th = info.battery_threshold.unwrap_or(self.custom_battery_th);
                    self.custom_kbd_br = info.keyboard_brightness.unwrap_or(self.custom_kbd_br);
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

        // Keyboard shortcuts
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

        // Zero-CPU Idle Scheduling:
        // Only schedule repaint if a transient status message needs to be auto-dismissed
        if let Some((_, _, time)) = self.status_message {
            let elapsed = time.elapsed();
            if elapsed >= Duration::from_secs(5) {
                self.status_message = None;
            } else {
                ctx.request_repaint_after(Duration::from_secs(5) - elapsed);
            }
        }

        // 1. Bottom Status Bar (0-CLS Stabilized)
        let chassis = self.info.as_ref().map(|i| i.chassis.as_str()).unwrap_or("MECHREVO Laptop");
        render_status_bar(
            ctx,
            &mut self.status_message,
            chassis,
            &mut self.minimize_to_tray,
            &mut self.autostart,
            &self.action_tx,
        );

        // 2. Top Navigation Bar
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_PANEL)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
                    .inner_margin(theme::MARGIN_PANEL),
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
                        // Refresh button with SVG icon
                        let refresh_btn = ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
                            icons::render_svg(ui, "refresh", icons::SVG_REFRESH, Vec2::splat(12.0), Color32::WHITE);
                            ui.button(if self.is_loading { "刷新中..." } else { "刷新" })
                        }).inner;

                        if refresh_btn.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if refresh_btn.clicked() {
                            self.is_loading = true;
                            let _ = self.action_tx.send(IpcAction::Refresh);
                        }

                        let auto_resp = ui.checkbox(&mut self.auto_refresh, "自动遥测 (2s)");
                        if auto_resp.changed() {
                            self.auto_refresh_active
                                .store(self.auto_refresh, Ordering::Relaxed);
                        }

                        if let Some(ref info) = self.info {
                            if info.ac_online {
                                icons::render_badge(
                                    ui,
                                    "AC 适配器供电",
                                    Color32::from_rgb(20, 83, 45),
                                    Color32::from_rgb(187, 247, 208),
                                    Some(("ac_online", icons::SVG_AC_PLUG)),
                                );
                            } else {
                                icons::render_badge(
                                    ui,
                                    "正在使用电池",
                                    Color32::from_rgb(124, 45, 18),
                                    Color32::from_rgb(254, 215, 170),
                                    Some(("bat_online", icons::SVG_BATTERY)),
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
                    .inner_margin(Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let active_profile = self.info.as_ref().map(|i| i.active_profile.as_str()).unwrap_or("balanced");
                        let is_boost = self.info.as_ref().and_then(|i| i.fan_boost).unwrap_or(false);
                        let (bat_pct, bat_status, bat_th) = self.info.as_ref()
                            .map(|i| (
                                i.battery_percent.unwrap_or(100),
                                i.battery_status.as_deref().unwrap_or("Full"),
                                i.battery_threshold.unwrap_or(80),
                            ))
                            .unwrap_or((100, "Full", 80));
                        let kbd_br = self.info.as_ref().and_then(|i| i.keyboard_brightness).unwrap_or(50);

                        // Module 1: Performance Release Profiles
                        render_profile_section(ui, active_profile, &self.action_tx);
                        ui.add_space(theme::SPACING_SECTION);

                        // Module 2: Cooling & Fan Boost
                        render_cooling_section(ui, is_boost, &self.action_tx);
                        ui.add_space(theme::SPACING_SECTION);

                        // Module 3: Battery Conservation
                        render_battery_section(
                            ui,
                            bat_pct,
                            bat_status,
                            bat_th,
                            &mut self.custom_battery_th,
                            &self.action_tx,
                        );
                        ui.add_space(theme::SPACING_SECTION);

                        // Module 4: Keyboard Backlight
                        render_keyboard_section(
                            ui,
                            kbd_br,
                            &mut self.custom_kbd_br,
                            &self.action_tx,
                        );
                        ui.add_space(theme::SPACING_SECTION);

                        // Module 5: Live Telemetry Dashboard
                        render_telemetry_section(ui, self.info.as_ref());
                        ui.add_space(12.0);
                    });
            });
    }
}
