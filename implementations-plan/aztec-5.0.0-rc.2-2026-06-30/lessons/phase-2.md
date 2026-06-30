# Phase 2 — Land the bump PR (#370) (2026-06-30)

Merged as squash `1a279f5`. **Native-bb-rc.2 SDK E2E green** (asserts `transmit`, not `fallback` → the prove/msgpack/proof interface is rc.2-compatible through the unchanged 1.0.6 prove code). **Local `download_and_verify` gate green** (`ACCELERATOR_DOWNLOAD_TEST=1 AZTEC_BB_VERSION=5.0.0-rc.2 cargo test download_and_verify` from `packages/accelerator/core`: download rc.2 → SHA-256 → extract, the real deployed-binary path, this arch).

## Gotcha — the Opus-predicted Windows landmine fired
First CI run: everything green **except Windows Build + Prebuild Smoke** → `No pinned Windows bb.exe SHA-256 for @aztec/bb.js 5.0.0-rc.2` — `copy-bb.ts` `WINDOWS_BB_CHECKSUMS` had no rc.2 entry, so `resolveWindowsBbChecksum` throws. The audit flagged this exactly as a latent landmine. Fixed: pinned `5.0.0-rc.2` → `c0bf2429…` (sha256 of `barretenberg-amd64-windows.tar.gz` from the v5.0.0-rc.2 release; verified 5.5 MB gzip, reproducible). Re-push → Windows smokes pass → auto-merge. (Also closes the "next accelerator release breaks on Windows" gap.)

**Gate — PASS:** sdk.yml (native-bb-rc.2 e2e) + app.yml + accelerator.yml (incl. Windows) + actionlint all green; `download_and_verify` green; PR auto-merged.

LESSONS_FILE=implementations-plan/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-2.md
