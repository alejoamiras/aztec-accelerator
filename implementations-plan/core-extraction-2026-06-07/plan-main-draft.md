# Core Extraction — MAIN agent independent draft

> One of three independent Phase-1 plans (main / codex / opus). Consolidation merges these into `plan.md`.

## Summary
Extract a Tauri-free **`accelerator-core`** library crate holding the GUI-agnostic proving stack, so the
headless **`accelerator-server`** depends on `core` instead of the GUI crate and stops compiling the
entire Tauri tree. The GUI keeps only its Tauri layer. Secondary: restructure the release CI to exploit
the split and measure the build-time / dependency-count delta. Behavior-preserving; validated by a
dedicated rc dry-run.

## Crate topology
**Before:** `aztec-accelerator` (src-tauri, GUI lib+bin) ← `accelerator-server` (server, `path=../src-tauri`).
**After:**
```
accelerator-core   (packages/accelerator/core)   NEW lib, Tauri-free
   ├── default features: server(HTTP) + bb + versions + config + authorization
   ├── feature "tls":            server/tls.rs (start_https) + certs.rs   → optional deps rcgen, rustls/axum-server tls
   └── feature "verified-sites": verified_sites.rs
aztec-accelerator  (src-tauri, GUI lib+bin)  →  core { features = ["tls","verified-sites"] }
   └── keeps: commands, tray, updater, windows, crash_recovery, main
accelerator-server (server, bin)             →  core { default-features = false }
```

## Q2 verdict — **Option (a): one core crate, cargo features.** 
Pick (a) over (b: leave certs/verified_sites in the GUI crate). Reasons:
1. **Single source of truth** for ALL server logic. `certs.rs`/`verified_sites.rs` are pure, unit-tested
   logic — they belong in the testable lib, not stranded in the Tauri bin crate. (b) keeps the GUI crate
   doubling as "a lib headless reaches into," which is the very smell we're removing.
2. **Future-proof:** if headless ever wants HTTPS, flip `--features tls` — no second migration.
3. **The (a) footgun (feature unification) is neutralized here** (see below), so its main cost is gone.
4. Cost of (a) = optional-dep + `#[cfg(feature=...)]` plumbing on `tls.rs`/`certs.rs`. Modest, idiomatic.

### Why the unification footgun does NOT bite (verified)
No `[workspace]` exists (no root Cargo.toml; no `[workspace]` table); `src-tauri/` and `server/` each have
their **own `Cargo.lock`** → standalone build units. The headless build resolves `core` with
`default-features=false` in its **own** graph; the GUI's `features=["tls"]` is a *different* build graph.
Cross-crate feature unification only happens **within one workspace/build**, which doesn't exist here.
**Regression tripwire (added in Phase 4):** CI asserts `cargo tree -p accelerator-server` contains no
`tauri`, `rcgen`, or `rustls` — so if someone later introduces a `[workspace]`, the leak fails loudly.

## Phases (small behavior-preserving PRs)

**Phase 0 — Recon + baseline (no code change).** Confirm no-workspace (done), resolver/edition (2021).
Record BASELINE: `cargo tree -p accelerator-server` (expect `tauri` present) + crate count; cold-build
time of the headless crate (`cargo build --release` from clean target). → `lessons/phase-0.md`.

**Phase 1 — Create `accelerator-core` (move, no behavior change).** New `core/` crate. Move the Tauri-free
modules (`server/` dir, `bb`, `versions`, `config`, `authorization`, `certs`, `verified_sites`) + their
unit tests from `src-tauri/src/` into `core/src/`. `core/src/lib.rs` `pub mod` + re-exports. Add features:
`default` (no tls/verified-sites deps), `tls = ["dep:rcgen", "dep:rustls", "axum-server?/tls-rustls", ...]`
gating `certs` + `server::tls`; `verified-sites = []` gating `verified_sites`. Optional deps marked
`optional = true`. `cargo test -p accelerator-core --all-features` green (the moved unit tests run here now).

**Phase 2 — Rewire GUI (`src-tauri`) onto core (no behavior change).** Add
`accelerator-core = { path="../core", features=["tls","verified-sites"] }`. Delete the moved modules from
`src-tauri/src/`; update `src/lib.rs` + `use crate::{server,bb,…}` → `use accelerator_core::{…}` across
`commands.rs`/`main.rs`/`tray.rs`/`updater.rs`/`crash_recovery.rs`. GUI keeps its Tauri modules. `cargo
build` + `cargo test` (in src-tauri) green; `bun run lint` (cargo fmt) clean.

