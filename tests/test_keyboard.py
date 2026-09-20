"""Tests for keyboard backlight controller."""

from pathlib import Path

import pytest

from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.hal import SysfsContext
from mechrevo.core.keyboard import KeyboardDriver


def test_read_keyboard_brightness(mock_sysfs_dir: Path):
    """Verifies reading brightness and converting to percentage."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = KeyboardDriver(ctx=ctx)

    # 128 / 255 ~= 50%
    percent = driver.get_brightness_percent()
    assert percent == 50


def test_set_keyboard_brightness(mock_sysfs_dir: Path):
    """Verifies writing brightness from percentage to raw scale."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = KeyboardDriver(ctx=ctx)

    driver.set_brightness_percent(100)
    brightness_file = ctx.resolve("/sys/class/leds/uniwill::kbd_backlight/brightness")
    assert brightness_file.read_text(encoding="utf-8").strip() == "255"

    driver.set_brightness_percent(0)
    assert brightness_file.read_text(encoding="utf-8").strip() == "0"


def test_set_keyboard_brightness_validation(mock_sysfs_dir: Path):
    """Ensures percentage out of 0-100 raises ValueError."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = KeyboardDriver(ctx=ctx)

    with pytest.raises(
        ValueError, match="Brightness percentage must be between 0 and 100"
    ):
        driver.set_brightness_percent(-5)

    with pytest.raises(
        ValueError, match="Brightness percentage must be between 0 and 100"
    ):
        driver.set_brightness_percent(120)


def test_missing_keyboard_sysfs(tmp_path: Path):
    """Verifies error when keyboard led node is not present."""
    ctx = SysfsContext(root_path=tmp_path)
    driver = KeyboardDriver(ctx=ctx)

    with pytest.raises(
        HardwareInterfaceError, match="Keyboard LED sysfs node not found"
    ):
        driver.get_brightness_percent()


def test_keyboard_probing_alternate_led(tmp_path: Path):
    """Verifies detecting non-standard LED nodes such as tuxedo::kbd_backlight."""
    ctx = SysfsContext(root_path=tmp_path)
    tuxedo_led = ctx.resolve("/sys/class/leds/tuxedo::kbd_backlight")
    tuxedo_led.mkdir(parents=True, exist_ok=True)
    (tuxedo_led / "brightness").write_text("100\n", encoding="utf-8")
    (tuxedo_led / "max_brightness").write_text("200\n", encoding="utf-8")

    driver = KeyboardDriver(ctx=ctx)
    assert driver.get_brightness_percent() == 50

    driver.set_brightness_percent(80)
    assert (tuxedo_led / "brightness").read_text(encoding="utf-8").strip() == "160"


def test_keyboard_low_percent_not_zero(tmp_path: Path):
    """Verifies that non-zero brightness percentage never rounds down to 0 on low max_brightness hardware."""
    ctx = SysfsContext(root_path=tmp_path)
    led = ctx.resolve("/sys/class/leds/uniwill::kbd_backlight")
    led.mkdir(parents=True, exist_ok=True)
    # Hardware with 4 brightness steps (e.g. 0, 1, 2, 3, 4)
    (led / "brightness").write_text("0\n", encoding="utf-8")
    (led / "max_brightness").write_text("4\n", encoding="utf-8")

    driver = KeyboardDriver(ctx=ctx)
    # 10% on 4 max_brightness: standard round(0.1*4) = 0, but user intended non-zero light!
    driver.set_brightness_percent(10)
    written_val = int((led / "brightness").read_text(encoding="utf-8").strip())
    assert written_val >= 1
