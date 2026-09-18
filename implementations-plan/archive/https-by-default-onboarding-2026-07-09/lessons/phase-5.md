# Phase 5 — onboarding wizard (UI + IPC + E2E)

Done ahead of Phase 4 because it's locally-validatable (cross-platform Rust IPC + biome), whereas Phase 4 (Windows trust) can't be compiled on this Linux box. P5's wizard works with P3's macOS/Linux trust; the HTTPS step just errors on Windows until P4 (row hidden there).

## What shipped
- **commands.rs**: `enable_https_inner` + `set_autostart_inner` extracted (reused by the wizard). New IPC:
  - `get_onboarding_state` → `{platform, https_default: true (A9/§2.1 — pre-checked for ALL incl. upgraders), autostart_enabled, auto_update, trust_status}`.
  - `complete_onboarding{https,autostart,autoUpdate}` → runs each action INDEPENDENTLY (a failure doesn't abort others); returns per-action `Result<(),String>` (serde `{Ok:null}`/`{Err}`); sets the once-per-version marker ONLY if all succeed (R4 marker discipline).
  - `dismiss_onboarding` → the ONLY unconditional marker set ("Continue without HTTPS" after a failure + "Skip for now").
- **main.rs**: `open_onboarding` command (Settings "Run setup again"); first-launch auto-show when `onboarding_version < ONBOARDING_VERSION` — **gated `#[cfg(not(feature="webdriver"))]`** so it doesn't disturb the existing webdriver settings-bootstrap specs (see deferred note). Registered all 4 commands.
- **windows.rs**: `show_onboarding_window` (open-or-focus, 520×600).
- **onboarding.html**: Variant A single-card. Prefill via `get_onboarding_state`; per-OS HTTPS cert copy (macOS password / Windows+Linux install-on-Start, no separate prompt — R8/A8); Start→complete_onboarding with per-row ✓/✗; partial HTTPS failure → toggle off + Retry + "Continue without HTTPS"; success/dismiss → `getCurrentWindow().close()`.
- **settings.html**: "Run setup again" button → `open_onboarding`.
- **e2e/tauri-mock.js**: new command mocks + a `window.getCurrentWindow().close()` mock (records `__window.close`).
- **e2e/onboarding.spec.ts**: 8 Playwright specs — pre-checked defaults, per-OS copy, upgrader prefill, Start payload + close-on-success, partial-failure (HTTPS off + Retry + relabel + no-close), Retry re-checks, Skip dismisses+closes.

## Local validation
- ✅ `cargo build` + `clippy -D warnings` clean; `cargo test` 24 lib + 7 main pass; fmt clean.
- ✅ biome clean on onboarding.html / onboarding.spec.ts / tauri-mock.js / settings.html.
- ⏳ Playwright specs: CI-only (Ubuntu 26.04 has no Playwright browser build — same limitation as Phases 1/3).

## Deferred (documented, CI-iteration or later phase)
- **Window-scoped capability (S4/final-pass)**: NOT added. Tauri v2 makes app commands callable from all first-party windows by default; scoping them per-window is fiddly and build-schema-validated (uncheckable locally). Per the plan's S4 fallback, **explicitly accepting the bundled-content-XSS risk** — all frontend HTML is first-party bundled, no remote content is ever loaded. Revisit if a remote-content window is ever added.
- **WebDriver onboarding.spec.ts**: NOT added. The auto-show is gated off for webdriver builds to protect the existing 9 specs; a real WebDriver wizard test needs the harness `ensureWindowByTitle` generalization + an explicit wizard-open path, which can only be iterated against CI (blocked on the workflow-scope push). Playwright mock coverage is thorough in the meantime.
- **Renewal consent window**: that's Phase 4 (macOS/Windows), not P5.
