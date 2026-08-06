# c6-persistence — Claude (Opus) raw findings

> **COORDINATOR NOTE (main agent):** F-C6-1 is a **bypass of a fix shipped earlier the same day**
> (arc-bug-hunt r6 #1, PR #429 `0033c0e`, which introduced `appimage_self` precisely to stop an
> inherited `$APPIMAGE` being trusted). The fix's provenance test is `exe.starts_with(APPDIR)`, which
> `APPDIR=/` satisfies. The unit test written with that fix
> (`appimage_trusted_only_when_our_exe_lives_under_appdir`) only covers the honest `/tmp/.mount_*` shape
> and an obviously-disjoint foreign mount — it cannot catch an ancestor-widening `APPDIR`. F-C6-3 also
> confirms uncertainty flag #5 from the repo map as exploitable.

## F-C6-1 — `appimage_self` accepts ANY ancestor as `$APPDIR`, so a same-user actor makes the app write attacker-chosen autostart + systemd persistence (Linux)

**Impact.** Integrity (the OS executes a binary the app did not intend, at every login, under the app's
identity) + Availability (crash recovery redirected away from the app). Blast radius: one account, but the
persistence is SELF-REPAIRING (rewritten every launch) and ATTRIBUTED TO A LEGITIMATE SIGNED APPLICATION,
defeating "unknown autostart entry" triage and EDR heuristics. The relaunched payload runs with the user's
full session privileges incl. `~/.aztec-accelerator/` (CA material, updater state) and the loopback
listener handling private witnesses. Vector local; complexity LOW (set two env vars); privileges low
(unprivileged same-user, no root); user interaction NONE for the heal/rearm variants.
**Confidence** high for the bypass (`APPDIR=/` demonstrably accepted); moderate for end-to-end severity
(needs to influence the app's process environment). **CWE-454** primary; CWE-426, CWE-15 siblings.
OWASP A08:2021.

**Trace.**
1. Source `autostart.rs:1202-1208` (`appimage_self_from_env`) reads `APPIMAGE`/`APPDIR` from the process
   env. The module comment `:1176-1184` correctly identifies that both are inherited by children and
   builds a defence against it.
2. **The defective check** `autostart.rs:1197`: `exe_c.starts_with(&dir_c).then(|| PathBuf::from(appimage))`
   — the ENTIRE provenance test. It proves only that `current_exe()` lives SOMEWHERE UNDER `$APPDIR`.
   `APPDIR=/` satisfies it for every absolute exe; so do `/usr`, `$HOME`, `/opt`. Nothing checks that
   `$APPDIR` is an AppImage squashfs mount, that `$APPIMAGE` relates to `$APPDIR`, that `$APPIMAGE` is a
   regular file, or that it is a squashfs image at all.
3. Sink A — `.desktop` autostart: `autostart.rs:1227-1229` (`desired_path`) returns the attacker's
   `$APPIMAGE` verbatim → `:1684-1760` (`set_enabled_at`) → `:933-945` (`backend::enable_write`) →
   `desktop_quote` → `Exec=` in `~/.config/autostart/Aztec Accelerator.desktop`. Reached from the
   Settings/onboarding toggle (`commands.rs:462`).
4. Sink B — systemd user unit: `crash_recovery.rs:264`
   `recovery_target(crate::autostart::appimage_self_from_env(&exe), exe)` → `:245-252` returns the
   attacker path → `:276` `systemd_exec_start` → `:287-305` writes
   `~/.config/systemd/user/aztec-accelerator.service` with `ExecStart=":/attacker/path"`,
   `Restart=on-failure`, `WantedBy=default.target` → `:311-313` `systemctl --user enable`. This path
   re-reads the env INDEPENDENTLY of `desired_path`, so one variable poisons both sinks.
5. Sink C — no user interaction at all: `autostart.rs:1346-1408` (`heal_if_broken_at`) writes `desired`
   into the `.desktop` whenever the stored entry classifies `Broken`; called unconditionally at startup
   from `main.rs:611`.
6. **Ownership-gate inversion:** `autostart.rs:1627-1632` passes `desired_path(app)` — the POISONED path —
   as the ownership reference to `implicit_arm_gate` (`:1547-1555`). `classify_program` (`:670-688`) then
   compares the stored `Exec` against the attacker's path; if they match, `points_elsewhere` is false →
   `implicit_arm_allowed` true (`:1524-1527`) → `enable_crash_recovery()` arms recovery AT THE ATTACKER'S
   BINARY. The gate whose entire purpose is "decline when another copy owns the entry" reads the
   attacker's path as "us".

**Missing control.** No validation that `$APPDIR` is a genuine AppImage mount; no cross-check binding
`$APPIMAGE` to `$APPDIR`. `starts_with` is a CONTAINMENT test masquerading as a PROVENANCE test.
Sound alternatives: compare `st_dev` of `current_exe()` against `/` (a real AppImage exe is on the
squashfs loop device); require `$APPDIR` to be a PROPER mountpoint per `/proc/self/mountinfo`; or drop env
trust entirely and derive the AppImage path from `/proc/self/mountinfo` for the device backing
`current_exe()`.

**Exploit.** Same-user code drops a payload at `~/.cache/fontconfig/.fc-cache.AppImage`, appends
`APPDIR=/` + `APPIMAGE=/home/u/.cache/fontconfig/.fc-cache.AppImage` to the session environment
(`~/.config/environment.d/10-x.conf`, `~/.profile`, or a shadowing `.desktop` with
`Exec=env APPDIR=/ APPIMAGE=… /usr/bin/AztecAccelerator`) → on launch `appimage_self` returns the
attacker path because `/usr/bin/AztecAccelerator`.starts_with(`/`) → onboarding "Start on Login" (or an
already-on entry) writes `Exec=` and enables `aztec-accelerator.service` with the attacker's `ExecStart`
→ payload runs at every login, respawned by systemd on failure, and **re-written and re-enabled on every
subsequent Accelerator launch** via `startup_rearm`, surviving the user deleting it.

**Why mitigations fail.** `autostart_path_is_safe` (`crash_recovery.rs:211-216`) rejects only non-absolute
paths and control bytes. `desktop_quote` (`autostart.rs:402-414`) and `systemd_exec_start`
(`crash_recovery.rs:225-239`) are CORRECT serializers — they prevent directive injection, not a wrong path.
`set_enabled_at`'s existence check (`:1690`) is satisfied (the attacker's file exists).
`implicit_arm_gate` is the designated foreign-copy defence and is inverted by the same variable.
**The documented r6 #1 defence IS `appimage_self`; this is a bypass of the mitigation, not its absence.**
The existing test `appimage_trusted_only_when_our_exe_lives_under_appdir` (`autostart.rs:1789-1818`) covers
only the honest `/tmp/.mount_AbC` shape and an obviously-disjoint foreign mount — no case widens `APPDIR`
to an ancestor, so the suite cannot catch this.

