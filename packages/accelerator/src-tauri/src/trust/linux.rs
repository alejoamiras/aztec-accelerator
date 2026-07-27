//! Linux trust backend — **user-level NSS only, no root**. Installs the CA into the Chromium-family
//! user DB (`~/.pki/nssdb`) and every discoverable Firefox profile (`profiles.ini`), mkcert-style,
//! via `certutil`. Trust is inherently per-browser here, so status is reported honestly per store and
//! a missing `certutil` degrades gracefully (the wizard shows it as an install failure with a hint).

use super::{AnchorRef, StoreStatus, TrustReport};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve `certutil` to a SAFE absolute path. Prefer known system locations; only accept a
/// PATH-resolved binary if it is absolute AND neither it nor any ancestor is group/world-writable or
/// foreign-owned — a planted `certutil` in a writable PATH dir must not win (plan §8 / codex S2).
/// `None` ⇒ absent.
///
/// Returns the **canonical** path — the exact one that was validated. Returning the original would
/// leave a TOCTOU: a foreign-owned SYMLINK whose target is currently `/usr/bin/certutil` passes the
/// check, and its owner can then re-point it before we `Command::new(...)` it (post-impl codex High
/// R3). Executing the canonical target closes that window.
fn certutil_bin() -> Option<PathBuf> {
    // Even the known locations get the guard — `/usr/local/bin` is group/other-writable on some
    // setups, and accepting a planted binary there would be the ACE this guard exists to prevent
    // (post-impl review). A rejected path just falls through; if none qualifies, HTTPS degrades with a
    // "certutil not found" hint rather than executing an attacker binary.
    for p in [
        "/usr/bin/certutil",
        "/bin/certutil",
        "/usr/local/bin/certutil",
    ] {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            if let Some(safe) = safe_canonical(&pb) {
                return Some(safe);
            }
        }
    }
    match which::which("certutil") {
        Ok(p) if p.is_absolute() => safe_canonical(&p),
        _ => None,
    }
}

/// Canonicalize `p` and return it ONLY if the resolved binary and every ancestor pass the
/// [`is_writable_by_nonowner`] check. `None` ⇒ unsafe or unresolvable (fail closed). The returned path
/// is what callers must execute, so validation and execution refer to the same inode path.
fn safe_canonical(p: &Path) -> Option<PathBuf> {
    let resolved = p.canonicalize().ok()?;
    if is_writable_by_nonowner(&resolved) {
        return None;
    }
    Some(resolved)
}

/// True if the resolved binary — or ANY ancestor directory up to `/` — is unsafe to trust as the
/// source of a privileged `certutil` invocation, i.e. group/world-writable OR owned by a user that is
/// neither root nor us (so a DIFFERENT user could replace it), OR unreadable / un-canonicalizable
/// (fail closed). Two prior gaps this closes: checking only the immediate parent let a writable
/// grandparent (`/opt/tools` 0777 over `bin/` 0755) pass (codex High R1); and a mode-only check
/// accepted an attacker-OWNED but `0755` tree like `/opt/attacker/bin` (codex High R2) — ownership is
/// now required. Canonicalizing first resolves symlinks + `..` so a safe-looking path can't tunnel
/// through a writable/foreign-owned link.
fn is_writable_by_nonowner(p: &Path) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // SAFETY: geteuid() takes no arguments, has no preconditions, and cannot fail.
    let euid = unsafe { libc::geteuid() };
    let unsafe_component = |m: &Path| match std::fs::metadata(m) {
        Ok(md) => {
            let mode_unsafe = md.permissions().mode() & 0o022 != 0;
            // Owned by neither root (0) nor the current user ⇒ another user can modify it.
            let owner = md.uid();
            let owner_unsafe = owner != 0 && owner != euid;
            mode_unsafe || owner_unsafe
        }
        Err(_) => true, // can't stat → fail closed
    };
    let Ok(resolved) = p.canonicalize() else {
        return true; // can't resolve → fail closed
    };
    if unsafe_component(&resolved) {
        return true;
    }
    let mut ancestor = resolved.parent();
    while let Some(dir) = ancestor {
        if unsafe_component(dir) {
            return true;
        }
        ancestor = dir.parent();
    }
    false
}

