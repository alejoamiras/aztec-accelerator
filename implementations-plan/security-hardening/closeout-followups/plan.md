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

**eli5_mode: Artifact** (Artifact tool available; publishable). Fallback `eli5.html`. **Artifact URL:**
https://claude.ai/code/artifact/93aac53d-8d91-4c35-bbcb-52f5adc3e4ca (source: `eli5.html` — redeploy the same file
path to keep this URL).

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
| D11 | "convert every warn!+return → Err" | Would map idempotent macOS already-armed **success** to Err → roll back a working enable | **Enumerate** error-vs-success exits per `enable_impl`; only true failures become `Err`. |
| D12 | rollback `let _ = manager.disable()` | Swallows a rollback failure → autostart left ON silently | Rollback **surfaces + combines** disable/disarm failures; error states "enable failed AND rollback incomplete". |
| D13 | rollback helper "optional" | Make it mandatory + injectable + tested for all failure modes | **Mandatory** injectable transaction helper; unit-tested (manager-fail, arm-fail-after-partial, cleanup-fail, prior-enabled, per-platform). |

**Final-pass folds (v2 → v2-final):** a fresh-context reviewer (opus, no prior context) re-audited v2 and returned
**conditional approve (4 conditions)** — all fail-safe / test-rigor gaps, no falsely-Allow hole. Folded: **D14** arbiter
release+promote on all termination paths incl. a window-close listener + build-failure release (else a queued popup is
orphaned); **D15** `get_pending_auth` returns `{ origin, active }` so queued-button-disable is a real server signal, not
cosmetic; **D16** C8 enumeration explicitly classifies macOS plist-unreadable (`:83-88`) + patch-failure (`:78-80`) +
analogs as `Err`; **D17** Phase-3 effective-DACL readback + reparse-rejection is THE gate — the construction-only
downgrade is no longer pre-authorized (needs explicit re-approval). Config temp-file-then-rename + FAT/exFAT no-op notes
folded into Item 1.

**Audit verdicts** (transcripts: `audit-codex.md`, `audit-fable.md`): v1 codex **reject** + opus **conditional approve
(7)** → all folded (D1–D13) + tier escalated to MID. v2 fresh-pass opus **conditional approve (4)** → folded (D14–D17).
v2 fresh-pass **codex reject** — 3 NEW real findings the opus legs missed → **folded into v3**: **D18** popup timer
armed on ACTIVATION not enqueue (kills the deterministic auth-starvation DoS); **D19** arbiter enforced SERVER-SIDE
(`respond_auth` rejects a non-active request; resolve+promote atomic under lock) — not frontend-only; **D20** rollback
helper wraps the FULL transaction (incl. `manager.enable`) + snapshot/restore prior state + both cleanups run even if
one fails; **D21** ACL atomic self-creation for the tempdir + runtime effective-DACL readback (fail-closed on FAT/exFAT
silent no-op) + effective-DACL tests for every final artifact + full per-exit C8 enumeration. Cross-family value
confirmed: codex caught what two opus passes did not. (Codex's recurring infra-kills forced constrained/stdin retries;
each verdict here is a completed run.) **v3 narrow re-verify (codex): D19/D20/D21 confirmed resolved ("Yes"), D18's
operative section confirmed correct — sole blocker was a STALE "Alternative not taken" sentence contradicting D18's
adopted activation-timer; rewritten. In substance a conditional-approve on one documentary fix (now applied). Trajectory
across 3 codex passes: big design gaps (v1) → 3 real bugs (v2) → 1 stale sentence (v3) — converged.**

**Honest threat-model note (both auditors):** the Windows ACL gives cross-Windows-user isolation + strips inherited
group ACEs (PROTECTED). It gives **zero** isolation between agents under the SAME Windows user — same as Unix
`0o700`. Recorded in Security section; Item 1 is defense-in-depth parity with Unix, not a same-user silver bullet.

---

## Architecture & Implementation

### Item 1 — Windows ACL (`core` + `src-tauri/certs.rs`)

