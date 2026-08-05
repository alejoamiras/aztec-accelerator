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

    /// The staged set under `dir`, written + trusted (per-OS via [`crate::trust`]) before the atomic
    /// swap. Staging names are unique PER PROCESS (`.new.<pid>`), not the fixed `*.new` they used to
    /// be: the lifecycle mutex only serializes rotations WITHIN one process, and a second resident
    /// instance (the crash-recovery relaunch can briefly overlap the outgoing one) would otherwise
    /// stage over the same three paths — interleaving two rotations into a mixed CA/leaf/key set and
    /// corrupting HTTPS (post-impl codex High). Distinct names make the worst case two independent
    /// stagings, one of which simply loses the swap.
    fn staged(dir: &std::path::Path) -> Self {
        let pid = std::process::id();
        Self {
            ca_cert: dir.join(format!("ca.pem.new.{pid}")),
            leaf_cert: dir.join(format!("localhost.pem.new.{pid}")),
            leaf_key: dir.join(format!("localhost.key.new.{pid}")),
        }
    }

    /// True iff all three files exist (presence only — validity is checked by the caller).
    fn exists(&self) -> bool {
        self.ca_cert.exists() && self.leaf_cert.exists() && self.leaf_key.exists()
    }

    /// Best-effort remove all three (used to discard a failed staging on any platform whose trust
    /// install rejected the new anchor).
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
        .push(DnType::CommonName, CA_COMMON_NAME);
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

/// The CN every anchor of ours carries. Also what the trust backends delete by.
pub const CA_COMMON_NAME: &str = "Aztec Accelerator Local CA";

/// Reject anything that is not the CA profile [`ca_params`] builds, BEFORE it reaches an OS trust
/// store (F-02, audit 2026-07-31).
///
/// `certs_dir()` is an ordinary user-writable directory. The install path read `ca.pem` and handed
/// the bytes straight to the platform trust store, and the only checks anywhere near it —
/// `certs_exist()` / `leaf_matches_ca()` — verify the set is SELF-CONSISTENT, which an attacker who
/// can write the directory trivially satisfies by planting a complete CA + leaf + key of their own.
/// The app would then install THEIR root: on macOS/Windows behind a trust dialog the user has every
/// reason to accept (it is the app they just clicked "Enable HTTPS" in), and on Linux silently into
/// the NSS databases, with no prompt at all.
///
/// What makes our anchor harmless is not that it is ours — it is the profile: a keyless CA whose
/// name constraints limit it to loopback, so it can vouch for nothing on the real internet. This
/// check is what makes that property load-bearing instead of merely true-by-habit. It cannot stop an
/// attacker who already runs as the user from installing a root by other means; it stops them from
/// using OUR consent ceremony to do it, which is the part we control.
///
/// Deliberately structural, not an equality check against a pinned blob: rotation generates a fresh
/// CA every ~2 years, so there is no fixed certificate to compare against.
pub fn validate_ca_profile(pem_bytes: &[u8]) -> Result<(), String> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(pem_bytes)
        .map_err(|e| format!("not a readable PEM certificate: {e}"))?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)
        .map_err(|e| format!("not a readable X.509 certificate: {e}"))?;

    let cn = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .unwrap_or_default()
        .to_string();
    if cn != CA_COMMON_NAME {
        return Err(format!(
            "subject CN is {cn:?}, not {CA_COMMON_NAME:?} — refusing to install a foreign CA"
        ));
    }

    match cert.basic_constraints() {
        Ok(Some(bc)) if bc.value.ca => {}
        _ => return Err("not a CA certificate (basicConstraints CA is not true)".into()),
    }

    match cert.key_usage() {
        Ok(Some(ku)) if ku.value.key_cert_sign() => {}
        _ => return Err("keyUsage does not include keyCertSign".into()),
    }

    // The load-bearing one. An anchor with NO name constraints — or with a permitted subtree outside
    // loopback — can vouch for any site on the internet once it is trusted.
    let nc = match cert.name_constraints() {
        Ok(Some(nc)) => nc,
        _ => return Err("no nameConstraints extension — an unconstrained root".into()),
    };
    if !nc.critical {
        return Err("nameConstraints is not marked critical, so a verifier may ignore it".into());
    }
    if nc
        .value
        .excluded_subtrees
        .as_ref()
        .is_some_and(|s| !s.is_empty())
    {
        return Err("unexpected excludedSubtrees in nameConstraints".into());
    }
    let permitted = nc
        .value
        .permitted_subtrees
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or("nameConstraints has no permittedSubtrees, which constrains nothing")?;
    for subtree in permitted {
        if !is_loopback_subtree(&subtree.base) {
            return Err(format!(
                "nameConstraints permits {:?}, which is outside loopback",
                subtree.base
            ));
        }
    }
    Ok(())
}

