//! F-004 Layer A: in-app verification of a signed update-manifest envelope.
//!
//! `tauri-plugin-updater` verifies each downloaded ARTIFACT's minisign signature against the
//! pinned pubkey, but it decides "is this newer?" from the UNSIGNED feed `version` field. A
//! feed-writer can therefore advertise a high `version` while pointing `url`/`signature` at an
//! OLD, still-validly-signed artifact → a silent downgrade to a vulnerable build (F-004).
//!
//! This module closes that by binding the version to the exact signed artifacts. The feed embeds:
//!   - `manifest`     : base64 of the EXACT envelope JSON bytes that were signed, and
//!   - `manifest_sig` : base64 of the minisign `.sig` document over those bytes.
//!
//! We base64-decode `manifest` and verify the signature over those VERBATIM bytes — no JSON
//! re-serialization, so there is zero canonicalization drift (the base64 string survives the
//! plugin's parse of `raw_json` intact). We then require the outer feed's `version`, `pub_date`,
//! and `platforms` projection — and the specific artifact the plugin selected — to equal the
//! signed envelope EXACTLY. Any mismatch is fail-closed. The signing key is the existing updater
//! key (the threat is a feed-writer, not key theft), so no new secret is introduced.

use std::collections::BTreeMap;

use base64::Engine as _;
use serde::Deserialize;

/// Max base64 length of the embedded `manifest` field, checked BEFORE decoding (DoS guard).
const MANIFEST_B64_MAX: usize = 64 * 1024;
/// Max base64 length of the `manifest_sig` field (a minisign `.sig` is tiny).
const MANIFEST_SIG_B64_MAX: usize = 4 * 1024;

/// The decoded, signed envelope. `deny_unknown_fields` so an attacker cannot smuggle extra keys.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SignedEnvelope {
    /// Envelope schema discriminator — domain-separates this signature from artifact signatures.
    schema: String,
    version: String,
    pub_date: String,
    platforms: BTreeMap<String, PlatformEntry>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PlatformEntry {
    url: String,
    /// Mandatory — the signed byte size. Also makes the SEC-03 pre-flight cap trustworthy.
    size: u64,
    signature: String,
}

/// Fixed schema string the signer must use; a different value fails closed.
const ENVELOPE_SCHEMA: &str = "aztec-accelerator-update-manifest-v1";

/// Successful verification result: the SemVer-parsed version and the signed artifact size.
#[derive(Debug, Clone)]
pub struct VerifiedManifest {
    pub version: semver::Version,
    pub size: u64,
}

