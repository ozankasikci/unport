// Tests for client module helper functions
// Note: Most client functions require a running daemon, so we test what we can

use std::time::Duration;

// Test the format_duration function logic (reimplemented for testing)
fn format_duration(duration: Duration) -> String {
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

#[test]
fn test_format_duration_seconds() {
    assert_eq!(format_duration(Duration::from_secs(0)), "0s");
    assert_eq!(format_duration(Duration::from_secs(30)), "30s");
    assert_eq!(format_duration(Duration::from_secs(59)), "59s");
}

#[test]
fn test_format_duration_minutes() {
    assert_eq!(format_duration(Duration::from_secs(60)), "1m 0s");
    assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
    assert_eq!(format_duration(Duration::from_secs(3599)), "59m 59s");
}

#[test]
fn test_format_duration_hours() {
    assert_eq!(format_duration(Duration::from_secs(3600)), "1h 0m");
    assert_eq!(format_duration(Duration::from_secs(7200)), "2h 0m");
    assert_eq!(format_duration(Duration::from_secs(3660)), "1h 1m");
    assert_eq!(format_duration(Duration::from_secs(86399)), "23h 59m");
}

#[test]
fn test_format_duration_days() {
    assert_eq!(format_duration(Duration::from_secs(86400)), "1d 0h");
    assert_eq!(format_duration(Duration::from_secs(172800)), "2d 0h");
    assert_eq!(format_duration(Duration::from_secs(90000)), "1d 1h");
}

// Test is_process_alive logic (reimplemented for testing)
fn is_process_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    if result == 0 {
        return true;
    }
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    errno == libc::EPERM
}

#[test]
fn test_is_process_alive_current_process() {
    let pid = std::process::id();
    assert!(is_process_alive(pid));
}

#[test]
fn test_is_process_alive_nonexistent() {
    // Use a very high PID that's unlikely to exist
    assert!(!is_process_alive(4000000));
}

#[test]
fn test_is_process_alive_init() {
    // PID 1 should always exist (init/systemd)
    assert!(is_process_alive(1));
}

// Test domain normalization logic
fn normalize_domain(domain: &str) -> String {
    if domain.contains('.') {
        domain.to_string()
    } else {
        format!("{}.localhost", domain)
    }
}

#[test]
fn test_normalize_domain_simple() {
    assert_eq!(normalize_domain("api"), "api.localhost");
    assert_eq!(normalize_domain("web"), "web.localhost");
    assert_eq!(normalize_domain("my-app"), "my-app.localhost");
}

#[test]
fn test_normalize_domain_already_full() {
    assert_eq!(normalize_domain("api.localhost"), "api.localhost");
    assert_eq!(normalize_domain("web.example.com"), "web.example.com");
}

#[test]
fn test_normalize_domain_empty() {
    assert_eq!(normalize_domain(""), ".localhost");
}

// Tests for config loading integration
use std::fs;
use tempfile::tempdir;
use unport_cli::config::Config;

#[test]
fn test_client_config_loading() {
    let dir = tempdir().unwrap();
    let config_content = r#"{"domain": "test-api"}"#;
    fs::write(dir.path().join("unport.json"), config_content).unwrap();

    let config = Config::load(dir.path()).unwrap();
    assert_eq!(config.domain, "test-api");
    assert_eq!(config.full_domain(), "test-api.localhost");
}

#[test]
fn test_client_config_with_start_command() {
    let dir = tempdir().unwrap();
    let config_content = r#"{"domain": "api", "start": "npm run dev"}"#;
    fs::write(dir.path().join("unport.json"), config_content).unwrap();

    let config = Config::load(dir.path()).unwrap();
    assert_eq!(config.start, Some("npm run dev".to_string()));
}

#[test]
fn test_client_config_port_strategy_env() {
    let dir = tempdir().unwrap();
    let config_content = r#"{"domain": "api", "portEnv": "SERVER_PORT"}"#;
    fs::write(dir.path().join("unport.json"), config_content).unwrap();

    let config = Config::load(dir.path()).unwrap();
    assert_eq!(config.port_env, Some("SERVER_PORT".to_string()));
}

#[test]
fn test_client_config_port_strategy_arg() {
    let dir = tempdir().unwrap();
    let config_content = r#"{"domain": "api", "portArg": "--port"}"#;
    fs::write(dir.path().join("unport.json"), config_content).unwrap();

    let config = Config::load(dir.path()).unwrap();
    assert_eq!(config.port_arg, Some("--port".to_string()));
}

// Tests for detection integration
use unport_cli::detect::{detect, PortStrategy};

#[test]
fn test_client_detection_nextjs() {
    let dir = tempdir().unwrap();
    let package_json = r#"{"dependencies": {"next": "13.0.0"}}"#;
    fs::write(dir.path().join("package.json"), package_json).unwrap();

    let result = detect(dir.path()).unwrap();
    assert_eq!(result.framework, "Next.js");
    assert_eq!(result.port_strategy, PortStrategy::EnvVar("PORT".into()));
}

#[test]
fn test_client_detection_vite() {
    let dir = tempdir().unwrap();
    let package_json = r#"{"devDependencies": {"vite": "4.0.0"}}"#;
    fs::write(dir.path().join("package.json"), package_json).unwrap();

    let result = detect(dir.path()).unwrap();
    assert_eq!(result.framework, "Vite");
    assert_eq!(result.port_strategy, PortStrategy::CliFlag("--port".into()));
}

#[test]
fn test_client_detection_rails() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("Gemfile"), "gem 'rails'").unwrap();

    let result = detect(dir.path()).unwrap();
    assert_eq!(result.framework, "Rails");
    assert_eq!(result.port_strategy, PortStrategy::CliFlag("-p".into()));
}
