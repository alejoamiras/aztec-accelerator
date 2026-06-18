# Harden Report: security (verification re-run)
**Repo:** aztec-accelerator
**Date:** 2026-06-09
**Effort:** high (scoped verification re-run)
**Run ID:** 2026-06-09-accel-reaudit-7f2a
**Models:** 2× Claude opus (cluster agents) + Codex xhigh (consolidated) → main-agent reduce
**Scope:** `packages/accelerator` (core + server + src-tauri). Excluded: `target/`, generated, `node_modules`, the SDK/playground/landing packages (no trust boundary touched by this work).

## Executive summary
This is a **verification re-run** confirming the `security-hardening-2026-06-09` effort (5 merged PRs #338–#342 + post-impl fixes M1/L3) actually closed its findings in the merged + working-tree code, and that nothing regressed in the touched trust boundaries. It is NOT a cold discovery sweep — the surface received a full blueprint-deep double-audit pre-impl, per-PR CI, two `/code-review max` agents, and a fresh codex post-impl pass days earlier; this run independently re-verifies closure with cross-model coverage.

**Verdict: all 8 hardening findings (SEC-01, SEC-01b/c, SEC-03, SEC-04, SEC-05, SEC-06, SEC-07, SEC-08) and both post-impl fixes (M1, L3) are CONFIRMED CLOSED**, source→sink, by all three agents. **One Medium residual remains** — the updater memory-DoS (SEC-03/M2) — which is the already-tracked, deliberately-deferred R3 residual (#345). Two known deferrals stay open as designed: **SEC-02** (#343, upstream bb signing) and **SEC-09** (#344, owner macOS smoke). No new High/Critical.

## Methodology
Map-reduce, but **deliberately deviated from the full 10-cluster cold ceremony** (documented honestly per the skill): the surface is small and already heavily audited, so it was consolidated into 2 trust-boundary clusters + 1 consolidated codex pass.
- **Cluster A (ingress + auth):** `server/host.rs`, `server.rs` (router/health), `server/auth.rs`, `authorization.rs`, `config.rs`, `commands.rs` (respond_auth/enable_safari_support), `windows.rs` → 1 Claude opus agent.
- **Cluster B (supply-chain + TLS + headless):** `versions/downloader.rs`, `versions/mod.rs`, `updater.rs`, `certs.rs`, `main.rs` (startup gate), `server/src/main.rs` (resolve_gating) → 1 Claude opus agent.
- **Consolidated cross-family pass:** Codex xhigh over the whole accelerator security surface.
- **Cross-model coverage** (the skill's core invariant) is preserved: Claude+Codex both ran. The agents read the **actually-resolved dependency source** (http 1.4.0 `Authority` parser, uuid 1.23 v4, the vendored `tauri-plugin-updater-2.10.1`) — not just the app code.
- Negative list applied (no test/fixture findings unless production-wired; no defense-in-depth wishlist; concrete trace required). Tests not run inside codex's read-only sandbox; the local `cargo test`/`clippy` + `bun run test` gate ran green separately.

## Findings

### [MEDIUM] R7A-01: Updater preflight size cap is defeated by manifest tampering
**Impact:** Availability (memory-DoS). **Confidence:** high (Codex + both Claude agents converge on the mechanism). **Mapping:** CWE-400 (Uncontrolled Resource Consumption). **Found by:** codex (re-flagged) + claude (noted as the known deferral). **Status:** KNOWN DEFERRAL — tracked #345, accepted in audit R3.

**Instances:** `packages/accelerator/src-tauri/src/updater.rs:67-81, 88-118`

**Description:** The SEC-03 preflight rejects an update whose advertised `size` exceeds 500 MB *before* `download()`. But `size` lives in the same attacker-supplied `raw_json` feed, so it is not an independent authority. A tampered `latest.json` defeats the cap by **omitting** `size` (the `None` arm proceeds) OR by declaring a **small false `size`** while `url` points at a huge blob. The plugin then buffers the whole artifact into an unbounded `Vec` *before* `verify_signature` (vendored `tauri-plugin-updater-2.10.1/src/updater.rs:696-713`), so memory is exhausted before the signature rejects the bytes.

**Why it matters:** A feed-compromising attacker (S3/CloudFront compromise, or TLS MITM) can OOM the updater. Integrity is NOT affected — minisign still rejects tampered bytes; this is availability-only and requires feed compromise.

**Why deferred (not fixed here):** The only real fix is an independent bound on bytes actually read (a streaming abort cap), which `tauri-plugin-updater` does not expose. Closing it needs upstream plugin support OR replacing the verified download path — and that self-managed reqwest+minisign rewrite was **rejected in audit R3** (a hand-rolled verify would become the sole authenticity control = signature-bypass risk, a strictly worse trade). "Make `size` mandatory" is **insufficient** (a present size can lie) — #345 updated to reflect this. Owner/release decision.

## Findings NOT pursued (with reasoning)
- **No-Origin loopback auto-approval** (ingress agent) — a local non-browser process that omits `Origin` and sends a loopback `Host` is allowed. This is the correct trust model for a loopback IPC surface (a local process already has the user's privileges); the DNS-rebinding no-Origin variant is still 403'd at the Host guard. Documented + tested; not a finding.
- All other agent observations resolved to "confirmed closed" (see below).

## Confirmed closed (source→sink, by ≥2 agents)
- **SEC-01** Host guard — `core/src/server/host.rs:22-74`, outermost via `server.rs:204-228`, reused on HTTPS `src-tauri/src/server/tls.rs:21-23`. No DNS-rebinding/userinfo/IPv4-mapped/decimal/wrong-port/h1-h2-disagree/CRLF bypass; no false-403 on real SDK/Safari-h1/curl forms. The `authority.contains('@')` userinfo pre-check (`host.rs:24`) is load-bearing (http 1.4.0 `host()` does `rsplit('@')`).
- **SEC-01b/c** headless deny-by-default — `server/src/main.rs:57-86,140-148`; only explicit `--allow-all`/`ACCEL_ALLOW_ALL` disables gating; `Gated([])` denies non-localhost.
- **SEC-04** localhost prompt-once — `config.rs:56-73` desktop default false; `authorization.rs:269-276` honors it; only headless sets true.
- **SEC-05** /health Origin-tier — `server.rs:237-309`; detailed only for absent-or-approved Origin; present-unapproved/malformed → minimal body.
- **SEC-06** per-request auth id — `authorization.rs:171-246`, `server/auth.rs:76-99`, `commands.rs:108-135`; request-id-keyed end to end; the 122-bit v4 UUID never appears in any network-reachable response; both maps cleared on resolve; dual 60s timeouts.
- **SEC-07** bb decompression cap — `versions/downloader.rs:220-299`; 512 MB cumulative running counter under `GzDecoder` + per-entry declared-size check; lying header trips at cap+1; no off-by-one.
- **SEC-08** cert fail-closed — `src-tauri/src/main.rs:419-428` (startup) AND `commands.rs:151-180` (Settings toggle, the M1 fix); both gate HTTPS-start on `migrate_legacy_ca_key()` which re-checks + returns Err if the legacy key persists.
- **M1** SEC-08 toggle gate — `commands.rs:162` runs the fail-closed migration before `spawn_https`.
- **L3** request_id-keyed popup — `windows.rs:77-127`, `commands.rs:126-135`, `frontend/authorize.html:35-54`; popup label = SHA-256(request_id); stale timeout can't close a newer same-origin window.

## Deferrals still open (as designed)
- **SEC-02** (#343) — circular GitHub-asset digest (`versions/mod.rs:287`); needs upstream bb publisher signing. Comment present + accurate.
- **M2 / R7A-01** (#345) — updater memory-DoS above.
- **SEC-09** (#344) — macOS Keychain negative-binding manual smoke; owner-run, can't run in CI.

## Cross-cutting observations
- **Trust-boundary topology is now coherent:** the loopback Host guard is the single outermost gate on both listeners, the Origin gate sits behind it, and identity (auth) is request-id-keyed end-to-end including the UI. The one structural residual (updater) is isolated to a third-party plugin's buffer-before-verify design — an upstream limitation, not a local design flaw.
- **Cross-model signal:** Claude classified the updater residual as "known deferral, not re-flagged"; Codex re-flagged it as Medium. Substantively convergent (same mechanism, same file:line) — the disagreement is labeling, not facts. That convergence is the strongest evidence the residual is real and correctly scoped.