/// Every failure is a distinct variant so the caller can log a `SECURITY:`-prefixed reason. All
/// are treated identically by the caller (reject the update), but distinct for forensics.
#[derive(Debug)]
pub enum ManifestError {
    MissingField(&'static str),
    FieldTooLarge(&'static str),
    Base64(&'static str),
    Utf8(&'static str),
    PubkeyDecode,
    SignatureDecode,
    /// The minisign signature over the manifest bytes did not verify against the pinned key.
    SignatureInvalid,
    EnvelopeParse,
    BadSchema,
    /// Envelope `version` is not canonical SemVer (e.g. leading `v`, or non-canonical form).
    NonCanonicalVersion,
    /// Outer feed `version`/`pub_date`/`platforms` disagrees with the signed envelope.
    OuterMismatch(&'static str),
    /// The claimed `Update.version` does not equal the signed envelope `version`.
    VersionMismatch,
    /// Zero or more-than-one platform entry matched the plugin-selected download URL.
    ArtifactNotUniquelyMatched,
    /// The selected artifact's signature/size does not equal the signed envelope's.
    ArtifactMismatch(&'static str),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ManifestError {}

fn b64(s: &str, what: &'static str) -> Result<Vec<u8>, ManifestError> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|_| ManifestError::Base64(what))
}

/// Verify the signed manifest envelope embedded in `raw_json` and bind it to the plugin-selected
/// artifact. `pubkey_b64` is the pinned updater pubkey (base64 of the minisign `.pub` document).
/// `claimed_version` / `download_url` / `artifact_sig` are the `Update` fields the plugin would act
/// on. On success the update is cryptographically bound to the exact signed artifact set.
pub fn verify_manifest(
    raw_json: &serde_json::Value,
    pubkey_b64: &str,
    claimed_version: &str,
    download_url: &str,
    artifact_sig: &str,
) -> Result<VerifiedManifest, ManifestError> {
    // 1. Extract the embedded fields (base64 strings survive the plugin's JSON parse verbatim).
    let manifest_b64 = raw_json
        .get("manifest")
        .and_then(serde_json::Value::as_str)
        .ok_or(ManifestError::MissingField("manifest"))?;
    let manifest_sig_b64 = raw_json
        .get("manifest_sig")
        .and_then(serde_json::Value::as_str)
        .ok_or(ManifestError::MissingField("manifest_sig"))?;
    if manifest_b64.len() > MANIFEST_B64_MAX {
        return Err(ManifestError::FieldTooLarge("manifest"));
    }
    if manifest_sig_b64.len() > MANIFEST_SIG_B64_MAX {
        return Err(ManifestError::FieldTooLarge("manifest_sig"));
    }

    // 2. Decode the EXACT signed bytes + the signature document.
    let manifest_bytes = b64(manifest_b64, "manifest")?;
    let sig_doc = b64(manifest_sig_b64, "manifest_sig")?;
    let sig_doc = String::from_utf8(sig_doc).map_err(|_| ManifestError::Utf8("manifest_sig"))?;

    // 3. Verify the minisign signature over the verbatim manifest bytes with the pinned key.
    //    allow_legacy=false requires the prehashed format the Tauri 2 signer emits.
    let pubkey_doc = b64(pubkey_b64, "pubkey")?;
    let pubkey_doc = String::from_utf8(pubkey_doc).map_err(|_| ManifestError::Utf8("pubkey"))?;
    let public_key =
        minisign_verify::PublicKey::decode(&pubkey_doc).map_err(|_| ManifestError::PubkeyDecode)?;
    let signature =
        minisign_verify::Signature::decode(&sig_doc).map_err(|_| ManifestError::SignatureDecode)?;
    public_key
        .verify(&manifest_bytes, &signature, false)
        .map_err(|_| ManifestError::SignatureInvalid)?;

    // 4. Parse the now-trusted envelope (strict).
    let env: SignedEnvelope =
        serde_json::from_slice(&manifest_bytes).map_err(|_| ManifestError::EnvelopeParse)?;
    if env.schema != ENVELOPE_SCHEMA {
        return Err(ManifestError::BadSchema);
    }

    // 5. Envelope version must be canonical SemVer (reject leading `v` / non-canonical forms even
    //    though the plugin would accept them) and round-trip to the exact same string.
    let env_version =
        semver::Version::parse(&env.version).map_err(|_| ManifestError::NonCanonicalVersion)?;
    if env_version.to_string() != env.version {
        return Err(ManifestError::NonCanonicalVersion);
    }

    // 6. Bind the OUTER feed to the signed envelope (B3): version, pub_date, and the full platforms
    //    projection must match exactly. `notes` stays unsigned/informational.
    let outer_version = raw_json.get("version").and_then(serde_json::Value::as_str);
    if outer_version != Some(env.version.as_str()) {
        return Err(ManifestError::OuterMismatch("version"));
    }
    let outer_pub_date = raw_json.get("pub_date").and_then(serde_json::Value::as_str);
    if outer_pub_date != Some(env.pub_date.as_str()) {
        return Err(ManifestError::OuterMismatch("pub_date"));
    }
    let outer_platforms = raw_json
        .get("platforms")
        .ok_or(ManifestError::OuterMismatch("platforms"))?;
    let outer_projection: BTreeMap<String, PlatformEntry> =
        serde_json::from_value(outer_platforms.clone())
            .map_err(|_| ManifestError::OuterMismatch("platforms"))?;
    if outer_projection != env.platforms {
        return Err(ManifestError::OuterMismatch("platforms"));
    }

    // 7. The claimed Update.version must equal the signed version exactly.
    if claimed_version != env.version {
        return Err(ManifestError::VersionMismatch);
    }

    // 8. Exactly ONE signed platform entry must match the plugin-selected download URL, and its
    //    signature/size must equal the signed values.
    let mut matches = env.platforms.values().filter(|p| p.url == download_url);
    let selected = matches
        .next()
        .ok_or(ManifestError::ArtifactNotUniquelyMatched)?;
    if matches.next().is_some() {
        return Err(ManifestError::ArtifactNotUniquelyMatched);
    }
    if selected.signature != artifact_sig {
        return Err(ManifestError::ArtifactMismatch("signature"));
    }

    Ok(VerifiedManifest {
        version: env_version,
        size: selected.size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Fixture signed with a THROWAWAY test key (private key never committed). See
    // tests/fixtures/updater/ — regenerate via `tauri signer generate/sign` if the schema changes.
    const PUBKEY_B64: &str = include_str!("../tests/fixtures/updater/pubkey.b64");
    const LATEST_JSON: &str = include_str!("../tests/fixtures/updater/latest.json");
    const URL: &str = "https://example.test/app.AppImage";
    const ART_SIG: &str = "ARTIFACT_SIG_PLACEHOLDER";

    fn feed() -> serde_json::Value {
        serde_json::from_str(LATEST_JSON).unwrap()
    }
    fn pk() -> &'static str {
        PUBKEY_B64.trim()
    }

    #[test]
    fn happy_path_verifies_and_binds() {
        let v = verify_manifest(&feed(), pk(), "1.0.8", URL, ART_SIG).expect("valid manifest");
        assert_eq!(v.version, semver::Version::parse("1.0.8").unwrap());
        assert_eq!(v.size, 12345);
    }

    #[test]
    fn splice_high_outer_version_rejected() {
        // F-004 core attack: attacker bumps the outer/claimed version but cannot re-sign the
        // envelope (still 1.0.8). Outer≠envelope ⇒ rejected before any download.
        let mut f = feed();
        f["version"] = json!("9.9.9");
        assert!(matches!(
            verify_manifest(&f, pk(), "9.9.9", URL, ART_SIG),
            Err(ManifestError::OuterMismatch("version"))
        ));
    }

    #[test]
    fn claimed_version_mismatch_rejected() {
        // Update.version disagrees with the signed envelope (outer left intact).
        assert!(matches!(
            verify_manifest(&feed(), pk(), "2.0.0", URL, ART_SIG),
            Err(ManifestError::VersionMismatch | ManifestError::OuterMismatch(_))
        ));
    }

    #[test]
    fn tampered_manifest_bytes_fail_signature() {
        let mut f = feed();
        let m = f["manifest"].as_str().unwrap().to_string();
        // flip one char in the base64 → different signed bytes → signature no longer verifies
        let mut chars: Vec<char> = m.chars().collect();
        let i = chars.len() / 2;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        f["manifest"] = json!(chars.into_iter().collect::<String>());
        assert!(matches!(
            verify_manifest(&f, pk(), "1.0.8", URL, ART_SIG),
            Err(ManifestError::SignatureInvalid
                | ManifestError::EnvelopeParse
                | ManifestError::Base64(_))
        ));
    }

    #[test]
    fn outer_size_tamper_rejected() {
        let mut f = feed();
        f["platforms"]["linux-x86_64"]["size"] = json!(999);
        assert!(matches!(
            verify_manifest(&f, pk(), "1.0.8", URL, ART_SIG),
            Err(ManifestError::OuterMismatch("platforms"))
        ));
    }

    #[test]
    fn missing_envelope_fails_closed() {
        let mut f = feed();
        f.as_object_mut().unwrap().remove("manifest");
        assert!(matches!(
            verify_manifest(&f, pk(), "1.0.8", URL, ART_SIG),
            Err(ManifestError::MissingField("manifest"))
        ));
    }

    #[test]
    fn wrong_artifact_sig_rejected() {
        assert!(matches!(
            verify_manifest(&feed(), pk(), "1.0.8", URL, "NOT_THE_SIGNED_SIG"),
            Err(ManifestError::ArtifactMismatch("signature"))
        ));
    }

    #[test]
    fn unmatched_download_url_rejected() {
        assert!(matches!(
            verify_manifest(
                &feed(),
                pk(),
                "1.0.8",
                "https://evil.test/x.AppImage",
                ART_SIG
            ),
            Err(ManifestError::ArtifactNotUniquelyMatched)
        ));
    }

    #[test]
    fn wrong_pubkey_rejected() {
        // A different (valid-format) key must not verify the signature.
        let other = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIzNzEzODFFMEEzMzU2QTgKUldTb1ZqTUtIamh4czNrcWR3QVZiUmIwdyttSzg5OUhtZXJobURnY05KL2pCZWMydDFPcWkvWFAK";
        assert!(matches!(
            verify_manifest(&feed(), other, "1.0.8", URL, ART_SIG),
            Err(ManifestError::SignatureInvalid)
        ));
    }
}