/// Is one permitted subtree confined to loopback? Mirrors [`ca_params`]'s three entries; the IP forms
/// are the RFC 5280 address-plus-mask encoding (4+4 bytes for v4, 16+16 for v6).
fn is_loopback_subtree(base: &x509_parser::extensions::GeneralName<'_>) -> bool {
    use x509_parser::extensions::GeneralName;
    const V4: &[u8] = &[127, 0, 0, 1, 255, 255, 255, 255];
    const V6: &[u8] = &[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, //
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ];
    match base {
        GeneralName::DNSName(name) => *name == "localhost",
        GeneralName::IPAddress(bytes) => *bytes == V4 || *bytes == V6,
        _ => false,
    }
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

/// Whether a usable cert set exists: the CA anchor + leaf cert/key are present, the leaf parses and is
/// not expired, AND the leaf+key are a matching pair (they load into a rustls `ServerConfig`).
/// Validity- AND consistency-checked (not just `.exists()`) so a corrupt/expired/half-written OR
/// mismatched set triggers regeneration instead of being skipped forever. The cross-file rename swap
/// (`swap_into`) isn't atomic across all three files, so a crash / Windows file-lock mid-swap can
/// leave a NEW leaf next to an OLD key (a mismatched pair) — `with_single_cert` rejects that, so this
/// returns `false` and the set is regenerated on the next enable, rather than `generate_and_save`
/// refusing forever and HTTPS being unrecoverable (post-impl codex High). `ca.pem` is not part of the
/// served identity (`load_rustls_config` uses only the leaf + key), so a lone stale `ca.pem` doesn't
/// gate here; it's re-installed on the next enable. `ca.key` is intentionally NOT required — never written.
pub fn certs_exist() -> bool {
    // Check the raw seconds-remaining, NOT days: `days > 0` truncates a leaf with <24h left to 0 and
    // would wrongly report a still-valid cert as unusable (post-impl review). `> 0` on seconds is exact.
    CertPaths::live().exists()
        && leaf_secs_remaining().map(|s| s > 0).unwrap_or(false)
        && load_rustls_config().is_ok()
        && leaf_matches_ca()
}

/// Is the live leaf actually SIGNED BY the live CA?
///
/// `load_rustls_config()` proves leaf↔key, and nothing more. `swap_into` renames three files in
/// sequence, so a crash (or a Windows file lock) between them can leave `ca = B` with `leaf/key = A` —
/// a set that passes every other check while chaining to an anchor that isn't installed. The listener
/// then records B's fingerprint and, after a trust removal + re-enable installs only B, the app
/// reports success while serving untrusted A (post-impl codex High). Verifying the signature is what
/// makes a partial swap detectable, so `generate_and_save` regenerates instead of adopting it.
fn leaf_matches_ca() -> bool {
    let live = CertPaths::live();
    let Ok(leaf_pem) = std::fs::read(&live.leaf_cert) else {
        return false;
    };
    let Ok(ca_pem) = std::fs::read(&live.ca_cert) else {
        return false;
    };
    let (Ok((_, leaf_pem)), Ok((_, ca_pem))) = (
        x509_parser::pem::parse_x509_pem(&leaf_pem),
        x509_parser::pem::parse_x509_pem(&ca_pem),
    ) else {
        return false;
    };
    let (Ok((_, leaf)), Ok((_, ca))) = (
        x509_parser::parse_x509_certificate(&leaf_pem.contents),
        x509_parser::parse_x509_certificate(&ca_pem.contents),
    ) else {
        return false;
    };
    // Fail closed: an unverifiable pair is treated as mismatched, so we regenerate rather than serve
    // something we cannot prove chains correctly.
    leaf.verify_signature(Some(ca.public_key())).is_ok()
}

/// Generate a CA + leaf and write the CA cert + leaf cert + leaf key to the three given paths.
/// The CA private key is generated in memory (`Zeroizing`), signs the leaf, and is dropped+scrubbed
/// EARLY — right after signing, before the file writes (F-016) — and is **never written to disk.**
/// Writing to caller-chosen paths lets rotation stage a new set
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
        // F-003 Windows tail: the leaf TLS key (localhost.key) is a real private key — write the temp with
        // an owner-only DACL (the SD travels with the same-volume rename). Clear any stale temp for
        // CREATE_NEW. Applies to the cert siblings too (harmless — they're per-user files).
        #[cfg(windows)]
        let mut file = {
            let _ = std::fs::remove_file(&tmp);
            accelerator_core::win_acl::secure_create_file(&tmp)?
        };
        #[cfg(all(not(unix), not(windows)))]
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
/// Seconds until the live leaf's notAfter (negative if already expired). Parsed from the actual X.509
/// certificate (not file mtime, which can be wrong after copy/restore/touch).
fn leaf_secs_remaining() -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let pem_bytes = std::fs::read(CertPaths::live().leaf_cert)?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&pem_bytes)?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents)?;
    let not_after = cert.validity().not_after.timestamp();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    Ok(not_after - now)
}

