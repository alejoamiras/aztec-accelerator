# Harden Report: security
**Repo:** aztec-accelerator
**Date:** 2026-06-09
**Effort:** high (focused scope — see Methodology)
**Run ID:** 2026-06-09-accel-closeout
**Models:** 2× Claude (Opus) cluster agents + Codex (xhigh) full-surface, adversarial
**Scope:** the accelerator's security-critical surface — the localhost server's origin-auth trust boundary, TLS/cert generation, bb-download supply chain, and the auto-update path. SDK localhost trust assumptions in passing. NOT a maximal whole-repo sweep (playground/landing/infra excluded as low-security-surface).

## Executive summary
This audit was scheduled as the post-implementation security pass for `quality-fixes-2026-06-08` (whose F-02 hardened the origin-canonicalization trust boundary). **Every finding below is PRE-EXISTING — none was introduced by the quality-fixes refactors** (F-01..F-09 were behavior-preserving except F-02, which *tightened* canonicalization). The audit deliberately looked past the diff at the standing posture, because the app is a localhost HTTP/HTTPS server that arbitrary browser pages talk to, plus a self-auto-updater — a genuinely adversarial surface.

Posture: **medium overall.** The desktop present-`Origin` auth path, TLS keylessness, and the auto-update (pinned-minisign) path are materially well-built and were independently confirmed sound by both model families. The two **High** findings are: (1) the localhost server's origin gate is bypassable — there is **no `Host`-header validation** and it **fails open** (absent `Origin` on desktop; absent `ALLOWED_ORIGINS` makes the headless binary default-open); and (2) the `bb` binary's integrity check is **circular** — the expected digest is fetched from the same online GitHub control-plane as the binary, so a release/API compromise (or dual-endpoint MITM) installs and executes an attacker binary. Both are fixable with patterns already proven elsewhere in the codebase (a Host allowlist; the auto-updater's pinned-key model).

Recommended priority: **fix the two Highs before any release that widens the install base.** The cheapest high-value fix is the `Host`-header allowlist (hours), which closes the desktop DNS-rebinding vector and hardens the headless default.

## Methodology
Map-reduce, focused scope. Phase 2: 2 Claude cluster agents (origin-auth; crypto+supply-chain) + 1 Codex full-surface agent, all with the adversarial security prompt (attacker-driven exploit framing, source→sink traces, ≤4-function inter-procedural cap). Phase 3 reduce (this doc): dedup by root cause across the 3 sources; cross-model convergence used as the primary confidence signal; severity assigned here (not at map time). **Deviation from the formal spec (stated honestly):** ran 1 Claude + (shared) Codex per concern rather than 2×2 per cluster, and folded Phase 2.5 cross-rebuttal into this reduce — appropriate for a focused post-impl pass in an autonomous loop, not a from-scratch ultra audit. Raw agent outputs: `raw/origin-auth-claude.md`, `raw/crypto-supply-claude.md`, `raw/full-surface-codex.md`.

## Findings

### [HIGH] SEC-01: Localhost server origin gate is bypassable — no `Host`-header validation + fail-open
**Confidence:** high (cross-model convergence on the root gaps)
**Mapping:** CWE-350 (reliance on untrusted inputs / DNS rebinding), CWE-1390 (weak authentication), OWASP A01 (broken access control)
**Found by:** both (Claude traced desktop rebinding; Codex anchored the headless default-open; both independently noted "no Host check")

**Instances:**
- `packages/accelerator/core/src/server/auth.rs:30` — `None => return Ok(())` (absent `Origin` ⇒ approved); pinned by test `prove_skips_auth_when_no_origin_header` (`core/src/server.rs:908`).
- `packages/accelerator/core/src/server/auth.rs:18` + `server/prove.rs:116` — ingress reads only `Origin`; **no `Host` header is read or validated anywhere** (exhaustive grep).
- `packages/accelerator/server/src/main.rs:43` — headless: `ALLOWED_ORIGINS` unset ⇒ `auth_manager: None` ⇒ `authorize_origin` returns `Ok(())` for everything (default-open).
- `packages/accelerator/core/src/server.rs:194` — wildcard CORS (`allow_origin(Any)`) means a passed request's response is fully readable cross-origin (amplifier).
- Server binds `127.0.0.1` only (`server.rs:186`) — so this is browser-mediated (DNS rebinding), not raw network.

**Exploit scenarios:**
- *Headless (simplest, no rebinding):* a developer runs `accelerator-server` without `ALLOWED_ORIGINS` (a plausible default). Any web page they visit can `fetch('http://127.0.0.1:59833/prove', …)` — `auth_manager` is `None`, so it's approved; wildcard CORS lets the page read the proof and trigger an arbitrary `bb` version download via `x-aztec-version`.
- *Desktop (DNS rebinding):* attacker serves a lure page on `http://evil.com:59833`, then rebinds `evil.com → 127.0.0.1` (short TTL). The page's follow-up `fetch('http://evil.com:59833/prove')` is now **same-origin** (same host+port) ⇒ the browser sends **no `Origin` header** ⇒ the server's `None => Ok(())` arm approves it; the `Host: evil.com:59833` is never checked. No approval popup.

**Why it matters:** unapproved arbitrary origins drive native proving (CPU/resource abuse, a proof oracle) and trigger attacker-chosen binary downloads — without the MetaMask-style consent the design promises.

**Recommended fix (cheapest high-value first):**
1. **Add a `Host`-header allowlist** at ingress: reject any request whose `Host` host-component ∉ {`127.0.0.1`, `localhost`, `[::1]`} (+ the bound port). This single check defeats DNS rebinding regardless of `Origin`, on both desktop and headless. (~hours.)
2. **Fail closed on absent `Origin`** for browser-shaped requests (deny, or require it); if non-browser local scripts need access, gate them on the loopback-`Host` check from (1), not a blanket approve. Update/flip `prove_skips_auth_when_no_origin_header` to encode the new contract.
3. **Headless deny-by-default:** treat unset `ALLOWED_ORIGINS` as "deny all non-localhost" (consistent with present-but-empty) rather than `auth_manager: None`, or require an explicit `--allow-all` opt-in.

**Effort:** (1) hours; (1)+(2)+(3) ~1 day incl. tests. **This is the finding to fix first.**
**Cross-model nuance:** Codex did not independently confirm the desktop same-origin-rebinding chain ("no `Origin` bypass found to pair with the missing Host check"); Claude traced it via the same-origin-omits-`Origin` property + the pinning test. Both agree the `Host` allowlist is the correct root fix and that the headless default-open is real.

### [HIGH] SEC-02: `bb` download integrity is circular against a compromised/MITM'd GitHub release
**Confidence:** high (both models; Codex rated High, Claude Medium — taking the higher per blast radius = RCE)
**Mapping:** CWE-494 (download of code without integrity check), CWE-345 (insufficient verification of authenticity), OWASP A08 (software/data integrity failures)
**Found by:** both

**Instances:**
- `packages/accelerator/core/src/versions/mod.rs:282-326` — `fetch_github_asset_digest` reads the expected SHA-256 from the GitHub **API** (`api.github.com`).
- `packages/accelerator/core/src/versions/downloader.rs:31,156-175` — `verify_digest` compares the downloaded tarball to that fetched digest; on match → install + `chmod 0o755` → later executed (`server/prove.rs:200`).
- Admitted gap: comment at `mod.rs:285-288` (unshipped TODO).

**Exploit:** an attacker who compromises the upstream GitHub release (account/CI) or can MITM **both** `api.github.com` and the download host re-uploads a malicious `barretenberg-*.tar.gz` **and** overwrites the asset `digest`. The client fetches the attacker's digest, the malicious binary matches it, verification "passes," and the app executes attacker code. Pure network MITM is blocked (both hops are HTTPS/rustls); the **supply-chain/release-compromise** path is not. macOS ad-hoc codesign provides no provenance.

**Why it matters:** RCE on the user's machine via a trusted-looking update of the proving backend.

**Recommended fix:** verify the binary against a **publisher signature pinned offline in the shipped app** (minisign/cosign/TUF-style), independent of the download channel — **mirror the auto-updater, which already does exactly this** (pinned minisign pubkey, confirmed sound below). Until then, at minimum pin known-good digests in the released app rather than fetching them at install time. **Effort:** days (needs an upstream signing story or a curated pinned-digest manifest).

### [MEDIUM] SEC-03: Updater buffers the entire artifact in memory before signature verification (no size cap)
**Confidence:** moderate (Codex; partly in the `tauri-plugin-updater` dep)
**Mapping:** CWE-400 (uncontrolled resource consumption)
**Found by:** codex
**Instances:** `packages/accelerator/src-tauri/src/updater.rs:66` → `tauri-plugin-updater-2.10.1` `updater.rs:702`.
**Exploit:** a tampered `latest.json` / compromised CDN / MITM points the app at a multi-GB blob; the updater extends a `Vec<u8>` to completion and only rejects (signature) afterward → memory-exhaustion DoS. **Fix:** enforce a hard artifact size cap (from `latest.json` + a sane ceiling) and/or stream to a disk-backed temp file before verifying. Note: the *signature* check itself is sound (pinned minisign) — this is availability only. **Effort:** hours (cap) / days (upstream streaming).

### [MEDIUM] SEC-04: Any localhost-origin page is auto-approved (no per-app scoping)
**Confidence:** high
**Mapping:** CWE-923 (over-broad trust), OWASP A01
**Found by:** claude
**Instances:** `core/src/authorization.rs:213-223` (`is_auto_approved` → any `http(s)://{localhost,127.0.0.1,[::1]}:<any-port>`), used by `is_approved` (`:230`).
**Exploit:** a second local dev server, a malicious `npm postinstall` that spins up a localhost listener, or any local-foothold tool gets unprompted, unlimited `/prove`. **Why it matters:** "anything on localhost is trusted" is broad for a CPU-proving oracle. **Fix:** scope auto-approval (e.g. only the app's own playground origin) or prompt for non-self localhost origins. **Effort:** hours–day (UX decision).

### [MEDIUM] SEC-05: `/health` is unauthenticated with wildcard CORS → cross-site fingerprinting
**Confidence:** high
**Mapping:** CWE-200 (information exposure)
**Found by:** codex
**Instances:** `core/src/server.rs:194,215`.
**Exploit:** any website silently probes whether the accelerator is installed and reads app version, cached Aztec versions, `bb_available`, and HTTPS status — recon for a targeted attack (and a privacy signal). **Fix:** restrict `/health` CORS to approved origins, or return a minimal unauthenticated surface (liveness only) and gate the detail behind approval. **Effort:** hours.

### [MEDIUM] SEC-06: `respond_auth` resolves on the raw (non-canonical) origin; origin-as-identity (no per-request token)
**Confidence:** moderate
**Mapping:** CWE-352-adjacent (missing per-request token), CWE-362 (race)
**Found by:** claude
**Instances:** `src-tauri/src/commands.rs:131` (`auth.resolve(&origin, …)` on the raw arg) vs the canonical-keyed pending map (`server/auth.rs:71`).
**Exploit:** a Deny on a malformed-origin payload is a no-op (real request only dies on the 60s timeout); and because decisions are addressed by origin string (no opaque per-request id), any caller of `respond_auth` with a known canonical origin could resolve a *different* concurrent pending request for that origin as `Allow{remember}`. Bounded today by the Tauri IPC trust boundary (local UI only). **Fix:** server-issued opaque per-request authorization id. **Effort:** day.

### [LOW] SEC-07: Decompression bomb — tarball extraction is unbounded (the 64 MB cap is compressed-only)
`core/src/versions/downloader.rs:240` (`entry.unpack`) vs `:129-149` (compressed cap). Only bites once SEC-02's digest defense is already defeated. **Fix:** cap cumulative decompressed bytes / check `entry.header().size()`. Found by claude.

### [LOW] SEC-08: Legacy `ca.key` cleanup is best-effort — a delete failure silently leaves the mint-any-cert primitive
`src-tauri/src/certs.rs:181-198`, called `main.rs:418/422`. On an upgraded install where `remove_file` fails, the old readable CA key + still-trusted anchor remain; the app proceeds (fails open). **Fix:** verify the key is gone / retry / surface to the user / drop the orphaned anchor; consider failing closed for Safari HTTPS until removed. Found by both (convergence).

### [LOW] SEC-09: Cert rotation verifies the staged CA anchor but not that the staged leaf chains to it; non-atomic 3-rename swap
`src-tauri/src/certs.rs:298-327` (`verify-cert -c ca.pem.new` only); `swap_into` is 3 renames (`:71-76`). A crash mid-swap or future staging divergence could leave a live ca/leaf mismatch no validity check catches until ~expiry. **Fix:** `verify-cert -c leaf -r ca` before swap; atomic staging-dir swap. Found by claude.

## Findings NOT pursued (with reasoning)
- **Locally-trusted CA relies on client-side NameConstraints enforcement** — flagged by Claude as defense-in-depth only; **Codex independently confirmed the keylessness backstop holds** (CA key in-memory only + `zeroize` + tests pin it never-written + leaf SAN limited to localhost). Not a finding today; tracked as: *a future regression that persists the CA key would turn this into a `*.com`-minting Critical* — add a test/guard asserting non-persistence stays true.
- **Global pending cap = liveness DoS / popup spam** (`server.rs:130`) — bounded + self-healing (60s), memory-cap is intended; noted, not pursued.
- **Tarball path traversal** — refuted: `entry.unpack(dest.join(bb_binary_name()))` discards the archive entry's path (fixed destination). Defense holds.

## Cross-cutting observations
1. **The localhost server treats the network position (loopback) and the `Origin` header as the whole trust story, with no `Host` binding.** SEC-01/04/05 are facets of one theme: a browser-reachable local service needs a `Host` allowlist + per-origin consent as defense-in-depth, not `Origin`-only + wildcard CORS. Fixing the `Host` check (SEC-01.1) is the single highest-leverage change.
2. **Two different "trust the network for integrity" instances with opposite maturity:** the auto-updater does it right (pinned minisign, HTTPS, downgrade-prevention — confirmed by both models); the `bb` downloader does it wrong (circular online digest, SEC-02). The fix for SEC-02 is to copy the updater's pattern.
3. **Several "fail-open on error" choices** (absent `Origin` → approve; `ca.key` delete fails → proceed; headless no-allowlist → open). A consistent fail-closed posture on the security-critical paths would close SEC-01, SEC-08 together.

## Defenses that HOLD (independently confirmed by both model families)
- Desktop **present-`Origin`** auth: reads `Origin`, canonicalizes (the F-02 `CanonicalOrigin` work), rejects malformed/null-ish, checks approval **before** body buffering.
- **TLS generation**: CA private key in-memory only (dropped, `zeroize`), CA constrained to localhost/127.0.0.1/[::1], cert+key written atomically with owner-only (0o600) perms.
- **Auto-update**: HTTPS feed + **pinned minisign public key** signature verification + default version comparison preventing downgrades.
- **Network MITM** on the `bb` download is blocked (both hops HTTPS/rustls) — the residual is supply-chain/release-compromise (SEC-02), not passive MITM.
