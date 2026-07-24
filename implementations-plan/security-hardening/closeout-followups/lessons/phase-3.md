# Phase 3 — F-003 Windows tail: owner-only ACLs

**Local status: compile + clippy validated for Windows via cross-check; runtime validates in `windows-build` CI + the codex FFI audit.**

## Key unblock: cross-compile-CHECK for Windows works on this Linux box
`cargo check`/`clippy` do NOT link, so `rustup target add x86_64-pc-windows-gnu` + `cargo check --target
x86_64-pc-windows-gnu` compiles the `#[cfg(windows)]` FFI locally — real compile + clippy feedback for the
one part of the plan I'd assumed was "CI-only". The `core` crate (where `win_acl` + `bb.rs` + `config.rs`
live) cross-checks cleanly; `src-tauri` does not (tauri build.rs needs a Windows bb sidecar), so `certs.rs`'s
one-line call validates in CI.

## `core/src/win_acl.rs` (`#![cfg(windows)]`) — the FFI, hand-rolled over `windows-sys` 0.61
Design folds every dual-audit + codex-final FFI condition:
- **Reparse-safe + existence-atomic**: objects created with `CREATE_NEW` / `CreateDirectoryW` — FAIL if
  anything is already at the path, so a pre-planted symlink/junction can't be adopted.
- **Handle-based PROTECTED DACL**: ACL applied to the OPEN handle via `SetSecurityInfo` with
  `PROTECTED_DACL_SECURITY_INFORMATION` (strips inherited ACEs; handle-based does NOT follow the name,
  unlike `SetNamedSecurityInfoW`).
- **Fail-closed readback**: `verify_owner_only` reads the effective DACL back (`GetSecurityInfo` + `GetAce`),
  asserts every ACE is our SID (`EqualSid`) and rejects `WinWorldSid`/`WinBuiltinUsersSid` — a null/empty
  DACL (FAT/exFAT no-op) is an ERROR, not a falsely-"secured" path.
- **Memory hygiene**: token handle `CloseHandle`d (RAII `HandleGuard`); the `SetEntriesInAclW` ACL and every
  `GetSecurityInfo` descriptor `LocalFree`d exactly once (RAII `LocalFreeGuard`); the SID is `CopySid`'d out
  of the token buffer (never aliased/freed separately); `GetTokenInformation` two-call sizing;
  `Win32_System_Threading` feature pulled in for `GetCurrentProcess`/`OpenProcessToken`.
- API: `secure_create_dir` (inheritable), `secure_create_file` (returns a ready-to-write `std::fs::File` via
  `from_raw_handle`), `harden_existing_file`/`harden_existing_dir`.

## Wiring (fail-closed — an ACL error PROPAGATES, never a silent `%TEMP%` fallback)
- `bb.rs prove_tmp_parent`: `secure_create_dir` (or `harden_existing_dir` if it pre-exists) for the
  persistent `prove-tmp`. No OS-temp fallback on Windows (`None` → caller fails).
- `bb.rs create_prove_tempdir`: fails closed if no per-user dir (D4). The `prove-tmp` parent is owner-only +
  **inheritable**, so the `tempfile`-created child inherits owner-only AT creation (no window) and is then
  hardened explicitly (PROTECTED) — this is why "tempfile + harden the child" is safe here (D21).
- `bb.rs write_witness`: `secure_create_file` (empty file gets the DACL before any bytes).
- `config.rs save_to`: `secure_create_file` for the temp (SD travels with the same-volume rename to
  `config.json`).
- `certs.rs write_pem_file` (src-tauri): `accelerator_core::win_acl::secure_create_file` for the temp →
  covers `localhost.key` (the **leaf TLS key** — corrected D1; the CA key is keyless/never on disk).

## Scope decision (which paths) — cross-user only, per the honest threat note
Prove workspace + witness + leaf TLS key + `config.json` (user-confirmed A1). The ACL isolates DIFFERENT
Windows users + strips inherited group ACEs; it does NOT isolate same-Windows-user processes (Unix-`0o700`
parity), documented in the plan's Security section.

## Validation
- **Local green**: `core` Windows cross-check `--all-targets` (incl. the `#[cfg(windows)]` effective-DACL
  test) + Windows `clippy -D` clean; `core` Linux 178 tests + Linux `clippy -D` + `fmt` clean; `src-tauri`
  Linux compiles.
- **CI (windows-build lane)**: RUNTIME of the effective-DACL test + `certs.rs`'s Windows path. A successful
  `create_prove_tempdir` + `write_witness` there IS the effective-DACL assertion (the internal readback
  fail-closes otherwise).
- **Codex FFI audit** (plan hard-gate): still to run at post-impl — the correctness backstop for the unsafe
  SID/ACL/handle/free/reparse details, per the plan.

## Post-impl codex audit — consult log
- First post-impl codex run (full-diff prompt) was **infra-killed** mid-exploration (~5.9k lines streamed,
  no verdict) — the recurring codex-kill on this box (also hit 3× during planning). Per AFK protocol: logged,
  not silently skipped.
- Relaunched **tight**, `win_acl.rs`-only (the highest-risk unsafe FFI), read-only, via stdin — the
  invocation shape that survived during planning. Verdict folded on completion.
- Tight re-audit COMPLETED (survived): **REJECT, 3 real FFI defects**, all folded + Windows-cross-check+clippy clean:
  (1) misaligned `TOKEN_USER` deref → `addr_of!`+`read_unaligned` for the SID pointer;
  (2) ACE parsed without type/mask verification → `AceType==ACCESS_ALLOWED_ACE_TYPE` (307cabc) + `Mask & FILE_ALL_ACCESS == FILE_ALL_ACCESS`;
  (3) `secure_create_dir` create→open junction race → `reject_if_reparse` (GetFileInformationByHandle) in `apply_and_verify_owner_only`.
