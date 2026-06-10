conditional approve (conditions: keep `invalid_host` out of the `{error,message}` unification; make F-01 mode-aware instead of one shared `migrate→generate→trust→load` path; do not adopt D-3 propagate-by-default; remove F-10 `defuse()` on success/restart)

**Security**
- [plan.md:60](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:60) + [host.rs:49](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server/host.rs:49) + [server.rs:312](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server.rs:312): F-03 cannot both “fold in” `host.rs` and preserve behavior. `invalid_host` is currently a deliberately minimal `{"error":"invalid_host"}` `application/json` body; `/prove` errors are `text/plain` JSON strings with `{error,message}`. Unifying them would silently change the SEC-01 failure surface.
- [plan.md:64](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:64) + [main.rs:55](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:55) + [commands.rs:157](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:157): F-01 is oversimplified. Startup intentionally does not trust-prompt or regenerate; Settings enable does. A single shared `migrate→generate→trust→load` flow risks prompting on launch or changing the fail-closed/reset behavior around SEC-08.
- [plan.md:74](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:74) + [updater.rs:164](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:164): `defuse()` on the restart path is backwards. Current code re-arms before `app.restart()` so a failed relaunch does not leave crash recovery off.

**Assumptions**
- Facts
- [plan.md:172](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:172) omits a crucial current fact: `ProveError` already exists as a tuple alias at [server.rs:312](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server.rs:312), and `prove`/`auth` already use it. F-03 is not “introduce a type”; it is “replace an existing wire representation.”
- [plan.md:186](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:186) overstates source-level pinning for F-10. In `src`, I do not see updater/crash-recovery characterization tests, only comments and smoke infrastructure.
- Inferences
- [plan.md:179](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:179): unsafe. Exact status/body preservation fails if `host.rs` is included.
- [plan.md:180](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:180): unsafe. F-12 is not purely mechanical; cross-cutting router/security tests in [server.rs:669](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server.rs:669) and [server.rs:1112](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server.rs:1112) span multiple submodules.
- [plan.md:181](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:181): unsafe. The `"unknown"` sentinel at [server.rs:37](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server.rs:37) is consumed by the SDK at [accelerator-prover.ts:302](/Users/alejoamiras/Projects/aztec-accelerator/packages/sdk/src/lib/accelerator-prover.ts:302); changing that contract is not a pure internal round-trip.
- Asks
- [plan.md:103](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:103) + [commands.rs:64](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:64): if `remove_approved_origin` becomes typed, should non-canonical equivalents start deleting canonical entries, or remain a no-op?

**Plan-soundness**
- Main outline is better than Alt B for the SDK hotspot and for keeping F-03 away from F-08, but PR-2 still has single-failure-blocks-all risk: F-01/F-10/F-13 can stall F-12/F-15.
- F-12 → F-03 is fine for diff hygiene; just do not force all integration tests out of `server.rs`.
- D-3 is not safe as stated. Shared helper: yes. Shared error policy: no.

**Missing**
- Add exact tests for `invalid_host` body/content-type before F-03, launch-vs-settings HTTPS behavior before F-01, and rearm-before-restart before F-10.
- F-13’s smallest safe refactor is shared lock/mutate/save with per-call `on_error`, not one unified propagation policy.

**Looks sound**
- [plan.md:33](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:33): PR-1 keeps the SDK hotspot together.
- [plan.md:92](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:92): F-07 and F-08 in the same PR is the right overlap boundary.
- [plan.md:147](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/quality-fixes-2026-06-10/plan.md:147): F-14 only if semantics match is the right call.