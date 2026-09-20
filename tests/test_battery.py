"""Tests for Battery charging threshold controller."""

from pathlib import Path

import pytest

from mechrevo.core.battery import BatteryDriver
from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.hal import SysfsContext


def test_read_battery_status(mock_sysfs_dir: Path):
    """Verifies reading charge threshold and status from sysfs."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = BatteryDriver(ctx=ctx)

    status = driver.get_battery_status()
    assert status.capacity_percent == 85
    assert status.status == "Discharging"
    assert status.charge_threshold == 100


def test_set_battery_threshold(mock_sysfs_dir: Path):
    """Verifies writing charging limit threshold."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = BatteryDriver(ctx=ctx)

    driver.set_charge_threshold(80)

    threshold_file = ctx.resolve(
        "/sys/class/power_supply/BAT0/charge_control_end_threshold"
    )
    assert threshold_file.read_text(encoding="utf-8").strip() == "80"
    assert driver.get_battery_status().charge_threshold == 80


def test_set_battery_threshold_validation(mock_sysfs_dir: Path):
    """Ensures threshold out of 20-100 range raises ValueError."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = BatteryDriver(ctx=ctx)

    with pytest.raises(
        ValueError, match="Battery threshold must be between 20 and 100"
    ):
        driver.set_charge_threshold(15)

    with pytest.raises(
        ValueError, match="Battery threshold must be between 20 and 100"
    ):
        driver.set_charge_threshold(105)


def test_missing_battery_sysfs(tmp_path: Path):
    """Verifies error when battery sysfs node does not exist."""
    ctx = SysfsContext(root_path=tmp_path)
    driver = BatteryDriver(ctx=ctx)

    with pytest.raises(HardwareInterfaceError, match="Battery sysfs node not found"):
        driver.get_battery_status()


def test_battery_probing_bat1(tmp_path: Path):
    """Verifies that BatteryDriver auto-detects BAT1 if BAT0 does not exist."""
    ctx = SysfsContext(root_path=tmp_path)
    bat1_path = ctx.resolve("/sys/class/power_supply/BAT1")
    bat1_path.mkdir(parents=True, exist_ok=True)
    (bat1_path / "capacity").write_text("92\n", encoding="utf-8")
    (bat1_path / "status").write_text("Charging\n", encoding="utf-8")
    (bat1_path / "charge_control_end_threshold").write_text("80\n", encoding="utf-8")

    driver = BatteryDriver(ctx=ctx)
    status = driver.get_battery_status()
    assert status.capacity_percent == 92
    assert status.status == "Charging"
    assert status.charge_threshold == 80

    driver.set_charge_threshold(60)
    assert (bat1_path / "charge_control_end_threshold").read_text(
        encoding="utf-8"
    ).strip() == "60"


def test_battery_fallback_charge_stop_threshold(tmp_path: Path):
    """Verifies that BatteryDriver supports legacy/vendor charge_stop_threshold node."""
    ctx = SysfsContext(root_path=tmp_path)
    bat0_path = ctx.resolve("/sys/class/power_supply/BAT0")
    bat0_path.mkdir(parents=True, exist_ok=True)
    (bat0_path / "capacity").write_text("70\n", encoding="utf-8")
    (bat0_path / "status").write_text("Discharging\n", encoding="utf-8")
    # Only charge_stop_threshold exists here
    (bat0_path / "charge_stop_threshold").write_text("80\n", encoding="utf-8")

    driver = BatteryDriver(ctx=ctx)
    status = driver.get_battery_status()
    assert status.charge_threshold == 80

    driver.set_charge_threshold(60)
    assert (bat0_path / "charge_stop_threshold").read_text(
        encoding="utf-8"
    ).strip() == "60"
