# Harden Report: security

**Repo:** aztec-accelerator (`packages/sdk`, `packages/accelerator`, `infra/tofu`, `.github`, `scripts`)
**Date:** 2026-07-09
**Effort:** high
**Run ID:** 2026-07-09-5c788c0 (HEAD `5c788c0`)
**Models:** Phase 1 map — Claude (Explore ×3). Phase 2 — per cluster: Claude Sonnet + Codex (read-only, high reasoning). Phase 3 reduce + Phase 4 verify — Fable (main), with direct upstream-source verification. 
**Scope:** whole repo MINUS `packages/playground` and `packages/landing`; excluded `audit/`, `implementations-plan/`, `node_modules`, `dist`, `target`. Special emphasis (user-requested): privacy leaks, cryptography, ZK witness risk.

## Executive summary

Ten clusters were audited by paired Claude + Codex agents (20 independent passes). The codebase is **well-built**: the loopback `Host`/`:authority` DNS-rebinding guard, the `CanonicalOrigin` newtype, deny-by-default origin authorization, the UUID-keyed approval resolution (SEC-06), fail-closed download-digest verification in the Rust runtime, and the headless `ALLOWED_ORIGINS` parser all held up against direct attack. Most clusters produced 0–2 findings; several produced none.

The findings cluster around **one architectural theme and one operational theme.** Architecturally, the trust boundary between the browser SDK and the local accelerator is **unauthenticated in the SDK→server direction**: the SDK will hand the private proving witness to whatever process answers on `127.0.0.1:59833`, and a second bug lets a local impostor evict the real accelerator to guarantee it holds that port (**F-001 + F-002**). Because the witness is the crown jewel — it deanonymizes an otherwise-private transaction — this is the highest-value exposure in the report, though it requires a local foothold. A third witness issue (**F-003**) is a plain file-permission miss: the witness is written to a world-readable temp file, readable by any other local user during proving on a default-umask multi-tenant host (verified against `tempfile` 3.27.0's actual behavior — `tempdir()` does **not** apply `0o700`).

Operationally, the **update + supply-chain channel** is the other concentration: the updater trusts a feed-declared version that is not bound to the signed artifact, enabling a **downgrade to an old-but-signed vulnerable build** (**F-004**); the CI deploy role can write the whole S3 bucket — including `releases/latest.json` — from unprotected wildcard branches (**F-005**), which is the concrete path to controlling that feed; a reusable publish workflow has a **`dist_tag` shell-injection** that leaks `NPM_TOKEN` (**F-006**); and the `bb` binary that processes witnesses has two integrity gaps in its non-runtime download/pin paths (**F-007, F-008**).

Recommended priorities: (1) the SDK→server authentication pair **F-001+F-002**; (2) the update-channel pair **F-004+F-005**; (3) the cheap, high-certainty fixes **F-003, F-006, F-009**. No Critical (unauthenticated-remote) issues were found — every High requires either a local foothold or control of the CI/update infrastructure.

## Methodology

Map-reduce with a coordinator-of-specialists shape. **Phase 1**: 3 parallel Explore mappers (sdk / accelerator-Rust / misc-ci-infra) → `raw/repo-map/`. **Phase 2**: 10 clusters, each audited by 1 Claude Sonnet agent **and** 1 Codex agent (read-only sandbox, `model_reasoning_effort=high`), using an identical structured 10-field security certificate prompt carrying the privacy/crypto/witness emphasis and the negative list → `raw/<cluster>-{claude,codex}.md`. Inter-procedural context capped ~4 functions with handoff-edge escalation (event/IPC/RPC boundaries). **Phase 3**: Fable coordinator deduped by root-cause+sink+boundary, kept cross-model convergence as a confidence signal, assigned CVSS-style bands → `findings/consolidated.md`. **Phase 4**: independent source re-read of every contested/divergent claim (and direct upstream-source checks, e.g. the `tempfile` dir-mode question and the `_publish-sdk.yml` interpolation) before confirming → `findings/verified.md`.

**Deviations from the formal `high` spec (stated honestly):** (a) The witness-path clusters were weighted heaviest per the user's emphasis; cluster count (10) matches the reference. (b) **Phase 2.5 cross-rebuttal was folded into Phase 3+4** rather than run as 20 separate rebuttal agents: the coordinator (Fable) read all 20 raw outputs, adjudicated every convergence/divergence, and independently re-read source for each contested finding — a stronger check than a light rebuttal pass for the specific disputes here (it flipped one finding's validity, F-003, and downgraded another, F-014). (c) No separate Codex-at-reduce pass (that is a `max`-tier feature); cross-family coverage is carried by the 10 Codex Phase-2 legs. (d) Codex legs were throttled 3-wide with a 25-min ceiling; all 10 completed.

