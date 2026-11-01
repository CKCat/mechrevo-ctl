//! System tray integration via ksni.

use std::sync::mpsc::Sender;
use eframe::egui;

#[derive(Clone, Debug)]
pub enum TrayCommand {
    ToggleWindow,
    ShowWindow,
    #[allow(dead_code)]
    HideWindow,
    SetProfile(String),
    SetFanBoost(bool),
    SetBattery(u32),
    Quit,
}

#[derive(Clone)]
pub struct MechrevoTray {
    pub active_profile: String,
    pub fan_boost: bool,
    pub cpu_temp: Option<i32>,
    pub gpu_temp: Option<i32>,
    pub gpu_d3cold: bool,
    pub battery_th: Option<u32>,
    pub ac_online: bool,
    pub tray_tx: Sender<TrayCommand>,
    pub ctx: egui::Context,
}

/// Formats the system tray tooltip strings.
/// Explicitly reports PCIe RTD3 D3cold deep sleep state (0W) instead of misleading "--".
pub fn format_tray_tooltip(
    active_profile: &str,
    cpu_temp: Option<i32>,
    gpu_temp: Option<i32>,
    gpu_d3cold: bool,
    ac_online: bool,
    fan_boost: bool,
) -> (String, String) {
    let prof_name = match active_profile {
        "turbo" => "狂暴满血 (105W/140W)",
        "office" => "办公静音 (45W/0W)",
        _ => "日常平衡 (75W/115W)",
    };
    let cpu_str = cpu_temp
        .map(|t| format!("{}°C", t))
        .unwrap_or_else(|| "--".to_string());
    let gpu_str = if gpu_d3cold {
        "深度休眠 (0W)".to_string()
    } else if let Some(t) = gpu_temp {
        format!("{}°C", t)
    } else {
        "离线".to_string()
    };
    let pwr_str = if ac_online { "AC 适配器供电" } else { "电池供电" };
    let boost_str = if fan_boost { " | 强冷开启" } else { "" };

    (
        format!("机械革命控制中心 - {}", prof_name),
        format!("CPU: {} | GPU: {} | {}{}", cpu_str, gpu_str, pwr_str, boost_str),
    )
}

impl ksni::Tray for MechrevoTray {
    fn id(&self) -> String {
        "mechrevo-control-center".to_string()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn title(&self) -> String {
        "机械革命控制中心".to_string()
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn icon_name(&self) -> String {
        "mechrevo-control-center".to_string()
    }

    fn icon_theme_path(&self) -> String {
        if let Ok(home) = std::env::var("HOME") {
            let local_dir = format!("{}/.local/share/icons/hicolor/scalable/apps", home);
            if std::path::Path::new(&local_dir).exists() {
                return local_dir;
            }
        }
        "/usr/share/icons/hicolor/scalable/apps".to_string()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let (title, description) = format_tray_tooltip(
            &self.active_profile,
            self.cpu_temp,
            self.gpu_temp,
            self.gpu_d3cold,
            self.ac_online,
            self.fan_boost,
        );

        ksni::ToolTip {
            title,
            description,
            icon_name: "mechrevo-control-center".to_string(),
            icon_pixmap: Vec::new(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tray_tx.send(TrayCommand::ToggleWindow);
        self.ctx.request_repaint();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let tx = self.tray_tx.clone();
        let ctx = self.ctx.clone();

        let tx_prof = tx.clone();
        let ctx_prof = ctx.clone();
        let current_prof = self.active_profile.clone();

        let tx_bat = tx.clone();
        let ctx_bat = ctx.clone();
        let current_th = self.battery_th.unwrap_or(80);

        let make_cb = |cmd: TrayCommand| -> Box<dyn Fn(&mut Self) + Send> {
            let tx = tx.clone();
            let ctx = ctx.clone();
            Box::new(move |_: &mut Self| {
                let _ = tx.send(cmd.clone());
                ctx.request_repaint();
            })
        };

        let current_boost = self.fan_boost;

        let selected_prof = match current_prof.as_str() {
            "turbo" => 0,
            "balanced" => 1,
            "office" => 2,
            _ => 1,
        };

        let selected_bat = match current_th {
            th if th <= 65 => 2, // 60%
            th if th <= 85 => 0, // 80%
            _ => 1,              // 100%
        };

        vec![
            RadioGroup {
                selected: selected_prof,
                select: Box::new(move |tray: &mut Self, index: usize| {
                    let (prof, cmd) = match index {
                        0 => ("turbo", TrayCommand::SetProfile("turbo".to_string())),
                        1 => ("balanced", TrayCommand::SetProfile("balanced".to_string())),
                        2 => ("office", TrayCommand::SetProfile("office".to_string())),
                        _ => ("balanced", TrayCommand::SetProfile("balanced".to_string())),
                    };
                    tray.active_profile = prof.to_string();
                    let _ = tx_prof.send(cmd);
                    ctx_prof.request_repaint();
                }),
                options: vec![
                    RadioItem {
                        label: "狂暴满血 (Turbo 105W/140W)".to_string(),
                        ..Default::default()
                    },
                    RadioItem {
                        label: "日常平衡 (Balanced 75W/115W)".to_string(),
                        ..Default::default()
                    },
                    RadioItem {
                        label: "办公静音 (Office 45W/0W)".to_string(),
                        ..Default::default()
                    },
                ],
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "双风扇 100% 极限强冷 (Fan Boost)".to_string(),
                enabled: true,
                visible: true,
                checked: current_boost,
                activate: make_cb(TrayCommand::SetFanBoost(!current_boost)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "电池健康与充电养护".to_string(),
                enabled: true,
                visible: true,
                submenu: vec![
                    RadioGroup {
                        selected: selected_bat,
                        select: Box::new(move |tray: &mut Self, index: usize| {
                            let th = match index {
                                0 => 80,
                                1 => 100,
                                2 => 60,
                                _ => 80,
                            };
                            tray.battery_th = Some(th);
                            let _ = tx_bat.send(TrayCommand::SetBattery(th));
                            ctx_bat.request_repaint();
                        }),
                        options: vec![
                            RadioItem {
                                label: "80% 常年插电养护 (推荐)".to_string(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: "100% 满电出行备战".to_string(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: "60% 深度存放养护".to_string(),
                                ..Default::default()
                            },
                        ],
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "打开控制中心主界面".to_string(),
                enabled: true,
                visible: true,
                activate: make_cb(TrayCommand::ShowWindow),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "彻底退出".to_string(),
                enabled: true,
                visible: true,
                activate: make_cb(TrayCommand::Quit),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tray_tooltip() {
        let (title, desc) = format_tray_tooltip("office", Some(56), None, true, true, false);
        assert_eq!(title, "机械革命控制中心 - 办公静音 (45W/0W)");
        assert_eq!(desc, "CPU: 56°C | GPU: 深度休眠 (0W) | AC 适配器供电");

        let (title, desc) = format_tray_tooltip("turbo", Some(78), Some(65), false, true, true);
        assert_eq!(title, "机械革命控制中心 - 狂暴满血 (105W/140W)");
        assert_eq!(desc, "CPU: 78°C | GPU: 65°C | AC 适配器供电 | 强冷开启");

        let (title, desc) = format_tray_tooltip("balanced", Some(48), None, false, false, false);
        assert_eq!(title, "机械革命控制中心 - 日常平衡 (75W/115W)");
        assert_eq!(desc, "CPU: 48°C | GPU: 离线 | 电池供电");
    }
}

