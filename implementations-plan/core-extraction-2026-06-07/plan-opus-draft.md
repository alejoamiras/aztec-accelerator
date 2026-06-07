# Core Extraction — OPUS architect independent draft

> One of three independent Phase-1 plans. Verified by main against the code (build.rs dual-purpose,
> AZTEC_BB_VERSION consumers split core/GUI, /health=CARGO_PKG_VERSION@server.rs:159, bundle invariant
> @release yml:190, AZTEC_VERSION gitignored+generated — ALL CONFIRMED).

## Decisive verified facts
- **No `[workspace]`** anywhere; two independent crates, each with committed `Cargo.lock`. Feature
  unification structurally impossible across crates unless a workspace is introduced.
- **`src-tauri/build.rs` is load-bearing + dual-purpose**: (a) emits `cargo:rustc-env=AZTEC_BB_VERSION`
  from gitignored prebuild-generated `src-tauri/AZTEC_VERSION`; (b) syntax-checks committed
  `packages/accelerator/verified-sites.json`; (c) calls `tauri_build::build()`. `AZTEC_BB_VERSION` is
  consumed via `env!()` by CORE code (`server.rs:146`, `server/prove.rs:62`) AND GUI code (`tray.rs:80`,
  `main.rs:264`). So the env-emit half MUST live in core too, or core won't compile.
- **`certs.rs` = sole rcgen user**; **`server/tls.rs` = sole rustls-serving user**; both reachable only
  from `commands.rs` (143–161) + `main.rs` (60–95). One `tls` feature can gate both. ✔ brief correct.
- Headless imports exactly `authorization`, `config`, `server::{start,AppState,HeadlessState}`; never
  `certs`/`verified_sites`/`start_https`.

## Q2 verdict — Option (a): one `accelerator-core` with `tls` + `verified-sites` features
Reject (b). `certs.rs` + `server/tls.rs` are ONE feature concept (HTTPS/Safari): `tls::start_https`
consumes the `rustls::ServerConfig` that `certs::load_rustls_config` builds. Splitting them across crates
forces a feature flag on the `tls` submodule anyway, so (b) buys nothing while orphaning `certs.rs` in the
GUI crate away from the server module it serves. Single `tls` feature gates both `mod certs` + `mod
server::tls`. GUI deps core `features=["tls","verified-sites"]`; headless deps core
`default-features=false`. **Unification not a risk: no `[workspace]` → separate resolution graphs.**
**Hard constraint: do NOT introduce a `[workspace]`.** Keep three independent packages. Belt-and-suspenders:
`cargo tree -p accelerator-server` no-rcgen/no-tauri CI tripwire.

## Phases
- **P1 Create `accelerator-core`** (additive): new `core/` lib (no `[[bin]]`). Copy non-Tauri deps; put
  `rcgen`/`tokio-rustls`/`rustls-pemfile`/`x509-parser` behind `[features] tls` (optional=true). `[features]
  verified-sites=[]`. Move modules `authorization,bb,config,versions,server.rs+server/{auth,bind,probe,
  prove,tls},certs,verified_sites` → core/src/. `lib.rs`: `#[cfg(feature="tls")] pub mod certs;` etc.;
  `server.rs` gates `#[cfg(feature="tls")] pub use tls::start_https;`. Move `log_dir()` to core.
  **Split build.rs**: `core/build.rs` does env-emit + `../verified-sites.json` check (same relative depth,
  core is sibling of src-tauri); NO `tauri_build::build()`. `src-tauri/build.rs` keeps `tauri_build::build()`
  + its OWN `AZTEC_BB_VERSION` emit (tray/main still `env!` it). Intra-core `crate::` paths unchanged.
- **P2 Rewire `src-tauri`**: add `accelerator-core = { path="../core", features=["tls","verified-sites"] }`;
  `lib.rs` becomes a thin facade: `pub use accelerator_core::{authorization,bb,certs,config,server,
  verified_sites,versions,log_dir};` + GUI-only `pub mod commands/updater/crash_recovery`. This keeps
  `main.rs`/`tray.rs`/`windows.rs` edit-free (they import via `aztec_accelerator::`). Edit only
  `commands.rs`+`updater.rs` (`crate::<core>` → `accelerator_core::<core>`).
- **P3 Repoint `accelerator-server`**: `path=../core, default-features=false`; main.rs 3 import lines
  `aztec_accelerator::`→`accelerator_core::`; regen `server/Cargo.lock`; assert `cargo tree -p
  accelerator-server` empty of `tauri|rcgen|tokio-rustls`.
- **P4 CI restructure + measure** (see below).
- **P5 rc dry-run** validation.

## CI restructure (Q3) + measurement
- **Measure FIRST on main (baseline, read-only):** per target, `cargo tree -p accelerator-server --edges
  normal --prefix none | sort -u | wc -l` + cold `cargo build --release` wall-time. Repeat post-P3.
