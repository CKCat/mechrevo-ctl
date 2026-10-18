//! Command-line interface for mechrevo-ctl.

use clap::{Parser, Subcommand};
use mechrevo::core::battery::BatteryDriver;
use mechrevo::core::cpu::IntelRaplDriver;
use mechrevo::core::gpu::NvidiaSmiDriver;
use mechrevo::core::keyboard::KeyboardDriver;
use mechrevo::core::platform::{AcpiPlatformDriver, FanDriver, ProfileManager};
use mechrevo::daemon::{IpcRequest, IpcResponse};
use mechrevo::error::{MechrevoError, Result};
use mechrevo::hal::SysfsContext;

#[derive(Parser, Debug)]
#[command(name = "mechrevo-ctl")]
#[command(about = "MECHREVO Laptop Hardware Control Utility for Linux (Rust Native)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show current hardware status and power limits
    Status,
    /// Switch performance release profile
    Profile {
        /// Target mode: office, balanced, turbo
        #[arg(value_parser = ["office", "balanced", "turbo"])]
        mode: String,
    },
    /// Manage battery charge conservation threshold
    Battery {
        /// Battery charge limit percentage (e.g. 60, 80, 100)
        threshold: u32,
    },
    /// Control keyboard backlight
    Keyboard {
        /// Brightness percentage (0 to 100)
        #[arg(long, short)]
        brightness: u32,
    },
    /// Control maximum cooling fan boost
    FanBoost {
        /// Target state: on, off, 1, 0
        #[arg(value_parser = ["on", "off", "1", "0"])]
        state: String,
    },
    /// Reload and re-apply current performance profile
    ReloadProfile,
}

#[cfg(unix)]
fn send_ipc_command(req: &IpcRequest) -> Option<IpcResponse> {
    send_ipc_command_with_retry(req, 1)
}

