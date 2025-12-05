use anyhow::{Context, Result};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio_rustls::TlsAcceptor;
use tracing::{error, warn};

use crate::log_info;

use crate::daemon::Registry;
use crate::types::Service;

pub type SharedRegistry = Arc<RwLock<Registry>>;
pub type SharedTlsAcceptor = Arc<RwLock<TlsAcceptor>>;

/// Run the HTTP proxy server on port 80
pub async fn run_http(registry: SharedRegistry) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 80));
    let listener = TcpListener::bind(addr).await.context(
        "Failed to bind to port 80. Try running with sudo or check if another process is using it.",
    )?;

    log_info!("HTTP proxy listening on http://127.0.0.1:80");

    loop {
        let (stream, _) = listener.accept().await?;
        let registry = registry.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, registry).await {
                error!("HTTP connection error: {}", e);
            }
        });
    }
}

/// Run the HTTPS proxy server on port 443
pub async fn run_https(registry: SharedRegistry, acceptor: SharedTlsAcceptor) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 443));
    let listener = TcpListener::bind(addr).await.context(
        "Failed to bind to port 443. Try running with sudo or check if another process is using it.",
    )?;

    log_info!("HTTPS proxy listening on https://127.0.0.1:443");

    loop {
        let (stream, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let registry = registry.clone();

        tokio::spawn(async move {
            // Get the current acceptor (allows hot-reload)
            let tls_acceptor = acceptor.read().await.clone();
            match tls_acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    if let Err(e) = handle_tls_connection(tls_stream, registry).await {
                        error!("HTTPS connection error: {}", e);
                    }
                }
                Err(e) => {
                    error!("TLS handshake error: {}", e);
                }
            }
        });
    }
}

/// Handle a TLS connection
async fn handle_tls_connection<S>(stream: S, registry: SharedRegistry) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // For TLS connections, we use hyper directly (no WebSocket peek needed for now)
    let io = TokioIo::new(stream);
    let service = service_fn(move |req| {
        let registry = registry.clone();
        async move { handle_http_request(req, registry).await }
    });

    if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
        if !e.to_string().contains("connection reset") {
            error!("HTTPS proxy connection error: {}", e);
        }
    }

    Ok(())
}

/// Handle a single connection - detect WebSocket upgrades vs regular HTTP
async fn handle_connection(mut stream: TcpStream, registry: SharedRegistry) -> Result<()> {
    // Peek at the first bytes to parse the HTTP request
    let mut buf = vec![0u8; 4096];
    let n = stream.peek(&mut buf).await?;
    let peek_data = &buf[..n];

    // Parse headers to check for WebSocket upgrade
    let header_str = String::from_utf8_lossy(peek_data);
    let is_websocket =
        header_str.contains("Upgrade: websocket") || header_str.contains("upgrade: websocket");

    // Extract host from headers
    let host = extract_host_from_headers(&header_str).unwrap_or_default();
    let domain = host.split(':').next().unwrap_or(&host).to_string();

    // Look up the service
    let port = {
        let reg = registry.read().await;
        reg.get(&domain).map(|s| s.port)
    };

    if is_websocket {
        // WebSocket: tunnel raw TCP
        if let Some(port) = port {
            handle_websocket_tunnel(stream, port).await?;
        } else {
            // No service found - send 404 and close
            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            stream.write_all(response.as_bytes()).await?;
        }
    } else {
        // Regular HTTP: use hyper
        let io = TokioIo::new(stream);
        let service = service_fn(move |req| {
            let registry = registry.clone();
            async move { handle_http_request(req, registry).await }
        });

        if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
            // Don't log connection reset errors - they're normal
            if !e.to_string().contains("connection reset") {
                error!("Proxy connection error: {}", e);
            }
        }
    }

    Ok(())
}

/// Extract Host header from raw HTTP headers
fn extract_host_from_headers(headers: &str) -> Option<String> {
    for line in headers.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("host:") {
            return Some(line[5..].trim().to_string());
        }
    }
    None
}

/// Handle WebSocket upgrade by tunneling raw TCP
async fn handle_websocket_tunnel(mut client: TcpStream, backend_port: u16) -> Result<()> {
    use tokio::io::copy_bidirectional;

    // Connect to backend
    let mut backend = match TcpStream::connect(format!("127.0.0.1:{}", backend_port)).await {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to connect to backend for WebSocket: {}", e);
            return Ok(());
        }
    };

    // Tunnel all data bidirectionally (including the initial HTTP upgrade request)
    match copy_bidirectional(&mut client, &mut backend).await {
        Ok((client_to_backend, backend_to_client)) => {
            log_info!(
                "WebSocket tunnel closed: {} bytes up, {} bytes down",
                client_to_backend, backend_to_client
            );
        }
        Err(e) => {
            // Connection reset is normal when WebSocket closes
            if !e.to_string().contains("reset") {
                warn!("WebSocket tunnel error: {}", e);
            }
        }
    }

    Ok(())
}

