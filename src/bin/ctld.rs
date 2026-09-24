//! Background daemon binary for MECHREVO hardware control.

use mechrevo::daemon::MechrevoDaemon;
use mechrevo::hal::SysfsContext;

#[cfg(unix)]
fn run_daemon() -> std::io::Result<()> {
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use mechrevo::daemon::DEFAULT_SOCKET_PATH;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Setup signal handler for clean graceful shutdown
    let _ = ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    });

    let socket_path = DEFAULT_SOCKET_PATH;
    let path = Path::new(socket_path);
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => {
            // Set 0666 permissions so desktop users can adjust hardware without sudo
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o666));
            println!("[+] IPC UNIX socket bound: {}", socket_path);
            Some(l)
        }
        Err(e) => {
            eprintln!("[!] Warning: Failed to bind UNIX socket {}: {}", socket_path, e);
            None
        }
    };

    if let Some(ref l) = listener {
        l.set_nonblocking(true)?;
    }

    // Initialize Linux Netlink KObject UEvent socket for zero-polling hardware event wakeups
    #[cfg(target_os = "linux")]
    let netlink_fd: Option<i32> = {
        const NETLINK_KOBJECT_UEVENT: i32 = 15;
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                NETLINK_KOBJECT_UEVENT,
            )
        };
        if fd >= 0 {
            let mut sa: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
            sa.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            sa.nl_groups = 1; // Multicast group 1: kernel uevents

            let bind_res = unsafe {
                libc::bind(
                    fd,
                    &sa as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
                )
            };
            if bind_res == 0 {
                println!("[+] Kernel Netlink UEvent listener: ENABLED (zero-polling energy saving)");
                Some(fd)
            } else {
                eprintln!("[!] Warning: Failed to bind Netlink socket: errno {}", unsafe { *libc::__errno_location() });
                unsafe { libc::close(fd) };
                None
            }
        } else {
            None
        }
    };

    #[cfg(not(target_os = "linux"))]
    let netlink_fd: Option<i32> = None;

    let ctx = SysfsContext::new();
    let daemon = Arc::new(Mutex::new(MechrevoDaemon::new(
        ctx,
        Some(socket_path.to_string()),
    )));

    println!("[*] MECHREVO control daemon running (Rust native event-driven engine).");
    {
        let mut d = daemon.lock().unwrap();
        d.evaluate_ac_state();
    }

    while running.load(Ordering::SeqCst) {
        let mut poll_fds: Vec<libc::pollfd> = Vec::with_capacity(2);

        if let Some(ref l) = listener {
            poll_fds.push(libc::pollfd {
                fd: l.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }

        if let Some(n_fd) = netlink_fd {
            poll_fds.push(libc::pollfd {
                fd: n_fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }

        if poll_fds.is_empty() {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }

        // Sleep in poll() until a real hardware uevent occurs or an IPC client connects.
        // 60s timeout acts as an ultra-low-frequency fallback sanity check.
        // Zero busy-looping -> CPU stays in C8/C10 deep package sleep.
        let poll_ret = unsafe {
            libc::poll(
                poll_fds.as_mut_ptr(),
                poll_fds.len() as libc::nfds_t,
                60_000,
            )
        };

        if poll_ret < 0 {
            // Interrupted by signal or error
            continue;
        }

        let mut trigger_ac_eval = false;

        // Check Netlink UEvent events
        if let Some(n_fd) = netlink_fd {
            if let Some(pfd) = poll_fds.iter().find(|p| p.fd == n_fd) {
                if (pfd.revents & libc::POLLIN) != 0 {
                    let mut buf = [0u8; 4096];
                    loop {
                        let bytes_read = unsafe {
                            libc::recv(
                                n_fd,
                                buf.as_mut_ptr() as *mut libc::c_void,
                                buf.len(),
                                0,
                            )
                        };
                        if bytes_read <= 0 {
                            break;
                        }
                        let slice = &buf[..bytes_read as usize];
                        if slice.windows(12).any(|w| w == b"power_supply") {
                            trigger_ac_eval = true;
                        }
                    }
                }
            }
        }

        // Check UNIX IPC incoming connections
        if let Some(ref l) = listener {
            let l_fd = l.as_raw_fd();
            if let Some(pfd) = poll_fds.iter().find(|p| p.fd == l_fd) {
                if (pfd.revents & libc::POLLIN) != 0 {
                    while let Ok((mut stream, _)) = l.accept() {
                        let daemon_clone = daemon.clone();
                        // Spawn isolated worker thread with 1.0s timeout to prevent Slowloris DoS
                        std::thread::spawn(move || {
                            // 显式切为阻塞模式，确保 1.0s 超时在所有 Unix 平台与内核版本均 100% 生效
                            let _ = stream.set_nonblocking(false);
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                            let mut buf = vec![0u8; 4096];
                            if let Ok(n) = stream.read(&mut buf) {
                                if n > 0 {
                                    let payload = String::from_utf8_lossy(&buf[..n]);
                                    let response_str = {
                                        let mut d = daemon_clone.lock().unwrap();
                                        d.handle_ipc_request(&payload)
                                    };
                                    let _ = stream.write_all(response_str.as_bytes());
                                    let _ = stream.shutdown(std::net::Shutdown::Write);
                                }
                            }
                        });
                    }
                }
            }
        }

        // Trigger AC state check if netlink detected power change or poll timed out
        if trigger_ac_eval || poll_ret == 0 {
            let mut d = daemon.lock().unwrap();
            d.evaluate_ac_state();
        }
    }

    if let Some(n_fd) = netlink_fd {
        unsafe { libc::close(n_fd) };
    }
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    println!("[*] MECHREVO control daemon stopped.");
    Ok(())
}

#[cfg(not(unix))]
fn run_daemon() -> std::io::Result<()> {
    let ctx = SysfsContext::new();
    let mut daemon = MechrevoDaemon::new(ctx, None);
    println!("[*] MECHREVO control daemon running in stub/non-unix mode.");
    daemon.evaluate_ac_state();
    Ok(())
}

fn main() -> std::io::Result<()> {
    run_daemon()
}
