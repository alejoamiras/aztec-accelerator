# Audit — fable leg (run on Opus 4.8, Fable unavailable)

Backup adversarial audit that became substantive because the codex leg kept getting infra-killed. Verified
against source at the `sechard-closeout-followups` worktree tip. Converges strongly with the codex leg
(`audit-codex.md`).

## 1. Security / Adversarial

**(a) Windows DACL**
- **The "CA private key" scope justification is FACTUALLY WRONG.** `certs.rs` is a keyless-CA design (F-016):
  the CA key is in-memory `Zeroizing`, signs the leaf, and is dropped BEFORE any write, never on disk
  (`certs.rs:139-160`); `migrate_legacy_ca_key_at` deletes any legacy on-disk `ca.key`, fail-closed
  (`certs.rs:187-226`). The no-op'd file (`certs.rs:251-252`) is the **leaf TLS key** (`localhost.key`,
  `certs.rs:160`) — sensitive (same-box reader can impersonate the localhost HTTPS leaf) but NOT a mint-any-cert
  primitive. Re-ground A1 on the leaf key's real (lower) severity.
- **"Windows has no atomic create-with-DACL" is FALSE.** `CreateDirectoryW`/`CreateFileW` accept
  `lpSecurityAttributes` (self-relative SD) → object created WITH the DACL atomically, zero TOCTOU. Post-create
  `SetNamedSecurityInfoW` is a legit reuse tradeoff but is a CHOICE, not a platform floor.
- **Bypass-traverse defeats "parent dir protects child".** Most Windows users hold `SeChangeNotifyPrivilege`
  (bypass traverse), so intermediate-dir ACLs don't gate a deep file. The witness file's OWN ACL matters — apply
  it to the EMPTY file (create_new → ACL → write_all), not after bytes are written (`bb.rs:138`).
- **Fail-open regression:** `prove_tmp_parent` returns `Option`/`.ok()?` and the caller falls back to OS `%TEMP%`
  (`bb.rs:117-121`). Threading the ACL via `.ok()?` silently degrades to an UNHARDENED temp dir on ACL failure —
  opposite of "fail-closed". The three sites have three different error contracts; a blanket "propagate" doesn't map.
- **Unsafe-FFI to hard-gate:** token-SID two-call `GetTokenInformation(TokenUser)` sizing + `CloseHandle`;
  `SetEntriesInAclW`→`LocalFree` exactly-once on every error path; don't free the SID (aliases the token buffer);
  `SetNamedSecurityInfoW` follows reparse points → prefer HANDLE-based `SetSecurityInfo`; feature set likely
  missing `Win32_System_Threading` (`GetCurrentProcess`/`OpenProcessToken`).
- **Honest value:** default `%LOCALAPPDATA%` ACLs are already owner+SYSTEM+Admins (not world); a local admin
  bypasses any DACL. Real gain = cross-Windows-user isolation + stripping inherited group ACEs (PROTECTED). It
  gives ZERO isolation between agents running as the SAME Windows user — exactly the "many agents on one box"
  threat if they share a user (same limitation as Unix 0o700).

**(b) get_pending_auth — binding is SOUND.** request_id is a v4 UUID; label `auth-{sha256_128(request_id)}`; a
mismatched popup computes a different expected label → rejected; `window` is unspoofable from JS. A different popup
CANNOT peek another request's origin. Caveats: no ACTIVE desync today (popup URL already server-built), so this is
defense-in-depth not a hole (soften the "desync" prose); and `get_verified_info` is still keyed off the query param
(`authorize.js:12`) — route it through the server origin too, and don't render the query origin even momentarily.

