"""Keyboard backlight and RGB driver."""

from typing import Optional

from mechrevo.core.exceptions import HardwareInterfaceError
from mechrevo.core.hal import SysfsContext


class KeyboardDriver:
    """Controls laptop keyboard backlight brightness via sysfs LED interfaces."""

    PRIMARY_LED_PATH = "/sys/class/leds/uniwill::kbd_backlight"
    LEDS_BASE_PATH = "/sys/class/leds"

    def __init__(self, ctx: Optional[SysfsContext] = None) -> None:
        self._ctx = ctx or SysfsContext()

    def _find_led_path(self) -> str:
        """Locates the keyboard backlight LED directory using pattern probing."""
        if self._ctx.exists(self.PRIMARY_LED_PATH):
            return self.PRIMARY_LED_PATH

        resolved_base = self._ctx.resolve(self.LEDS_BASE_PATH)
        if resolved_base.exists():
            for child in sorted(resolved_base.iterdir()):
                name_lower = child.name.lower()
                if "kbd_backlight" in name_lower or "keyboard" in name_lower:
                    if (child / "brightness").exists():
                        return f"{self.LEDS_BASE_PATH}/{child.name}"

        raise HardwareInterfaceError("Keyboard LED sysfs node not found")

    def get_brightness_percent(self) -> int:
        """Reads current brightness level as a percentage (0-100%)."""
        base_path = self._find_led_path()
        brightness_node = f"{base_path}/brightness"
        max_brightness_node = f"{base_path}/max_brightness"

        raw_brightness = int(self._ctx.read_text(brightness_node))
        max_brightness = int(self._ctx.read_text(max_brightness_node))
        if max_brightness <= 0:
            return 0

        return int(round((raw_brightness / float(max_brightness)) * 100))

    def set_brightness_percent(self, percent: int) -> None:
        """Sets keyboard brightness percentage (0-100%)."""
        if not (0 <= percent <= 100):
            raise ValueError(
                f"Brightness percentage must be between 0 and 100 (got {percent})"
            )

        base_path = self._find_led_path()
        brightness_node = f"{base_path}/brightness"
        max_brightness_node = f"{base_path}/max_brightness"

        max_brightness = int(self._ctx.read_text(max_brightness_node))
        raw_target = int(round((percent / 100.0) * max_brightness))
        # 工业级鲁棒性：非零百分比输入绝不能因四舍五入截断为0而导致意外灭灯
        if percent > 0 and raw_target == 0 and max_brightness > 0:
            raw_target = 1

        self._ctx.write_text(brightness_node, f"{raw_target}\n")
