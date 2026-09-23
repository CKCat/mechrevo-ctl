# 机械革命 极光X (GM6IQ0C) Linux 控制台工业级原生工具集 (mechrevo-ctl Rust)

专为 **机械革命 极光X（MECHREVO JiguangX Series，内部代号 GM6IQ0C，i7-14650HX + RTX 4060）** 及同方/Uniwill GM6 模具定制的原生 Rust 工业级控制台工具集，用于完整替代 Windows 下的电竞控制台 (`GamingCenter3`)。

本项目基于 Linux 内核设备驱动架构（ACPI, Powercap RAPL, Netlink UEvent, NVML/SMI）设计，遵循 Linux 内核低功耗与并发时序安全规范，采用纯 Rust 编写，具备内存安全、零运行时开销、瞬时启动与纳秒级响应。

---

## 一、 工业级内核特性

1. **时序安全防倒挂与 RAPL 双域同步 (Anti-Inversion & Dual-Domain MSR/MMIO)**：
   - 升档严格执行 **“先抬 PL2、再提 PL1”**，降档严格执行 **“先压 PL1、再降 PL2”**，杜绝违背物理时序导致内核拒写或 CPU P-Unit 降频锁死。
   - 针对 Intel 14 代 HX 架构同时同步写入 MSR (`intel-rapl`) 与 MMIO (`intel-rapl-mmio`)，击穿 P-Unit $\min(MSR, MMIO)$ 的 45W 默认限制，真正释放 105W 狂暴性能。
2. **零轮询能耗友好型守护进程 (Zero-Polling Event-Driven Daemon)**：
   - 彻底废除定时唤醒轮询，直接利用 Linux 内核 **Netlink KObject UEvent (`NETLINK_KOBJECT_UEVENT`)** 监听硬件中断事件。不插拔电源时进程挂起，**0% CPU 占用，允许 CPU 顺畅进入 Package C8/C10 深层睡眠，最大化电池续航**。
3. **PCIe RTD3 D3cold 零惊醒保护与 Class 隔离 (Zero-Wakeup Guard & Multi-Function Isolation)**：
   - 严格校验 PCI 设备 Class Code (`0x03*`)，隔离 NVIDIA HDAudio 伴随功能，仅当 3D 显示核心进入挂起时判定 D3cold。在节能模式下短路返回，严禁执行 `nvidia-smi` 触发 `pm_runtime_get_sync()` 引起 15W~30W 功率震荡。
4. **Intel CPU EPP 调频策略协同 (HWP Energy Performance Preference Sync)**：
   - 切换模式时自动同步 `/sys/devices/system/cpu/cpufreq/policy*/energy_performance_preference`。Turbo 模式激活 `performance` 击穿 HWP 升频迟滞、跑满 105W 算力；Office 模式锁定 `power` 引导核心沉入 C8/C10 深度睡眠。
5. **安全跨电源域休眠唤醒重放与双轨电源感知 (Dual-Rail Supply & Resume Hook)**：
   - 适配 DC 圆口适配器 (`Mains`) 与 100W/140W USB Type-C 氮化镓 PD 充电器 (`USB_PD`) 双轨供电接入探测，杜绝出差插 Type-C 时被误判为电池放电的低性能锁死缺陷。
   - 配套 systemd-sleep 唤醒钩子。合盖休眠唤醒时强制重验真实物理供电状态，拔电唤醒强制回落至 Office 节能模式，重放电池养护阈值，杜绝高功耗损坏电池。
6. **双轨驱动生态电池养护与防损坏 (Mainline & Tuxedo DKMS Dual-Track Battery Conservation)**：
   - 同时支持 Linux 6.19+ 主线 `uniwill-laptop` 驱动标准接口（严格维护 `start_threshold < end_threshold` 回滞时序防 `-EINVAL`）与广泛使用的 `tuxedo-drivers` DKMS 接口（自动双向映射 `stationary` 80% / `balanced` 90% / `high_capacity` 100%）。
