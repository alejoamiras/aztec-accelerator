# Phase 1 — copy-bb.ts win32 fetch + checksum pin

## What shipped
`copy-bb.ts` now has a `win32` branch: it fetches `barretenberg-amd64-windows.tar.gz`
from the aztec-packages release, verifies a pinned SHA-256, extracts `bb.exe`, and writes
the Tauri sidecar `binaries/bb-x86_64-pc-windows-msvc.exe`. macOS/Linux paths unchanged.

## Decisions
- **Supply chain = in-repo version→SHA-256 map, fail-closed.** Upstream publishes *no*
  checksum file for the release (confirmed: only the two `.tar.gz` assets), so verifying
  against an upstream hash would be TOFU an attacker could forge. A committed, review-gated
  pin (`WINDOWS_BB_CHECKSUMS`) is stronger: an unknown version *or* a hash mismatch throws,
  forcing a human to add the new hash on every bb bump. SHA-256 captured from the P0 spike:
  `55043d74…0c8f`.
- **Tag derives from the LIVE `@aztec/bb.js` version (`v${version}`), not the committed
  `AZTEC_VERSION`** — that file is generated and had drifted to `4.2.0-aztecnr-rc.2` (a
  *different* build) while the installed package is `4.2.0`. The prebuild also rewrites
  `AZTEC_VERSION`, self-healing the drift.
- **Testability via `import.meta.main` guard.** All side-effects (bb.js resolution, fetch,
  fs) moved into `main()`, run only as the entrypoint; the pure helpers
  (`windowsBbReleaseTag`, `resolveWindowsBbChecksum`, `assertSha256`) are exported and
  imported side-effect-free by the test.
- **The accelerator package had NO TS unit-test gate.** Added `test:unit` → wired into the
  root `test:unit` (local `bun run test`) AND a step in the accelerator.yml **Lint** job
  (CI, gated by the `integration` filter which covers `packages/accelerator/**`).

## Validation
- 5 `bun:test` units: tag derivation, pinned-checksum well-formedness, unknown-version
  throw, tamper (mismatch) throw, match passes.
- **End-to-end on real data, locally** (download is HTTPS + `tar` exists on macOS, so only
  "bb.exe executes" is Windows-specific — and P0 proved that): fetch → 4,849,818 B → SHA-256
  verified against the pin → extract → `bb.exe` = 21,731,840 B (matches the spike exactly).
- `bun run lint` + `actionlint` clean.

## Carry-forward to P2
- The prebuild now needs **network on the Windows build runner** (mac/linux still read the
  npm package, unchanged). The `win32` fetch only runs when `process.platform === "win32"`.
- Sidecar filename carries `.exe` (`bb-x86_64-pc-windows-msvc.exe`) — Tauri sidecar
  resolution on Windows expects the extension.
- `tauri.conf.json` `bundle.targets` is `"all"`; P2 narrows/configures the Windows (NSIS)
  bundle + adds the `win32` arms for crash-recovery (compile blocker), certs, paths.
