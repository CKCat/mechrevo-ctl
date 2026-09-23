//! Keyboard backlight and brightness driver.

use std::fs;

use crate::error::{MechrevoError, Result};
use crate::hal::SysfsContext;

pub const PRIMARY_LED_PATH: &str = "/sys/class/leds/uniwill::kbd_backlight";
pub const LEDS_BASE_PATH: &str = "/sys/class/leds";

pub const WELL_KNOWN_LED_CANDIDATES: &[&str] = &[
    "/sys/class/leds/uniwill::kbd_backlight",
    "/sys/class/leds/rgb:kbd_backlight",
    "/sys/class/leds/white:kbd_backlight",
    "/sys/class/leds/tuxedo::kbd_backlight",
    "/sys/class/leds/mechrevo::kbd_backlight",
    "/sys/class/leds/mech_forza::kbd_backlight",
];

pub struct KeyboardDriver {
    ctx: SysfsContext,
}

impl KeyboardDriver {
    pub fn new(ctx: SysfsContext) -> Self {
        Self { ctx }
    }

    /// Locates the active keyboard backlight sysfs path, prioritizing well-known OEM/DKMS drivers.
    pub fn find_led_path(&self) -> Result<String> {
        for candidate in WELL_KNOWN_LED_CANDIDATES {
            if self.ctx.exists(candidate) {
                return Ok(candidate.to_string());
            }
        }

        let base = self.ctx.resolve(LEDS_BASE_PATH);
        if base.exists() {
            if let Ok(entries) = fs::read_dir(&base) {
                let mut candidates: Vec<_> = entries.flatten().collect();
                candidates.sort_by_key(|e| e.file_name());

                for entry in candidates {
                    let file_name = entry.file_name();
                    let name_lower = file_name.to_string_lossy().to_lowercase();
                    if (name_lower.contains("kbd_backlight") || name_lower.contains("keyboard"))
                        && entry.path().join("brightness").exists()
                    {
                        return Ok(format!("{}/{}", LEDS_BASE_PATH, file_name.to_string_lossy()));
                    }
                }
            }
        }

        Err(MechrevoError::HardwareInterface(
            "Keyboard LED sysfs node not found".to_string(),
        ))
    }

    /// Reads keyboard brightness as a percentage (0-100%).
    pub fn get_brightness_percent(&self) -> Result<u32> {
        let base_path = self.find_led_path()?;
        let brightness_node = format!("{}/brightness", base_path);
        let max_brightness_node = format!("{}/max_brightness", base_path);

        let raw_brightness: u32 = self
            .ctx
            .read_text(&brightness_node)?
            .parse()
            .map_err(|e| MechrevoError::HardwareInterface(format!("Invalid brightness: {}", e)))?;
        let max_brightness: u32 = self
            .ctx
            .read_text(&max_brightness_node)?
            .parse()
            .map_err(|e| MechrevoError::HardwareInterface(format!("Invalid max_brightness: {}", e)))?;

        if max_brightness == 0 {
            return Ok(0);
        }

        let percent = ((raw_brightness as f64 / max_brightness as f64) * 100.0).round() as u32;
        Ok(percent.min(100))
    }

    /// Sets keyboard brightness as a percentage (0-100%).
    pub fn set_brightness_percent(&self, percent: u32) -> Result<()> {
        if percent > 100 {
            return Err(MechrevoError::InvalidParameter(format!(
                "Brightness percentage must be between 0 and 100 (got {})",
                percent
            )));
        }

        let base_path = self.find_led_path()?;
        let brightness_node = format!("{}/brightness", base_path);
        let max_brightness_node = format!("{}/max_brightness", base_path);

        let max_brightness: u32 = self
            .ctx
            .read_text(&max_brightness_node)?
            .parse()
            .map_err(|e| MechrevoError::HardwareInterface(format!("Invalid max_brightness: {}", e)))?;

        let mut raw_target = ((percent as f64 / 100.0) * max_brightness as f64).round() as u32;

        // 工业级保底：非零百分比输入绝不能因四舍五入归零导致误灭灯
        if percent > 0 && raw_target == 0 && max_brightness > 0 {
            raw_target = 1;
        }

        self.ctx
            .write_text(&brightness_node, &format!("{}\n", raw_target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_keyboard_brightness_percentage() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let led_dir = ctx.resolve(PRIMARY_LED_PATH);
        std::fs::create_dir_all(&led_dir).unwrap();
        std::fs::write(led_dir.join("brightness"), "128\n").unwrap();
        std::fs::write(led_dir.join("max_brightness"), "255\n").unwrap();

        let driver = KeyboardDriver::new(ctx.clone());
        assert_eq!(driver.get_brightness_percent().unwrap(), 50);

        driver.set_brightness_percent(100).unwrap();
        let updated = std::fs::read_to_string(led_dir.join("brightness")).unwrap();
        assert_eq!(updated.trim(), "255");
    }

    #[test]
    fn test_keyboard_low_percent_not_zero() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let led_dir = ctx.resolve(PRIMARY_LED_PATH);
        std::fs::create_dir_all(&led_dir).unwrap();
        std::fs::write(led_dir.join("brightness"), "0\n").unwrap();
        std::fs::write(led_dir.join("max_brightness"), "4\n").unwrap();

        let driver = KeyboardDriver::new(ctx.clone());
        // 10% on 4 levels: 0.4 -> would round to 0, but guard guarantees 1
        driver.set_brightness_percent(10).unwrap();
        let updated = std::fs::read_to_string(led_dir.join("brightness")).unwrap();
        assert_eq!(updated.trim(), "1");
    }

    #[test]
    fn test_keyboard_validation() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());
        let driver = KeyboardDriver::new(ctx);

        assert!(driver.set_brightness_percent(105).is_err());
    }

    #[test]
    fn test_keyboard_well_known_candidates_priority() {
        let dir = tempdir().unwrap();
        let ctx = SysfsContext::with_root(dir.path());

        // Create generic input3 node (alphabetically earlier than rgb)
        let generic_led = ctx.resolve("/sys/class/leds/input3::kbd_backlight");
        std::fs::create_dir_all(&generic_led).unwrap();
        std::fs::write(generic_led.join("brightness"), "10\n").unwrap();
        std::fs::write(generic_led.join("max_brightness"), "100\n").unwrap();

        // Create well-known rgb node
        let rgb_led = ctx.resolve("/sys/class/leds/rgb:kbd_backlight");
        std::fs::create_dir_all(&rgb_led).unwrap();
        std::fs::write(rgb_led.join("brightness"), "50\n").unwrap();
        std::fs::write(rgb_led.join("max_brightness"), "100\n").unwrap();

        let driver = KeyboardDriver::new(ctx);
        let found = driver.find_led_path().unwrap();
        assert_eq!(
            found, "/sys/class/leds/rgb:kbd_backlight",
            "Well-known LED node (e.g. rgb:kbd_backlight) should be prioritized over generic input devices"
        );
    }
}
