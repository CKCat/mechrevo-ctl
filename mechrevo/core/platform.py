"""ACPI platform_profile driver and integrated ProfileManager."""

from dataclasses import dataclass
from typing import List, Optional

from mechrevo.core.config import PowerProfile, get_profile_by_name
from mechrevo.core.cpu import IntelRaplDriver
from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.gpu import NvidiaSmiDriver
from mechrevo.core.hal import SysfsContext


class AcpiPlatformDriver:
    """Interfaces with Linux ACPI platform_profile sysfs nodes."""

    PROFILE_NODE = "/sys/firmware/acpi/platform_profile"
    CHOICES_NODE = "/sys/firmware/acpi/platform_profile_choices"

    def __init__(self, ctx: Optional[SysfsContext] = None) -> None:
        self._ctx = ctx or SysfsContext()

    def is_available(self) -> bool:
        """Checks if the kernel ACPI platform profile driver is active."""
        return self._ctx.exists(self.PROFILE_NODE)

    def get_current_profile(self) -> str:
        """Reads current active ACPI platform profile."""
        if not self._ctx.exists(self.PROFILE_NODE):
            raise HardwareInterfaceError("ACPI platform_profile node not found")
        return self._ctx.read_text(self.PROFILE_NODE)

    def get_available_profiles(self) -> List[str]:
        """Lists supported ACPI profiles provided by the firmware/kernel."""
        if not self._ctx.exists(self.CHOICES_NODE):
            return ["low-power", "balanced", "performance"]
        raw_choices = self._ctx.read_text(self.CHOICES_NODE)
        return [choice for choice in raw_choices.split() if choice]

    def set_profile(self, profile: str) -> None:
        """Sets new ACPI platform profile."""
        available = self.get_available_profiles()
        if profile not in available:
            raise ValueError(
                f"Unsupported ACPI profile '{profile}'. Supported: {', '.join(available)}"
            )

        if not self._ctx.exists(self.PROFILE_NODE):
            raise HardwareInterfaceError("ACPI platform_profile node not found")

        self._ctx.write_text(self.PROFILE_NODE, f"{profile}\n")


@dataclass(frozen=True)
class ProfileApplyResult:
    """Result summary of a profile switch action."""

    is_success: bool
    applied_profile: PowerProfile
    message: str


class ProfileManager:
    """Coordinates multi-subsystem power profiles (ACPI, CPU PL1/PL2, GPU TGP)."""

    def __init__(
        self,
        cpu_driver: Optional[IntelRaplDriver] = None,
        gpu_driver: Optional[NvidiaSmiDriver] = None,
        platform_driver: Optional[AcpiPlatformDriver] = None,
    ) -> None:
        self._cpu = cpu_driver or IntelRaplDriver()
        self._gpu = gpu_driver or NvidiaSmiDriver()
        self._platform = platform_driver or AcpiPlatformDriver()

    def apply_profile(self, profile_name: str) -> ProfileApplyResult:
        """Applies a named profile across ACPI, CPU, and GPU."""
        profile = get_profile_by_name(profile_name)

        # 1. Apply ACPI platform profile if supported
        if self._platform.is_available():
            available = self._platform.get_available_profiles()
            target_acpi = profile.platform_profile
            # Map low-power to quiet if needed
            if (
                target_acpi not in available
                and "quiet" in available
                and target_acpi == "low-power"
            ):
                target_acpi = "quiet"
            if target_acpi in available:
                self._platform.set_profile(target_acpi)

        # 2. Apply CPU PL1 and PL2 limits
        self._cpu.set_power_limits(pl1_w=profile.cpu_pl1_w, pl2_w=profile.cpu_pl2_w)

        # 3. Apply GPU power limits if NVIDIA GPU is present
        if self._gpu.is_available():
            # 功耗治理：office模式下严禁开启持久化模式，保护独立显卡进入 PCIe RTD3 D3cold 深度睡眠
            enable_pm = profile.name.lower() != "office"
            try:
                self._gpu.set_power_limit(
                    profile.gpu_tgp_w, enable_persistence=enable_pm
                )
            except Exception:
                # 容错机制：从 S3/s2idle 唤醒时独显 PCIe 链路可能存在延迟，不应阻断 CPU 与 ACPI 的就绪
                pass

        return ProfileApplyResult(
            is_success=True,
            applied_profile=profile,
            message=f"Successfully applied profile '{profile.name}' ({profile.display_name})",
        )
