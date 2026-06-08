# Phase 3 — CI: guard the win + bb-version hook (safe parts done; setup-slimming deferred → owner)

Base: `feat/core-extraction-phase-3` off `38e5720` (#326).

## Done (safe, additive, validated)
1. **cargo-tree regression tripwire** (Smoke job): asserts the headless tree has NO
   tauri/tao/wry/rcgen/tokio-rustls/x509-parser/rustls-pemfile. Protects the −56% reduction against a future
   regression (e.g. an accidental `[workspace]` re-unifying features) — fails the PR gate loudly if a GUI/TLS
   crate leaks back into the headless binary.
2. **bb-version hook** (Smoke job): `AZTEC_BB_VERSION="$(cat AZTEC_VERSION …)"; export …` before launching the
   server — surfaces the prebuild's resolved `@aztec/bb.js` version to `/health` via the runtime env (core is
   `build.rs`-free; the server reads `AZTEC_BB_VERSION` at construct-time). Closes the `"unknown"` gap from Phase 1/2.
3. **/health assertion extended**: `.aztec_version != "unknown"` — validates the hook end-to-end.
4. actionlint clean (fixed SC2155: split declare/assign for the export).

## Measurement (final)
- Headless dep tree: **446 → 194 packages (−56%)**; zero tauri/rcgen/tokio-rustls (Phase 2). The tripwire now
  guards this in CI on every PR.

## DEFERRED → owner review (Phase 3b — release-pipeline change; "CI speed is not the most important thing")
The plan's headless-setup slimming (drop `libwebkit2gtk`/GTK + the `copy-bb.ts` prebuild from the headless CI
legs via `install-tauri-system-deps`/`run-prebuild` composite inputs) is a **release-critical** change: the
`setup-accelerator` composite is shared by desktop + headless + e2e + release-smoke, and `copy-bb.ts` ALSO
**copies the bb binary the e2e needs** (not just the version). Slimming it safely needs per-job input flags +
an alternative bb source for the e2e, AND it touches the release pipeline the owner just hardened. Given AFK +
the owner's stated priority, I did NOT do this unsupervised — recommended as an owner-reviewed follow-up (a
`setup-accelerator-headless` variant, or boolean inputs defaulting to current behavior). Its build-time win is
incremental on top of the already-realized −56% dependency-surface drop, which is the headline result.

## Attempts
- GREEN. Safe parts only; SC2155 fixed on the first actionlint pass.
