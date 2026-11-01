//! Modern native GUI for MECHREVO Control Center.
//! Adhering to the `rust-egui-design` desktop design system.

pub mod app;
pub mod autostart;
pub mod components;
pub mod fonts;
pub mod icons;
pub mod theme;
pub mod tray;
pub mod worker;

pub use app::MechrevoGuiApp;

use eframe::egui::Vec2;

/// Runs the native MECHREVO Control Center GUI desktop application.
pub fn run() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let start_minimized = args.iter().any(|a| a == "--minimized" || a == "--tray");

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
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