/// Handle regular HTTP request
async fn handle_http_request(
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
                            log_info!("Killed service: {}", target_domain);
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

async fn forward_request(req: Request<Incoming>, port: u16) -> Result<Response<Full<Bytes>>> {
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

fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn render_dashboard(services: &[Service]) -> String {
    let service_rows = if services.is_empty() {
        r#"<tr><td colspan="4" class="empty">No services running. Start one with <code>unport start</code></td></tr>"#.to_string()
    } else {
        services
            .iter()
            .map(|s| {
                let url = format!("http://{}", s.domain);
                let status = if is_process_alive(s.pid) {
                    "running"
                } else {
                    "stopped"
                };
                let status_class = if is_process_alive(s.pid) {
                    "status-running"
                } else {
                    "status-stopped"
                };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_extract_host_from_headers_simple() {
        let headers = "GET / HTTP/1.1\r\nHost: api.localhost\r\n";
        let host = extract_host_from_headers(headers);
        assert_eq!(host, Some("api.localhost".to_string()));
    }

    #[test]
    fn test_extract_host_from_headers_with_port() {
        let headers = "GET / HTTP/1.1\r\nHost: api.localhost:80\r\n";
        let host = extract_host_from_headers(headers);
        assert_eq!(host, Some("api.localhost:80".to_string()));
    }

    #[test]
    fn test_extract_host_from_headers_lowercase() {
        let headers = "GET / HTTP/1.1\r\nhost: web.localhost\r\n";
        let host = extract_host_from_headers(headers);
        assert_eq!(host, Some("web.localhost".to_string()));
    }

    #[test]
    fn test_extract_host_from_headers_not_found() {
        let headers = "GET / HTTP/1.1\r\nConnection: keep-alive\r\n";
        let host = extract_host_from_headers(headers);
        assert!(host.is_none());
    }

    #[test]
    fn test_extract_host_with_whitespace() {
        let headers = "GET / HTTP/1.1\r\nHost:   spaces.localhost   \r\n";
        let host = extract_host_from_headers(headers);
        assert_eq!(host, Some("spaces.localhost".to_string()));
    }

    #[test]
    fn test_is_process_alive_current() {
        let pid = std::process::id();
        assert!(is_process_alive(pid));
    }

    #[test]
    fn test_is_process_alive_dead() {
        assert!(!is_process_alive(4000000));
    }

    #[test]
    fn test_render_dashboard_empty_services() {
        let services: Vec<Service> = vec![];
        let html = render_dashboard(&services);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("No services running"));
        assert!(html.contains("unport start"));
    }

    #[test]
    fn test_render_dashboard_with_services() {
        let services = vec![
            Service {
                domain: "api.localhost".to_string(),
                port: 4000,
                pid: std::process::id(), // Use current PID so it's "alive"
                directory: PathBuf::from("/app/api"),
            },
        ];
        let html = render_dashboard(&services);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("http://api.localhost"));
        assert!(html.contains("4000"));
    }

    #[test]
    fn test_render_dashboard_contains_required_elements() {
        let services: Vec<Service> = vec![];
        let html = render_dashboard(&services);
        assert!(html.contains("<title>"));
        assert!(html.contains("unport"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<thead>"));
        assert!(html.contains("<tbody>"));
    }

    #[test]
    fn test_render_dashboard_with_dead_process() {
        let services = vec![Service {
            domain: "dead.localhost".to_string(),
            port: 4000,
            pid: 4000000, // Non-existent PID
            directory: PathBuf::from("/app/dead"),
        }];
        let html = render_dashboard(&services);
        assert!(html.contains("stopped"));
        assert!(html.contains("status-stopped"));
    }

    #[test]
    fn test_render_dashboard_has_actions() {
        let services = vec![Service {
            domain: "test.localhost".to_string(),
            port: 4001,
            pid: std::process::id(),
            directory: PathBuf::from("/app/test"),
        }];
        let html = render_dashboard(&services);
        assert!(html.contains("Copy"));
        assert!(html.contains("Open"));
        assert!(html.contains("Kill"));
    }

    #[test]
    fn test_render_dashboard_multiple_services() {
        let services = vec![
            Service {
                domain: "api.localhost".to_string(),
                port: 4000,
                pid: std::process::id(),
                directory: PathBuf::from("/app/api"),
            },
            Service {
                domain: "web.localhost".to_string(),
                port: 4001,
                pid: std::process::id(),
                directory: PathBuf::from("/app/web"),
            },
        ];
        let html = render_dashboard(&services);
        assert!(html.contains("http://api.localhost"));
        assert!(html.contains("http://web.localhost"));
        assert!(html.contains("4000"));
        assert!(html.contains("4001"));
    }
}
