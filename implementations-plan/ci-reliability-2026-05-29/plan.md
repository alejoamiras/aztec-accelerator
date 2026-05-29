# CI Reliability — WebDriver flake fix + codex post-1.0.2 hardening

**Status**: v2 — final codex pass returned **"ship-to-approval: v2 is sound"** (cfg matrix walked, no dead-code, no residual prompt path, security clean). Pending user approval.
**Date**: 2026-05-29
**Type**: Tier A (cross-cutting: product bug + CI/test infra + release hardening)
**No release**: per user — land fixes to `main`, cut a version another day.

## Context

`E2E WebDriver (linux)` fails on every PR since `accelerator-v1.0.2` shipped, wedging `main` (required `Accelerator Status` aggregator red). Root cause fully diagnosed in `diagnosis.md`: the **ungated background update check** (`main.rs:345-354`) pops the update-prompt window ~5s into every `--features webdriver` run now that `1.0.2` is on the prod updater feed, stealing WebDriver's active window so `settings.spec` can't find the Settings DOM. Plus codex post-1.0.2 audit follow-ups (server lock, version patch, bundle invariant, updater E2E).

## Consolidation trace (which decision came from where)

- **WS1 self-unblock claim TRUE** — verified by both codex + opus against `_e2e-webdriver.yml:54` (dev `bunx tauri dev --features webdriver`, release `cargo build --release --features webdriver`). No ruleset change needed.
- **Gate `debug_assertions` too, not just `webdriver`** — codex (opus wanted feature-only). Adopted codex: a human's `cargo tauri dev` shouldn't get a prod modal, and it kills the latent `_e2e.yml cargo run` variant opus separately flagged.
- **Also `#[cfg]` the `run_update_check` fn, not just the spawn** — opus (dead-code under feature). Adopted.
- **`--locked` not `--frozen`** — both. Adopted.
- **No cargo workspace this round** — both (a workspace would NOT reintroduce the 1.0.2 bundle bug — that was same-package bin auto-discovery, not workspace membership — but it changes lock/target/metadata semantics during a hotfix for little payoff). Adopted: keep separate crates + separate `server/Cargo.lock`.
- **WS4 reframed/demoted** — codex proved `/health` version comes from the shared lib (`server.rs:145`, `env!` of `aztec-accelerator` = `src-tauri/Cargo.toml`, already patched), NOT `server/Cargo.toml`. So WS4 as a correctness fix is **rejected**. Kept only as optional metadata hygiene + an optional real `--version` observable.
- **WS6 downgraded to manual runbook + deferred automation** — both. The updater verifies Ed25519 against the embedded pubkey and enforces TLS; a published N-1 binary can't be redirected to a throwaway-key localhost feed without new Rust override code, and the gate must be post-build. WS5's bundle invariant already catches the specific 1.0.1 signature-shape regression deterministically.
- **WS2 out of the unblock PR; anchor must FAIL (not mask)** — codex. Adopted: WS2 → PR-B, and the anchor switches-to-Settings-if-present but fails if the bootstrap window is absent (no `navigateTo` masking, no auto-dismiss in smoke).

## Goal / success criteria

1. `E2E WebDriver (linux)` green + deterministic — no prod update check in webdriver/dev/CI builds.
2. Fix PR self-unblocks `main` (no branch-protection change; `Accelerator Status` stays required).
3. codex must-fix landed: `server/Cargo.lock` committed + built `--locked`.
4. Bundle invariant asserts the exact `Contents/MacOS` entry set (any type).
5. A documented manual pre-release updater runbook exists; automated updater-E2E scoped as a future plan.

## Non-goals (deferred)

- Shared-core crate extraction (server keeps its path-dep on `aztec-accelerator`).
- Cutting a release.
- Automated updater-path E2E (own future plan; needs post-build privileged smoke or a test-only Rust updater override + throwaway key).
- Cargo workspace conversion.

## Workstreams

