# Quality audit — tauri-app cluster (claude)

Run: 2026-06-10-max-q7e3 · Scope: maintainability smells only (no correctness/security).
Files read: `packages/accelerator/src-tauri/src/{main,commands,tray,windows,verified_sites,server,lib}.rs`, `src-tauri/src/server/tls.rs`, `packages/accelerator/server/src/main.rs`, plus `packages/accelerator/core/src/server.rs` + `core/src/server/auth.rs` and `core/lib.rs` to verify the cross-crate leads.

Churn (commits since 2026-03-01): src-tauri `main.rs` **36 (HOT)**, `commands.rs` **19 (HOT)**, headless `server/src/main.rs` 8, `windows.rs` 6, `tray.rs` 2, `verified_sites.rs` 1.

## Lead verdicts (attacked, not anchored)

1. **`.setup()` Long Method — CONFIRMED** (F1). Still ~154 lines / ~9 concerns despite F-03 already extracting `spawn_http_server` + `spawn_update_poller`.
2. **Cross-crate AppState/origin-gating/TLS duplication — LARGELY ALREADY RESOLVED.** The core extraction (`implementations-plan/core-extraction-2026-06-07`, F-01) unified state construction: both binaries call `HeadlessState::headless` (`core/src/server.rs:141-156`) and the `AppState::desktop`/`AppState::headless` builders (`core/src/server.rs:158-184`). Origin-gating *enforcement* lives once in `core/src/server/auth.rs:16-60`; the binaries differ only in how they populate the allowlist (env parsing in `server/src/main.rs:104-149` vs persisted config + popup on desktop) — intentional surface divergence, not duplication. HTTPS/TLS wiring is desktop-only (`src-tauri/src/server/tls.rs`); no headless counterpart exists to dedupe. **Residual cross-crate dup = the tracing bootstrap only** (F6, Low).
3. **`mutate_config` — NOT Middle Man.** It adds lock-sequencing + persistence + error mapping (`commands.rs:12-19`); a good prior Extract Method, used by 6 commands. The smell is the one *bypass* site that re-hand-rolls the pattern (F5).
4. **Window-label duplication — CONFIRMED** (F4), and it spans the bin/lib module boundary, which is why it exists.

---

## F1 — The `.setup()` closure is still a ~154-line god-method with 10 named clone bindings

**Smell:** Long Method (Fowler), with the secondary "clone stutter" symptom the code itself acknowledges (`core/src/server.rs:84` fixed the *cost*; the *readability* stutter remains).

**Impact:** High. Blast radius: single file, but it is the wiring hub for every desktop feature. Change frequency: `main.rs` is the hottest file in the cluster (36 commits since March) — most of those land inside or adjacent to this closure.

**Instances:**
- `packages/accelerator/src-tauri/src/main.rs:314-467` — the closure. Distinct concerns inline: activation policy (316-317), status menu item (320-322), crash-recovery check (324-330), tray menu + icon + menu-event handler (332-355), animation loop (357-359), `on_versions_changed` construction (366-384), `show_auth_popup` construction (386-397), `on_status` construction (399-410), state assembly (411-417), HTTPS bring-up gate (419-428), `SharedAppState` manage (430-433), bb diagnostics (435-444), webdriver window (446-448), HTTP-server spawn (450-456), update poller (458-464).
- The 10 clone bindings that exist only because every closure is built in one scope: `status_clone` (362), `status_for_diagnostics` (363), `tray_clone` (364), `app_handle` (367), `bundled_for_cb` (368), `tray_for_versions` (369), `app_handle_for_auth` (387), `auth_manager_for_timeout` (388), `is_animating_for_status` (399), `tray_for_diagnostics` (439).

**Why future change gets harder:** every new startup concern (a callback, a window, a background task) has exactly one place to land — this closure — so it grows monotonically and concentrates merge conflicts in the hottest file. Readers must mentally bind each `X_for_Y` clone to its consuming closure before they can change anything. None of the three callback constructions is unit-testable (the only tests in `main.rs` are for `should_prevent_exit`); the callbacks are tested in core only via hand-built mock states.

**Smallest safe refactoring:** Extract Method, three times, into free functions in `main.rs` (no API change): `make_on_versions_changed(app_handle, dev_mode, bundled, status, tray) -> VersionsChangedCallback`, `make_show_auth_popup(app_handle, auth_manager) -> ShowAuthPopupCallback`, `make_on_status(status, tray, is_animating) -> StatusCallback` (or take the F3 `StatusSurface`). Each clone becomes an owned parameter. The closure shrinks to ~40 lines of named orchestration steps; F-03 already established the precedent.

