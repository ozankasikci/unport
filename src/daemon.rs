use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::proxy;
use crate::types::{
    pid_path, registry_path, socket_path, unport_dir, Request as DaemonRequest,
    Response as DaemonResponse, Service, PORT_RANGE_END, PORT_RANGE_START,
};

/// Registry of services
#[derive(Default)]
pub struct Registry {
    services: HashMap<String, Service>,
    next_port: u16,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            next_port: PORT_RANGE_START,
        }
    }

    /// Load registry from disk
    pub fn load() -> Self {
        let path = registry_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(services) = serde_json::from_str::<HashMap<String, Service>>(&content) {
                    let max_port = services
                        .values()
                        .map(|s| s.port)
                        .max()
                        .unwrap_or(PORT_RANGE_START - 1);
                    return Self {
                        services,
                        next_port: max_port + 1,
                    };
                }
            }
        }
        Self::new()
    }

    /// Save registry to disk
    pub fn save(&self) -> Result<()> {
        let path = registry_path();
        let content = serde_json::to_string_pretty(&self.services)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Get next available port (checks if port is actually free)
    pub fn get_port(&mut self) -> u16 {
        let start = self.next_port;
        loop {
            let port = self.next_port;
            self.next_port += 1;
            if self.next_port > PORT_RANGE_END {
                self.next_port = PORT_RANGE_START;
            }

            // Check if port is actually available by trying to bind
            if is_port_available(port) {
                return port;
            }

            // Prevent infinite loop if all ports are taken
            if self.next_port == start {
                // Fall back to returning the port anyway; it will fail at app startup
                return port;
            }
        }
    }

    /// Register a service
    pub fn register(&mut self, service: Service) {
        self.services.insert(service.domain.clone(), service);
        let _ = self.save();
    }

    /// Unregister a service
    pub fn unregister(&mut self, domain: &str) -> Option<Service> {
        let service = self.services.remove(domain);
        let _ = self.save();
        service
    }

    /// Get a service by domain
    pub fn get(&self, domain: &str) -> Option<&Service> {
        self.services.get(domain)
    }

    /// List all services
    pub fn list(&self) -> Vec<Service> {
        self.services.values().cloned().collect()
    }

    /// Clean up dead processes
    pub fn cleanup_dead(&mut self) {
        let dead: Vec<String> = self
            .services
            .iter()
            .filter(|(_, s)| !is_process_alive(s.pid))
            .map(|(domain, _)| domain.clone())
            .collect();

        for domain in dead {
            info!("Cleaning up dead service: {}", domain);
            self.services.remove(&domain);
        }
        let _ = self.save();
    }
}

fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Check if a port is available by attempting to bind to it
fn is_port_available(port: u16) -> bool {
    // Check both IPv4 and IPv6 since apps may bind to either
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
        && std::net::TcpListener::bind(("::1", port)).is_ok()
        && std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

type SharedRegistry = Arc<RwLock<Registry>>;

/// Run the daemon
pub async fn run(detach: bool) -> Result<()> {
    // If detach requested, spawn daemon in background and exit
    if detach {
        let exe = std::env::current_exe().context("Failed to get current executable")?;

        // Ensure unport directory exists
        let dir = unport_dir();
        std::fs::create_dir_all(&dir).context("Failed to create ~/.unport directory")?;

        // Open log file for daemon output
        let log_path = dir.join("daemon.log");
        let log_file = std::fs::File::create(&log_path)
            .context("Failed to create daemon log file")?;
        let log_file_err = log_file.try_clone()?;

        std::process::Command::new(exe)
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(log_file)
            .stderr(log_file_err)
            .spawn()
            .context("Failed to spawn daemon process")?;

        println!("Daemon started in background. Logs at: {:?}", log_path);
        return Ok(());
    }

    // Ensure unport directory exists
    let dir = unport_dir();
    std::fs::create_dir_all(&dir).context("Failed to create ~/.unport directory")?;

    // Check if daemon is already running
    let pid_file = pid_path();
    if pid_file.exists() {
        let pid_str = std::fs::read_to_string(&pid_file)?;
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if is_process_alive(pid) {
                anyhow::bail!("Daemon already running (PID {})", pid);
            }
        }
        std::fs::remove_file(&pid_file)?;
    }

    // Write PID file
    std::fs::write(&pid_file, std::process::id().to_string())?;

    // Remove old socket if exists
    let sock_path = socket_path();
    if sock_path.exists() {
        std::fs::remove_file(&sock_path)?;
    }

    // Load registry and cleanup dead processes
    let registry = Arc::new(RwLock::new(Registry::load()));
    {
        let mut reg = registry.write().await;
        reg.cleanup_dead();
    }

    info!("Starting unport daemon...");

    // Start Unix socket listener for CLI commands
    let socket_registry = registry.clone();
    let socket_handle = tokio::spawn(async move {
        if let Err(e) = run_socket_server(socket_registry).await {
            error!("Socket server error: {}", e);
        }
    });

    // Start HTTP proxy
    let proxy_registry = registry.clone();
    let proxy_handle = tokio::spawn(async move {
        if let Err(e) = proxy::run(proxy_registry).await {
            error!("Proxy server error: {}", e);
        }
    });

    // Start periodic cleanup of dead processes
    let cleanup_registry = registry.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let mut reg = cleanup_registry.write().await;
            reg.cleanup_dead();
        }
    });

    info!("Daemon running. Proxy on :80, socket at {:?}", sock_path);

    // Wait for shutdown
    tokio::select! {
        _ = socket_handle => {},
        _ = proxy_handle => {},
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down...");
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(&sock_path);
    let _ = std::fs::remove_file(&pid_file);

    Ok(())
}

