# Research — desktop-shell (main/tray/updater/windows/commands/crash_recovery/certs) · Q1-consumer, Q4, Q6, Q7, Q9, Q10-consumer, Q15 + minors

## Call graph / surface
- **Q4 crash_recovery**: enable callers main.rs:276, commands.rs:36, updater.rs:128; disable callers main.rs:294 (Win-gated quit), commands.rs:39, updater.rs:92 (**gates install on Windows bool return**),99,109,117 (re-arm). Signatures: mac/linux enable/disable→(), **Windows disable→bool**.
- **Q6 update flow**: PendingUpdate state commands.rs:17; poll/store main.rs:150-170; window windows.rs:88; user-response commands.rs:206-255; policy/install updater.rs:14-131.
- **Q7 safari/https**: startup main.rs:54-101 (try_start_https → reset_safari_support recovery on cert/trust fail); settings commands.rs:134-165 (**no recovery path** — divergence).
- **Q9 config mutate**: commands.rs:49,60,147,171,195 (propagate via `?`) + **:219 respond_update_prompt SWALLOWS** (`if let Err(e)=save{warn!}`).
- **Q10 tray**: is_animating main.rs:306; trigger main.rs:356 `text.contains("Proving")||contains("Downloading")`.
- **Q15**: windows.rs:72 + server.rs:361. **Q1-consumer**: AppState clone-stutter main.rs:83/376/397 (https_port patched on 2 clones).

## Invariants (SAFETY-CRITICAL — #96/#97 hardened)
- **Crash-recovery disarm-before-install** (updater.rs:61-120): download → Win disable (query-confirm gone or abort+rearm) → install() mutates files → success: rearm BEFORE app.restart → failure: rearm. **Breaking the order = live race (tick spawns exe mid-NSIS).** Win uses disable()→bool to gate install.
- Safari fail-closed: try_start_https Some ⟹ HTTPS live + cert valid+trusted; recovery = reset_safari_support on certs_exist false / is_ca_trusted false / load_rustls_config err. **commands enable path omits recovery.**
- Config persistence: save() succeeds or command returns error (5 sites); :219 is the lone swallow.
- Update prompt: check returns Once/None; respond consumes exactly once (pending.take()).

## Tests
crash_recovery.rs: task_xml repeating-trigger+escape, patch_plist insert/nested/invalid. certs.rs: certs_dir, generate CA+leaf (no ca.key), rustls load, write_pem 0o600, no-ca-key invariant, migrate-legacy. main.rs: should_prevent_exit ×3. E2E: update-prompt.spec (version/Update/Later/checkbox/error/missing-param), auth-flow.spec, smoke.spec.
**GAPS:** disarm-before-install ordering (NO test — exercised only by updater-smoke CI); Win 3-attempt /Query retry; rearm idempotency; is_ca_trusted macOS; try_start_https recovery branches; Safari enable/disable; respond_update_prompt save divergence.

## Safe seams
- **Q4** `trait CrashRecovery{enable(&self); disable(&self)->bool}` + per-platform ZSTs; mac/linux disable always `true` (behavior-neutral); Win keeps query-gated bool. Free fns become thin dispatch.
- **Q6** `UpdateCoordinator{pending,config}` owns store/take/transition — isolate state machine from UI/commands. **Must preserve check→store→prompt→take→install→rearm order.**
- **Q7** `SafariSupportManager{try_start_on_launch, enable_from_settings, disable_from_settings}` — both paths call same reset_safari_support on cert/trust fail (fixes the divergence).
- **Q9** `mutate_config(&ConfigState, impl FnOnce(&mut Cfg))->Result<(),String>` (always propagates). The :219 swallow → make EXPLICIT: either `let _ = mutate_config(...)` (keep behavior) or fix to propagate (DELIBERATE behavior change, separate PR + flag).
- minors: 3 window-open helpers → `open_or_focus_window(WindowConfig)`; home-dir fallback `"."`vs`"~"` inconsistency → shared `home_dir()`; write_pem double-0o600; certs test dup w/ stale consts 3650/825.

## Behavior-change risks
- **Q6/Q7 reordering the crash-recovery/updater lifecycle = HIGHEST risk** (safety net; only updater-smoke CI catches a break). Extract as pure move; add a sequence test; lean on updater-smoke gate.
- Q9 :219 swallow-fix = user-visible behavior change (flag separately).
- Q4 unify disable→bool = neutral (mac/linux always true) — test confirms.
- All Tauri command signatures UNCHANGED → WebDriver E2E unaffected.