**What disappears:** the 10 `X_for_Y`/`X_clone` bindings (each becomes a function parameter moved into one closure); the need to read 154 lines to find where a callback is wired; the untestability of the callback construction.

---

## F2 — HTTPS bring-up sequence (SEC-08 gate → TLS load → spawn) duplicated and comment-coupled across two hot files

**Smell:** Duplicate Code (Fowler), with Shotgun Surgery as the consequence: a change to the "bring up HTTPS" contract must be applied at two sites in two files. F-09 (`src-tauri/src/server.rs:11-14`) consciously unified only the innermost spawn wrapper and left the rest "intentionally divergent" — but the *gate + load + spawn* trio is not divergent, it is mirrored.

**Impact:** Medium-High. Blast radius: `main.rs` (HOT, 36) + `commands.rs` (HOT, 19), same package. The repo's own history proves the failure mode: the `commands.rs:156-161` comment records that the SEC-08 gate originally existed only on the startup path and the Settings path had to be patched after a codex post-impl review (M1) — i.e. the duplication already produced exactly the missed-mirror defect once.

**Instances (the duplicated sequence):**
- Startup path: gate at `packages/accelerator/src-tauri/src/main.rs:424-428` (`migrate_legacy_ca_key` → only then `try_start_https`); TLS load + spawn at `main.rs:72-84` inside `try_start_https`.
- Settings path: gate at `packages/accelerator/src-tauri/src/commands.rs:162-164`; TLS load + spawn at `commands.rs:176-180`.
- Already-shared tail: `spawn_https` in `packages/accelerator/src-tauri/src/server.rs:15-24` (F-09).
- The coupling is maintained by prose, not structure: `commands.rs:156-161` ("the startup path runs this same fail-closed migration… Without mirroring it here…") cross-references `main.rs:420-423`.

**Why future change gets harder:** any new pre-flight condition for HTTPS (another migration, a cert-shape check, a renewal hook) must be remembered at both call sites; forgetting one is a *silent* policy gap with no compile or test failure — the proven historical failure shape. The genuinely divergent parts (startup checks `certs_exist`/`is_ca_trusted` and resets config on failure; Settings *generates* certs and bubbles errors to the UI) are interleaved with the mirrored parts, so readers can't tell which lines are policy and which are accident.

**Smallest safe refactoring:** Extract Function into the lib crate (next to `spawn_https`): `pub fn spawn_https_checked(state: AppState) -> Result<(), String>` owning gate → `load_rustls_config` → `spawn_https`. Each caller keeps its divergent middle (cert generation / trust checks) and its divergent failure policy (`reset_safari_support` + log vs `map_err` to the Settings UI) around a single call.

**What disappears:** both mirrored 3-step sequences (each becomes one call + one error-map); the two "keep these in sync" comments; the class of missed-mirror regressions for future pre-flight conditions.

---

## F3 — `(status MenuItem, tray TrayIcon)` is a Data Clump written in lockstep at three sites

**Smell:** Data Clumps (Fowler) — the pair always travels together and every write is a dual-write of the same string; the resulting multi-site edits are incipient Shotgun Surgery. The invariant ("status text and tooltip must mirror, because production hides the menu item but shows the tooltip") is documented twice instead of being enforced once (`main.rs:435-438`, `tray.rs:106-109`).

**Impact:** Medium. Blast radius: `main.rs` (HOT) + `tray.rs`; also degrades testability of `spawn_http_server`'s error path (constructing a `MenuItem` + `TrayIcon` requires a live Tauri app, so the AddrInUse classification at `main.rs:192-216` can only be tested end-to-end).

**Instances (all dual-write sites):**
- `packages/accelerator/src-tauri/src/main.rs:212-218` — server bind-failure path (`set_text` + `set_tooltip` with the same `msg`).
- `packages/accelerator/src-tauri/src/main.rs:400-410` — the `on_status` callback (`set_text` 403, `set_tooltip` 406).
- `packages/accelerator/src-tauri/src/main.rs:440-444` — bb-not-found diagnostics.
- Clump in a signature: `spawn_http_server(state, status: MenuItem, tray: TrayIcon, app_handle)` at `main.rs:180-185`.

**Why future change gets harder:** adding a third status surface (e.g. a Settings-window status field) or changing the mirroring rule means finding and editing three scattered sites; missing one yields the exact prod-visible drift the comments warn about (tooltip says one thing, menu says another). The clump also forces the paired clones counted in F1 (`status_clone`+`tray_clone`, `status_for_diagnostics`+`tray_for_diagnostics`).

