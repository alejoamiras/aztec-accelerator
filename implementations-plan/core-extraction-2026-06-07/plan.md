# Core Extraction — `accelerator-core` from GUI + headless

**Tier:** `/blueprint deep` (3 parallel plans: main + codex `019ea411` + opus → consolidated here).
**Status:** consolidated; contradiction-check + double audit + final codex pending.

## Summary
Extract a **GUI-agnostic, HTTP-only `accelerator-core`** library crate holding the proving stack, so the
headless **`accelerator-server`** (used by external projects to accelerate Aztec/Noir proving in **CI**)
depends on `core` instead of the Tauri GUI crate and **structurally stops compiling Tauri, rustls, and
rcgen**. The GUI keeps its Tauri layer + the TLS/cert/Safari surface. Secondary: drop desktop-only setup
from the headless CI leg and measure the build-time / dependency-count delta. Behavior-preserving;
validated by a dedicated rc dry-run, decoupled from the pending 1.0.5 stable cut.

---

## Decision ledger

### HEADLINE DISPUTE — Q2 core boundary: **resolved → Option (b) "structural", codex's call**
- **main + opus → (a)**: one core crate; move ALL Tauri-free modules (incl. `certs`, `server/tls.rs`) in;
  gate `tls`/`verified-sites` behind cargo features the GUI enables and headless doesn't.
- **codex → (b)**: core = headless-needs-only (HTTP router + proving + state); `certs`, `verified_sites`,
  and the TLS accept-loop stay in the GUI; the GUI's `start_https` wraps a `core::server::router(...)`.
- **RESOLUTION = (b)**, because:
  1. **Structural > conventional.** (a) keeps rustls/rcgen code *in core* and merely feature-gates it off
     for headless — leanness is a convention that a future `[workspace]` can silently break (codex's
     verified warning: a workspace build unifies `accelerator-core/tls` even with `default-features=false`
     on server). (b) puts that code in a *different crate headless never depends on* → headless **cannot**
     compile it. The user's PRIMARY goal is a lean/low-supply-chain headless CI artifact; structural wins.
  2. **It's cheap, verified.** `server.rs:120` already exposes `pub fn router(state) -> Router`, and
     `server/tls.rs::start_https` already just calls `router(state)` + owns the accept loop. So (b) is a
     *move* of a self-contained adapter, not surgery — only `bind_with_retry` + `HTTPS_PORT` need
     `pub(crate)` → `pub`.
  3. **YAGNI.** Headless is `127.0.0.1`-bound CI proving; it will never need HTTPS (HTTPS exists only for
     desktop Safari mixed-content). (a)'s "flip a feature later" benefit is ~worthless here.
  4. **Cohesion.** `certs.rs` is the Safari/macOS keychain-trust story (`install_ca_trust`) — GUI in
     *purpose* even though Tauri-free in imports. `verified_sites` is the popup DTO. Both keep their unit
     tests (run under `cargo test` in `src-tauri`). No test loss.
- **What (a) had going for it (rejected):** single-source-of-truth for the HTTPS path + "flip a feature"
  extensibility. Rejected as YAGNI + outweighed by structural safety. **Open to reversal** if the
  contradiction-check surfaces a concrete headless-HTTPS requirement.

