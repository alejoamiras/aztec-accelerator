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

/// 824 days — one day under Apple's inclusive 825-day TLS-server-cert cap (applies even to
/// user-trusted certs; see implementations-plan/safari-tls-ca-removal-2026-06-04).
const LEAF_VALIDITY_DAYS: i64 = 824;
/// CA anchor validity. The CA is keyless on disk, so this only bounds how long the anchor is valid;
/// the leaf's 824-day cap drives rotation well before this.
const CA_VALIDITY_DAYS: i64 = 3650;

/// Params for the CA anchor cert. Its signing key is generated per-call and **discarded** right after
/// it signs the leaf — no CA private key is ever written to disk, so the trusted anchor cannot mint
/// any other cert (closes the audit HIGH).
fn ca_params(now: OffsetDateTime) -> CertificateParams {
    let mut p = CertificateParams::default();
    p.distinguished_name
        .push(DnType::CommonName, "Aztec Accelerator Local CA");
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    p.not_before = now;
    p.not_after = now + time::Duration::days(CA_VALIDITY_DAYS);
    p.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![
            GeneralSubtree::IpAddress(CidrSubnet::V4([127, 0, 0, 1], [255, 255, 255, 255])),
            GeneralSubtree::IpAddress(CidrSubnet::V6(
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                [255; 16],
            )),
            GeneralSubtree::DnsName("localhost".into()),
        ],
        excluded_subtrees: vec![],
    });
    p
}

/// Params for the served leaf cert (the one the HTTPS server presents).
fn leaf_params(
    now: OffsetDateTime,
) -> Result<CertificateParams, Box<dyn std::error::Error + Send + Sync>> {
    let mut p = CertificateParams::default();
    p.distinguished_name.push(DnType::CommonName, "localhost");
    p.is_ca = IsCa::NoCa;
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    p.subject_alt_names = vec![
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        SanType::DnsName("localhost".try_into()?),
    ];
    p.not_before = now;
    p.not_after = now + time::Duration::days(LEAF_VALIDITY_DAYS);
    Ok(p)
}

/// Whether a usable cert set exists: the CA anchor + leaf cert/key are present AND the leaf parses
/// and is not expired. Validity-checked (not just `.exists()`) so a corrupt/expired/half-written leaf
/// triggers regeneration instead of being skipped forever. Note: `ca.key` is intentionally NOT
/// required — it is never written.
pub fn certs_exist() -> bool {
    ca_cert_path().exists()
        && leaf_cert_path().exists()
        && leaf_key_path().exists()
        && leaf_cert_days_remaining().map(|d| d > 0).unwrap_or(false)
}

/// Generate the CA + leaf and persist `ca.pem` (anchor) + `localhost.pem`/`.key` (served).
/// The CA private key is generated in memory, signs the leaf, and is dropped at function end —
/// **never written to disk.**
fn generate_certs() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dir = certs_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let now = OffsetDateTime::now_utc();
    let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let ca_cert = ca_params(now).self_signed(&ca_key)?;
    let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let leaf_cert = leaf_params(now)?.signed_by(&leaf_key, &ca_cert, &ca_key)?;

    // Write the CA CERT (trusted anchor) + leaf cert + leaf key — but NOT the CA key.
    write_pem_file(&ca_cert_path(), &ca_cert.pem())?;
    write_pem_file(&leaf_cert_path(), &leaf_cert.pem())?;
    write_pem_file(&leaf_key_path(), &leaf_key.serialize_pem())?;
    // `ca_key` drops here — the only copy of the CA signing key is gone.
    tracing::info!(dir = %dir.display(), "Generated CA + leaf (CA signing key discarded, not written)");
    Ok(())
}

/// Generate certs if a valid set doesn't already exist. Idempotent.
pub fn generate_and_save() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if certs_exist() {
        tracing::info!("Valid certificates already exist, skipping generation");
        return Ok(());
    }
    generate_certs()
}

/// Delete a legacy on-disk CA private key (`ca.key`) left by older installs — it is the readable
/// mint-any-cert primitive (audit HIGH). The CA *cert* anchor stays trusted but, with no key, can
/// sign nothing. Idempotent; safe on installs that never had one.
pub fn migrate_legacy_ca_key() {
    let p = ca_key_path();
    if p.exists() {
        match std::fs::remove_file(&p) {
            Ok(_) => tracing::warn!(
                "Removed legacy on-disk CA key (ca.key) — the mint-any-cert primitive is gone. The \
                 legacy keychain CA anchor (now keyless) remains; use Settings to fully remove it."
            ),
            Err(e) => {
                tracing::error!(error = %e, "Failed to delete legacy ca.key — the readable minting key persists")
            }
        }
    }
}

/// Write a PEM file **atomically** with `0o600` perms: write a temp sibling (owner-only), fsync, then
/// rename over the target. Avoids both a world-readable TOCTOU window and a truncate-in-place crash that
/// would leave a corrupt-but-present PEM (which `certs_exist`'s validity check would then reject).
fn write_pem_file(
    path: &std::path::Path,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Write;
    // Distinct temp name per file (e.g. `localhost.key.tmp`) so concurrent/sequential writes of
    // `.pem` and `.key` siblings can't collide on one temp path.
    let file_name = path.file_name().ok_or("cert path has no file name")?;
    let tmp = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));
    {
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?
        };
        #[cfg(not(unix))]
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
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
/// Rotate ~30 days before the leaf expires — while the old leaf still serves, leaving a window to
/// prompt for the new anchor's trust before HTTPS would otherwise break.
const ROTATE_BEFORE_DAYS: i64 = 30;

/// Rotate the cert identity if the served leaf is within the pre-expiry window.
///
/// The previous CA's signing key was discarded (it is never written to disk), so we cannot re-sign a
/// new leaf under the old CA — we regenerate the WHOLE identity (a fresh keyless CA + leaf). The new
/// CA is a new trust anchor, so on macOS this **re-installs trust** (a user prompt — rotation is NOT
/// silent). Order: regenerate → install new trust → (return Err if trust not granted, so the caller
/// keeps serving the still-valid old leaf rather than an untrusted new one).
///
/// TODO (Phase 2, needs real-macOS validation): after installing the new anchor, remove the OLD CA
/// anchor by its SHA-1 so keyless anchors don't accumulate across rotations; and gate rotation to
/// interactive sessions so a headless startup defers (keeps the old leaf) instead of prompting.
pub fn regenerate_leaf_if_expiring() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match leaf_cert_days_remaining() {
        Ok(days) if days > ROTATE_BEFORE_DAYS => {
            tracing::debug!(days_remaining = days, "Leaf cert not expiring soon");
            return Ok(());
        }
        Ok(days) => tracing::info!(
            days_remaining = days,
            "Leaf cert expiring soon — rotating (fresh keyless CA + leaf)"
        ),
        Err(e) => tracing::warn!("Could not check leaf cert expiry: {e}, rotating"),
    }

    generate_certs()?;

    #[cfg(target_os = "macos")]
    if let Err(e) = install_ca_trust() {
        tracing::warn!(
            "Rotated certs but re-trust was not granted; keeping HTTPS off until trusted: {e}"
        );
        return Err(e);
    }

    tracing::info!("Rotated cert identity (fresh keyless CA + leaf) and re-installed trust");
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
        // Separator-agnostic: compare path components, not a "/"-joined string.
        let tail: std::path::PathBuf = [".aztec-accelerator", "certs"].iter().collect();
        assert!(
            dir.ends_with(&tail),
            "certs_dir {dir:?} should end with {tail:?}"
        );
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
