# 机械革命 极光X (GM6IQ0C) Linux 控制台工业级工具集 (mechrevo-ctl)

专为 **机械革命 极光X（MECHREVO JiguangX Series，内部代号 GM6IQ0C，i7-14650HX + RTX 4060）** 及同方/Uniwill GM6 模具定制的 Linux 控制台工具集，用于完整替代 Windows 下的电竞控制台 (`GamingCenter3`)。

本项目基于 Linux 内核设备驱动架构（ACPI, Powercap RAPL, Netlink UEvent, NVML/SMI）设计，遵循 Linux 内核低功耗与并发时序安全规范。

---

## 一、 工业级内核特性

1. **时序安全防倒挂 (Anti-Inversion Power Limits)**：
   - 升档严格执行 **“先抬 PL2、再提 PL1”**，降档严格执行 **“先压 PL1、再降 PL2”**，杜绝违背物理法则导致内核拒写或 CPU P-Unit 降频锁死。
2. **零轮询能耗友好型守护进程 (Zero-Polling Event-Driven Daemon)**：
   - 彻底废除定时唤醒轮询，直接利用 Linux 内核 **Netlink KObject UEvent (`NETLINK_KOBJECT_UEVENT`)** 监听硬件中断事件。不插拔电源时进程挂起，**0% CPU 占用，允许 CPU 顺畅进入 Package C8/C10 深层睡眠，最大化电池续航**。
3. **系统休眠唤醒状态重放 (S3 / s2idle Sleep Resume Hook)**：
   - 配套 systemd-sleep 唤醒钩子，彻底解决笔记本合盖唤醒后 EC 固件重置导致的功耗与电池上限失效问题。
4. **NVIDIA 驱动持久化 (Persistence Mode)**：
   - 针对双显卡动态切换环境，自动激活 `nvidia-smi -pm 1` 持久化模式，避免空闲切核显时独显驱动卸载导致的 TGP 功耗限额丢失。
5. **动态 Sysfs 命名空间嗅探 (Dynamic Sysfs Probing)**：
   - 自适应扫描匹配 `BAT*`（兼容 `BAT0`, `BAT1`, `BATT`）与 `*kbd_backlight*`，避免不同 BIOS/驱动批次设备节点名称差异。
6. **无特权安全 IPC 架构 (Non-root UNIX Socket)**：
   - 守护进程以 root 运行并通过 `/run/mechrevo-ctl.sock` 通信，普通桌面用户**无需输入 sudo 密码**即可调节硬件，杜绝了向 Python 脚本开放全局 sudoers 的提权隐患。

---

## 二、 快速安装与部署 (在 Linux 环境下)

### 1. 基础环境依赖
进入你的 Linux 系统（如 Ubuntu 24.04、Fedora 40/41 或 Arch Linux）后，打开终端克隆或拷贝本目录代码：

```bash
# Ubuntu / Debian
sudo apt update && sudo apt install -y python3 python3-pip

# Arch Linux
sudo pacman -S python python-pip
```

### 2. 一键安装
在项目根目录下执行安装脚本：

```bash
chmod +x install.sh
sudo ./install.sh
```

该脚本将自动完成：
- 安装 `mechrevo-ctl` 命令行工具与 `mechrevo-ctld` 守护进程；
- 启动 `mechrevo-ctl.service` 并配置开机自启；
- 部署 `/usr/lib/systemd/system-sleep/mechrevo-ctl-resume.sh` 唤醒重放钩子。

---

## 三、 常用使用命令 (普通用户即可执行，无需 sudo)

### 1. 查看当前硬件状态
```bash
mechrevo-ctl status
```
**输出示例**：
```text
==================================================
      MECHREVO Control Center - System Status
==================================================
[*] Intel RAPL (CPU): PL1 = 75W | PL2 = 90W
[*] NVIDIA dGPU: NVIDIA GeForce RTX 4060 Laptop GPU
    TGP Limit: 115W (Max: 140W)
    Temperature: 48°C
[*] ACPI Platform Profile: balanced
[*] Battery: 85% (Discharging) | Threshold Limit: 80%
[*] Keyboard Backlight: 50%
==================================================
```

### 2. 切换性能模式
```bash
# 切换为满血狂暴模式 (游戏/编译/AI 推理，CPU 105W, GPU 140W)
mechrevo-ctl profile turbo

# 切换为日常平衡模式 (CPU 75W, GPU 115W)
mechrevo-ctl profile balanced

# 切换为办公静音模式 (CPU 45W, GPU 60W)
mechrevo-ctl profile office
```

### 3. 设置电池养护上限
```bash
# 设置为充至 80% 停止充电 (推荐常年插电办公使用)
mechrevo-ctl battery 80

# 准备出差需要充满时恢复 100%
mechrevo-ctl battery 100
```

### 4. 调节键盘背光
```bash
# 调至 80% 亮度
mechrevo-ctl keyboard --brightness 80

# 关闭键盘灯
mechrevo-ctl keyboard --brightness 0
```

---

## 四、 本地运行自动化测试

在任意操作系统（包括 Windows 物理机开发环境）上均可通过虚拟 sysfs 夹具运行完整测试套件：

```bash
python -m pytest tests -v
```
