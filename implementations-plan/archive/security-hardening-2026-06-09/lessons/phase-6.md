# Phase 6 — post-impl codex audit + fixes

**Context:** All 5 PRs merged (#338–#342). `/code-review max --fix` (2 Claude agents) on the merged net-diff returned 2 LOW accepted residuals (size-less-feed + a doc nit) — nothing to commit. Fresh codex post-impl audit (session `019eaece`, a different model family) then swept the same diff.

## Codex verdict: SHIP-WITH-CHANGES — 2 Medium + 1 Low (no High/Critical)

### M1 (FIXED) — SEC-08 enforced only at startup, not the Settings toggle
- **Finding:** `enable_safari_support` (commands.rs) calls `spawn_https` without the fail-closed `migrate_legacy_ca_key()` that the startup path (main.rs:424) runs. On an upgraded macOS install where launch correctly skipped HTTPS because the legacy `ca.key` couldn't be deleted, a Settings off→on toggle re-enables Safari HTTPS next to the readable mint-any-cert key — reopening the exact SEC-08 condition.
- **Root cause:** a **slip against the plan's own R6**, which explicitly listed BOTH `main.rs` AND `enable_safari_support` (commands.rs) as the gate sites. PR-5 implemented only `main.rs`. The audit caught the missing half.
- **Fix:** `certs::migrate_legacy_ca_key().map_err(...)?` at the top of `enable_safari_support`, before `generate_and_save`. Fail-closed + user-surfaced (the Settings UI shows the error string) — stronger than the startup path's silent `tracing::error!` skip, because here the user explicitly asked to enable it.
- **Lesson:** when a plan revision enumerates *multiple* call sites for one control, grep all of them at implementation time. The startup path was the obvious one; the runtime toggle was the one an attacker actually drives.

### L3 (FIXED) — auth popup window keyed by origin, not request_id
- **Finding:** R10 re-keyed auth *resolution* by `request_id`, but the popup *window label* (`auth-{hash(origin)}`), its 60s timeout's `get_webview_window`, and `respond_auth`'s close were all still origin-derived. Race: request A resolves fast → request B (same origin, new `is_first`) opens a popup reusing the label → A's stale timeout fires at A+60s, finds B's window by the shared label, closes it. `resolve(request_id_A)` is a correct no-op, but B's live window is gone and B hangs until its own timeout-deny.
- **Severity:** LOW — UI bookkeeping, not an authorization-property violation (resolution was already request_id-safe via R10/SEC-06).
- **Fix:** label by `request_id` in windows.rs (`show_auth_popup_window`) + commands.rs (`respond_auth` close). Safe because the piggyback gate (`is_first`, server/auth.rs:88) shows at most one popup per origin at a time, so per-request labels never spawn duplicates. `origin` stays on the `respond_auth` payload for a diagnostic `tracing::debug!` only.
- **Lesson:** re-keying an identity (origin→request_id) has to be carried through EVERY layer that consumed the old key — resolution AND the UI lifetime that mirrors it. R10 did the security layer; the UI layer was a half-mile behind.

### M2 (DEFERRED + tracked) — updater `None`-size arm proceeds
- **Finding (sharpened):** the `None => warn + proceed` arm (updater.rs) fully disables SEC-03 when the feed omits `size`. codex's sharpening over the in-code comment: for the **availability** property the signature check is NOT the control, because the plugin buffers the whole artifact BEFORE it verifies — so a manifest-only tamper (strip `size`, point the URL at a multi-GB blob) re-opens the memory-DoS with **no signing key**.
- **Why deferred (not fixed autonomously):**
  1. **Release-coupled.** The live prod `latest.json` is still size-less; PR-4 made the release workflow emit + assert `size` only for FUTURE cuts. Flipping to fail-closed-on-absent now would brick auto-update for every client until a size-carrying feed propagates — a high-blast-radius availability failure traded for a memory-DoS.
  2. **The clean alternative was already rejected.** A self-managed ranged/HEAD `Content-Length` probe was rejected in audit R3 (don't reshape the verified download path).
  3. **Owner decision.** The fail-closed flip is gated on a release-state fact the autonomous loop can't establish (is every served feed carrying size across all supported update paths?).
- **Action taken:** sharpened the in-code comment to codex's framing; filed a tracking issue; documented the flip condition.

## Local gate (post-fix, all green)
- `cargo fmt --check` (src-tauri) ✓
- `cargo clippy --all-targets -- -D warnings` (src-tauri + core) ✓ — 0 warnings
- `cargo test` ✓ — src-tauri 19+3, core 132, 0 fail
- `bun run test` ✓ — biome (1 pre-existing non-failing warning), SDK `tsc --noEmit`, sdk/playground/accelerator unit (120 tests), 0 fail

## Deferrals carried out of this plan (surface to owner)
- **SEC-02** — upstream Aztec `bb` publisher signing (circular online digest; nightlies block in-app pinning). Tracking issue filed.
- **SEC-09** — macOS Keychain negative-binding manual smoke (leaf chaining to a *different* trusted anchor → must reject), per R5. Owner-run; tracking issue filed.
- **M2** — updater mandatory-`size` fail-closed flip (above). Tracking issue filed.
