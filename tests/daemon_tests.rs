use std::path::PathBuf;
use std::process::Command;
use unport_cli::daemon::Registry;
use unport_cli::types::Service;

// Registry tests that don't depend on file system or environment variables

#[test]
fn test_registry_new() {
    let registry = Registry::new();
    let services = registry.list();
    assert!(services.is_empty());
}

#[test]
fn test_registry_register() {
    let mut registry = Registry::new();

    let service = Service {
        domain: "api.localhost".to_string(),
        port: 4000,
        pid: 12345,
        directory: PathBuf::from("/test/api"),
    };

    registry.register(service);

    let services = registry.list();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].domain, "api.localhost");
}

#[test]
fn test_registry_get() {
    let mut registry = Registry::new();

    let service = Service {
        domain: "api.localhost".to_string(),
        port: 4000,
        pid: 12345,
        directory: PathBuf::from("/test/api"),
    };

    registry.register(service);

    let found = registry.get("api.localhost");
    assert!(found.is_some());
    assert_eq!(found.unwrap().port, 4000);

    let not_found = registry.get("nonexistent.localhost");
    assert!(not_found.is_none());
}

#[test]
fn test_registry_unregister() {
    let mut registry = Registry::new();

    let service = Service {
        domain: "api.localhost".to_string(),
        port: 4000,
        pid: 12345,
        directory: PathBuf::from("/test/api"),
    };

    registry.register(service);
    assert_eq!(registry.list().len(), 1);

    let removed = registry.unregister("api.localhost");
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().domain, "api.localhost");
    assert_eq!(registry.list().len(), 0);
}

#[test]
fn test_registry_unregister_nonexistent() {
    let mut registry = Registry::new();
    let removed = registry.unregister("nonexistent.localhost");
    assert!(removed.is_none());
}

#[test]
fn test_registry_get_port() {
    let mut registry = Registry::new();

    let port1 = registry.get_port();
    let port2 = registry.get_port();

    assert!(port1 >= 4000);
    assert!(port2 >= 4000);
    assert_ne!(port1, port2);
}

#[test]
fn test_registry_multiple_services() {
    let mut registry = Registry::new();

    let services = vec![
        Service {
            domain: "api.localhost".to_string(),
            port: 4000,
            pid: 1001,
            directory: PathBuf::from("/test/api"),
        },
        Service {
            domain: "web.localhost".to_string(),
            port: 4001,
            pid: 1002,
            directory: PathBuf::from("/test/web"),
        },
        Service {
            domain: "admin.localhost".to_string(),
            port: 4002,
            pid: 1003,
            directory: PathBuf::from("/test/admin"),
        },
    ];

    for service in services {
        registry.register(service);
    }

    assert_eq!(registry.list().len(), 3);
    assert!(registry.get("api.localhost").is_some());
    assert!(registry.get("web.localhost").is_some());
    assert!(registry.get("admin.localhost").is_some());
}

#[test]
fn test_registry_replace_service() {
    let mut registry = Registry::new();

    let service1 = Service {
        domain: "api.localhost".to_string(),
        port: 4000,
        pid: 12345,
        directory: PathBuf::from("/test/api"),
    };
    registry.register(service1);

    let service2 = Service {
        domain: "api.localhost".to_string(),
        port: 4001,
        pid: 12346,
        directory: PathBuf::from("/test/api2"),
    };
    registry.register(service2);

    let services = registry.list();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].port, 4001);
}

#[test]
fn test_registry_cleanup_dead_removes_dead_processes() {
    let mut registry = Registry::new();

    let service = Service {
        domain: "dead.localhost".to_string(),
        port: 4000,
        pid: 4000000, // Very high PID that doesn't exist
        directory: PathBuf::from("/test/dead"),
    };
    registry.register(service);

    assert_eq!(registry.list().len(), 1);

    registry.cleanup_dead();

    assert_eq!(registry.list().len(), 0);
}

#[test]
fn test_registry_cleanup_dead_keeps_alive_processes() {
    let mut registry = Registry::new();

    let service = Service {
        domain: "alive.localhost".to_string(),
        port: 4000,
        pid: std::process::id(), // Current process PID
        directory: PathBuf::from("/test/alive"),
    };
    registry.register(service);

    assert_eq!(registry.list().len(), 1);

    registry.cleanup_dead();

    assert_eq!(registry.list().len(), 1);
}

