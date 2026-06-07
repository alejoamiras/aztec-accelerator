# Core Extraction — Shared Context Brief (verified facts)

**Task:** Extract a GUI-agnostic `accelerator-core` library crate from the Tauri desktop crate so the
headless `accelerator-server` (used by external projects to accelerate Noir/Aztec proving in **CI**)
stops dragging the entire Tauri dependency tree. Secondary: restructure the release CI to exploit the
split and **measure** the build-time / dependency-count delta.

This brief is the **single source of verified facts** for all three independent planners (main, codex,
opus). Draft against these; do not re-derive. Label anything you add beyond this as Fact / Inference / Ask.

---

## Current crate layout (verified)

- `packages/accelerator/src-tauri/` — Cargo package **`aztec-accelerator`** (the GUI desktop crate).
  - `autobins = false` (Cargo.toml:16) + single `[[bin]]` = `src/main.rs`. **Why:** the 1.0.1 macOS
    auto-update break was caused by a *second binary* (`accelerator-server`) auto-discovered under
    `src/bin/` and bundled into the `.app`'s `MacOS/` folder → changed the bundle signature shape →
    broke `amfid` revalidation on in-place update from 1.0.0. Fixed by moving the server to a sibling
    crate + disabling autobins.
  - `src/lib.rs` already declares the modules `pub mod` (authorization, bb, certs, commands, config,
    crash_recovery, server, updater, verified_sites, versions) — so the GUI crate already exposes a
    library surface that the headless crate consumes.
- `packages/accelerator/server/` — Cargo package **`accelerator-server`** (headless).
  - Depends on `aztec-accelerator = { path = "../src-tauri" }` → **reuses** server/auth/config/etc. with
    **zero code duplication**, but **compiles the full Tauri transitive tree** (tray-icon, image-png,
    webview, tauri-plugin-updater, rcgen, hyper, …). Its own Cargo.toml admits: *"Tauri transitive deps
    come along for the ride (compile-time cost, not runtime)."*
  - Distributed as a **raw `tar.gz` of the binary** (NOT a Tauri `.app`, NOT notarized like the GUI).

## Module map (verified — `tauri::`/AppHandle/tauri_plugin/WebviewWindow grep)

**Tauri-free (core candidates), ~4,716 LOC:**
| module | LOC | notes |
|---|---|---|
| `server.rs` + `server/{auth,bind,probe,prove,tls}.rs` | 1087 + 612 | Axum HTTP server; `start` (HTTP), `start_https` (HTTPS, receives RustlsConfig). `https_bound: Arc<AtomicBool>` shared-flag (Q7). |
| `versions.rs` | 1209 | bb version resolution + download (the OOM-guard etc.). |
| `certs.rs` | 552 | **only** user of `rcgen` (cert generation) + `load_rustls_config`. |
| `config.rs` | 394 | `AcceleratorConfig`, `mutate_config`, persistence. |
| `authorization.rs` | 372 | `AuthorizationManager`, origin gating. |
| `bb.rs` | 279 | bb binary cache/exec. |
| `verified_sites.rs` | 211 | `VerifiedSitesRegistry`; used by `commands.rs` (GUI popup). Light deps (serde). |

**GUI-coupled (stay in `src-tauri`):** `commands.rs` (36 tauri refs), `main.rs` (22), `tray.rs` (16),
`windows.rs` (8), `updater.rs` (7 — tauri-plugin-updater), `crash_recovery.rs` (process lifecycle;
earlier count said 3 tauri refs but a narrow `use tauri|tauri::|AppHandle` grep found 0 — verify before
placing; it's GUI-process autostart/relaunch logic headless likely doesn't need).

## What the headless server actually uses (verified — `server/src/main.rs`)

```rust
use aztec_accelerator::authorization::AuthorizationManager;
use aztec_accelerator::config::AcceleratorConfig;
use aztec_accelerator::server::{start, AppState, HeadlessState};
```
- HTTP only (`start()`), never `start_https`. Origin gating via `ALLOWED_ORIGINS` env.
- Transitively needs `bb` + `versions` (proving). Does **not** import `certs` or `verified_sites`.

## Dependency-isolation story (verified — THE key design input)