- `build-headless`: add a `core -> target` cache scope alongside `server -> target`; desktop `build` adds
  `core -> target` too (different feature fingerprint → separate cache entry, correct).
- **CORRECTNESS-CRITICAL:** the version-patch step today patches `src-tauri/Cargo.toml` (for `/health` lib
  `env!`) + `server/Cargo.toml` (`--version`). After the move, `/health`'s `CARGO_PKG_VERSION` +
  `AZTEC_BB_VERSION` come from **core** (server.rs lives there). So patch **`core/Cargo.toml`** (→/health)
  + `server/Cargo.toml` (→--version); `src-tauri` patch stays only for desktop `build`. Same in
  `bump-source` (now 3 crates + 3 locks).
- `bun run lint:actions` before pushing workflow edits.

## Behavior-preservation / validation
1. `cargo test -p accelerator-core --features tls,verified-sites` (full) + `cargo test -p accelerator-core`
   (headless subset, proves gated build compiles+passes). certs tests need `tls`.
2. `cargo tree` asserts: headless ABSENT tauri/rcgen/tokio-rustls; desktop STILL contains tauri+rcgen.
3. `/health` + `--version` wire-contract guards: smoke hits `/health` (status==ok, api_version==1); extend
   to assert `.version==release` (closes the core-version-stamp gap). `--version` assert stays.
4. rc dry-run (Q1) full updater-smokes (mac arm64/Intel ±, Linux, Windows ±) — proves signed `.app`
   unchanged. Decoupled from 1.0.5.
5. PR sequencing: PR1 create core (additive) → PR2 rewire GUI+facade → PR3 repoint server+lock+tree-assert
   → PR4 CI restructure+measure. PR1–3 behavior-preserving; PR4 the only release-path touch, rc-validated.

## Security & Adversarial
- **Signed `.app` topology unchanged — CONFIRMED.** Bundle invariant (release yml ~200) asserts `MacOS/` ==
  `{aztec-accelerator, bb}`. `tauri build` bundles `[[bin]]` of the *same package* as desktop main; core is
  a lib *dependency* (no `[[bin]]`) → never bundled. `autobins=false` stays. amfid class can't recur.
- **Feature unification — neutralized by topology.** No workspace → headless resolver never sees GUI's
  `features=["tls"]`. ONLY a future root `[workspace]` could re-unify (resolver-v2 unifies features per
  dep). Mitigation: don't add a workspace; document in `core/Cargo.toml` (mirror `autobins=false` comment);
  `cargo tree` no-rcgen CI tripwire is the loud regression guard; keep `server` `default-features=false`.
- **No cycle**: deps flow strictly `server→core` and `aztec-accelerator→core`; GUI callbacks
  (`on_status`,`show_auth_popup`) already injected via `AppState` fn-pointers → core needs no upward ref.
- **Version-stamp integrity**: `--version` reads server's `CARGO_PKG_VERSION` (unchanged); `/health.version`
  reads CORE's (server.rs moved) → CI must patch `core/Cargo.toml`. `--version` assert catches server
  mismatch; extend `/health` jq to catch core mismatch.
- **build.rs/AZTEC_BB_VERSION (most likely silent break):** both build scripts emit it (per-crate `env!`).
  Gitignored `AZTEC_VERSION` must be produced where each build.rs reads it — prebuild must seed `core/` or
  core/build.rs reads `../src-tauri/AZTEC_VERSION`. Guard: `cargo build -p accelerator-core` + assert
  `/health.aztec_version != "unknown"` in CI.
- **CI tarball attacker surface**: unchanged trust root, NARROWED — removes rcgen/rustls/tauri-updater from
  the shipped headless binary. `.sha256` is non-authenticated (pre-existing, out of scope). Reviewers must
  diff `server/Cargo.lock` to confirm no NEW transitive deps (core's headless deps ⊂ what src-tauri pulled).

## Assumptions
**Facts (verified):** no workspace, 2 committed locks; build.rs triple-duty; AZTEC_BB_VERSION used by
core+GUI; /health=CARGO_PKG_VERSION@server.rs:159; certs=sole rcgen, tls=sole rustls-serving; headless
imports only authorization/config/server::{start,AppState,HeadlessState}; setup-accelerator already
parameterizes per-crate rust-workspaces+shared-key; build/build-headless already parallel.
**Inferences:** facade lib.rs keeps main/tray/windows edit-free (high); crash_recovery stays GUI (medium —
0 core refs but runtime role not fully traced); build-time/dep delta large (high, unquantified).
**Asks (surface):** (1) **where does prebuild write `AZTEC_VERSION`** now core needs it (seed core/ vs read
../src-tauri/AZTEC_VERSION)? — the one decision that can silently break `/health.aztec_version`. (2) Confirm
no future `[workspace]` planned (the whole isolation guarantee rests on it). (3) Extend `smoke` `/health`
assert to check `.version==release`? (recommended).
