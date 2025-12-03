use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A registered service in the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub domain: String,
    pub port: u16,
    pub pid: u32,
    pub directory: PathBuf,
}

/// Messages sent from CLI to daemon via Unix socket
#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    /// Register a new service
    Register {
        domain: String,
        port: u16,
        pid: u32,
        directory: PathBuf,
    },
    /// Unregister a service
    Unregister { domain: String },
    /// Get an available port
    GetPort,
    /// List all services
    List,
    /// Stop a service by domain
    Stop { domain: String },
    /// Shutdown the daemon
    Shutdown,
}

/// Messages sent from daemon to CLI
#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// Success with optional message
    Ok(Option<String>),
    /// Assigned port number
    Port(u16),
    /// List of services
    Services(Vec<Service>),
    /// Error message
    Error(String),
}

/// Port assignment range
pub const PORT_RANGE_START: u16 = 4000;
pub const PORT_RANGE_END: u16 = 5000;

/// Get the unport home directory (~/.unport)
pub fn unport_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".unport")
}

/// Get the Unix socket path
pub fn socket_path() -> PathBuf {
    unport_dir().join("unport.sock")
}

/// Get the PID file path
pub fn pid_path() -> PathBuf {
    unport_dir().join("unport.pid")
}

/// Get the registry file path
pub fn registry_path() -> PathBuf {
    unport_dir().join("registry.json")
}
