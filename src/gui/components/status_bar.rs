//! 0-CLS fixed height bottom status & shortcut bar component.

use std::sync::mpsc::Sender;
use std::time::Instant;

use eframe::egui::{self, Color32, Margin, RichText, Stroke, Vec2};

use crate::gui::icons;
use crate::gui::theme;
use crate::gui::worker::IpcAction;

/// Formats raw IPC result messages into human-friendly Chinese titles and guidance.
pub fn format_status_message(raw: &str, is_err: bool) -> (String, Option<String>) {
    if is_err {
        if raw.contains("charge_control") || raw.contains("Battery") || raw.contains("battery") {
            (
                "电池充电养护阈值配置失败: 硬件节点不可用".to_string(),
                Some("当前 Linux 内核未挂载 Uniwill/Tuxedo 电池扩展驱动，可运行 sudo ./scripts/setup-tuxedo-drivers.sh 激活".to_string()),
            )
        } else if raw.contains("fan_boost") || raw.contains("Fan boost") || raw.contains("fan-boost") {
            (
                "散热强冷配置失败: 硬件节点不可用".to_string(),
                Some("请在终端运行 sudo ./scripts/setup-tuxedo-drivers.sh 激活 EC 强冷控制".to_string()),
            )
        } else if raw.contains("Permission denied") {
            (
                "操作权限不足: 请求被系统拒绝".to_string(),
                Some("请确保 mechrevo-ctld 守护进程正在以 root 身份运行".to_string()),
            )
        } else {
            (
                "操作执行失败".to_string(),
                Some(raw.to_string()),
            )
        }
    } else if raw.contains("Successfully applied profile 'turbo'") {
        (
            "已成功切换至【狂暴满血模式】(CPU 105W / GPU 140W)".to_string(),
            Some("散热由 EC 固件智能接管，如需极速降温可随时按键 [B] 开启 100% 强冷".to_string()),
        )
    } else if raw.contains("Successfully applied profile 'balanced'") {
        (
            "已成功切换至【日常平衡模式】(CPU 75W / GPU 115W)".to_string(),
            Some("独显空载自然沉入 PCIe RTD3 D3cold 深度休眠，兼顾性能与整机能效".to_string()),
        )
    } else if raw.contains("Successfully applied profile 'office'") {
        (
            "已成功切换至【办公静音模式】(CPU 45W / 独显 D3cold 断电 0W)".to_string(),
            Some("核心调频锁定节能 EPP，最大限度减少机身发热并延长电池续航".to_string()),
        )
    } else if raw.contains("Battery charge threshold set to") || raw.contains("Battery threshold") {
        let th = raw
            .split("set to ")
            .nth(1)
            .and_then(|s| s.split('%').next())
            .unwrap_or("80");
        (
            format!("已成功配置电池充电养护阈值: {}%", th),
            Some("已持久化存储至 state.json，关机断电后自动恢复".to_string()),
        )
    } else if raw.contains("fan-boost") || raw.contains("Fan boost") || raw.contains("FanBoost") {
        (
            "散热强冷状态已更新".to_string(),
            Some(raw.to_string()),
        )
    } else if raw.contains("Keyboard brightness set to") {
        let br = raw
            .split("set to ")
            .nth(1)
            .and_then(|s| s.split('%').next())
            .unwrap_or("50");
        (
            format!("已成功设置键盘背光亮度: {}%", br),
            None,
        )
    } else {
        (raw.to_string(), None)
    }
}

