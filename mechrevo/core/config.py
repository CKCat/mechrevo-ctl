"""Power profile definitions and preset configuration."""

from dataclasses import dataclass
from typing import Dict


@dataclass(frozen=True)
class PowerProfile:
    """Represents a performance and thermal release profile."""

    name: str
    display_name: str
    cpu_pl1_w: float
    cpu_pl2_w: float
    gpu_tgp_w: float
    platform_profile: str
    fan_policy: str


DEFAULT_PROFILES: Dict[str, PowerProfile] = {
    "office": PowerProfile(
        name="office",
        display_name="办公节能模式",
        cpu_pl1_w=45.0,
        cpu_pl2_w=65.0,
        gpu_tgp_w=60.0,
        platform_profile="low-power",
        fan_policy="quiet",
    ),
    "balanced": PowerProfile(
        name="balanced",
        display_name="日常平衡模式",
        cpu_pl1_w=75.0,
        cpu_pl2_w=90.0,
        gpu_tgp_w=115.0,
        platform_profile="balanced",
        fan_policy="balanced",
    ),
    "turbo": PowerProfile(
        name="turbo",
        display_name="满血狂暴模式",
        cpu_pl1_w=105.0,
        cpu_pl2_w=135.0,
        gpu_tgp_w=140.0,
        platform_profile="performance",
        fan_policy="turbo",
    ),
}


def get_profile_by_name(name: str) -> PowerProfile:
    """Retrieves a power profile by canonical identifier.

    Raises:
        KeyError: If profile name does not exist.
    """
    normalized_name = name.lower().strip()
    if normalized_name not in DEFAULT_PROFILES:
        raise KeyError(f"Unknown performance profile: {name}")
    return DEFAULT_PROFILES[normalized_name]
