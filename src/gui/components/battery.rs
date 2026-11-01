//! Battery health and charge conservation threshold component.

use std::sync::mpsc::Sender;
use eframe::egui::{self, RichText, Stroke, Vec2};

use crate::gui::components::render_pill_button;
use crate::gui::icons;
use crate::gui::theme;
use crate::gui::worker::IpcAction;

pub fn render_battery_section(
    ui: &mut egui::Ui,
    percent: u32,
    status: &str,
    current_th: u32,
    custom_th: &mut u32,
    action_tx: &Sender<IpcAction>,
) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .rounding(theme::ROUNDING_CONTAINER)
        .inner_margin(theme::MARGIN_CARD)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::render_svg(ui, "battery", icons::SVG_BATTERY, Vec2::splat(15.0), theme::TEXT_SECONDARY);
                ui.label(
                    RichText::new("电池健康与充电养护 (BATTERY CONSERVATION)")
                        .color(theme::TEXT_SECONDARY)
                        .strong()
                        .size(13.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "当前电量: {}% ({}) | 当前养护上限: {}%",
                            percent, status, current_th
                        ))
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
                        let _ = action_tx.send(IpcAction::SetBattery(*custom_th));
                    }
                    ui.add_sized(
                        [90.0, 20.0],
                        egui::Slider::new(custom_th, 50..=100).suffix("%"),
                    );

                    // Left side: Presets
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("预设:").size(12.0).color(theme::TEXT_SECONDARY));

                        if render_pill_button(
                            ui,
                            "80% 常年插电",
                            current_th == 80,
                            theme::ACCENT_CYAN,
                        ) {
                            *custom_th = 80;
                            let _ = action_tx.send(IpcAction::SetBattery(80));
                        }

                        if render_pill_button(
                            ui,
                            "100% 满电备战",
                            current_th == 100,
                            theme::ACCENT_GREEN,
                        ) {
                            *custom_th = 100;
                            let _ = action_tx.send(IpcAction::SetBattery(100));
                        }

                        if render_pill_button(
                            ui,
                            "60% 深度养护",
                            current_th == 60,
                            theme::ACCENT_ORANGE,
                        ) {
                            *custom_th = 60;
                            let _ = action_tx.send(IpcAction::SetBattery(60));
                        }
                    });
                });
            });
        });
}