### Other adopted decisions
- **Version-stamp fix (codex, supersedes opus's CI-patch):** `/health.version` today is
  `env!("CARGO_PKG_VERSION")` at `server.rs:159` → after the move it would silently report **core's**
  version. **Inject `app_version` (and the bundled bb-version default) through `AppState`/`HeadlessState`**
  so the reported version is independent of which crate compiles `server.rs`. This is cleaner than patching
  a third `Cargo.toml` in CI, and may let **core be `build.rs`-free** entirely. (codex: "the biggest
  concrete bug in the brief.")
- **CI win (codex, supersedes main+opus's cache-sharing idea):** the real saving is **removing desktop-only
  setup from the headless leg** — `build-headless` currently routes through the desktop `setup-accelerator`
  composite, installs Linux WebKit/GTK, and runs the Tauri-oriented Bun prebuild. Split it
  (`run-prebuild: false` / `install-tauri-system-deps: false`, or a `setup-accelerator-headless`). A shared
  `[workspace]` "for cache" is **rejected** (codex+opus: creates feature-bleed risk for ~no value).
- **`build.rs` handling (opus, verified):** `src-tauri/build.rs` is triple-duty (emits `AZTEC_BB_VERSION`,
  syntax-checks `../verified-sites.json`, calls `tauri_build::build()`). The JSON check + `tauri_build`
  stay in `src-tauri` (verified_sites + Tauri are GUI). `AZTEC_BB_VERSION` is the open question — see Asks.
- **No `[workspace]`** (all three agree): keep three independent packages, each with its own `Cargo.lock`.
  Belt-and-suspenders: a `cargo tree -p accelerator-server` CI tripwire asserting **no tauri/rcgen/rustls**.
- **Signed `.app` bundle topology unchanged** (all three agree, verified): a *library* dep adds no `[[bin]]`;
  the release "stowaway" invariant (release yml:190) + `autobins=false` still hold. rc dry-run still proves it.

---

## Final crate topology
```
accelerator-core   (packages/accelerator/core)   NEW lib, no [[bin]], NO rcgen / rustls / tokio-rustls / tauri
   modules: authorization, bb, config, versions,
            server.rs (router + start[HTTP] + /health + AppState/HeadlessState incl. https_bound),
            server/{auth, bind, probe, prove}
   app_version + bb_version: injected via AppState/HeadlessState (not env!-from-crate)
aztec-accelerator  (src-tauri)   GUI lib+bin  →  accelerator-core (no features)
   keeps: certs, verified_sites, commands, tray, updater, crash_recovery, windows, main,
          + server/tls.rs as a GUI-local start_https adapter calling accelerator_core::server::router()
   build.rs: tauri_build::build() + verified-sites.json check + AZTEC_BB_VERSION (GUI tray/main env!; core is build.rs-free)
accelerator-server (server)      bin  →  accelerator-core (no features); drops the src-tauri dep
```
Dependencies flow strictly **downward** (`server → core`, `aztec-accelerator → core`); core imports neither
GUI crate. GUI callbacks (`on_status`, `show_auth_popup`) are already injected via `AppState` fn-pointers →
**no cyclic dependency**, no trait back-edge to Tauri.

## Phases (small behavior-preserving PRs; each merges green under branch protection + auto-merge)

**Phase 0 — ✓ DONE — Baseline + version-decoupling prep (no crate move yet). [scope tightened by the contradiction-check]**
- Record BASELINE (codex measured **475** unique packages in `cargo tree -p accelerator-server` today, incl.
  tauri/tauri-build/tauri-plugin-*/rcgen/rustls/tokio-rustls). Capture per-target cold `cargo build` wall-time.
- **Version-decoupling prep PR — covers ALL THREE core-bound reads** (the real extraction blockers, flagged by
  BOTH contradiction-checks): `/health.version` = `env!("CARGO_PKG_VERSION")` @server.rs:159, AND the bundled
  bb-version fallback `env!("AZTEC_BB_VERSION")` @**server.rs:146 AND server/prove.rs:62** (the 2nd site neither
  earlier draft named; `env!` is *compile-time*, so missing it = core won't compile in Phase 1). Add
  `app_version: Option<String>` + `bb_version: Option<String>` to `AppState`/`HeadlessState`; read via
  **`…unwrap_or(env!(…))` with the fallback PRESERVED** so `AppState::default()`-based tests (server.rs:213) stay
  behavior-preserving until every constructor injects. GUI keeps its OWN `env!` reads (main.rs:151/264,
  tray.rs:79/80, updater.rs:40) — Phase 0 is *narrowly* server/core decoupling, NOT "all version sourcing
  injected." New test: `/health.version == injected`. → `lessons/phase-0.md`.

**Phase 1 — ✓ DONE — Create `accelerator-core` AND rewire the GUI onto it, in ONE atomic PR. [merged per final codex — the move is NOT merge-green if split]**
- *Why atomic:* "create core (additive)" and "rewire src-tauri" can't be two separate green PRs — the instant
  the modules MOVE out of `src-tauri/src/`, `src-tauri/src/lib.rs` (still `pub mod`s them) and the headless
  binary (still `use aztec_accelerator::…`) break. So one PR moves the modules into `core/` AND repoints
  `src-tauri` at it. Headless still depends on `src-tauri` here (unchanged) → stays green; repointed in Phase 2.
- **New `core/` lib crate** (no `[[bin]]`, no `[workspace]`; comment mirroring `autobins=false`). Move modules
  `authorization`, `bb`, `config`, `versions`, `server.rs` (router + start + /health + state),
  `server/{auth,bind,probe,prove}`. Expose the GUI-adapter seam: `pub use bind::bind_with_retry;` + `pub const
  HTTPS_PORT` (bare `pub fn` is insufficient if the module path stays private — codex). With Phase 0's injection
  in place, **core needs no `build.rs`** (`AZTEC_VERSION` read + `verified-sites.json` check +
  `tauri_build::build()` all stay GUI-side).
- **Rewire `src-tauri`:** add `accelerator-core = { path = "../core" }` (no features). **Thin
  `src-tauri/src/server.rs` wrapper**: `pub use accelerator_core::server::*;` + `mod tls; pub use
  tls::start_https;` → keeps `aztec_accelerator::server::{router, start_https, …}` paths STABLE so
  `main.rs`/`tray.rs`/`windows.rs`/`commands.rs` stay **edit-free**. `lib.rs` re-exports the rest
  (`authorization,bb,config,versions,log_dir`). **Keep** `certs.rs`, `verified_sites.rs`; move `server/tls.rs`
  → `src-tauri/src/server/tls.rs` (`use super::{…}` → `use accelerator_core::server::{router, AppState,
  HTTPS_PORT, bind_with_retry}`). **Construct GUI state with `app_version: Some(env!("CARGO_PKG_VERSION"))` +
  `bb_version: Some(env!("AZTEC_BB_VERSION"))`** so GUI `/health`+`/prove` match its tray label.
- **Add a dedicated `core` CI job** (clippy + `cargo test -p accelerator-core`) to `accelerator.yml` — current
  CI only runs clippy/test in `src-tauri` (final codex), so the new crate would otherwise ship untested.
- Green gate: `cargo test` (core + src-tauri) + WebDriver/e2e + `bun run lint`. → `lessons/phase-1.md`.

**Phase 2 — Repoint headless onto core (the payoff).**
- `server/Cargo.toml`: replace `aztec-accelerator = { path = "../src-tauri" }` with
  `accelerator-core = { path = "../core" }`. `server/src/main.rs`: `use aztec_accelerator::…` →
  `use accelerator_core::…`, and **construct `HeadlessState` injecting `app_version` (its own
  `CARGO_PKG_VERSION`) + `bb_version` (from the copy-bb.ts-equivalent hook — Ask #1)**. Regenerate
  `server/Cargo.lock`. Assert `cargo tree -p accelerator-server` shows **no tauri / tauri-plugin-* / wry/tao
  (webview) / rcgen / tokio-rustls** (the GUI + TLS-*serving* subtree). NOTE (final codex): `reqwest` (bb
  download in `versions.rs`) STAYS → its TLS backend stays too (native-tls→`libssl-dev`, or rustls if we opt
  in); the tripwire targets the GUI/serving subtree, NOT "all TLS." Record NEW build-time + package count;
  compute delta. → `lessons/phase-2.md`.

**Phase 3 — CI restructure + measurement (Q3). [scope widened + claims corrected by final codex]**
- Headless setup drops WebKit/GTK + the Tauri Bun-prebuild — but is **NOT** "Rust-only": `reqwest` still needs
  `libssl-dev` (native-tls) unless we deliberately switch reqwest to `rustls-tls` (a dep choice — note it, don't
  silently assume). The desktop-only setup to strip lives in **THREE places**, not just `build-headless`:
  `_e2e.yml` (GTK/WebKit + prebuild + `BB_BINARY_PATH`) and PR `smoke`/`release-smoke` via the shared
  `setup-accelerator` composite — split it (`run-prebuild`/`install-tauri-system-deps` inputs, or a
  `setup-accelerator-headless`). Wire the headless `bb_version` hook (Ask #1) here. Add the `cargo tree`
  GUI/serving-subtree tripwire as a CI step. (The `accelerator.yml` path-filter **already** covers `core/**` via
  `packages/accelerator/**` — NO filter change needed; the earlier "update path-filters" line was wrong.) Keep
  the `--version` self-report assert; extend the `smoke` `/health` jq to assert `.version == release`.
  `bun run lint:actions` clean. Put the measured before/after delta in the plan/lessons. → `lessons/phase-3.md`.

**Phase 4 — Validation: rc dry-run.** `release-accelerator.yml -f version=X.Y.Z-rc.N` → watch BLOCKING
updater-smokes (macOS arm64/Intel ±, Linux, Windows ±) prove the signed `.app` still builds/signs/auto-updates,
and the 4 headless tarballs are produced + version-correct. Green = behavior preserved end-to-end.

## Behavior-preservation proof
- ~90 Rust unit tests move WITH their modules (server/bb/etc. → core; certs/verified_sites tests stay in
  src-tauri). Same assertions.
- GUI binary functionally identical (same code, relinked from core) → `.app` bundle topology unchanged
  (single Mach-O; `autobins=false`; stowaway invariant) → `amfid` class cannot recur.
- WebDriver E2E + Playwright unchanged. `cargo tree` assertions prove the dependency reduction.
- `/health` (status/api_version/version) + `--version` are the wire-contract guards; rc updater-smokes are
  the live signed-update proof.

## CI / release-speed analysis (Q3)
Baseline: **475** packages in the headless tree (codex-measured), including the entire tauri/webview/tray/
plugin-updater subtree + rcgen + rustls. After (b), headless compiles a strict subset → expected large cold
(cache-miss) build reduction across the 4 headless platforms, plus dropping WebKit/GTK install + Bun prebuild
from the headless setup. Exact numbers measured Phase 0 vs Phase 3 + reported. The GUI build is ~unchanged.

## Security & Adversarial Considerations
- **Threat surface (net NARROWER, but NOT the hot path):** the CI-distributed headless `tar.gz` ships a strict
  subset of build deps (no tauri/webview/rcgen/tokio-rustls-serving) → fewer supply-chain entry points. (b)
  makes this **structural** (the code isn't in core), stronger than (a)'s feature-gate. **But (final codex) the
  real supply-chain target isn't Tauri** — it's the live **bb download+execute** path: the server fetches `bb`
  from GitHub and runs it after verifying a GitHub-API digest (`versions.rs:462`), and the headless tarball is
  itself **unsigned `.tar.gz` + same-channel `.sha256`**. The extraction neither widens nor fixes that (pre-
  existing, out of scope) — the plan does NOT claim a security win on the hot path.
- **Feature-leak / future-workspace:** with (b) there is no `tls` feature on core at all → nothing to leak.
  The `cargo tree` GUI/serving-subtree CI tripwire is the loud regression guard if a workspace is ever added.
- **Bundle integrity:** library extraction adds no `[[bin]]`; the release stowaway invariant + minisign
  signing + notarization of the GUI `.app` are unchanged. Headless `tar.gz` `.sha256` is integrity (not
  authenticity) — pre-existing, out of scope; the extraction neither widens nor fixes it.
- **No new third-party deps:** the move relocates existing crates; reviewers must diff `server/Cargo.lock` to
  confirm only *removals* on the headless side (core's headless deps ⊂ what src-tauri already pulled).
- **Least privilege:** no new secrets/permissions; `build-headless` keeps its scoped tokens.
- **Cyclic-dep / back-edge:** core is strictly downward-only (callbacks already injected via `AppState`).

## Assumptions
**Facts (verified this session):** no Rust `[workspace]` (per-crate committed `Cargo.lock`); `server.rs:120`
`pub fn router(state)->Router` already factored out; `start_https` calls `router()` + owns the accept loop;
`certs.rs` = sole rcgen user, `server/tls.rs` = sole rustls-serving user, both reached only from
`commands.rs`+`main.rs`; headless imports only `server::{start,AppState,HeadlessState}`,`config`,
`authorization`; `/health.version`=`env!("CARGO_PKG_VERSION")`@server.rs:159; `build.rs` triple-duty;
`AZTEC_BB_VERSION` used by core (server.rs:146, prove.rs:62) + GUI (tray.rs:80, main.rs:264); `AZTEC_VERSION`
gitignored + prebuild-generated; baseline 475 pkgs (codex-measured); `autobins=false` + single `[[bin]]`.
**Inferences (attack these):** the `src-tauri/src/server.rs` wrapper (re-export core + GUI-local `start_https`)
keeps main/tray/windows/commands call sites edit-free (high — both contradiction-checks confirmed the
path-stability requirement); `crash_recovery.rs` stays GUI (**high** — contradiction-check confirmed 0 core
refs; pure launchd/systemd/Task-Scheduler lifecycle called only from updater/commands/main); moving
`server/tls.rs` to GUI needs `bind_with_retry` via a **public re-export path** (`pub use bind::bind_with_retry`)
+ `pub const HTTPS_PORT` (not merely `pub fn`); build-time delta large (high, unquantified until measured).
**Asks (RESOLVED by the contradiction-check; recorded for the gate):**
1. **bb-version source for headless → DECIDED: inject from a hook matching `copy-bb.ts` semantics (NOT a vague
   "packaging step").** Final-codex correction: the headless tarball does NOT package `bb` (only the
   `accelerator-server` binary + `.sha256`), so "tracks the packaged bb" was wrong. The **authoritative** source
   today is `packages/accelerator/scripts/copy-bb.ts` (resolves the live `@aztec/bb.js` version → writes the
   gitignored `src-tauri/AZTEC_VERSION`). After dropping the Bun prebuild from the headless leg, headless must
   inject `bb_version` from a step that re-runs the SAME `@aztec/bb.js` resolution (version-only, no full bb
   copy) — NOT `@aztec/stdlib` from `packages/sdk/package.json` (not guaranteed equal), NOT ad-hoc runtime env.
   This is a **named Phase-3 task** (the hook) and it determines whether `/health.aztec_version` stays truthful.
   Core stays `build.rs`-free; the GUI keeps its own `copy-bb.ts`→`AZTEC_VERSION`→`env!` path unchanged.
2. **`/health.version` = injected release/app version → DECIDED** (not the core crate version, which would be
   the silent-misreport bug). Closed.

## Audit trail
### Contradiction-check (Phase 3) — DONE; both model families CONVERGED, findings folded into Phases 0–2
- **Fresh opus verdict:** *"Plan holds with changes — one blocking-if-literal flaw + lower-sev gaps; the (b)
  boundary is sound and behavior-preserving."* Confirmed the `https_bound` Arc survives the crate split (crate
  boundaries are irrelevant to Arc sharing); upgraded `crash_recovery` to high-confidence.
- **Codex (resumed) verdict:** *"(b) faithfully represented; everything merge-green IF the sequencing is respected."*
- **Convergent findings (BOTH), folded in:** (A) `env!("AZTEC_BB_VERSION")` is load-bearing in core
  (server.rs:146 **+ prove.rs:62**) → Phase 0 decouples all 3 core-bound reads, `unwrap_or(env!())` fallback
  preserved until constructors inject; core then `build.rs`-free. (B) **facade contradiction** → fixed with the
  thin `src-tauri/src/server.rs` wrapper (paths stay stable, call sites edit-free). (C) `bind_with_retry` needs a
  public re-export path. (D) GUI constructs state with `Some(env!(...))` so its `/health` matches its tray label.
- **Caveat:** no unit test exercises the real `start_https` bind→flag→/health round-trip across the new crate
  boundary; the rc dry-run (Phase 5) is the only end-to-end proof (acceptable — Safari is desktop-only).
- **Double-audit note:** the two contradiction-checks ran *adversarially* from both families and converged,
  serving as the deep-tier double audit on the consolidated plan; the remaining gate is the final fresh-codex pass.
### Final fresh-context codex pass — DONE (session 019ea424)
- **`VERDICT: conditional approve`** — 4 conditions, ALL now folded into the plan above:
  (1) Phase 1/2 **collapsed** into one atomic merge-green PR + a dedicated `core` CI job (the split move wasn't
      merge-green — `src-tauri` breaks the instant modules move out);
  (2) Ask #1 rewritten to the **`copy-bb.ts` (`@aztec/bb.js`)** authoritative source — the tarball doesn't package bb;
  (3) Phase 3 CI scope **widened** to `_e2e.yml` + smoke/release-smoke; the false "core path-filter" + "Rust-only
      setup" claims corrected (reqwest keeps `libssl-dev`); the `cargo tree` tripwire retargeted to the GUI/serving subtree;
  (4) Security: the **bb download+execute** path named as the real supply-chain target (not Tauri).
- Net: the **design is approved** (Option b + version-injection + wrapper are sound); the conditions were
  plan-accuracy fixes (sequencing / CI / the bb-version hook), now applied. No design rework.

## Seeds
**Recommended — `/goal`** (completion is transcript-observable; stops cleanly before the owner-only release):
```
/goal Phases 0-3 marked ✓ in implementations-plan/core-extraction-2026-06-07/plan.md (the per-phase headers in the file); for each, the agent printed `LESSONS_FILE=implementations-plan/core-extraction-2026-06-07/lessons/phase-N.md` in the transcript; the transcript shows `cargo tree -p accelerator-server` with NO tauri/tauri-plugin/rcgen/tokio-rustls; `bun run test`, `cargo test` (core + src-tauri), `bun run lint`, and `bun run lint:actions` all report exit 0; `/code-review max --fix` applied+committed and the codex post-impl audit clean (or high/critical addressed). STOP before Phase 4 (the rc dry-run dispatches a release — owner-only) and surface it. Never merge to main without green CI; never dispatch a release.
```
**Alternative — `/loop`** (self-paced):
```
/loop Each turn: read implementations-plan/core-extraction-2026-06-07/plan.md + lessons/ as source-of-truth; `git status`; if a PR is open, `gh pr view --json statusCheckRollup` (no --watch). CI in flight on HEAD? watch up to 10 min. Failed check/local failure? triage+fix, `/codex xhigh` if non-trivial, commit small+push (stop after 5 fails on one step). In-flight phase green? mark ✓ in plan.md, file lessons/phase-N.md, print `LESSONS_FILE=…`, advance. Nothing in flight? do the next pending step (edit → cargo test/bun run test → lint → commit → push). Phases 0-3 all ✓? `/code-review max --fix` → commit → codex post-impl audit → address high/critical, then STOP and surface the Phase-4 rc dry-run for the owner. Never merge to main or dispatch a release.
```
_Use exactly one per session — they don't compose._