pub fn render_status_bar(
    ctx: &egui::Context,
    status_message: &mut Option<(String, bool, Instant)>,
    chassis: &str,
    minimize_to_tray: &mut bool,
    autostart: &mut bool,
    action_tx: &Sender<IpcAction>,
) {
    let (has_status, is_err) = match status_message {
        Some((_, err, _)) => (true, *err),
        None => (false, false),
    };

    let bottom_bg = if has_status {
        if is_err {
            Color32::from_rgb(46, 20, 24)
        } else {
            Color32::from_rgb(14, 34, 44)
        }
    } else {
        theme::BG_PANEL
    };

    let bottom_stroke = if has_status {
        if is_err {
            Stroke::new(1.0_f32, theme::ACCENT_RED)
        } else {
            Stroke::new(1.0_f32, theme::ACCENT_CYAN)
        }
    } else {
        Stroke::new(1.0_f32, theme::BORDER_SUBTLE)
    };

    egui::TopBottomPanel::bottom("bottom_panel")
        .frame(
            egui::Frame::none()
                .fill(bottom_bg)
                .stroke(bottom_stroke)
                .inner_margin(Margin::symmetric(16.0, 7.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some((ref msg, is_err, _)) = status_message {
                    let (title, detail) = format_status_message(msg, *is_err);
                    let (badge_bg, icon_name, icon_svg, title_color, detail_color) = if *is_err {
                        (
                            Color32::from_rgb(185, 28, 28),
                            "alert_triangle",
                            icons::SVG_ALERT_TRIANGLE,
                            Color32::from_rgb(254, 202, 202),
                            Color32::from_rgb(252, 165, 165),
                        )
                    } else {
                        (
                            Color32::from_rgb(8, 145, 178),
                            "check",
                            icons::SVG_CHECK,
                            Color32::WHITE,
                            Color32::from_rgb(153, 246, 228),
                        )
                    };

                    let badge_text = if *is_err { "执行失败" } else { "生效成功" };
                    icons::render_badge(ui, badge_text, badge_bg, Color32::WHITE, Some((icon_name, icon_svg)));
                    ui.add_space(4.0);

                    ui.add(
                        egui::Label::new(
                            RichText::new(&title)
                                .color(title_color)
                                .strong()
                                .size(12.0),
                        )
                        .truncate(true),
                    );

                    if let Some(ref det) = detail {
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("• {}", det))
                                    .color(detail_color)
                                    .size(11.0),
                            )
                            .truncate(true),
                        );
                    }

                    ui.add_space(6.0);
                    let close_btn = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                        icons::render_svg(ui, "close", icons::SVG_CLOSE, Vec2::splat(10.0), Color32::from_rgb(203, 213, 225));
                        ui.small_button(
                            RichText::new("关闭")
                                .color(Color32::from_rgb(203, 213, 225))
                                .size(10.5),
                        )
                    }).inner;

                    if close_btn.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if close_btn.clicked() {
                        *status_message = None;
                    }
                } else {
                    ui.label(
                        RichText::new("快捷键: [1] 狂暴  [2] 平衡  [3] 办公  [B] 强冷  [R/F5] 刷新  [Ctrl+Q] 退出")
                            .color(theme::TEXT_SECONDARY)
                            .size(11.5),
                    );
                }

                // Right anchored controls (Permanent, zero layout shift)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(chassis)
                            .color(theme::TEXT_MUTED)
                            .size(11.0),
                    );
                    ui.add_space(8.0);
                    ui.checkbox(minimize_to_tray, "关闭最小化到托盘")
                        .on_hover_text("点击关闭窗口按钮时最小化到系统托盘，而非退出程序");
                    ui.add_space(8.0);

                    let autostart_resp = ui.checkbox(autostart, "开机静默自启");
                    if autostart_resp.changed() {
                        let _ = action_tx.send(IpcAction::SetAutostart(*autostart));
                    }
                    autostart_resp.on_hover_text("开机登录桌面后自动静默启动并常驻系统托盘");
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_status_message() {
        // Success scenarios
        let (title, detail) = format_status_message("Successfully applied profile 'turbo'", false);
        assert!(title.contains("狂暴满血模式"));
        assert!(detail.is_some());

        let (title, detail) = format_status_message("Battery charge threshold set to 80%", false);
        assert_eq!(title, "已成功配置电池充电养护阈值: 80%");
        assert!(detail.unwrap().contains("state.json"));

        // Error scenarios
        let (title, detail) = format_status_message("Hardware sysfs node or interface not accessible: Sysfs node not found: /sys/class/power_supply/BAT0/charge_control_end_threshold", true);
        assert_eq!(title, "电池充电养护阈值配置失败: 硬件节点不可用");
        assert!(detail.unwrap().contains("setup-tuxedo-drivers.sh"));

        let (title, detail) = format_status_message("Permission denied when writing battery threshold", true);
        assert!(title.contains("电池充电养护阈值配置失败") || title.contains("权限不足"));
        assert!(detail.is_some());
    }
}
