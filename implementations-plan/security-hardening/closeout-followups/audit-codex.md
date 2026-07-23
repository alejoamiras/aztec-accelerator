Reading additional input from stdin...
OpenAI Codex v0.144.6
--------
workdir: /home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-closeout-followups
model: gpt-5.6-sol
provider: openai
approval: never
sandbox: read-only
reasoning effort: xhigh
reasoning summaries: none
session id: 019f90b3-b5eb-79e1-9c3c-10170d657c86
--------
user
Audit a LIGHT-tier plan. Budget is tight — be FAST. Read ONLY these two files, nothing else:
- implementations-plan/security-hardening/closeout-followups/plan.md
- implementations-plan/security-hardening/closeout-followups/recon.md

recon.md already contains VERIFIED file:line facts from a codebase recon — TREAT THEM AS GIVEN. Do NOT open or read any other source files. Critique the DESIGN in plan.md against recon.md. Do not explore. Return within a few minutes.

Three-part critique as one packet, concise (bullet points, not prose):

1. SECURITY/ADVERSARIAL: pressure-test — (a) Windows DACL: is "current-user-only + PROTECTED_DACL via SetNamedSecurityInfoW" correct/complete (SID acquisition, LocalFree, post-create TOCTOU, symlink/junction on the tempdir, should config.rs/certs.rs CA-key be in scope)? (b) get_pending_auth: can another popup peek a DIFFERENT request's origin — is the exact-label guard airtight? (c) click-delay guard (~700ms): does it actually defeat click-steal or is full popup serialization required — any bypass (keyboard/programmatic activation)? (d) C8 rollback: is disable()+disarm a COMPLETE undo of manager.enable() on every platform; do the log-and-continue callers create a false-"rearmed" state?

2. ASSUMPTION-ATTACK: attack the plan's Assumptions (Facts/Inferences/Asks). Which Inferences are unsafe (700ms, TOCTOU-negligible, "click-delay alone suffices")? Which Asks (A1 config/certs scope, A2 failure UX) resolved wrongly? Bucket findings as Facts/Inferences/Asks.

3. IMPLEMENTATION CRITIQUE: right structure or simpler/more idiomatic? windows-sys hand-rolled FFI vs windows-acl crate? shared ACL helper vs three copies? Does it ignore recon's reuse (CrashRecoveryGuard, disable_impl, get_verified_info)? Weak/unrunnable validation gates? Is light the right tier or should it be mid?

END with an explicit verdict on its own line, EXACTLY one of:
VERDICT: approve
VERDICT: conditional approve (with conditions: ...)
VERDICT: reject (with blocking findings: ...)
codex
I’ll audit only the two specified files and treat recon’s file:line statements as authoritative.
exec
/usr/bin/zsh -lc "sed -n '1,260p' implementations-plan/security-hardening/closeout-followups/plan.md && sed -n '1,320p' implementations-plan/security-hardening/closeout-followups/recon.md" in /home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-closeout-followups
 succeeded in 0ms:
# Plan — security-hardening closeout follow-ups (light)

Close out the three tracked deferred follow-ups from the security-hardening campaign, landing them on
`security-hardening` so the final `main` integration ships them too. One branch (`sechard/closeout-followups`),
one PR into `security-hardening`, three phases.

- **Item 1 (F-003 Windows tail)** — owner-only Windows ACL on the bb prove workspace (Unix already 0o700/0o600; a no-op on Windows).
- **Item 2 (C9 popup edge cases)** — extension-scheme host validation (LOW), server-authoritative popup origin via `get_pending_auth` (LOW), click-steal guard (MED).
- **Item 3 (C8 rollback)** — autostart `enable()` returns a `Result` and rolls back its half-done changes on partial failure.

**Tier: light** (single codex audit). Rubric: security-sensitivity HIGH; novelty / blast-radius / irreversibility /
migration / external-coupling all LOW → nominally "mid", downgraded to `light` because the designs are pre-specified
by the prior C8/C9 cluster audits (`clusters/C8-*`, `clusters/C9-*`) and every change is small + additive. **Borderline
call**: the codex audit is explicitly asked to scrutinize the two trust-boundary pieces (the new `get_pending_auth`
caller-binding and the click-steal guard); if it returns High/Critical design findings, escalate to `mid`.

**eli5_mode: Artifact** (the `Artifact` tool is available; plan is publishable). Fallback `eli5.html` if unavailable.

**Base**: `security-hardening` (879e211). These build on code that exists ONLY on that branch (C2 witness perms, C8
autostart, C9 popup) — NOT on `main`. Verified via recon against the worktree tip.

---

## Architecture & Implementation

### Item 1 — Windows ACL (`packages/accelerator/core`)

- **Shape**: one new pure helper `apply_owner_only_windows_acl(path: &Path) -> std::io::Result<()>` in `core/src/bb.rs`
  (or a small `core/src/win_acl.rs` module), `#[cfg(windows)]`. Builds a DACL granting FULL control to the current
  user SID only, `PROTECTED_DACL_SECURITY_INFORMATION` to block inherited ACEs, applied via
  `SetNamedSecurityInfoW`. On non-Windows it's absent (call sites gate with `#[cfg(windows)]`).
- **Wiring**: call the helper right after directory/file creation at the three sites — `prove_tmp_parent()`
  (`bb.rs:97-98` Windows branch), `create_prove_tempdir()` (`bb.rs:107-124`, add a `#[cfg(windows)]` branch), and
  `write_witness()` (`bb.rs:129-140`, after `create_new(true)` succeeds).
- **Dep**: promote `windows-sys` from transitive to a **direct** dep in `core/Cargo.toml`, version-aligned to the
  already-resolved `0.61.x`, features `Win32_Security`, `Win32_Security_Authorization`, `Win32_Foundation`,
  `Win32_System_Memory`. NOT the `windows-acl` crate (new external dep; supply-chain caution). Hand-rolled FFI in one
  small, unsafe-audited helper.
- **Interfaces/types**: helper is `io::Result<()>`; call sites map its error into their existing error type
  (`prove()` returns `Option`/`Result` — match the current signature; a failed ACL is fail-closed = propagate, do not
  silently continue with a world-readable dir).
