"""Intel RAPL CPU power limits driver."""

from dataclasses import dataclass
from typing import Optional

from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.hal import SysfsContext


@dataclass(frozen=True)
class CpuPowerLimits:
    """CPU Long-term (PL1) and Short-term (PL2) power limit values in Watts."""

    pl1_w: float
    pl2_w: float


class IntelRaplDriver:
    """Controls Intel processor RAPL (Running Average Power Limit) parameters."""

    RAPL_BASE_PATH = "/sys/class/powercap/intel-rapl/intel-rapl:0"
    PL1_NODE = f"{RAPL_BASE_PATH}/constraint_0_power_limit_uw"
    PL2_NODE = f"{RAPL_BASE_PATH}/constraint_1_power_limit_uw"

    MIN_PL_W = 15.0
    MAX_PL1_W = 150.0
    MAX_PL2_W = 200.0

    def __init__(self, ctx: Optional[SysfsContext] = None) -> None:
        self._ctx = ctx or SysfsContext()

    def _resolve_nodes(self) -> tuple[str, str]:
        """Resolves PL1 and PL2 sysfs node paths by inspecting constraint names dynamically."""
        resolved_base = self._ctx.resolve(self.RAPL_BASE_PATH)
        pl1_path = self.PL1_NODE
        pl2_path = self.PL2_NODE

        if resolved_base.exists():
            for child in resolved_base.iterdir():
                if child.name.startswith("constraint_") and child.name.endswith(
                    "_name"
                ):
                    try:
                        name = child.read_text(encoding="utf-8").strip().lower()
                        idx = child.name.split("_")[1]
                        limit_node = (
                            f"{self.RAPL_BASE_PATH}/constraint_{idx}_power_limit_uw"
                        )
                        if name == "long_term":
                            pl1_path = limit_node
                        elif name == "short_term":
                            pl2_path = limit_node
                    except OSError:
                        pass
        return pl1_path, pl2_path

    def get_power_limits(self) -> CpuPowerLimits:
        """Reads current PL1 and PL2 values in Watts."""
        pl1_node, pl2_node = self._resolve_nodes()
        if not self._ctx.exists(pl1_node) or not self._ctx.exists(pl2_node):
            raise HardwareInterfaceError("Intel RAPL sysfs node not found")

        raw_pl1 = int(self._ctx.read_text(pl1_node))
        raw_pl2 = int(self._ctx.read_text(pl2_node))

        return CpuPowerLimits(
            pl1_w=raw_pl1 / 1_000_000.0,
            pl2_w=raw_pl2 / 1_000_000.0,
        )

    def set_power_limits(self, pl1_w: float, pl2_w: float) -> None:
        """Sets new PL1 and PL2 limits in Watts.

        Raises:
            ValueError: If limits are outside safe thermal hardware ranges.
            HardwareInterfaceError: If RAPL sysfs nodes are unavailable.
        """
        if not (self.MIN_PL_W <= pl1_w <= self.MAX_PL1_W):
            raise ValueError(
                f"PL1 must be between {self.MIN_PL_W}W and {self.MAX_PL1_W}W (got {pl1_w}W)"
            )
        if not (self.MIN_PL_W <= pl2_w <= self.MAX_PL2_W):
            raise ValueError(
                f"PL2 must be between {self.MIN_PL_W}W and {self.MAX_PL2_W}W (got {pl2_w}W)"
            )
        if pl1_w > pl2_w:
            raise ValueError(f"PL1 ({pl1_w}W) cannot exceed PL2 ({pl2_w}W)")

        pl1_node, pl2_node = self._resolve_nodes()
        if not self._ctx.exists(pl1_node) or not self._ctx.exists(pl2_node):
            raise HardwareInterfaceError("Intel RAPL sysfs node not found")

        pl1_uw = int(pl1_w * 1_000_000)
        pl2_uw = int(pl2_w * 1_000_000)

        # 核心时序保护：升档先抬 PL2，降档先压 PL1，数学证明保证瞬态不倒挂
        current_limits = self.get_power_limits()
        if pl2_w >= current_limits.pl2_w:
            # 升档时序：先升高 PL2，再提升 PL1
            self._ctx.write_text(pl2_node, f"{pl2_uw}\n")
            self._ctx.write_text(pl1_node, f"{pl1_uw}\n")
        else:
            # 降档时序：先压低 PL1，再降低 PL2
            self._ctx.write_text(pl1_node, f"{pl1_uw}\n")
            self._ctx.write_text(pl2_node, f"{pl2_uw}\n")
