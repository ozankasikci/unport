use anyhow::{Context, Result};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose, SanType};
use rustls_pemfile::{certs, private_key};
use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

use crate::log_info;
use crate::types::unport_dir;

/// Get the CA key path
pub fn ca_key_path() -> PathBuf {
    unport_dir().join("ca.key")
}

/// Get the CA cert path
pub fn ca_cert_path() -> PathBuf {
    unport_dir().join("ca.crt")
}

/// Get the certs directory
pub fn certs_dir() -> PathBuf {
    unport_dir().join("certs")
}

/// Get the localhost key path
pub fn localhost_key_path() -> PathBuf {
    certs_dir().join("localhost.key")
}

/// Get the localhost cert path
pub fn localhost_cert_path() -> PathBuf {
    certs_dir().join("localhost.crt")
}

/// Ensure the CA exists, creating it if necessary
pub fn ensure_ca() -> Result<()> {
    let key_path = ca_key_path();
    let cert_path = ca_cert_path();

    if key_path.exists() && cert_path.exists() {
        return Ok(());
    }

    // Generate CA key pair
    let key_pair = KeyPair::generate().context("Failed to generate CA key pair")?;

    // Configure CA certificate
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    params
        .distinguished_name
        .push(DnType::CommonName, "unport Local CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "unport");

    // Generate CA certificate
    let cert = params
        .self_signed(&key_pair)
        .context("Failed to generate CA certificate")?;

    // Write CA key and cert
    fs::write(&key_path, key_pair.serialize_pem()).context("Failed to write CA key")?;
    fs::write(&cert_path, cert.pem()).context("Failed to write CA cert")?;

    log_info!("CA certificate created at {:?}", cert_path);

    Ok(())
}

/// Generate a certificate with the given domains as SANs
pub fn generate_cert(domains: &[String]) -> Result<()> {
    let key_path = localhost_key_path();
    let cert_path = localhost_cert_path();

    // Ensure certs directory exists
    fs::create_dir_all(certs_dir()).context("Failed to create certs directory")?;

    // Load CA key
    let ca_key_pem = fs::read_to_string(ca_key_path()).context("Failed to read CA key")?;
    let ca_key_pair = KeyPair::from_pem(&ca_key_pem).context("Failed to parse CA key")?;

    // Recreate CA cert params for signing
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "unport Local CA");
    ca_params
        .distinguished_name
        .push(DnType::OrganizationName, "unport");
    let ca_cert = ca_params
        .self_signed(&ca_key_pair)
        .context("Failed to reconstruct CA cert")?;

    // Generate server key pair
    let server_key_pair = KeyPair::generate().context("Failed to generate server key pair")?;

    // Build SANs list
    // Note: *.localhost wildcard doesn't work in OpenSSL/LibreSSL because it requires
    // at least 2 dots after the wildcard (e.g., *.example.com works, *.localhost doesn't)
    // So we add explicit domain SANs for each registered service
    let mut sans: Vec<SanType> = vec![
        SanType::DnsName("localhost".try_into().unwrap()),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
    ];

    // Add each domain explicitly
    for domain in domains {
        if let Ok(name) = domain.as_str().try_into() {
            sans.push(SanType::DnsName(name));
        }
    }

    let mut params = CertificateParams::default();
    params.subject_alt_names = sans;
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "unport");

    // Sign with CA
    let server_cert = params
        .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
        .context("Failed to sign server certificate")?;

    // Write server key and cert
    fs::write(&key_path, server_key_pair.serialize_pem()).context("Failed to write server key")?;
    fs::write(&cert_path, server_cert.pem()).context("Failed to write server cert")?;

    if domains.is_empty() {
        log_info!("TLS certificate generated for: localhost");
    } else {
        log_info!("TLS certificate generated for: localhost, {}", domains.join(", "));
    }

    Ok(())
}

/// Ensure a basic cert exists (for initial startup)
pub fn ensure_cert() -> Result<()> {
    let key_path = localhost_key_path();
    let cert_path = localhost_cert_path();

    if key_path.exists() && cert_path.exists() {
        return Ok(());
    }

    generate_cert(&[])
}

