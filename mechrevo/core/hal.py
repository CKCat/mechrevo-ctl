"""Hardware Abstraction Layer (HAL) and SysfsContext."""

import os
from pathlib import Path
from typing import Optional

from mechrevo.core.exceptions import (
    HardwareInterfaceError,
    PermissionDeniedError,
)


class SysfsContext:
    """Manages file interactions with the Linux /sys filesystem.

    Supports path redirection (via root_path) for safe automated testing and mocking.
    """

    def __init__(self, root_path: Optional[Path] = None) -> None:
        self._root_path = root_path

    @property
    def root_path(self) -> Optional[Path]:
        """Returns the base mock root path, if configured."""
        return self._root_path

    def resolve(self, path_str: str) -> Path:
        """Resolves an absolute sysfs path against the configured root directory."""
        clean_path = path_str.lstrip("/\\")
        if self._root_path is not None:
            if os.name == "nt":
                clean_path = clean_path.replace(":", "_")
            return self._root_path / clean_path
        return Path("/" + clean_path)

    def read_text(self, path_str: str) -> str:
        """Reads stripped UTF-8 text from a sysfs path."""
        target = self.resolve(path_str)
        if not target.exists():
            raise HardwareInterfaceError(f"Sysfs node not found: {path_str}")
        try:
            return target.read_text(encoding="utf-8").strip()
        except PermissionError as exc:
            raise PermissionDeniedError(
                f"Permission denied reading {path_str}"
            ) from exc
        except OSError as exc:
            raise HardwareInterfaceError(
                f"Failed to read sysfs node {path_str}: {exc}"
            ) from exc

    def write_text(self, path_str: str, value: str) -> None:
        """Writes text content to a sysfs path."""
        target = self.resolve(path_str)
        if not target.exists():
            raise HardwareInterfaceError(f"Sysfs node not found: {path_str}")
        try:
            target.write_text(value, encoding="utf-8")
        except PermissionError as exc:
            raise PermissionDeniedError(
                f"Permission denied writing to {path_str}. Root privileges required."
            ) from exc
        except OSError as exc:
            raise HardwareInterfaceError(
                f"Failed to write to sysfs node {path_str}: {exc}"
            ) from exc

    def exists(self, path_str: str) -> bool:
        """Checks if a sysfs node exists."""
        return self.resolve(path_str).exists()