- **`rcgen`** (cert generation) → `certs.rs` ONLY.
- **rustls / axum-server / `RustlsConfig`** (TLS *serving*) → `server/tls.rs` (+ certs/commands/main).
  `start_https(state, tls_config)` **receives** the `RustlsConfig`; the GUI builds it via
  `certs::load_rustls_config()` and passes it in. So `server/tls.rs` needs the rustls *serving* stack but
  NOT rcgen.
- ⇒ A single **`tls` cargo feature** can gate BOTH `server/tls.rs` (serving) AND `certs.rs` (generation).
  An HTTP-only headless that doesn't enable `tls` compiles **neither rustls-serving nor rcgen**.
- `verified_sites` is standalone + light; its placement is an SRP question, not a dependency-weight one.

## Release pipeline (verified — `.github/workflows/release-accelerator.yml`)

- `build` (3 Tauri platforms) + `build-headless` (4 platforms: macos arm64/x86_64, linux x86_64/arm64).
- `build-headless` (job ~L237): `cargo build` the `server` crate, assert `accelerator-server --version`
  == release version, `tar -czf` + `.sha256`, upload. Rust-cache-key `accelerator-server-${target}`.
- Stable cut also does latest.json + S3 + bump-source; rc/prerelease skips those.

## User decisions (locked this session)

- **Q1 / validation:** dedicated **rc dry-run** (updater-smokes prove the GUI still auto-updates;
  headless tarballs still produced + version-correct). Justified by the **CI restructure**, NOT by any
  GUI bundle-signature change — a *library* extraction adds no `[[bin]]`, so the signed `.app` bundle
  topology is **unchanged**. Decoupled from the pending 1.0.5 stable cut (they need not be linked).
- **Q2 / core boundary:** "all Tauri-free → core" in spirit, but **headless must stay lean** (no
  rcgen/rustls compiled for HTTP-only). Open fork for planners: **(a)** one `accelerator-core` crate with
  `tls`/`verified-sites` behind cargo *features* (GUI enables, headless doesn't), vs **(b)** core =
  headless-needs-only, `certs`/`verified_sites` stay in the GUI crate. Resolve with reasoning.
- **Q3 / CI:** **restructure the release CI** to exploit the split (separate core-crate caching,
  parallelize/speed the headless legs) — IN SCOPE, its own phase. Also **measure + report** real
  before/after headless build time + `cargo tree` dependency-count delta.
- **Q4 / hardening:** **No `/harden`.** Post-impl `/code-review max` + codex audit + rc dry-run suffice.

## Success criteria (measurable)

1. Behavior-preserving: existing ~90 Rust unit tests + WebDriver/e2e green; no runtime behavior change.
2. `cargo tree -p accelerator-server` shows **no `tauri`** (and ideally no `rcgen`/rustls-serving).
3. GUI app unchanged functionally; rc dry-run **green** (all updater-smokes incl. Windows/Linux/macOS).
4. Release CI restructured; measured headless build-time + dep-count delta reported in the plan/lessons.
5. Small, behavior-preserving PRs that each merge green (main is branch-protected: PR + auto-merge).

## Environment constraints

- `main` branch-protected → branch + PR + auto-merge (never direct push).
- SSH transport flaky this session → push via `git -c credential.helper='!gh auth git-credential' push https://github.com/alejoamiras/aztec-accelerator.git <branch>:<branch>`.
- Commit-signing flakes → `git -c commit.gpgsign=false`. Commitlint enforces conventional + body line ≤100.
- `bun run test` (lint+typecheck+TS unit), `bun run lint` (biome+pkg+**cargo fmt**), `cargo test`
  (in src-tauri), `bun run lint:actions` (actionlint) before pushing workflow changes.

## Asks for each planner

Draft an **independent implementation plan** for this extraction, phased into small behavior-preserving
PRs. Cover: the crate-topology change (new `accelerator-core` + rewiring src-tauri & server), the Q2
feature-vs-split fork (pick one, justify), the CI restructure + measurement, the rc-dry-run validation,
and a behavior-preservation strategy (how do we *prove* nothing changed). Include the required
**Security & Adversarial Considerations** and **Assumptions (Facts/Inferences/Asks)** sections.
Be adversarial about your own plan: what breaks the macOS bundle/signing, what breaks the headless
version-stamp, what cyclic-dependency or feature-unification footgun could bite, what an attacker targets
in the CI-distributed headless tarball, where Cargo feature unification (workspace-wide) could silently
re-introduce Tauri/rcgen into headless.
