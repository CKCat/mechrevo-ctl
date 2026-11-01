//! Performance release profile cards component.

use std::sync::mpsc::Sender;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

use crate::gui::icons;
use crate::gui::theme;
use crate::gui::worker::IpcAction;

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
        .rounding(theme::ROUNDING_CARD)
        .inner_margin(theme::MARGIN_CARD);

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
                    icons::render_badge(
                        ui,
                        "生效中",
                        accent_color,
                        Color32::WHITE,
                        Some(("check_badge", icons::SVG_CHECK)),
                    );
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
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(5.0, 0.0);
                icons::render_status_dot(ui, theme::TEXT_MUTED, 1.8);
                ui.label(
                    RichText::new(*spec)
                        .color(Color32::from_rgb(160, 175, 195))
                        .size(11.0),
                );
            });
        }
        ui.add_space(6.0);

        let btn_label = if is_active { "当前模式已生效" } else { "点击切换此模式" };
        let btn = egui::Button::new(
            RichText::new(btn_label)
                .color(if is_active { Color32::WHITE } else { Color32::from_rgb(203, 213, 225) })
                .size(11.5),
        )
        .fill(if is_active { accent_color } else { theme::BTN_INACTIVE_BG })
        .rounding(theme::ROUNDING_CONTROL);

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

pub fn render_profile_section(
    ui: &mut egui::Ui,
    current_profile: &str,
    action_tx: &Sender<IpcAction>,
) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .rounding(theme::ROUNDING_CONTAINER)
        .inner_margin(theme::MARGIN_CARD)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::render_svg(ui, "speedometer", icons::SVG_SPEEDOMETER, Vec2::splat(15.0), theme::TEXT_SECONDARY);
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
                if render_profile_card(
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
                    let _ = action_tx.send(IpcAction::SetProfile("turbo".to_string()));
                }

                // 2. Balanced
                if render_profile_card(
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
                    let _ = action_tx.send(IpcAction::SetProfile("balanced".to_string()));
                }

                // 3. Office
                if render_profile_card(
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
                    let _ = action_tx.send(IpcAction::SetProfile("office".to_string()));
                }
            });
        });
}
