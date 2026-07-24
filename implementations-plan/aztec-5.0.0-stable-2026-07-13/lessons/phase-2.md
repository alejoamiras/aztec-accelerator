# Phase 2 lessons — publish hardening + landing the PR (2026-07-13)

## Gate result: ✅ all green

- `promote-latest.yml` created (tag-only; allowlist `X.Y.Z` OR `X.Y.Z-revision.N`; published-version check; `env:` indirection; shared non-cancelling `publish-npm` concurrency group). `_publish-sdk.yml` dispatch hardened (no `latest` input on dispatch; `dist_tag` → `type: choice [testnet, nightlies]`; reject-`latest` guard on BOTH trigger paths; all `${{ }}`-into-`run:` interpolations env-indirected). `publish-testnet.yml`: `cancel-in-progress: false`, `build_accelerator: true` on its e2e (publish-time native gate — rc.1's dropped recommendation restored), partial-publish redispatch rule documented in-file. `bun run lint:actions` green.
- **PR #376 merged**: all CI green including `sdk.yml`'s native-bb e2e (asserts `transmit`, not `fallback`) against a genuine 5.0.0 sandbox (setup-aztec auto-detects from the sdk pin).
- **Runtime-download gate PASSED locally**: `ACCELERATOR_DOWNLOAD_TEST=1 AZTEC_BB_VERSION=5.0.0 cargo test download_and_verify_bb` from `packages/accelerator/core` — download → SHA-256 digest verify → extract, current arch.
- Follow-up **PR #378** (found by P3b, rode P2's machinery): the /status-405 node-check fix + production sqlite3-wasm unhashed-asset fix + mocked-spec/smoke-filter contract updates. Squash-then-continue made the follow-up PR DIRTY → rebased `--onto origin/main`, force-pushed own branch. SSH agent flaked once mid-push → pushed via HTTPS direct URL.

LESSONS_FILE=implementations-plan/aztec-5.0.0-stable-2026-07-13/lessons/phase-2.md