/// The DER bytes of a PEM-encoded cert (for a content-stable nickname).
fn cert_der(ca_pem: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(ca_pem).ok()?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&bytes).ok()?;
    Some(pem.contents)
}

/// Per-anchor NSS nickname: `aztec-accelerator-ca-<first 8 hex of sha256(DER)>`. Stable per anchor,
/// so a rotation installs a NEW nickname and removes the OLD one unambiguously (D4 — Linux deletes
/// by nickname, not serial).
fn nickname_for(ca_pem: &Path) -> Option<String> {
    let der = cert_der(ca_pem)?;
    let hash = Sha256::digest(&der);
    Some(format!("aztec-accelerator-ca-{}", hex::encode(&hash[..4])))
}

/// An NSS database we may install into.
struct NssStore {
    /// Human label for the UI.
    label: String,
    /// The DB directory (passed to certutil as `sql:<dir>`).
    dir: PathBuf,
    /// Whether to create the DB if it's absent (true for the Chromium user DB; Firefox profiles must
    /// already exist — we never fabricate a profile).
    create_if_absent: bool,
}

impl NssStore {
    fn sql(&self) -> String {
        format!("sql:{}", self.dir.display())
    }
}

/// A Firefox profile parsed from `profiles.ini`.
#[derive(Debug, PartialEq)]
struct FirefoxProfile {
    name: String,
    /// Relative (to the profiles.ini dir) vs absolute path.
    relative: bool,
    path: String,
}

/// Parse `profiles.ini` into its profile entries. Pure + defensively-bounded (ignores malformed
/// sections/keys); unit-tested. Deliberately hand-rolled — no `rust-ini` dependency.
fn parse_profiles_ini(content: &str) -> Vec<FirefoxProfile> {
    let mut out = Vec::new();
    let mut in_profile = false;
    let mut name = String::new();
    let mut path: Option<String> = None;
    let mut relative = true;

    let flush =
        |out: &mut Vec<FirefoxProfile>, name: &str, path: &Option<String>, relative: bool| {
            if let Some(p) = path {
                if !p.is_empty() {
                    out.push(FirefoxProfile {
                        name: name.to_string(),
                        relative,
                        path: p.clone(),
                    });
                }
            }
        };

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            if in_profile {
                flush(&mut out, &name, &path, relative);
            }
            let section = &line[1..line.len() - 1];
            in_profile = section.starts_with("Profile");
            name.clear();
            path = None;
            relative = true;
            continue;
        }
        if !in_profile {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Name" => name = v.trim().to_string(),
                "Path" => path = Some(v.trim().to_string()),
                "IsRelative" => relative = v.trim() != "0",
                _ => {}
            }
        }
    }
    if in_profile {
        flush(&mut out, &name, &path, relative);
    }
    out
}

/// The three Firefox root layouts we cover: native, snap, flatpak (all best-effort).
fn firefox_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".mozilla/firefox"),
        home.join("snap/firefox/common/.mozilla/firefox"),
        home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
    ]
}

/// Discover all NSS stores to install into: the Chromium-family user DB + every Firefox profile that
/// already has a `cert9.db`.
fn discover_stores(home: &Path) -> Vec<NssStore> {
    let mut stores = vec![NssStore {
        label: "System NSS (Chrome/Chromium/Edge/Brave)".into(),
        dir: home.join(".pki/nssdb"),
        create_if_absent: true,
    }];

    // Canonical $HOME to bound every candidate profile dir under it (post-impl codex High / S3):
    // profiles.ini is user-owned but attacker-influenceable, so an absolute path, a `../` escape, or a
    // symlinked profile dir must NOT let certutil operate outside the user's home. `canonicalize`
    // resolves symlinks + `..`; we then require the result to live under canonical $HOME.
    let home_canon = home.canonicalize().ok();

    for root in firefox_roots(home) {
        let ini = root.join("profiles.ini");
        let Ok(content) = std::fs::read_to_string(&ini) else {
            continue;
        };
        for prof in parse_profiles_ini(&content) {
            let raw_dir = if prof.relative {
                root.join(&prof.path)
            } else {
                PathBuf::from(&prof.path)
            };
            // Only touch an existing, initialized profile (has cert9.db). Never fabricate one.
            if !raw_dir.join("cert9.db").exists() {
                continue;
            }
            // Canonicalize (resolves symlinks + `..`) and require the result under canonical $HOME.
            let Ok(dir) = raw_dir.canonicalize() else {
                continue;
            };
            match &home_canon {
                Some(h) if dir.starts_with(h) => {}
                _ => {
                    tracing::warn!(path = %dir.display(), "Skipping Firefox profile outside $HOME");
                    continue;
                }
            }
            let label = format!(
                "Firefox ({})",
                if prof.name.is_empty() {
                    prof.path.clone()
                } else {
                    prof.name.clone()
                }
            );
            stores.push(NssStore {
                label,
                dir,
                create_if_absent: false,
            });
        }
    }
    stores
}

