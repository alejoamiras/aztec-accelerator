# Phase 1 — version-only bb resolver

Base: `feat/headless-ci-slim` off `002486e`.

## Done
- Extracted `export function resolveAztecBb(): { version, bbJsRoot }` from `copy-bb.ts` (lifts the 144–150
  resolution). `main()` now calls it (DRY — no re-inline, the anti-drift requirement both auditors named).
  NOTE: returns BOTH `version` + `bbJsRoot` because `main()` needs `bbJsRoot` for the Linux/macOS bb copy —
  cleaner than the plan's tentative `resolveAztecBbVersion()` name, and still single-source.
- New `scripts/bb-version.ts` (imports `resolveAztecBb`, prints `.version`) + a `prebuild:version` package script.
- New test in `copy-bb.test.ts`: `resolveAztecBb()` returns a real semver version (not `"unknown"`) + an existing
  `bbJsRoot` (closes the coverage gap both auditors flagged — the file only tested Windows checksum helpers before).

## Validation
- `bun test scripts/` → **6 pass** (was 5). `bun run prebuild:version` → prints **`4.2.0`** (the live
  `@aztec/bb.js` version). GREEN.
