"""NVIDIA GPU power management driver via nvidia-smi."""

import subprocess
from dataclasses import dataclass
from typing import Optional

from mechrevo.core.exceptions import (
    HardwareInterfaceError,
    PermissionDeniedError,
)


@dataclass(frozen=True)
class GpuInfo:
    """NVIDIA GPU status and power profile details."""

    name: str
    current_power_limit_w: float
    max_power_limit_w: float
    temperature_c: int


class NvidiaSmiDriver:
    """Controls NVIDIA discrete GPU power limit (TGP) and queries status."""

    MIN_TGP_W = 30.0
    MAX_TGP_W = 175.0

    def __init__(self, binary_path: Optional[str] = None) -> None:
        self._binary_path = binary_path or "nvidia-smi"

    def is_available(self) -> bool:
        """Checks if nvidia-smi command is present and responsive."""
        try:
            result = subprocess.run(
                [self._binary_path, "-L"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                text=True,
            )
            return result.returncode == 0
        except (FileNotFoundError, OSError):
            return False

    def get_gpu_info(self) -> GpuInfo:
        """Queries GPU name, current power limit, max power limit, and temperature."""
        try:
            result = subprocess.run(
                [
                    self._binary_path,
                    "--query-gpu=name,power.limit,power.max_limit,temperature.gpu",
                    "--format=csv,noheader,nounits",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
        except FileNotFoundError as exc:
            raise HardwareInterfaceError(
                "NVIDIA GPU management tool not available (nvidia-smi missing)"
            ) from exc

        if result.returncode != 0:
            raise HardwareInterfaceError(
                f"NVIDIA GPU management tool not available: {result.stderr.strip()}"
            )

        output_line = result.stdout.strip().splitlines()[0]
        parts = [part.strip() for part in output_line.split(",")]
        if len(parts) < 4:
            raise HardwareInterfaceError(
                f"Unexpected nvidia-smi output format: {output_line}"
            )

        def _parse_float(val: str, default: float = 0.0) -> float:
            cleaned = val.upper().replace("W", "").strip()
            if cleaned in ("[N/A]", "N/A", "NONE", ""):
                return default
            try:
                return float(cleaned)
            except ValueError:
                return default

        def _parse_int(val: str, default: int = 0) -> int:
            cleaned = val.upper().replace("C", "").strip()
            if cleaned in ("[N/A]", "N/A", "NONE", ""):
                return default
            try:
                return int(cleaned)
            except ValueError:
                return default

        name = parts[0]
        current_pl = _parse_float(parts[1], default=0.0)
        max_pl = _parse_float(parts[2], default=140.0)
        temp = _parse_int(parts[3], default=0)

        return GpuInfo(
            name=name,
            current_power_limit_w=current_pl,
            max_power_limit_w=max_pl,
            temperature_c=temp,
        )

    def ensure_persistence_mode(self) -> None:
        """Enables NVIDIA persistence mode to prevent driver unloading and limit reset in hybrid graphics."""
        try:
            subprocess.run(
                [self._binary_path, "-pm", "1"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
        except (FileNotFoundError, OSError):
            pass

    def set_power_limit(self, tgp_w: float, enable_persistence: bool = True) -> None:
        """Sets NVIDIA discrete GPU Total Graphics Power (TGP) limit in Watts.

        Args:
            tgp_w: Target graphics power in Watts.
            enable_persistence: If True, locks driver in memory with -pm 1.
                Should be False on battery/office mode to allow PCIe RTD3 D3cold deep sleep.
        """
        if not (self.MIN_TGP_W <= tgp_w <= self.MAX_TGP_W):
            raise ValueError(
                f"GPU power limit must be between {self.MIN_TGP_W}W and {self.MAX_TGP_W}W (got {tgp_w}W)"
            )

        if enable_persistence:
            self.ensure_persistence_mode()

        try:
            result = subprocess.run(
                [self._binary_path, "-i", "0", "-pl", str(int(tgp_w))],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
        except FileNotFoundError as exc:
            raise HardwareInterfaceError("nvidia-smi not found") from exc

        if result.returncode != 0:
            err_msg = result.stderr.strip()
            if "Insufficient Permissions" in err_msg or "root" in err_msg.lower():
                raise PermissionDeniedError(
                    "Root privileges required to adjust GPU power limit"
                )
            raise HardwareInterfaceError(f"Failed to set GPU power limit: {err_msg}")