### WS1 — Gate the background update check [PR-A, the fix]

`main.rs:345-354` spawns the poller unconditionally. Changes (all in `main.rs`):

```rust
// Defined only for non-webdriver builds — avoids dead_code under the feature.
#[cfg(not(feature = "webdriver"))]
async fn run_update_check(app: &AppHandle, config_state: &ConfigState) { /* unchanged body */ }

// Decide whether the background poller should run at all.
#[cfg(not(feature = "webdriver"))]
fn should_poll_for_updates() -> bool {
    if std::env::var("AZTEC_ACCEL_NO_UPDATE").is_ok() {
        tracing::warn!("AZTEC_ACCEL_NO_UPDATE set — background update checks suppressed");
        return false;
    }
    // A dev build (`cargo tauri dev`) must not poll the PROD feed or pop a modal
    // unless explicitly opted in. This also covers `_e2e.yml`'s debug `cargo run`.
    if cfg!(debug_assertions) && std::env::var("AZTEC_ACCEL_FORCE_UPDATE_CHECK").is_err() {
        tracing::info!("Debug build — background update checks disabled (set AZTEC_ACCEL_FORCE_UPDATE_CHECK=1 to enable)");
        return false;
    }
    true
}

// In setup():
#[cfg(not(feature = "webdriver"))]
{
    if should_poll_for_updates() {
        let update_handle = app.handle().clone();
        let update_config = config_state.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            loop {
                run_update_check(&update_handle, &update_config).await;
                tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
            }
        });
    }
}
```

- Gate axis = **`webdriver` feature (compile-time) ∪ `debug_assertions` (runtime, opt-in override) ∪ `AZTEC_ACCEL_NO_UPDATE` kill switch**. Kills the entire "non-prod build polls prod feed" class.
- Do **NOT** gate `check_for_update` (the shared primitive) — it's reused by the manual "Update Now" path via `commands.rs respond_update_prompt` + `PendingUpdate`. The guard lives at the background poller only (opus's concern).
- Prod release desktop build (no `webdriver`, release profile, no env) → polls normally. Unchanged behavior.

**Regression coverage (PR-A)**: in `_e2e-webdriver.yml`, after the test step (with `if: always()` so it still signals when tests fail for another reason — codex final nit), assert the runtime log contains **no** `Showing update prompt` (grep `/tmp/tauri.log`). Loud failure if the gate ever regresses.

**PR-A scope = WS1 + that assertion ONLY** (codex: keep the unblock PR minimal).

### WS2 — Self-anchor specs to the Settings window [PR-B]

Add `ensureSettingsWindow()` to `helpers.ts`: enumerate handles, switch to the one whose title is `"Aztec Accelerator Settings"` (or URL ends `settings.html`), then `waitForExist("#speed-label")`. **If no such window exists, FAIL** (do not `navigateTo` / auto-create — that would mask a real bootstrap regression). Call it in `before()` of `smoke.spec` + `settings.spec`. Do **not** auto-dismiss stray windows in smoke. `auth-flow.spec` keeps its existing explicit handle management.

### WS3 — Commit `server/Cargo.lock` + build `--locked` [PR-B, codex must-fix]

- `cargo generate-lockfile --manifest-path packages/accelerator/server/Cargo.toml`; commit `server/Cargo.lock`.
- Build the server with `--locked` (not `--frozen` — frozen forbids network on cold caches) in:
  - `release-accelerator.yml` build-headless
  - `accelerator.yml` Smoke + Release Smoke
  - `_e2e.yml`: replace `cargo run` (recompiles) with `cargo build --locked` once + run the produced binary (codex).
- Keep separate crates (no workspace). Accept manual lock sync; add a brief note in `server/Cargo.toml` that its lock must be regenerated when `src-tauri` deps change.

### WS4 — headless version observable [PR-B, APPROVED]