- **New module `core/src/win_acl.rs`** (`#[cfg(windows)]`), audited `unsafe`, with RAII wrappers:
  - `current_user_sid()` — `OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY)` → two-call
    `GetTokenInformation(TokenUser)` sizing → own the token-info buffer (SID aliases INTO it — never `LocalFree` the
    SID separately) → `CloseHandle` the token. Needs `Win32_System_Threading` feature (for
    `GetCurrentProcess`/`OpenProcessToken`) in addition to `Win32_Security`, `Win32_Security_Authorization`,
    `Win32_Foundation`, `Win32_System_Memory`.
  - `owner_only_sd(sid, inheritable: bool)` — `SetEntriesInAclW` with ONE `EXPLICIT_ACCESS_W` (FULL control, current
    user; `CONTAINER_INHERIT_ACE|OBJECT_INHERIT_ACE` when `inheritable` so children are private at creation) →
    build a self-relative SD with the DACL set `PROTECTED` (strip inherited ACEs). RAII guard `LocalFree`s the
    returned `PACL` **exactly once on every path**.
  - `secure_create_dir(path)` / `secure_create_file(path)` — call `CreateDirectoryW` / `CreateFileW` with
    `lpSecurityAttributes` pointing at the SD → **atomic** owner-only creation, zero TOCTOU. `CreateFileW` uses
    `FILE_FLAG_OPEN_REPARSE_POINT` semantics / `CREATE_NEW` to refuse a pre-planted reparse point.
  - `harden_existing(path)` fallback — for a path created by `tempfile`/`std` we don't control: open a handle with
    reparse-reject, verify it's not a reparse point, then `SetSecurityInfo(handle, …)` (HANDLE-based, does NOT
    follow names — defeats junction/symlink redirection that `SetNamedSecurityInfoW` would follow).
  - `verify_owner_only(handle)` — **runtime readback (v3 codex-final cond. 6)**: after applying the SD, read the
    effective DACL back off the OPEN HANDLE (`GetSecurityInfo`) and assert owner-only; if it doesn't match, **fail
    closed** (abort the operation). This catches the FAT/exFAT / network-FS case where ACL calls **silently no-op** —
    "no error returned" is NOT proof the ACL applied. Every wired site runs this before proceeding.
- **Wiring** (`#[cfg(windows)]` branches, fail-closed — an ACL error PROPAGATES, never silently degrades):
  - `bb.rs prove_tmp_parent` (`:97-98`) → `secure_create_dir`; **remove the `%TEMP%` fallback on the Windows ACL
    path** (D4).
  - `bb.rs create_prove_tempdir` (`:107-124`) → **atomic self-creation (v3 codex-final cond. 5)**: on Windows create
    the dir OURSELVES via `secure_create_dir` + a random suffix — do NOT use `tempfile` + post-hoc `harden_existing`
    (which contradicts "zero TOCTOU"). `harden_existing` remains only for paths we can't create ourselves, and
    reparse-rejecting.
  - `bb.rs write_witness` (`:129-140`) → `secure_create_file` for `ivc-inputs.msgpack` (atomic), then `write_all`.
  - `certs.rs write_pem_file` / `localhost.key` (`:160,251-252`) → `secure_create_file` on Windows.
  - `config.rs` save (`:172`) → `secure_create_dir` for the parent + `secure_create_file` for the **temp file** that
    config writes-then-renames (the SD travels with a same-volume rename), not the final `config.json` path
    (final-pass note). `data_local_dir` is NTFS in practice; note SA/`SetSecurityInfo` silently no-op on FAT/exFAT.
- **Alternative not taken**: the `windows-acl` crate (new external, ~unmaintained dep) vs. hand-rolled `windows-sys`
  (already transitive; one audited module). Chosen: `windows-sys`.

### Item 2 — Popup (`core` + `src-tauri` + frontend)