/// Run the Unix socket server for CLI commands
async fn run_socket_server(registry: SharedRegistry) -> Result<()> {
    let sock = socket_path();
    let listener = UnixListener::bind(&sock)?;

    // Make socket world-writable so non-root users can connect
    // when daemon runs as root
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o777))?;

    loop {
        let (stream, _) = listener.accept().await?;
        let registry = registry.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_socket_client(stream, registry).await {
                error!("Client error: {}", e);
            }
        });
    }
}

async fn handle_socket_client(
    stream: tokio::net::UnixStream,
    registry: SharedRegistry,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let request: DaemonRequest = serde_json::from_str(&line)?;
        let response = handle_request(request, &registry).await;
        let response_json = serde_json::to_string(&response)? + "\n";
        writer.write_all(response_json.as_bytes()).await?;
        line.clear();
    }

    Ok(())
}

async fn handle_request(request: DaemonRequest, registry: &SharedRegistry) -> DaemonResponse {
    match request {
        DaemonRequest::Register {
            domain,
            port,
            pid,
            directory,
        } => {
            let mut reg = registry.write().await;
            if reg.get(&domain).is_some() {
                return DaemonResponse::Error(format!("Domain '{}' already registered", domain));
            }
            reg.register(Service {
                domain: domain.clone(),
                port,
                pid,
                directory,
            });
            info!("Registered: {} -> localhost:{}", domain, port);
            DaemonResponse::Ok(Some(format!("Registered {}", domain)))
        }
        DaemonRequest::Unregister { domain } => {
            let mut reg = registry.write().await;
            if reg.unregister(&domain).is_some() {
                info!("Unregistered: {}", domain);
                DaemonResponse::Ok(Some(format!("Unregistered {}", domain)))
            } else {
                DaemonResponse::Error(format!("Domain '{}' not found", domain))
            }
        }
        DaemonRequest::GetPort => {
            let mut reg = registry.write().await;
            let port = reg.get_port();
            DaemonResponse::Port(port)
        }
        DaemonRequest::List => {
            let reg = registry.read().await;
            DaemonResponse::Services(reg.list())
        }
        DaemonRequest::Stop { domain } => {
            let mut reg = registry.write().await;
            if let Some(service) = reg.unregister(&domain) {
                // Send SIGTERM to the process
                unsafe {
                    libc::kill(service.pid as i32, libc::SIGTERM);
                }
                info!("Stopped: {}", domain);
                DaemonResponse::Ok(Some(format!("Stopped {}", domain)))
            } else {
                DaemonResponse::Error(format!("Domain '{}' not found", domain))
            }
        }
        DaemonRequest::Shutdown => {
            info!("Shutdown requested");
            std::process::exit(0);
        }
    }
}