/// Days until the live leaf expires (floor of the seconds-based value). For logging + the rotation
/// window; validity checks use [`leaf_secs_remaining`] directly to avoid the sub-day truncation.
pub fn leaf_cert_days_remaining() -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    Ok(leaf_secs_remaining()? / 86400)
}

/// Rotate ~30 days before the leaf expires — while the old leaf still serves, leaving a window to
/// prompt for the new anchor's trust before HTTPS would otherwise break.
const ROTATE_BEFORE_DAYS: i64 = 30;

/// Rotate the cert identity if the served leaf is within the pre-expiry window (≤30 days). Delegates
/// to `rotate()`, which is safe + non-silent. Used for the **silent** background rotation on Linux
/// (user NSS needs no prompt); macOS/Windows instead surface a renewal consent window (see
/// [`leaf_is_expiring`] + the `renew_cert` command).
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

/// Whether the served leaf is within the pre-expiry rotation window (≤30 days). Drives the
/// macOS/Windows renewal consent window (§7) — a `true` here means the app should offer "Renew now"
/// rather than silently raising the OS trust dialog from a background thread.
pub fn leaf_is_expiring() -> bool {
    matches!(leaf_cert_days_remaining(), Ok(days) if days <= ROTATE_BEFORE_DAYS)
}

/// Public entry to rotate the cert identity now (the renewal window's "Renew now" button). Raises the
/// OS trust dialog with context (the user asked for it), unlike a surprise background prompt.
pub fn rotate_now() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    rotate()
}

/// Rotate the whole cert identity. The previous CA's key was discarded (never on disk), so we cannot
/// re-sign under it — we generate a FRESH keyless CA + leaf.
///
/// **Fail-closed + non-silent:** the new set is STAGED (`*.new`), then trusted + verified BEFORE it
/// replaces the live certs. A cancelled/failed trust discards the staging and leaves the old,
/// still-valid certs serving — no outage, never an untrusted cert. Per-OS trust lives in
/// [`crate::trust`].
///
/// **The OLD anchor is deliberately NOT removed here** (post-impl codex High). Rotation runs while
/// the HTTPS listener is already serving the OLD leaf from an in-memory `TlsAcceptor` that is not
/// reloaded — the rotated set only takes effect on the NEXT launch. Removing the old anchor now would
/// leave the still-served old leaf with no trusted anchor, breaking HTTPS until restart. So both
/// anchors stay trusted. The old anchor is **keyless** (can sign nothing) and **name-constrained to
/// loopback**, so a stale one is harmless; at most one accrues per rotation (~every 2 years — the
/// 824-day leaf minus the 30-day window), so lifetime accumulation is a handful. "Remove certificate
/// trust" (Settings) and the uninstaller clear them all by CN.
fn rotate() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dir = certs_dir();
    std::fs::create_dir_all(&dir)?;
    let staged = CertPaths::staged(&dir);

    write_new_cert_set(&staged)?;

    // F-02: validate what is actually ON DISK and trust THAT COPY. We generated it a moment ago, but
    // the staged path is in the same user-writable directory, so this closes the write→trust window.
    let vetted = match vetted_copy_of(&staged.ca_cert) {
        Ok(v) => v,
        Err(why) => {
            staged.remove();
            tracing::error!(%why, "SECURITY: staged CA certificate is not ours — rotation aborted");
            return Err(format!("staged CA certificate failed validation: {why}").into());
        }
    };

    // Trust + verify the NEW anchor BEFORE swapping. Fail-closed — discard staging, keep live.
    if let Err(e) = crate::trust::trust_new_anchor(vetted.path()) {
        staged.remove();
        return Err(
            format!("new CA cert could not be trusted — kept the existing certs: {e}").into(),
        );
    }

    // Atomic swap: the new set replaces the live certs. Trust is content-keyed, so rename keeps it.
    staged.swap_into(&CertPaths::live())?;

    tracing::info!(
        "Rotated cert identity (fresh keyless CA + leaf); new anchor trusted, takes effect next launch"
    );
    Ok(())
}

