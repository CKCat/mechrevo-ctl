//! Cooling and Fan Boost control component.

use std::sync::mpsc::Sender;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

use crate::gui::icons;
use crate::gui::theme;
use crate::gui::worker::IpcAction;

pub fn render_cooling_section(
    ui: &mut egui::Ui,
    is_boost: bool,
    action_tx: &Sender<IpcAction>,
) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .rounding(theme::ROUNDING_CONTAINER)
        .inner_margin(theme::MARGIN_CARD)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::render_svg(ui, "fan", icons::SVG_FAN, Vec2::splat(15.0), theme::TEXT_SECONDARY);
                ui.label(
                    RichText::new("散热与极限强冷 (COOLING & FAN BOOST)")
                        .color(theme::TEXT_SECONDARY)
                        .strong()
                        .size(13.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_boost {
                        icons::render_badge(
                            ui,
                            "100% 强冷运行中",
                            theme::ACCENT_RED,
                            Color32::WHITE,
                            Some(("fan_boost", icons::SVG_FAN)),
                        );
                    } else {
                        icons::render_badge(
                            ui,
                            "EC 智能调速",
                            theme::BTN_INACTIVE_BG,
                            theme::TEXT_SECONDARY,
                            None,
                        );
                    }
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
                    .rounding(theme::ROUNDING_CONTROL);

                    let resp = ui.add_sized([160.0, 30.0], btn);
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        let _ = action_tx.send(IpcAction::SetFanBoost(target));
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
