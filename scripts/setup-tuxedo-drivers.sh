#!/usr/bin/env bash
# ==============================================================================
# MECHREVO GM6 (极光X) DKMS 驱动安装脚本 (tuxedo-drivers)
# 作用: 为 Linux Mint 22 (Ubuntu 24.04 noble) 安装同方/Uniwill 模具专用 DKMS 驱动，
#       激活电池养护上限 (battery 80)、键盘背光调节 (keyboard) 与极限强冷 (fan-boost)
# ==============================================================================

set -e

if [ "$EUID" -ne 0 ]; then
  echo "[!] 请使用 root 权限运行此脚本: sudo ./scripts/setup-tuxedo-drivers.sh"
  exit 1
fi

echo "=================================================================="
echo "    正在为机械革命 极光X 配置 Uniwill / Tuxedo DKMS 原厂生态驱动"
echo "=================================================================="

# 1. 导入 TUXEDO 官方 GPG 证书并解密为二进制密钥环 (规避 ASCII armored NO_PUBKEY 错误)
echo "[*] 1/5 正在导入 TUXEDO 官方 APT 签名密钥..."
mkdir -p /etc/apt/keyrings
rm -f /etc/apt/trusted.gpg.d/tuxedo-computers.gpg
wget -qO- https://deb.tuxedocomputers.com/0x54840598.pub.asc | gpg --dearmor > /etc/apt/keyrings/tuxedo-computers.gpg
chmod 644 /etc/apt/keyrings/tuxedo-computers.gpg

# 2. 写入软件源配置 (绑定 signed-by 密钥环，适配 Linux Mint 22 / Ubuntu 24.04 noble)
echo "[*] 2/5 正在配置 APT 软件源 (/etc/apt/sources.list.d/tuxedo-computers.list)..."
echo "deb [signed-by=/etc/apt/keyrings/tuxedo-computers.gpg] https://deb.tuxedocomputers.com/ubuntu noble main" > /etc/apt/sources.list.d/tuxedo-computers.list

# 3. 更新包缓存并安装 tuxedo-drivers
echo "[*] 3/5 正在更新软件源并安装 tuxedo-drivers DKMS 模块..."
apt-get update
apt-get install -y tuxedo-drivers

# 3.5 解除 TUXEDO 驱动对非 TUXEDO 品牌 DMI (MECHREVO 极光X) 的白名单拦截并补齐 fan_boost 硬件接口
echo "[*] 正在解除 TUXEDO DKMS 驱动白名单拦截并激活 fan_boost 硬件节点..."
SRC_DIR=$(ls -d /usr/src/tuxedo-drivers-* 2>/dev/null | sort -V | tail -n 1)
if [ -n "$SRC_DIR" ] && [ -d "$SRC_DIR" ]; then
  TUXEDO_VER=$(basename "$SRC_DIR" | sed 's/tuxedo-drivers-//')
  COMPAT_FILE="$SRC_DIR/tuxedo_compatibility_check/tuxedo_compatibility_check.c"
  TK_FILE="$SRC_DIR/tuxedo_keyboard.c"
  UWK_FILE="$SRC_DIR/uniwill_keyboard.h"

  if [ -f "$COMPAT_FILE" ]; then
    if [ -f "${COMPAT_FILE}.bak" ]; then
      cp "${COMPAT_FILE}.bak" "$COMPAT_FILE"
    else
      cp "$COMPAT_FILE" "${COMPAT_FILE}.bak"
    fi
    python3 -c "
import re, sys
p = sys.argv[1]
c = open(p).read()
c = re.sub(r'bool\s+tuxedo_is_compatible\s*\(\s*void\s*\)\s*\{[\s\S]*?\n\}\s*\nEXPORT_SYMBOL', 'bool tuxedo_is_compatible(void) {\n\treturn true;\n}\nEXPORT_SYMBOL', c)
open(p, 'w').write(c)
" "$COMPAT_FILE"
  fi

  if [ -f "$TK_FILE" ]; then
    if [ -f "${TK_FILE}.bak" ]; then
      cp "${TK_FILE}.bak" "$TK_FILE"
    else
      cp "$TK_FILE" "${TK_FILE}.bak"
    fi
    python3 -c "
import re, sys
p = sys.argv[1]
c = open(p).read()
c = re.sub(r'if\s*\(!tuxedo_is_compatible\(\)\)\s*return\s+-ENODEV;', '// bypass compatibility check', c)
open(p, 'w').write(c)
" "$TK_FILE"
  fi

  if [ -f "$UWK_FILE" ]; then
    if [ -f "${UWK_FILE}.bak" ]; then
      cp "${UWK_FILE}.bak" "$UWK_FILE"
    else
      cp "$UWK_FILE" "${UWK_FILE}.bak"
    fi
    python3 -c "