Finding density: 16 findings / 10 clusters = 1.6 (slightly above the ~1.2 target, driven by the two supply-chain clusters), of which 2 High, 7 Medium, 7 Low.

---

## Findings

### [HIGH] F-001: SDK sends the private witness to an unauthenticated local server
**Impact:** High (Confidentiality of the ZK witness; one user; local vector, low complexity, low privilege, no user interaction beyond a normal proof request)
**Confidence:** high · **Mapping:** OWASP A07 / CWE-306 · **Found by:** both (convergent)
**Instances:** `packages/sdk/src/lib/accelerator-transport.ts:112,116,148`; `packages/sdk/src/lib/accelerator-prover.ts:177,194,262,273,299,309`
**Coupled with:** F-002

**Description.** The SDK detects the accelerator with an unauthenticated `GET /health` probe (first success wins via `Promise.any`), pins the winning protocol, then POSTs the msgpack-serialized private execution steps (the witness) to `/prove`. It never verifies it is talking to the *real* accelerator — no certificate/public-key pin, no shared secret, no process attestation. A bare `{}` health body counts as "available", and even a version mismatch (`needsDownload`) still proceeds to send the witness.

**Trace.** `createChonkProof(executionSteps)` (`accelerator-prover.ts:262`) → status check (`:273`) → `/health` probe (`accelerator-transport.ts:112`) → first-win pin (`:116`, `accelerator-prover.ts:194`) → witness serialized (`accelerator-prover.ts:299`) → `postProve` → `POST {baseUrl}/prove` (`accelerator-transport.ts:148`). No identity check anywhere on this path.

**Why it matters.** A local process that binds `127.0.0.1:59833` before the genuine app answers `/health` and receives the witness. The witness deanonymizes an otherwise-private Aztec transaction. The server-side deny-by-default origin model never runs, because the real server isn't the one receiving the request.

**Recommended fix.** Authenticate the server before transmitting the witness: pin the accelerator's self-signed cert / public key, or exchange a per-install token out-of-band (e.g. via a file in the app's `0o700` data dir the SDK reads). Treat unrecognized/legacy `/health` responses as *not available* (fall back to WASM) instead of proceeding. **Effort:** days.

---

### [HIGH] F-004: Updater rollback — feed `version` not bound to the signed artifact
**Impact:** High (Integrity of the installed binary; all users; feed/CI-control vector, low complexity once feed is controlled, no user interaction for auto-update)
**Confidence:** high · **Mapping:** OWASP A08 / CWE-347 · **Found by:** both (convergent)
**Instances:** `packages/accelerator/src-tauri/src/updater.rs:27,39-56,126-137,170`
**Coupled with:** F-005

**Description.** `tauri-plugin-updater`'s minisign check authenticates the artifact *bytes*, but the "is this newer?" decision uses the feed-declared `version` string, which is not cryptographically bound to the `url`/`signature` it ships beside in the same feed JSON. An attacker who controls the feed can advertise `version: 999.0.0` while pointing at an older, still-validly-signed build; the updater installs it. No signing-key compromise is needed, and there is no app-side monotonic rollback floor.

**Why it matters.** This is the "signed updates" guarantee defeated by replay: users are silently rolled back to a build with known-and-fixed vulnerabilities. Blast radius is every auto-update user. (Platform-installer downgrade protection, e.g. MSI, mitigates on some platforms; macOS/generic paths do not.)

**Recommended fix.** After download, assert the artifact's embedded app-version equals the feed `version` and is strictly greater than the installed version; persist a monotonic floor. Pair with F-005 to protect the feed itself. **Effort:** days.

