use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, KeyPair, KeyUsagePurpose, NameConstraints, SanType,
};
use std::io::BufReader;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::Arc;
use time::OffsetDateTime;
use tokio_rustls::rustls;

/// Returns `~/.aztec-accelerator/certs/`.
pub fn certs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aztec-accelerator")
        .join("certs")
}

fn ca_cert_path() -> PathBuf {
    certs_dir().join("ca.pem")
}

fn ca_key_path() -> PathBuf {
    certs_dir().join("ca.key")
}

fn leaf_cert_path() -> PathBuf {
    certs_dir().join("localhost.pem")
}

fn leaf_key_path() -> PathBuf {
    certs_dir().join("localhost.key")
}

/// Check whether all 4 PEM files exist.
pub fn certs_exist() -> bool {
    ca_cert_path().exists()
        && ca_key_path().exists()
        && leaf_cert_path().exists()
        && leaf_key_path().exists()
}

/// Generate CA + leaf certificates and write them to disk.
/// Idempotent: skips if all files already exist.
pub fn generate_and_save() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if certs_exist() {
        tracing::info!("Certificates already exist, skipping generation");
        return Ok(());
    }

    let dir = certs_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let now = OffsetDateTime::now_utc();
    let ten_years = time::Duration::days(3650);
    let leaf_validity = time::Duration::days(825); // Apple TLS maximum

    // ── CA certificate (10-year validity, Name Constraints) ──
    let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;

    let mut ca_params = CertificateParams::default();
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Aztec Accelerator Local CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params.not_before = now;
    ca_params.not_after = now + ten_years;
    ca_params.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![
            GeneralSubtree::IpAddress(CidrSubnet::V4([127, 0, 0, 1], [255, 255, 255, 255])),
            GeneralSubtree::IpAddress(CidrSubnet::V6(
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                [
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ],
            )),
            GeneralSubtree::DnsName("localhost".into()),
        ],
        excluded_subtrees: vec![],
    });

    let ca_cert = ca_params.self_signed(&ca_key)?;

    // ── Leaf certificate (825 days — Apple's TLS maximum) ──
    let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;

    let mut leaf_params = CertificateParams::default();
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf_params.is_ca = IsCa::NoCa;
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params.subject_alt_names = vec![
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        SanType::DnsName("localhost".try_into()?),
    ];
    leaf_params.not_before = now;
    leaf_params.not_after = now + leaf_validity;

    let leaf_cert = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key)?;

    // ── Write files with restricted permissions ──
    write_pem_file(&ca_cert_path(), &ca_cert.pem())?;
    write_pem_file(&ca_key_path(), &ca_key.serialize_pem())?;
    write_pem_file(&leaf_cert_path(), &leaf_cert.pem())?;
    write_pem_file(&leaf_key_path(), &leaf_key.serialize_pem())?;

    tracing::info!(dir = %dir.display(), "Generated CA + leaf certificates");
    Ok(())
}

/// Write a PEM file with 0o600 permissions (owner read/write only).
fn write_pem_file(
    path: &std::path::Path,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Load the leaf cert + key from PEM files and build a rustls ServerConfig.
pub fn load_rustls_config(
) -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
    let cert_pem = std::fs::read(leaf_cert_path())?;
    let key_pem = std::fs::read(leaf_key_path())?;

    let certs: Vec<_> =
        rustls_pemfile::certs(&mut BufReader::new(&cert_pem[..])).collect::<Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(&key_pem[..]))?
        .ok_or("no private key found in PEM file")?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(Arc::new(config))
}

/// Approximate days remaining on the leaf certificate.
/// Uses file modification time as a proxy for creation date.
/// Parse the leaf certificate's notAfter field and return days until expiry.
/// Uses the actual X.509 certificate, not file mtime (which can be wrong if
/// the file is copied, restored from backup, or touched).
pub fn leaf_cert_days_remaining() -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let pem_bytes = std::fs::read(leaf_cert_path())?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&pem_bytes)?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)?;
    let not_after = cert.validity().not_after.timestamp();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    Ok((not_after - now) / 86400)
}