// ── Trust management (delegates to the per-OS `crate::trust` backend) ──

/// Install the live CA cert (`ca.pem`) as a trusted root in the platform's browser stores. `Err` iff
/// no store accepted it (the message carries the first store's failure detail — e.g. certutil missing).
pub fn install_ca_trust() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // F-02: never hand the OS trust store bytes we have not established are OUR profile. `ca.pem`
    // lives in a user-writable directory and every other check on it only proves self-consistency.
    let vetted = vetted_copy_of(&CertPaths::live().ca_cert)?;
    let report = crate::trust::install_ca_trust(vetted.path());
    if report.any_installed() {
        Ok(())
    } else {
        let detail = report
            .stores
            .iter()
            .find_map(|s| s.detail.clone())
            .unwrap_or_else(|| "certificate trust could not be installed".to_string());
        Err(detail.into())
    }
}

/// Read a CA cert, validate its profile, and return a private temp file holding **exactly the bytes
/// that were validated**, for handing to the OS trust tools.
///
/// The copy is the point (round 2, codex pass over F-02). Validating bytes and then passing the
/// original PATHNAME to `security` / `certutil` / NSS leaves a swap window: those tools re-open the
/// file, and the same-user attacker this fix is about — who must already be able to write
/// `certs_dir()` — can replace it in between. Linux's per-store loop opens it once PER STORE, which
/// widens the window further. Trusting a `tempfile`-created path instead removes the window: the name
/// is random and the handle is ours, so there is nothing to race.
///
/// The temp file lives until the returned guard drops, which must outlive the trust call — hence
/// returning it rather than a `PathBuf`. `tempfile` creates it `0600` in the system temp dir.
fn vetted_copy_of(
    ca_cert: &std::path::Path,
) -> Result<tempfile::NamedTempFile, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Write;
    let pem = std::fs::read(ca_cert)?;
    if let Err(why) = validate_ca_profile(&pem) {
        tracing::error!(%why, "SECURITY: refusing to install a CA certificate that is not ours");
        return Err(format!("refusing to install this CA certificate: {why}").into());
    }
    let mut f = tempfile::Builder::new()
        .prefix("aztec-accelerator-ca-")
        .suffix(".pem")
        .tempfile()?;
    f.write_all(&pem)?;
    f.flush()?;
    Ok(f)
}

/// Whether the live CA cert is trusted in at least one platform store.
pub fn is_ca_trusted() -> bool {
    crate::trust::is_ca_trusted(&CertPaths::live().ca_cert)
}

/// The live CA cert path — so callers (Settings "remove trust", the uninstall CLI) can hand it to the
/// trust backend without reaching into `CertPaths`.
pub fn live_ca_cert_path() -> PathBuf {
    CertPaths::live().ca_cert
}

/// Content fingerprint (SHA-256 hex of the DER) of the CA cert currently ON DISK. `None` if it can't
/// be read or parsed.
///
/// Used to answer a question `https_bound` cannot: *is the running listener still serving the CURRENT
/// cert identity?* A rotation swaps the files but the in-memory `TlsAcceptor` keeps serving the OLD
/// leaf until relaunch, so "bound" does not imply "serving what's on disk" (post-impl codex Medium).
pub fn live_ca_fingerprint() -> Option<String> {
    ca_fingerprint_at(&CertPaths::live().ca_cert)
}

