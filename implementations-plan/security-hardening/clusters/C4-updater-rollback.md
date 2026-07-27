# C4 — updater-rollback (F-004) · /blueprint deep — MAIN plan (1 of 3)

**Cluster:** C4 · **Branch:** `sechard/updater-rollback` off `security-hardening` · **Tier:** deep.

## Problem
`tauri-plugin-updater` verifies each downloaded artifact's Ed25519/minisign `.sig` against the pinned `pubkey` (tauri.conf.json:18) — so it can't install *unsigned* bytes. But the "is this newer?" decision (updater.rs:39-41) uses `update.version`, a field of the **unsigned** feed JSON (`latest.json`). An attacker who can write the feed (S3 `releases/latest.json` — reachable via F-005's over-broad IAM) advertises `version: "999.0.0"` while pointing `url`/`signature` at an OLD, still-validly-signed release → the app "upgrades" to a vulnerable build. No signing-key compromise needed. The SEC-03 size-cap comments (updater.rs:88-108) already note the feed is attacker-controlled and not independent.

## Fix (two independent layers — defense in depth)
**Layer A — sign the manifest (bind version↔artifacts).** Embed a canonical **manifest envelope** in `latest.json` covering `{version, pub_date, platforms.*.{url, size, signature}}`, signed with the updater minisign key; verify it in-app with the pinned pubkey BEFORE trusting `update.version`/size/url. This makes the version cryptographically bound to the exact signed artifacts, so a high-version+old-artifact splice fails verification.
**Layer B — monotonic floor (rollback ratchet).** Persist the highest version that has ever *successfully run* as an atomic `0600` config value; require candidate `> max(current_running, floor)`. Even if Layer A were bypassed, the app refuses to go backwards. Bump the floor only AFTER the new build starts successfully (a crashing bad update can't advance it). A corrupt/unreadable floor ⇒ updater **fail-closed** (disabled), never reset to 0.

## Phases (each ends with a Validation gate)
**Phase 1 — monotonic floor (Rust, local-testable).** Add a `min_version_floor` (or reuse config) persisted atomically 0600 (mirror config.rs pattern). In `check_for_update`, after the plugin returns an `Update`, reject if `parse(update.version) <= max(current, floor)`. Bump floor after a successful post-update launch (crash-recovery hook). Corrupt floor ⇒ disable updater.
- *Gate:* `cargo fmt + clippy -D warnings + test`; unit tests: replay (version ≤ floor rejected), equal-version rejected, corrupt-floor fail-closed, floor advances only post-successful-launch. GUI-less VPS: **yes** (pure Rust).

**Phase 2 — manifest envelope verification (Rust, local-testable).** Add `minisign-verify` (already transitive) as a direct dep + a MANIFEST pubkey (same updater key). Define the canonical manifest serialization; verify a `manifest_sig` (detached, over the canonical bytes) embedded in / alongside `latest.json` (in `update.raw_json`) using the pinned key; assert the version/url/size/artifact-sig the plugin will use EXACTLY match the signed manifest. Fail-closed on missing/invalid envelope.
- *Gate:* `cargo test`; fixtures — valid envelope accepted; tampered version/url/size rejected; missing envelope rejected; envelope signed by wrong key rejected; spliced (high version + old artifacts) rejected. GUI-less VPS: **yes**.

**Phase 3 — release signing job (CI/release, CI-validated).** In `release-accelerator.yml`, after `latest.json` is generated (L641+), add a dedicated job with **NO AWS/GitHub write perms** that canonicalizes the manifest and signs it with `TAURI_SIGNING_PRIVATE_KEY` (minisign), embedding `manifest_sig`. Update the updater smoke (`_e2e-updater*.yml`) to exercise a rollback-attempt negative case (high version + old artifact ⇒ rejected).
- *Gate:* `bun run lint:actions`; the updater smoke's new negative case; CI green. macOS/Windows updater E2E via CI.

## Security & Adversarial Considerations
- **Threat model:** feed-writer (via F-005 IAM, or CDN/DNS) attempts (a) rollback to old-signed build, (b) high-version+old-artifact splice, (c) size lie (SEC-03). Layer A defeats (a)+(b); the signed `size` in the envelope finally makes the SEC-03 cap trustworthy (residual: a host can still stream huge bytes before the plugin's buffered verify — documented, needs upstream streaming cap).
- **Crypto:** minisign/Ed25519 via `minisign-verify` (battle-tested; same family the plugin uses). NO hand-rolled crypto. Canonical serialization must be deterministic (fixed field order, no floats) to avoid signature ambiguity.
- **Least privilege:** the signing job holds the signing key but NO cloud/repo write — it emits the signed manifest as an artifact for the (separately-privileged) upload job.
- **Key mgmt:** reuse the existing updater key (no new secret). Verify: is the manifest key == artifact-sig key, or a separate key? (Ask/audit.)

## Assumptions
**Facts:** (1) plugin verifies artifact `.sig` vs pubkey but NOT version binding (updater.rs:39-41 uses feed `update.version`). (2) `update.raw_json` exposes the full feed JSON (size_from_feed reads it, updater.rs:67). (3) pubkey pinned in tauri.conf.json:18. (4) latest.json generated in release-accelerator.yml:641+ from per-platform `.sig`. (5) config.rs has an atomic-0600 write pattern to mirror.
**Inferences:** `minisign-verify` is reachable as a direct dep (transitive via plugin); the updater key can sign an arbitrary manifest blob (not just Tauri's artifact format). **Attack these in audit.**
**Asks (surface to audit):** same-key-vs-separate-key for the manifest; exact envelope location (embedded field vs sidecar `.sig`); whether tauri-plugin-updater already exposes any manifest-signature hook (avoid reinventing); floor storage location + how "successful launch" is observed (crash_recovery integration).

## Seeds
Campaign-level `/goal` + `/loop` drive this; C4 is one (deep) loop iteration.