/// Regenerate the leaf certificate if it's expiring within 30 days.
/// Uses the existing CA to re-sign, so no new trust prompt is needed.
pub fn regenerate_leaf_if_expiring() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match leaf_cert_days_remaining() {
        Ok(days) if days > 30 => {
            tracing::debug!(days_remaining = days, "Leaf cert not expiring soon");
            return Ok(());
        }
        Ok(days) => {
            tracing::info!(
                days_remaining = days,
                "Leaf cert expiring soon, regenerating"
            );
        }
        Err(e) => {
            tracing::warn!("Could not check leaf cert expiry: {e}, regenerating");
        }
    }

    // Load existing CA from PEM and re-sign
    let ca_key_pem = std::fs::read_to_string(ca_key_path())?;
    let ca_key = KeyPair::from_pem(&ca_key_pem)?;

    let ca_cert_pem = std::fs::read_to_string(ca_cert_path())?;
    let ca_params = CertificateParams::from_ca_cert_pem(&ca_cert_pem)?;
    let ca_cert = ca_params.self_signed(&ca_key)?;

    // Generate new leaf
    let now = OffsetDateTime::now_utc();
    let leaf_validity = time::Duration::days(825);

    let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let mut leaf_params = CertificateParams::default();
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf_params.is_ca = IsCa::NoCa;
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params.subject_alt_names = vec![
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        SanType::DnsName("localhost".try_into()?),
    ];
    leaf_params.not_before = now;
    leaf_params.not_after = now + leaf_validity;

    let leaf_cert = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key)?;

    write_pem_file(&leaf_cert_path(), &leaf_cert.pem())?;
    write_pem_file(&leaf_key_path(), &leaf_key.serialize_pem())?;

    tracing::info!("Regenerated leaf certificate");
    Ok(())
}

// ── macOS trust management ──

/// Install the CA certificate in the macOS login Keychain.
/// Returns Ok(()) on success, Err on failure (user cancelled or other error).
#[cfg(target_os = "macos")]
pub fn install_ca_trust() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ca_path = ca_cert_path();
    let output = std::process::Command::new("security")
        .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
        .arg(
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Library/Keychains/login.keychain-db"),
        )
        .arg(&ca_path)
        .output()?;

    if output.status.success() {
        tracing::info!("CA certificate installed in macOS login Keychain");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!(%stderr, "Failed to install CA trust");
        Err(format!("security add-trusted-cert failed: {stderr}").into())
    }
}

/// Check whether the CA certificate is still trusted in the macOS Keychain.
#[cfg(target_os = "macos")]
pub fn is_ca_trusted() -> bool {
    let ca_path = ca_cert_path();
    if !ca_path.exists() {
        return false;
    }
    let output = std::process::Command::new("security")
        .args(["verify-cert", "-c"])
        .arg(&ca_path)
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Stub for non-macOS platforms — trust management is macOS-only.
#[cfg(not(target_os = "macos"))]
pub fn install_ca_trust() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Err("CA trust installation is only supported on macOS".into())
}

/// Stub for non-macOS platforms.
#[cfg(not(target_os = "macos"))]
pub fn is_ca_trusted() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certs_dir_is_under_home() {
        let dir = certs_dir();
        assert!(dir.to_string_lossy().contains(".aztec-accelerator/certs"));
    }

    #[test]
    fn generate_ca_and_leaf_certs() {
        let now = OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Test CA");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.not_before = now;
        ca_params.not_after = now + time::Duration::days(3650);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut leaf_params = CertificateParams::default();
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
        leaf_params.not_before = now;
        leaf_params.not_after = now + time::Duration::days(825);

        let leaf_cert = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key).unwrap();

        // Verify PEM output is valid
        assert!(ca_cert.pem().starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(ca_key
            .serialize_pem()
            .starts_with("-----BEGIN PRIVATE KEY-----"));
        assert!(leaf_cert.pem().starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(leaf_key
            .serialize_pem()
            .starts_with("-----BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn leaf_cert_loads_into_rustls() {
        // Install a default crypto provider — needed when both aws-lc-rs and ring are available
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
        let now = OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Test CA");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.not_before = now;
        ca_params.not_after = now + time::Duration::days(3650);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut leaf_params = CertificateParams::default();
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
        leaf_params.not_before = now;
        leaf_params.not_after = now + time::Duration::days(825);

        let leaf_cert = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key).unwrap();

        let cert_pem = leaf_cert.pem();
        let key_pem = leaf_key.serialize_pem();

        let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(cert_pem.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(certs.len(), 1);

        let key = rustls_pemfile::private_key(&mut BufReader::new(key_pem.as_bytes()))
            .unwrap()
            .unwrap();

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key);
        assert!(config.is_ok(), "rustls config should build successfully");
    }

    #[test]
    fn write_pem_file_sets_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.pem");
        write_pem_file(&path, "test content").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "test content");
    }
}
