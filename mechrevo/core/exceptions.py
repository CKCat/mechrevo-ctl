"""Exception hierarchies for mechrevo-ctl."""


class MechrevoControlError(Exception):
    """Base exception for all MECHREVO control center errors."""


class HardwareInterfaceError(MechrevoControlError):
    """Raised when a hardware sysfs node or controller interface is not accessible."""


class PermissionDeniedError(MechrevoControlError):
    """Raised when root privileges are required to perform hardware modifications."""


class UnsupportedProfileError(MechrevoControlError):
    """Raised when an unrecognized performance profile is specified."""
