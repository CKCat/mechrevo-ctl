#!/usr/bin/env bash
# ==============================================================================
# MECHREVO Linux Control Center (mechrevo-ctl) Uninstaller
# Target Machine: MECHREVO JiguangX (GM6IQ0C / Intel 14th + RTX 4060)
# Purpose: Cleanly and safely removes all installed binaries, services, hooks,
#          udev rules, modprobe configs, and state directories.
# ==============================================================================

set -e

if [ "$EUID" -ne 0 ]; then
  echo "[!] 请使用 root 权限运行此脚本: sudo ./uninstall.sh"
  exit 1
fi

echo "=================================================================="
echo "    MECHREVO 原生控制台 (mechrevo-ctl) 卸载程序启动"
echo "=================================================================="

# 1. 停止并禁用 systemd 守护进程服务
if systemctl is-active --quiet mechrevo-ctl.service 2>/dev/null; then
  echo "[*] 正在停止 mechrevo-ctl.service 守护进程..."
  systemctl stop mechrevo-ctl.service 2>/dev/null || true
fi

if systemctl is-enabled --quiet mechrevo-ctl.service 2>/dev/null; then
  echo "[*] 正在禁用 mechrevo-ctl.service 开机自启..."
  systemctl disable mechrevo-ctl.service 2>/dev/null || true
fi

if [ -f "/etc/systemd/system/mechrevo-ctl.service" ]; then
  echo "[*] 移除 systemd 服务单元文件..."
  rm -f /etc/systemd/system/mechrevo-ctl.service
  systemctl daemon-reload
fi

# 2. 移除系统睡眠与唤醒恢复钩子
echo "[*] 清理系统睡眠恢复钩子..."
rm -f /usr/lib/systemd/system-sleep/mechrevo-ctl-resume.sh
rm -f /lib/systemd/system-sleep/mechrevo-ctl-resume.sh

# 3. 移除原生二进制可执行文件与 GUI 桌面集成
echo "[*] 移除可执行文件 (mechrevo-ctl, mechrevo-ctld & mechrevo-gui)..."
rm -f /usr/local/bin/mechrevo-ctl /usr/local/bin/mechrevo-ctld /usr/local/bin/mechrevo-gui
rm -f /usr/bin/mechrevo-ctl /usr/bin/mechrevo-ctld /usr/bin/mechrevo-gui

echo "[*] 移除桌面快捷方式、自启动配置与图标..."
rm -f /usr/share/applications/mechrevo-control-center.desktop
rm -f /etc/xdg/autostart/mechrevo-control-center.desktop
if [ -n "$SUDO_USER" ] && id "$SUDO_USER" >/dev/null 2>&1; then
  USER_HOME="$(eval echo ~"$SUDO_USER")"
  rm -f "$USER_HOME/.config/autostart/mechrevo-control-center.desktop"
fi
rm -f /usr/share/icons/hicolor/scalable/apps/mechrevo-control-center.svg
rm -f /usr/share/pixmaps/mechrevo-control-center.svg
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database /usr/share/applications 2>/dev/null || true

# 4. 移除 udev 规则并重载规则库
if [ -f "/etc/udev/rules.d/99-mechrevo-powercap.rules" ]; then
  echo "[*] 移除 powercap udev 规则并刷新 udevadm..."
  rm -f /etc/udev/rules.d/99-mechrevo-powercap.rules
  udevadm control --reload-rules 2>/dev/null || true
  udevadm trigger --subsystem-match=powercap 2>/dev/null || true
fi

# 5. 清理内核模块参数配置 (modprobe.d)
if [ -f "/etc/modprobe.d/mechrevo-nvidia-pm.conf" ]; then
  echo "[*] 移除 NVIDIA PCIe RTD3 模块参数: /etc/modprobe.d/mechrevo-nvidia-pm.conf"
  rm -f /etc/modprobe.d/mechrevo-nvidia-pm.conf
fi

if [ -f "/etc/modprobe.d/mechrevo-uniwill.conf" ]; then
  echo "[*] 移除 Uniwill 驱动模块参数: /etc/modprobe.d/mechrevo-uniwill.conf"
  rm -f /etc/modprobe.d/mechrevo-uniwill.conf
fi

if [ -f "/etc/modules-load.d/mechrevo.conf" ]; then
  echo "[*] 移除 RAPL 双域开机模块配置: /etc/modules-load.d/mechrevo.conf"
  rm -f /etc/modules-load.d/mechrevo.conf
fi

# 6. 清理运行时套接字与持久化状态数据
echo "[*] 清理通信套接字与状态持久化目录..."
rm -f /run/mechrevo-ctl.sock
rm -rf /var/lib/mechrevo

# 7. 优雅恢复系统原有桌面电源管理服务 (power-profiles-daemon)
if systemctl list-unit-files power-profiles-daemon.service 2>/dev/null | grep -q "power-profiles-daemon"; then
  echo "[*] 检测到系统原有 power-profiles-daemon.service，正在解除屏蔽并尝试恢复..."
  systemctl unmask power-profiles-daemon.service 2>/dev/null || true
  systemctl start power-profiles-daemon.service 2>/dev/null || true
  echo "[+] 已恢复 power-profiles-daemon.service 默认运行状态。"
fi

echo "=================================================================="
echo "    [+] MECHREVO 控制台及其所有系统集成组件已彻底卸载！"
echo "=================================================================="