**Instances.** `autostart.rs:1197` (the check — single fix point); `:1227-1229` (`desired_path` consumer);
`crash_recovery.rs:264` (`recovery_target` consumer); `autostart.rs:1627-1632` (ownership-reference
consumer).

## F-C6-2 — The update-window marker is an unauthenticated file in a user-writable dir; a forged, refreshed marker indefinitely disables auto-update, crash recovery, and the autostart heal (Windows)

**Impact.** Availability + Integrity — denial of the PATCH CHANNEL. A pinned, un-updatable Accelerator
keeps every present and future vulnerability exploitable, including the ones in this audit. Vector local;
complexity low (write one well-formed JSON file); privileges low; user interaction none. Fully silent —
the only signal is `tracing::warn!` the user never sees. **Confidence high.** **CWE-345** primary;
CWE-732 sibling. OWASP A08:2021.

**Trace.** `update_marker.rs:62-64` places the marker at
`%USERPROFILE%\.aztec-accelerator\update-in-progress.json`, writable by any same-user process. `:141-168`
(`load`) validates SHAPE ONLY (schema, canonical SemVer, future clamp) — **no MAC, no signature, no
owner/ACL check on read, no binding to an update this process started**. `:30` sets
`DEADLINE_SECS = 15 min` but `:33` allows `MAX_FUTURE_SECS = 24 h` (enforced `:164`), and there is **no
creation timestamp in the payload**, so nothing distinguishes an app-issued 15-minute window from a forged
24-hour one. Sinks: update denial on poll (`updater.rs:283-289`); update denial + recovery left disarmed
at install (`:443` disarms as a hard precondition, then `:470` → `CreateErr::Live` at `:477-487` aborts and
calls `guard.defuse()`, so **the disarm is NOT rolled back**); heal denial (`autostart.rs:1372-1377`
fast path, `:1387-1392` authoritative); rearm denial (`:1586-1591` — so the task disarmed above is never
restored); whole-launch suppression (`:1474-1489` → `reconcile_under_lock` → with no completion token
`removal_decision` returns `Suppress("no completion token")` (`update_marker.rs:350`) →
`startup_reconcile` false → `main.rs:606` skips BOTH heal and rearm); and user-control denial with a
MISLEADING message (`autostart.rs:1710-1718` returns "an update is finishing; try turning Start on Login
on again in a moment" — disguising the attack as transient).

**Missing control.** No authenticity binding between the marker and an update this app started. The `txn`
nonce is NOT an authenticator: it is read from the very file it would authenticate, and its counterpart
`update-txn-done` sits in the same user-writable directory, so anyone who can read the marker can mint a
matching token — collapsing the advertised "ALL FOUR" removal conditions (`update_marker.rs:12-16`) to
"version + path" for any same-user actor. Also: `deadline_unix` is unconstrained relative to creation (no
`created_unix`, no `deadline ≤ created + DEADLINE_SECS + slack` rule); and while `try_create_exclusive`
uses `win_acl::secure_create_file` (`:271`) to make OUR markers owner-private, `load` never verifies the
ACL or owner of a marker it did not create — **the hardening is write-side only**.

**Exploit.** Write `{"schema":1,"txn":"0","candidate":"<running version>","expected_install_path":
"C:\\nonexistent","intent_at_disarm":true,"deadline_unix":<now+86400>}` and no token file → `load` returns
`Valid`, `is_live` true for 24 h → every poll bails, every launch is `Suppressed`, and if an install had
reached `updater.rs:443` the recovery task is gone and stays gone → attacker refreshes every few hours →
suppression indefinite, no expiry ever fires.

**Related, same root cause (inverse).** DELETING the marker during a genuine NSIS window re-enables
`heal_if_broken` and `startup_rearm`; a launch in that window can register the every-minute Task Scheduler
relauncher while NSIS is mid-rewrite of `AztecAccelerator.exe` — exactly the "tick during NSIS file
mutation could spawn a half-written binary" hazard `updater.rs:440-442` exists to prevent.
Timing-dependent, lower confidence, same missing control.

**Why mitigations fail.** `deny_unknown_fields` (`:70`) and the canonical-SemVer round trip (`:158-161`)
reject MALFORMED payloads; a well-formed forgery passes by construction. The 24 h clamp caps a single
forgery, not refreshing. The compare-and-create **deliberately never deletes a live foreign marker**
(`:219`, D22) — the property that makes the app's own concurrency safe is what makes the forgery durable.
The `autostart.lock` discipline defends races between HONEST processes. The module comment `:17-18`
correctly notes "File mtime is never consulted: it is forgeable by the same user who can write the file" —
**the same reasoning applies to the entire payload and was not carried through**.

**Instances.** `update_marker.rs:141-168`, `:33,164`, `:297-302` (`read_token_nonce`); consumers
`updater.rs:283-289`, `:470-487`, `autostart.rs:1372-1377`, `:1387-1392`, `:1586-1591`, `:1710-1718`,
`:1474-1489`.

## F-C6-3 — `schtasks_exe()` resolves through `SystemRoot`/`windir` without the hardcoded-System32 preference the repo applies to `certutil`, breaking the disarm-confirmation contract (Windows)

**Impact.** Integrity + Availability — the boolean "the always-armed relauncher is confirmed gone" is a
hard precondition for mutating installed files, and it becomes attacker-controlled in BOTH directions.
No privilege boundary is crossed (the planted binary runs as the same user) — this is contract
subversion, not EoP. Vector local; complexity low; privileges low; no user interaction.
**Confidence** high for the asymmetry and consequences; moderate for the specific shadowing mechanics
(a hostile launching parent can always set it regardless). **CWE-426**, CWE-454. OWASP A08:2021.

**Trace.** `crash_recovery.rs:388-395` derives the path entirely from the inherited environment:
`std::env::var("SystemRoot").or_else(|_| std::env::var("windir")).unwrap_or_else(|_| "C:\\Windows")`.
Contrast `trust/windows.rs:36-47`, which resolves the SAME CLASS of tool and explicitly prefers the
hardcoded path first — its doc at `:31-33` says so: *"Prefers the hardcoded
`C:\Windows\System32\certutil.exe` when it exists, so a tainted `SystemRoot`/`windir` environment can't
redirect this privileged trust operation (post-impl codex High)."* `trust/windows.rs:4` even CITES
`crash_recovery::schtasks_exe` as the pattern it follows — **the hardening was applied to only one of the
two**.
Sink A (denial): if `schtasks.exe` cannot spawn, `/Query` errors and `.unwrap_or(true)` reports "still
present" (`:459-465`); after three attempts `disable_impl` returns false (`:476`) → `updater.rs:443-450`
"cannot confirm ⇒ do NOT install" → **every update aborted, permanently and silently**. Also makes
`set_enabled(false)` error (`autostart.rs:1765-1771`) and the Quit path log a relaunch warning
(`:1429-1433`). Setting `SystemRoot` to any directory lacking `System32\schtasks.exe` suffices — a second,
independent update-denial primitive alongside F-C6-2.
Sink B (false confirmation): a planted `%SystemRoot%\System32\schtasks.exe` exiting non-zero on `/Query`
makes `disable_impl` return true (`:466-469`) while the real task remains armed; the updater then hands off
to NSIS believing the relauncher is gone — exactly the hazard `updater.rs:440-442` documents. The same
stub also intercepts `/Create` (`:427-430`), yielding the task XML and control over what is registered.

**Missing control.** No hardcoded-`C:\Windows\System32` preference and no `GetSystemDirectoryW`.
`crash_recovery.rs:385-386` claims the defence ("avoids a bare-name PATH lookup … a planted `schtasks`
earlier on PATH can't win") — true for PATH, false for `SystemRoot`/`windir`. Secondarily `:465`'s
`.unwrap_or(true)` is right for a TRANSIENT error but converts a persistently unreachable `schtasks` into
an unrecoverable, silent update block with no distinct diagnostic.

**Why mitigations fail.** The absolute-path construction defeats PATH planting only; the ROOT of that
absolute path is attacker-supplied. The three-attempt retry + `/Query` verification (`:455-472`) exist to
make the disarm result trustworthy but all route through the same poisoned resolver — more retries against
an attacker-controlled binary is not verification. `trust/windows.rs:36-47` proves the project already
accepts this threat model and already has the fix.

**Instances.** `crash_recovery.rs:388-395` (resolver, single fix point); call sites `:427` (`/Create`),
`:456` (`/Delete`), `:459` (`/Query`). Reference implementation to mirror: `trust/windows.rs:36-47`.

## NON-FINDINGS (assessed, not missed)

- **Task Scheduler XML temp-file TOCTOU** (`crash_recovery.rs:414-425`): `tempfile::Builder` uses
  `CREATE_NEW` + random name in per-user `%TEMP%`; no cross-user boundary, no privilege gain — a same-user
  attacker who could win the race can call `schtasks /Create` directly.
- **`xml_escape`** (`:522-528`): complete and correctly ordered (`&` first) for the single element-text
  context used (`<Command>`). No injection path.
- **`run_value_quote`/`run_value_candidates`** (`autostart.rs:528-578`): the parser is strictly MORE
  conservative than `CreateProcess` in every direction that matters — quoted values with trailing content
  and unquoted values carrying `-` options both fail closed to `Unreadable`, which `implicit_arm_allowed`
  (`:1537`) declines. No round-trip asymmetry lets an attacker's value classify as ours.
- **`desktop_quote`/`desktop_exec_program` round-trip** (`:333-414`): decode is the exact inverse of
  encode; duplicate-key detection (`desktop_field` `:298`) makes ambiguous entries `Unreadable`. A `Hidden`
  key duplicated twice reads as not-hidden (`:315-317`) — correctness wrinkle, needs pre-existing write
  access, not a security-property violation.
- **`systemd_exec_start`** (`crash_recovery.rs:225-239`): rejects everything systemd's `string_is_safe`
  rejects, `:`-prefixes to disable `$` expansion, doubles `%`; the round-trip test (`:735-746`) is a genuine
  oracle. No unit-directive injection.
- **NSIS `POSTUNINSTALL` guards** (`nsis/hooks.nsi:96-112`): the `$UpdateMode <> 1` AND short-path-normalized
  `$EXEDIR != $INSTDIR` pair is sound; `GetFullPathName /SHORT` normalization with raw fallback closes the
  casing/trailing-slash/8.3 mismatch, returns the long path on volumes with 8.3 disabled, and both operands
  are normalized identically. No attacker-influenced input reaches either operand. `RMDir /r` on a `certs`
  junction would follow the reparse point, but this is a `currentUser` non-elevated bundle — no boundary
  crossed.
- **NSIS `POSTINSTALL` token handoff** (`:75-86`): `Delete`-then-`Rename` is idempotent; failure is
  non-fatal and leaves the safe suppressed state. Its forgeability is folded into F-C6-2.
- **`implicit_arm_gate` in isolation** (`autostart.rs:1521-1555`): the decision table is correct on its own
  terms; it becomes invertible only through the poisoned ownership REFERENCE (F-C6-1 step 6).
- **macOS `crash_recovery::enable_impl` writes the LaunchAgent plist with `std::fs::write`** (`:159`),
  bypassing the symlink refusal + atomic-rename discipline `autostart::write_artifact_atomic`
  (`autostart.rs:737-761`) applies to the SAME file. Real asymmetry, but the primitive is "insert a fixed
  `KeepAlive` block into a user-writable file containing `</dict>`" as the same user — no security property
  violated. Worth fixing for consistency; below the bar.
