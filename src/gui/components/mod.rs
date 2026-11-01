//! Reusable UI component modules.

pub mod battery;
pub mod cooling;
pub mod keyboard;
pub mod profile_cards;
pub mod status_bar;
pub mod telemetry;

use eframe::egui::{self, Color32, Margin, Rounding, Stroke, Vec2};
use super::icons;
use super::theme;

/// Renders a clickable preset pill button with an optional SVG active indicator and hover feedback.
pub fn render_pill_button(
    ui: &mut egui::Ui,
    label: &str,
    is_active: bool,
    accent_color: Color32,
) -> bool {
    let mut clicked = false;
    let bg = if is_active {
        Color32::from_rgb(26, 44, 66)
    } else {
        theme::BTN_INACTIVE_BG
    };
    let stroke = if is_active {
        Stroke::new(1.5_f32, accent_color)
    } else {
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE)
    };

    let frame = egui::Frame::none()
        .fill(bg)
        .stroke(stroke)
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(9.0, 4.0));

    let res = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
            if is_active {
                icons::render_svg(ui, "pill_check", icons::SVG_CHECK, Vec2::splat(11.0), accent_color);
            }
            ui.label(
                egui::RichText::new(label)
                    .color(if is_active { Color32::WHITE } else { theme::TEXT_SECONDARY })
                    .strong()
                    .size(11.0),
            );
        });
    });

    let interact = ui.interact(res.response.rect, res.response.id.with(label), egui::Sense::click());
    if interact.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if interact.clicked() {
        clicked = true;
    }

    clicked
}
