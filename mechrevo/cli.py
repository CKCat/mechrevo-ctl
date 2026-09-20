"""Command-line interface for mechrevo-ctl."""

import argparse
import sys
from typing import List, Optional

from mechrevo.core.battery import BatteryDriver
from mechrevo.core.cpu import IntelRaplDriver
from mechrevo.core.exceptions import MechrevoControlError, PermissionDeniedError
from mechrevo.core.gpu import NvidiaSmiDriver
from mechrevo.core.hal import SysfsContext
from mechrevo.core.keyboard import KeyboardDriver
from mechrevo.core.platform import AcpiPlatformDriver, ProfileManager


def get_default_sysfs_context() -> SysfsContext:
    """Returns the default SysfsContext pointing to the host /sys filesystem."""
    return SysfsContext()


def build_parser() -> argparse.ArgumentParser:
    """Builds CLI argument parser."""
    parser = argparse.ArgumentParser(
        prog="mechrevo-ctl",
        description="MECHREVO Laptop Hardware Control Utility for Linux",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    # 1. status subcommand
    subparsers.add_parser(
        "status", help="Show current hardware status and power limits"
    )

    # 2. profile subcommand
    profile_parser = subparsers.add_parser(
        "profile", help="Switch performance release profile"
    )
    profile_parser.add_argument(
        "mode",
        choices=["office", "balanced", "turbo"],
        help="Target performance profile mode",
    )

    # 3. battery subcommand
    battery_parser = subparsers.add_parser(
        "battery", help="Manage battery charge conservation threshold"
    )
    battery_parser.add_argument(
        "threshold",
        type=int,
        help="Battery charge limit percentage (e.g. 60, 80, 100)",
    )

    # 4. keyboard subcommand
    kbd_parser = subparsers.add_parser("keyboard", help="Control keyboard backlight")
    kbd_parser.add_argument(
        "--brightness",
        type=int,
        required=True,
        help="Brightness percentage (0 to 100)",
    )

    return parser


def cmd_status(ctx: SysfsContext) -> int:
    """Handles `status` subcommand."""
    print("=" * 50)
    print("      MECHREVO Control Center - System Status")
    print("=" * 50)

    # CPU
    try:
        cpu_drv = IntelRaplDriver(ctx=ctx)
        limits = cpu_drv.get_power_limits()
        print(
            f"[*] Intel RAPL (CPU): PL1 = {limits.pl1_w:.0f}W | PL2 = {limits.pl2_w:.0f}W"
        )
    except MechrevoControlError as exc:
        print(f"[-] Intel RAPL (CPU): Unavailable ({exc})")

    # GPU
    gpu_drv = NvidiaSmiDriver()
    if gpu_drv.is_available():
        try:
            info = gpu_drv.get_gpu_info()
            print(f"[*] NVIDIA dGPU: {info.name}")
            print(
                f"    TGP Limit: {info.current_power_limit_w:.0f}W (Max: {info.max_power_limit_w:.0f}W)"
            )
            print(f"    Temperature: {info.temperature_c}°C")
        except MechrevoControlError as exc:
            print(f"[-] NVIDIA dGPU: Query error ({exc})")
    else:
        print("[-] NVIDIA dGPU: Not available or driver not loaded")

    # ACPI Profile
    try:
        platform_drv = AcpiPlatformDriver(ctx=ctx)
        current = platform_drv.get_current_profile()
        print(f"[*] ACPI Platform Profile: {current}")
    except MechrevoControlError:
        print("[-] ACPI Platform Profile: Not supported by current kernel")

    # Battery
    try:
        bat_drv = BatteryDriver(ctx=ctx)
        status = bat_drv.get_battery_status()
        print(
            f"[*] Battery: {status.capacity_percent}% ({status.status}) | Threshold Limit: {status.charge_threshold}%"
        )
    except MechrevoControlError as exc:
        print(f"[-] Battery: Unavailable ({exc})")

    # Keyboard
    try:
        kbd_drv = KeyboardDriver(ctx=ctx)
        brightness = kbd_drv.get_brightness_percent()
        print(f"[*] Keyboard Backlight: {brightness}%")
    except MechrevoControlError as exc:
        print(f"[-] Keyboard Backlight: Unavailable ({exc})")

    print("=" * 50)
    return 0


def send_ipc_command(
    data: dict, socket_path: str = "/run/mechrevo-ctl.sock"
) -> Optional[dict]:
    """Attempts to forward command to running daemon via UNIX domain socket."""
    import json
    import os
    import socket

    if not hasattr(socket, "AF_UNIX"):
        return None
    if not os.path.exists(socket_path):
        return None

    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(2.0)
            client.connect(socket_path)
            client.sendall(json.dumps(data).encode("utf-8"))
            resp = client.recv(4096)
            return json.loads(resp.decode("utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def cmd_profile(ctx: SysfsContext, mode: str) -> int:
    """Handles `profile` subcommand."""
    ipc_resp = send_ipc_command({"action": "profile", "mode": mode})
    if ipc_resp is not None:
        if ipc_resp.get("success"):
            print(f"[+] {ipc_resp.get('message')}")
            return 0
        print(f"[!] Error: {ipc_resp.get('error')}", file=sys.stderr)
        return 1

    cpu_drv = IntelRaplDriver(ctx=ctx)
    gpu_drv = NvidiaSmiDriver()
    platform_drv = AcpiPlatformDriver(ctx=ctx)

    mgr = ProfileManager(
        cpu_driver=cpu_drv, gpu_driver=gpu_drv, platform_driver=platform_drv
    )
    result = mgr.apply_profile(mode)
    print(
        f"[+] Successfully switched to [{result.applied_profile.name}]: {result.applied_profile.display_name}"
    )
    print(
        f"    CPU PL1/PL2: {result.applied_profile.cpu_pl1_w:.0f}W / {result.applied_profile.cpu_pl2_w:.0f}W"
    )
    print(f"    GPU TGP: {result.applied_profile.gpu_tgp_w:.0f}W")
    return 0


def cmd_battery(ctx: SysfsContext, threshold: int) -> int:
    """Handles `battery` subcommand."""
    ipc_resp = send_ipc_command({"action": "battery", "threshold": threshold})
    if ipc_resp is not None:
        if ipc_resp.get("success"):
            print(f"[+] {ipc_resp.get('message')}")
            return 0
        print(f"[!] Error: {ipc_resp.get('error')}", file=sys.stderr)
        return 1

    bat_drv = BatteryDriver(ctx=ctx)
    bat_drv.set_charge_threshold(threshold)
    print(f"[+] Battery charge threshold set to {threshold}%")
    return 0


def cmd_keyboard(ctx: SysfsContext, brightness: int) -> int:
    """Handles `keyboard` subcommand."""
    ipc_resp = send_ipc_command({"action": "keyboard", "brightness": brightness})
    if ipc_resp is not None:
        if ipc_resp.get("success"):
            print(f"[+] {ipc_resp.get('message')}")
            return 0
        print(f"[!] Error: {ipc_resp.get('error')}", file=sys.stderr)
        return 1

    kbd_drv = KeyboardDriver(ctx=ctx)
    kbd_drv.set_brightness_percent(brightness)
    print(f"[+] Keyboard brightness set to {brightness}%")
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    """Entry point for CLI command dispatch."""
    parser = build_parser()
    args = parser.parse_args(argv)

    ctx = get_default_sysfs_context()

    try:
        if args.command == "status":
            return cmd_status(ctx)
        elif args.command == "profile":
            return cmd_profile(ctx, args.mode)
        elif args.command == "battery":
            return cmd_battery(ctx, args.threshold)
        elif args.command == "keyboard":
            return cmd_keyboard(ctx, args.brightness)
    except PermissionDeniedError as exc:
        print(f"[!] Error: {exc}", file=sys.stderr)
        print("[!] Hint: Please run this command with 'sudo'.", file=sys.stderr)
        return 13
    except MechrevoControlError as exc:
        print(f"[!] Error: {exc}", file=sys.stderr)
        return 1
    except ValueError as exc:
        print(f"[!] Invalid parameter: {exc}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    sys.exit(main())
