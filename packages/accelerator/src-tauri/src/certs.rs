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
use zeroize::Zeroizing;

/// Returns `~/.aztec-accelerator/certs/`.
pub fn certs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aztec-accelerator")
        .join("certs")
}

/// The legacy on-disk CA private key path. NOT part of [`CertPaths`] — the keyless-CA design never
/// writes it; this exists only so `migrate_legacy_ca_key` can delete one left by older installs.
fn ca_key_path() -> PathBuf {
    certs_dir().join("ca.key")
}

/// The trio of TLS artifact paths that always travel together: CA cert, leaf cert, leaf key. Bundling
/// them kills the 3×`&Path` arg-swap foot-gun (all the same type) + the basenames that were
/// duplicated across the accessors, the staging set, and the swap. (F-07)
struct CertPaths {
    ca_cert: PathBuf,
    leaf_cert: PathBuf,
    leaf_key: PathBuf,
}

impl CertPaths {
    /// The live served set under `certs_dir()` (`ca.pem` / `localhost.pem` / `localhost.key`).
    fn live() -> Self {
        let dir = certs_dir();
        Self {
            ca_cert: dir.join("ca.pem"),
            leaf_cert: dir.join("localhost.pem"),
            leaf_key: dir.join("localhost.key"),
        }
    }

    /// The staged set (`*.new`) under `dir`, written + (macOS) trusted before the atomic swap.
    fn staged(dir: &std::path::Path) -> Self {
        Self {
            ca_cert: dir.join("ca.pem.new"),
            leaf_cert: dir.join("localhost.pem.new"),
            leaf_key: dir.join("localhost.key.new"),
        }
    }

    /// True iff all three files exist (presence only — validity is checked by the caller).
    fn exists(&self) -> bool {
        self.ca_cert.exists() && self.leaf_cert.exists() && self.leaf_key.exists()
    }

    /// Best-effort remove all three (used to discard a failed staging).
    /// Only the macOS trust-failure path discards a staged set, so this is
    /// macOS-only — gating it keeps Linux/Windows clippy `-D warnings` clean.
    #[cfg(target_os = "macos")]
    fn remove(&self) {
        let _ = std::fs::remove_file(&self.ca_cert);
        let _ = std::fs::remove_file(&self.leaf_cert);
        let _ = std::fs::remove_file(&self.leaf_key);
    }

    /// Atomically rename this (staged) set over `live`, preserving order ca → leaf → key.
    fn swap_into(&self, live: &CertPaths) -> std::io::Result<()> {
        std::fs::rename(&self.ca_cert, &live.ca_cert)?;
        std::fs::rename(&self.leaf_cert, &live.leaf_cert)?;
        std::fs::rename(&self.leaf_key, &live.leaf_key)?;
        Ok(())
    }
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
    CertPaths::live().exists() && leaf_cert_days_remaining().map(|d| d > 0).unwrap_or(false)
}

/// Generate a CA + leaf and write the CA cert + leaf cert + leaf key to the three given paths.
/// The CA private key is generated in memory, signs the leaf, and is dropped at function end —
/// **never written to disk.** Writing to caller-chosen paths lets rotation stage a new set
/// (`*.new`) and atomically swap it in only after the new anchor is trusted.
fn write_new_cert_set(paths: &CertPaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = OffsetDateTime::now_utc();
    // F-016: wrap the CA signing key in `Zeroizing` so rcgen scrubs its serialized-DER copy on drop (a
    // plain drop scrubs nothing), and drop it as EARLY as possible — right after it signs the leaf, before
    // the fallible file writes.
    let ca_key = Zeroizing::new(KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?);
    let ca_cert = ca_params(now).self_signed(&ca_key)?;
    let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let leaf_cert = leaf_params(now)?.signed_by(&leaf_key, &ca_cert, &ca_key)?;
    // Scrub rcgen's serialized-DER CA key now. RESIDUAL (F-016): `Zeroizing` wipes ONLY that `Vec` — the
    // ring backend's ECDSA scalar/nonce, key-generation temporaries, swap pages, and any core dump are NOT
    // scrubbed, so this is best-effort post-use reduction, not a guarantee the CA key is unrecoverable. The
    // CA key is never written to disk; the leaf key is persisted at 0600 by design.
    drop(ca_key);

    write_pem_file(&paths.ca_cert, &ca_cert.pem())?;
    write_pem_file(&paths.leaf_cert, &leaf_cert.pem())?;
    write_pem_file(&paths.leaf_key, &leaf_key.serialize_pem())?;
    Ok(())
}

