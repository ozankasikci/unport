use unport_cli::detect::PortStrategy;
use unport_cli::process::spawn_app;

#[test]
fn test_spawn_app_with_env_var() {
    // Use a simple command that exits immediately
    let result = spawn_app(
        "true",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        None,
        None,
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_spawn_app_with_cli_flag() {
    let result = spawn_app(
        "true",
        4000,
        &PortStrategy::CliFlag("--port".into()),
        None,
        None,
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_spawn_app_with_env_override() {
    let result = spawn_app(
        "true",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        Some("CUSTOM_PORT"),
        None,
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    child.wait().unwrap();
}

#[test]
fn test_spawn_app_with_arg_override() {
    let result = spawn_app(
        "true",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        None,
        Some("-p"),
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    child.wait().unwrap();
}

#[test]
fn test_spawn_app_django_style_flag() {
    // Django uses special "0.0.0.0:" flag where port is appended directly
    let result = spawn_app(
        "true",
        4000,
        &PortStrategy::CliFlag("0.0.0.0:".into()),
        None,
        None,
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    child.wait().unwrap();
}

#[test]
fn test_spawn_app_with_args() {
    let result = spawn_app(
        "echo hello world",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        None,
        None,
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_spawn_app_empty_command() {
    let result = spawn_app(
        "",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        None,
        None,
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Empty command"));
}

#[test]
fn test_spawn_app_complex_command() {
    let result = spawn_app(
        "echo one two three",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        None,
        None,
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    child.wait().unwrap();
}

#[test]
fn test_spawn_app_nonexistent_command() {
    let result = spawn_app(
        "nonexistent_command_12345",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        None,
        None,
    );
    // Should fail to spawn
    assert!(result.is_err());
}

#[test]
fn test_spawn_app_different_ports() {
    for port in [3000, 4000, 5000, 8000, 8080] {
        let result = spawn_app(
            "true",
            port,
            &PortStrategy::EnvVar("PORT".into()),
            None,
            None,
        );
        assert!(result.is_ok());
        let mut child = result.unwrap();
        child.wait().unwrap();
    }
}

#[test]
fn test_spawn_app_env_override_takes_precedence() {
    // Even with CliFlag strategy, env override should be used
    let result = spawn_app(
        "true",
        4000,
        &PortStrategy::CliFlag("--port".into()),
        Some("MY_PORT"),
        None,
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    child.wait().unwrap();
}

#[test]
fn test_spawn_app_arg_override_takes_precedence() {
    // Even with EnvVar strategy, arg override should be used
    let result = spawn_app(
        "true",
        4000,
        &PortStrategy::EnvVar("PORT".into()),
        None,
        Some("--listen"),
    );
    assert!(result.is_ok());
    let mut child = result.unwrap();
    child.wait().unwrap();
}
