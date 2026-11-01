//! Theme definitions, surface elevation tokens, and styling standards
//! adhering to the `rust-egui-design` desktop design system.

use eframe::egui::{self, Color32, Margin, Rounding, Stroke, Vec2};

// Surface Hierarchy Palette (Modern Dark Theme - VS Code / Linear style)
pub const BG_BASE: Color32 = Color32::from_rgb(15, 23, 42); // #0f172a Main container background
pub const BG_PANEL: Color32 = Color32::from_rgb(24, 24, 30); // #18181e Top title bar & bottom status bar
pub const BG_CARD: Color32 = Color32::from_rgb(30, 41, 59); // #1e293b Elevated feature cards
pub const BG_CARD_SUBTLE: Color32 = Color32::from_rgb(20, 28, 42); // #141c2a Nested telemetry tiles
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(51, 65, 85); // #334155 Subtle border stroke (1.0px)
pub const BORDER_CARD: Color32 = Color32::from_rgb(42, 55, 82);

// Interactive Controls
pub const BTN_INACTIVE_BG: Color32 = Color32::from_rgb(40, 44, 56); // #282c38 Inactive buttons & chips
pub const BTN_HOVERED_BG: Color32 = Color32::from_rgb(55, 65, 81); // #374151 Hover feedback
pub const BTN_ACTIVE_BG: Color32 = Color32::from_rgb(71, 85, 105); // #475569 Pressed state

// Accent Colors
pub const ACCENT_CYAN: Color32 = Color32::from_rgb(6, 182, 212); // #06b6d4 Primary brand highlights
pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(249, 115, 22); // #f97316 Fan boost & heavy alerts
pub const ACCENT_GREEN: Color32 = Color32::from_rgb(34, 197, 94); // #22c55e AC online & success badges
pub const ACCENT_RED: Color32 = Color32::from_rgb(239, 68, 68); // #ef4444 Critical warnings & failures

// Typography
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(241, 245, 249);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(148, 163, 184);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(100, 116, 139);

// Standard Geometric Tokens
pub const ROUNDING_CONTAINER: Rounding = Rounding::same(10.0_f32);
pub const ROUNDING_CARD: Rounding = Rounding::same(8.0_f32);
pub const ROUNDING_CONTROL: Rounding = Rounding::same(6.0_f32);
pub const ROUNDING_PILL: Rounding = Rounding::same(12.0_f32);

pub const MARGIN_CARD: Margin = Margin {
    left: 14.0_f32,
    right: 14.0_f32,
    top: 10.0_f32,
    bottom: 10.0_f32,
};

pub const MARGIN_PANEL: Margin = Margin {
    left: 16.0_f32,
    right: 16.0_f32,
    top: 10.0_f32,
    bottom: 10.0_f32,
};

pub const PADDING_BUTTON: Vec2 = Vec2::new(10.0_f32, 5.0_f32);
pub const SPACING_ITEM: Vec2 = Vec2::new(8.0_f32, 8.0_f32);
pub const SPACING_SECTION: f32 = 10.0_f32;

/// Configures global visuals, geometry and spacing on an `egui::Context`.
pub fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.panel_fill = BG_PANEL;
    visuals.window_fill = BG_BASE;

    // Non-interactive containers
    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.rounding = ROUNDING_CONTROL;

    // Interactive buttons / chips
    visuals.widgets.inactive.bg_fill = BTN_INACTIVE_BG;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.rounding = ROUNDING_CONTROL;

    visuals.widgets.hovered.bg_fill = BTN_HOVERED_BG;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_CYAN);
    visuals.widgets.hovered.rounding = ROUNDING_CONTROL;

    visuals.widgets.active.bg_fill = BTN_ACTIVE_BG;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.active.rounding = ROUNDING_CONTROL;

    visuals.selection.bg_fill = ACCENT_CYAN;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

    ctx.set_visuals(visuals);

    // Ergonomic spacing tokens
    let mut style = (*ctx.style()).clone();
    style.spacing.button_padding = PADDING_BUTTON;
    style.spacing.item_spacing = SPACING_ITEM;
    ctx.set_style(style);
}