**(c) Click-delay guard — materially mitigates, does NOT "defeat".** The per-window show/focus guard does defeat the
cross-origin stacking steal (popup #2 is a fresh build → fresh guard). But: must gate activation at entry in shared
`wireButton` (so keyboard Enter/Space is covered); time-only guards lose to click-frenzy/timed re-click; identical
`.center()` stacking means A's sufficiency is CONTINGENT on C's server-origin display — A and C are coupled, not
independent. Restate as "materially reduces"; full serialization not strictly required for the stacking vector but
the argument must be made explicit + residual documented.

**(d) C8 rollback — correct skeleton, two real gaps.** (i) Rollback discards signals: `let _ = manager.disable();`
— if disable fails during rollback, autostart is left ENABLED while returning the crash-recovery error (the exact
half-enabled state, relocated). Surface a rollback failure distinctly. (ii) "Convert every warn!+return to Err" is
too blunt — macOS "already has KeepAlive" (`crash_recovery.rs:66-69`) is a SUCCESS; mapping it to Err would roll
back a WORKING enable. Enumerate error vs success exits. Confirm macOS/Windows `disable_impl` fully revert (plist,
Run-key, Task-Scheduler). The log-and-continue asymmetry (main.rs/updater.rs NOT rolled back) is right + load-bearing.

## 2. Assumption-attack
- **Facts WRONG:** certs "CA private key" (it's the leaf key); "Windows has no atomic create-with-ACL" (it does).
- **Unsafe Inferences:** "TOCTOU negligible" (rests on the false premise + ignores bypass-traverse); "click-delay
  alone suffices" (contingent on C + click-frenzy residual); 700ms is fine (not the risk).
- **Asks:** A1 — decision to include config/certs is reasonable but rationale is wrong; narrowing to prove-only while
  a real TLS private key stays default-ACL is the LESS defensible outcome, not a safe fallback. A2 — disable-Allow +
  hint is right; add: on `None` (stale/resolved) close the popup; mismatch → query is ignored anyway; transient IPC
  error → keep Allow disabled + offer retry.

## 3. Implementation critique
- Structure mostly right + reuse-aware. `windows-sys` hand-rolled over the transitive crate (vs unmaintained
  `windows-acl`) is the correct supply-chain call. One shared ACL helper (not 3 copies). Reusing respond_auth's guard,
  `peek_origin` mirroring map discipline, and the `CrashRecoveryGuard` Cell-counter test pattern are all good.
- Click guard in shared `wireButton` — correct; gate activation at entry, not post-hoc `disabled`.
- Weakest gate: Phase-3 effective-DACL readback is the RIGHT target but fiddly; the documented fallback (descriptor
  construction + "runs error-free") STOPS verifying the actual security property — if taken, ACL correctness rests
  entirely on the FFI audit, so hard-gate that.
- **Tier: nudge to MID** — Item 1 is net-new `unsafe` Win32 FFI on a security boundary + a scope decision touching a
  real private-key file the plan mis-describes. LIGHT survivable only if codex hard-gates the FFI + certs fact is
  corrected first.

## VERDICT: conditional approve (with conditions:
1. Correct certs.rs facts (keyless CA; no-op'd file is the leaf TLS key); re-ground A1 on leaf-key + config-integrity
   severity; don't narrow scope to prove-only while a TLS private key stays default-ACL.
2. Fix TOCTOU story: use `SECURITY_ATTRIBUTES` at create OR apply ACL to the EMPTY witness file before write_all;
   account for bypass-traverse (parent-dir ACL doesn't protect the child).
3. Per-site fail-closed semantics so an ACL failure in `prove_tmp_parent` does NOT fall back to unhardened `%TEMP%`.
4. Hard-gate the codex/FFI audit: token-SID two-call sizing + CloseHandle, SetEntriesInAclW→LocalFree exactly-once,
   SID/token buffer aliasing, reparse-following (prefer HANDLE-based SetSecurityInfo), likely-missing
   `Win32_System_Threading`.
5. C8: enumerate error vs success `enable_impl` exits (don't map idempotent macOS already-armed to Err); surface a
   rollback `disable()` failure distinctly instead of `let _ =`.
6. C9 coherence: route `get_verified_info` through the server origin; never render the query origin as authoritative;
   soften "defeats"→"materially mitigates"; note A depends on C for identical-position stacking.
7. Reconfirm macOS/Windows `disable_impl` fully revert enable() on every platform.)