7. **NVIDIA Dynamic Boost 满血 140W 算力解锁与服务协同 (140W Full-Power Release)**：
   - 联动激活系统级 `nvidia-powerd.service` 守护进程，跨 CPU 与 GPU 动态搬移 25W 热功耗预算，击穿 Ada 架构移动端驱动锁，真正释放 RTX 4060 满血 140W 算力。
8. **发行版桌面服务互斥治理 (power-profiles-daemon Conflict Mitigation)**：
   - systemd 显式声明 `Conflicts=power-profiles-daemon.service`，安装脚本自检并治理 PPD 桌面竞争，杜绝 ACPI `platform_profile` 状态撕裂。
9. **无特权安全 IPC 架构与 PLATYPUS 安全穿透 (Non-root UNIX Socket & CVE-2020-8694 Pass-Through)**：
   - 守护进程以 root 运行并通过 `/run/mechrevo-ctl.sock` 通信。普通桌面用户**无需输入 sudo 密码**即可调节硬件；`mechrevo-ctl status` 优先走 IPC 透传，彻底解决发行版内核收紧 RAPL `0600` 权限导致的非 root 状态查询被拒问题。

---

## 二、 快速安装与部署 (在 Linux 环境下)

### 1. 基础环境依赖

进入你的 Linux 系统（如 Ubuntu 24.04、Fedora 40/41 或 Arch Linux）后，只需具备 Rust 编译工具链：

```bash
# 官方标准一键安装 Rust (含 cargo 与 rustc)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 或者使用系统包管理器:
# Ubuntu / Debian: sudo apt update && sudo apt install -y cargo rustc
# Arch Linux:      sudo pacman -S rust
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
[*] Hardware Chassis: GM6IQ0C
[*] Intel RAPL (CPU): PL1 = 75W | PL2 = 90W | Temp: 52°C
    Energy Perf Preference (EPP): balance_performance
[*] NVIDIA dGPU: NVIDIA GeForce RTX 4060 Laptop GPU
    TGP Limit: 115W (115W Base + 25W Dynamic Boost [nvidia-powerd active])
    Temperature: 48°C
[*] ACPI Platform Profile: balanced
[*] Battery: 85% (Discharging) | Threshold Limit: 80%
[*] Keyboard Backlight: 50%
[*] Cooling (Fans): Fan 1: 2800 RPM | Fan 2: 3000 RPM (Boost: OFF)
==================================================
```

> **注意 (内核级诚实契约)**：若当前系统尚未安装 `tuxedo-drivers` 且未暴露 `platform_profile` 节点，系统将明确标注由原生 RAPL & EPP 接管，并提示 EC 硬件风扇曲线未同步事实，建议开启 `fan-boost on` 或安装 DKMS，杜绝重负载撞温控墙。

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

### 5. 控制风扇强冷 (Fan Boost)

狂暴模式（Turbo）已恢复为与 EC 内部高级温控表（`M3T1`）动态联动，避免日常无谓满速起飞。如果用户在重度游戏或极限拷机场景下需要强制拉满 100% 风扇强冷，可随时通过独立指令按需开关：

```bash
# 强制开启双风扇 100% 极限强冷 (Fan Boost ON)
mechrevo-ctl fan-boost on

# 恢复为由 EC 固件动态温控曲线接管 (Fan Boost OFF)
mechrevo-ctl fan-boost off
```

> **注意**：强冷接口依赖底层内核驱动暴露的控制节点（如 `tuxedo-drivers`）。若当前系统未加载支持强冷的驱动模块，命令将诚实提示硬件接口未就绪，并引导用户安装对应驱动。

## 四、 本地运行自动化测试

在任意操作系统（包括 Windows 物理机开发环境与 Linux）上均可通过内置 Mock Sysfs 内存/文件系统运行完整测试套件：

```bash
# 运行 Rust 原生工业级测试套件 (100% 覆盖核心硬件时序、安全边界与双域同步)
cargo test --all-targets

# 针对 Linux 目标环境执行跨架构静态语法与架构契约检查
cargo check --target x86_64-unknown-linux-gnu
```
