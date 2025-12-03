use anyhow::{Context, Result};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixListener};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

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

    /// Get next available port
    pub fn get_port(&mut self) -> u16 {
        let port = self.next_port;
        self.next_port += 1;
        if self.next_port > PORT_RANGE_END {
            self.next_port = PORT_RANGE_START;
        }
        port
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

type SharedRegistry = Arc<RwLock<Registry>>;

/// Run the daemon
pub async fn run() -> Result<()> {
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
        if let Err(e) = run_proxy_server(proxy_registry).await {
            error!("Proxy server error: {}", e);
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

/// Run the HTTP proxy server
async fn run_proxy_server(registry: SharedRegistry) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 80));
    let listener = TcpListener::bind(addr).await.context(
        "Failed to bind to port 80. Try running with sudo or check if another process is using it.",
    )?;

    info!("Proxy listening on http://127.0.0.1:80");

    loop {
        let (stream, _) = listener.accept().await?;
        let registry = registry.clone();

        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |req| {
                let registry = registry.clone();
                async move { handle_proxy_request(req, registry).await }
            });

            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                error!("Proxy connection error: {}", e);
            }
        });
    }
}

async fn handle_proxy_request(
    req: Request<Incoming>,
    registry: SharedRegistry,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Extract host from request
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Remove port from host if present
    let domain = host.split(':').next().unwrap_or(&host).to_string();

    // Look up the service
    let port = {
        let reg = registry.read().await;
        reg.get(&domain).map(|s| s.port)
    };

    match port {
        Some(port) => {
            // Forward the request to the backend
            match forward_request(req, port).await {
                Ok(response) => Ok(response),
                Err(e) => {
                    warn!("Failed to forward request to {}: {}", domain, e);
                    Ok(Response::builder()
                        .status(502)
                        .body(Full::new(Bytes::from(format!("Bad Gateway: {}", e))))
                        .unwrap())
                }
            }
        }
        None => {
            // Show dashboard for localhost, 404 for unknown domains
            if domain == "localhost" || domain == "127.0.0.1" {
                let path = req.uri().path();

                // Handle kill API endpoint
                if path.starts_with("/api/kill/") {
                    let target_domain = path.strip_prefix("/api/kill/").unwrap_or("");
                    if !target_domain.is_empty() {
                        let mut reg = registry.write().await;
                        if let Some(service) = reg.unregister(target_domain) {
                            unsafe {
                                libc::kill(service.pid as i32, libc::SIGTERM);
                            }
                            info!("Killed service: {}", target_domain);
                            return Ok(Response::builder()
                                .status(200)
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"ok":true}"#)))
                                .unwrap());
                        } else {
                            return Ok(Response::builder()
                                .status(404)
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"error":"not found"}"#)))
                                .unwrap());
                        }
                    }
                }

                let reg = registry.read().await;
                let services = reg.list();
                let html = render_dashboard(&services);
                Ok(Response::builder()
                    .status(200)
                    .header("content-type", "text/html; charset=utf-8")
                    .body(Full::new(Bytes::from(html)))
                    .unwrap())
            } else {
                let reg = registry.read().await;
                let services = reg.list();
                let available = services
                    .iter()
                    .map(|s| format!("  - http://{}", s.domain))
                    .collect::<Vec<_>>()
                    .join("\n");

                let body = format!(
                    "unport: Domain '{}' not found.\n\nAvailable services:\n{}",
                    domain,
                    if available.is_empty() {
                        "  (none)".to_string()
                    } else {
                        available
                    }
                );

                Ok(Response::builder()
                    .status(404)
                    .header("content-type", "text/plain")
                    .body(Full::new(Bytes::from(body)))
                    .unwrap())
            }
        }
    }
}

async fn forward_request(
    req: Request<Incoming>,
    port: u16,
) -> Result<Response<Full<Bytes>>> {
    // Try localhost (which resolves to IPv4 or IPv6) first, then fallback to 127.0.0.1
    let stream = match TcpStream::connect(format!("localhost:{}", port)).await {
        Ok(s) => s,
        Err(_) => TcpStream::connect(format!("127.0.0.1:{}", port)).await?,
    };
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            error!("Connection error: {}", e);
        }
    });

    let (parts, body) = req.into_parts();
    let body_bytes = body.collect().await?.to_bytes();
    let new_req = Request::from_parts(parts, Full::new(body_bytes));

    let response = sender.send_request(new_req).await?;
    let (parts, body) = response.into_parts();
    let body_bytes = body.collect().await?.to_bytes();

    Ok(Response::from_parts(parts, Full::new(body_bytes)))
}

