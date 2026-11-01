//! Native GUI binary for MECHREVO Control Center.
//! Ultra-lightweight, high-performance, native Rust GUI using eframe/egui.

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    mechrevo::gui::run()
}
