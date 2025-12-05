// TLS module tests
// These tests verify the TLS path functions and cert generation
// Note: Tests that use environment-dependent functions (ensure_ca, generate_cert)
// need to run with --test-threads=1 to avoid race conditions

use unport_cli::tls::{
    ca_cert_path, ca_key_path, certs_dir, localhost_cert_path, localhost_key_path,
};

// Tests for path functions - these don't modify state and can run in parallel

#[test]
fn test_ca_key_path_contains_expected_components() {
    let path = ca_key_path();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains(".unport"), "Should contain .unport directory");
    assert!(path_str.ends_with("ca.key"), "Should end with ca.key");
}

#[test]
fn test_ca_cert_path_contains_expected_components() {
    let path = ca_cert_path();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains(".unport"), "Should contain .unport directory");
    assert!(path_str.ends_with("ca.crt"), "Should end with ca.crt");
}

#[test]
fn test_certs_dir_contains_expected_components() {
    let path = certs_dir();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains(".unport"), "Should contain .unport directory");
    assert!(path_str.ends_with("certs"), "Should end with certs");
}

#[test]
fn test_localhost_key_path_contains_expected_components() {
    let path = localhost_key_path();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("certs"), "Should contain certs directory");
    assert!(path_str.ends_with("localhost.key"), "Should end with localhost.key");
}

#[test]
fn test_localhost_cert_path_contains_expected_components() {
    let path = localhost_cert_path();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("certs"), "Should contain certs directory");
    assert!(path_str.ends_with("localhost.crt"), "Should end with localhost.crt");
}

#[test]
fn test_path_functions_return_consistent_values() {
    // Multiple calls should return same paths
    let key1 = ca_key_path();
    let key2 = ca_key_path();
    assert_eq!(key1, key2, "ca_key_path should be consistent");

    let cert1 = ca_cert_path();
    let cert2 = ca_cert_path();
    assert_eq!(cert1, cert2, "ca_cert_path should be consistent");

    let certs1 = certs_dir();
    let certs2 = certs_dir();
    assert_eq!(certs1, certs2, "certs_dir should be consistent");
}

#[test]
fn test_localhost_paths_are_in_certs_dir() {
    let certs = certs_dir();
    let key = localhost_key_path();
    let cert = localhost_cert_path();

    assert!(key.starts_with(&certs), "localhost.key should be in certs dir");
    assert!(cert.starts_with(&certs), "localhost.crt should be in certs dir");
}

#[test]
fn test_ca_paths_are_in_unport_dir() {
    let ca_key = ca_key_path();
    let ca_cert = ca_cert_path();

    // Both should be siblings (in the same .unport directory)
    assert_eq!(
        ca_key.parent(),
        ca_cert.parent(),
        "CA key and cert should be in same directory"
    );
}

#[test]
fn test_certs_dir_is_under_unport_dir() {
    let ca_key = ca_key_path();
    let certs = certs_dir();

    // certs dir should be a sibling of ca.key (both under .unport)
    let unport_dir = ca_key.parent().expect("CA key should have parent");
    assert!(
        certs.starts_with(unport_dir),
        "certs dir should be under .unport"
    );
}

// Tests for path structure without relying on specific HOME value
#[test]
fn test_path_structure_is_valid() {
    let ca_key = ca_key_path();
    let ca_cert = ca_cert_path();
    let certs = certs_dir();
    let localhost_key = localhost_key_path();
    let localhost_cert = localhost_cert_path();

    // All paths should be absolute
    assert!(ca_key.is_absolute(), "CA key path should be absolute");
    assert!(ca_cert.is_absolute(), "CA cert path should be absolute");
    assert!(certs.is_absolute(), "Certs dir should be absolute");
    assert!(localhost_key.is_absolute(), "Localhost key should be absolute");
    assert!(localhost_cert.is_absolute(), "Localhost cert should be absolute");
}

#[test]
fn test_file_extensions() {
    let ca_key = ca_key_path();
    let ca_cert = ca_cert_path();
    let localhost_key = localhost_key_path();
    let localhost_cert = localhost_cert_path();

    assert_eq!(ca_key.extension().map(|s| s.to_str()), Some(Some("key")));
    assert_eq!(ca_cert.extension().map(|s| s.to_str()), Some(Some("crt")));
    assert_eq!(localhost_key.extension().map(|s| s.to_str()), Some(Some("key")));
    assert_eq!(localhost_cert.extension().map(|s| s.to_str()), Some(Some("crt")));
}

#[test]
fn test_file_names() {
    let ca_key = ca_key_path();
    let ca_cert = ca_cert_path();
    let localhost_key = localhost_key_path();
    let localhost_cert = localhost_cert_path();

    assert_eq!(ca_key.file_name().map(|s| s.to_str()), Some(Some("ca.key")));
    assert_eq!(ca_cert.file_name().map(|s| s.to_str()), Some(Some("ca.crt")));
    assert_eq!(localhost_key.file_name().map(|s| s.to_str()), Some(Some("localhost.key")));
    assert_eq!(localhost_cert.file_name().map(|s| s.to_str()), Some(Some("localhost.crt")));
}
