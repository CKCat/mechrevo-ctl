"""Tests for power profiles and configuration definitions."""

import pytest

from mechrevo.core.config import (
    DEFAULT_PROFILES,
    get_profile_by_name,
)


def test_default_profiles_exist():
    """Verifies that the three canonical MECHREVO profiles exist."""
    assert "office" in DEFAULT_PROFILES
    assert "balanced" in DEFAULT_PROFILES
    assert "turbo" in DEFAULT_PROFILES


def test_profile_power_limits():
    """Ensures power limits match the extracted Windows Control Center specs."""
    office = DEFAULT_PROFILES["office"]
    assert office.cpu_pl1_w == 45
    assert office.cpu_pl2_w == 65
    assert office.gpu_tgp_w == 60
    assert office.platform_profile == "low-power"

    balanced = DEFAULT_PROFILES["balanced"]
    assert balanced.cpu_pl1_w == 75
    assert balanced.cpu_pl2_w == 90
    assert balanced.gpu_tgp_w == 115
    assert balanced.platform_profile == "balanced"

    turbo = DEFAULT_PROFILES["turbo"]
    assert turbo.cpu_pl1_w == 105
    assert turbo.cpu_pl2_w == 135
    assert turbo.gpu_tgp_w == 140
    assert turbo.platform_profile == "performance"


def test_get_profile_by_name_success():
    """Tests successful profile retrieval."""
    profile = get_profile_by_name("turbo")
    assert profile.name == "turbo"
    assert profile.cpu_pl1_w == 105


def test_get_profile_by_name_invalid():
    """Tests that querying an unknown profile raises KeyError."""
    with pytest.raises(KeyError, match="Unknown performance profile: invalid_mode"):
        get_profile_by_name("invalid_mode")
