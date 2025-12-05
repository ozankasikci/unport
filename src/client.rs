use anyhow::{Context, Result};
use std::env;
use std::os::unix::net::UnixStream;
use std::io::{BufRead, BufReader, Write};
use tracing::warn;

use crate::log_info;

use crate::config::Config;
use crate::detect::{detect, PortStrategy};
use crate::process::spawn_app;
use crate::types::{pid_path, socket_path, Request, Response};

/// Send a request to the daemon and get a response
fn send_request(request: &Request) -> Result<Response> {
    let socket = socket_path();
    let mut stream = UnixStream::connect(&socket).context(
        "Could not connect to daemon. Is it running? Start it with: unport daemon",
    )?;

    let request_json = serde_json::to_string(request)? + "\n";
    stream.write_all(request_json.as_bytes())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    // Handle empty response (e.g., daemon closed connection after shutdown)
    if line.is_empty() {
        return Ok(Response::Ok(None));
    }

    let response: Response = serde_json::from_str(&line)?;
    Ok(response)
}

/// Start an app and register with daemon
pub async fn start() -> Result<()> {
    let cwd = env::current_dir()?;

    // Load config
    let config = Config::load(&cwd)?;
    let domain = config.full_domain();

    // Detect framework
    let detection = detect(&cwd)?;
    log_info!("Detected framework: {}", detection.framework);

    // Get start command (from config or detection)
    let start_command = config.start.as_deref().unwrap_or(&detection.start_command);

    // Get port from daemon
    let port = match send_request(&Request::GetPort)? {
        Response::Port(p) => p,
        Response::Error(e) => anyhow::bail!("{}", e),
        _ => anyhow::bail!("Unexpected response from daemon"),
    };

    log_info!("Assigned port: {}", port);

    // Determine port strategy
    let port_strategy = if config.port_arg.is_some() {
        PortStrategy::CliFlag(config.port_arg.clone().unwrap())
    } else if config.port_env.is_some() {
        PortStrategy::EnvVar(config.port_env.clone().unwrap())
    } else {
        detection.port_strategy
    };

    // Check if HTTPS is enabled
    let https_enabled = match send_request(&Request::HttpsStatus) {
        Ok(Response::HttpsEnabled(enabled)) => enabled,
        _ => false,
    };

    if https_enabled {
        log_info!("HTTPS enabled, certificate updated for https://{}", domain);
    }

    // Spawn the app
    println!("Starting {}...", config.domain);
    println!("Running: {} (port {})", start_command, port);
    if https_enabled {
        println!("Available at: http://{}", domain);
        println!("              https://{}", domain);
    } else {
        println!("Available at: http://{}", domain);
    }
    println!();

    let mut child = spawn_app(
        start_command,
        port,
        &port_strategy,
        config.port_env.as_deref(),
        config.port_arg.as_deref(),
    )?;

    let pid = child.id();

    // Register with daemon
    match send_request(&Request::Register {
        domain: domain.clone(),
        port,
        pid,
        directory: cwd,
    })? {
        Response::Ok(_) => {}
        Response::Error(e) => {
            warn!("Failed to register: {}", e);
        }
        _ => {}
    }

    // Set up Ctrl+C handler
    let domain_clone = domain.clone();
    ctrlc::set_handler(move || {
        // Unregister on exit
        let _ = send_request(&Request::Unregister {
            domain: domain_clone.clone(),
        });
        std::process::exit(0);
    })?;

    // Wait for child to exit
    let status = child.wait()?;

    // Unregister
    let _ = send_request(&Request::Unregister { domain });

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Process exited with status: {}", status)
    }
}

/// List all registered services
pub async fn list() -> Result<()> {
    let response = send_request(&Request::List)?;

    match response {
        Response::Services(services) => {
            if services.is_empty() {
                println!("No services registered.");
            } else {
                println!(
                    "{:<24} {:<8} {:<8} {}",
                    "DOMAIN", "PORT", "PID", "DIRECTORY"
                );
                for service in services {
                    let status = if is_process_alive(service.pid) {
                        ""
                    } else {
                        " (dead)"
                    };
                    println!(
                        "{:<24} {:<8} {:<8} {}{}",
                        service.domain,
                        service.port,
                        service.pid,
                        service.directory.display(),
                        status
                    );
                }
            }
        }
        Response::Error(e) => {
            anyhow::bail!("{}", e);
        }
        _ => {
            anyhow::bail!("Unexpected response");
        }
    }

    Ok(())
}