**Phase 3 — Repoint headless onto core (the payoff).** `server/Cargo.toml`: replace
`aztec-accelerator = { path="../src-tauri" }` with `accelerator-core = { path="../core", default-features=false }`.
`server/src/main.rs`: `use aztec_accelerator::…` → `use accelerator_core::…`. Assert
`cargo tree -p accelerator-server` shows **no tauri / rcgen / rustls**. Record NEW build time + crate
count; compute delta. → `lessons/phase-3.md`.

**Phase 4 — Release CI restructure + measurement (Q3).** `release-accelerator.yml build-headless`: ensure
it builds the `server` crate against `core` (not src-tauri); update rust-cache-key; consider a separate
core-crate cache layer; keep the `--version` stamp assertion (reads `accelerator-server`'s
`CARGO_PKG_VERSION` — unaffected by the move). Update `accelerator.yml` path-filters for the new `core/`
path (a `core/**` change must trip the desktop + headless gates). Add the `cargo tree` no-leak tripwire as
a CI step. `bun run lint:actions` clean. Put the measured before/after delta in the plan.

**Phase 5 — Validation: rc dry-run.** `release-accelerator.yml -f version=X.Y.Z-rc.N` → watch the BLOCKING
updater-smokes (macOS arm64/Intel, Linux, Windows) prove the `.app` still builds/signs/auto-updates, and
the 4 headless tarballs are produced + version-correct. Green = end-to-end behavior preserved.

## Behavior-preservation proof
- ~90 Rust unit tests move WITH their modules → run in `core` (+ GUI-specific tests stay in src-tauri).
  Same assertions, same code.
- GUI binary is functionally identical — same code, statically relinked from `core`. No `[[bin]]` added →
  macOS `.app` bundle topology unchanged (single Mach-O) → `amfid` revalidation path untouched.
- WebDriver E2E + Playwright UI tests unchanged (GUI behavior unchanged).
- rc dry-run updater-smokes = the live proof the signed auto-update path still works.
- `cargo tree` assertions = the proof the dependency reduction actually happened.

## CI / release-speed analysis (Q3, secondary)
The headless leg currently compiles the full Tauri tree (tauri, wry/tao webview, tray-icon, image,
plugin-updater, rcgen, …) — likely **hundreds of transitive crates** it never runs. Dropping them should
cut the headless **cold** (cache-miss) build substantially across all 4 platforms; warm-cache runs benefit
less but the headless cache shrinks. Exact numbers measured in Phase 0 vs Phase 3. Note: this is the
*headless* leg only; the GUI build is ~unchanged (it still compiles everything, just sourced from `core`).

## Security & Adversarial Considerations
- **Threat surface (net NARROWER):** the CI-distributed headless `tar.gz` ships with a much smaller
  transitive dependency set (no Tauri/webview/rcgen) → fewer supply-chain entry points in the artifact
  external projects run in *their* CI. This is a security *win*, not just hygiene.
- **Feature-leak (integrity):** a future `[workspace]` could re-unify features and silently re-arm
  tls/rcgen in headless → mitigated by the Phase-4 `cargo tree` no-leak CI tripwire (fails the build).
- **Artifact integrity:** the headless tarball is `sha256`-summed in the release; confirm the checksum
  step still covers the (smaller) binary. The GUI `.app` stays minisign-signed + notarized — unchanged.
- **Least privilege:** no new secrets/permissions; `build-headless` uses the same scoped tokens. The move
  introduces **no new third-party crates** (it relocates existing ones) — re-confirm with a `cargo tree`
  diff that no *new* dependency appears, only removals on the headless side.
- **Supply chain:** `bun.lock`/`Cargo.lock` committed; the new `core/Cargo.lock` (if standalone) must be
  committed and CI built `--locked`-aware where applicable.

## Assumptions
**Facts (verified):** no Rust `[workspace]` / per-crate `Cargo.lock` (standalone units); edition 2021;
`rcgen` only in `certs.rs`; `server/tls.rs::start_https` receives a prebuilt `RustlsConfig` (no rcgen);
headless imports only `server::{start,AppState,HeadlessState}`,`config`,`authorization`; the 8 core
modules are Tauri-free; `autobins=false` + single `[[bin]]` in src-tauri.
**Inferences (attack these):** dropping Tauri yields a *large* headless cold-build reduction (unmeasured —
Phase 0/3 confirm); the GUI binary relinks byte-functionally-equivalently; `crash_recovery.rs` is GUI-only
(its earlier "3 tauri refs" may be comment/string matches — verify before leaving it in GUI); no current
CI job does a whole-tree `cargo build` that would couple the crates.
**Asks (resolved):** Q1 rc-dry-run (CI-restructure-justified, decoupled from 1.0.5); Q2 = Option (a)
features; Q3 restructure CI + measure; Q4 no `/harden`. No open blocking asks.