- **Scope decision (ASK, see Assumptions)**: the same Windows no-op exists in `config.rs` (config.json) and
  `certs.rs` (**CA private key**). Recommended: make the helper reusable and wire it into config + certs too — the CA
  key is at least as sensitive as the transient witness. Default in this plan = **cover all three**; the approval
  gate confirms or narrows to prove-workspace-only.
- **Alternative not taken**: the `windows-acl` crate (rejected: new unmaintained-ish external dep vs. one small
  hand-rolled helper over the already-present `windows-sys`).

### Item 2 — Popup edge cases (`core` + `src-tauri` + frontend)

- **(B) extension host validation** — `core/src/authorization.rs:52-58`: after lowercasing, reject any extension ID
  that isn't plain ASCII graphic (no bidi/zero-width/combining/non-ASCII). Chrome IDs are 32× `a-p`; Firefox/Safari
  are UUID-like — a `bytes().all(is_ascii_graphic)` + no-control/bidi check is zero behavior change for legit IDs.
  Pure function; extend the `canon_*` tests.
- **(C) server-authoritative origin** — add `AuthorizationManager::peek_origin(&self, request_id: &str) ->
  Option<CanonicalOrigin>` (non-consuming lookup on the existing `by_request` map, `authorization.rs:181`). Add tauri
  command `get_pending_auth(window, auth, request_id: String) -> Result<Option<String>, String>` in `commands.rs`,
  guarded by the **same** exact-label check `respond_auth` uses (`require_label(window.label(),
  &format!("{AUTH_LABEL_PREFIX}{}", sanitize_window_label(&request_id)))`) so a popup can only peek its OWN request.
  Register in `main.rs` invoke_handler; add `allow-get-pending-auth` to `capabilities/authorize.json`. `authorize.js`
  calls it on load, renders the returned origin as authoritative; on `None`/mismatch/error it disables Allow and shows
  an error hint (query param becomes non-authoritative — display only until the command answers, or dropped).
- **(A) click-steal guard** — in `frontend-src/bridge.js`, add a short input-guard: Allow/Deny ignore activation for
  ~700 ms after the popup is shown/focused (defeats "pop a window under the cursor and steal the click", the standard
  browser permission-prompt mitigation). Belongs next to `wireButton`/`wireToggle` as a shared helper.
- **Alternative not taken (A)**: full cross-origin popup serialization / queue (rejected for `light`: large windowing
  change that tangles with the `MAX_PENDING_ORIGINS=10` × 60 s auto-deny timeout math — high regression risk for a MED
  item; the click-delay guard neutralizes the click-steal vector without touching the windowing/timeout invariants).
- **Data/control flow (C)**: popup loads → `invoke("get_pending_auth", {requestId})` → manager `peek_origin` under
  lock → returns canonical origin string (or `None`) → frontend renders + enables Allow only on a real origin.

### Item 3 — Autostart rollback (`src-tauri`)

- **Shape**: `CrashRecovery::enable(&self)` → `enable(&self) -> Result<(), String>`; all three `enable_impl` bodies
  convert swallowed `tracing::warn!`+return points into `Err(...)`; `enable_crash_recovery()` →
  `Result<(), String>`. In `commands.rs::set_autostart`, after `manager.enable()` succeeds, call the now-fallible
  `enable_crash_recovery()`; on `Err`, roll back with the **exact two-liner already at `commands.rs:73-74`**
  (`let _ = manager.disable(); disable_crash_recovery();`) then return the error to the UI.
- **Reuse**: `disable_impl` cleanup is already correct (GATE-3 fold) — reuse as-is. Model the rollback's testability on
  the `CrashRecoveryGuard` RAII pattern (`updater.rs:432-463`, unit-tested with a `Cell` counter, no real
  `AppHandle`). Optionally factor a small `enable_with_rollback(enable_fn, disable_fn, arm_fn, disarm_fn)` taking
  injected closures so ordering/completeness is unit-testable on Linux CI.
- **Must-not-break**: the two OTHER callers of `enable_crash_recovery()` — `main.rs:512-518` (startup rearm) and
  `updater.rs:411-419`/`:379` (`CrashRecoveryGuard` rearm) — must **log-and-continue** on the new `Err`, never abort
  startup, never falsely mark rearmed.
- **Alternative not taken**: a full RAII transaction guard wrapping the whole `set_autostart` (rejected as heavier than
  needed — the single rollback point after `manager.enable()` is the only multi-step seam; the two-liner suffices).

### File-level change map

- `core/Cargo.toml` — add direct `windows-sys` dep (+features). **[modify]**
- `core/src/bb.rs` — new `apply_owner_only_windows_acl` helper + 3 wiring sites + `#[cfg(windows)]` DACL test. **[modify]**
- `core/src/config.rs`, `src-tauri/src/certs.rs` — wire the helper on Windows (if scope-approved). **[modify, conditional]**
- `core/src/authorization.rs` — extension-ID charset validation + `peek_origin` + tests. **[modify]**
- `src-tauri/src/commands.rs` — `get_pending_auth` command; `set_autostart` rollback. **[modify]**
- `src-tauri/src/main.rs` — register `get_pending_auth`; keep startup rearm log-and-continue. **[modify]**
- `src-tauri/src/crash_recovery.rs` — `enable → Result`, `enable_impl` error propagation + unit test. **[modify]**
- `src-tauri/src/updater.rs` — `rearm_crash_recovery_if_enabled` log-and-continue on new Result. **[modify]**
- `src-tauri/capabilities/authorize.json` — add `allow-get-pending-auth`. **[modify]**
- `frontend-src/authorize.js`, `frontend-src/bridge.js` — call `get_pending_auth` + click-delay guard. **[modify]**
- `e2e/authorize.spec.ts`, `e2e/tauri-mock.js`, `e2e-webdriver/auth-flow.spec.ts` — test updates. **[modify]**

---

## Phases

### Phase 1 — C8 autostart `enable() → Result` rollback

Steps: (1) `CrashRecovery::enable` + `enable_crash_recovery` return `Result<(),String>`; propagate real failures out of
the three `enable_impl` bodies (esp. Linux `systemctl --user enable`). (2) `set_autostart` rolls back
(`manager.disable()` + `disable_crash_recovery()`) on crash-recovery-enable `Err`, then returns the error. (3)
`main.rs` + `updater.rs` callers log-and-continue. (4) Optional `enable_with_rollback` closure helper. (5) Unit test:
inject a failing arm-closure, assert the disable/disarm rollback ran exactly once and in order (model on
`crash_recovery_guard_rearms_on_drop`).