/// Stop a service by domain
pub async fn stop_service(domain: &str) -> Result<()> {
    let full_domain = if domain.contains('.') {
        domain.to_string()
    } else {
        format!("{}.localhost", domain)
    };

    let response = send_request(&Request::Stop { domain: full_domain })?;

    match response {
        Response::Ok(msg) => {
            println!("{}", msg.unwrap_or_else(|| "Stopped".into()));
        }
        Response::Error(e) => {
            anyhow::bail!("{}", e);
        }
        _ => {
            anyhow::bail!("Unexpected response");
        }
    }

    Ok(())
}

/// Stop the daemon
pub async fn stop_daemon() -> Result<()> {
    let response = send_request(&Request::Shutdown)?;

    match response {
        Response::Ok(_) => {
            println!("Daemon stopped.");
        }
        Response::Error(e) => {
            anyhow::bail!("{}", e);
        }
        _ => {}
    }

    Ok(())
}

/// Show daemon status
pub async fn daemon_status() -> Result<()> {
    let pid_file = pid_path();

    // Check if PID file exists
    if !pid_file.exists() {
        println!("Status: stopped");
        println!("  Daemon is not running (no PID file)");
        return Ok(());
    }

    // Read PID
    let pid_str = std::fs::read_to_string(&pid_file)?;
    let pid: u32 = pid_str.trim().parse().context("Invalid PID file")?;

    // Check if process is alive
    if !is_process_alive(pid) {
        println!("Status: stopped");
        println!("  Daemon is not running (stale PID file, process {} not found)", pid);
        return Ok(());
    }

    // Try to connect to daemon
    let service_count = match send_request(&Request::List) {
        Ok(Response::Services(services)) => services.len(),
        Ok(_) => 0,
        Err(_) => {
            println!("Status: error");
            println!("  Process {} is running but daemon is not responding", pid);
            return Ok(());
        }
    };

    // Get uptime from PID file modification time
    let uptime = if let Ok(metadata) = std::fs::metadata(&pid_file) {
        if let Ok(created) = metadata.modified() {
            if let Ok(duration) = created.elapsed() {
                format_duration(duration)
            } else {
                "unknown".to_string()
            }
        } else {
            "unknown".to_string()
        }
    } else {
        "unknown".to_string()
    };

    println!("Status: running");
    println!("  PID:      {}", pid);
    println!("  Uptime:   {}", uptime);
    println!("  Services: {}", service_count);

    Ok(())
}

fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

fn is_process_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    if result == 0 {
        return true;
    }
    // EPERM means process exists but we can't signal it (e.g., root-owned process)
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    errno == libc::EPERM
}

/// Add or remove the CA certificate from the system trust store
pub async fn trust_ca(remove: bool) -> Result<()> {
    crate::tls::trust_ca(remove)
}

/// Regenerate TLS certificate with SANs for all registered domains
pub async fn regen_cert() -> Result<()> {
    use crate::types::Response;

    // Get list of registered services from daemon
    let domains: Vec<String> = match send_request(&crate::types::Request::List) {
        Ok(Response::Services(services)) => {
            services.into_iter().map(|s| s.domain).collect()
        }
        Ok(_) => vec![],
        Err(_) => {
            println!("Note: Daemon not running, generating cert for localhost only.");
            vec![]
        }
    };

    // Generate new certificate with all domains
    crate::tls::generate_cert(&domains)?;

    println!("\n⚠️  Restart the daemon to use the new certificate:");
    println!("   sudo unport daemon stop && sudo unport daemon start -d --https");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(std::time::Duration::from_secs(0)), "0s");
        assert_eq!(format_duration(std::time::Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(std::time::Duration::from_secs(59)), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(std::time::Duration::from_secs(60)), "1m 0s");
        assert_eq!(format_duration(std::time::Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(std::time::Duration::from_secs(3599)), "59m 59s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(std::time::Duration::from_secs(3600)), "1h 0m");
        assert_eq!(format_duration(std::time::Duration::from_secs(7200)), "2h 0m");
        assert_eq!(format_duration(std::time::Duration::from_secs(3660)), "1h 1m");
        assert_eq!(format_duration(std::time::Duration::from_secs(86399)), "23h 59m");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(std::time::Duration::from_secs(86400)), "1d 0h");
        assert_eq!(format_duration(std::time::Duration::from_secs(172800)), "2d 0h");
        assert_eq!(format_duration(std::time::Duration::from_secs(90000)), "1d 1h");
    }

    #[test]
    fn test_is_process_alive_current() {
        let pid = std::process::id();
        assert!(is_process_alive(pid));
    }

    #[test]
    fn test_is_process_alive_dead() {
        // Very high PID that doesn't exist
        assert!(!is_process_alive(4000000));
    }

    #[test]
    fn test_is_process_alive_init() {
        // PID 1 should always exist (init/systemd)
        assert!(is_process_alive(1));
    }
}
