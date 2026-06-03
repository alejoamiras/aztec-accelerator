# Opus audit (round 1) — verdict: SHIP-WITH-CHANGES (subagent_type: Plan, model: opus)

Directional calls sound (sequence #96→P5, bound-the-hang, negative-leg fail-closed), but two
factually-wrong premises would produce no-op/misleading work.

## Critical
1. **#95 premise false — `src-tauri/Cargo.lock` is ALREADY synced** (both read `1.0.4-rc.1`).
   Step 1 would be an empty diff. The real (forward-looking) problem: `bump-source` edits the
   `Cargo.toml`s + `tauri.conf.json` but never the lock → the *next* release drifts it. Reframe as
   preventative. *(Codex sharpened this: the actually-stale lock is `server/Cargo.lock`.)*
2. **#96 Level-1 arming is a silent no-op** — `Config` (config.rs:45-54) has no `autostart` field
   (serde drops unknown keys); arming gates on the tauri-plugin-autostart **Registry Run key**
   (`HKCU\…\Run`) via `is_enabled()` (main.rs:260-265), set by `set_autostart` (commands.rs:31-41).
   The harness must arm via the Run key / `set_autostart(true)`, else it tests a never-registered task.

## High
3. **P5 blocking-flip CAN wedge the pipeline; timeout-tighten ≠ sufficient.** Any deterministic red
   (flake/image-change/Defender hiccup) on a now-required leg hard-blocks tag+release. Bake in the
   **documented revert** (drop-from-needs + restore-advisory) in the same PR, like the linux flip
   (_e2e-updater-linux.yml:115-120). And ~35-40min vs ~25-30min healthy on variable windows-latest
   is barely 1.2-1.3x headroom → spurious timeouts; keep more headroom.

## Medium
4. Level-2 SYSTEM-principal task name must be **provably disjoint** from the shipped constant
   `"Aztec Accelerator Crash Recovery"` (crash_recovery.rs:200); spike must NOT also arm autostart
   (two tasks racing). Enforce, don't assert.
5. **Structural-convergence (real N-1) BREAKS the first stable-release bootstrap** — the N-1 resolver
   is `gh release list --exclude-pre-releases` (_e2e-updater.yml:68), `1.0.4-rc.1` is a prerelease,
   no Windows stable exists → resolver errors → first stable Windows release wedges. **Defer to a
   fast-follow.** Ship P5 as flip+timeout; parity is satisfied by behavior, not build-topology.

## Low
6. `bump-source` has NO Rust toolchain (only setup-bun) → use a `sed` on the lock version line
   (matches how the job already seds the Cargo.tomls); `cargo generate-lockfile` would churn deps.
7. plan cite ":281" is the accelerator-server build, not the Tauri build (conclusion stands).

## Looks fine
- #96-before-P5 is genuinely correct (linux/mac recovery keys on exit code; no install-race there).
- Negative-leg fail-closed; ephemeral-key isolation (separate jobs); Defender hygiene already correct.