**Assumptions**: `disable_impl` cleanup is already correct (recon: GATE-3 fold, `crash_recovery.rs:254-282`) — Fact.
The rollback two-liner at `commands.rs:73-74` is the intended shape (recon + `C8-plan.md:63-66`) — Fact. No silent Asks.

**Validation gate**
- Commands: `bun run --cwd packages/accelerator lint` · `cd packages/accelerator/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- Pass: exit 0; new rollback unit test green; no clippy warnings.
- Layers: typecheck/lint · unit (Rust).

### Phase 2 — C9 popup edge cases (B → C → A)

Steps: **(B)** extension-ID charset validation in `authorization.rs` + `canon_*` accept/reject tests. **(C)**
`peek_origin` + `get_pending_auth` command (own-label guard) + `main.rs` register + `capabilities/authorize.json`
grant + `authorize.js` sources origin from the command (Allow disabled on None/mismatch) + Playwright mock handler
(`tauri-mock.js`) + retitle the query-param test + a WebDriver assertion that the popup shows the server origin.
**(A)** click-delay guard in `bridge.js` + a WebDriver case (two distinct-origin `/prove` requests → two windows →
a click on the just-shown Allow within the guard window is ignored). Keep the same-origin piggyback + request_id 60 s
timeout intact.

**Assumptions**: `get_pending_auth` absent today (recon: not in `commands.rs`/`main.rs:487-500`/capabilities) — Fact.
`respond_auth` resolves by `request_id`, origin arg diagnostics-only (`commands.rs:177`) — Fact. Extension arm
lowercases ASCII only (`authorization.rs:52-58`) — Fact. Click-delay ms value (700) is an Inference — tune in review.
No silent Asks.

**Validation gate**
- Commands: `bun run --cwd packages/accelerator lint` · `cd packages/accelerator/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` · `cargo test --manifest-path packages/accelerator/core/Cargo.toml` · `bun run --cwd packages/accelerator test:e2e:ui` (Playwright mock)
- Pass: exit 0; new `canon_*` reject tests green; Playwright authorize specs green (incl. the new `get_pending_auth` path); WebDriver popup cases green in CI (GUI-less VPS ⇒ CI-only).
- Layers: typecheck/lint · unit (Rust) · integration (Playwright mock) · e2e (WebDriver, CI).

### Phase 3 — F-003 Windows ACL on the bb prove workspace

Steps: (1) add direct `windows-sys` dep + features to `core/Cargo.toml`. (2) `apply_owner_only_windows_acl` helper
(`#[cfg(windows)]`, one audited `unsafe` block). (3) wire into `prove_tmp_parent` / `create_prove_tempdir` /
`write_witness` (fail-closed). (4) if scope-approved, wire into `config.rs` + `certs.rs`. (5) `#[cfg(windows)] #[test]`
mirroring `prove_workspace_and_witness_have_private_modes`: create a real prove tempdir + witness, read back the
effective DACL, assert owner-only (no `BUILTIN\Users`, no `Everyone`, inheritance blocked). **Fallback per your call**:
if the effective-DACL readback test proves too fiddly on the runner, downgrade to unit-testing the descriptor
construction + asserting the helper runs without error on Windows CI, and log the downgrade in lessons.

**Assumptions**: `windows-build` runs `cargo test` on the **core** crate on windows-latest (recon:
`accelerator.yml:433-434`) — Fact, so the test runs in CI. `windows-sys 0.61.2` already resolved transitively (recon:
`core/Cargo.lock`) — Fact. Windows has no atomic create-with-ACL ⇒ a narrow post-create TOCTOU window — Inference,
documented + mitigated by applying the ACL before any content write. The config/certs scope is an explicit **Ask**
(resolved at approval), not silent.

**Validation gate**
- Commands: `cd packages/accelerator/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings` · `cargo test --manifest-path packages/accelerator/core/Cargo.toml` (Linux: compiles, non-Windows sites unaffected) · **CI windows-build lane** runs the `#[cfg(windows)]` DACL test.
- Pass: local Linux fmt/clippy/test exit 0; CI `windows-build` green with the DACL test executing (not skipped).
- Layers: typecheck/lint · unit (Rust, incl. Windows-CI).

---

## Security & Adversarial Considerations

- **Threat model**: local multi-tenant host (the user runs many agents on one box) + a hostile local process racing to
  bind `127.0.0.1:59833` or read the transient witness. Item 1 closes a local-read of the crown-jewel witness (and, if
  scoped in, the CA private key) on Windows. Item 2 hardens the human-in-the-loop authorization popup against a
  malicious dApp/origin (click-steal, homograph/bidi extension IDs, display↔decision desync). Item 3 is a correctness
  /robustness fix (no new external attack surface) — half-enabled autostart is a reliability + confused-state risk, not
  a direct exploit.
- **Least privilege**: `get_pending_auth` is grantable ONLY to auth-popup windows (`capabilities/authorize.json`) and
  self-scoped by exact request_id label — a window cannot peek another request's origin. Windows DACL grants the
  current user only; inheritance blocked (`PROTECTED_DACL`).
- **Cryptography**: none rolled. Windows ACL via OS `SetNamedSecurityInfoW` (battle-tested Win32), `windows-sys`
  0.61.x pinned to the already-resolved version. No new crypto.
- **Input validation**: extension-ID charset allowlist at the single ingress canonicalizer
  (`canonicalize_origin`) — the same trust boundary that already rejects path/query/userinfo/trailing-dot.
- **Supply chain**: one new **direct** dep edge to an already-present transitive crate (`windows-sys`), no new crate
  pulled; `bun.lock` / `Cargo.lock` committed; version-aligned to avoid a second major.
- **Domain-specific**: clickjacking (Item 2A — click-delay guard, the browser-standard mitigation); homograph/bidi
  spoofing (Item 2B); TOCTOU on the Windows ACL (Item 1 — applied before content write, narrow window, documented).

---

## Assumptions

