"""Battery charging threshold and health status driver."""

from dataclasses import dataclass
from typing import Optional

from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.hal import SysfsContext


@dataclass(frozen=True)
class BatteryStatus:
    """Battery state and charging threshold limit information."""

    capacity_percent: int
    status: str
    charge_threshold: int


class BatteryDriver:
    """Controls battery charging thresholds to extend battery lifespan."""

    POWER_SUPPLY_BASE = "/sys/class/power_supply"
    MIN_THRESHOLD = 20
    MAX_THRESHOLD = 100

    def __init__(self, ctx: Optional[SysfsContext] = None) -> None:
        self._ctx = ctx or SysfsContext()

    def _find_battery_path(self) -> str:
        """Dynamically locates the active ACPI battery device node (e.g. BAT0, BAT1, BATT)."""
        resolved_base = self._ctx.resolve(self.POWER_SUPPLY_BASE)
        if resolved_base.exists():
            # Check for BAT0 first, then any directory starting with BAT
            candidates = sorted(resolved_base.iterdir())
            for child in candidates:
                if child.is_dir() and child.name.upper().startswith("BAT"):
                    if (child / "capacity").exists():
                        return f"{self.POWER_SUPPLY_BASE}/{child.name}"
        raise HardwareInterfaceError("Battery sysfs node not found")

    THRESHOLD_CANDIDATES = (
        "charge_control_end_threshold",
        "charge_stop_threshold",
        "charge_control_threshold",
    )

    def _find_threshold_node(self, bat_path: str) -> Optional[str]:
        """Finds the active charge threshold sysfs node name if supported by ACPI/kernel driver."""
        for candidate in self.THRESHOLD_CANDIDATES:
            node = f"{bat_path}/{candidate}"
            if self._ctx.exists(node):
                return node
        return None

    def get_battery_status(self) -> BatteryStatus:
        """Reads current battery level, charging state, and end threshold."""
        bat_path = self._find_battery_path()
        capacity_node = f"{bat_path}/capacity"
        status_node = f"{bat_path}/status"

        capacity = int(self._ctx.read_text(capacity_node))
        status = self._ctx.read_text(status_node)

        threshold = 100
        threshold_node = self._find_threshold_node(bat_path)
        if threshold_node and self._ctx.exists(threshold_node):
            threshold = int(self._ctx.read_text(threshold_node))

        return BatteryStatus(
            capacity_percent=capacity,
            status=status,
            charge_threshold=threshold,
        )

    def set_charge_threshold(self, threshold_percent: int) -> None:
        """Sets the upper charging threshold percentage (e.g. 60, 80, 100)."""
        if not (self.MIN_THRESHOLD <= threshold_percent <= self.MAX_THRESHOLD):
            raise ValueError(
                f"Battery threshold must be between {self.MIN_THRESHOLD} and {self.MAX_THRESHOLD} (got {threshold_percent})"
            )

        bat_path = self._find_battery_path()
        threshold_node = self._find_threshold_node(bat_path)
        if not threshold_node:
            threshold_node = f"{bat_path}/charge_control_end_threshold"

        self._ctx.write_text(threshold_node, f"{threshold_percent}\n")
