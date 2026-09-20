"""Tests for Daemon IPC and event handling."""

import json
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from mechrevo.core.hal import SysfsContext
from mechrevo.daemon import MechrevoDaemon


def test_daemon_handle_ipc_message(
    mock_sysfs_dir: Path, monkeypatch: pytest.MonkeyPatch
):
    """Verifies that daemon processes IPC JSON commands correctly."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    mock_run.return_value.stdout = "OK"
    monkeypatch.setattr("subprocess.run", mock_run)

    ctx = SysfsContext(root_path=mock_sysfs_dir)
    daemon = MechrevoDaemon(ctx=ctx)

    # 1. Test profile command
    response = daemon.handle_ipc_request(
        json.dumps({"action": "profile", "mode": "turbo"})
    )
    resp_data = json.loads(response)
    assert resp_data["success"] is True
    assert "turbo" in resp_data["message"]

    # 2. Test battery command
    response = daemon.handle_ipc_request(
        json.dumps({"action": "battery", "threshold": 80})
    )
    resp_data = json.loads(response)
    assert resp_data["success"] is True
    assert "80" in resp_data["message"]

    # 3. Test keyboard command
    response = daemon.handle_ipc_request(
        json.dumps({"action": "keyboard", "brightness": 60})
    )
    resp_data = json.loads(response)
    assert resp_data["success"] is True

    # 4. Test invalid command
    response = daemon.handle_ipc_request(json.dumps({"action": "invalid"}))
    resp_data = json.loads(response)
    assert resp_data["success"] is False


def test_daemon_user_profile_lock_prevents_overwrite(
    mock_sysfs_dir: Path, monkeypatch: pytest.MonkeyPatch
):
    """Verifies that manual user selection of Turbo is locked and not overwritten by AC evaluations."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    monkeypatch.setattr("subprocess.run", mock_run)

    ctx = SysfsContext(root_path=mock_sysfs_dir)
    daemon = MechrevoDaemon(ctx=ctx)

    # Initial state on AC is balanced
    monkeypatch.setattr(daemon, "is_ac_online", lambda: True)
    daemon._evaluate_ac_state()
    assert daemon._last_active_profile == "balanced"

    # User explicitly sets turbo mode via IPC
    daemon.handle_ipc_request(json.dumps({"action": "profile", "mode": "turbo"}))
    assert daemon._last_active_profile == "turbo"

    # Now AC disconnects (battery mode) -> office profile is applied
    monkeypatch.setattr(daemon, "is_ac_online", lambda: False)
    daemon._evaluate_ac_state()
    assert daemon._last_active_profile == "office"

    # Now AC reconnects -> It should restore user's previous manual profile (turbo), NOT force balanced!
    monkeypatch.setattr(daemon, "is_ac_online", lambda: True)
    daemon._evaluate_ac_state()
    assert daemon._last_active_profile == "turbo"