---

### [MEDIUM] F-002: Spoofable `/health` probe evicts the real accelerator (Windows)
**Impact:** Medium (Availability + Confidentiality via F-001; one user, Windows; local) · **Confidence:** high · **Mapping:** OWASP A07 / CWE-287 · **Found by:** both · **Instances:** `packages/accelerator/core/src/server/probe.rs:14-17,24-44`; `packages/accelerator/src-tauri/src/main.rs:238-245` · **Coupled with:** F-001

The redundant-instance self-probe classifies "healthy Aztec" from only the public, forgeable `{status:"ok", api_version:1}`. On the Windows startup path the real app `exit(0)`s when the probe is positive, so a local impostor that binds the port first and answers `/health` makes the genuine accelerator quit — then keeps the port that F-001's SDK will trust. **Fix:** authenticate the incumbent (per-install secret / named-pipe identity / signed challenge) before self-terminating. **Effort:** hours–days.

### [MEDIUM] F-003: Private witness written to a world-readable temp file
**Impact:** Medium (Confidentiality of the witness on multi-user hosts; local; umask-dependent) · **Confidence:** high · **Mapping:** OWASP A01 / CWE-200, CWE-732 · **Found by:** Claude (verified) · **Instances:** `packages/accelerator/core/src/bb.rs:86-90`

