//! Keyboard backlight regulation component.

use std::sync::mpsc::Sender;
use eframe::egui::{self, RichText, Stroke, Vec2};

use crate::gui::components::render_pill_button;
use crate::gui::icons;
use crate::gui::theme;
use crate::gui::worker::IpcAction;

pub fn render_keyboard_section(
    ui: &mut egui::Ui,
    current_br: u32,
    custom_br: &mut u32,
    action_tx: &Sender<IpcAction>,
) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .rounding(theme::ROUNDING_CONTAINER)
        .inner_margin(theme::MARGIN_CARD)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::render_svg(ui, "keyboard", icons::SVG_KEYBOARD, Vec2::splat(15.0), theme::TEXT_SECONDARY);
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
                    let slider = ui.add_sized(
                        [100.0, 20.0],
                        egui::Slider::new(custom_br, 0..=100).suffix("%"),
                    );
                    if slider.drag_stopped() || (slider.changed() && !slider.has_focus()) {
                        let _ = action_tx.send(IpcAction::SetKeyboard(*custom_br));
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
                                *custom_br = val;
                                let _ = action_tx.send(IpcAction::SetKeyboard(val));
                            }
                        }
                    });
                });
            });
        });
}