Not a correctness fix (`/health` already reports the patched `src-tauri` lib version), but the user approved adding it as a real observable: add `accelerator-server --version` (prints the `accelerator-server` crate version) and patch `server/Cargo.toml` version in **both** the `build-headless` step and the `bump-source` job so the observable is accurate per release. CI can assert `accelerator-server --version` == release version in the release dry-run.

### WS5 — Harden the bundle invariant [PR-B, codex should-fix]

`release-accelerator.yml` bundle check: replace `find … -type f` with depth-1 `find -mindepth 1 -maxdepth 1` (any type) → `basename` → sort → exact-diff against `{aztec-accelerator, bb}`. Fail on any deviation (incl. stray symlink/dir). Unit-fixture test: a dir with a stray symlink must fail the check.

### WS6 — (DEFERRED) updater-path validation

- **Now**: write `packages/accelerator/UPDATER_TESTING.md` — a manual pre-release runbook: install the N-1 stable from the real GitHub release, click Update, confirm relaunch + `/health` reports the new version, on macOS. Gate promoting an rc → stable on this check.
- **Note**: WS5's bundle invariant already catches the 1.0.1 signature-shape regression deterministically and for free.
- **Future plan**: automated updater-E2E as a **post-build** privileged release smoke (after the signed bundle exists; current E2E gate runs before patching/signing per `release-accelerator.yml:51`), OR a test-only Rust updater-endpoint/pubkey override (Rust-only, never exposed to frontend/HTTP) signed with a throwaway key. Own design + plan.

## Sequencing

1. **PR-A** — WS1 + the `tauri.log` no-prompt assertion. Minimal, self-unblocking. Merge first → `main` green.
2. **PR-B** — WS2 + WS3 + WS5 (+ optional WS4). Hardening; independent.
3. **Docs** — WS6 manual runbook (can ride PR-B or its own tiny PR).

No release. Branch all off `main` after PR-A merges.

## Test strategy

| WS | Validation |
|----|-----------|
| WS1 | PR-A's own `E2E WebDriver (linux+macos)` green (proves self-unblock) + `tauri.log` shows no `Showing update prompt`. |
| WS2 | Specs fail loudly if the Settings bootstrap window is absent; pass when present. |
| WS3 | Server builds `--locked`; removing the committed lock fails CI. |
| WS5 | Fixture dir with stray symlink fails the invariant; clean `{aztec-accelerator,bb}` passes. |
| WS6 | Manual runbook executed before any future rc→stable promotion. |

## Security & Adversarial Considerations

- **Update gating (WS1)**: gating dev/webdriver does not weaken prod (shipped binary is release + no `webdriver`). `AZTEC_ACCEL_NO_UPDATE` could suppress security updates, but the setter already has code-exec; we `warn!`-log when it fires for auditability. `AZTEC_ACCEL_FORCE_UPDATE_CHECK` only *enables* checks (no downside).
- **`--locked` (WS3)**: committing `server/Cargo.lock` makes the headless tarball's deps reviewed + reproducible — supply-chain improvement. Manual sync risk with `src-tauri`'s lock is mitigated by the shared path-dep deduping; note it for reviewers.
- **Bundle invariant (WS5)**: stronger guard on what ships in the signed `.app`. Pure win; also the standing guard for the 1.0.1 regression class.
- **Updater test override (WS6, future)**: any endpoint/pubkey override must be Rust-only + test-only, never reachable from frontend or the HTTP server. A throwaway-key test build is not the shipped artifact — document that limitation. No prod signing key in CI.
- **No ruleset change**: `Accelerator Status` stays required throughout — strictly better than de-requiring.

## Open questions (for approval gate)

1. WS4: implement the `--version` observable, or drop entirely? (Lean: drop / defer — low value.)
2. WS6 runbook location + whether to also add a CI reminder comment in `release-accelerator.yml`.
3. Confirm we skip the ruleset change (contingent on PR-A's gate going green as predicted — fallback is a one-time admin-bypass of PR-A only).