### Facts (verified against the code at 879e211)
1. bb prove workspace perms live in `core/src/bb.rs`; Windows branches no-op (`bb.rs:97-98`, and no `#[cfg(windows)]`
   in `create_prove_tempdir`/`write_witness`). Unix template test at `bb.rs:250-272`.
2. `accelerator.yml` job `windows-build` runs `cargo test` on the **core** crate on windows-latest
   (`accelerator.yml:433-434`) → a `#[cfg(windows)]` core test executes in CI.
3. `windows-sys 0.61.2` + `windows 0.61.3` are already resolved transitively in `core` (no new crate needed to add the
   direct edge).
4. `get_pending_auth` does not exist (absent from `commands.rs`, `main.rs:487-500` invoke_handler, and
   `capabilities/authorize.json`); `respond_auth` resolves by `request_id`, origin arg diagnostics-only
   (`commands.rs:177`).
5. The extension-scheme arm at `authorization.rs:52-58` only `to_ascii_lowercase()`s the host (folds A-Z), leaving
   non-ASCII/bidi in the canonical origin; opaque-host schemes skip `url` IDNA.
6. `CrashRecovery::enable` returns `()` and all three `enable_impl` swallow errors via `tracing::warn!`
   (`crash_recovery.rs:16,62,171,321`); `disable_impl` already does remove-before-reload + confirmed-disarm
   (`:254-282`); the rollback two-liner exists at `commands.rs:73-74`; `CrashRecoveryGuard` is unit-tested with no real
   `AppHandle` (`updater.rs:469-524`).
7. `main.rs:512-518` and `updater.rs:411-419` are the other `enable_crash_recovery()` callers that must log-and-continue.

### Inferences (may be wrong — attack these)
- A 700 ms click-delay is enough to defeat click-steal without annoying users (browsers use ~500 ms) — tune in review.
- Applying the DACL immediately after create (before any write) makes the TOCTOU window negligible on a per-user
  `%LOCALAPPDATA%` path.
- The click-delay guard alone (no popup serialization) is sufficient for the MED click-steal at `light` scope.

