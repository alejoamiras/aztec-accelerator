# Fable Plan-subagent audit — quality-fixes-2026-06-10

**Verdict: conditional approve** (conditions: F-10 re-arm-before-restart not defuse; F-03 keep host.rs byte-identical + pin text/plain; D-3 Result-helper with per-site disposition; F-06 3-state transition; F-01 enumerate launch-vs-settings divergences).

## Security (blocking-level)
- **F-10 `defuse()`-on-restart inverts a security property.** `perform_update` re-arms crash recovery BEFORE `app.restart()` on success (updater.rs:163-171). A guard that defuses on the restart path → failed relaunch leaves recovery disarmed with autostart on. Guard's drop must reproduce the CONDITIONAL re-arm (`rearm_crash_recovery_if_enabled` checks autolaunch). RAII adds rearm-on-panic (new behavior — acceptable, NOT "behavior-preserving"). **Plan's "crash-recovery tests pin this ordering" is FALSE** — tests cover only task_xml/plist + size_from_feed; #[cfg(windows)] → silent regression.
- **F-03 changes the host-guard wire shape.** host.rs:69-72 = `axum::Json` (application/json) `{"error":"invalid_host"}` NO message; the 11 sites = text/plain JSON strings WITH message (pinned `prove_error_responses_stay_text_plain_json_string` server.rs:662-695). Host-guard test pins only status+substring → folding host.rs in silently changes body+content-type. Naive `axum::Json` IntoResponse breaks SDK ky `err.data`. → delegate IntoResponse to `(StatusCode, String)`; exclude host.rs.
- **D-3 unconditional-propagate is an availability regression on SEC-04**: auth.rs:117-126 a remembered-Allow whose save fails currently warns + returns Ok (prove proceeds); propagate → request failure after user clicked Allow. False dichotomy: Result-helper + per-site disposition = zero behavior change, no on_error param needed.
- **F-01 plan text = settings sequence, not launch.** Launch never generates, only verifies trust (NO Keychain prompt — deliberately off startup, main.rs:86-93), resets safari_support on missing-certs/load-fail, skips-without-reset on untrusted, spawns background renewal. Settings generates + installs-trust (prompt) + saves safari_support=true BETWEEN trust and load. mode enum carries ≥4 divergences; settings error strings (commands.rs:162-177) not test-pinned. SEC-08 migrate-first correctly preserved.
- **F-08 canonicalization SAFE** (request() passes already-parsed origin; Borrow<str>/Display round-trip exact; SEC-06 untouched), BUT `remove_approved_origin` (commands.rs:64-72) takes raw UI String — today non-canonical = silent no-op; parse-or-Err changes the contract → parse-or-no-op preserves. AND `"unknown"`→Option is NOT mechanical: SDK keys on literal (prover.ts:302), `AztecVersion::parse("unknown")` succeeds, cleanup runs eviction with bundled="unknown" → Option::None changes eviction; keep /health emitting "unknown" byte-identical.

## Assumptions
- Facts verified; minor: "30 prover tests" → 27 (+10 transport +4 contract). F-14 third instance (canonicalize_origin) is a PHANTOM — no loopback set there; 2-site consolidation, and the 2 DIFFER (`::1` vs `[::1]`) → D-4 surface-don't-unify WILL trigger.
- Inferences: F-03-exact-preservation unsafe as written (host.rs); F-12 not purely mechanical (shared fixture `auth_state_with_popup` server.rs:935 + router-level tests span submodules → needs shared #[cfg(test)] support module; Rust submodule privacy moots the private-item risk); F-08-round-trip true EXCEPT the sentinel sub-item.
- F-04 hazards: `status` cloned (:362-363) before move into on_versions_changed closure (:370-384) — clone-ordering must survive; `app.manage::<SharedAppState>` (:433) must stay before webdriver settings-window + HTTP spawn — phase reorder breaks E2E at runtime.

## Plan-soundness
- Main > Alt B (confirmed). PR-2 9-findings = accepted mitigated risk; **the PR-2 gate must run the Windows/macOS CI legs** (F-10/parts-of-F-01 are #[cfg]-gated → green local one-OS test doesn't exercise them).
- F-12→F-03 correct. PR-3-after-PR-2 correct direction BUT D-5 misses: F-09 (PR-2) and F-08-CanonicalOrigin (PR-3) BOTH rewrite authorization.rs:172-241 → decide D-2 with F-08 in view (drop-by_origin makes the thread-through item vanish; PendingAuthorizations with String keys means PR-3 re-keys → key by CanonicalOrigin from the start).
- F-08 `ResolvedVersion` collapse borrow hazard: `to_download` moved into download arm while version still needed for bb::prove (prove.rs:156-200) → as_ref/as_deref care.

## Missing
- F-06 "derive pin from discriminant" CANNOT reproduce behavior: `!response.ok` LEAVES pinned protocol (prover.ts:245-253); malformed JSON CLEARS (:268); tests pin only no-pin-on-!ok → unified clear passes CI but changes which endpoint /prove hits. commitStatus needs 3 states (set/clear/keep).
- F-13 helper: auth.rs saves only when !contains → lock-mutate-ALWAYS-save adds redundant write on piggyback-Allow → closure returns bool (changed) or site keeps conditional.
- Success criterion omits headless `server` crate → add to gate.
- **F-numbering collision**: codebase already has (F-01)/(F-03)/(F-08) from earlier plans; public-contract.test.ts is "F-05 doc-sync guard" → use `q7e3-F-NN` prefix in code comments/commits.
- F-01 "one CORE function" WRONG — certs.rs is in src-tauri (rcgen + macOS security CLI). F-05/F-11: #fallbackToWasm/#proveLocally already exist (Q5) — don't shadow.