/// Load TLS configuration for the HTTPS server
pub fn load_tls_config() -> Result<TlsAcceptor> {
    let cert_path = localhost_cert_path();
    let key_path = localhost_key_path();
    let ca_path = ca_cert_path();

    // Load server certificate
    let cert_file = fs::File::open(&cert_path).context("Failed to open cert file")?;
    let mut cert_reader = BufReader::new(cert_file);
    let mut cert_chain: Vec<CertificateDer<'static>> = certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse certificates")?;

    // Load CA certificate and add to chain (required for clients to verify)
    let ca_file = fs::File::open(&ca_path).context("Failed to open CA cert file")?;
    let mut ca_reader = BufReader::new(ca_file);
    let ca_certs: Vec<CertificateDer<'static>> = certs(&mut ca_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to parse CA certificate")?;
    cert_chain.extend(ca_certs);

    // Load private key
    let key_file = fs::File::open(&key_path).context("Failed to open key file")?;
    let mut key_reader = BufReader::new(key_file);
    let key: PrivateKeyDer<'static> = private_key(&mut key_reader)
        .context("Failed to parse private key")?
        .context("No private key found")?;

    // Build TLS config
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .context("Failed to build TLS config")?;

    log_info!("TLS configuration loaded");
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Initialize TLS (ensure CA and cert exist, return acceptor)
pub fn init_tls() -> Result<TlsAcceptor> {
    ensure_ca()?;
    ensure_cert()?;
    load_tls_config()
}

/// Delete generated certificates (forces regeneration on next daemon start)
pub fn clean_certs() -> Result<()> {
    let cert_path = localhost_cert_path();
    let key_path = localhost_key_path();

    let mut deleted = false;

    if cert_path.exists() {
        fs::remove_file(&cert_path).context("Failed to delete certificate")?;
        println!("Deleted: {:?}", cert_path);
        deleted = true;
    }

    if key_path.exists() {
        fs::remove_file(&key_path).context("Failed to delete key")?;
        println!("Deleted: {:?}", key_path);
        deleted = true;
    }

    if deleted {
        println!("✓ Certificates cleaned. They will be regenerated on next daemon start with --https.");
    } else {
        println!("No certificates to clean.");
    }

    Ok(())
}

/// Add or remove the CA certificate from the system trust store
pub fn trust_ca(remove: bool) -> Result<()> {
    let ca_path = ca_cert_path();

    if !ca_path.exists() {
        anyhow::bail!(
            "CA certificate not found at {:?}. Start the daemon with --https first.",
            ca_path
        );
    }

    if remove {
        remove_ca_from_trust_store(&ca_path)
    } else {
        add_ca_to_trust_store(&ca_path)
    }
}

#[cfg(target_os = "macos")]
fn add_ca_to_trust_store(ca_path: &std::path::Path) -> Result<()> {
    use std::process::Command;

    println!("Adding CA to macOS system trust store...");

    // First, remove any existing certificate with the same name to avoid conflicts
    let _ = Command::new("security")
        .args([
            "delete-certificate",
            "-c",
            "unport Local CA",
            "/Library/Keychains/System.keychain",
        ])
        .output();

    // Add the certificate with explicit SSL trust policy
    let status = Command::new("security")
        .args([
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-p",
            "ssl",
            "-p",
            "basic",
            "-k",
            "/Library/Keychains/System.keychain",
        ])
        .arg(ca_path)
        .status()
        .context("Failed to run security command")?;

    if !status.success() {
        anyhow::bail!("Failed to add CA to trust store. Make sure you're running with sudo.");
    }

    println!("✓ CA added to system trust store");
    println!("✓ https://*.localhost is now trusted");

    Ok(())
}

#[cfg(target_os = "macos")]
fn remove_ca_from_trust_store(ca_path: &std::path::Path) -> Result<()> {
    use std::process::Command;

    println!("Removing CA from macOS system trust store...");

    let status = Command::new("security")
        .args(["remove-trusted-cert", "-d"])
        .arg(ca_path)
        .status()
        .context("Failed to run security command")?;

    if !status.success() {
        anyhow::bail!("Failed to remove CA from trust store. Make sure you're running with sudo.");
    }

    println!("✓ CA removed from system trust store");

    Ok(())
}

#[cfg(target_os = "linux")]
fn add_ca_to_trust_store(ca_path: &std::path::Path) -> Result<()> {
    use std::process::Command;

    println!("Adding CA to Linux system trust store...");

    let dest = std::path::Path::new("/usr/local/share/ca-certificates/unport-ca.crt");
    fs::copy(ca_path, dest).context("Failed to copy CA certificate. Run with sudo.")?;

    let status = Command::new("update-ca-certificates")
        .status()
        .context("Failed to run update-ca-certificates")?;

    if !status.success() {
        anyhow::bail!("Failed to update CA certificates");
    }

    println!("✓ CA added to system trust store");
    println!("✓ https://*.localhost is now trusted");

    try_add_to_firefox_nss(ca_path);

    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_ca_from_trust_store(_ca_path: &std::path::Path) -> Result<()> {
    use std::process::Command;

    println!("Removing CA from Linux system trust store...");

    let dest = std::path::Path::new("/usr/local/share/ca-certificates/unport-ca.crt");
    if dest.exists() {
        fs::remove_file(dest).context("Failed to remove CA certificate. Run with sudo.")?;
    }

    let status = Command::new("update-ca-certificates")
        .status()
        .context("Failed to run update-ca-certificates")?;

    if !status.success() {
        anyhow::bail!("Failed to update CA certificates");
    }

    println!("✓ CA removed from system trust store");

    Ok(())
}

#[cfg(target_os = "linux")]
fn try_add_to_firefox_nss(ca_path: &std::path::Path) {
    use std::process::Command;

    if Command::new("certutil").arg("--version").output().is_err() {
        println!("Note: Install libnss3-tools to trust CA in Firefox");
        return;
    }

    let home = dirs::home_dir().unwrap_or_default();
    let firefox_dir = home.join(".mozilla/firefox");

    if !firefox_dir.exists() {
        return;
    }

    if let Ok(entries) = fs::read_dir(&firefox_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("cert9.db").exists() {
                let _ = Command::new("certutil")
                    .args(["-A", "-n", "unport CA", "-t", "C,,", "-i"])
                    .arg(ca_path)
                    .args(["-d"])
                    .arg(&path)
                    .status();
                println!("✓ CA added to Firefox profile at {:?}", path);
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn add_ca_to_trust_store(ca_path: &std::path::Path) -> Result<()> {
    println!("Automatic trust store installation not supported on this OS.");
    println!("Please manually trust the CA certificate at: {:?}", ca_path);
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_ca_from_trust_store(_ca_path: &std::path::Path) -> Result<()> {
    println!("Automatic trust store removal not supported on this OS.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose, SanType};
    use std::fs;
    use tempfile::tempdir;
    use x509_parser::prelude::*;

    // Tests for path functions
    #[test]
    fn test_ca_key_path() {
        let path = ca_key_path();
        assert!(path.to_string_lossy().contains(".unport"));
        assert!(path.to_string_lossy().ends_with("ca.key"));
    }

    #[test]
    fn test_ca_cert_path() {
        let path = ca_cert_path();
        assert!(path.to_string_lossy().contains(".unport"));
        assert!(path.to_string_lossy().ends_with("ca.crt"));
    }

    #[test]
    fn test_certs_dir() {
        let path = certs_dir();
        assert!(path.to_string_lossy().contains(".unport"));
        assert!(path.to_string_lossy().ends_with("certs"));
    }

    #[test]
    fn test_localhost_key_path() {
        let path = localhost_key_path();
        assert!(path.to_string_lossy().contains("certs"));
        assert!(path.to_string_lossy().ends_with("localhost.key"));
    }

    #[test]
    fn test_localhost_cert_path() {
        let path = localhost_cert_path();
        assert!(path.to_string_lossy().contains("certs"));
        assert!(path.to_string_lossy().ends_with("localhost.crt"));
    }

    #[test]
    fn test_path_consistency() {
        // Multiple calls should return the same path
        assert_eq!(ca_key_path(), ca_key_path());
        assert_eq!(ca_cert_path(), ca_cert_path());
        assert_eq!(certs_dir(), certs_dir());
        assert_eq!(localhost_key_path(), localhost_key_path());
        assert_eq!(localhost_cert_path(), localhost_cert_path());
    }

    #[test]
    fn test_localhost_paths_under_certs_dir() {
        let certs = certs_dir();
        assert!(localhost_key_path().starts_with(&certs));
        assert!(localhost_cert_path().starts_with(&certs));
    }

    fn parse_pem(input: &str) -> Result<::pem::Pem, ::pem::PemError> {
        ::pem::parse(input)
    }

    #[test]
    fn test_ca_generation() {
        let dir = tempdir().unwrap();
        let key_path = dir.path().join("ca.key");
        let cert_path = dir.path().join("ca.crt");

        // Generate CA
        let key_pair = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "Test CA");

        let cert = params.self_signed(&key_pair).unwrap();

        fs::write(&key_path, key_pair.serialize_pem()).unwrap();
        fs::write(&cert_path, cert.pem()).unwrap();

        assert!(key_path.exists());
        assert!(cert_path.exists());

        // Verify PEM format
        let key_content = fs::read_to_string(&key_path).unwrap();
        let cert_content = fs::read_to_string(&cert_path).unwrap();
        assert!(key_content.contains("BEGIN PRIVATE KEY"));
        assert!(cert_content.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn test_generate_cert_with_explicit_domains() {
        let dir = tempdir().unwrap();

        // Set up test environment
        std::env::set_var("HOME", dir.path());
        let unport_dir = dir.path().join(".unport");
        fs::create_dir_all(&unport_dir).unwrap();
        let certs_dir = unport_dir.join("certs");
        fs::create_dir_all(&certs_dir).unwrap();

        // Generate CA first
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "unport Local CA");
        ca_params
            .distinguished_name
            .push(DnType::OrganizationName, "unport");
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        fs::write(unport_dir.join("ca.key"), ca_key_pair.serialize_pem()).unwrap();
        fs::write(unport_dir.join("ca.crt"), ca_cert.pem()).unwrap();

        // Generate server certificate with explicit domains
        let domains = vec![
            "game-analytics-api.localhost".to_string(),
            "dashboard.localhost".to_string(),
            "api.localhost".to_string(),
        ];

        let server_key_pair = KeyPair::generate().unwrap();

        let mut sans: Vec<SanType> = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
            SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        ];
        for domain in &domains {
            sans.push(SanType::DnsName(domain.as_str().try_into().unwrap()));
        }

        let mut params = CertificateParams::default();
        params.subject_alt_names = sans;
        params
            .distinguished_name
            .push(DnType::CommonName, "localhost");

        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();
        fs::write(certs_dir.join("localhost.crt"), &cert_pem).unwrap();
        fs::write(certs_dir.join("localhost.key"), server_key_pair.serialize_pem()).unwrap();

        // Parse the certificate and verify SANs
        let pem = parse_pem(&cert_pem).unwrap();
        let (_, cert) = X509Certificate::from_der(pem.contents()).unwrap();

        // Get Subject Alternative Names
        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .expect("Certificate should have SAN extension");

        let san = match san_ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => san,
            _ => panic!("Expected SubjectAlternativeName"),
        };

        let dns_names: Vec<&str> = san
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(*dns),
                _ => None,
            })
            .collect();

        // Verify all expected domains are in the SANs
        assert!(dns_names.contains(&"localhost"), "Should contain localhost");
        assert!(
            dns_names.contains(&"game-analytics-api.localhost"),
            "Should contain game-analytics-api.localhost"
        );
        assert!(
            dns_names.contains(&"dashboard.localhost"),
            "Should contain dashboard.localhost"
        );
        assert!(
            dns_names.contains(&"api.localhost"),
            "Should contain api.localhost"
        );
    }

    #[test]
    fn test_cert_with_hyphenated_subdomain() {
        // This test verifies that domains with hyphens work correctly
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
            SanType::DnsName("my-cool-app.localhost".try_into().unwrap()),
            SanType::DnsName("another-hyphenated-domain.localhost".try_into().unwrap()),
        ];

        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();

        // Parse and verify
        let pem = parse_pem(&cert_pem).unwrap();
        let (_, cert) = X509Certificate::from_der(pem.contents()).unwrap();

        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .expect("Certificate should have SAN extension");

        let san = match san_ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => san,
            _ => panic!("Expected SubjectAlternativeName"),
        };

        let dns_names: Vec<&str> = san
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(*dns),
                _ => None,
            })
            .collect();

        assert!(dns_names.contains(&"my-cool-app.localhost"));
        assert!(dns_names.contains(&"another-hyphenated-domain.localhost"));
    }

    #[test]
    fn test_cert_includes_ip_address() {
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
            SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        ];

        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();

        // Parse and verify
        let pem = parse_pem(&cert_pem).unwrap();
        let (_, cert) = X509Certificate::from_der(pem.contents()).unwrap();

        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .expect("Certificate should have SAN extension");

        let san = match san_ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => san,
            _ => panic!("Expected SubjectAlternativeName"),
        };

        let has_ip = san.general_names.iter().any(|name| {
            matches!(name, GeneralName::IPAddress(ip) if ip == &[127, 0, 0, 1])
        });

        assert!(has_ip, "Certificate should include 127.0.0.1 IP address");
    }

    #[test]
    fn test_empty_domains_generates_localhost_only() {
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();

        // Simulate generate_cert with empty domains
        let domains: Vec<String> = vec![];
        let mut sans: Vec<SanType> = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
            SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        ];
        for domain in &domains {
            if let Ok(name) = domain.as_str().try_into() {
                sans.push(SanType::DnsName(name));
            }
        }

        let mut params = CertificateParams::default();
        params.subject_alt_names = sans;

        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();

        // Parse and verify
        let pem = parse_pem(&cert_pem).unwrap();
        let (_, cert) = X509Certificate::from_der(pem.contents()).unwrap();

        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .expect("Certificate should have SAN extension");

        let san = match san_ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => san,
            _ => panic!("Expected SubjectAlternativeName"),
        };

        let dns_names: Vec<&str> = san
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(*dns),
                _ => None,
            })
            .collect();

        assert_eq!(dns_names.len(), 1, "Should only have localhost");
        assert!(dns_names.contains(&"localhost"));
    }

    #[test]
    fn test_cert_with_many_domains() {
        // Test certificate generation with many domains (stress test)
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();

        // Generate 50 domains
        let mut sans: Vec<SanType> = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
            SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        ];

        for i in 0..50 {
            let domain = format!("service-{}.localhost", i);
            sans.push(SanType::DnsName(domain.as_str().try_into().unwrap()));
        }

        let mut params = CertificateParams::default();
        params.subject_alt_names = sans;

        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();

        // Parse and verify
        let pem = parse_pem(&cert_pem).unwrap();
        let (_, cert) = X509Certificate::from_der(pem.contents()).unwrap();

        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .expect("Certificate should have SAN extension");

        let san = match san_ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => san,
            _ => panic!("Expected SubjectAlternativeName"),
        };

        let dns_names: Vec<&str> = san
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(*dns),
                _ => None,
            })
            .collect();

        // Should have localhost + 50 services
        assert_eq!(dns_names.len(), 51, "Should have 51 DNS names");
        assert!(dns_names.contains(&"service-0.localhost"));
        assert!(dns_names.contains(&"service-49.localhost"));
    }

    #[test]
    fn test_cert_with_special_characters_in_subdomain() {
        // Test domains with numbers and valid special chars
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
            SanType::DnsName("app123.localhost".try_into().unwrap()),
            SanType::DnsName("my-app-v2.localhost".try_into().unwrap()),
            SanType::DnsName("test--double-dash.localhost".try_into().unwrap()),
        ];

        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();
        let pem = parse_pem(&cert_pem).unwrap();
        let (_, cert) = X509Certificate::from_der(pem.contents()).unwrap();

        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .unwrap();

        let san = match san_ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => san,
            _ => panic!("Expected SubjectAlternativeName"),
        };

        let dns_names: Vec<&str> = san
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(*dns),
                _ => None,
            })
            .collect();

        assert!(dns_names.contains(&"app123.localhost"));
        assert!(dns_names.contains(&"my-app-v2.localhost"));
        assert!(dns_names.contains(&"test--double-dash.localhost"));
    }

    #[test]
    fn test_cert_with_long_subdomain() {
        // Test with a long but valid subdomain (max label is 63 chars)
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();

        // 63 character subdomain (max allowed)
        let long_subdomain = "a".repeat(63);
        let long_domain = format!("{}.localhost", long_subdomain);

        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
            SanType::DnsName(long_domain.as_str().try_into().unwrap()),
        ];

        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();
        let pem = parse_pem(&cert_pem).unwrap();
        let (_, cert) = X509Certificate::from_der(pem.contents()).unwrap();

        let san_ext = cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
            .unwrap();

        let san = match san_ext.parsed_extension() {
            ParsedExtension::SubjectAlternativeName(san) => san,
            _ => panic!("Expected SubjectAlternativeName"),
        };

        let dns_names: Vec<&str> = san
            .general_names
            .iter()
            .filter_map(|name| match name {
                GeneralName::DNSName(dns) => Some(*dns),
                _ => None,
            })
            .collect();

        assert!(dns_names.contains(&long_domain.as_str()));
    }

    #[test]
    fn test_ca_certificate_properties() {
        // Verify CA certificate has correct properties
        let key_pair = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params
            .distinguished_name
            .push(DnType::CommonName, "unport Local CA");
        params
            .distinguished_name
            .push(DnType::OrganizationName, "unport");

        let cert = params.self_signed(&key_pair).unwrap();
        let cert_pem = cert.pem();

        let pem = parse_pem(&cert_pem).unwrap();
        let (_, x509_cert) = X509Certificate::from_der(pem.contents()).unwrap();

        // Verify it's a CA certificate
        let basic_constraints = x509_cert
            .extensions()
            .iter()
            .find(|ext| ext.oid == x509_parser::oid_registry::OID_X509_EXT_BASIC_CONSTRAINTS);

        assert!(basic_constraints.is_some(), "CA cert should have BasicConstraints");

        // Verify subject
        let subject = x509_cert.subject();
        let cn = subject.iter_common_name().next();
        assert!(cn.is_some());
    }

    #[test]
    fn test_server_cert_signed_by_ca() {
        // Verify server certificate is properly signed by CA
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Test CA");
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();
        let mut server_params = CertificateParams::default();
        server_params.subject_alt_names = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
        ];
        server_params
            .distinguished_name
            .push(DnType::CommonName, "localhost");

        let server_cert = server_params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let server_pem = server_cert.pem();
        let ca_pem = ca_cert.pem();

        // Parse both certificates
        let server_parsed = parse_pem(&server_pem).unwrap();
        let ca_parsed = parse_pem(&ca_pem).unwrap();

        let (_, server_x509) = X509Certificate::from_der(server_parsed.contents()).unwrap();
        let (_, ca_x509) = X509Certificate::from_der(ca_parsed.contents()).unwrap();

        // Server cert issuer should match CA subject
        assert_eq!(
            server_x509.issuer(),
            ca_x509.subject(),
            "Server cert issuer should match CA subject"
        );
    }

    #[test]
    fn test_duplicate_domains_handled() {
        // Test that duplicate domains don't cause issues
        let ca_key_pair = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key_pair).unwrap();

        let server_key_pair = KeyPair::generate().unwrap();

        // Add same domain twice (simulating edge case)
        let domains = vec![
            "api.localhost".to_string(),
            "api.localhost".to_string(), // duplicate
            "web.localhost".to_string(),
        ];

        let mut sans: Vec<SanType> = vec![
            SanType::DnsName("localhost".try_into().unwrap()),
        ];

        for domain in &domains {
            if let Ok(name) = domain.as_str().try_into() {
                sans.push(SanType::DnsName(name));
            }
        }

        let mut params = CertificateParams::default();
        params.subject_alt_names = sans;

        // Should not panic with duplicates
        let server_cert = params
            .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
            .unwrap();

        let cert_pem = server_cert.pem();
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
    }
}