### Asks (resolve at approval — NOT silently assumed)
- **A1 — Windows ACL scope**: cover only the prove workspace (audit's literal F-003), or also `config.rs` + `certs.rs`
  (the CA private key)? Plan default = cover all three via the shared helper. **Recommend: all three.**
- **A2 — get_pending_auth failure UX**: on `None`/mismatch, disable Allow + show an error hint (recommended), or
  auto-close the popup? Plan default = disable Allow + hint.

---

## Seeds (draft — finalized post-approval)

### /goal
```
/goal All 3 phases in implementations-plan/security-hardening/closeout-followups/plan.md marked ✓ (the per-phase headers in the file), each ✓ backed by its phase's validation gate reported passing in the transcript; for each phase the agent printed LESSONS_FILE=implementations-plan/security-hardening/closeout-followups/lessons/phase-N.md; `/code-review max --fix` complete with findings applied + committed; codex post-impl audit (-m gpt-5.6-sol -c model_reasoning_effort=xhigh) complete with high/critical findings addressed; PR into security-hardening is green (all accelerator-status + WebDriver lanes); `bun run test` and `bun run lint:actions` report exit 0 in the transcript.
```

### /loop
```
/loop 15m Drive implementations-plan/security-hardening/closeout-followups forward. Never idle. Each firing: (1) read plan.md + lessons/ (authoritative), `git status`, `git log --oneline -5`; if a PR exists, `gh pr checks` (no --watch). (2) Waiting on CI is fine — confirm it's progressing; use the wait to review the diff / strengthen tests. (3) No task in hand? Take the next pending step from plan.md; after each edit run the fast layers (cargo fmt/clippy + the touched crate's tests, or bun lint), then commit → push. (4) Stuck or a real decision? `/codex xhigh`, log the consult in lessons/phase-N.md, act on the stronger argument; never merge to main/release, never expand scope. (5) Same step failed 5×? Reassess with codex. (6) Phase green = its plan.md validation gate passes → mark ✓, file lessons, print LESSONS_FILE=..., advance. (7) All phases ✓? Run `/code-review max --fix` → commit → codex post-impl audit → address high/critical → wrap-up report + stop. Keep an ASCII checklist visible.
```

Recommended: **/loop 15m** (Windows-CI + WebDriver signals aren't fully transcript-observable turn-by-turn; interval cadence keeps it driving through CI waits). `/goal` is the alternative if you'd rather it run to a condition.
# Recon — security-hardening closeout follow-ups

Read-only codebase recon (Phase 0.4), 3 parallel Sonnet explorers, against the `security-hardening` base
(worktree tip 879e211). Feeds the plan + the codex audit. File:line anchors are the source of truth.

## Item 1 — F-003 Windows tail (bb prove workspace ACL)

**Lives in the `core` crate, not `src-tauri`.** `packages/accelerator/core/src/bb.rs`:
- `prove_tmp_parent()` (`bb.rs:82-100`): Unix branch `DirBuilder::new().mode(0o700)` + `set_permissions`; **Windows branch is a bare `create_dir_all(&base).ok()?` (`bb.rs:97-98`) — no ACL.** Comment assumes "%LOCALAPPDATA% is already per-user" (assumption, not enforced).
- `create_prove_tempdir()` (`bb.rs:107-124`): `tempfile::Builder` with `#[cfg(unix)] .permissions(0o700)` (`:110-114`); **no `#[cfg(windows)]` branch at all.**
- `write_witness()` (`bb.rs:129-140`): `OpenOptions::new().create_new(true)` with `#[cfg(unix)] .mode(0o600)` (`:133-137`); **no Windows branch.** Witness file = `ivc-inputs.msgpack`.
- Existing test `prove_workspace_and_witness_have_private_modes` (`bb.rs:250-272`), `#[cfg(unix)] #[test]`, reads `MetadataExt::mode()` → **the template to mirror with a `#[cfg(windows)]` twin that reads the effective DACL.**

**No existing Windows ACL/SID/SECURITY_DESCRIPTOR code anywhere in the Rust crates.** Every `#[cfg(windows)]` hit is behavioral (URL scheme in `windows.rs:23-30`, Task Scheduler XML in `crash_recovery.rs`). So the ACL logic is net-new; only the `#[cfg(windows)]` gating + test-shape conventions are reusable.

**Deps**: `windows-sys 0.61.2` + `windows 0.61.3` are present only **transitively** (via tempfile/tokio/tauri) in `core/Cargo.lock`. To write ACL code, add `windows-sys` as a **direct** dep to `core/Cargo.toml` with `Win32_Security`, `Win32_Security_Authorization`, `Win32_Foundation` (+ `Win32_System_Memory` for `LocalFree`) features. **`windows-acl` crate is NOT in the repo** — using it = brand-new external dep (avoid per supply-chain caution; promoting the already-resolved `windows-sys` edge is cleaner). Align to the resolved `0.61.x` to avoid Cargo.lock churn.

**CI (decisive)**: `.github/workflows/accelerator.yml` job `windows-build` (`runs-on: windows-latest`, `:413-434`) runs `cargo test` for src-tauri **AND** `cargo test --manifest-path ../core/Cargo.toml` (`:433-434`), gated on `desktop` path-filter (any `packages/accelerator/**` change). **⇒ a `#[cfg(windows)]` test in `core/src/bb.rs` runs in real CI, no workflow change needed.**

**Reuse/adapt**: extract a pure `apply_owner_only_windows_acl(path: &Path) -> io::Result<()>` (testable directly on a tempdir/tempfile, matching `config.rs`'s `save_to`/`load_from` testability split). Wire into all 3 sites. Windows has no O_CREAT-mode equivalent → ACL applied as a follow-up `SetNamedSecurityInfoW` (a narrow TOCTOU window worth calling out).

**Scope fork (flag for approval)**: `config.rs:171-174` and `certs.rs:251-252` **also** no-op perms on Windows — `certs.rs` holds the **CA private key**. Audit F-003 named only the prove workspace, but a shared helper makes covering config + certs nearly free, and leaving the CA key world-readable on Windows is a worse inconsistency. Decide intentionally.

## Item 2 — C9 authorize-popup edge cases

**(A) MED click-steal** — `src-tauri/src/windows.rs`: `show_auth_popup_window` (`:109-160`) → `open_or_focus_window` (`:61-87`) builds every popup `always_on_top(true)` + `.center()` + `set_focus()` on create. Per-origin dedup exists (`core/src/server/auth.rs:64-73`, `is_first` piggyback), but **distinct origins each get their own centered always-on-top focused window** (up to `MAX_PENDING_ORIGINS=10`, `authorization.rs:165`). No cross-origin serialization ⇒ popup #2 steals focus + identical screen position from popup #1. **No click-delay guard** in `bridge.js wireButton` (`:67-87`) — it only disables buttons AFTER a click starts. Full serialization/queue = big lift + tangles with the 10×60s timeout math (`authorization.rs:253-255`). Proportionate fix = a click-delay "ignore input for ~N ms after show/focus" guard in `bridge.js`. Testable via a new WebDriver two-origin case (`auth-flow.spec.ts`) + a Rust unit assertion; NOT testable at the Playwright mock layer (single page-per-test).

**(B) LOW extension-scheme host validation** — `core/src/authorization.rs:52-58`: the `chrome-extension|moz-extension|safari-web-extension` arm rejects a port and `to_ascii_lowercase()`s the host, but **`to_ascii_lowercase()` folds only A-Z**, leaving non-ASCII/bidi/zero-width/combining chars in the canonical string. `url::Url` treats these as opaque-host schemes ⇒ no IDNA/punycode (unlike http(s) at `:44-51`, protected per `canonical_origin_idn_punycode_no_homograph_collision`, `:625-637`). Real extension IDs are always plain ASCII (chrome: 32× `a-p`; firefox/safari: UUID-like). Fix = charset allowlist (reject non-`is_ascii_graphic`/bidi). Extend `canon_chrome_extension_*` tests (`:528-552`), `co()` idiom. Server ingress `auth.rs:36-42` calls `CanonicalOrigin::parse` ⇒ fix auto-hardens that path.

**(C) LOW get_pending_auth authoritative display** — `frontend-src/authorize.js:3-8`: origin rendered from **URL query param only** (`params.get("origin")`); no server re-check. `get_pending_auth` **does not exist** (absent from `commands.rs`, `main.rs:487-500` invoke_handler, `capabilities/authorize.json:8`). `respond_auth` (`commands.rs:149-183`) already treats the origin arg as diagnostics-only (`:177`), resolving purely by `request_id`. Needed: `AuthorizationManager::peek_origin(&self, request_id) -> Option<CanonicalOrigin>` (non-consuming lookup on `by_request` map, `authorization.rs:181`; mirrors the `get_mut` at `:247`); `get_pending_auth(window, auth, request_id) -> Result<Option<String>,String>` bound by the **same** `auth-{hash(request_id)}` label check `respond_auth` uses (`require_label`, `commands.rs:163-164`); register in `main.rs`; add `allow-get-pending-auth` to `capabilities/authorize.json`; `authorize.js` calls it and trusts its answer (disable Allow on None/mismatch). Closest reuse template = `get_verified_info` (`commands.rs:136-147`). Playwright mock (`e2e/authorize.spec.ts:25-28` "shows decoded origin from URL params" + `e2e/tauri-mock.js` defaults) needs a `get_pending_auth` handler and the query-param test retitled.

**Collision risks**: any windowing change must not break the same-origin piggyback (`is_first`) or the request_id-keyed 60s timeout (`windows.rs:140-159`); `get_pending_auth`'s label check must stay in lock-step with `respond_auth`'s (same `sanitize_window_label`) or it leaks/refuses wrongly; if popups ever serialize, revisit the 10×60s timeout DoS math.

## Item 3 — C8 autostart enable()→Result rollback

`src-tauri/src/commands.rs:56-87` `set_autostart` enable branch order: `current_exe()` → `autostart_path_is_safe` preflight (on unsafe: `manager.disable()` + `disable_crash_recovery()` + `Err`, `:73-78`) → `manager.enable()` (plugin writes systemd/.desktop/plist/Run-key, `?`-propagated) → **`enable_crash_recovery()` (returns `()`, swallows errors, `:81`)** → `Ok`.

`crash_recovery.rs`: `CrashRecovery::enable(&self)` (`:16`) returns `()`; all 3 `enable_impl` (macOS `:62-90`, Linux `:171-249`, Windows `:321-380`) only `tracing::warn!` on failure — **the residual**. Linux's actual arming step `systemctl --user enable` failure (`:237-244`) is swallowed. `disable_impl` already does remove-before-reload + confirmed-disarm bool (GATE-3 fold, `:254-282`) — **reuse as-is for rollback**.

**Fix** (already specified in `clusters/C8-plan.md:63-66,195-199` + `C8-audit-gate3-codex.md:16-23`): make `enable(&self) -> Result<(),String>` + `enable_crash_recovery() -> Result<...>`; in `commands.rs`, on `Err` from crash-recovery-enable, roll back with the **same two-liner already at `:73-74`** (`manager.disable()` + `disable_crash_recovery()`) then return the error. **Reuse `CrashRecoveryGuard` RAII (`updater.rs:432-463`)** — unit-tested with a `Cell` counter, no real `AppHandle` (`updater.rs:469-524`) — as the testable-rollback precedent; factor a rollback helper taking injected closures so ordering/completeness is unit-testable on Linux CI.

**Must NOT hard-fail** (prior GATE-3 ask): the other two `enable_crash_recovery()` callers — `main.rs:512-518` (startup rearm) and `updater.rs:411-419`/`:379` (`CrashRecoveryGuard` rearm) — must **log-and-continue**, never abort startup / never falsely mark rearmed. Error conventions: commands use `Result<(),String>`; internal fns use `Result<(),Box<dyn Error+Send+Sync>>` `.map_err`'d at the boundary. Tests colocated `#[cfg(test)]`; only pure/injectable logic (`systemd_exec_start`, `autostart_path_is_safe`, `task_xml`, `CrashRecoveryGuard`) is covered today — `enable_impl`/`set_autostart` have none. Playwright `e2e/settings.spec.ts:137-172` tests only the mocked frontend toggle.

**Out of scope (don't conflate)**: the AppImage/canonicalized-path preflight-vs-plugin mismatch (`C8-audit-gate3-codex.md` finding #1) is a separate documented deferral; `enable_safari_support` (`commands.rs:247-282`) is a sibling unrolled-back multi-step enable — flag only, don't scope-creep.

codex
## 1. SECURITY/ADVERSARIAL

- **Windows DACL**
  - Directionally correct, but incomplete. The design must specify effective-token SID acquisition, `GetTokenInformation(TokenUser)` sizing, handle cleanup, SID lifetime, Win32 error mapping, and `LocalFree` for ACL/security-descriptor allocations on every path.
  - Directories need inheritable object/container ACEs; `PROTECTED_DACL` alone does not ensure children are private at creation.
  - “Windows has no atomic create-with-ACL” is unsafe: Win32 creation APIs accept `SECURITY_ATTRIBUTES`. Path-based post-create `SetNamedSecurityInfoW` leaves a real race.
  - `SetNamedSecurityInfoW` follows names. Pre-existing or swapped symlinks/junctions/reparse points can redirect ACL application. Prefer handle-based creation/security, reject reparse points, and verify owner/DACL using the opened handle.
  - Applying the ACL before writing limits disclosure but does not eliminate unauthorized opens during creation. For config/cert files, applying it after writing would be worse.
  - Current-user-only ACLs do not isolate hostile processes running under the same SID, contradicting the “many agents under one user” portion of the threat model.
  - CA private-key protection should be mandatory scope. Config protection depends on its data classification, but should use the same helper if sensitive.

- **`get_pending_auth`**
  - The guard works for ordinary distinct IDs only if `sanitize_window_label` is collision-resistant and request IDs are never reused. Exact string comparison is not proof of request identity when the derivation is non-injective.
  - Factor one `auth_window_label(request_id)`/guard helper shared by `respond_auth` and `get_pending_auth`; duplication can drift.
  - Add adversarial tests for cross-request lookup, sanitizer collisions, removed requests, and ID reuse.
  - Never render the query origin, even temporarily. A server/query mismatch should ignore the query—not disable a valid authoritative request.

- **Click-delay**
  - A 700 ms delay mitigates only immediate pointer activation; it does not defeat popup replacement after the timer expires.
  - It must reset on every native focus/show, not merely page load. A frontend load timer cannot prove when native focus was stolen.
  - Bypasses include keyboard activation, held/repeated keys, synthetic `.click()`, and pointer-down before/reset with activation afterward. At minimum require trusted activation plus a post-guard pointer-down/keyboard-down.
  - Robust closure requires popup serialization—or an equivalent single-active-actionable-popup arbiter. Multiple centered, always-on-top popups remain a confused-deputy UI even with a timer.

- **C8 rollback**
  - `disable() + disarm` is best-effort compensation, not a proven complete undo.
  - `manager.enable()` may fail after partial mutation; that error path currently receives no rollback.
  - Ignoring both rollback results can return the original error while leaving either mechanism enabled.
  - Disabling does not restore prior state when autostart was already enabled before the operation.
  - Partial platform-specific crash-recovery writes also require verified cleanup; `disable_impl` structure does not make cleanup infallible.
  - Startup/updater log-and-continue preserves availability but can leave the system operationally “rearmed” only in logs/assumptions. Record degraded state, retry or expose it, and mark rearmed only after `Ok`.

## 2. ASSUMPTION-ATTACK

- **Facts**
  - Recon-backed locations, callers, CI lanes, and current return types are valid facts.
  - Existing `disable_impl` ordering is a fact; “complete rollback” is not.
  - A Windows test executing in CI is a fact; that test establishing effective security is not.
  - Existing exact-label use in `respond_auth` is a fact; collision-free caller binding is not.

- **Inferences**
  - **Unsafe:** 700 ms is sufficient or browser-equivalent.
  - **Unsafe:** post-create DACL TOCTOU is negligible.
  - **Unsafe:** delay alone solves cross-origin focus stealing.
  - **Hidden/unsafe:** label sanitization is collision-resistant; ASCII-graphic is an adequate extension-ID grammar; current-user ACL matches the stated same-user threat; disable/disarm restores prior state; log-and-continue cannot create false-rearmed state.
  - `is_ascii_graphic` is overly broad—it admits nearly all printable punctuation. Prefer scheme-specific extension-host grammars.

- **Asks**
  - **A1:** resolved incorrectly as conditional. Include the CA private key mandatorily; classify config explicitly and protect it if it contains secrets.
  - **A2:** one policy for `None`/mismatch/error is wrong. `None` means stale/finished and should close; query mismatch should be irrelevant because the query is ignored; transient IPC failure should keep Allow disabled while offering retry/close.

## 3. IMPLEMENTATION CRITIQUE

- `windows-sys` is preferable to adding `windows-acl` given recon’s supply-chain facts, but the FFI is not “one small helper.” Put audited SID/ACL/handle RAII in one Windows module; do not copy it across files.
- Use shared secure-create/apply-and-verify primitives with explicit file/directory inheritance behavior. A path-only helper is insufficient against reparse races.
- Make the injectable rollback transaction mandatory, not optional. Surface and combine rollback failures.
- The plan mentions `CrashRecoveryGuard` and `disable_impl`, but only as precedent. It should concretely reuse their injectable/testable structure and propagate cleanup outcomes.
- Follow `get_verified_info` for command/state/error structure and share the existing authorization-window guard.
- Validation weaknesses:
  - The DACL fallback test is unacceptable; descriptor construction and “no error” do not establish effective protection.
  - Assert the exact SID/ACE set, masks, inheritance flags, protected status, owner, files/directories, and reparse rejection.
  - WebDriver is declared CI-only without a runnable gate command and will be timing-sensitive.
  - Add deterministic fake-clock input tests plus focus-reset, keyboard, synthetic activation, and cross-request-label cases.
  - Rollback tests must cover manager failure, arm failure after partial mutation, cleanup failure, prior-enabled state, and all platform implementations.
- **Tier:** `mid`, not `light`: net-new unsafe Windows security FFI, a human authorization boundary, platform-specific transactions, and weakly testable focus behavior are not low-novelty changes.

VERDICT: reject (with blocking findings: design atomic/reparse-safe Windows ACL handling, serialize actionable authorization popups, and make autostart rollback state-restoring and failure-observable)
tokens used
21,321
## 1. SECURITY/ADVERSARIAL

- **Windows DACL**
  - Directionally correct, but incomplete. The design must specify effective-token SID acquisition, `GetTokenInformation(TokenUser)` sizing, handle cleanup, SID lifetime, Win32 error mapping, and `LocalFree` for ACL/security-descriptor allocations on every path.
  - Directories need inheritable object/container ACEs; `PROTECTED_DACL` alone does not ensure children are private at creation.
  - “Windows has no atomic create-with-ACL” is unsafe: Win32 creation APIs accept `SECURITY_ATTRIBUTES`. Path-based post-create `SetNamedSecurityInfoW` leaves a real race.
  - `SetNamedSecurityInfoW` follows names. Pre-existing or swapped symlinks/junctions/reparse points can redirect ACL application. Prefer handle-based creation/security, reject reparse points, and verify owner/DACL using the opened handle.
  - Applying the ACL before writing limits disclosure but does not eliminate unauthorized opens during creation. For config/cert files, applying it after writing would be worse.
  - Current-user-only ACLs do not isolate hostile processes running under the same SID, contradicting the “many agents under one user” portion of the threat model.
  - CA private-key protection should be mandatory scope. Config protection depends on its data classification, but should use the same helper if sensitive.

- **`get_pending_auth`**
  - The guard works for ordinary distinct IDs only if `sanitize_window_label` is collision-resistant and request IDs are never reused. Exact string comparison is not proof of request identity when the derivation is non-injective.
  - Factor one `auth_window_label(request_id)`/guard helper shared by `respond_auth` and `get_pending_auth`; duplication can drift.
  - Add adversarial tests for cross-request lookup, sanitizer collisions, removed requests, and ID reuse.
  - Never render the query origin, even temporarily. A server/query mismatch should ignore the query—not disable a valid authoritative request.

- **Click-delay**
  - A 700 ms delay mitigates only immediate pointer activation; it does not defeat popup replacement after the timer expires.
  - It must reset on every native focus/show, not merely page load. A frontend load timer cannot prove when native focus was stolen.
  - Bypasses include keyboard activation, held/repeated keys, synthetic `.click()`, and pointer-down before/reset with activation afterward. At minimum require trusted activation plus a post-guard pointer-down/keyboard-down.
  - Robust closure requires popup serialization—or an equivalent single-active-actionable-popup arbiter. Multiple centered, always-on-top popups remain a confused-deputy UI even with a timer.

- **C8 rollback**
  - `disable() + disarm` is best-effort compensation, not a proven complete undo.
  - `manager.enable()` may fail after partial mutation; that error path currently receives no rollback.
  - Ignoring both rollback results can return the original error while leaving either mechanism enabled.
  - Disabling does not restore prior state when autostart was already enabled before the operation.
  - Partial platform-specific crash-recovery writes also require verified cleanup; `disable_impl` structure does not make cleanup infallible.
  - Startup/updater log-and-continue preserves availability but can leave the system operationally “rearmed” only in logs/assumptions. Record degraded state, retry or expose it, and mark rearmed only after `Ok`.

## 2. ASSUMPTION-ATTACK

- **Facts**
  - Recon-backed locations, callers, CI lanes, and current return types are valid facts.
  - Existing `disable_impl` ordering is a fact; “complete rollback” is not.
  - A Windows test executing in CI is a fact; that test establishing effective security is not.
  - Existing exact-label use in `respond_auth` is a fact; collision-free caller binding is not.

- **Inferences**
  - **Unsafe:** 700 ms is sufficient or browser-equivalent.
  - **Unsafe:** post-create DACL TOCTOU is negligible.
  - **Unsafe:** delay alone solves cross-origin focus stealing.
  - **Hidden/unsafe:** label sanitization is collision-resistant; ASCII-graphic is an adequate extension-ID grammar; current-user ACL matches the stated same-user threat; disable/disarm restores prior state; log-and-continue cannot create false-rearmed state.
  - `is_ascii_graphic` is overly broad—it admits nearly all printable punctuation. Prefer scheme-specific extension-host grammars.

- **Asks**
  - **A1:** resolved incorrectly as conditional. Include the CA private key mandatorily; classify config explicitly and protect it if it contains secrets.
  - **A2:** one policy for `None`/mismatch/error is wrong. `None` means stale/finished and should close; query mismatch should be irrelevant because the query is ignored; transient IPC failure should keep Allow disabled while offering retry/close.

## 3. IMPLEMENTATION CRITIQUE

- `windows-sys` is preferable to adding `windows-acl` given recon’s supply-chain facts, but the FFI is not “one small helper.” Put audited SID/ACL/handle RAII in one Windows module; do not copy it across files.
- Use shared secure-create/apply-and-verify primitives with explicit file/directory inheritance behavior. A path-only helper is insufficient against reparse races.
- Make the injectable rollback transaction mandatory, not optional. Surface and combine rollback failures.
- The plan mentions `CrashRecoveryGuard` and `disable_impl`, but only as precedent. It should concretely reuse their injectable/testable structure and propagate cleanup outcomes.
- Follow `get_verified_info` for command/state/error structure and share the existing authorization-window guard.
- Validation weaknesses:
  - The DACL fallback test is unacceptable; descriptor construction and “no error” do not establish effective protection.
  - Assert the exact SID/ACE set, masks, inheritance flags, protected status, owner, files/directories, and reparse rejection.
  - WebDriver is declared CI-only without a runnable gate command and will be timing-sensitive.
  - Add deterministic fake-clock input tests plus focus-reset, keyboard, synthetic activation, and cross-request-label cases.
  - Rollback tests must cover manager failure, arm failure after partial mutation, cleanup failure, prior-enabled state, and all platform implementations.
- **Tier:** `mid`, not `light`: net-new unsafe Windows security FFI, a human authorization boundary, platform-specific transactions, and weakly testable focus behavior are not low-novelty changes.

VERDICT: reject (with blocking findings: design atomic/reparse-safe Windows ACL handling, serialize actionable authorization popups, and make autostart rollback state-restoring and failure-observable)
CODEX_EXIT=0

==============================================================
## FINAL FRESH-CONTEXT CODEX PASS on plan v2 (verdict: REJECT)
==============================================================
codex
I’ll audit only the two named artifacts, treating recon facts as authoritative, and check D1–D13 plus the requested invariants.
exec
/usr/bin/zsh -lc "sed -n '1,260p' implementations-plan/security-hardening/closeout-followups/plan.md && sed -n '1,260p' implementations-plan/security-hardening/closeout-followups/recon.md" in /home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-closeout-followups
 succeeded in 0ms:
# Plan — security-hardening closeout follow-ups (MID, v2)

Close out the three tracked deferred follow-ups from the security-hardening campaign, landing them on
`security-hardening` (one branch `sechard/closeout-followups` → one PR → three phases) so the final `main`
integration ships them.

- **Item 1 (F-003 Windows tail)** — owner-only Windows ACL on the bb prove workspace **+ the leaf TLS key + config.json** (Unix already 0o700/0o600; a no-op on Windows).
- **Item 2 (C9 popup)** — extension-scheme host validation (LOW), server-authoritative popup origin via `get_pending_auth` (LOW), and a click-steal guard **+ single-active-popup arbiter** (MED).
- **Item 3 (C8)** — autostart `enable()` returns a `Result` and rolls back its half-done changes on partial failure, **failure-observably**.

**Tier: MID** (escalated from light by user after the dual audit). Rubric: security-sensitivity HIGH **and** Item 1 is
net-new `unsafe` Win32 security FFI (novelty MED) → both codex + opus recommended MID. The dual audit
(`audit-codex.md` = codex REJECT-v1; `audit-fable.md` = opus conditional-v1) is folded below; a final fresh-codex
pass on THIS v2 is the remaining gate.

**eli5_mode: Artifact** (Artifact tool available; publishable). Fallback `eli5.html`.

**Base**: `security-hardening` (879e211). These build on code that exists ONLY on that branch.

---

## Decisions folded from the dual audit (v1 → v2)

| # | v1 said | Audit finding (codex + opus) | v2 decision |
|---|---|---|---|
| D1 | "certs.rs holds the **CA private key**" | FALSE — keyless CA (`certs.rs:139-160,187-226`); the no-op file is the **leaf TLS key** `localhost.key` (`certs.rs:160,251-252`) | Corrected. A1 re-grounded on leaf-TLS-key (localhost-impersonation) + config integrity. |
| D2 | "Windows has no atomic create-with-DACL" (TOCTOU negligible) | FALSE — `CreateDirectoryW`/`CreateFileW` + `SECURITY_ATTRIBUTES` is atomic | **Atomic create with a self-relative SD** where we own creation; handle-based + reparse-reject where we don't. TOCTOU eliminated, not "negligible". |
| D3 | ACL on file "after create_new succeeds" | Bypass-traverse: parent ACL doesn't gate a deep file; must protect the file itself, empty, pre-write | ACL bound to the object at creation (SA) / to an open handle before any `write_all`. |
| D4 | Blanket "propagate" error | `prove_tmp_parent` `.ok()?` → silent fallback to unhardened `%TEMP%` | **Per-site fail-closed**: Windows ACL failure aborts the prove; NO `%TEMP%` fallback. |
| D5 | "one small unsafe helper" | FFI is not small; SID/ACL/handle hygiene is the risk | One audited `win_acl` module with RAII wrappers; hard-gated by the final codex FFI pass. |
| D6 | ACL scope = "cover all three (CA key framing)" | Scope OK, framing wrong | **prove workspace + leaf TLS key + config.json** (user-confirmed), re-grounded severity. |
| D7 | extension host: `is_ascii_graphic` allowlist | Too broad (admits punctuation) | **Scheme-specific grammar**: chrome/edge = 32× `a-p`; moz/safari = UUID (8-4-4-4-12 hex). |
| D8 | get_pending_auth + still render query origin | Never render query origin even momentarily; `get_verified_info` still query-keyed | Placeholder until `get_pending_auth` answers; route `get_verified_info` through the server origin; shared `auth_window_label` guard helper. |
| D9 | click-delay guard ALONE (MED) | Mitigates not defeats; A coupled to C; codex wants serialization | **Guard + single-active-popup arbiter** (user-confirmed): exactly one popup actionable+topmost at a time. |
| D10 | A2: one policy for None/mismatch/error | Wrong — three cases differ | `None`→close (stale/resolved); mismatch→moot (query ignored); transient IPC error→Allow stays disabled + retry/close. |