fn render_dashboard(services: &[Service]) -> String {
    let service_rows = if services.is_empty() {
        r#"<tr><td colspan="4" class="empty">No services running. Start one with <code>unport start</code></td></tr>"#.to_string()
    } else {
        services
            .iter()
            .map(|s| {
                let url = format!("http://{}", s.domain);
                let status = if is_process_alive(s.pid) { "running" } else { "stopped" };
                let status_class = if is_process_alive(s.pid) { "status-running" } else { "status-stopped" };
                format!(
                    r#"<tr id="row-{}">
                        <td><span class="status-dot {}"></span>{}</td>
                        <td class="url">{}</td>
                        <td>{}</td>
                        <td class="actions">
                            <button class="btn btn-copy" onclick="copyToClipboard('{}')">Copy</button>
                            <a href="{}" class="btn btn-go" target="_blank">Open</a>
                            <button class="btn btn-kill" onclick="killService('{}')">Kill</button>
                        </td>
                    </tr>"#,
                    s.domain, status_class, status, url, s.port, url, url, s.domain
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>unport - Local Development Services</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            background: #0a0a0a;
            color: #e5e5e5;
            min-height: 100vh;
            padding: 40px 20px;
        }}
        .container {{
            max-width: 800px;
            margin: 0 auto;
        }}
        header {{
            margin-bottom: 40px;
        }}
        h1 {{
            font-size: 28px;
            font-weight: 600;
            color: #fff;
            margin-bottom: 8px;
        }}
        .subtitle {{
            color: #666;
            font-size: 14px;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            background: #141414;
            border-radius: 8px;
            overflow: hidden;
        }}
        th {{
            text-align: left;
            padding: 12px 16px;
            font-size: 12px;
            font-weight: 500;
            color: #666;
            text-transform: uppercase;
            letter-spacing: 0.5px;
            border-bottom: 1px solid #222;
        }}
        td {{
            padding: 16px;
            border-bottom: 1px solid #1a1a1a;
            font-size: 14px;
        }}
        tr:last-child td {{
            border-bottom: none;
        }}
        tr:hover {{
            background: #1a1a1a;
        }}
        .url {{
            font-family: 'SF Mono', Monaco, 'Courier New', monospace;
            color: #3b82f6;
        }}
        .status-dot {{
            display: inline-block;
            width: 8px;
            height: 8px;
            border-radius: 50%;
            margin-right: 8px;
        }}
        .status-running {{
            background: #22c55e;
            box-shadow: 0 0 8px rgba(34, 197, 94, 0.5);
        }}
        .status-stopped {{
            background: #ef4444;
        }}
        .actions {{
            display: flex;
            gap: 8px;
        }}
        .btn {{
            padding: 6px 12px;
            border-radius: 4px;
            font-size: 12px;
            font-weight: 500;
            cursor: pointer;
            transition: all 0.15s ease;
            text-decoration: none;
            border: none;
        }}
        .btn-copy {{
            background: #222;
            color: #e5e5e5;
            border: 1px solid #333;
        }}
        .btn-copy:hover {{
            background: #333;
            border-color: #444;
        }}
        .btn-go {{
            background: #3b82f6;
            color: #fff;
        }}
        .btn-go:hover {{
            background: #2563eb;
        }}
        .btn-kill {{
            background: #dc2626;
            color: #fff;
        }}
        .btn-kill:hover {{
            background: #b91c1c;
        }}
        .empty {{
            text-align: center;
            color: #666;
            padding: 40px 16px;
        }}
        code {{
            background: #222;
            padding: 2px 6px;
            border-radius: 4px;
            font-family: 'SF Mono', Monaco, 'Courier New', monospace;
            font-size: 13px;
        }}
        .toast {{
            position: fixed;
            bottom: 20px;
            left: 50%;
            transform: translateX(-50%) translateY(100px);
            background: #22c55e;
            color: #fff;
            padding: 12px 24px;
            border-radius: 6px;
            font-size: 14px;
            font-weight: 500;
            opacity: 0;
            transition: all 0.3s ease;
        }}
        .toast.show {{
            transform: translateX(-50%) translateY(0);
            opacity: 1;
        }}
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>unport</h1>
            <p class="subtitle">Local Development Services</p>
        </header>
        <table>
            <thead>
                <tr>
                    <th>Status</th>
                    <th>URL</th>
                    <th>Port</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {}
            </tbody>
        </table>
    </div>
    <div class="toast" id="toast">Copied to clipboard</div>
    <script>
        function copyToClipboard(text) {{
            navigator.clipboard.writeText(text).then(() => {{
                showToast('Copied to clipboard');
            }});
        }}
        function killService(domain) {{
            if (confirm('Kill ' + domain + '?')) {{
                fetch('/api/kill/' + domain, {{ method: 'POST' }})
                    .then(r => r.json())
                    .then(data => {{
                        if (data.ok) {{
                            const row = document.getElementById('row-' + domain);
                            if (row) row.remove();
                            showToast('Killed ' + domain);
                        }} else {{
                            showToast('Failed to kill service');
                        }}
                    }})
                    .catch(() => showToast('Failed to kill service'));
            }}
        }}
        function showToast(msg) {{
            const toast = document.getElementById('toast');
            toast.textContent = msg;
            toast.classList.add('show');
            setTimeout(() => toast.classList.remove('show'), 2000);
        }}
    </script>
</body>
</html>"##,
        service_rows
    )
}
