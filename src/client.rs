use anyhow::{Context, Result};
use std::env;
use std::os::unix::net::UnixStream;
use std::io::{BufRead, BufReader, Write};
use tracing::{info, warn};

use crate::config::Config;
use crate::detect::{detect, PortStrategy};
use crate::process::spawn_app;
use crate::types::{socket_path, Request, Response};

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
    info!("Detected framework: {}", detection.framework);

    // Get start command (from config or detection)
    let start_command = config.start.as_deref().unwrap_or(&detection.start_command);

    // Get port from daemon
    let port = match send_request(&Request::GetPort)? {
        Response::Port(p) => p,
        Response::Error(e) => anyhow::bail!("{}", e),
        _ => anyhow::bail!("Unexpected response from daemon"),
    };

    info!("Assigned port: {}", port);

    // Determine port strategy
    let port_strategy = if config.port_arg.is_some() {
        PortStrategy::CliFlag(config.port_arg.clone().unwrap())
    } else if config.port_env.is_some() {
        PortStrategy::EnvVar(config.port_env.clone().unwrap())
    } else {
        detection.port_strategy
    };

    // Spawn the app
    println!("Starting {}...", config.domain);
    println!("Running: {} (port {})", start_command, port);
    println!("Available at: http://{}", domain);
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

fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
