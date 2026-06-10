# Quality audit — tauri-platform cluster (claude)

Date: 2026-06-10 · Run: max-q7e3 · Scope: maintainability only (no correctness/security)

Files: `packages/accelerator/src-tauri/src/certs.rs` (632), `crash_recovery.rs` (493), `updater.rs` (237), `server/tls.rs` (73). Callers traced: `main.rs`, `commands.rs`, `server.rs` (caller-tracing capped at 4 functions: `try_start_https`, `enable_safari_support`, `perform_update`, the tray `quit` handler).

## Lead verdicts (attacked, not anchored)

- **crash_recovery 3 platform impls = copy-pasted Parallel Inheritance?** NO. Dispatch is cleanly trait-abstracted post-#305 (`CrashRecovery` trait → `PlatformRecovery` ZST → `#[cfg]`-selected `enable_impl`/`disable_impl`). The three bodies are irreducibly different mechanisms (plist patch / systemd unit / Task Scheduler XML) — flagging them would be flagging irreducible `#[cfg]` gating. The real smells are *above* the trait (Findings 2, 3, 5) and in micro-boilerplate (Finding 6).
- **certs.rs Temporal Coupling without a type enforcing order?** YES — but the coupling lives in the **callers**, not inside certs.rs (Finding 1). Large Module confirmed separately (Finding 4).
- **updater.rs ↔ crash_recovery coupling?** YES, two distinct smells: a convention-enforced disarm/rearm protocol (Finding 2) and a triple-maintained arming policy (Finding 3).
- **tls.rs build-vs-load split responsibility?** NON-FINDING. The lead's premise is off: `server/tls.rs` does not build TLS config — it consumes `Arc<rustls::ServerConfig>` built by `certs::load_rustls_config()` (certs.rs:257-273). certs.rs owning PEM→config is cohesive (it owns the PEMs); tls.rs:15-73 is a clean single-purpose accept loop. The only residual cost (the load→spawn pairing duplicated at two call sites) is subsumed by Finding 1.

---

## Finding 1 — The Safari-HTTPS enable sequence is a comment-enforced state machine duplicated across two orchestrators

**Smell:** Temporal Coupling (ops must run in a fixed order: migrate → generate/verify → trust → load → spawn, with no type or function enforcing it) + Duplicate Code / Shotgun Surgery (every new mandatory step must be mirrored into both orchestrators) — Fowler: Duplicate Code, Shotgun Surgery; Temporal Coupling as the named analog for the order constraint.

**Impact:** High. Blast radius: certs.rs (WARM, 5 commits), `commands.rs` (19 commits), `main.rs` (36 commits) — the orchestrators live in the two churniest files of the app. Change frequency of the sequence itself: 3 of certs.rs's last 4 PRs (#288 keyless CA, #335 CertPaths, #342 fail-closed migration) altered a step or its ordering contract.

**Instances:**
- `packages/accelerator/src-tauri/src/commands.rs:162-180` (`enable_safari_support`): migrate_legacy_ca_key → generate_and_save → install_ca_trust → save config → load_rustls_config → spawn_https.
- `packages/accelerator/src-tauri/src/main.rs:424-428` + `main.rs:55-94` (`try_start_https`): migrate_legacy_ca_key → certs_exist → is_ca_trusted → load_rustls_config → spawn_https → background regenerate_leaf_if_expiring.
- certs.rs exposes the steps as 7 independent pub fns with no ordering type: `certs_exist` (133), `generate_and_save` (170), `migrate_legacy_ca_key` (181), `load_rustls_config` (257), `regenerate_leaf_if_expiring` (298), `install_ca_trust` (427/439), `is_ca_trusted` (433/444).
- The ordering rules exist only as comments: `commands.rs:157-161` ("Without mirroring it here, a Settings off→on toggle would re-enable Safari HTTPS next to a readable legacy mint-any-cert key…") and `main.rs:420-423`.

**Evidence it already bit:** the `commands.rs:157-161` comment documents that the Settings path *was* missing the migrate step until a post-impl audit (codex M1 / SEC-08) caught the divergence. The two preambles were left "intentionally divergent" by a prior refactor (`server.rs:11-14`, F-09) — that refactor unified only the spawn wrapper, so the invariant-order problem was seen and deliberately deferred.

