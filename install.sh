#!/usr/bin/env bash
# ==============================================================================
# MECHREVO Linux Control Center (mechrevo-ctl) Production-Grade Installer
# Target Machine: MECHREVO JiguangX (GM6IQ0C / Intel 14th + RTX 4060)
# ==============================================================================

set -e

if [ "$EUID" -ne 0 ]; then
  echo "[!] 请使用 root 权限运行此脚本: sudo ./install.sh"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "[*] 正在安装 mechrevo-ctl 到系统 Python 环境..."

if command -v pip3 >/dev/null 2>&1; then
  pip3 install . --upgrade --break-system-packages 2>/dev/null || pip3 install . --upgrade
elif command -v pip >/dev/null 2>&1; then
  pip install . --upgrade
else
  echo "[!] 未检测到 pip，尝试直接运行 python3 模块安装..."
  python3 -m pip install . --upgrade
fi

echo "[*] 安装 systemd 守护进程服务与睡眠唤醒钩子..."

# 1. 安装 systemd 服务
if [ -d "/etc/systemd/system" ]; then
  DAEMON_BIN="$(command -v mechrevo-ctld || true)"
  if [ -z "$DAEMON_BIN" ]; then
    if [ -f "/usr/local/bin/mechrevo-ctld" ]; then
      DAEMON_BIN="/usr/local/bin/mechrevo-ctld"
    else
      DAEMON_BIN="/usr/bin/mechrevo-ctld"
    fi
  fi
  sed "s|^ExecStart=.*|ExecStart=$DAEMON_BIN|" systemd/mechrevo-ctl.service > /etc/systemd/system/mechrevo-ctl.service
  systemctl daemon-reload
  systemctl enable mechrevo-ctl.service
  systemctl restart mechrevo-ctl.service
  echo "[+] systemd 服务 ($DAEMON_BIN) 已安装并成功启动。"
fi


# 2. 安装睡眠恢复钩子 (解决 S3/s2idle 唤醒后 EC 与功耗限额丢失)
SLEEP_HOOK_DIR=""
if [ -d "/usr/lib/systemd/system-sleep" ]; then
  SLEEP_HOOK_DIR="/usr/lib/systemd/system-sleep"
elif [ -d "/lib/systemd/system-sleep" ]; then
  SLEEP_HOOK_DIR="/lib/systemd/system-sleep"
fi

if [ -n "$SLEEP_HOOK_DIR" ]; then
  cp systemd/mechrevo-ctl-resume.sh "$SLEEP_HOOK_DIR/"
  chmod +x "$SLEEP_HOOK_DIR/mechrevo-ctl-resume.sh"
  echo "[+] 系统睡眠唤醒恢复钩子已就绪: $SLEEP_HOOK_DIR/mechrevo-ctl-resume.sh"
fi

echo "=================================================================="
echo "    MECHREVO 工业级控制台移植工具已部署完成！"
echo "=================================================================="
echo "安全架构特性:"
echo "  - 守护进程作为 root 服务常驻后台，监听内核 Netlink UEvent 硬件事件"
echo "  - 通过 /run/mechrevo-ctl.sock 本地套接字交互，普通用户【无需 sudo】即可调节硬件"
echo "  - 支持合盖休眠唤醒自动恢复上次性能档位"
echo ""
echo "常用指令 (普通用户终端直接输入):"
echo "  mechrevo-ctl status                # 查看当前功耗、电池与温度"
echo "  mechrevo-ctl profile turbo         # 切换至狂暴模式 (CPU 105W, GPU 140W)"
echo "  mechrevo-ctl profile office        # 切换至办公省电模式 (CPU 45W, GPU 60W)"
echo "  mechrevo-ctl battery 80            # 设置电池充至 80% 停止 (长寿养护)"
echo "  mechrevo-ctl keyboard --brightness 80  # 设置键盘背光亮度"
echo "=================================================================="