fn ca_fingerprint_at(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&bytes).ok()?;
    Some(hex::encode(Sha256::digest(&pem.contents)))
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

    // ── F-02 regression (audit 2026-07-31) ────────────────────────────────────────────────────
    // `certs_dir()` is user-writable and the install path handed `ca.pem` straight to the OS trust
    // store. `certs_exist()` / `leaf_matches_ca()` only prove the set is SELF-consistent, which an
    // attacker who can write the directory satisfies by planting a complete CA + leaf + key of their
    // own — so the app installed THEIR root, silently on Linux. These fail against the pre-fix code,
    // where no profile check existed at all.

    /// The anchor we actually ship must pass our own gate. This is also what pins the assumptions the
    /// validator makes about `ca_params` — including that rcgen marks nameConstraints CRITICAL.
    #[test]
    fn our_own_generated_ca_passes_validation() {
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let ca = ca_params(OffsetDateTime::now_utc())
            .self_signed(&key)
            .unwrap();
        validate_ca_profile(ca.pem().as_bytes()).expect("the production CA profile must validate");
    }

    /// Build a CA the way an attacker planting `ca.pem` would, with one knob per rejection reason.
    fn rogue_ca(tweak: impl FnOnce(&mut CertificateParams)) -> String {
        let mut p = ca_params(OffsetDateTime::now_utc());
        tweak(&mut p);
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        p.self_signed(&key).unwrap().pem()
    }

    /// The headline case: a real, well-formed, self-consistent CA that simply is not ours. Pre-fix it
    /// would have been installed as a trusted root — with no name constraints, it can vouch for any
    /// site on the internet.
    #[test]
    fn an_unconstrained_foreign_ca_is_refused() {
        let pem = rogue_ca(|p| {
            p.distinguished_name = rcgen::DistinguishedName::new();
            p.distinguished_name
                .push(DnType::CommonName, "Evil Root CA");
            p.name_constraints = None;
        });
        let why = validate_ca_profile(pem.as_bytes()).unwrap_err();
        assert!(why.contains("foreign CA"), "unexpected reason: {why}");
    }

    /// …and it stays refused when the attacker copies our CN to look legitimate. The name constraints
    /// are the property that actually makes the anchor harmless, so they are checked independently.
    #[test]
    fn our_name_on_an_unconstrained_ca_is_still_refused() {
        let why =
            validate_ca_profile(rogue_ca(|p| p.name_constraints = None).as_bytes()).unwrap_err();
        assert!(
            why.contains("unconstrained root"),
            "unexpected reason: {why}"
        );
    }

    /// Constrained, but to something that is not loopback — the subtle version of the same attack.
    #[test]
    fn constraints_that_reach_beyond_loopback_are_refused() {
        let pem = rogue_ca(|p| {
            p.name_constraints = Some(NameConstraints {
                permitted_subtrees: vec![GeneralSubtree::DnsName("example.com".into())],
                excluded_subtrees: vec![],
            });
        });
        let why = validate_ca_profile(pem.as_bytes()).unwrap_err();
        assert!(why.contains("outside loopback"), "unexpected reason: {why}");
    }

    /// An empty permittedSubtrees list constrains nothing, and must not read as "constrained".
    #[test]
    fn empty_constraints_are_refused() {
        let pem = rogue_ca(|p| {
            p.name_constraints = Some(NameConstraints {
                permitted_subtrees: vec![],
                excluded_subtrees: vec![],
            });
        });
        assert!(validate_ca_profile(pem.as_bytes()).is_err());
    }

    /// Everything that is not a certificate at all.
    #[test]
    fn garbage_is_refused() {
        assert!(validate_ca_profile(b"").is_err());
        assert!(validate_ca_profile(
            b"-----BEGIN CERTIFICATE-----\nnope\n-----END CERTIFICATE-----"
        )
        .is_err());
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
    fn ca_fingerprint_is_stable_and_discriminating() {
        // The R8 served-identity check is exactly `served_fingerprint != live_ca_fingerprint()`, so it
        // is only as good as this helper: it must be STABLE for the same cert (or a re-enable would
        // spuriously demand a restart every time) and DIFFERENT after a rotation (or a listener
        // serving a since-removed anchor would go undetected).
        let dir = tempfile::tempdir().unwrap();

        let (ca_a, _k, _l, _lk) = build_test_ca_and_leaf();
        let path_a = dir.path().join("a.pem");
        std::fs::write(&path_a, ca_a.pem()).unwrap();

        let first = ca_fingerprint_at(&path_a).expect("fingerprint a");
        let again = ca_fingerprint_at(&path_a).expect("fingerprint a again");
        assert_eq!(first, again, "same cert file must fingerprint identically");
        assert_eq!(first.len(), 64, "sha256 hex");

        // A rotation mints a FRESH keyless CA — the fingerprint must move.
        let (ca_b, _k2, _l2, _lk2) = build_test_ca_and_leaf();
        let path_b = dir.path().join("b.pem");
        std::fs::write(&path_b, ca_b.pem()).unwrap();
        assert_ne!(
            first,
            ca_fingerprint_at(&path_b).expect("fingerprint b"),
            "a rotated (different) CA must fingerprint differently"
        );

        // Unreadable / non-PEM → None, so the caller can't mistake garbage for a match.
        let junk = dir.path().join("junk.pem");
        std::fs::write(&junk, b"not a certificate").unwrap();
        assert!(ca_fingerprint_at(&junk).is_none());
        assert!(ca_fingerprint_at(&dir.path().join("missing.pem")).is_none());
    }

    #[test]
    fn leaf_signature_check_detects_a_partial_swap() {
        // `swap_into` renames three files in sequence, so a crash between them can leave `ca = B` with
        // `leaf/key = A`. That set loads into rustls perfectly well (leaf↔key still match), which is
        // why the leaf↔CA SIGNATURE is the only thing that can catch it (post-impl codex High).
        let (ca_a, _ka, leaf_a, _la) = build_test_ca_and_leaf();
        let (ca_b, _kb, _leaf_b, _lb) = build_test_ca_and_leaf();

        let parse = |pem: &str| {
            let (_, p) = x509_parser::pem::parse_x509_pem(pem.as_bytes()).unwrap();
            p
        };
        let (pa, pb, pl) = (parse(&ca_a.pem()), parse(&ca_b.pem()), parse(&leaf_a.pem()));
        let (_, ca_a_cert) = x509_parser::parse_x509_certificate(&pa.contents).unwrap();
        let (_, ca_b_cert) = x509_parser::parse_x509_certificate(&pb.contents).unwrap();
        let (_, leaf_a_cert) = x509_parser::parse_x509_certificate(&pl.contents).unwrap();

        assert!(
            leaf_a_cert
                .verify_signature(Some(ca_a_cert.public_key()))
                .is_ok(),
            "a matched leaf/CA pair must verify"
        );
        assert!(
            leaf_a_cert
                .verify_signature(Some(ca_b_cert.public_key()))
                .is_err(),
            "leaf A against CA B (a half-completed swap) must NOT verify"
        );
    }

    #[test]
    fn mismatched_leaf_and_key_fail_to_load() {
        // The consistency mechanism `certs_exist()` now relies on: rustls REJECTS a leaf paired with a
        // key that didn't sign it — exactly the mixed set a non-atomic 3-file rename crash can leave
        // (new leaf next to old key). So `certs_exist()` returns false for such a set and it is
        // regenerated on the next enable, instead of `generate_and_save` refusing forever and HTTPS
        // being unrecoverable (post-impl codex High).
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
        let (_ca, _ca_key, leaf_cert, _leaf_key) = build_test_ca_and_leaf();
        // A fresh, UNRELATED key — not the one that signed `leaf_cert`.
        let wrong_key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();

        let cert_pem = leaf_cert.pem();
        let key_pem = wrong_key.serialize_pem();
        let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(cert_pem.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = rustls_pemfile::private_key(&mut BufReader::new(key_pem.as_bytes()))
            .unwrap()
            .unwrap();

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key);
        assert!(
            config.is_err(),
            "a leaf paired with the WRONG key must not build a rustls config (drives certs_exist=false)"
        );
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