/// Generate the live CA + leaf into the standard paths (ca.pem + localhost.pem/.key). No CA key.
fn generate_certs() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dir = certs_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    write_new_cert_set(&CertPaths::live())?;
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
pub fn migrate_legacy_ca_key() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    migrate_legacy_ca_key_at(&ca_key_path())
}

/// Inner, path-parameterized for testability. **SEC-08, fail-closed:** returns `Err` if `ca.key`
/// still exists after the removal attempt (retried once for a transient lock/AV scan). The caller
/// MUST treat that as a security failure and NOT bring up Safari HTTPS — a live HTTPS server next to
/// a readable mint-any-cert key + its still-trusted anchor is the exact exposure we're closing.
fn migrate_legacy_ca_key_at(
    ca_key: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !ca_key.exists() {
        return Ok(()); // never had one / already gone — the common path
    }
    for attempt in 0..2 {
        match std::fs::remove_file(ca_key) {
            Ok(_) => break,
            Err(e) if attempt == 1 => {
                tracing::error!(error = %e, "Failed to delete legacy ca.key after retry");
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    // Re-check: fail closed if it persists (a failed remove_file, an immutable flag, a perms issue).
    if ca_key.exists() {
        return Err(
            "legacy ca.key persists after removal attempt — the readable mint-any-cert key is still \
             on disk; refusing to proceed"
                .into(),
        );
    }
    tracing::warn!(
        "Removed legacy on-disk CA key (ca.key) — the mint-any-cert primitive is gone. The legacy \
         keychain CA anchor (now keyless) remains; use Settings to fully remove it."
    );
    Ok(())
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
    let live = CertPaths::live();
    let cert_pem = std::fs::read(&live.leaf_cert)?;
    let key_pem = std::fs::read(&live.leaf_key)?;

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
    let pem_bytes = std::fs::read(CertPaths::live().leaf_cert)?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&pem_bytes)?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)?;
    let not_after = cert.validity().not_after.timestamp();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    Ok((not_after - now) / 86400)
}

/// Rotate ~30 days before the leaf expires — while the old leaf still serves, leaving a window to
/// prompt for the new anchor's trust before HTTPS would otherwise break.
const ROTATE_BEFORE_DAYS: i64 = 30;

/// Rotate the cert identity if the served leaf is within the pre-expiry window (≤30 days). Delegates
/// to `rotate()`, which is safe + non-silent.
pub fn regenerate_leaf_if_expiring() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match leaf_cert_days_remaining() {
        Ok(days) if days > ROTATE_BEFORE_DAYS => {
            tracing::debug!(days_remaining = days, "Leaf cert not expiring soon");
            return Ok(());
        }
        Ok(days) => tracing::info!(days_remaining = days, "Leaf cert expiring soon — rotating"),
        Err(e) => tracing::warn!("Could not check leaf cert expiry: {e}; rotating"),
    }
    rotate()
}

