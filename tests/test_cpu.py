"""Tests for Intel RAPL CPU power limit controller."""

from pathlib import Path

import pytest

from mechrevo.core.cpu import IntelRaplDriver
from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.hal import SysfsContext


def test_read_cpu_power_limits(mock_sysfs_dir: Path):
    """Verifies reading PL1 and PL2 from sysfs."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = IntelRaplDriver(ctx=ctx)

    limits = driver.get_power_limits()
    assert limits.pl1_w == 75.0
    assert limits.pl2_w == 90.0


def test_set_cpu_power_limits(mock_sysfs_dir: Path):
    """Verifies writing PL1 and PL2 to sysfs."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = IntelRaplDriver(ctx=ctx)

    driver.set_power_limits(pl1_w=105, pl2_w=135)

    limits = driver.get_power_limits()
    assert limits.pl1_w == 105.0
    assert limits.pl2_w == 135.0

    # Verify raw file content in microwatts
    pl1_file = ctx.resolve(
        "/sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_power_limit_uw"
    )
    assert pl1_file.read_text(encoding="utf-8").strip() == "105000000"


def test_set_cpu_power_limits_validation(mock_sysfs_dir: Path):
    """Ensures setting absurd or negative power limits raises ValueError."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = IntelRaplDriver(ctx=ctx)

    with pytest.raises(ValueError, match="PL1 must be between"):
        driver.set_power_limits(pl1_w=-10, pl2_w=90)

    with pytest.raises(ValueError, match="PL1 must be between"):
        driver.set_power_limits(pl1_w=250, pl2_w=300)


def test_missing_rapl_interface(tmp_path: Path):
    """Verifies graceful exception when RAPL node is missing."""
    empty_ctx = SysfsContext(root_path=tmp_path)
    driver = IntelRaplDriver(ctx=empty_ctx)

    with pytest.raises(HardwareInterfaceError, match="Intel RAPL sysfs node not found"):
        driver.get_power_limits()


def test_set_cpu_power_limits_write_order(
    mock_sysfs_dir: Path, monkeypatch: pytest.MonkeyPatch
):
    """Verifies that increasing limits writes PL2 before PL1, and decreasing writes PL1 before PL2."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = IntelRaplDriver(ctx=ctx)

    write_history = []
    original_write = ctx.write_text

    def tracking_write(path_str: str, val: str):
        write_history.append((path_str, val))
        original_write(path_str, val)

    monkeypatch.setattr(ctx, "write_text", tracking_write)

    # Currently 75W / 90W. Increasing to 105W / 135W:
    write_history.clear()
    driver.set_power_limits(105, 135)
    assert len(write_history) == 2
    assert "constraint_1" in write_history[0][0]
    assert "constraint_0" in write_history[1][0]

    # Decreasing to 45W / 65W:
    write_history.clear()
    driver.set_power_limits(45, 65)
    assert len(write_history) == 2
    assert "constraint_0" in write_history[0][0]
    assert "constraint_1" in write_history[1][0]


def test_set_cpu_power_limits_mathematical_order(
    mock_sysfs_dir: Path, monkeypatch: pytest.MonkeyPatch
):
    """Verifies PL2 expansion writes PL2 first even when new PL1 <= old PL2."""
    ctx = SysfsContext(root_path=mock_sysfs_dir)
    driver = IntelRaplDriver(ctx=ctx)

    write_history = []
    original_write = ctx.write_text

    def tracking_write(path_str: str, val: str):
        write_history.append((path_str, val))
        original_write(path_str, val)

    monkeypatch.setattr(ctx, "write_text", tracking_write)

    # Currently 75W / 90W. Target: 80W / 100W (PL2 expands from 90W to 100W, but 80W < 90W)
    # Mathematical proof demands: writing PL2 first is the only sequence preserving PL1 <= PL2 invariant.
    write_history.clear()
    driver.set_power_limits(80, 100)
    assert len(write_history) == 2
    # First write must be PL2 (constraint_1)
    assert "constraint_1" in write_history[0][0]
    # Second write must be PL1 (constraint_0)
    assert "constraint_0" in write_history[1][0]


def test_rapl_dynamic_constraint_name_matching(tmp_path: Path):
    """Verifies that driver resolves PL1/PL2 via constraint_X_name when order is reversed."""
    ctx = SysfsContext(root_path=tmp_path)
    base = ctx.resolve("/sys/class/powercap/intel-rapl/intel-rapl:0")
    base.mkdir(parents=True, exist_ok=True)

    # Inverted order in mock sysfs: constraint_0 is short_term (PL2), constraint_1 is long_term (PL1)
    (base / "constraint_0_name").write_text("short_term\n", encoding="utf-8")
    (base / "constraint_0_power_limit_uw").write_text("90000000\n", encoding="utf-8")
    (base / "constraint_1_name").write_text("long_term\n", encoding="utf-8")
    (base / "constraint_1_power_limit_uw").write_text("75000000\n", encoding="utf-8")

    driver = IntelRaplDriver(ctx=ctx)
    limits = driver.get_power_limits()
    assert limits.pl1_w == 75.0
    assert limits.pl2_w == 90.0

    driver.set_power_limits(105, 135)
    # PL1 (105W) should go to constraint_1, PL2 (135W) should go to constraint_0
    assert (base / "constraint_1_power_limit_uw").read_text(
        encoding="utf-8"
    ).strip() == "105000000"
    assert (base / "constraint_0_power_limit_uw").read_text(
        encoding="utf-8"
    ).strip() == "135000000"