import sys
p = sys.argv[1]
c = open(p).read()
fan_boost_code = '''
static ssize_t fan_boost_show(struct device *dev, struct device_attribute *attr, char *buf)
{
\tu8 mode_data = 0;
\tuniwill_read_ec_ram(0x0751, &mode_data);
\treturn sprintf(buf, \"%d\\\\n\", (mode_data & 0x40) ? 1 : 0);
}

static ssize_t fan_boost_store(struct device *dev, struct device_attribute *attr, const char *buf, size_t count)
{
\tif (count > 0) {
\t\tbool enable = (buf[0] == '1' || buf[0] == 'y' || buf[0] == 'Y');
\t\tu8 mode_data = 0;
\t\tuniwill_read_ec_ram(0x0751, &mode_data);
\t\tif (enable) {
\t\t\tif (!(mode_data & 0x40)) {
\t\t\t\tuniwill_write_ec_ram(0x0751, mode_data | 0x40);
\t\t\t}
\t\t} else {
\t\t\tif (mode_data & 0x40) {
\t\t\t\tuniwill_write_ec_ram(0x0751, mode_data & ~0x40);
\t\t\t}
\t\t}
\t}
\treturn count;
}
static DEVICE_ATTR_RW(fan_boost);
'''
c = c.replace('static int uniwill_keyboard_probe(struct platform_device *dev)', fan_boost_code + '\\nstatic int uniwill_keyboard_probe(struct platform_device *dev)')
c = c.replace('uw_battery_init();', 'uw_battery_init();\\n\\tif (device_create_file(&dev->dev, &dev_attr_fan_boost))\\n\\t\\tpr_warn(\"Failed to create fan_boost sysfs file\\\\n\");')
c = c.replace('uw_battery_uninit();', 'device_remove_file(&dev->dev, &dev_attr_fan_boost);\\n\\tuw_battery_uninit();')
open(p, 'w').write(c)
" "$UWK_FILE"
  fi

  echo "[*] 正在清理旧构建缓存并重新编译 DKMS 驱动 (tuxedo-drivers/$TUXEDO_VER)..."
  dkms unbuild "tuxedo-drivers/$TUXEDO_VER" -k "$(uname -r)" 2>/dev/null || true
  dkms build "tuxedo-drivers/$TUXEDO_VER" -k "$(uname -r)"
  dkms install "tuxedo-drivers/$TUXEDO_VER" -k "$(uname -r)" --force
  echo "[+] DKMS 驱动重编并安装完成。"
fi

# 4. 加载驱动模块并配置开机自启
echo "[*] 4/5 正在装载驱动内核模块..."
# 依次卸载所有依赖模块以解除 tuxedo_keyboard 占用锁定
for mod in \
  tuxedo_nb02_nvidia_power_ctrl \
  tuxedo_nb04_sensors \
  tuxedo_nb04_power_profiles \
  tuxedo_nb04_kbd_backlight \
  tuxedo_nb04_keyboard \
  tuxedo_nb04_wmi_bs \
  tuxedo_nb04_wmi_ab \
  uniwill_wmi \
  tuxedo_io \
  tuxedo_keyboard \
  tuxedo_compatibility_check
do
  modprobe -r "$mod" 2>/dev/null || true
done

# 如果仍有残留占用，通过 rmmod 针对性解除
for mod in tuxedo_nb02_nvidia_power_ctrl uniwill_wmi tuxedo_io tuxedo_keyboard tuxedo_compatibility_check; do
  if lsmod | grep -q "^$mod "; then
    rmmod "$mod" 2>/dev/null || true
  fi
done

modprobe tuxedo_compatibility_check
modprobe tuxedo_keyboard
modprobe tuxedo_io
modprobe uniwill_wmi
modprobe tuxedo_nb04_wmi_ab 2>/dev/null || true
modprobe tuxedo_nb04_keyboard 2>/dev/null || true
modprobe tuxedo_nb04_kbd_backlight 2>/dev/null || true
modprobe tuxedo_nb04_power_profiles 2>/dev/null || true

MODULES_CONF="/etc/modules-load.d/mechrevo.conf"
if [ -f "$MODULES_CONF" ]; then
  for mod in tuxedo_compatibility_check tuxedo_keyboard tuxedo_io uniwill_wmi tuxedo_nb04_wmi_ab tuxedo_nb04_power_profiles; do
    if ! grep -q "^$mod$" "$MODULES_CONF"; then
      echo "$mod" >> "$MODULES_CONF"
    fi
  done
fi

# 5. 重启 mechrevo-ctl 守护进程以立即重新探测并绑定新节点
echo "[*] 5/5 正在重启 mechrevo-ctl.service 守护进程..."
systemctl restart mechrevo-ctl.service

echo ""
echo "=================================================================="
echo "    [+] 驱动与硬件接口已成功就绪！"
echo "=================================================================="
echo "正在检查最新硬件状态与功能可用性:"
mechrevo-ctl status
echo ""