**Smallest safe refactoring:** Extract Class — `struct StatusSurface { status_item: MenuItem<Wry>, tray: TrayIcon<Wry> }` in `tray.rs` with `fn set(&self, text: &str)` (and the existing error-logging). Construct once in `.setup()`; pass one `StatusSurface` to `spawn_http_server` and into `on_status`. Optionally Extract Method the pure bind-failure classification (`fn classify_bind_failure(&dyn Error) -> BindFailure`) so it becomes unit-testable without Tauri — that also answers the "spawn_http_server mixes error-classification + tray wiring" lead.

**What disappears:** three dual-write sites collapse to three `surface.set(...)` calls; four clone bindings become two; one of the two mirrored invariant comments; the impossibility of unit-testing the bind-failure policy.

---

## F4 — Window-label wire format derived independently by the window creator and the window closer

**Smell:** Duplicate Code (Fowler) — a magic format string acting as a cross-module wire contract, constructed at both ends. (The deeper cause is the bin/lib split: `windows.rs` is a *binary* module, `commands.rs` is the *lib*, and lib can't call into bin — so the closers re-derive labels inline. `sanitize_window_label` was already moved to the lib for exactly this reason; the composition wasn't.)

**Impact:** Low-Medium. Blast radius: 2 files, 4 sites; failure mode is silent (`get_webview_window` returns `None` and the `if let Some` no-ops — the popup simply stops closing, no error, no test failure). Change frequency: `commands.rs` HOT (19), `windows.rs` 6.

**Instances:**
- Auth label `format!("auth-{}", sanitize_window_label(..))`: creator `packages/accelerator/src-tauri/src/windows.rs:87` vs closer `packages/accelerator/src-tauri/src/commands.rs:131` (the 60s-timeout closer at `windows.rs:121` reuses the captured string — fine).
- Update-prompt label literal `"update-prompt"`: creator `windows.rs:144` vs closer `commands.rs:273` (`close_update_prompt`).

**Why future change gets harder:** renaming a label, changing the hash truncation, or adding a discriminator must be done in two files in lockstep; the SEC-06 history (labels re-keyed from origin to request_id) shows this exact string *does* get redesigned. A half-applied change ships green and fails only as a stuck window in manual testing.

**Smallest safe refactoring:** Extract Function in the lib next to `sanitize_window_label`: `pub fn auth_window_label(request_id: &str) -> String` (fold the `auth-` prefix in — these two sites are its only callers) and `pub const UPDATE_PROMPT_LABEL: &str = "update-prompt";`. Consume from both `windows.rs` and `commands.rs`.

**What disappears:** four inline label constructions become two shared definitions; the silent creator/closer desync mode; `sanitize_window_label` stops being a half-shared contract.

---

## F5 — `reset_safari_support` hand-rolls the exact lock-mutate-save pattern `mutate_config` was extracted to kill

**Smell:** Duplicate Code (Fowler) — a surviving copy of an already-extracted pattern, and it has *already diverged* (the textbook cost): `mutate_config` propagates the save error; the copy discards it with `let _ =`.

**Impact:** Low. Blast radius: 2 files; one bypass site. But it sits in the HOT pair (`main.rs` 36 / `commands.rs` 19), and `mutate_config`'s doc comment claims it is "the single source of truth", which is now false — misleading for the next contributor.