`bb::prove` does `tempfile::tempdir()?` then `std::fs::write(&input_path, ivc_inputs)`. **Verified against tempfile 3.27.0** (`src/dir/imp/unix.rs::create`): the directory mode is applied only when explicit `permissions` are passed; `tempdir()` passes `None`, so the dir gets the umask default (**~0o755**) and the file **~0o644**. `config.rs`/`certs.rs` correctly force `0o700`/`0o600`; this path does not. On a default-umask multi-tenant host (shared dev box, CI runner, a homelab running many agents) any local user can read `ivc-inputs.msgpack` — the plaintext witness — for the proving window (≤5 min). **Fix:** `tempfile::Builder::new().permissions(Permissions::from_mode(0o700)).tempdir()` and write the file `0o600` (or nest under the app's `0o700` data dir). **Effort:** hours.

### [MEDIUM] F-005: Deploy trust reaches the update feed (wildcard OIDC + whole-bucket write)
**Impact:** Medium (Integrity/Availability of the update feed + hosted sites; all users; CI-control vector) · **Confidence:** high · **Mapping:** OWASP A01/A08 / CWE-269 · **Found by:** both · **Instances:** `infra/tofu/iam.tf:32-38,53-72`; `infra/rulesets/main-branch-protection.json:6-20`; `.github/workflows/publish-nightlies.yml:69-82`, `release-accelerator.yml:809-824` · **Coupled with:** F-004

The AWS OIDC trust policy accepts `sub` for `refs/heads/chore/aztec-nightlies-*` and `chore/aztec-stable-*`; branch protection covers only `main` (which itself requires 0 approvals), leaving those wildcard namespaces and `nightlies` unprotected. The assumed role grants `s3:PutObject/DeleteObject` over the **entire** site bucket (shared by 4 differently-trusted pipelines) plus CloudFront invalidation, so it can overwrite `landing/releases/latest.json` (→ F-004) or deface `landing/`/`playground/`. **Threat-model note:** this is a single-owner public repo, so the actor is the owner or a compromised owner/CI token, not an anonymous outsider — this is blast-radius/least-privilege, not remote RCE; minisign still blocks arbitrary-code installs, leaving rollback/DoS/defacement. **Fix:** scope OIDC `sub` to protected refs; protect `nightlies` + `chore/aztec-*`; split S3 write by prefix per pipeline; require ≥1 review on `main`. **Effort:** hours–days.

### [MEDIUM] F-006: `_publish-sdk.yml` `dist_tag` shell injection → token exfiltration
**Impact:** Medium (Confidentiality of `NPM_TOKEN`/`GH_TOKEN`, integrity of the published package; all SDK consumers; requires workflow-dispatch/write privilege) · **Confidence:** high · **Mapping:** OWASP A03 / CWE-78 · **Found by:** Codex (verified) · **Instances:** `.github/workflows/_publish-sdk.yml:101,108,124,126`

The `workflow_dispatch` input `dist_tag` is interpolated raw into `run:` — unquoted in `npm publish … --tag ${{ inputs.dist_tag }}` (line 101, with `NODE_AUTH_TOKEN` in env) and inside a double-quoted `NOTES=` in a later step carrying `GH_TOKEN` (line 124). GitHub expands `${{ }}` before bash executes, so a crafted `dist_tag` runs attacker commands with the tokens present. Claude's cluster-10 leg missed this; Codex caught it; confirmed by reading the file. **Fix:** pass `dist_tag` via `env:` and reference it quoted (`"$DIST_TAG"`); validate against `^[a-z0-9._-]+$`. **Effort:** hours.

### [MEDIUM] F-007: `download-bb.ts` poisons the runtime-trusted bb cache (no integrity check)
**Impact:** Medium (Confidentiality/Integrity — a malicious `bb` reads the witness; one user; supply-chain/local) · **Confidence:** high · **Mapping:** OWASP A08 / CWE-494 · **Found by:** both · **Instances:** `scripts/download-bb.ts:32-106`; `packages/accelerator/core/src/versions/cache_layout.rs:7-30`; `packages/accelerator/core/src/server/prove.rs:75`; `packages/accelerator/core/src/bb.rs:31-35`

`download-bb.ts` (exposed as `package.json` script `bb:download`) fetches and `tar -xzf`s the `bb` tarball with **no** SHA/signature check into `~/.aztec-accelerator/versions/{version}/bb` — the exact cache layout the Rust runtime trusts. The runtime's own downloader (`downloader.rs`) is fail-closed, but on a **cache hit** it short-circuits and executes the cached binary without ever re-verifying a digest. So a compromised/MITM'd tarball installed by the script becomes a trusted binary that later reads the witness via `--ivc_inputs_path`. **Fix:** mirror the Rust fail-closed digest check in the script, or have the runtime verify a persisted digest marker on every cache hit rather than trusting file existence. **Effort:** hours.

### [MEDIUM] F-008: Windows `bb.exe` checksum pin is trust-on-first-use
**Impact:** Medium (Integrity of the shipped `bb.exe`; all Windows users of the release; supply-chain) · **Confidence:** high · **Mapping:** OWASP A08 / CWE-494 · **Found by:** both · **Instances:** `scripts/update-aztec-version.ts:79-93,137`; `packages/accelerator/scripts/copy-bb.ts:56-148`

`update-aztec-version.ts` computes the SHA-256 of whatever the GitHub release URL returns and writes it into `copy-bb.ts`'s `WINDOWS_BB_CHECKSUMS`; `copy-bb.ts` then "verifies" future downloads against that self-derived pin. With 0 required reviews the auto-pin merges unreviewed, so a compromised upstream asset at pin-time is blessed and shipped. The "review-gated pin" comment is contradicted by the live branch protection. **Fix:** derive the pin from an independent source (signed upstream checksums / a second mirror / reproducible build) and require human review of checksum diffs. **Effort:** hours–days.

### [MEDIUM] F-009: `/prove` buffers the full body before the concurrency semaphore (memory DoS)
**Impact:** Medium (Availability; one instance; requires approved origin or local no-Origin) · **Confidence:** high · **Mapping:** OWASP API4 / CWE-770 · **Found by:** Codex (verified) · **Instances:** `packages/accelerator/core/src/server/prove.rs:110,112,121`

Authorization runs first (good), but the handler then buffers up to 50MB (`to_bytes(raw_body, 50MB)`) **before** acquiring `prove_semaphore`. Many concurrent requests from an approved malicious dApp (or a local no-Origin client) each hold up to 50MB while queued → gigabytes resident → app/session instability. **Fix:** acquire the permit (or a bounded memory budget / connection cap) before buffering; or stream the body to the temp file under the permit. **Effort:** hours.

### [LOW] F-010: Linux crash-recovery systemd-unit path injection
**Impact:** Low (Integrity/local code-exec; needs attacker-controlled exe path) · **Confidence:** moderate · **Mapping:** OWASP A03 / CWE-74 · **Found by:** both · **Instances:** `packages/accelerator/src-tauri/src/crash_recovery.rs:134-184`
`ExecStart="{current_exe()}"` is written into a systemd unit with no escaping; the Windows sibling XML-escapes its path. If the app runs from a path containing quotes/newlines/`%`, injected `[Service]` directives execute at user privilege on autostart. Unusual precondition → Low. **Fix:** reject control chars / apply `systemd-escape` semantics. **Effort:** hours.

### [LOW] F-011: Trailing-dot origin canonicalization collapse
**Impact:** Low (Authorization; low exploitability) · **Confidence:** high (code), low (exploit) · **Mapping:** OWASP A01 / CWE-863 · **Found by:** Codex (verified) · **Instances:** `packages/accelerator/core/src/authorization.rs:37`
`host.trim_end_matches('.')` merges the RFC-6454-distinct origin `https://x.` into approved `https://x`, so a page whose browser Origin is the trailing-dot FQDN of an approved site skips the prompt (and borrows the verified badge). Exploiting it needs the attacker to serve *distinct* content at the dotted FQDN, which normally resolves to the same server → Low. **Fix:** reject trailing-dot origins (don't strip the dot). **Effort:** hours.

### [LOW] F-012: Global Tauri IPC + no CSP (defense-in-depth)
**Impact:** Low (no current injection vector; blast-radius amplifier) · **Confidence:** high (architecture) · **Mapping:** OWASP A05 / CWE-1021, CWE-862 · **Found by:** Codex · **Instances:** `packages/accelerator/src-tauri/src/main.rs:447-460`; `packages/accelerator/src-tauri/tauri.conf.json` (no `app.security.csp`; `withGlobalTauri:true`)
All commands share one global `invoke_handler` and no CSP is set, so any *future* webview XSS would reach `respond_auth`/`remove_approved_origin`/`set_auto_update`. No injection sink exists today (frontend is `textContent`/`createElement` only), so this is hardening, not a live bug. **Fix:** set a strict CSP, drop `withGlobalTauri`, scope commands per-window. **Effort:** hours.

### [LOW] F-013: Headless mode auto-approves every localhost origin
**Impact:** Low (Authorization on multi-tenant hosts; documented intent) · **Confidence:** high · **Mapping:** OWASP A01 / CWE-863 · **Found by:** Claude (verified) · **Instances:** `packages/accelerator/server/src/main.rs:78`
Gated headless mode hardcodes `auto_approve_localhost: true` (documented SEC-04/R13), so any localhost-claiming origin (any port) bypasses the operator's `ALLOWED_ORIGINS`. Real gap where untrusted local processes share the host. **Fix:** make localhost auto-approve an explicit headless opt-in. **Effort:** hours.

### [LOW] F-014: Authorize popup overflows on a long origin; "Remember" pre-checked
**Impact:** Low (UI trust misrepresentation) · **Confidence:** high · **Mapping:** OWASP A04 / CWE-451 · **Found by:** Claude (verified, downgraded) · **Instances:** `packages/accelerator/src-tauri/frontend/authorize.html:20,23,38`; `packages/accelerator/src-tauri/frontend/style.css` (`.popup-detail` word-break; `.popup-container` fixed height)
`.popup-detail` uses `word-break: break-all` inside a fixed-height (`100vh-40px`), non-scrolling popup, so a long origin wraps and overflows — vertically clipping the origin box and/or pushing buttons off-screen — while "Remember this site" is pre-checked. (Not the clean horizontal truncation the raw finding implied, hence Low.) **Fix:** middle-ellipsis that preserves the registrable domain, make the popup scroll, default "Remember" unchecked. **Effort:** hours.

### [LOW] F-015: Mutable major-tag pinning of third-party Actions
**Impact:** Low (supply chain) · **Confidence:** high · **Mapping:** OWASP A08 / CWE-1357 · **Found by:** Claude · **Instances:** `.github/workflows/*` (e.g. `aws-actions/configure-aws-credentials@v6`)
Third-party actions are pinned by mutable major tag — including the one that mints the AWS deploy session — while `create-github-app-token` is already SHA-pinned, so the gap is inconsistency, not policy. A compromised tag steals the OIDC/AWS session. **Fix:** SHA-pin all third-party actions. **Effort:** hours.

### [LOW] F-016: CA signing key not explicitly zeroized
**Impact:** Low (Confidentiality of the localhost CA key; needs local memory read) · **Confidence:** moderate · **Mapping:** OWASP A02 / CWE-212 · **Found by:** Claude · **Instances:** `packages/accelerator/src-tauri/src/certs.rs` (no `.zeroize()` call); `Cargo.toml:53` (rcgen `zeroize` feature enabled)
The keyless-CA design claims the in-memory CA key is scrubbed, but no code invokes zeroization; whether rcgen 0.13's Drop fully scrubs the ring/aws-lc-backed key (vs only outer DER) isn't verifiable here. Impact requires local process-memory access (ptrace/core/swap) — already high privilege — and the CA is NameConstraint-scoped to localhost. **Fix:** wrap key material in `Zeroizing` / confirm rcgen's ZeroizeOnDrop coverage. **Effort:** hours.

---

## Findings NOT pursued (dropped during reduce/verify)

- **Classic IDN/punycode homograph of the origin** — ruled out; `canonicalize_origin` normalizes via `url`/`idna` before the popup renders (pinned test proves no homograph collision).
- **Host/`:authority` DNS-rebinding bypass** — the guard held against absolute-form URIs, HTTP/2-authority vs Host disagreement, IPv6 zone id, userinfo, trailing-dot, case, and layer ordering.
- **`respond_auth` origin self-approval over HTTP** — the `request_id` is UUIDv4 and never disclosed outside its own popup window; no bypass today.
- **bb subprocess argv/env/stderr witness leak; TOCTOU/symlink on the temp path** — witness travels only as file *content*; the client receives a generic error, never bb output or paths.
- **`ALLOWED_ORIGINS` parse widening** — fail-closed on empty/whitespace/wildcard/malformed; `--allow-all` is mutually exclusive and fails loud.
- **`verified-sites.json` `displayName` missing the ASCII guard the `origin` field has** — the raw ground-truth origin is always shown separately; curator-only vector → cross-cutting note, not a finding.
- **Config-persistence TOCTOU, macOS `security` argv injection, version-string path traversal, tar decompression-bomb / path-traversal in extraction, digest fail-open** — investigated per cluster; no concrete trace.

## Cross-cutting observations

- **The SDK↔accelerator channel has no mutual identity.** F-001, F-002, and F-011 are all facets of "the code trusts a party it never authenticates" — the SDK trusts any local `/health` responder, the app trusts any local `/health` incumbent, and the origin comparator trusts a normalized string. A single design decision — a per-install identity (pinned key / token) exchanged through the `0o700` app data dir, used both by the SDK to authenticate the server and by the app to authenticate an incumbent — closes the first two and reframes the third.
- **The witness is protected in transit-logic but not at rest.** Deny-by-default authz, generic error bodies, and no-witness-in-logs are all present and correct; the gap (F-003) is a single missing `permissions(0o700)` on the one place the witness touches disk. Low effort, high symbolic importance for a privacy tool.
- **The update + deploy path concentrates trust.** F-004 (version unbound from artifact), F-005 (whole-bucket write from unprotected branches), F-006 (token-bearing injectable workflow), F-008 (self-derived pin), and F-015 (mutable action tags) are independent but compounding: each widens what a single CI/owner-credential compromise achieves. Tightening OIDC `sub` scoping, per-prefix S3 writes, and version→artifact binding shrinks the whole blast radius at once.
- **`verified-sites.json` `displayName` lacks the ASCII/bidi guard its `origin` sibling enforces** — low impact (ground-truth origin always shown) but worth a one-line validation to prevent a Trojan-Source display name slipping past single-curator review.

## Companion

- HTML (stakeholder-facing): `report.html` (same directory).
- Raw per-cluster outputs: `raw/*-claude.md`, `raw/*-codex.md`. Repo map: `raw/repo-map/`.
- Consolidated + verified certificates: `findings/consolidated.md`, `findings/verified.md`.