**Why future change gets harder:** any new mandatory pre-HTTPS step (next migration, a revocation check, a new staging rule) needs 2 edits in 2 files, and the failure mode of forgetting one is silent — the sequence still compiles and runs, just with the invariant hole on one path. A third entry point (e.g. a "repair certs" Settings action) would make it 3 edits.

**Smallest safe refactoring:** Extract Function into certs.rs — `prepare_https(mode: Provision | VerifyOnly) -> Result<Arc<rustls::ServerConfig>, …>` owning migrate→(generate+trust | exist+trusted)→load in one body; both call sites keep their own failure *reaction* (Settings surfaces the error string; startup calls `reset_safari_support`) but stop owning the order. Stretch (not required): a move-only `HttpsReady(Arc<ServerConfig>)` token consumed by `spawn_https` so serving-before-verifying can't compile.

**What disappears:** the mirrored 5-step preambles, the "mirror this in the other file" comments, and the whole class of one-path-missed-a-step divergences.

---

## Finding 2 — Windows disarm→install→rearm is a manual paired-call protocol; every exit path must remember the rearm

**Smell:** Temporal Coupling / paired-call protocol with no guard object (named analog of Fowler's Shotgun-Surgery-prone protocol code; remedy is the classic RAII "Extract Class: guard").

**Impact:** Medium-High. Blast radius: `updater.rs` (WARM — 4 commits, 2 in the last 10 days: #341, #346) and indirectly `crash_recovery.rs`. The invariant guards real behavior (recovery silently off ⇒ crashed app stays down), so a maintenance miss is invisible until a crash.

**Instances (all in `packages/accelerator/src-tauri/src/updater.rs`):**
- Disarm: 151-161 (`if !crate::crash_recovery::disable_crash_recovery()` …).
- Manual rearm site 1: 159 (abort path).
- Manual rearm site 2: 168-169 (post-install, pre-restart).
- Manual rearm site 3: 176-177 (install-failed path).
- The invariant exists only as a comment: 157-160 — "every path that leaves the app running must end armed."

**Why future change gets harder:** `perform_update` is the file's hot edit zone (size cap added at 109-122 in #341; comment block extended in #346). Any new early-return inserted between the disarm (152) and `install` (163) — another pre-flight check is the obvious next change — silently violates the invariant unless the author re-discovers the comment and adds a 4th rearm call. The compiler offers zero help; the cfg-gating means non-Windows CI can't catch it either.

**Smallest safe refactoring:** Replace the paired calls with a Drop guard in `crash_recovery` (Extract Class): `struct DisarmGuard; impl Drop { rearm_if_enabled() }`, with `fn defuse(self)` called only on the restart path. `perform_update` becomes `let _guard = crash_recovery::disarm_for_install(app)?;` — one line, all exit paths covered structurally.

**What disappears:** the 3 manual rearm calls, the load-bearing comment, and the forgot-to-rearm bug class on every future edit of `perform_update`.

---

## Finding 3 — The "recovery armed ⇔ autostart enabled" policy is owned by three different files

**Smell:** Shotgun Surgery (one policy, three edit sites) + Feature Envy (`updater.rs::rearm_crash_recovery_if_enabled` interrogates the autostart plugin to re-derive crash_recovery's desired state — knowledge that belongs next to the recovery code, not in the updater).

**Impact:** Medium. Blast radius: `main.rs`, `commands.rs`, `updater.rs`, `crash_recovery.rs`. Frequency: the policy was touched by both the Windows-parity work (#269/P4) and the updater hardening (#341/#346) streams — two independent change axes already converge on it.

**Instances:**
- `packages/accelerator/src-tauri/src/main.rs:324-330` — launch: arm iff `autolaunch().is_enabled()`.
- `packages/accelerator/src-tauri/src/commands.rs:42-53` (`set_autostart`) — toggle: enable→arm / disable→disarm.
- `packages/accelerator/src-tauri/src/updater.rs:184-190` (`rearm_crash_recovery_if_enabled`) — re-derives the same iff from `app.autolaunch().is_enabled()`.

**Why future change gets harder:** decoupling recovery from autostart (a plausible Settings ask — "restart on crash" without "launch at login") or changing the source of truth (config flag instead of plugin query) is a 3-file edit, and a missed site produces a state skew that only shows up after a crash or an update.

**Smallest safe refactoring:** Move Function — `crash_recovery::sync_with_autostart(app: &AppHandle)` (or `sync(enabled: bool)`) as the single owner of the iff; the three sites call it. Folds naturally into Finding 2's guard (the guard's Drop calls `sync`).

**What disappears:** triple maintenance of the invariant and updater.rs's reach into autostart-plugin state.

---

## Finding 4 — certs.rs is one module with five responsibilities and four independent change axes

**Smell:** Large Class (module form) → Divergent Change (Fowler). The macOS-keychain block also creates inline `#[cfg]` interleave in otherwise platform-neutral logic.

**Impact:** Medium. Blast radius: certs.rs only (callers are stable against the pub surface). Change frequency: WARM — 5 commits; history shows the axes changing independently: #75 (expiry parsing), #288 (crypto/key design), #335 (path layout), #342 (migration semantics).

**Instances (all `packages/accelerator/src-tauri/src/certs.rs`):**
- Responsibility clusters in one file: path layout 13-77 (`certs_dir`, `ca_key_path`, `CertPaths`); cert generation 89-176 (`ca_params`, `leaf_params`, `write_new_cert_set`, `generate_certs`, `generate_and_save`); legacy migration 181-217; atomic PEM file IO 222-254 (`write_pem_file`); rustls loading 257-273; X.509 expiry parsing 280-290; rotation orchestration 294-346; macOS `security`-CLI keychain adapter + non-mac stubs 348-446 (~95 lines).
- `rotate()` (317-346) carries three inline `#[cfg(target_os = "macos")]` blocks (325-326, 329-333, 339-342) inside platform-neutral staging/swap logic — every rotation edit must be reasoned about in 2 compile variants.
- Decay markers consistent with a too-big file: duplicated section banner at 348 + 350; contradictory stale doc on `leaf_cert_days_remaining` (275-279 says both "uses file modification time as a proxy" and "not file mtime" — the first paragraph is pre-#75 leftover).

**Why future change gets harder:** a keychain-mechanism change (e.g. `security` CLI arg changes, or Linux/Windows trust-store support — the stubs at 438-446 are the seam where that lands) forces edits inside the same file as the crypto-params and rotation logic, and grows the cfg interleave in `rotate()`. Each axis's reviewers must re-read the other axes' invariants (keyless-CA, staging order) on every diff.

**Smallest safe refactoring:** Extract Class/Module — move 348-446 to `certs/trust.rs` (or `keychain.rs`) behind a tiny platform-neutral surface (`install(cert) -> Result`, `is_trusted(cert) -> bool`, `current_anchor_sha1()`, `remove_anchor(sha1)`) with a Null Object non-mac impl; `rotate()` then calls it unconditionally and loses all three inline cfg blocks. Delete the stale doc paragraph and duplicate banner in the same diff.

**What disappears:** the cfg interleave in `rotate()`, the mixed-axis review burden, and ~100 lines from the largest WARM file in the cluster.

---

## Finding 5 — The CrashRecovery trait facade is incomplete: callers still hold cfg-gated Windows lifecycle knowledge

**Smell:** Shotgun Surgery via incomplete abstraction (named analog: leaky facade — the trait abstracts enable/disable but not the lifecycle moments where platforms actually diverge). Secondary: inconsistent use of the `disable() -> bool` contract.

**Impact:** Low-Medium. Blast radius: `main.rs`, `updater.rs`, `crash_recovery.rs`, `commands.rs`. Frequency: WARM-adjacent — every Windows-recovery change (#269, P4, #346 era) touched callers, not just the impls.

**Instances:**
- `packages/accelerator/src-tauri/src/crash_recovery.rs:12-19` — the trait *doc* hardcodes per-platform semantics ("always `true` where disarm is unconditional (macOS/Linux), the real /Query-verified result on Windows").
- `packages/accelerator/src-tauri/src/main.rs:338-348` — tray `quit` handler carries a `#[cfg(target_os = "windows")]` call + a 6-line comment explaining the Windows resurrection model.
- `packages/accelerator/src-tauri/src/updater.rs:151-152, 168-169, 177, 184-190` — four cfg-gated call sites encoding the same model.
- Contract inconsistency: `updater.rs:152` checks the `disable()` bool (must), `commands.rs:50` and `main.rs:346` discard it — each new caller must re-derive whether they're a "must know" caller from the trait doc.

**Why future change gets harder:** a 4th platform — or moving Linux to an always-armed model — means auditing every cfg'd call site for which lifecycle hooks it needs, not just writing a new `enable_impl`/`disable_impl` pair. The trait promises platform-agnosticism (`crash_recovery.rs:8-10`: "callers stay platform-agnostic") that callers demonstrably don't get.

**Smallest safe refactoring:** Form Template Method on the trait — add lifecycle hooks with no-op defaults: `on_clean_quit()` (Windows: delete task; others: no-op) and `disarm_for_install() -> DisarmGuard` (ties into Finding 2). The quit handler and updater then call them unconditionally; all caller-side `#[cfg]` for crash recovery is deleted.

**What disappears:** 5 cfg-gated caller sites, the duplicated Windows-model comments in two caller files, and the per-caller "do I check the bool?" decision.

---

## Finding 6 — Re-rolled OS-command and path boilerplate across certs.rs and crash_recovery.rs

**Smell:** Duplicate Code (Fowler), micro-scale but cross-file — the copies already diverge.

**Impact:** Low. Blast radius: 2 files, ~10 sites. Frequency: every new `security`/`systemctl`/`schtasks` interaction (each WARM file added one in its last 2 PRs) re-rolls the block.

**Instances:**
- The "spawn command → match { Ok(success) info / Ok(fail) lossy-stderr warn / Err warn }" triage block: `crash_recovery.rs:186-197` (systemctl enable), `crash_recovery.rs:306-315` (schtasks /Create), `certs.rs:366-379` (`add_trusted_cert`, Err-returning variant), `certs.rs:412-422` (`remove_trusted_cert_by_sha1`). One-line map variants of the same shape: `crash_recovery.rs:331-337`, `certs.rs:384-391`, `certs.rs:397-407`.
- `std::env::current_exe()` match + warn + early-return, copied verbatim: `crash_recovery.rs:134-140` (Linux) and `crash_recovery.rs:260-266` (Windows).
- `dirs::home_dir().unwrap_or_else(...)` fallback policy ×3 with **divergent** fallback values — `certs.rs:14-15` (`"."`), `certs.rs:354-355` (`"."`), `crash_recovery.rs:124-125` (`"~"`) — direct evidence the copies rot independently.

**Why future change gets harder:** a logging-policy change (e.g. include exit code, redact stderr, structured fields) or a fallback-policy decision is an N-site sweep across two files, and the `"."`-vs-`"~"` skew shows a sweep has already been missed once.

**Smallest safe refactoring:** Extract Function ×2 — `run_logged(cmd: &Path, args: &[&str], what: &str) -> Result<Output, …>` (callers keep their own success criteria) and a shared `home_dir_or_default()`; fold the two `current_exe` matches into a tiny `exe_path_or_warn(context) -> Option<PathBuf>`.

**What disappears:** ~50 lines of boilerplate and the silent fallback divergence.

---

## Finding 7 — `check_for_update` is a query that sometimes installs and restarts the app

**Smell:** Query-with-side-effects — Fowler's Separate Query from Modifier (refactoring name; the smell is the side-effecting query).

**Impact:** Low. Blast radius: `updater.rs` + `main.rs`. Frequency: updater WARM, but this seam itself is stable since #70.

**Instances:**
- `packages/accelerator/src-tauri/src/updater.rs:14-58` — `check_for_update` reads the `auto_update` pref (43) and either *performs the full update + restart* (47-50) or returns the `Update` for prompting.
- `packages/accelerator/src-tauri/src/main.rs:144-145` — the sole caller immediately re-reads the same pref to decide prompt copy, splitting one policy decision across two files.

**Why future change gets harder:** any second caller (a "Check for updates now" tray/Settings item is the obvious future feature) inherits a hidden may-install-and-restart side effect from a function named "check"; and a third pref state (e.g. auto-download-but-prompt-to-install) must be threaded through both the modifier arm here and the prompt logic in `main.rs`.

**Smallest safe refactoring:** Separate Query from Modifier — `check(app) -> Option<Update>` (pure) and let the caller own the `match pref { auto => perform_update, _ => prompt }` dispatch in one place (`run_update_check` already half-owns it).

**What disappears:** the double pref-read and the hidden-install trap for the next caller.

---

## Out-of-scope observations

`crash_recovery.rs:124-125` falls back to a literal `PathBuf::from("~")` (never a real directory, unlike the `"."` fallback used in certs.rs) — degenerate-path behavior nit, noted for the bugs track, not quality.
