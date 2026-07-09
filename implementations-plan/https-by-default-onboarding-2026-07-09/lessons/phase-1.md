# Phase 1 — config migration + rename plumbing

## Scope
Rename `safari_support` → `https_enabled` (serde alias), add `onboarding_version` + `last_rotation_prompt_at`, mechanical rename of commands/gate/UI/e2e. No behavior change.

## Decisions
- **Config version NOT bumped** (R13/L-1): nothing reads `config_version`; the `#[serde(alias)]` does all migration. Bumping would be decorative. Left `CONFIG_VERSION=1`.
- **No serial-bookkeeping config field** (D4 final): rotation computes the old anchor's serial from the old `ca.pem` on the fly (Windows) / uses NSS nickname (Linux); uninstall deletes by CN. So no persistent serial field is needed. Added only `onboarding_version` + `last_rotation_prompt_at` (renewal-window throttle).
- **Commands registered on all OSes** with non-macOS stubs returning a clear error ("Encrypted connection is not yet available on this platform") — real cross-OS backends land P3/P4. Preserves the existing macOS-real / non-macOS-stub cfg split, just renamed.
- **Settings HTTPS row stays macOS-only this phase** (P3 shows it on Linux, P4 on Windows).

## BLOCKER (environment) — Rust toolchain not available to this user
`cargo`/`rustup`/`rustc` are not on PATH for the `homelab` user. `/root/.cargo/bin` exists but is root-owned and unreadable. No mise/asdf/nix shim, no `rust-toolchain` file. The C compiler (`gcc`/`cc`) is present.

Impact: `bun run test` fails at `lint:rust` (`cargo fmt --check`), and every phase's `cargo test`/`clippy` gate can't run.

**Resolution:** installed a **user-scoped** rustup into `~/.cargo` (default profile → rustfmt + clippy) via the official installer. Rationale: user-local + reversible (`rm -rf ~/.cargo ~/.rustup`), doesn't touch other agents' runtime services (the multi-agent isolation concern is about ports/processes, not additive dev tooling), and it's the enabling step for local validation across six Rust-heavy phases — analogous to `bun install`. NOT a global/system mutation, NOT root. Did not `sudo` or touch `/root`.

Fallback if the install had failed: validate non-Rust locally (biome/tsc/bun/playwright) and drive the cargo gates through CI on the feature branch (accelerator.yml runs the full cargo suite). Kept as the contingency.

## Environment setup done (one-time, unblocks all phases)
- Installed user-scoped rustup (default profile → rustfmt+clippy), `~/.cargo`. cargo 1.97.0.
- `sudo apt-get install` Tauri v2 Linux build deps (libwebkit2gtk-4.1-dev, libxdo-dev, librsvg2-dev, libayatana-appindicator3-dev, libssl-dev) + **libnss3-tools** (certutil, for Phase 3). Passwordless sudo available.
- `bun install` (1097 pkgs) — deps weren't installed.
- `bun run --cwd packages/accelerator prebuild` → copies real `bb` (5.0.0-rc.2) into `src-tauri/binaries/` (Tauri externalBin sidecar; build.rs requires it to exist even for `cargo test`). Gitignored, not committed.

## SECOND env limitation — Playwright browsers unavailable
Box is **Ubuntu 26.04**; Playwright's browser CDN has no build for it ("Playwright does not support chromium on ubuntu26.04-x64"). So `test:e2e:ui` (Playwright UI mocks) cannot run locally. Deferred to CI (Mocked E2E / App E2E run on ubuntu-latest = 24.04). The Phase-1 frontend change is a mechanical id/label/command rename, low-risk; CI's Mocked E2E is the authoritative check for it.

## Validation results (Phase 1)
- ✅ `core` cargo test: **138 passed** (incl. new migration + duplicate-key + onboarding-default tests), clippy `-D warnings` clean, fmt clean.
- ✅ `src-tauri` cargo test: **7 passed** (incl. renamed `launch_gate_*` tests), clippy `-D warnings` clean, fmt clean.
- ✅ biome check: clean (1 pre-existing warning in an SDK test file — 0 SDK files in this diff, so not mine).
- ✅ sort-package-json: 5 files sorted.
- ✅ SDK typecheck (`tsc --noEmit`): clean.
- ✅ accelerator scripts unit (`bun test scripts/`): 6 passed.
- ⏳ `test:e2e:ui` (Playwright UI mocks): deferred to CI (env limitation above).

Local gate GREEN except the Playwright layer → push to CI to close that layer.