/// Rotate the whole cert identity. The previous CA's key was discarded (never on disk), so we cannot
/// re-sign under it — we generate a FRESH keyless CA + leaf.
///
/// **Fail-closed + non-silent:** the new set is STAGED (`*.new`), then (macOS) trusted + verified
/// BEFORE it replaces the live certs. A cancelled/failed trust prompt discards the staging and leaves
/// the old, still-valid certs serving — no outage, never an untrusted cert. Sequence:
/// stage → add-new-anchor (prompt) → verify → atomic swap → remove-old-anchor (by SHA-1).
fn rotate() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dir = certs_dir();
    std::fs::create_dir_all(&dir)?;
    let staged = CertPaths::staged(&dir);

    write_new_cert_set(&staged)?;

    // Capture the OLD anchor's SHA-1 before touching the keychain, for removal after the swap.
    #[cfg(target_os = "macos")]
    let old_sha1 = ca_keychain_sha1();

    // macOS: trust + verify the NEW anchor BEFORE swapping. Fail-closed — discard staging, keep live.
    #[cfg(target_os = "macos")]
    if add_trusted_cert(&staged.ca_cert).is_err() || !verify_cert_trusted(&staged.ca_cert) {
        staged.remove();
        return Err("new CA cert could not be trusted — kept the existing certs".into());
    }

    // Atomic swap: the new set replaces the live certs. Trust is content-keyed, so rename keeps it.
    staged.swap_into(&CertPaths::live())?;

    // Remove the OLD anchor now that the NEW one is live + trusted (no keyless-anchor accumulation).
    #[cfg(target_os = "macos")]
    if let Some(sha1) = old_sha1 {
        remove_trusted_cert_by_sha1(&sha1);
    }

    tracing::info!("Rotated cert identity (fresh keyless CA + leaf); trust re-installed");
    Ok(())
}

// ── macOS trust management ──

// ── macOS trust management ──

#[cfg(target_os = "macos")]
fn login_keychain() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Keychains/login.keychain-db")
}

/// Add a cert as a trusted root in the macOS login Keychain (prompts the user). Used for both the
/// initial Safari-enable and rotation (the new keyless CA anchor). Trust is keyed to the cert's
/// content, so a later atomic rename of the file does not invalidate it.
#[cfg(target_os = "macos")]
fn add_trusted_cert(
    cert_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let output = std::process::Command::new("security")
        .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
        .arg(login_keychain())
        .arg(cert_path)
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

/// Whether the given cert verifies as trusted.
#[cfg(target_os = "macos")]
fn verify_cert_trusted(cert_path: &std::path::Path) -> bool {
    cert_path.exists()
        && std::process::Command::new("security")
            .args(["verify-cert", "-c"])
            .arg(cert_path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// SHA-1 of the currently-installed "Aztec Accelerator Local CA" anchor (if any) — captured before
/// rotation so the OLD anchor can be removed after the NEW one is installed (keyless anchors must not
/// accumulate). Returns the first match.
#[cfg(target_os = "macos")]
fn ca_keychain_sha1() -> Option<String> {
    let output = std::process::Command::new("security")
        .args(["find-certificate", "-Z", "-c", "Aztec Accelerator Local CA"])
        .arg(login_keychain())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|l| l.trim().strip_prefix("SHA-1 hash:"))
        .map(|h| h.trim().to_string())
}

/// Best-effort removal of a trusted cert by SHA-1 (the old anchor, after rotation swapped in the new).
#[cfg(target_os = "macos")]
fn remove_trusted_cert_by_sha1(sha1: &str) {
    match std::process::Command::new("security")
        .args(["delete-certificate", "-Z", sha1])
        .arg(login_keychain())
        .output()
    {
        Ok(o) if o.status.success() => tracing::info!(sha1, "Removed old CA anchor after rotation"),
        Ok(o) => {
            tracing::warn!(stderr = %String::from_utf8_lossy(&o.stderr), "Could not remove old CA anchor (left in keychain)")
        }
        Err(e) => tracing::warn!(error = %e, "Could not run delete-certificate for the old anchor"),
    }
}

/// Install the live CA cert (`ca.pem`) as a trusted root. Public entry for the initial Safari-enable.
#[cfg(target_os = "macos")]
pub fn install_ca_trust() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    add_trusted_cert(&CertPaths::live().ca_cert)
}

/// Whether the live CA cert is still trusted in the macOS Keychain.
#[cfg(target_os = "macos")]
pub fn is_ca_trusted() -> bool {
    verify_cert_trusted(&CertPaths::live().ca_cert)
}

// Non-macOS stubs — trust management is macOS-only.
#[cfg(not(target_os = "macos"))]
pub fn install_ca_trust() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Err("CA trust installation is only supported on macOS".into())
}

