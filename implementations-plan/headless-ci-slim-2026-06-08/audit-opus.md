# Opus subagent audit — headless CI slim (Phase 3b)

**VERDICT: A holds-with-changes.** Approach A (boolean inputs) is correct; B forks all the shared setup
(host-assert, Bun, bun-cache, `--frozen-lockfile`, rust-toolchain, rust-cache) to guard just 2 lines — drift bait.

Findings (verified against files):
1. **A > B.** The only desktop-specific surface is one `apt-get` line (`action.yml:69`) + one prebuild step
   (`action.yml:96`). Everything else is genuinely shared (carries the `--frozen-lockfile` gate + cross-warming
   `shared-key`). A's 2 default-true flags aren't "sprawl." A correct.
2. **Smoke `cat AZTEC_VERSION` must be DELETED, not left alongside.** Once `run-prebuild:false`, that file is gone →
   `|| echo unknown` fires → the Phase-3 `.aztec_version != "unknown"` assert FAILS. Hard ordering constraint.
3. **e2e export path is `packages/accelerator/src-tauri/AZTEC_VERSION`** (NOT a bare `AZTEC_VERSION`). [Codex refined:
   must be in the SAME step as launch or via `$GITHUB_ENV` — a plain `export` in an earlier step won't persist.]
4. **Phase 1 test claim FALSE:** `copy-bb.test.ts` only covers the Windows tag/checksum/SHA surface, NOT
   `resolveAztecBbVersion()` (the `Bun.resolveSync → @aztec/bb.js/package.json` chain). It's a REAL new test.
   `main()` must call the extracted resolver (anti-drift).
5. **e2e hook is a `/prove` fast-path fix** (prove.rs:59-64: bundled unset → "unknown" → `v != bundled` → forced
   download), NOT a `/health` fix — e2e never asserts `aztec_version`. Frame it so a reviewer doesn't drop it.
6. **Drift surface:** the desktop apt list now lives in 3 places (composite true-branch, false-branch, `_e2e.yml:49`).
   `_e2e.yml`'s WebKit/GTK is ALSO waste (it only builds the headless server) — a follow-up, out of this scope.
Security holds: net-narrower surface, `--frozen-lockfile` stays in-composite, no new secrets.
