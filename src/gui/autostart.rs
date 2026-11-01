//! XDG desktop autostart entry management.

use std::path::{Path, PathBuf};

/// Returns the standard XDG autostart path for the user desktop entry.
pub fn get_autostart_path() -> Option<PathBuf> {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !config_home.trim().is_empty() {
            return Some(
                PathBuf::from(config_home)
                    .join("autostart")
                    .join("mechrevo-control-center.desktop"),
            );
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Some(
                PathBuf::from(home)
                    .join(".config")
                    .join("autostart")
                    .join("mechrevo-control-center.desktop"),
            );
        }
    }
    None
}

/// Parses an autostart desktop entry content to determine if it is actively enabled.
pub fn is_autostart_content_enabled(content: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("hidden=true")
            || trimmed.eq_ignore_ascii_case("x-gnome-autostart-enabled=false")
        {
            return false;
        }
    }
    true
}

/// Checks whether mechrevo-gui is configured to autostart upon user login.
pub fn is_autostart_enabled() -> bool {
    // 1. Check user local autostart file first
    if let Some(user_path) = get_autostart_path() {
        if user_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&user_path) {
                return is_autostart_content_enabled(&content);
            }
        }
    }

    // 2. Check system-wide autostart file
    let sys_path = Path::new("/etc/xdg/autostart/mechrevo-control-center.desktop");
    if sys_path.exists() {
        if let Ok(content) = std::fs::read_to_string(sys_path) {
            return is_autostart_content_enabled(&content);
        }
    }

    false
}

/// Enables or disables autostart for the GUI desktop application.
pub fn set_autostart(enabled: bool) -> Result<(), std::io::Error> {
    let path = get_autostart_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "未找到有效的用户配置目录 (~/.config)")
    })?;

    let sys_path = Path::new("/etc/xdg/autostart/mechrevo-control-center.desktop");
    let sys_exists = sys_path.exists();

    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = "\
[Desktop Entry]
Type=Application
Name=MECHREVO Control Center
Name[zh_CN]=机械革命控制中心
Comment=Hardware power, thermal, battery and performance management for MECHREVO laptops
Comment[zh_CN]=机械革命笔记本功耗释放、性能模式、风扇强冷与电池养护控制中心
Exec=mechrevo-gui --minimized
Icon=mechrevo-control-center
Terminal=false
Categories=Settings;HardwareSettings;System;Utility;
StartupNotify=false
X-GNOME-Autostart-enabled=true
";
        std::fs::write(&path, content)?;
    } else if sys_exists {
        // Mask the system-wide autostart entry with Hidden=true in user directory
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = "\
[Desktop Entry]
Type=Application
Name=MECHREVO Control Center
Exec=mechrevo-gui --minimized
Hidden=true
X-GNOME-Autostart-enabled=false
";
        std::fs::write(&path, content)?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_content_parsing() {
        let normal = "[Desktop Entry]\nType=Application\nExec=mechrevo-gui --minimized\n";
        assert!(is_autostart_content_enabled(normal));

        let hidden = "[Desktop Entry]\nType=Application\nHidden=true\nExec=mechrevo-gui --minimized\n";
        assert!(!is_autostart_content_enabled(hidden));

        let gnome_disabled = "[Desktop Entry]\nType=Application\nX-GNOME-Autostart-enabled=false\n";
        assert!(!is_autostart_content_enabled(gnome_disabled));

        let gnome_enabled = "[Desktop Entry]\nType=Application\nX-GNOME-Autostart-enabled=true\n";
        assert!(is_autostart_content_enabled(gnome_enabled));
    }
}