#[cfg(not(target_os = "macos"))]
pub fn is_ca_trusted() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CA / leaf validity used by the test fixtures, mirroring production (`generate_certs`).
    const TEST_CA_VALIDITY_DAYS: i64 = 3650;
    const TEST_LEAF_VALIDITY_DAYS: i64 = 825;

    /// Build a self-signed test CA + a `localhost` leaf signed by it. Dedups the cert-building
    /// boilerplate shared by `generate_ca_and_leaf_certs` and `leaf_cert_loads_into_rustls`.
    fn build_test_ca_and_leaf() -> (rcgen::Certificate, KeyPair, rcgen::Certificate, KeyPair) {
        let now = OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Test CA");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.not_before = now;
        ca_params.not_after = now + time::Duration::days(TEST_CA_VALIDITY_DAYS);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let leaf_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut leaf_params = CertificateParams::default();
        leaf_params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
        leaf_params.not_before = now;
        leaf_params.not_after = now + time::Duration::days(TEST_LEAF_VALIDITY_DAYS);
        let leaf_cert = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key).unwrap();

        (ca_cert, ca_key, leaf_cert, leaf_key)
    }

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
        let (ca_cert, ca_key, leaf_cert, leaf_key) = build_test_ca_and_leaf();

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
        let (_ca_cert, _ca_key, leaf_cert, leaf_key) = build_test_ca_and_leaf();

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

    #[test]
    fn generation_writes_no_ca_key() {
        let tmp = tempfile::tempdir().unwrap();
        let ca = tmp.path().join("ca.pem");
        let leaf = tmp.path().join("localhost.pem");
        let key = tmp.path().join("localhost.key");

        write_new_cert_set(&CertPaths {
            ca_cert: ca.clone(),
            leaf_cert: leaf.clone(),
            leaf_key: key.clone(),
        })
        .unwrap();

        assert!(ca.exists(), "ca.pem (anchor) should be written");
        assert!(
            leaf.exists() && key.exists(),
            "leaf cert + key should be written"
        );
        // THE security invariant: the CA signing key must NEVER hit disk.
        assert!(
            !tmp.path().join("ca.key").exists(),
            "ca.key must never be written — it is the mint-any-cert primitive"
        );

        // The written leaf must be a usable served identity.
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
        let cert_pem = std::fs::read(&leaf).unwrap();
        let key_pem = std::fs::read(&key).unwrap();
        let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(&cert_pem[..]))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let pk = rustls_pemfile::private_key(&mut BufReader::new(&key_pem[..]))
            .unwrap()
            .unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, pk);
        assert!(config.is_ok(), "served leaf should build a rustls config");
    }

    #[test]
    fn migrate_deletes_legacy_ca_key_but_keeps_certs() {
        let tmp = tempfile::tempdir().unwrap();
        let ca_key = tmp.path().join("ca.key");
        let leaf = tmp.path().join("localhost.pem");
        std::fs::write(&ca_key, "legacy key").unwrap();
        std::fs::write(&leaf, "leaf cert").unwrap();

        migrate_legacy_ca_key_at(&ca_key)
            .expect("removal of an existing legacy key should succeed");

        assert!(!ca_key.exists(), "legacy ca.key must be deleted");
        assert!(leaf.exists(), "the served leaf must be untouched");

        // Idempotent: a second call on an absent key is Ok (no panic, no error).
        migrate_legacy_ca_key_at(&ca_key).expect("absent key is Ok");
    }

    /// SEC-08: if the legacy key cannot be removed, migration FAILS (so the caller skips Safari HTTPS)
    /// rather than proceeding with the readable mint-any-cert key still on disk.
    #[cfg(unix)]
    #[test]
    fn migrate_fails_closed_when_key_cannot_be_removed() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("locked");
        std::fs::create_dir(&dir).unwrap();
        let ca_key = dir.join("ca.key");
        std::fs::write(&ca_key, "legacy key").unwrap();
        // Read+execute only on the PARENT dir → `remove_file` inside it fails (needs dir-write).
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();

        let result = migrate_legacy_ca_key_at(&ca_key);

        // Restore perms so the tempdir can clean up.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            result.is_err(),
            "must fail closed when the legacy key can't be removed"
        );
    }
}