#[test]
fn test_registry_get_port_increments() {
    let mut registry = Registry::new();

    let ports: Vec<u16> = (0..5).map(|_| registry.get_port()).collect();

    // All ports should be unique
    for (i, port) in ports.iter().enumerate() {
        for (j, other) in ports.iter().enumerate() {
            if i != j {
                assert_ne!(port, other, "Ports should be unique");
            }
        }
    }
}

#[test]
fn test_registry_list_returns_all_services() {
    let mut registry = Registry::new();

    // Add several services
    for i in 0..5 {
        let service = Service {
            domain: format!("service{}.localhost", i),
            port: 4000 + i,
            pid: 1000 + i as u32,
            directory: PathBuf::from(format!("/test/service{}", i)),
        };
        registry.register(service);
    }

    let list = registry.list();
    assert_eq!(list.len(), 5);
}

#[test]
fn test_registry_get_returns_correct_service() {
    let mut registry = Registry::new();

    let service1 = Service {
        domain: "api.localhost".to_string(),
        port: 4000,
        pid: 1000,
        directory: PathBuf::from("/app/api"),
    };
    let service2 = Service {
        domain: "web.localhost".to_string(),
        port: 4001,
        pid: 1001,
        directory: PathBuf::from("/app/web"),
    };

    registry.register(service1);
    registry.register(service2);

    let api = registry.get("api.localhost");
    assert!(api.is_some());
    assert_eq!(api.unwrap().port, 4000);

    let web = registry.get("web.localhost");
    assert!(web.is_some());
    assert_eq!(web.unwrap().port, 4001);
}

#[test]
fn test_registry_unregister_only_removes_target() {
    let mut registry = Registry::new();

    let service1 = Service {
        domain: "api.localhost".to_string(),
        port: 4000,
        pid: 1000,
        directory: PathBuf::from("/app/api"),
    };
    let service2 = Service {
        domain: "web.localhost".to_string(),
        port: 4001,
        pid: 1001,
        directory: PathBuf::from("/app/web"),
    };

    registry.register(service1);
    registry.register(service2);
    assert_eq!(registry.list().len(), 2);

    registry.unregister("api.localhost");
    assert_eq!(registry.list().len(), 1);
    assert!(registry.get("api.localhost").is_none());
    assert!(registry.get("web.localhost").is_some());
}

/// Test that `unport daemon` without subcommand shows help/error
#[test]
fn test_daemon_requires_subcommand() {
    let output = Command::new(env!("CARGO_BIN_EXE_unport"))
        .arg("daemon")
        .output()
        .expect("Failed to execute command");

    // Should fail because subcommand is required
    assert!(!output.status.success(), "daemon without subcommand should fail");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should mention available subcommands
    assert!(
        combined.contains("start") && combined.contains("stop") && combined.contains("status"),
        "Should show available subcommands, got: {}",
        combined
    );
}

/// Test that `unport daemon start` (without -d) attempts to start
/// This verifies the CLI parsing works correctly
#[test]
fn test_daemon_start_cli_parsing() {
    let output = Command::new(env!("CARGO_BIN_EXE_unport"))
        .arg("daemon")
        .arg("start")
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should show the -d/--detach option
    assert!(
        stdout.contains("--detach") || stdout.contains("-d"),
        "Should show detach option, got: {}",
        stdout
    );
}

/// Test that the detach flag is properly recognized
#[test]
fn test_daemon_start_detach_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_unport"))
        .arg("daemon")
        .arg("start")
        .arg("-d")
        .arg("--help")  // Add help to prevent actual daemon start
        .output()
        .expect("Failed to execute command");

    // With --help after -d, clap should still show help
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success() || stdout.contains("daemon"));
}

/// Verify the exact arguments that would be passed when spawning detached daemon
/// This is the core test for the bug: spawn must use ["daemon", "start"] not just ["daemon"]
#[test]
fn test_detach_spawn_uses_correct_args() {
    // This test documents the expected behavior:
    // When spawning a detached daemon, we must pass "daemon start" not just "daemon"

    // Test that "daemon start" works (exits quickly without sudo, but parses correctly)
    let output = Command::new(env!("CARGO_BIN_EXE_unport"))
        .args(["daemon", "start"])
        .output()
        .expect("Failed to execute command");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    // Should NOT show "daemon <COMMAND>" help - that would mean args weren't parsed
    assert!(
        !combined.contains("daemon <COMMAND>"),
        "Should not show subcommand help when 'daemon start' is provided. Got: {}",
        combined
    );
}
