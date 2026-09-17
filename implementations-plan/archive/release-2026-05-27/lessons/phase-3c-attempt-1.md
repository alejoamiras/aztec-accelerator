# Phase 3c attempt 1 — `1.0.1-rc.2` dry-run

## Result

Two things happened, one good and one expected-then-bad:

### Good — tag-after-build ordering worked perfectly

Run 26533457162 had **all 4 `build-headless` jobs fail**, but the new `validate → e2e-webdriver → [build, build-headless] → smoke → tag → release → bump-source` job graph from PR #226 (A.3) correctly held back the tag-and-release path:

| Job | Result |
|---|---|
| Validate Version | success |
| Pre-release E2E Gate / WebDriver E2E (release) | success |
| Build (aarch64-apple-darwin) | success |
| Build (x86_64-apple-darwin) | success |
| Build (x86_64-unknown-linux-gnu) | success |
| Build Headless (macos-arm64) | **failure** |
| Build Headless (macos-x86_64) | **failure** |
| Build Headless (linux-x86_64) | **failure** |
| Build Headless (linux-arm64) | **failure** |
| Post-build Smoke | success |
| Create Git Tag | **skipped** ✅ |
| Create GitHub Release | **skipped** ✅ |
| Bump source version | **skipped** ✅ |

No `accelerator-v1.0.1-rc.2` tag pushed to origin. The pre-existing bug from earlier release pipelines (where a failed build would still tag) is dead.

### Bad — `build-headless` was under-provisioned

I built `build-headless` in PR #225 with a smaller setup than the Tauri `build` job. Two failures across all 4 platforms:

1. **`tauri-build` requires `binaries/bb-<target>`.** The build script runs for the whole crate (not just the selected `[[bin]]`), so it asserts the sidecar exists. Without `bun run --cwd packages/accelerator prebuild`, every platform fails with `resource path 'binaries/bb-<target>' doesn't exist`.

2. **Linux only had `libssl-dev`.** The headless build pulls Tauri's transitive `glib-sys`, `gobject-sys`, `gtk-sys`, `webkit-gtk` via crate-level deps. Linux failed with `gobject-2.0.pc` not found.

### Why PR-gate didn't catch it

`accelerator.yml`'s `release-smoke` job uses the `setup-accelerator` composite, which installs the full apt deps AND runs `prebuild`. That validated the host build correctly — but didn't exercise the standalone configuration of `release-accelerator.yml`'s `build-headless` job, which had a different (under-provisioned) setup.

## Fix

PR #228: add `oven-sh/setup-bun@v2`, `bun install --frozen-lockfile`, the full Tauri apt deps, and `bun run --cwd packages/accelerator prebuild` to `build-headless`. Two-job-step change.

## What I should have done in PR #225

The `build-headless` job should have reused the Tauri `build` job's setup steps verbatim from the start (just swapping `cargo build --release --bin accelerator-server` for `bunx tauri build`). I optimized for a "minimal setup" without realizing `tauri-build`'s build script runs unconditionally.

## What's next

After PR #228 merges, re-trigger with `-rc.3` (since `-rc.2` is now a wasted slot — even though no GH release was created, treating it as a tested slot).
