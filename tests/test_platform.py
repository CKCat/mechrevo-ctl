"""Tests for ACPI platform profile driver and ProfileManager."""

from pathlib import Path
from unittest.mock import MagicMock

import pytest

from mechrevo.core.cpu import IntelRaplDriver
from mechrevo.core.gpu import NvidiaSmiDriver
from mechrevo.core.hal import SysfsContext
from mechrevo.core.platform import AcpiPlatformDriver, ProfileManager


def test_read_platform_profile(mock_sysfs_dir: Path):
    """Verifies reading current ACPI platform profile."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = AcpiPlatformDriver(ctx=ctx)

    assert driver.get_current_profile() == "balanced"
    assert driver.get_available_profiles() == [
        "low-power",
        "balanced",
        "performance",
    ]


def test_set_platform_profile(mock_sysfs_dir: Path):
    """Verifies writing valid ACPI platform profile."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = AcpiPlatformDriver(ctx=ctx)

    driver.set_profile("performance")
    profile_file = ctx.resolve("/sys/firmware/acpi/platform_profile")
    assert profile_file.read_text(encoding="utf-8").strip() == "performance"


def test_set_invalid_platform_profile(mock_sysfs_dir: Path):
    """Verifies rejecting unsupported platform profile."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = AcpiPlatformDriver(ctx=ctx)

    with pytest.raises(ValueError, match="Unsupported ACPI profile"):
        driver.set_profile("ultra_extreme")


def test_profile_manager_apply(mock_sysfs_dir: Path, monkeypatch: pytest.MonkeyPatch):
    """Verifies ProfileManager coordinating ACPI, CPU, and GPU settings."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    mock_run.return_value.stdout = "OK"
    monkeypatch.setattr("subprocess.run", mock_run)

    ctx = SysfsContext(root_path=mock_sysfs_dir)
    cpu_drv = IntelRaplDriver(ctx=ctx)
    gpu_drv = NvidiaSmiDriver()
    platform_drv = AcpiPlatformDriver(ctx=ctx)

    mgr = ProfileManager(
        cpu_driver=cpu_drv, gpu_driver=gpu_drv, platform_driver=platform_drv
    )
    result = mgr.apply_profile("turbo")

    assert result.is_success is True
    assert result.applied_profile.name == "turbo"

    # Verify CPU PL1 / PL2
    cpu_limits = cpu_drv.get_power_limits()
    assert cpu_limits.pl1_w == 105.0
    assert cpu_limits.pl2_w == 135.0

    # Verify ACPI profile
    assert platform_drv.get_current_profile() == "performance"

    # Verify GPU SMI called
    mock_run.assert_called()
