# Codex post-impl audit — closeout (all 4 PRs merged)

Session `019ead05-4da7-7653-bb86-3df2b38fadb8`. Scope: the full net diff `d3311dd..HEAD` (PR-1+PR-2+PR-3+PR-4), reviewed AFTER two Claude `/code-review max` agents (Rust + SDK) both returned `[]`. Codex ran at xhigh, read-only, adversarial framing ("find what the Claude agents missed").

## Verdict: SHIP-WITH-CHANGES — no Critical / High / Medium

### Low (1) — addressed
**Misleading `ALLOWED_ORIGINS` "deny-all" comments** (`server/src/main.rs`). The F-02 comments said a present-but-empty `ALLOWED_ORIGINS` "denies ALL browser origins." That overstates the boundary: `AuthorizationManager::is_auto_approved` (authorization.rs:213) still auto-approves `localhost` / `127.0.0.1` / `[::1]` regardless of the approved list (`is_approved = is_auto_approved(o) || approved.contains(o)`, authorization.rs:230). An operator setting `ALLOWED_ORIGINS=""` expecting a hard browser lockout would still leave localhost pages able to hit `/prove`. Documentation/posture bug, **not** a codepath regression (the localhost auto-approve is pre-existing + intended for the playground/dev dApps).

**Fix (this closeout):** corrected the three comments (`main.rs:44`, `:88`, `:125`) to state "denies every NON-localhost origin; localhost/127.0.0.1/[::1] stay auto-approved via `is_auto_approved`." Code unchanged (behavior is correct + intended). Verified: `cargo fmt --check` clean, server crate builds.

### Looks correct (codex confirmed, independent of the Claude agents)
- **F-02**: no non-canonical `Origin` bypass — ingress canonicalizes into `CanonicalOrigin` before approval; the lenient config-field deserializer prevents old mixed-case/default-port on-disk entries from bricking startup (while `CanonicalOrigin`'s own Deserialize stays strict).
- **F-01 / F-08**: semaphore always present + held across resolve/download/prove; `StatusGuard` returns `Idle` on all post-status early returns incl. download failure.
- **F-03 / F-09**: `.setup` extraction + shared `spawn_https` preserved side-effect order; the two TLS-failure policies stay distinct.
- **F-04**: `versions` split preserved the public surface; no dropped re-export / reordered macOS finalize tail.
- **F-07**: `CertPaths` introduces no new regression (the pre-existing mid-swap partial-set risk is unchanged — noted as out-of-scope for this refactor).
- **F-06**: `/health` `fetch`→`ky` materially equivalent on 2xx, non-2xx, timeout/network-failure, and no hidden retry.

## Net result
Two Claude review agents (0 findings) + codex (0 Critical/High/Medium, 1 Low comment fix applied) converge: the refactors are behavior-preserving as claimed; F-02's trust boundary is sound. Closeout carries the comment fix + the plan/lessons/index finalization.