- **(A) guard + single-active-popup arbiter**:
  - *Arbiter* (**authoritative state in `AuthorizationManager`**, under its lock): tracks the ONE `request_id` that
    currently owns the actionable + `always_on_top` slot, FIFO. `show_auth_popup_window`: if no active popup, this
    becomes active (`always_on_top(true)`, focused, guard-armed); if one is active, the new popup is built
    **not-topmost, not-focused, buttons disabled** and enqueued. Preserve the same-origin piggyback (`auth.rs is_first`).
  - **Timer-on-activation, not enqueue (v3 codex-final cond. 1 — starvation DoS)** — the per-request 60 s auto-deny is
    armed **when the request is PROMOTED to active**, NOT at enqueue. With enqueue-timers, an active popup consuming
    ~60 s leaves every queued popup ~zero actionable time (deterministic auth-starvation; the 10-cap does not prevent
    it). Activation-armed timers give each request its full 60 s actionable window. **Accepted bound**: worst-case a
    queued request waits `position × 60 s` before its window (≤ 10×60 s via `MAX_PENDING_ORIGINS=10`) — long, but never
    starved of actionable time. Documented as the queue policy.
  - **Server-side active-ownership (v3 codex-final cond. 2 — arbiter must not be frontend-only)** — `{ active }`
    disabling webview buttons is NOT the enforcement. The manager holds the authoritative active `request_id`;
    `respond_auth` **rejects a resolve from a non-active request** (returns an error, no state change); resolve +
    slot-release + promote-next happen **atomically under the manager lock** (no interleaving that could hand two
    popups the active slot). The `{ active }` flag is merely the frontend reflection of this server truth.
  - **Release+promote on ALL active-popup termination paths (final-pass cond. 1)** — the slot must be released and the
    next queued request promoted (topmost + focus + arm guard + enable buttons) on: (i) button resolve
    (`respond_auth`), (ii) the 60 s auto-deny task (`windows.rs:150-159`), **and (iii) user-closes-window without
    responding** — which is NOT a resolution event today (`windows.rs:141-145`), so add a `WindowEvent::CloseRequested`/
    destroyed listener that resolves-as-Deny + releases the slot. Also release the slot if `open_or_focus_window`'s
    build fails. Without all four, a queued legitimate popup is orphaned until its own 60 s deny (fail-safe, but a
    regression vs. today's independent windows). Test: a timing-out AND a user-closed active popup each promote the
    queued one.
  - **Server-authoritative active/queued signal (final-pass cond. 2)** — the webview cannot know it's queued from a
    time-based guard alone, so `get_pending_auth` returns `{ origin, active }` (not bare origin); `authorize.js`
    disables Allow/Deny while `active=false` and a lightweight poll / promote event re-enables on promotion. Without
    this the "buttons disabled" is cosmetic (the topmost/focus arbiter defense still holds regardless).
  - *Click-delay guard* in shared `frontend-src/bridge.js`: gate **activation at entry** in `wireButton` (covers
    keyboard Enter/Space, not post-hoc `disabled`), ignoring activation for ~700 ms after the popup becomes active
    — **reset on every native focus/show**, not page load. Require a trusted activation. Documented residual:
    click-frenzy timed re-click (mitigated, not eliminated).
- **(B) extension grammar** — `core/src/authorization.rs:52-58`: per-scheme grammar (D7). Reject non-conforming IDs.
  Pure fn; extend `canon_*` tests with reject cases (bidi, zero-width, punctuation, wrong length).
- **(C) server-authoritative origin** — `AuthorizationManager::peek_origin(&self, request_id) ->
  Option<CanonicalOrigin>` (non-consuming). `get_pending_auth(window, auth, request_id) ->
  Result<Option<PendingAuthDto>, String>` where `PendingAuthDto { origin: String, active: bool }` (the `active` flag
  serves final-pass cond. 2), guarded by a **shared** `auth_window_label(request_id)` helper reused by `respond_auth`
  (D8) — a popup can only peek its OWN request. Register in `main.rs`; `allow-get-pending-auth` in `capabilities/authorize.json`.
  `authorize.js`: render a placeholder until `get_pending_auth` answers; on a real origin, render it + feed it to
  `get_verified_info`; on `None` close the popup, on IPC error keep Allow disabled + offer retry/close (D10). Never
  render the query-param origin as authoritative.
- **Alternative not taken (A)**: (i) click-guard only (rejected per user + codex — incomplete for the only MED item);
  (ii) **collapsing to a single reused popup window with an enqueue-started timer** (rejected — the enqueue-started
  timer is exactly what caused the starvation DoS). NOTE: arming the per-request timer **on activation** is the
  ADOPTED design (D18), not a rejected alternative — the chosen arbiter runs multiple windows with exactly one active
  and each request's 60 s clock starts when it becomes active.

### Item 3 — Autostart rollback (`src-tauri`)

- `CrashRecovery::enable(&self) -> Result<(), String>`; `enable_impl` bodies **enumerate** exits (D11): idempotent
  already-armed (macOS `crash_recovery.rs:66-69`) = `Ok`; every *arming failure* = `Err` — explicitly incl. macOS
  **plist-unreadable** (`:83-88`) and **patch-failure / no closing `</dict>`** (`:78-80`), plus write /
  `systemctl --user enable` (`:237-244`) / `schtasks` failures (final-pass cond. 3; today these silently
  `warn!`+return = the C8 defect). **v3 codex-final cond. 4**: Phase 1 lands an explicit per-exit table — EVERY early
  return in each of the three `enable_impl` bodies classified `Ok` (idempotent/already-armed) or `Err` (any arming
  failure), no "analogs" hand-wave — recorded in `lessons/phase-1.md`. `enable_crash_recovery() -> Result<(), String>`.
- **Mandatory** injectable rollback transaction helper (D13) covering the **FULL transaction (v3 codex-final cond. 3)**
  — `enable_transaction(snapshot_prior, plugin_enable, crash_arm, plugin_disable, crash_disarm)` (closures), so a
  `plugin_enable` (`manager.enable()`) failure is INSIDE the transaction (not bypassing cleanup as an "after
  `manager.enable()`" helper would). Steps: (i) **snapshot prior autostart state** (`get_autostart_enabled()`); (ii)
  run `plugin_enable` then `crash_arm`; (iii) on ANY failure, run **BOTH** `plugin_disable` + `crash_disarm` **even if
  one fails** (don't short-circuit), then **restore to the prior snapshot** — do NOT unconditionally `disable()` (that
  would clobber a pre-existing-enabled state); (iv) return a **combined** error surfacing every sub-result ("enable
  failed at <step>; rollback: disable=<ok/err>, disarm=<confirmed/failed>, restored_to=<prior>") (D12). Unit-testable
  on Linux CI without a real `AppHandle` via injected closures (model on `CrashRecoveryGuard`,
  `updater.rs:432-463,469-524`).
- Log-and-continue callers `main.rs:512-518` + `updater.rs:411-419` record degraded state and mark rearmed **only
  after `Ok`** (never falsely rearmed). Confirm macOS/Windows `disable_impl` fully revert (plist / Run-key /
  Task-Scheduler) (D-audit cond. 7).

### File-level change map
`core/Cargo.toml` (+direct `windows-sys`), `core/src/win_acl.rs` (new), `core/src/bb.rs` (3 sites + `#[cfg(windows)]`
DACL test), `core/src/config.rs` (Windows ACL), `src-tauri/src/certs.rs` (leaf-key ACL), `core/src/authorization.rs`
(extension grammar + `peek_origin` + arbiter state + tests), `src-tauri/src/commands.rs` (`get_pending_auth` +
`set_autostart` rollback + shared label helper), `src-tauri/src/windows.rs` (arbiter wiring in
`show_auth_popup_window`), `src-tauri/src/main.rs` (register cmd; log-and-continue rearm),
`src-tauri/src/crash_recovery.rs` (enable→Result + exits + tests), `src-tauri/src/updater.rs` (rearm log-and-continue),
`src-tauri/capabilities/authorize.json` (+grant), `frontend-src/authorize.js` + `bridge.js` (origin + guard),
`e2e/authorize.spec.ts` + `e2e/tauri-mock.js` + `e2e-webdriver/auth-flow.spec.ts` (tests).

---

## Competing outline (MID requirement) + why rejected

**Alt approach — "one hardening module, monolithic PR, crate-first":** (1) pull the `windows-acl` crate for the ACL
work to avoid hand-rolled `unsafe`; (2) implement full pre-emptive popup serialization (single window ever, strict
queue) instead of an arbiter over multiple windows; (3) a single big commit. **Rejected because**: (1) `windows-acl`
is a new, thinly-maintained external dep vs. promoting the already-resolved `windows-sys` — worse supply-chain per
the user's caution, and it still needs the same SID/reparse care; (2) full serialization reworks the
`MAX_PENDING_ORIGINS × 60 s` auto-deny/DoS invariant (both auditors flagged the timeout entanglement) — the arbiter
keeps that invariant intact while still making exactly one popup actionable; (3) three phased commits keep blame +
revert clean and let each phase's gate run independently.

---

## Phases

### Phase 1 — C8 autostart `enable() → Result` rollback (Rust, self-contained) — ✓ GREEN (fmt/clippy-D/37 tests; lessons/phase-1.md)
Steps: enumerate error/success exits → `enable → Result`; mandatory injectable `enable_with_rollback`; `set_autostart`
rolls back + surfaces combined failure; `main.rs`/`updater.rs` log-and-continue, rearm-only-on-Ok; confirm per-platform
`disable_impl` revert. Tests: rollback ordering + manager-fail + arm-fail-after-partial + cleanup-fail + prior-enabled,
all via injected closures.
**Validation gate** — `bun run --cwd packages/accelerator lint` · `cd packages/accelerator/src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`. Pass: exit 0, new rollback tests green. Layers: lint · unit(Rust).

### Phase 2 — C9 popup (B → C → A)
Steps: **(B)** per-scheme extension grammar + `canon_*` accept/reject tests. **(C)** `peek_origin` + `get_pending_auth`
(shared label guard) + register + capability grant + `authorize.js` placeholder→server-origin→`get_verified_info`, A2
UX (None/error) + Playwright mock handler + retitle query-param test + WebDriver "shows server origin" case. **(A)**
arbiter (one actionable+topmost popup, FIFO promote on resolve/timeout, piggyback + 60 s timer preserved) + reset-on-
focus click guard in `wireButton` gating activation at entry.
Tests: Rust unit — arbiter promotes FIFO, only one active, piggyback intact, cross-request-label rejection; fake-clock
guard tests (focus-reset, keyboard, synthetic activation); WebDriver two-origin case (second popup not actionable while
first pending; click within guard ignored).
**Validation gate** — lint · `cargo fmt --check && cargo clippy -- -D warnings && cargo test` (src-tauri) · `cargo test --manifest-path packages/accelerator/core/Cargo.toml` · `bun run --cwd packages/accelerator test:e2e:ui` · WebDriver (`bun run --cwd packages/accelerator test:e2e:webdriver`, **CI-only**, timing-sensitive). Pass: exit 0; new reject/arbiter/guard tests green; Playwright + WebDriver popup cases green in CI. Layers: lint · unit(Rust) · integration(Playwright) · e2e(WebDriver, CI).

### Phase 3 — F-003 Windows ACL (prove workspace + leaf TLS key + config.json)
Steps: `windows-sys` direct dep (+`Win32_System_Threading`); `win_acl` module (RAII SID/ACL/handle, atomic
`secure_create_*`, reparse-rejecting `harden_existing`); wire the 5 sites fail-closed; `#[cfg(windows)] #[test]`
asserting the **effective** DACL (via `GetSecurityInfo`/parse ACEs) — exact owner SID, single FULL ACE, PROTECTED
set, inheritable flags on the dir, NO `BUILTIN\Users`/`Everyone` — **and reparse-point rejection**, for **every final
artifact (v3 codex-final cond. 6): the prove dir + witness, `config.json` after its temp→rename, and `localhost.key`**
— not merely helper construction. **This readback
IS the Phase-3 gate (final-pass cond. 4)** — descriptor-construction "no error" does NOT prove the OS applied
anything (a mis-passed SA or a silently-failing `SetSecurityInfo` leaves the file world-accessible). The
construction-only downgrade is **NOT pre-authorized**: if the readback proves genuinely intractable on the runner,
STOP and get explicit re-approval before downgrading, logging why in lessons. (Effective-DACL readback on Windows CI
is readily doable — expect to land it.)
**Validation gate** — `cargo fmt --check && cargo clippy --all-targets -- -D warnings` (src-tauri + core) · `cargo test --manifest-path packages/accelerator/core/Cargo.toml` (Linux: compiles, non-Windows sites unaffected) · **CI `windows-build` lane runs the `#[cfg(windows)]` DACL test** (`accelerator.yml:433-434`). Pass: local fmt/clippy/test exit 0; CI windows-build green with the DACL test EXECUTING (not skipped). Layers: lint · unit(Rust incl. Windows-CI).

---

## Security & Adversarial Considerations
- **Threat model**: local multi-tenant host + a hostile local process racing the port / reading the transient witness
  or the leaf TLS key; a malicious dApp/origin abusing the human-in-the-loop popup (click-steal, homograph/bidi
  extension IDs, display↔decision desync). **Item 1 caveat**: cross-Windows-user isolation only; SAME-user agents are
  not isolated (Unix-parity defense-in-depth, not a same-user boundary).
- **Least privilege**: `get_pending_auth` grantable only to auth popups, self-scoped by exact request_id label. DACL =
  current user FULL only, PROTECTED (no inherited ACEs), inheritable on dirs.
- **Cryptography**: none rolled; Win32 `CreateFile*`/`SetEntriesInAclW`/`SetSecurityInfo`; `windows-sys` pinned to the
  resolved `0.61.x`.
- **Input validation**: per-scheme extension-ID grammar at the single `canonicalize_origin` ingress.
- **Supply chain**: one new **direct** edge to an already-transitive crate; `Cargo.lock`/`bun.lock` committed.
- **TOCTOU / reparse**: eliminated via atomic `SECURITY_ATTRIBUTES` create where we own creation; handle-based +
  reparse-reject where we adopt a `tempfile`/`std`-created path.

## Assumptions
### Facts (verified @ 879e211)
1. bb perms in `core/src/bb.rs`; Windows branches no-op (`:97-98`; no `#[cfg(windows)]` in create_prove_tempdir/write_witness). Unix template test `:250-272`.
2. `certs.rs` keyless CA (key never on disk, `:139-160,187-226`); the no-op'd file is the **leaf TLS key** `localhost.key` (`:160,251-252`).
3. `accelerator.yml windows-build` runs `cargo test` on the **core** crate on windows-latest (`:433-434`) → `#[cfg(windows)]` core test runs in CI.
4. `windows-sys 0.61.x` already resolved transitively in `core`.
5. `get_pending_auth` absent; `respond_auth` resolves by `request_id`, origin diagnostics-only (`commands.rs:177`). Label = `auth-{sha256_128(request_id)}`; `request_id` is a v4 UUID.
6. extension arm only `to_ascii_lowercase()`s (`authorization.rs:56`); opaque-host schemes skip `url` IDNA.
7. `CrashRecovery::enable` returns `()`; `enable_impl` swallow via `warn!`; macOS already-armed exit is a success (`crash_recovery.rs:66-69`); `disable_impl` = remove-before-reload + confirmed-disarm bool (`:254-282`); other rearm callers `main.rs:512-518`, `updater.rs:411-419`.
8. `CreateFileW`/`CreateDirectoryW` accept `SECURITY_ATTRIBUTES` (atomic create-with-DACL); `SetNamedSecurityInfoW` follows reparse points, `SetSecurityInfo(handle)` does not.

### Inferences (attack these)
- 700 ms guard is adequate (browsers ~500 ms) — tune in review; not the main risk.
- ~~The arbiter's per-request 60 s timer (unchanged) doesn't starve a queued popup~~ — WRONG (codex-final); RESOLVED by arming the timer on activation (D18). Accepted policy: queued request waits ≤ `position × 60 s` (≤10×60 s) but always gets a full actionable window.
- `harden_existing` on the tempfile-created tempdir closes the same window as atomic create — verify the object never exists writable-to-others before the handle-based SD applies.

### Asks (resolved with user)
- **A1 scope** → prove workspace + leaf TLS key + config.json (confirmed). **A2 UX** → placeholder→server-origin; None→close, IPC error→disabled+retry (confirmed). **Tier** → MID (confirmed). **Popup A** → guard + arbiter (confirmed).

---

## Seeds (draft — finalized post-approval)

### /goal
```
/goal All 3 phases in implementations-plan/security-hardening/closeout-followups/plan.md marked ✓ (per-phase headers in the file), each ✓ backed by its phase's validation gate reported passing in the transcript; for each phase the agent printed LESSONS_FILE=implementations-plan/security-hardening/closeout-followups/lessons/phase-N.md; `/code-review max --fix` complete + committed; codex post-impl audit (-m gpt-5.6-sol -c model_reasoning_effort=xhigh) complete with high/critical findings addressed (esp. the win_acl unsafe FFI); PR into security-hardening green (accelerator-status + WebDriver + windows-build lanes); `bun run test` and `bun run lint:actions` exit 0 in the transcript.
```
### /loop
```
/loop 15m Drive implementations-plan/security-hardening/closeout-followups forward. Never idle. Each firing: (1) read plan.md + lessons/ (authoritative), `git status`, `git log --oneline -5`; PR exists → `gh pr checks` (no --watch). (2) Waiting on CI is fine — confirm progress; use waits to review the diff / strengthen tests. (3) No task? Take the next pending step; after each edit run fast layers (cargo fmt/clippy + touched crate tests, or bun lint), commit → push. (4) Stuck or a real decision? `/codex xhigh`, log the consult in lessons/phase-N.md, act on the stronger argument; never merge to main/release, never expand scope. (5) Same step failed 5×? Reassess with codex. (6) Phase green = its plan.md gate passes → mark ✓, file lessons, print LESSONS_FILE=..., advance. (7) All ✓? `/code-review max --fix` → commit → codex post-impl audit → address high/critical → wrap-up report + stop. Keep an ASCII checklist visible.
```
Recommended: **/loop 15m** (Windows-CI + WebDriver signals aren't fully transcript-observable; interval cadence drives through CI waits). `/goal` is the alternative.
