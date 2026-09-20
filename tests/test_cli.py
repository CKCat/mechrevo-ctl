"""Tests for CLI interface."""

from pathlib import Path
from unittest.mock import MagicMock

import pytest

from mechrevo.cli import main
from mechrevo.core.hal import SysfsContext


def test_cli_status_command(
    mock_sysfs_dir: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture,
):
    """Verifies `mechrevo-ctl status` prints formatted hardware overview."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    mock_run.return_value.stdout = (
        "NVIDIA GeForce RTX 4060 Laptop GPU, 115.00 W, 140.00 W, 55 C\n"
    )
    monkeypatch.setattr("subprocess.run", mock_run)

    test_ctx = SysfsContext(root_path=mock_sysfs_dir)
    monkeypatch.setattr("mechrevo.cli.get_default_sysfs_context", lambda: test_ctx)

    exit_code = main(["status"])
    assert exit_code == 0

    captured = capsys.readouterr()
    assert "MECHREVO Control Center" in captured.out
    assert "Intel RAPL (CPU)" in captured.out
    assert "Battery" in captured.out
    assert "Keyboard" in captured.out


def test_cli_profile_command(
    mock_sysfs_dir: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture,
):
    """Verifies `mechrevo-ctl profile turbo` activates turbo mode."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    mock_run.return_value.stdout = "OK"
    monkeypatch.setattr("subprocess.run", mock_run)

    test_ctx = SysfsContext(root_path=mock_sysfs_dir)
    monkeypatch.setattr("mechrevo.cli.get_default_sysfs_context", lambda: test_ctx)

    exit_code = main(["profile", "turbo"])
    assert exit_code == 0

    captured = capsys.readouterr()
    assert "Successfully switched to [turbo]" in captured.out


def test_cli_battery_command(
    mock_sysfs_dir: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture,
):
    """Verifies `mechrevo-ctl battery 80` sets battery charging threshold."""
    test_ctx = SysfsContext(root_path=mock_sysfs_dir)
    monkeypatch.setattr("mechrevo.cli.get_default_sysfs_context", lambda: test_ctx)

    exit_code = main(["battery", "80"])
    assert exit_code == 0

    captured = capsys.readouterr()
    assert "Battery charge threshold set to 80%" in captured.out


def test_cli_keyboard_command(
    mock_sysfs_dir: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture,
):
    """Verifies `mechrevo-ctl keyboard --brightness 75` sets keyboard brightness."""
    test_ctx = SysfsContext(root_path=mock_sysfs_dir)
    monkeypatch.setattr("mechrevo.cli.get_default_sysfs_context", lambda: test_ctx)

    exit_code = main(["keyboard", "--brightness", "75"])
    assert exit_code == 0

    captured = capsys.readouterr()
    assert "Keyboard brightness set to 75%" in captured.out


def test_cli_ipc_error_passthrough_not_masked(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
):
    """Verifies that daemon error response is reported directly, without masking as sudo permission error."""
    # Simulate daemon running and returning an error
    monkeypatch.setattr(
        "mechrevo.cli.send_ipc_command",
        lambda data, **kwargs: {
            "success": False,
            "error": "Hardware node offline",
        },
    )

    exit_code = main(["profile", "turbo"])
    assert exit_code != 0

    captured = capsys.readouterr()
    assert (
        "Hardware node offline" in captured.err
        or "Hardware node offline" in captured.out
    )
    assert "sudo" not in captured.err
