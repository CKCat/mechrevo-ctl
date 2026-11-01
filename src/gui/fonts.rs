//! Multi-tiered font definition and CJK fallback typography configuration.

use eframe::egui;

const STATIC_FONT_CANDIDATES: &[&str] = &[
    // 1. WenQuanYi Micro Hei / Zen Hei (Top priority for Linux desktop CJK glyph rendering)
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    // 2. Noto Sans CJK SC
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
    "/usr/share/fonts/truetype/arphic/ukai.ttc",
    // 3. Microsoft YaHei / SimHei (Dual-boot Windows & Wine paths)
    "/mnt/c/Windows/Fonts/msyh.ttc",
    "/mnt/c/Windows/Fonts/msyhbd.ttc",
    "/mnt/c/Windows/Fonts/simhei.ttf",
];

/// Configures fallback font definitions prioritizing CJK glyphs to prevent placeholder '?' boxes.
pub fn setup_custom_fonts(ctx: &egui::Context) {
    // Check static system paths first
    for path in STATIC_FONT_CANDIDATES {
        if std::path::Path::new(path).exists() {
            if let Ok(bytes) = std::fs::read(path) {
                apply_cjk_font(ctx, bytes, path);
                return;
            }
        }
    }

    // Check user local home font candidates
    if let Ok(home) = std::env::var("HOME") {
        let user_candidates = [
            format!("{}/.local/share/fonts/NotoSansCJK-Regular.ttc", home),
            format!("{}/.local/share/fonts/DroidSansFallbackFull.ttf", home),
            format!("{}/.wine/drive_c/windows/Fonts/msyh.ttc", home),
            format!("{}/.wine/drive_c/windows/Fonts/simhei.ttf", home),
        ];

        for path in &user_candidates {
            if std::path::Path::new(path).exists() {
                if let Ok(bytes) = std::fs::read(path) {
                    apply_cjk_font(ctx, bytes, path);
                    return;
                }
            }
        }
    }
}

fn apply_cjk_font(ctx: &egui::Context, bytes: Vec<u8>, path: &str) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk_fallback".to_owned(),
        egui::FontData::from_owned(bytes),
    );

    if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        vec.insert(0, "cjk_fallback".to_owned());
    }
    if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        vec.push("cjk_fallback".to_owned());
    }

    ctx.set_fonts(fonts);
    tracing::info!("Loaded CJK fallback font from: {}", path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_loading() {
        for path in &[
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if std::path::Path::new(path).exists() {
                let bytes = std::fs::read(path).unwrap();
                let mut fonts = egui::FontDefinitions::default();
                fonts.font_data.insert(
                    "cjk_fallback".to_owned(),
                    egui::FontData::from_owned(bytes),
                );
                if let Some(vec) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                    vec.push("cjk_fallback".to_owned());
                }
                let ctx = egui::Context::default();
                ctx.set_fonts(fonts);

                let _ = ctx.run(Default::default(), |ctx| {
                    let galley = ctx.fonts(|f| {
                        f.layout_no_wrap(
                            "机械革命 控制中心 狂暴模式 75W".to_owned(),
                            egui::FontId::proportional(14.0),
                            egui::Color32::WHITE,
                        )
                    });
                    assert!(!galley.rows.is_empty());
                    assert!(galley.size().x > 10.0);
                });
            }
        }
    }
}