**Instances:**
- Canonical helper: `packages/accelerator/src-tauri/src/commands.rs:12-19` (private `fn`, which is *why* the copy exists — main.rs can't call it).
- Hand-rolled copy: `packages/accelerator/src-tauri/src/main.rs:98-104` (`cfg_lock.write()` → mutate → `config::save`).

**Why future change gets harder:** any change to the persistence pattern (validation before save, debounce, error surfacing, audit log) lands in `mutate_config` and silently misses the copy; the divergent error handling already demonstrates the drift.

**Smallest safe refactoring:** change visibility (`pub fn mutate_config`) and replace the body of `reset_safari_support` with `let _ = mutate_config(cfg_lock, |c| c.safari_support = false);` — keeping the discard *explicit and local* if intentional.

**What disappears:** the last hand-rolled instance of the pattern; the false "single source of truth" claim becomes true again.

---

## F6 — Tracing bootstrap duplicated across the two binaries (the residue of the cross-crate lead)

**Smell:** Duplicate Code (Fowler), cross-crate — the only duplication actually left between `src-tauri/src/main.rs` and `server/src/main.rs` (see Lead verdict 2: state construction and origin-gating are already unified in core).

**Impact:** Low. Blast radius: 2 crates (cross-crate = elevated per line, but it's ~6 mirrored lines + a 4-line import block). Change frequency of logging setup itself: low.

**Instances:**
- `packages/accelerator/src-tauri/src/main.rs:259-265` — `EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))` + `registry().with(env_filter).with(stdout fmt layer)` (+ desktop-only file layer at 250-257, 264) + `.init()`.
- `packages/accelerator/server/src/main.rs:37-42` — byte-equivalent filter + registry + stdout layer + `.init()`.
- Mirrored import blocks: `src-tauri/src/main.rs:24-27` and `server/src/main.rs:19-22`.

**Why future change gets harder:** changing the default level, the line format, or adding a shared field/JSON output requires editing two crates that don't share CI-visible structure; they can drift (one binary gets the improvement, the other doesn't, and nothing flags it).

**Smallest safe refactoring:** Extract Function — `pub fn init_tracing(file_dir: Option<&Path>) -> Option<WorkerGuard>` shared by both binaries. Placement is the only real decision: adding `tracing-subscriber` (env-filter feature only — no GUI deps, the headless crate already depends on it) to `accelerator-core` does not violate the no-feature-unification rule in `core/Cargo.toml`. If keeping core lean is preferred, accept the duplication consciously — at ~6 lines this is also defensible; flagging it here mainly closes out the lead.

**What disappears:** the mirrored bootstrap + import blocks; the drift channel between the two binaries' log behavior.

---

## F7 — `VerifiedSite` / `VerifiedSitesEntry` are parallel structs joined by a 4-field copy loop

**Smell:** Duplicate Code (Fowler) — parallel data structures: two structs with four identical fields plus a manual field-by-field copy keeping them in sync.

**Impact:** Low. Blast radius: 1 file; churn 1 commit (cold). The registry's *data* changes in JSON; the schema changes rarely — but when it does (schema v2, a new curator field), it's a 3-site edit today.

**Instances:**
- `packages/accelerator/src-tauri/src/verified_sites.rs:23-34` (`VerifiedSite`) vs `verified_sites.rs:43-51` (`VerifiedSitesEntry` = same four fields + `origins`).
- The sync copy: `verified_sites.rs:93-99`.

**Why future change gets harder:** every schema field addition must touch both structs and the copy loop (and forgetting the copy compiles fine if the field has a `Default`/`Option`).

**Smallest safe refactoring:** Collapse Hierarchy via serde — `struct VerifiedSitesEntry { origins: Vec<String>, #[serde(flatten)] site: VerifiedSite }`; the loop body becomes `let site = entry.site;`. Behavior-preserving (same JSON shape), existing tests pin it.

**What disappears:** one struct's worth of mirrored fields and the 4-field copy; field additions become single-site edits.

---

## F8 — `build_tray_menu` repeats the shared menu tail in both dev/prod arrays

**Smell:** Duplicate Code (Fowler), minor — the dev and prod `.items(&[...])` arrays share 5 items (`settings`, separator, `version_text`, `github`, `quit`) listed twice.

**Impact:** Low. Blast radius: 1 file; churn 2 commits (cold).

**Instances:** `packages/accelerator/src-tauri/src/tray.rs:94-105` (dev array) vs `tray.rs:110-112` (prod array).

**Why future change gets harder:** any menu item meant for both modes must be added to two literals; ordering mistakes between the arrays are easy and only visible by eyeballing the running tray.

**Smallest safe refactoring:** Extract Variable/Method — build a `Vec<&dyn IsMenuItem<Wry>>` with the shared tail and conditionally `insert`/`push` the dev-only items (`status`, versions submenu, `show_logs`), then one `MenuBuilder::new(app).items(&items)` call.

**What disappears:** the duplicated 5-item tail; the two-array ordering hazard.

---

## Non-findings (verified, deliberately not flagged)

- **`mutate_config` as Middle Man** — no; it owns sequencing + persistence + error mapping (see Lead verdict 3).
- **`HeadlessState`/`AppState` builders + lib.rs re-export shim** (`src-tauri/src/lib.rs:8`, `src-tauri/src/server.rs:6`) — module-level `pub use` forwards new items automatically; delegation cost ≈ 0, documented as a compatibility layer. Not a Middle Man worth money.
- **Headless gating parser vs desktop config** — different inputs by design over shared core enforcement; unifying them would be speculative generality.
- **`ICON_PROVING` 24× `include_bytes!`** (`tray.rs:13-38`) — mechanical, but `include_bytes!` needs literal paths; deduping requires a proc-macro dep or build script. Cost exceeds benefit; style-adjacent.
- **`#[cfg(feature = "webdriver")]` gating** (7 sites in `main.rs`) — inherent to the E2E strategy; no measurable change-cost amplification found beyond what the feature requires.
- **13 commands in one `commands.rs`** — 276 cohesive lines; not Large Class.

## Out-of-scope observations

`reset_safari_support` (`main.rs:102`) discards the config-save error that `mutate_config` deliberately propagates (Q9 fixed exactly this swallow elsewhere) — flagging as behavior, not quality.