fn run_certutil(bin: &Path, args: &[&std::ffi::OsStr]) -> std::io::Result<std::process::Output> {
    Command::new(bin).args(args).output()
}

/// Ensure the store's NSS DB exists (create an empty, password-less one for the Chromium user DB).
fn ensure_db(bin: &Path, store: &NssStore) -> bool {
    if store.dir.join("cert9.db").exists() {
        return true;
    }
    if !store.create_if_absent {
        return false;
    }
    if std::fs::create_dir_all(&store.dir).is_err() {
        return false;
    }
    run_certutil(
        bin,
        &[
            "-N".as_ref(),
            "--empty-password".as_ref(),
            "-d".as_ref(),
            store.sql().as_ref(),
        ],
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

fn add_to_store(bin: &Path, store: &NssStore, nick: &str, ca_cert: &Path) -> bool {
    run_certutil(
        bin,
        &[
            "-A".as_ref(),
            "-t".as_ref(),
            "C,,".as_ref(),
            "-n".as_ref(),
            nick.as_ref(),
            "-i".as_ref(),
            ca_cert.as_os_str(),
            "-d".as_ref(),
            store.sql().as_ref(),
        ],
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// Whether `nick` is present AND TRUSTED for SSL-CA usage in the store. `-V -u L` VALIDATES the cert
/// for the "SSL CA" usage rather than merely confirming it exists (`-L -n`): a cert that is present
/// but explicitly distrusted, or that lacks the `C` SSL trust flag, fails this — so status reflects
/// real trust, not bare existence (post-impl codex Medium). Our fresh anchors install with `-t C,,`
/// and are self-signed + long-lived, so they pass.
fn is_trusted_in_store(bin: &Path, store: &NssStore, nick: &str) -> bool {
    run_certutil(
        bin,
        &[
            "-V".as_ref(),
            "-u".as_ref(),
            "L".as_ref(),
            "-n".as_ref(),
            nick.as_ref(),
            "-d".as_ref(),
            store.sql().as_ref(),
        ],
    )
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// Tri-state presence of ANY of our anchors in a store, for the removal post-check. Unlike a bare
/// bool, this distinguishes "definitely gone" from "couldn't read the DB" — a locked / permission-
/// denied store returns `Unknown` so removal is reported as UNconfirmed rather than silently
/// successful (post-impl codex High).
enum Present {
    Yes,
    No,
    Unknown,
}

fn our_anchors_present(bin: &Path, store: &NssStore) -> Present {
    match run_certutil(bin, &["-L".as_ref(), "-d".as_ref(), store.sql().as_ref()]) {
        Ok(o) if o.status.success() => {
            let has = String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|line| line.split_whitespace().next())
                .any(|tok| tok.starts_with(NICK_PREFIX));
            if has {
                Present::Yes
            } else {
                Present::No
            }
        }
        // Non-zero exit OR spawn error: the DB is unreadable → we cannot confirm removal.
        _ => Present::Unknown,
    }
}

fn delete_from_store(bin: &Path, store: &NssStore, nick: &str) {
    let _ = run_certutil(
        bin,
        &[
            "-D".as_ref(),
            "-n".as_ref(),
            nick.as_ref(),
            "-d".as_ref(),
            store.sql().as_ref(),
        ],
    );
}

/// The `aztec-accelerator-ca-*` nickname prefix all our anchors share.
const NICK_PREFIX: &str = "aztec-accelerator-ca-";

/// List every one of OUR anchor nicknames currently in a store. `certutil -L` prints one row per
/// cert as `<nickname>  <trust-flags>`; the nickname is the leading whitespace-delimited token. We
/// keep only tokens starting with our distinctive prefix, so this can't misfire on foreign certs.
fn our_nicks_in_store(bin: &Path, store: &NssStore) -> Vec<String> {
    let Ok(out) = run_certutil(bin, &["-L".as_ref(), "-d".as_ref(), store.sql().as_ref()]) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|tok| tok.starts_with(NICK_PREFIX))
        .map(str::to_string)
        .collect()
}

/// Delete ALL of our anchors from a store (the live one AND any left by prior rotations, each under a
/// different content-hash nickname — post-impl review: remove/uninstall must clear them all).
fn delete_all_ours(bin: &Path, store: &NssStore) {
    for nick in our_nicks_in_store(bin, store) {
        delete_from_store(bin, store, &nick);
    }
}

/// The extra informational row disclaiming coverage we can't verify (audit M-2): sandboxed
/// snap/flatpak Chromium may keep its own confined NSS DB we don't reach.
fn sandbox_disclaimer() -> StoreStatus {
    StoreStatus {
        store: "Sandboxed Chromium (snap/flatpak)".into(),
        installed: false,
        detail: Some(
            "not covered — confined browsers keep a private trust store; restart the browser after install".into(),
        ),
    }
}

fn missing_certutil_report() -> TrustReport {
    TrustReport {
        stores: vec![StoreStatus::fail(
            "NSS trust stores",
            "certutil not found — install libnss3-tools to enable HTTPS in your browsers",
        )],
    }
}

pub fn install(ca_cert: &Path) -> TrustReport {
    let Some(bin) = certutil_bin() else {
        return missing_certutil_report();
    };
    let Some(nick) = nickname_for(ca_cert) else {
        return TrustReport {
            stores: vec![StoreStatus::fail(
                "NSS trust stores",
                "could not read the CA certificate",
            )],
        };
    };
    let Some(home) = dirs::home_dir() else {
        return TrustReport {
            stores: vec![StoreStatus::fail("NSS trust stores", "no home directory")],
        };
    };

    let mut stores: Vec<StoreStatus> = discover_stores(&home)
        .iter()
        .map(|store| {
            if !ensure_db(&bin, store) {
                return StoreStatus::fail(store.label.clone(), "NSS database unavailable");
            }
            if add_to_store(&bin, store, &nick, ca_cert) && is_trusted_in_store(&bin, store, &nick)
            {
                StoreStatus::ok(store.label.clone())
            } else {
                StoreStatus::fail(
                    store.label.clone(),
                    "certutil could not add + trust the certificate",
                )
            }
        })
        .collect();
    stores.push(sandbox_disclaimer());
    TrustReport { stores }
}

pub fn status(ca_cert: &Path) -> TrustReport {
    let Some(bin) = certutil_bin() else {
        return missing_certutil_report();
    };
    let (Some(nick), Some(home)) = (nickname_for(ca_cert), dirs::home_dir()) else {
        return TrustReport::default();
    };
    let mut stores: Vec<StoreStatus> = discover_stores(&home)
        .iter()
        .filter(|s| s.dir.join("cert9.db").exists())
        .map(|store| StoreStatus {
            store: store.label.clone(),
            installed: is_trusted_in_store(&bin, store, &nick),
            detail: None,
        })
        .collect();
    stores.push(sandbox_disclaimer());
    TrustReport { stores }
}

pub fn remove(_ca_cert: &Path) -> TrustReport {
    let Some(bin) = certutil_bin() else {
        // Can't remove without certutil — report trust as possibly-still-present (installed:true) so
        // the caller FAILS the Settings/CLI action rather than reporting a clean removal it never
        // performed (post-impl codex High).
        return TrustReport {
            stores: vec![StoreStatus {
                store: "NSS trust stores".into(),
                installed: true,
                detail: Some(
                    "certutil not found — cannot remove trust (install libnss3-tools)".into(),
                ),
            }],
        };
    };
    let Some(home) = dirs::home_dir() else {
        return TrustReport::default();
    };
    // Remove ALL our anchors (live + any left by prior rotations) from every store, then CONFIRM.
    let stores: Vec<StoreStatus> = discover_stores(&home)
        .iter()
        .filter(|s| s.dir.join("cert9.db").exists())
        .map(|store| {
            delete_all_ours(&bin, store);
            // Post-check: a DB we couldn't read is "still present" (Unknown → installed:true), so a
            // locked/denied store fails the removal instead of silently reporting success.
            let installed = !matches!(our_anchors_present(&bin, store), Present::No);
            StoreStatus {
                store: store.label.clone(),
                installed,
                detail: installed
                    .then(|| "anchor may still be trusted (removal unconfirmed)".to_string()),
            }
        })
        .collect();
    TrustReport { stores }
}

pub fn current_anchor(live_ca: &Path) -> AnchorRef {
    AnchorRef(nickname_for(live_ca))
}

/// Trust the freshly-staged anchor in every store; success = installed in ≥1 store (Linux trust is
/// inherently partial). `Err` only if certutil is absent or no store accepted it.
pub fn trust_new_anchor(staged_ca: &Path) -> Result<(), String> {
    let report = install(staged_ca);
    if report.any_installed() {
        Ok(())
    } else {
        Err("no NSS store accepted the new anchor".into())
    }
}

pub fn remove_anchor(old: AnchorRef) {
    let (Some(bin), Some(nick), Some(home)) = (certutil_bin(), old.0, dirs::home_dir()) else {
        return;
    };
    for store in discover_stores(&home)
        .iter()
        .filter(|s| s.dir.join("cert9.db").exists())
    {
        delete_from_store(&bin, store, &nick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_and_absolute_profiles() {
        let ini = "\
[Install123]
Default=abc.default

[Profile0]
Name=default
IsRelative=1
Path=abc.default
Default=1

[Profile1]
Name=dev-edition
IsRelative=0
Path=/home/u/.mozilla/firefox/xyz.dev

[General]
StartWithLastProfile=1
";
        let profiles = parse_profiles_ini(ini);
        assert_eq!(profiles.len(), 2);
        assert_eq!(
            profiles[0],
            FirefoxProfile {
                name: "default".into(),
                relative: true,
                path: "abc.default".into()
            }
        );
        assert_eq!(
            profiles[1],
            FirefoxProfile {
                name: "dev-edition".into(),
                relative: false,
                path: "/home/u/.mozilla/firefox/xyz.dev".into()
            }
        );
    }

    #[test]
    fn ignores_non_profile_sections_and_malformed_lines() {
        let ini = "\
[General]
Path=should-be-ignored

[Profile0]
garbage-without-equals
Path=real.profile
";
        let profiles = parse_profiles_ini(ini);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].path, "real.profile");
        assert!(
            profiles[0].relative,
            "IsRelative defaults to true when absent"
        );
    }

    #[test]
    fn empty_or_pathless_profiles_are_dropped() {
        assert!(parse_profiles_ini("").is_empty());
        assert!(parse_profiles_ini("[Profile0]\nName=x\n").is_empty());
    }

    #[test]
    fn nickname_is_stable_and_prefixed() {
        // A real self-signed PEM so x509-parser can extract the DER; two calls on the same file yield
        // the same nickname.
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = rcgen::CertificateParams::default();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Aztec Accelerator Local CA");
        let pem = params.self_signed(&key).unwrap().pem();

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ca.pem");
        std::fs::write(&p, &pem).unwrap();

        let n1 = nickname_for(&p).unwrap();
        let n2 = nickname_for(&p).unwrap();
        assert_eq!(n1, n2);
        assert!(n1.starts_with("aztec-accelerator-ca-"));
        assert_eq!(n1.len(), "aztec-accelerator-ca-".len() + 8);
    }

    #[test]
    fn writable_dir_binary_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let bin = dir.path().join("certutil");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Parent dir is world-writable → rejected regardless of the binary's own perms.
        assert!(is_writable_by_nonowner(&bin));
    }
}
