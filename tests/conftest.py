"""Pytest fixtures and configuration for mechrevo-ctl tests."""

from pathlib import Path

import pytest

from mechrevo.core.hal import SysfsContext


@pytest.fixture
def mock_sysfs_dir(tmp_path: Path) -> Path:
    """Creates a mock Linux sysfs hierarchy for testing hardware drivers."""
    ctx = SysfsContext(root_path=tmp_path)

    def mock_write(path_str: str, text: str) -> None:
        target = ctx.resolve(path_str)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    # 1. Intel RAPL sysfs
    mock_write(
        "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw",
        "75000000\n",
    )
    mock_write(
        "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_1_power_limit_uw",
        "90000000\n",
    )

    # 2. ACPI platform profile
    mock_write("/sys/firmware/acpi/platform_profile", "balanced\n")
    mock_write(
        "/sys/firmware/acpi/platform_profile_choices",
        "low-power balanced performance\n",
    )

    # 3. Battery charge threshold
    mock_write("/sys/class/power_supply/BAT0/charge_control_end_threshold", "100\n")
    mock_write("/sys/class/power_supply/BAT0/capacity", "85\n")
    mock_write("/sys/class/power_supply/BAT0/status", "Discharging\n")

    # 4. Keyboard backlight
    mock_write("/sys/class/leds/uniwill::kbd_backlight/brightness", "128\n")
    mock_write("/sys/class/leds/uniwill::kbd_backlight/max_brightness", "255\n")

    return tmp_path
