//! High-fidelity SVG vector icons and rendering utilities, eliminating Unicode/Emoji dependencies.

use eframe::egui::{self, Color32, Image, Margin, Response, Rounding, Sense, Vec2};

pub const SVG_CHECK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>"#;

pub const SVG_ALERT_TRIANGLE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>"#;

pub const SVG_CLOSE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>"#;

pub const SVG_CPU: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/><line x1="9" y1="1" x2="9" y2="4"/><line x1="15" y1="1" x2="15" y2="4"/><line x1="9" y1="20" x2="9" y2="23"/><line x1="15" y1="20" x2="15" y2="23"/><line x1="20" y1="9" x2="23" y2="9"/><line x1="20" y1="14" x2="23" y2="14"/><line x1="1" y1="9" x2="4" y2="9"/><line x1="1" y1="14" x2="4" y2="14"/></svg>"#;

pub const SVG_GPU: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="6" width="20" height="12" rx="2"/><circle cx="8" cy="12" r="3"/><circle cx="16" cy="12" r="3"/><line x1="6" y1="2" x2="6" y2="6"/><line x1="10" y1="2" x2="10" y2="6"/></svg>"#;

pub const SVG_FAN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="2.5"/><path d="M12 9.5V3a2.5 2.5 0 0 1 2.5 2.5v2M9.5 12H3a2.5 2.5 0 0 1 2.5-2.5h2M12 14.5V21a2.5 2.5 0 0 1-2.5-2.5v-2M14.5 12H21a2.5 2.5 0 0 1-2.5 2.5h-2"/></svg>"#;

pub const SVG_BATTERY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="1" y="6" width="18" height="12" rx="2"/><line x1="23" y1="11" x2="23" y2="13"/></svg>"#;

pub const SVG_BATTERY_CHARGING: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 18H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h3.19M15 6h2a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-3.19"/><line x1="23" y1="11" x2="23" y2="13"/><polyline points="11 6 7 12 13 12 9 18"/></svg>"#;

pub const SVG_KEYBOARD: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="16" rx="2"/><line x1="6" y1="8" x2="6.01" y2="8"/><line x1="10" y1="8" x2="10.01" y2="8"/><line x1="14" y1="8" x2="14.01" y2="8"/><line x1="18" y1="8" x2="18.01" y2="8"/><line x1="6" y1="12" x2="6.01" y2="12"/><line x1="18" y1="12" x2="18.01" y2="12"/><line x1="7" y1="16" x2="17" y2="16"/></svg>"#;

pub const SVG_SPEEDOMETER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a10 10 0 0 0-7.07 17.07A10 10 0 0 0 19.07 4.93 9.94 9.94 0 0 0 12 2z"/><path d="M12 6v6l4 2"/></svg>"#;

pub const SVG_REFRESH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>"#;

pub const SVG_AC_PLUG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 7v6a6 6 0 0 0 12 0V7"/><line x1="9" y1="2" x2="9" y2="7"/><line x1="15" y1="2" x2="15" y2="7"/><line x1="12" y1="19" x2="12" y2="22"/></svg>"#;

pub const SVG_ACTIVITY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>"#;

/// Renders an SVG icon with customizable size and color tint.
pub fn render_svg(
    ui: &mut egui::Ui,
    name: &'static str,
    svg_str: &'static str,
    size: Vec2,
    tint: Color32,
) -> Response {
    let uri = format!("bytes://mechrevo_icon_{}.svg", name);
    let image = Image::from_bytes(uri, svg_str.as_bytes())
        .fit_to_exact_size(size)
        .tint(tint);
    ui.add(image)
}

/// Renders a crisp antialiased status dot with zero layout jitter.
pub fn render_status_dot(ui: &mut egui::Ui, color: Color32, radius: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(radius * 2.0 + 2.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(rect.center(), radius, color);
    }
    response
}

/// Renders a modern rounded badge pill (Rounding::same(12.0)) with optional vector icon.
pub fn render_badge(
    ui: &mut egui::Ui,
    text: &str,
    bg: Color32,
    fg: Color32,
    icon: Option<(&'static str, &'static str)>,
) {
    egui::Frame::none()
        .fill(bg)
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::symmetric(8.0, 2.5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
                if let Some((name, svg)) = icon {
                    render_svg(ui, name, svg, Vec2::splat(11.0), fg);
                }
                ui.label(
                    egui::RichText::new(text)
                        .color(fg)
                        .strong()
                        .size(11.0),
                );
            });
        });
}