#[cfg(unix)]
fn send_ipc_command_with_retry(req: &IpcRequest, max_retries: usize) -> Option<IpcResponse> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let socket_path = "/run/mechrevo-ctl.sock";
    for attempt in 0..max_retries {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(150));
        }

        let mut stream = match UnixStream::connect(socket_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        if stream.set_read_timeout(Some(Duration::from_secs(1))).is_err()
            || stream.set_write_timeout(Some(Duration::from_secs(1))).is_err()
        {
            continue;
        }

        let data = match serde_json::to_vec(req) {
            Ok(d) => d,
            Err(_) => continue,
        };

        if stream.write_all(&data).is_err() {
            continue;
        }

        let mut buf = Vec::new();
        if stream.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
            if let Ok(resp) = serde_json::from_slice(&buf) {
                return Some(resp);
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn send_ipc_command(_req: &IpcRequest) -> Option<IpcResponse> {
    None
}

#[cfg(not(unix))]
fn send_ipc_command_with_retry(_req: &IpcRequest, _max_retries: usize) -> Option<IpcResponse> {
    None
}


fn cmd_status(ctx: &SysfsContext) -> Result<()> {
    // 优先通过 root 守护进程 IPC 获取完整硬件状态 (规避 CVE-2020-8694 PLATYPUS 对非 root 读取 0600 powercap 的拦截)
    if let Some(resp) = send_ipc_command(&IpcRequest::Status) {
        if resp.success {
            if let Some(report) = resp.message {
                println!("{}", report);
                return Ok(());
            }
        }
    }

    // 本地回退：直接扫描 sysfs 节点 (适用于守护进程未启动或独立使用场景)
    println!("==================================================");
    println!("      MECHREVO Control Center - System Status");
    println!("==================================================");

    // 0. Hardware Chassis
    let platform_drv = AcpiPlatformDriver::new(ctx.clone());
    if let Some(chassis) = platform_drv.get_dmi_product_name() {
        println!("[*] Hardware Chassis: {}", chassis);
    }

    // 1. CPU
    let cpu_drv = IntelRaplDriver::new(ctx.clone());
    let fan_drv = FanDriver::new(ctx.clone());
    let hwmon = fan_drv.get_hwmon_snapshot();
    match cpu_drv.get_power_limits() {
        Ok(limits) => {
            let temp_str = hwmon
                .cpu_temp_c
                .map(|t| format!(" | Temp: {}°C", t))
                .unwrap_or_default();
            println!(
                "[*] Intel RAPL (CPU): PL1 = {:.0}W | PL2 = {:.0}W{}",
                limits.pl1_w, limits.pl2_w, temp_str
            );
        }
        Err(e) => {
            let temp_str = hwmon
                .cpu_temp_c
                .map(|t| format!(" (Temp: {}°C)", t))
                .unwrap_or_default();
            println!("[-] Intel RAPL (CPU): Unavailable ({}){}", e, temp_str);
        }
    }
    let epp_nodes = cpu_drv.find_epp_nodes();
    if let Some(first_epp) = epp_nodes.first() {
        if let Ok(curr_epp) = ctx.read_text(first_epp) {
            println!("    Energy Perf Preference (EPP): {}", curr_epp);
        }
    }

    // 2. GPU
    let gpu_drv = NvidiaSmiDriver::default().with_ctx(ctx.clone());
    if gpu_drv.is_available() {
        match gpu_drv.get_gpu_info() {
            Ok(info) if info.is_suspended => {
                println!("[*] NVIDIA dGPU: {}", info.name);
                println!("    Power State: Suspended (PCIe RTD3 D3cold - Zero Wakeup)");
            }
            Ok(info) => {
                println!("[*] NVIDIA dGPU: {}", info.name);
                println!(
                    "    TGP Limit: {:.0}W (Max: {:.0}W)",
                    info.current_power_limit_w, info.max_power_limit_w
                );
                println!("    Temperature: {}°C", info.temperature_c);
            }
            Err(e) => println!("[-] NVIDIA dGPU: Query error ({})", e),
        }
    } else {
        println!("[-] NVIDIA dGPU: Not available or driver not loaded");
    }

    // 3. ACPI Platform Profile
    let platform_drv = AcpiPlatformDriver::new(ctx.clone());
    match platform_drv.get_current_profile() {
        Ok(profile) => println!("[*] ACPI Platform Profile: {}", profile),
        Err(_) => println!(
            "[-] ACPI Platform Profile: Not available in kernel (managed via native Intel RAPL & EPP; install tuxedo-drivers for EC thermal curve link)"
        ),
    }

    // 4. Battery
    let bat_drv = BatteryDriver::new(ctx.clone());
    match bat_drv.get_battery_status() {
        Ok(status) => println!(
            "[*] Battery: {}% ({}) | Threshold Limit: {}%",
            status.capacity_percent, status.status, status.charge_threshold
        ),
        Err(e) => println!("[-] Battery: Unavailable ({})", e),
    }

    // 5. Keyboard
    let kbd_drv = KeyboardDriver::new(ctx.clone());
    match kbd_drv.get_brightness_percent() {
        Ok(brightness) => println!("[*] Keyboard Backlight: {}%", brightness),
        Err(e) => println!("[-] Keyboard Backlight: Unavailable ({})", e),
    }

    // 6. Cooling & Fans (hwmon + Fan Boost)
    let boost_str = match fan_drv.get_fan_boost() {
        Some(true) => "Boost: ON",
        Some(false) => "Boost: OFF",
        None => "Boost: N/A",
    };
    let fan_rpm_str = match (hwmon.fan1_rpm, hwmon.fan2_rpm) {
        (Some(f1), Some(f2)) => format!("Fan 1: {} RPM | Fan 2: {} RPM", f1, f2),
        (Some(f1), None) => format!("Fan 1: {} RPM", f1),
        (None, Some(f2)) => format!("Fan 2: {} RPM", f2),
        (None, None) => "Managed by EC firmware".to_string(),
    };
    println!("[*] Cooling (Fans): {} ({})", fan_rpm_str, boost_str);

    println!("==================================================");
    Ok(())
}

fn cmd_profile(ctx: &SysfsContext, mode: &str) -> Result<()> {
    if let Some(resp) = send_ipc_command(&IpcRequest::Profile {
        mode: mode.to_string(),
    }) {
        if resp.success {
            println!("[+] {}", resp.message.unwrap_or_default());
            return Ok(());
        } else {
            eprintln!("[!] Error: {}", resp.error.unwrap_or_default());
            std::process::exit(1);
        }
    }

    // Fallback to local root execution
    let cpu = IntelRaplDriver::new(ctx.clone());
    let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
    let platform = AcpiPlatformDriver::new(ctx.clone());
    let fan = FanDriver::new(ctx.clone());
    let mgr = ProfileManager::new(cpu, gpu, platform, fan);

    let res = mgr.apply_profile(mode)?;

    // 若无守护进程在运行 (本地 root 执行模式)，持久化 Profile 锁定状态至磁盘
    let state_file = ctx.resolve(mechrevo::daemon::DEFAULT_STATE_FILE_PATH);
    if let Some(parent) = state_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing_state: mechrevo::daemon::DaemonState = if state_file.exists() {
        std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        mechrevo::daemon::DaemonState::default()
    };
    let updated_state = mechrevo::daemon::DaemonState {
        charge_threshold: existing_state.charge_threshold,
        manual_profile: Some(mode.to_string()),
    };
    if let Ok(json) = serde_json::to_string_pretty(&updated_state) {
        let _ = std::fs::write(&state_file, json);
    }

    println!(
        "[+] Successfully switched to [{}]: {}",
        res.applied_profile.name, res.applied_profile.display_name
    );
    println!(
        "    CPU PL1/PL2: {:.0}W / {:.0}W",
        res.applied_profile.cpu_pl1_w, res.applied_profile.cpu_pl2_w
    );
    println!("    GPU TGP: {:.0}W", res.applied_profile.gpu_tgp_w);
    Ok(())
}

fn cmd_battery(ctx: &SysfsContext, threshold: u32) -> Result<()> {
    if let Some(resp) = send_ipc_command(&IpcRequest::Battery { threshold }) {
        if resp.success {
            println!("[+] {}", resp.message.unwrap_or_default());
            return Ok(());
        } else {
            eprintln!("[!] Error: {}", resp.error.unwrap_or_default());
            std::process::exit(1);
        }
    }

    let bat_drv = BatteryDriver::new(ctx.clone());
    bat_drv.set_charge_threshold(threshold)?;

    // 若无守护进程在运行 (本地 root 执行模式)，持久化电池阈值至磁盘
    let state_file = ctx.resolve(mechrevo::daemon::DEFAULT_STATE_FILE_PATH);
    if let Some(parent) = state_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing_state: mechrevo::daemon::DaemonState = if state_file.exists() {
        std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        mechrevo::daemon::DaemonState::default()
    };
    let updated_state = mechrevo::daemon::DaemonState {
        charge_threshold: Some(threshold),
        manual_profile: existing_state.manual_profile,
    };
    if let Ok(json) = serde_json::to_string_pretty(&updated_state) {
        let _ = std::fs::write(&state_file, json);
    }

    println!(
        "[+] Battery charge threshold set to {}% (Note: If battery continues charging past limit, toggle conservation mode once in BIOS/Windows to initialize EC register 0x087F)",
        threshold
    );
    Ok(())
}

fn cmd_keyboard(ctx: &SysfsContext, brightness: u32) -> Result<()> {
    if let Some(resp) = send_ipc_command(&IpcRequest::Keyboard { brightness }) {
        if resp.success {
            println!("[+] {}", resp.message.unwrap_or_default());
            return Ok(());
        } else {
            eprintln!("[!] Error: {}", resp.error.unwrap_or_default());
            std::process::exit(1);
        }
    }

    let kbd_drv = KeyboardDriver::new(ctx.clone());
    kbd_drv.set_brightness_percent(brightness)?;
    println!("[+] Keyboard brightness set to {}%", brightness);
    Ok(())
}

fn cmd_fan_boost(ctx: &SysfsContext, state: &str) -> Result<()> {
    let enable = state == "on" || state == "1";
    if let Some(resp) = send_ipc_command(&IpcRequest::FanBoost { enable }) {
        if resp.success {
            println!("[+] {}", resp.message.unwrap_or_default());
            return Ok(());
        } else {
            eprintln!("[!] Error: {}", resp.error.unwrap_or_default());
            std::process::exit(1);
        }
    }

    let fan_drv = FanDriver::new(ctx.clone());
    fan_drv.set_fan_boost(enable)?;
    let state_msg = if enable {
        "enabled (maximum cooling)"
    } else {
        "disabled (automatic thermal curve)"
    };
    println!("[+] Fan boost {}", state_msg);
    Ok(())
}

fn cmd_reload_profile(ctx: &SysfsContext) -> Result<()> {
    if let Some(resp) = send_ipc_command_with_retry(&IpcRequest::ReloadProfile, 4) {
        if resp.success {
            println!(
                "[+] {}",
                resp.message
                    .unwrap_or_else(|| "Profile reloaded successfully.".to_string())
            );
            return Ok(());
        } else {
            eprintln!("[!] Error: {}", resp.error.unwrap_or_default());
            std::process::exit(1);
        }
    }

    eprintln!("[!] Warning: Daemon IPC unreachable during resume. Performing power-aware hardware state self-healing...");
    let daemon = mechrevo::daemon::MechrevoDaemon::new(ctx.clone(), None);
    let fallback_mode = if daemon.is_ac_online() {
        "balanced"
    } else {
        "office"
    };
    let target_mode = daemon.manual_profile_override.as_deref().unwrap_or(fallback_mode);

    let cpu = IntelRaplDriver::new(ctx.clone());
    let gpu = NvidiaSmiDriver::default().with_ctx(ctx.clone());
    let platform = AcpiPlatformDriver::new(ctx.clone());
    let fan = FanDriver::new(ctx.clone());
    let mgr = ProfileManager::new(cpu, gpu, platform, fan);
    let res = mgr.apply_profile(target_mode)?;
    println!(
        "[+] Hardware state self-healed: Applied [{}] profile ({})",
        res.applied_profile.name, res.applied_profile.display_name
    );
    Ok(())
}


fn main() {
    let cli = Cli::parse();
    let ctx = SysfsContext::new();

    let result = match cli.command {
        Commands::Status => cmd_status(&ctx),
        Commands::Profile { mode } => cmd_profile(&ctx, &mode),
        Commands::Battery { threshold } => cmd_battery(&ctx, threshold),
        Commands::Keyboard { brightness } => cmd_keyboard(&ctx, brightness),
        Commands::FanBoost { state } => cmd_fan_boost(&ctx, &state),
        Commands::ReloadProfile => cmd_reload_profile(&ctx),
    };

    if let Err(e) = result {
        match e {
            MechrevoError::PermissionDenied => {
                eprintln!("[!] Error: Permission denied.");
                eprintln!("[!] Hint: Please run this command with 'sudo'.");
                std::process::exit(13);
            }
            MechrevoError::InvalidParameter(msg) => {
                eprintln!("[!] Invalid parameter: {}", msg);
                std::process::exit(2);
            }
            err => {
                eprintln!("[!] Error: {}", err);
                std::process::exit(1);
            }
        }
    }
}
