# Phase 1 — Inspection &amp; sanity check

## `bun install` postinstall scan

Surveyed every `package.json` for `preinstall`, `postinstall`, `prepare`, `preprepare`, `postprepare` scripts.

| File | Hook | Risk |
|------|------|------|
| `./package.json` | `prepare: husky` | None — git hooks setup, no Rust. |
| `./packages/accelerator/package.json` | none | — |
| `./packages/landing/package.json` | none | — |
| `./packages/playground/package.json` | none | — |
| `./packages/sdk/package.json` | none | — |

Cross-check: only `cargo`/`rustc`/`rustup` mentions in `package.json` files are in the root's `lint:fix` and `lint:rust` scripts (manually invoked, not install hooks).

**Conclusion**: ordering Rust toolchain installation AFTER `bun install` in the composite is safe — no install hook tries to invoke `cargo`.

## Composite consumer inventory

`grep -rln "setup-accelerator" .github/` → only `.github/workflows/accelerator.yml`.

`accelerator.yml` callers (verified by re-reading the file):
- `clippy` (line 11, ubuntu-latest)
- `test` (line 42, ubuntu-latest)
- `lint` (line 57, ubuntu-latest)
- `smoke` (line 71, ubuntu-latest)
- `release-smoke` (line 139, **ubuntu-latest** — not macOS as v1 plan claimed; codex flagged this)
- `e2e` (line 162, ubuntu-latest)

All 6 current consumers run on Linux. The composite's unconditional `sudo apt-get` has never run on macOS to date. Adding `if: runner.os == 'Linux'` guard is forward-compat for the soon-to-be-added `build` job (macOS matrix entries).

## Confirmed claims for downstream phases

| Claim | Status |
|-------|--------|
| `Swatinem/rust-cache@v2` `key:` is appended to auto key | ✓ (codex confirmed against README) |
| `copy-bb.ts` (prebuild) does NOT read Cargo.toml or tauri.conf.json | ✓ (re-read at packages/accelerator/scripts/copy-bb.ts) |
| `release-accelerator.yml` job graph: validate → e2e-webdriver → [build, build-headless] → smoke → tag → release → bump-source | ✓ |
| `Cargo.toml` has `panic = "abort"` in release profile | ✓ (Cargo.toml:57) — informs verified-sites plan, not this one |
| `_e2e-webdriver.yml` has duplicate setup (rust-toolchain, rust-cache, setup-bun, bun install, apt, prebuild) | ✓ (lines 28-64) |
| `_e2e-webdriver.yml` extra apt deps: xvfb, stalonetray, dbus-x11 | ✓ (line 49) |

## Pre-existing inconsistencies caught (not in plan scope but worth noting)

- `release-accelerator.yml` `build` job's apt gate uses `matrix.platform == 'linux-x86_64'` while `build-headless` uses `runner.os == 'Linux'`. Both currently equivalent (build has no linux-arm64 matrix entry). After dedup, both use the composite's `runner.os == 'Linux'`.
- `accelerator.yml` `release-smoke` runs on `ubuntu-latest` — v1 plan inaccurately said `macos-latest`. v2.1 corrected.

Ready for Phase 2.
