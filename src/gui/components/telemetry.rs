//! Live telemetry dashboard component with 0-CLS fixed height and monospace alignment.

use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use crate::daemon::SystemInfoReport;

use crate::gui::icons;
use crate::gui::theme;

/// Classifies CPU temperature into localized status and alert color.
pub fn classify_cpu_temp(temp: Option<i32>) -> (String, Color32) {
    match temp {
        Some(t) if t >= 80 => (format!("{}°C (高温)", t), theme::ACCENT_RED),
        Some(t) if t >= 65 => (format!("{}°C (温热)", t), theme::ACCENT_ORANGE),
        Some(t) => (format!("{}°C (正常)", t), theme::ACCENT_CYAN),
        None => ("--°C".to_string(), theme::TEXT_MUTED),
    }
}

/// Classifies GPU temperature into localized status and alert color.
pub fn classify_gpu_temp(temp: Option<i32>) -> (String, Color32) {
    match temp {
        Some(t) if t >= 80 => (format!("{}°C (高温)", t), theme::ACCENT_RED),
        Some(t) if t >= 65 => (format!("{}°C (温热)", t), theme::ACCENT_ORANGE),
        Some(t) => (format!("{}°C (清凉)", t), theme::ACCENT_CYAN),
        None => ("--°C".to_string(), theme::TEXT_MUTED),
    }
}

/// Formats CPU PL1/PL2 power limit telemetry. Avoids misleading 0W when readings are unavailable.
pub fn format_cpu_pl(pl1: Option<f64>, pl2: Option<f64>) -> String {
    match (pl1, pl2) {
        (Some(p1), Some(p2)) => format!("PL1: {:.0}W | PL2: {:.0}W", p1, p2),
        (Some(p1), None) => format!("PL1: {:.0}W | PL2: --W", p1),
        (None, Some(p2)) => format!("PL1: --W | PL2: {:.0}W", p2),
        (None, None) => "PL1: --W | PL2: --W".to_string(),
    }
}

/// Formats dual cooling fan RPM and boost telemetry description.
pub fn format_fan_status(fan1: Option<u32>, fan2: Option<u32>, boost: Option<bool>) -> String {
    match (fan1, fan2) {
        (Some(f1), Some(f2)) => format!("F1: {} | F2: {} RPM", f1, f2),
        (Some(f1), None) => format!("F1: {} RPM", f1),
        (None, Some(f2)) => format!("F2: {} RPM", f2),
        _ => if boost == Some(true) {
            "双风扇 100% 满速强冷".to_string()
        } else {
            "EC 固件智能调速".to_string()
        },
    }
}

/// Formats GPU power limit telemetry.
pub fn format_gpu_power(gpu_power_w: Option<f64>) -> String {
    match gpu_power_w {
        Some(p) if p > 0.0 => format!("活跃 · 功耗上限 {:.0}W", p),
        _ => "活跃 · 基础功耗释放".to_string(),
    }
}

