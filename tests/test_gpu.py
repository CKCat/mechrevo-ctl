"""Tests for NVIDIA GPU power limit controller."""

from unittest.mock import MagicMock

import pytest

from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.gpu import NvidiaSmiDriver


def test_get_gpu_info_success(monkeypatch: pytest.MonkeyPatch):
    """Verifies parsing nvidia-smi query output."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    mock_run.return_value.stdout = (
        "NVIDIA GeForce RTX 4060 Laptop GPU, 115.00 W, 140.00 W, 58 C\n"
    )

    monkeypatch.setattr("subprocess.run", mock_run)

    driver = NvidiaSmiDriver()
    info = driver.get_gpu_info()

    assert info.name == "NVIDIA GeForce RTX 4060 Laptop GPU"
    assert info.current_power_limit_w == 115.0
    assert info.max_power_limit_w == 140.0
    assert info.temperature_c == 58


def test_get_gpu_info_with_na_power_limit(monkeypatch: pytest.MonkeyPatch):
    """Verifies handling of [N/A] power limit values in laptop GPUs."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    mock_run.return_value.stdout = (
        "NVIDIA GeForce RTX 4060 Laptop GPU, [N/A], 140.00 W, 52 C\n"
    )

    monkeypatch.setattr("subprocess.run", mock_run)

    driver = NvidiaSmiDriver()
    info = driver.get_gpu_info()

    assert info.name == "NVIDIA GeForce RTX 4060 Laptop GPU"
    assert info.current_power_limit_w == 0.0
    assert info.max_power_limit_w == 140.0
    assert info.temperature_c == 52


def test_set_gpu_power_limit_success(monkeypatch: pytest.MonkeyPatch):
    """Verifies invoking nvidia-smi -pl <watts>."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    mock_run.return_value.stdout = "Power limit for GPU 0 set to 140.00 W\n"

    monkeypatch.setattr("subprocess.run", mock_run)

    driver = NvidiaSmiDriver()
    driver.set_power_limit(140)

    assert mock_run.call_count == 2
    # 第一次调用确保持久化模式开启
    first_call_cmd = mock_run.call_args_list[0][0][0]
    assert "-pm" in first_call_cmd
    # 第二次调用设置具体功耗上限
    second_call_cmd = mock_run.call_args_list[1][0][0]
    assert "-pl" in second_call_cmd
    assert "140" in second_call_cmd


def test_set_gpu_power_limit_validation():
    """Verifies that invalid TGP values raise ValueError."""
    driver = NvidiaSmiDriver()
    with pytest.raises(ValueError, match="GPU power limit must be between"):
        driver.set_power_limit(20)

    with pytest.raises(ValueError, match="GPU power limit must be between"):
        driver.set_power_limit(200)


def test_gpu_unavailable(monkeypatch: pytest.MonkeyPatch):
    """Verifies graceful handling when nvidia-smi is not found."""

    def mock_raise(*args, **kwargs):
        raise FileNotFoundError("nvidia-smi not found")

    monkeypatch.setattr("subprocess.run", mock_raise)

    driver = NvidiaSmiDriver()
    assert driver.is_available() is False

    with pytest.raises(
        HardwareInterfaceError, match="NVIDIA GPU management tool not available"
    ):
        driver.get_gpu_info()


def test_ensure_persistence_mode(monkeypatch: pytest.MonkeyPatch):
    """Verifies that driver calls nvidia-smi -pm 1 to keep driver loaded in hybrid mode."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    monkeypatch.setattr("subprocess.run", mock_run)

    driver = NvidiaSmiDriver()
    driver.ensure_persistence_mode()

    called_cmd = mock_run.call_args[0][0]
    assert "nvidia-smi" in called_cmd
    assert "-pm" in called_cmd
    assert "1" in called_cmd


def test_set_gpu_power_limit_d3cold_safety(monkeypatch: pytest.MonkeyPatch):
    """Verifies that persistence mode is skipped when persistence=False to protect RTD3 D3cold."""
    mock_run = MagicMock()
    mock_run.return_value.returncode = 0
    monkeypatch.setattr("subprocess.run", mock_run)

    driver = NvidiaSmiDriver()
    driver.set_power_limit(60, enable_persistence=False)

    # Should only call nvidia-smi -pl, without calling -pm 1
    assert mock_run.call_count == 1
    cmd = mock_run.call_args_list[0][0][0]
    assert "-pl" in cmd
    assert "60" in cmd
    assert "-pm" not in cmd
