# Planner briefing — security-hardening-2026-06-09

Produce an **implementation plan** to fix the security findings from the `/harden security` audit of the **aztec-accelerator** (repo root = cwd). Full findings: `audit/security/2026-06-09-accel-closeout/report.md` (read it). This is a `/blueprint deep` planning task — be adversarial, attack your own assumptions, and think about what an attacker does AFTER each fix.

## What the app is (threat model)
A Tauri desktop app + a headless server binary, both running a localhost HTTP/HTTPS server (`:59833`/`:59834`) that **arbitrary browser web pages POST to `/prove`** to offload Aztec ZK proving to a native `bb` binary. Origin approval is MetaMask-style (popup). It also auto-updates (signed, pinned minisign — confirmed sound). Attackers: a malicious web page, a DNS-rebinding page, a local-foothold process, a network MITM, a compromised upstream release.

## Locked decisions (from the user — do NOT relitigate)
1. **Scope = all findings EXCEPT SEC-02, which is DEFERRED.** Implement SEC-01, SEC-03, SEC-04, SEC-05, SEC-06, SEC-07, SEC-08, SEC-09.
2. **SEC-01 headless: deny-by-default + opt-in.** Flip "unset `ALLOWED_ORIGINS` ⇒ approve everyone" to "unset ⇒ deny all non-localhost"; add an explicit opt-in (`--allow-all` flag or `ACCEL_ALLOW_ALL=1` env) to restore today's open behavior. **Docs MUST be updated** (README headless section, the `//!` module doc in `server/src/main.rs`, CLAUDE.md if relevant).
3. **SEC-02 DEFERRED with a tracked note.** Rationale (user): an in-app pinned-digest manifest is impractical because barretenberg **nightlies ship every night** → the manifest would be perpetually stale. The right fix is verifying an **upstream publisher signature**, which Aztec does not currently provide for `bb`. Leave a durable note: a code comment/TODO at the digest-fetch site (`core/src/versions/mod.rs` ~`fetch_github_asset_digest`) AND a tracking entry (plan + suggest a GitHub issue / roadmap note) to implement signature verification once upstream signs `bb`. Do NOT implement a pinned-digest manifest.
4. **Rollout: production + `/harden security` re-audit after.** These ship to auto-updating users. Plan must keep the full CI suite + WebDriver E2E green; `main` is branch-protected (branch → PR → green CI → merge).

## In-scope findings (file:line from the audit — verify against current code)
- **SEC-01 [HIGH]** — origin gate bypass. `core/src/server/auth.rs:30` (`None => Ok(())` absent-Origin fail-open; pinned by test `prove_skips_auth_when_no_origin_header` at `core/src/server.rs:908`), no `Host`-header validation anywhere, `server/src/main.rs:43` headless default-open, wildcard CORS `server.rs:194`. Fix: **(a) `Host`-header allowlist** at ingress (reject Host whose host ∉ {127.0.0.1, localhost, [::1]}+port) — this alone defeats DNS rebinding; **(b) fail-closed on absent `Origin`** for the browser path (the SDK always sends Origin on cross-origin `/prove`; a loopback-`Host` request may legitimately omit it — reconcile carefully so the SDK + Safari HTTPS still work); **(c) headless deny-by-default + opt-in** per decision #2.
- **SEC-03 [MED]** — `src-tauri/src/updater.rs:66` updater buffers whole artifact in memory before signature verify, no size cap. Fix: hard size cap from `latest.json` + ceiling; signature path itself is sound.
- **SEC-04 [MED]** — `core/src/authorization.rs:213-223` `is_auto_approved` trusts ANY `{localhost,127.0.0.1,[::1]}:<any-port>`. Tension: the **playground (localhost:5173) and dev dApps RELY on zero-config localhost auto-approve** — do NOT break that. Consider whether SEC-01's Host-check already neutralizes the rebinding vector, leaving SEC-04 as an optional configurable restriction (e.g. an allowlist/denylist of localhost origins, or a setting) rather than a hard narrowing. Resolve this knot explicitly.
- **SEC-05 [MED]** — `core/src/server.rs:194,215` `/health` unauthenticated + wildcard CORS leaks app version, cached versions, `bb_available`, https status → cross-site fingerprinting. Fix: restrict `/health` CORS to approved origins, or return a minimal unauthenticated liveness surface and gate the detail.
- **SEC-06 [MED]** — `src-tauri/src/commands.rs:131` `respond_auth` resolves on raw origin string (origin-as-identity); canonical-keyed pending map (`core/src/server/auth.rs:71`); no per-request token. Fix: server-issued opaque per-request id; the popup carries+returns it; resolve by id. **Cross-package** (Tauri backend + the popup frontend JS) — preserve the IPC contract / avoid a frontend break, or update both.
- **SEC-07 [LOW]** — `core/src/versions/downloader.rs:240` tarball extraction unbounded (64MB cap is compressed-only). Fix: cap cumulative decompressed bytes / check `entry.header().size()`.
- **SEC-08 [LOW]** — `src-tauri/src/certs.rs:181-198` (called `main.rs:418/422`) legacy `ca.key` delete is best-effort; failure leaves the mint-any-cert primitive + proceeds. Fix: verify gone / retry / surface; consider fail-closed for Safari HTTPS until removed.
- **SEC-09 [LOW]** — `src-tauri/src/certs.rs:298-327` rotation verifies staged CA anchor but not that the staged leaf chains to it; 3 non-atomic renames. Fix: `verify-cert -c leaf -r ca` before swap; atomic staging-dir swap.

## What I want from your plan
1. **Phased PR structure** — group the 8 findings into coherent, independently-reviewable, package-aligned PRs (the repo convention: per-package CI gates `accelerator.yml`/`sdk.yml`; behavior-preserving where possible; the headless deny-by-default + SEC-06 IPC change are the breaking/cross-package ones). Order by risk/dependency.
2. **Per-finding: concrete code shape** (function signatures, where the check goes, the new flag/env, the test that proves it + the test that proves the attack is now blocked). Especially nail SEC-01's Host-check + the absent-Origin reconciliation so the SDK (HTTP + Safari HTTPS) and the playground still work.
3. **The SEC-01/SEC-04 localhost-trust knot** — resolve it explicitly with reasoning.
4. **Test plan** — incl. a DNS-rebinding regression test (forged Host header) and a headless-deny-by-default test; what existing tests/e2e must change (e.g. the `prove_skips_auth_when_no_origin_header` contract flip; any e2e that runs the headless server without ALLOWED_ORIGINS).
5. **Migration & docs** — the headless behavior change (opt-in flag + every doc surface), and any auto-update rollout/rollback note (these reach real users).
6. **Security & Adversarial Considerations** section + an **Assumptions** section (Facts with file:line / Inferences / Asks).

### Adversarial asks (mandatory)
- After each fix, what does the attacker do next? Does the Host-check have a bypass (IPv6 forms, `0.0.0.0`, decimal IP, missing Host, `Host: localhost` from a rebound page served on :59833, port confusion)? Does fail-closed-on-missing-Origin break any legitimate caller (the SDK, Safari, curl-based health, the e2e harness)?
- Attack the Assumptions: which "the playground relies on X" / "the SDK always sends Origin" claims are unverified and could make a fix break production?
- Least-privilege / supply-chain / crypto angle on each change.

Respond with a complete, well-structured implementation plan (markdown). Be concrete and opinionated; flag where you're unsure.