pub fn render_telemetry_section(
    ui: &mut egui::Ui,
    info_opt: Option<&SystemInfoReport>,
) {
    egui::Frame::none()
        .fill(theme::BG_CARD)
        .stroke(Stroke::new(1.0_f32, theme::BORDER_SUBTLE))
        .rounding(theme::ROUNDING_CONTAINER)
        .inner_margin(theme::MARGIN_CARD)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::render_svg(ui, "activity", icons::SVG_ACTIVITY, Vec2::splat(15.0), theme::TEXT_SECONDARY);
                ui.label(
                    RichText::new("实时硬件传感器与状态遥测 (LIVE TELEMETRY)")
                        .color(theme::TEXT_SECONDARY)
                        .strong()
                        .size(13.0),
                );
            });
            ui.add_space(8.0);

            ui.columns(3, |columns| {
                // 1. CPU Tile
                egui::Frame::none()
                    .fill(theme::BG_CARD_SUBTLE)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER_CARD))
                    .rounding(theme::ROUNDING_CONTROL)
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .show(&mut columns[0], |ui| {
                        ui.set_min_height(72.0);
                        ui.horizontal(|ui| {
                            icons::render_svg(ui, "cpu", icons::SVG_CPU, Vec2::splat(13.0), theme::TEXT_MUTED);
                            ui.label(RichText::new("中央处理器 (CPU)").color(theme::TEXT_MUTED).size(11.0));
                        });
                        ui.add_space(3.0);
                        if let Some(info) = info_opt {
                            let pl_str = format_cpu_pl(info.cpu_pl1_w, info.cpu_pl2_w);
                            ui.label(RichText::new(pl_str).color(Color32::WHITE).strong().monospace().size(12.5));

                            let (temp_str, temp_color) = classify_cpu_temp(info.cpu_temp_c);
                            let epp = info.cpu_epp.as_deref().unwrap_or("balance");
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("温度: {}", temp_str)).color(temp_color).monospace().size(11.0));
                                ui.label(RichText::new(format!("EPP: {}", epp)).color(theme::TEXT_SECONDARY).size(11.0));
                            });
                        } else {
                            ui.label(RichText::new("读取中...").color(theme::TEXT_MUTED));
                        }
                    });

                // 2. GPU Tile
                egui::Frame::none()
                    .fill(theme::BG_CARD_SUBTLE)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER_CARD))
                    .rounding(theme::ROUNDING_CONTROL)
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .show(&mut columns[1], |ui| {
                        ui.set_min_height(72.0);
                        let gpu_title = info_opt
                            .and_then(|i| i.gpu_name.as_deref())
                            .unwrap_or("独立显卡 (NVIDIA dGPU)");
                        ui.horizontal(|ui| {
                            icons::render_svg(ui, "gpu", icons::SVG_GPU, Vec2::splat(13.0), theme::TEXT_MUTED);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(gpu_title)
                                        .color(theme::TEXT_MUTED)
                                        .size(11.0),
                                )
                                .truncate(true),
                            );
                        });
                        ui.add_space(3.0);
                        if let Some(info) = info_opt {
                            if info.gpu_d3cold {
                                ui.horizontal(|ui| {
                                    icons::render_status_dot(ui, theme::ACCENT_GREEN, 2.5);
                                    ui.label(RichText::new("PCIe RTD3 深度休眠").color(theme::ACCENT_GREEN).strong().size(12.5));
                                });
                                ui.label(RichText::new("0W 功耗 · 零发热零耗电待机").color(theme::TEXT_SECONDARY).size(11.0));
                            } else {
                                let p_str = format_gpu_power(info.gpu_power_w);
                                ui.label(RichText::new(p_str).color(Color32::WHITE).strong().monospace().size(12.5));

                                let (temp_str, temp_color) = classify_gpu_temp(info.gpu_temp_c);
                                ui.label(RichText::new(format!("核心温度: {}", temp_str)).color(temp_color).monospace().size(11.0));
                            }
                        } else {
                            ui.label(RichText::new("读取中...").color(theme::TEXT_MUTED));
                        }
                    });

                // 3. Power / Fans Tile
                egui::Frame::none()
                    .fill(theme::BG_CARD_SUBTLE)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER_CARD))
                    .rounding(theme::ROUNDING_CONTROL)
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .show(&mut columns[2], |ui| {
                        ui.set_min_height(72.0);
                        ui.horizontal(|ui| {
                            icons::render_svg(ui, "plug", icons::SVG_AC_PLUG, Vec2::splat(13.0), theme::TEXT_MUTED);
                            ui.label(RichText::new("供电输入与双风扇").color(theme::TEXT_MUTED).size(11.0));
                        });
                        ui.add_space(3.0);
                        if let Some(info) = info_opt {
                            if info.ac_online {
                                ui.horizontal(|ui| {
                                    icons::render_status_dot(ui, theme::ACCENT_GREEN, 2.5);
                                    ui.label(RichText::new("原装适配器已连接").color(theme::ACCENT_GREEN).strong().size(12.5));
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    icons::render_status_dot(ui, theme::ACCENT_ORANGE, 2.5);
                                    ui.label(RichText::new("正在使用电池供电").color(theme::ACCENT_ORANGE).strong().size(12.5));
                                });
                            }

                            let fan_str = format_fan_status(info.fan1_rpm, info.fan2_rpm, info.fan_boost);
                            ui.label(RichText::new(fan_str).color(theme::ACCENT_CYAN).monospace().size(11.0));
                        } else {
                            ui.label(RichText::new("读取中...").color(theme::TEXT_MUTED));
                        }
                    });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_pure_functions() {
        // CPU PL formatting
        assert_eq!(format_cpu_pl(Some(75.0), Some(90.0)), "PL1: 75W | PL2: 90W");
        assert_eq!(format_cpu_pl(Some(105.0), None), "PL1: 105W | PL2: --W");
        assert_eq!(format_cpu_pl(None, None), "PL1: --W | PL2: --W");

        // CPU & GPU Temperature Classification
        let (cpu_str, cpu_color) = classify_cpu_temp(Some(85));
        assert_eq!(cpu_str, "85°C (高温)");
        assert_eq!(cpu_color, theme::ACCENT_RED);

        let (cpu_str_warn, cpu_color_warn) = classify_cpu_temp(Some(70));
        assert_eq!(cpu_str_warn, "70°C (温热)");
        assert_eq!(cpu_color_warn, theme::ACCENT_ORANGE);

        let (cpu_str_norm, cpu_color_norm) = classify_cpu_temp(Some(55));
        assert_eq!(cpu_str_norm, "55°C (正常)");
        assert_eq!(cpu_color_norm, theme::ACCENT_CYAN);

        let (gpu_str_cool, gpu_color_cool) = classify_gpu_temp(Some(45));
        assert_eq!(gpu_str_cool, "45°C (清凉)");
        assert_eq!(gpu_color_cool, theme::ACCENT_CYAN);

        let (none_str, none_color) = classify_cpu_temp(None);
        assert_eq!(none_str, "--°C");
        assert_eq!(none_color, theme::TEXT_MUTED);

        // Fan RPM and Boost formatting
        assert_eq!(format_fan_status(Some(2400), Some(2600), Some(false)), "F1: 2400 | F2: 2600 RPM");
        assert_eq!(format_fan_status(Some(3000), None, Some(false)), "F1: 3000 RPM");
        assert_eq!(format_fan_status(None, None, Some(true)), "双风扇 100% 满速强冷");
        assert_eq!(format_fan_status(None, None, Some(false)), "EC 固件智能调速");

        // GPU Power formatting
        assert_eq!(format_gpu_power(Some(140.0)), "活跃 · 功耗上限 140W");
        assert_eq!(format_gpu_power(Some(0.0)), "活跃 · 基础功耗释放");
        assert_eq!(format_gpu_power(None), "活跃 · 基础功耗释放");
    }
}
