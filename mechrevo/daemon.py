"""Background daemon for hardware monitoring, uevents, and IPC."""

import json
import os
import select
import signal
import socket
import sys
from pathlib import Path
from typing import Optional

from mechrevo.core.battery import BatteryDriver
from mechrevo.core.cpu import IntelRaplDriver
from mechrevo.core.gpu import NvidiaSmiDriver
from mechrevo.core.hal import SysfsContext
from mechrevo.core.keyboard import KeyboardDriver
from mechrevo.core.platform import AcpiPlatformDriver, ProfileManager


class MechrevoDaemon:
    """Manages kernel uevent listening, AC state transitions, and IPC commands."""

    AC_ONLINE_NODE = "/sys/class/power_supply/AC/online"
    DEFAULT_SOCKET_PATH = "/run/mechrevo-ctl.sock"

    def __init__(
        self,
        ctx: Optional[SysfsContext] = None,
        socket_path: Optional[str] = None,
    ) -> None:
        self._ctx = ctx or SysfsContext()
        self._socket_path = socket_path or self.DEFAULT_SOCKET_PATH
        self._profile_mgr = ProfileManager(
            cpu_driver=IntelRaplDriver(ctx=self._ctx),
            gpu_driver=NvidiaSmiDriver(),
            platform_driver=AcpiPlatformDriver(ctx=self._ctx),
        )
        self._battery_drv = BatteryDriver(ctx=self._ctx)
        self._keyboard_drv = KeyboardDriver(ctx=self._ctx)
        self._running = False
        self._last_ac_state: Optional[bool] = None
        self._manual_profile_override: Optional[str] = None
        self._last_active_profile = "balanced"

    def is_ac_online(self) -> bool:
        """Returns True if AC adapter is plugged in."""
        if self._ctx.exists(self.AC_ONLINE_NODE):
            return self._ctx.read_text(self.AC_ONLINE_NODE).strip() == "1"
        try:
            status = self._battery_drv.get_battery_status()
            return status.status.lower() in ("charging", "full")
        except Exception:
            return True

    def handle_ipc_request(self, payload: str) -> str:
        """Processes an incoming JSON request and returns a JSON response."""
        try:
            data = json.loads(payload)
            action = data.get("action")

            if action == "profile":
                mode = data.get("mode")
                if not mode:
                    raise ValueError("Missing 'mode' argument")
                result = self._profile_mgr.apply_profile(mode)
                self._manual_profile_override = mode
                self._last_active_profile = mode
                return json.dumps({"success": True, "message": result.message})

            elif action == "battery":
                threshold = int(data.get("threshold"))
                self._battery_drv.set_charge_threshold(threshold)
                return json.dumps(
                    {
                        "success": True,
                        "message": f"Battery charge threshold set to {threshold}%",
                    }
                )

            elif action == "keyboard":
                brightness = int(data.get("brightness"))
                self._keyboard_drv.set_brightness_percent(brightness)
                return json.dumps(
                    {
                        "success": True,
                        "message": f"Keyboard brightness set to {brightness}%",
                    }
                )

            elif action == "reload_profile":
                # Used by system-sleep resume hook to re-arm settings after S3/s2idle wakeup
                target = self._manual_profile_override or self._last_active_profile
                result = self._profile_mgr.apply_profile(target)
                self._last_active_profile = target
                return json.dumps(
                    {"success": True, "message": f"Re-armed profile: {target}"}
                )

            else:
                return json.dumps(
                    {"success": False, "error": f"Unknown action: {action}"}
                )

        except Exception as exc:
            return json.dumps({"success": False, "error": str(exc)})

    def _setup_uevent_socket(self) -> Optional[socket.socket]:
        """Creates a Netlink KObject UEvent socket on Linux for zero-polling power event wakeups."""
        if hasattr(socket, "AF_NETLINK"):
            try:
                # 15 = NETLINK_KOBJECT_UEVENT
                sock = socket.socket(socket.AF_NETLINK, socket.SOCK_RAW, 15)
                sock.bind((os.getpid(), 1))  # 1 = multicast group
                sock.setblocking(False)
                return sock
            except OSError:
                return None
        return None

    def _setup_unix_socket(self) -> Optional[socket.socket]:
        """Creates the local UNIX domain socket for non-root CLI communication."""
        if hasattr(socket, "AF_UNIX"):
            try:
                sock_path = Path(self._socket_path)
                # Ensure dangling or stale socket inode is cleanly removed
                try:
                    sock_path.unlink(missing_ok=True)
                except OSError:
                    pass
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sock.bind(self._socket_path)
                # Allow any user on the system to write commands to the daemon without sudo
                try:
                    os.chmod(self._socket_path, 0o666)
                except OSError:
                    pass
                sock.listen(5)
                sock.setblocking(False)
                return sock
            except OSError as exc:
                print(
                    f"[!] Warning: Failed to bind UNIX socket {self._socket_path}: {exc}",
                    file=sys.stderr,
                )
                return None
        return None

    def run(self) -> None:
        """Starts the event-driven mainloop."""
        self._running = True

        def _handle_signal(sig, frame):
            self._running = False

        signal.signal(signal.SIGINT, _handle_signal)
        signal.signal(signal.SIGTERM, _handle_signal)

        uevent_sock = self._setup_uevent_socket()
        unix_sock = self._setup_unix_socket()

        print("[*] MECHREVO control daemon running.")
        if uevent_sock:
            print(
                "[+] Kernel Netlink UEvent listener: ENABLED (zero-polling energy saving)"
            )
        if unix_sock:
            print(f"[+] IPC UNIX socket: {self._socket_path}")

        # Initial power state evaluation
        self._evaluate_ac_state()

        while self._running:
            readers = []
            if uevent_sock:
                readers.append(uevent_sock)
            if unix_sock:
                readers.append(unix_sock)

            try:
                # Sleep in select() until a hardware uevent arrives or an IPC client connects
                readable, _, _ = select.select(readers, [], [], 60.0)

                for r in readable:
                    if r is uevent_sock:
                        try:
                            data = uevent_sock.recv(4096)
                            if b"power_supply" in data:
                                self._evaluate_ac_state()
                        except OSError:
                            pass

                    elif r is unix_sock:
                        try:
                            client, _ = unix_sock.accept()
                            with client:
                                # 核心安全加固：给客户端连接注入 1.0 秒强制超时，杜绝非特权客户端挂死 root 守护进程 (Local DoS)
                                client.settimeout(1.0)
                                req_bytes = client.recv(4096)
                                if req_bytes:
                                    resp_str = self.handle_ipc_request(
                                        req_bytes.decode("utf-8", errors="replace")
                                    )
                                    client.sendall(resp_str.encode("utf-8"))
                        except (OSError, socket.timeout):
                            pass

                # If no uevent socket (fallback platform), only run evaluation on select timeout
                if not uevent_sock and not readable:
                    self._evaluate_ac_state()

            except OSError:
                if not self._running:
                    break

        if unix_sock:
            unix_sock.close()
            try:
                Path(self._socket_path).unlink(missing_ok=True)
            except OSError:
                pass
        if uevent_sock:
            uevent_sock.close()

        print("[*] MECHREVO control daemon stopped.")

    def _evaluate_ac_state(self) -> None:
        """Checks AC state and triggers profile adjustments on transition."""
        try:
            ac_online = self.is_ac_online()
            if self._last_ac_state is None or self._last_ac_state != ac_online:
                self._last_ac_state = ac_online
                if ac_online:
                    # 优先恢复用户曾手动选择的档位，若未选择则默认切回 balanced
                    target = self._manual_profile_override or "balanced"
                    print(f"[*] AC adapter connected -> Applying {target} profile")
                    self._profile_mgr.apply_profile(target)
                    self._last_active_profile = target
                else:
                    print(
                        "[*] On battery power -> Applying office power saving profile"
                    )
                    self._profile_mgr.apply_profile("office")
                    self._last_active_profile = "office"
        except Exception as exc:
            print(f"[!] Warning evaluating AC state: {exc}", file=sys.stderr)


def main() -> None:
    """Daemon process entry point."""
    daemon = MechrevoDaemon()
    daemon.run()


if __name__ == "__main__":
    main()
