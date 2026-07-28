# Plan — autostart self-heal

`/blueprint deep`. Consolidated from three independent planning legs: `plan-codex.md` (codex,
`xhigh`), `plan-fable.md` (top-tier Claude planning subagent), and the main agent's own draft.
Grounded in `recon.md` (Phase 0.4), whose constraints are cited as **C1**–**C8**.

**Revision 4** — post-double-audit. Codex returned **reject** (6 blocking) and fable
**conditional-approve** (5 blocking); both are folded. See §12. Status: **draft — pre-final-codex-pass.**
Not approved.

---

## 1. The bug, in one sentence

The app writes an OS-level "start on login" entry containing an absolute binary path, once, at toggle
time, and never revalidates it — so relocating the app (the standard macOS download → run →
drag-to-`/Applications` flow) silently kills autostart while Settings still shows it **ON**, because
`auto-launch`'s `is_enabled()` only checks that the entry *exists*.

Owner scoping: **zero install base — no migration code.** That removes the upgrade trigger but not
the relocation trigger, which is a fresh-install scenario.

## 2. Success criteria

1. An entry whose stored target no longer resolves is repaired at next launch, without user action,
   without recreating the artifact, and **without stripping macOS `KeepAlive`** (C1).
2. **Settings never shows a bare, unqualified ON for an entry that will not launch.** The switch
   reflects the user's *intent*; an adjacent warning row carries *health* and the action that fixes
   it. Turning it OFF always works. (Revised in r2 — see D17; the r1 wording implied an unchecked
   switch for a broken entry, which took away the user's only OFF control.)
3. A freshly-enabled Windows entry is **quoted** (§9 — today's unquoted value is a live same-user
   persistence-hijack primitive).
4. **No Aztec code path can resurrect an entry that was `Absent` at its locked read.** Narrowed in r2:
   an absolute guarantee is impossible — a foreign process deleting the artifact between our locked
   read and our write cannot be defended against. What *is* guaranteed is that our own OFF and our own
   heal cannot interleave, because every owned mutation takes the same lock.
5. Autostart artifact content is asserted by tests — on all three OSes — where today **nothing is**.
6. A heal cannot fire inside the Windows NSIS install window (D18).

---

## 3. Decision ledger

### 3.1 Converged (independent agreement, adopted)

| # | Decision | Source |
|---|---|---|
| **D1** | **Heal iff the stored target does not resolve.** Never "differs from my `current_exe()`." | codex + fable, independently — both rejected the main agent's framing |
| **D2** | Never call the plugin's `enable()` in a heal; patch the artifact **in place** | codex + fable |
| **D3** | `get_autostart_enabled` returns a **structured status**, not a bare bool | codex + fable |
| **D4** | Own the per-platform **readers**; atomic same-dir temp + `rename`; quote and escape everything | codex + fable — **but see the honesty note below** |
| **D5** | Add a bare `cargo test` to a macOS CI leg — recon's **BONUS BUG** (no macOS job compiles `#[cfg(test)]`, so three `patch_plist_*` tests have never run anywhere) | codex + fable |
| **D6** | **Drop the wine-Rust spike as a merge gate** (recon measured exit 53) | codex + fable, same reasoning: `windows-latest` already tests production code faithfully |

**Ledger honesty note (r2, raised by fable).** D4 as written in r1 **overstated the convergence**.
Fable owned the readers and the *heal* writer but kept the plugin's **enable-time** writer. "Own the
writers" was therefore the substance of Fork A, not a converged point. Only the readers genuinely
converged.

**D1 is the load-bearing decision.** The main agent's diff-based framing creates a regression worse
than the bug: a leftover copy in `~/Downloads`, double-clicked once, repoints a healthy
`/Applications` entry at a file the user is about to delete. Resolve-based healing is a strict
repair — it only ever replaces a *dead* pointer with a *live* one. It works for the target case
precisely because Finder's drag is a **move**. If the user *copies* instead, both exist, nothing
heals, and autostart still launches a working app.

Note carefully what D1 does **not** buy, which r1 wrongly assumed: convergence holds only when the
stored target is *permanently* dead. It does **not** hold when the target is *transiently* absent —
which is exactly what NSIS does mid-install. See D18.

### 3.2 Forks resolved

**Fork A — keep the plugin (fable) vs remove it entirely (codex). → Remove. (D7)**

Codex's position wins, on an argument neither leg made explicitly:

> **The plugin's `enable()` *is* the unsafe serializer.** `auto-launch-0.5.0/src/windows.rs:37-43`
> writes `format!("{} {}", app_path, args)` — unquoted. Keeping the plugin for enable means the
> unquoted Windows Run value is written **at enable time, on every fresh install**, and healing only
> repairs it after the path has already broken. Fable's own security finding (§9) is therefore only
> half-fixed by fable's own plan.

Fable's contradiction-check conceded this "decisively" after verifying the plugin surface itself.

Supporting evidence found during consolidation: `winreg` appears **twice** in `Cargo.lock` — `0.10.1`
pulled by `auto-launch`, and `0.55.0` already present via another path. Removing the plugin therefore
**drops** a crate rather than adding one.

**Cost accounting (corrected in r2 — codex found three omissions).** Removal touches
`commands.rs:51,55,445-446`, `main.rs:23,556-557,609,614`, `updater.rs:364-365`. Adding
`repair_autostart` *additionally* requires:
- **`src-tauri/build.rs:143-165`** — the app-manifest command list. This is **ALL-OR-NOTHING**: Tauri
  only enforces the per-window ACL for app-local commands when the manifest exists, so a command
  omitted here silently escapes the F-012 trust boundary. Verified.
- **`scripts/tauri-trust-boundary.test.ts:145,147`** — a static command set mirroring the above.
- **Centralized app-name derivation.** With the plugin gone, `package_info().name` semantics must be
  preserved in exactly one helper feeding all three writers *and* the artifact filename, or the three
  platforms will drift.

No hidden bundler dependency, no granted plugin ACL, no JS guest package — both reviewers confirmed
independently. `e2e/tauri-mock.js:33` returns a bare bool and is already in the change map.

**Fork B — updater-window guard. → REVERSED IN r2: codex's marker goes IN. (D18)**

r1 deferred the marker, arguing the heal was safe in the NSIS window by construction. **Codex broke
that claim with a concrete interleaving, and it is correct:**

> NSIS removes installed path `P`. The Run value still targets `P`. A second copy `Q` — a legitimate
> separate copy, e.g. in `~/Downloads` — is launched. Startup reconciliation in `Q` sees `P` broken
> and `Q` live, and writes `Q`. NSIS then restores `P`. Later the user deletes `Q`, and autostart is
> broken.

r1's walk considered only a process starting from the *old* path, never **a process starting from a
different live path**. D1 does not save us here: the entry genuinely *is* broken at that instant,
because NSIS deleted `P` transiently. Codex additionally notes the desired-path existence check and
the registry write are not atomic against NSIS deletion — the same window, as a TOCTOU.

So **the marker protects autostart itself, not merely crash recovery**, and the r1 rationale for
deferring it does not survive. Fable's contradiction-check reached the opposite verdict, but it
worked through the same incomplete case set r1 did; codex's counterexample is strictly more general.

**Design — r4, after both audits rejected the r3 removal rule.** Windows-only: macOS/Linux
`perform_update` holds the lock across `app.restart()`, so they are already covered; the marker exists
*only* because Windows `install()` calls `std::process::exit(0)` and the lock dies with the process.

Written atomically after confirmed disarm, before `install()`, at
`~/.aztec-accelerator/update-in-progress.json`, containing the candidate version, the canonical
expected install path, **the intent snapshot at disarm time**, and **an absolute deadline**. Creation
is **compare-and-create**, never unconditional replacement (D22).

While a live marker exists: no process heals, no process rearms, **and no process starts a new update**
(D22).

**Removal requires all four (r3's three-part rule was circular and unsatisfiable):**

1. Matching candidate version — r1's path-only rule was rejected by codex because on Windows old and
   new run from the *same* install path.
2. Canonical expected install path.
3. **Installer-completion evidence** — a token written by a new production `NSIS_HOOK_POSTINSTALL`
   (D21). *Codex:* once the new exe is copied to `P` it can be launched manually while NSIS is still
   copying other files; it would match version **and** path **and** rearm successfully while the
   install is incomplete, reintroducing the half-installed-launch hazard the disarm exists to prevent.
   **Rearm is not completion evidence.**
4. **Recovery reconciled to current intent** — armed when intent is ON, *confirmed disarmed* when
   intent is OFF. This replaces r3's "successful rearm", which **codex withdrew as wrong when adopted
   literally**: it deadlocked against "no process rearms while a marker is live", and under an OFF
   intent it could never be satisfied at all. **The removal transaction is explicitly exempt from the
   no-rearm rule** — that exemption was the missing piece, and both audits found its absence.

**The TTL is a liveness heuristic, never a safety backstop.** Both audits: an installer hung past the
TTL reopens the exact `P`-absent race. It exists only to recover orphaned/corrupt markers. The deadline
is stored **inside the validated marker**, not derived from file mtime, because mtime is forgeable by
the same user who can write the file (D23) — and a backwards clock must not resurrect an expired one.

**Stranded states, each given a defined exit** (enumerated by both audits): returned install failure
(the writer is N−1 and can never match its own candidate version — so the Err path must remove the
marker under the lock before `CrashRecoveryGuard` rearms); downgrade or manual reinstall at another
version; matching version at another path; transient `schtasks` failure at N's first launch (retry, not
TTL limbo); OFF intent; corrupt or future-dated marker; **and the queued `AztecAccelerator` rename,
which changes the install dir and exe name, so a renamed N runs from a path ≠ the marker's expected
path** — fable's catch, and it would strand every first-rename release. The rename must therefore ship
*after* this, and the marker must treat "expected path absent but a matching-version app is running
from the current install location" as a valid completion.

**Fork C — Linux config dir. → Honour `XDG_CONFIG_HOME`. (D9)**

This fork was never independent: fable was *forced* to mirror `auto-launch`'s hardcoded `$HOME/.config`
only because it kept the plugin, and reading a different file than the plugin writes would be a
correctness bug. D7 frees the choice, and `dirs::config_dir()` is then right on all three counts — the
freedesktop autostart spec, parity with `crash_recovery.rs:246`, and eliminating C8's `.unwrap()`
panic. Conceded by fable. **Test isolation must set both `$HOME` and `XDG_CONFIG_HOME`**, since on a
dev host they coincide and a wrong-directory bug is invisible.

**Fork D — state taxonomy. → Codex's taxonomy, one healing branch. (D10)**

```
Absent      no entry                                → NEVER heal (never resurrect)
Healthy     stored target resolves to an executable → never heal
              └ points_elsewhere: bool  (canonicalized ≠ ours — drives Settings copy ONLY)
Broken      parsed fine, target does not resolve    → THE ONLY HEALABLE STATE
Unreadable  I/O or parse failure                    → never heal, never write
```

### 3.3 Decisions the consolidation made against both legs

| # | Decision | Why |
|---|---|---|
| **D11** | **Windows writes `current_exe()` verbatim, never canonicalized.** Canonicalize only for comparison. | Codex had specified canonicalized, and concedes. Rust's `canonicalize()` can yield an extended-length `\\?\C:\…` path. **Softened in r2:** the plan does *not* claim Explorer rejects such values — Microsoft warns only that shell components *may* not interpret extended paths, and Run values are additionally bounded at 260 chars. Compatibility is **unproven**, which is a reason to avoid it and to prove the written value natively (L6), not a reason to assert failure. |
| **D12** | **The existence precondition applies to the *desired* path, not `current_exe()`.** | Under a Linux AppImage they differ: `tauri/src/process.rs:48-51` prefers `env.appimage`, and `current_exe()` points inside the ephemeral `/tmp/.mount_XXXX` squashfs that vanishes at exit. The plugin already does this (`tauri-plugin-autostart/src/lib.rs:214-222`). Fable conceded this was a real bug in its own leg. |
| **D13** | **Intent ≠ presence. Define `intent_enabled` = artifact present AND not platform-disabled**, and key the startup crash-recovery rearm off *that*. | **Both reviewers flagged r1's "bare presence" as a real contradiction.** On Windows `auto-launch/windows.rs:73-95` combines Run-value presence with `StartupApproved`; bare presence would arm the schtasks relauncher against an explicit user OFF, at every launch and after every update — contradicting §4.5's own "TM-disabled stays disabled". The r1 *motivation* is verified correct: keying the rearm off *health* would stop protecting exactly the Broken-entry users whenever a heal fails. |
| **D14** | **Status carries `can_repair_now`.** | Fable's Fix button would silently no-op when the app was relocated *while running*: macOS `current_exe()` is the exec-time `_NSGetExecutablePath` snapshot, so it goes stale on a move. Both reviewers confirmed. Codex adds: Linux `/proc/self/exe` follows same-filesystem moves, so `can_repair_now` is usually **true** there. |
| **D15** | ~~No `plist` runtime dep~~ → **REVERSED in r2. Use `plist` in production for the macOS reader/writer.** | r1's rationale was **factually false**, and I adopted it without checking. `tauri-utils/Cargo.toml:149` declares `[dependencies.plist] version = "1"` — unconditional, non-optional — and `tauri` depends on `tauri-utils`. `plist 1.8.0` is therefore already compiled in every build on every platform, at exactly the version codex proposed; a direct dependency adds **zero crates**. Codex further notes a hand-rolled scanner would *reject binary plists* and buys byte-for-byte formatting preservation when the actual requirement is only semantic key preservation. Linux `.desktop` and Windows registry stay hand-rolled — small, line-oriented formats with no equivalent parser already present. |
| **D16** | **`prior_enabled` := `intent_enabled`.** | New in r2. `commands.rs:462-464` computes `prior_enabled` from the plugin; under D7 it becomes ours and was undefined. It **must** be intent: if it were the health-aware flag, a `Broken` entry would read `false` and send `enable_transaction` down the full `plugin_enable` path — **recreating the macOS plist and stripping `KeepAlive`, the exact operation C1 forbids.** This makes a dedicated `repair_autostart` *structurally necessary*, not merely nicer UX. |
| **D17** | **The Settings switch reflects intent; a warning row carries health; OFF always works.** | New in r2, from codex. r1 rendered a `Broken` entry as an *unchecked* switch while D13 treated intent as still on — so the user could not turn it OFF (clicking an unchecked switch sends ON), and `can_repair_now:false` offered no action at all. Status honesty must not cost the user their only control. |
| **D18** | **The updater marker is in scope.** | See Fork B. Reversed from r1. |
| **D19** | ~~Every owned mutation takes the *updater* lock~~ → **REVISED in r4: a dedicated, short-lived `autostart.lock` serialises owned autostart mutations.** The updater lock is taken *only* by the heal, and only to bow out. | Codex's r2 finding was right that `set_enabled` and the heal must not interleave; its choice of lock was wrong, and **fable caught the cost neither of us weighed**: `perform_update` holds `updater.lock` across the *entire multi-minute download* (`updater.rs:272`), so a non-blocking acquire makes the Settings toggle hard-fail during any background update, and a blocking one hangs the UI. A dedicated lock is held for microseconds around one read-modify-write. §3.4's rejection of a second lock is reversed accordingly — it was rejected on "another stale-lock failure mode" without weighing this. |
| **D21** | **Add a production `NSIS_HOOK_POSTINSTALL` completion token.** | New in r4, forced by codex's finding 3. `hooks.nsi` today has *only* `NSIS_HOOK_POSTUNINSTALL` (verified, `:44`), so this is new production installer code — the marker cannot be closed without evidence that NSIS actually finished, and no such evidence exists today. |
| **D22** | **`perform_update` becomes marker-aware: reject a live foreign marker under the updater lock; marker creation is compare-and-create.** | New in r4. **Both audits, independently.** The updater lock dies with the exiting process, so a second copy can acquire it and start a *whole concurrent install* inside the very window the marker protects — a strictly worse operation than the heal that criterion 6 forbids. r3 guarded the heal and left the bigger hole open. |
| **D23** | **The marker is owner-private, bounded-read, reparse/symlink-refusing, with its deadline stored inside the validated payload.** | New in r4, from both audits' security notes. A same-user-writable marker is a *renewable suppression* lever for healing and crash recovery; mtime-based expiry is forgeable by exactly the actor who can write it. |
| **D24** | **Redact user paths from the UI and logs.** | New in r4, from codex. `textContent` (F-B2) solves injection, not privacy: `stored_path` and the `from`→`to` `info` log both expose full usernames and home paths, and the §5 UI does not need them. |
| **D20** | **Read each platform's disable override, not just Windows'.** | New in r2, from codex's "resolves ≠ will launch". Linux `.desktop` has `Hidden=true`; macOS launchd has a `Disabled` key. These are the direct analogues of Windows `StartupApproved`, and reading all three makes `intent_enabled` uniform instead of Windows-special. Known limit retained in §10. |

### 3.4 Rejected

| Rejected | Why | Whose |
|---|---|---|
| Diff-based healing (`stored != current_exe()`) | §3.1 D1 — regression worse than the bug | main agent's original framing |
| `manager.enable()` / disable→enable to heal | C1: macOS `enable()` recreates the plist, stripping `KeepAlive`/`ThrottleInterval` | prior rejected rounds |
| `launchctl unload`/`load` after patching | `load` with `RunAtLoad` immediately spawns a second instance | fable — but see the §4.7 gap this leaves |
| Toggle-ON as the repair path | Unledgered fork in r1 (codex proposed it, fable proposed the Fix button). D16 settles it: toggle-ON on a `Broken` entry hits the `prior_enabled` skip. Off→on remains the correct route for `points_elsewhere`, where recreation is desired. | codex |
| `tauri-plugin-single-instance` | App-wide launch-semantics change, out of scope | fable |
| ~~A new dedicated lock~~ | **Un-rejected in r4 (D19).** The r2 reasoning ("`updater.lock` already exists; a second lock adds a second stale-lock failure mode") never weighed that `updater.lock` is held across a multi-minute download, which would break the Settings toggle. A short-lived `autostart.lock` is now the design. | codex, overturned by fable |
| Path-only marker removal | Old and new versions run from the same install path on Windows | r1's own amendment, rejected by codex |
| Permanent fail-closed on a corrupt marker | Bricks crash recovery forever after a catastrophic NSIS failure | codex's original, withdrawn by its author |
| Periodic / filesystem-watcher healing | One-shot per process; the marker (D18) now handles the transient-absence case | fable |
| SMAppService (macOS) | Larger product/distribution change; does not solve the cross-platform problem | codex |
| Migration / legacy-format code | Owner: zero install base | owner, Phase 0 |
| Wine-Rust spike as a merge gate | Recon measured exit 53; a green wine result on `CreateProcess` heuristics would be *misleading* | codex + fable |

---

## 4. Architecture & Implementation

### 4.1 Proposed architecture

One new module, `packages/accelerator/src-tauri/src/autostart.rs`, owning the whole surface the plugin
used to own, split in two layers:

- **Pure layer — not `#[cfg]`-gated.** All three platforms' parsers, serializers and classifiers
  compile and unit-test on Linux CI. The single biggest testability win: today *no* test on *any*
  platform asserts what the app writes to an autostart entry.
- **I/O layer — `#[cfg]`-dispatched.** Thin: locate artifact, read, write atomically.

Matches repo convention §H (pure logic factored out, closure-injected, inline `#[cfg(test)] mod
tests`) — the shape of `enable_transaction`, `systemd_exec_start`, `classify_launch_https`.

### 4.2 Key interfaces

```rust
pub enum StoredTarget {
    Absent,
    Healthy { program: PathBuf, raw: String, points_elsewhere: bool },
    Broken  { raw: String },
    Unreadable { reason: String },
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus {
    pub intent_enabled: bool,    // D13: artifact present AND not platform-disabled
    pub healthy: bool,           // stored target resolves
    pub points_elsewhere: bool,  // resolves, but to another copy — informational only
    pub can_repair_now: bool,    // D14: our desired path resolves, so Fix would succeed
    pub stored_path: Option<String>,
}

pub enum HealOutcome { NotNeeded, Healed { from: String, to: String }, Skipped(&'static str), Failed(String) }

pub fn desired_path(app: &tauri::AppHandle) -> Result<PathBuf, String>;   // D12
pub fn read_stored_target(app: &tauri::AppHandle) -> StoredTarget;
pub fn intent_enabled(app: &tauri::AppHandle) -> Result<bool, String>;    // D13/D20, drives the rearm
pub fn status(app: &tauri::AppHandle) -> Result<AutostartStatus, String>;
pub fn heal_if_broken(app: &tauri::AppHandle) -> HealOutcome;             // takes the lock (D19)
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String>;  // takes the lock (D19)
fn app_name(app: &tauri::AppHandle) -> String;   // D7: the ONE derivation, feeding all three writers

// pure — compiled on every OS, closure-injected `exists` per repo convention:
pub(crate) fn desktop_exec(ini: &str) -> Option<String>;
pub(crate) fn desktop_hidden(ini: &str) -> bool;              // D20
pub(crate) fn desktop_rewrite_exec(ini: &str, new_quoted: &str) -> Option<String>;
pub(crate) fn run_value_candidates(v: &str) -> Vec<String>;   // CreateProcess unquoted search order
pub(crate) fn resolve_first(c: &[String], exists: &dyn Fn(&str) -> bool) -> Option<String>;
pub(crate) fn desktop_quote(s: &str) -> Option<String>;       // freedesktop Exec quoting, incl. %%
pub(crate) fn desktop_unquote(s: &str) -> Option<String>;     // F-B3: decode BEFORE resolving
pub(crate) fn run_value_quote(s: &str) -> Option<String>;     // "…" — §9
pub(crate) fn run_value_unquote(s: &str) -> Option<String>;   // F-B3
```

### 4.3 Desired path, per platform (D11, D12)

| OS | Desired path written | Compared via |
|---|---|---|
| macOS | `current_exe().canonicalize()` — C7; matches `tauri-plugin-autostart/src/lib.rs:202`, which under `MacosLauncher::LaunchAgent` stores the **exe** path, not the `.app` bundle | canonicalized |
| Linux | `app.env().appimage` when present, else `current_exe()` — D12 | canonicalized |
| Windows | `current_exe()` **verbatim** — D11 | canonicalized both sides |

All three must pass, before any write: absolute; valid UTF-8; no control bytes; resolves to a regular
file; and `crash_recovery::autostart_path_is_safe(&exe)` (**C6** — today enforced only at toggle time).

### 4.4 Control flow — `heal_if_broken`, one-shot per process

1. **Windows only:** no live `update-in-progress.json` marker (D18). Else `Skipped("update in progress")`.
2. `desired_path()` → must exist (`try_exists`). Else `Skipped("own path unresolvable")` — the
   relocated-while-running case, which D14 surfaces in the UI.
3. `autostart_path_is_safe(&desired)` — C6. Fail ⇒ `Failed`.
4. `read_stored_target()`, **decoding before resolving** (F-B3). Only `Broken` proceeds.
5. `acquire_updater_lock()` non-blocking — `None` ⇒ `Skipped("updater active")`.
6. Re-read under the lock, then `write_program_path(&desired)` in place.
7. Log the `from`→`to` transition at `info`.

`set_enabled` takes the same lock for its whole read-modify-write (D19).

### 4.5 Writers — in-place, atomic, fail-closed

- **macOS** `~/Library/LaunchAgents/{app_name}.plist` — parse with `plist` (D15), replace
  `ProgramArguments[0]`, write back atomically. `KeepAlive`, `ThrottleInterval`, `RunAtLoad`, `Label`,
  `Disabled` and any unknown keys are preserved **semantically** (`plist::Dictionary` is
  `indexmap`-backed, so key order survives; only whitespace may differ). Handles binary plists for
  free. No `launchctl` — see the §4.7 gap.
- **Linux** `${XDG_CONFIG_HOME:-$HOME/.config}/autostart/{app_name}.desktop` (D9) — rewrite the
  `Exec=` line's program token, preserving args, `Hidden`, and every other line.
- **Windows** `HKCU\…\CurrentVersion\Run` value `{app_name}` — `set_value` to `run_value_quote(path)`.
  **A heal never touches `StartupApproved`**; an explicit ON sets it (matching `auto-launch`'s
  `[0x02,0,…]`), an explicit OFF deletes the Run value only — preserving the crate's exact semantics
  so a Task-Manager-disabled entry stays disabled.

**Behavioural delta to record (fable, r2):** because the heal never reads `StartupApproved`, it *will*
repair a Task-Manager-disabled `Broken` entry's path. That is deliberate and safe — it repairs a
pointer without re-enabling anything, and `intent_enabled` (D13) still reports OFF, so nothing arms.
Fable's leg skipped healing in that case; the difference is now recorded rather than silent.

Linux/macOS writes go through same-dir temp + `fs::rename`, `0600`, refusing a symlinked target.

### 4.6 File-level change map

| File | Change |
|---|---|
| `src-tauri/src/autostart.rs` | **new** — the whole module above |
| `src-tauri/src/update_marker.rs` | **new** (D18) — write / read / expire / remove, Windows-only behaviour, pure state machine. **Its removal call site is `main.rs` startup**, not `updater.rs` (fable: r3's map named no remover at all) |
| `src-tauri/nsis/hooks.nsi` | **(r4, D21)** add a production `NSIS_HOOK_POSTINSTALL` writing the completion token. Today the file has *only* `NSIS_HOOK_POSTUNINSTALL` (`:44`) — verified |
| `src-tauri/tests/autostart_heal.rs` | **new** — `#[ignore]`d real-OS integration (shape copied from `tests/trust_linux.rs`) |
| `e2e-webdriver/autostart.spec.ts` | **new** (L7) |
| `scripts/autostart-test.sh` | **new** — Docker harness, modelled on `nsis-hook-test.sh` |
| `src-tauri/src/commands.rs` | `get_autostart_enabled` → `AutostartStatus`; `set_autostart_inner` drives `autostart::set_enabled` with `prior_enabled := intent_enabled` (D16); **new** `repair_autostart` |
| `src-tauri/src/main.rs` | drop `tauri_plugin_autostart::init` (`:556`) + the `MacosLauncher` import (`:23`); replace `:607-625` with heal-then-rearm; **only the automatic heal is `#[cfg(not(feature = "webdriver"))]`-gated** (S1 — the command path stays live so L7 can exercise it); register `repair_autostart` |
| **`src-tauri/build.rs`** | **(r2, codex)** add `repair_autostart` to the app-manifest command list at `:143`. **ALL-OR-NOTHING** — an omitted command escapes the F-012 ACL |
| **`scripts/tauri-trust-boundary.test.ts`** | **(r2, codex)** add `repair_autostart` to the static command set at `:145` |
| `src-tauri/src/updater.rs` | `acquire_updater_lock` → `pub(crate)`; `:364-365` reads `autostart::intent_enabled`; write/remove the marker around the Windows disarm |
| `src-tauri/src/crash_recovery.rs` | **no logic change** — ungate the pure plist transforms so they unit-test off-macOS |
| `src-tauri/Cargo.toml` | −`tauri-plugin-autostart`; +`winreg` (windows target, replacing the `0.10.1` copy that leaves with `auto-launch`); +`plist` (**runtime**, D15) |
| `src-tauri/capabilities/settings.json` | +`allow-repair-autostart` |
| `src-tauri/frontend-src/settings.{js,html}`, `style.css` | intent switch + health warning row + **Fix** / reopen copy (D17); `stored_path` rendered via **`textContent`, never `innerHTML`** (F-B2) |
| `e2e/tauri-mock.js`, `e2e/settings.spec.ts` | status object; broken-state specs |
| `.github/workflows/accelerator.yml` | bare `cargo test` on the macOS `cert-trust` leg (**D5**); `--test autostart_heal -- --ignored` on all three legs; L6 cleanup (F-B1) |
| `.github/workflows/_e2e-updater-windows.yml` | **(r3)** add `workflow_dispatch` with an in-job N build; switch `workflow_call`'s N−1 to the real `accelerator-v1.0.7`; correct the stale "no Windows STABLE release yet" header |
| `scripts/updater-smoke-windows.ps1` | **(r3)** the L8 barrier scenario: hold after disarm, launch an alternate exe, assert no heal and no rearm, then assert marker clearance |
| `packages/accelerator/package.json` | `test:autostart` script |

### 4.7 Non-obvious mechanics, and one documented gap

**Windows `CreateProcess` search order.** An unquoted Run value containing spaces is tried
prefix-by-prefix: `C:\Users\x\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe` is attempted as
`C:\Users\x\AppData\Local\Aztec.exe` **first**. `run_value_candidates` models this exactly, so
`resolve_first` classifies a legacy unquoted value the way Windows would actually launch it — and the
same table proves §9's finding in a unit test on Linux.

**macOS: the persisted path heals, the loaded job does not — GAP (r2, codex).** r1 claimed patching
`ProgramArguments[0]` "heals crash recovery for free" because `crash_recovery.rs:196-201` and
`auto-launch/macos.rs:184-190` resolve to the same file. **That claim was too strong.** Editing the
plist repairs autostart for the *next* login, but does not update an **already-loaded launchd job**;
`enable_crash_recovery()` then sees `KeepAlive` present and early-returns (C2), so the current
session's job still targets the old executable.

This is a **documented until-next-login gap, not a regression**: recon C2 records that macOS crash
recovery is *already* permanently stale once armed. The plan improves the persisted state and leaves
the session state exactly as it is today. Reloading via `launchctl bootout`/`bootstrap` is rejected
(§3.4) because `RunAtLoad` would immediately spawn a second instance, and booting out a job that may
be supervising us risks terminating the running app.

### 4.8 Trade-offs

- **Owning three readers/writers vs. one dependency.** Cost: ~3 parsers, ~3 serializers, real test
  surface. Bought: the only way to know what is stored, safe serialization at *enable* time, and XDG
  correctness. Net crate count goes **down** (`winreg 0.10.1` leaves; `plist` was already compiled).
- **One-shot at startup vs. continuous.** One-shot keeps the write path trivially auditable.
  Relocation-while-running isn't fixed until next launch — covered by D14's copy.
- **The marker's cost.** A cross-process state file plus a state machine, for a window measured in
  seconds. Accepted because codex's counterexample shows the failure mode is *silent autostart
  breakage* — the exact bug this plan exists to fix.

---

## 5. Status honesty and Settings (D17)

The switch reflects **intent**. Health is a separate, adjacent row. Turning it OFF always works.

| State | Switch | Row |
|---|---|---|
| `Healthy` | on | — |
| `Healthy`, `points_elsewhere` | on | "Start on Login points to another copy of Aztec Accelerator. Turn it off and on to use this one." |
| `Broken`, `can_repair_now` | on + warning styling | "Start on Login points to a file that no longer exists." + **Fix** → `repair_autostart` |
| `Broken`, `!can_repair_now` | on + warning styling | "Start on Login needs repair. Reopen Aztec Accelerator from its new location." (D14) |
| platform-disabled (`StartupApproved` / `Hidden=true` / launchd `Disabled`) | off | — (respect the OS-level OFF; D20) |
| `Unreadable` | disabled | existing "state unavailable" treatment (`commands.rs:52-57`) |

Opening Settings **never writes OS state**. `stored_path` is attacker-influenceable (same-user) and is
rendered with `textContent` (F-B2).

---

## 6. Test plan

**L1 — pure unit, `cargo test`, Linux, every PR.** All three formats' parse + rewrite. macOS: `plist`
round-trip asserting `KeepAlive`/`ThrottleInterval`/unknown keys survive **structurally**, XML and
**binary** plists both, `& < > "` in paths. Linux `Exec=` encode/decode for spaces, quotes,
backslashes, `$`, backticks, `%`→`%%`, Unicode, controls, duplicate `Exec`, field-code injection,
`Hidden=true`. Windows value encoding, trailing-argument rejection, `run_value_candidates` order,
`StartupApproved` fixtures. **Decode-before-resolve (F-B3):** a healthy `&`-containing path must
classify `Healthy`, not `Broken` — otherwise it is rewritten identically every launch while the status
lies. Full injected-`exists` state table: absent, valid symlink, dangling symlink, directory,
non-executable, permission error, malformed, `points_elsewhere`.

**Plus the D18 marker state machine**, exhaustively: every stranded state in §3.2 with its defined
exit, the removal transaction's no-rearm exemption, compare-and-create rejection of a second writer,
future-dated and backwards-clock deadlines, and the `install()` **Err** path removing its own marker
before `CrashRecoveryGuard` rearms (fable: the writer is N−1 and can never satisfy its own
candidate-version match, so without this the failed transaction strands itself). Three scenarios both
audits called out as missing: **update while intent is OFF** (candidate clears the marker *without*
arming recovery), a returned install failure, and a second updater attempt against a live marker.

**L2 — differential oracle.** Structural assertions against `plist`-parsed output (now the production
parser, so the oracle checks *intent*, not self-consistency).

**L3 — real-OS integration, `tests/autostart_heal.rs`, `#[ignore]`d, throwaway `$HOME`.** Enable →
relocate (delete the target) → heal → re-read, on each OS. Wired into the **existing PR-gated
`cert-trust` matrix** (`accelerator.yml:113-136`, three runners, already builds the crate — nearly
free). Windows asserts real registry semantics, `StartupApproved` preservation, quoting. macOS asserts
`KeepAlive` survives a real patch. Same step adds the bare `cargo test` that finally compiles the three
dead `patch_plist_*` tests (**D5**).

**Windows caveat (fable):** a throwaway `$HOME` does **not** isolate `HKCU`, so the Windows leg writes
the real Run hive. These tests must use a uniquely-named value, run serialized (not under `cargo test`'s
default thread parallelism), and clean up panic-safely via an RAII guard — otherwise a failed test
leaves a live autostart entry on the runner.

**L4 — Docker, `scripts/autostart-test.sh`.** `rust:bookworm`, inline heredoc Dockerfile,
`docker run --rm -i` + `bash -s`, covered by `lint:shell`. Runs the Linux leg under hermetic `HOME=/h`
with `XDG_CONFIG_HOME=/xdg` **deliberately divergent**, plus one run with `HOME` unset asserting
`Skipped`, not abort. *The only hermetic proof of D9/C8 — on a dev host the two paths coincide.* Fable
confirmed this is not ceremony. Local-only, like `test:nsis`; **not claimed as a CI job**.

**L5 — Playwright, `e2e/settings.spec.ts`.** Broken-state rows, Fix wiring, `can_repair_now:false`
copy, and that OFF still works from a broken state (D17).

**L6 — Windows Build Smoke extension** (`accelerator.yml:518`, PR-gated). Seed the production Run
value with a missing spaced path; assert it becomes the exactly quoted installed executable.
**Cleanup is mandatory (F-B1):** the healed entry causes the production build to arm a real schtasks
task, and the smoke then `Stop-Process -Force` at `:594` — a "crash" the task would answer by
relaunching the app mid-assertion. Delete both the Run value and the schtasks task **before** the kill.

**Then execute it (r4 — fable's highest-value finding).** L1–L8 all assert stored *bytes*; not one
launches from them, so the entire quoting claim rests on `run_value_candidates` — a model checked
against itself. After the equality assertion, spawn the **raw Run value** via `Win32_Process Create`
(the same `CreateProcess` semantics Run-key processing uses) and health-probe it. D11 explicitly
promises to "prove the written value natively (L6)"; without this step it does not. This is also the
only end-to-end proof of the §9 security fix.

**L7 — WebDriver, `e2e-webdriver/autostart.spec.ts`** (S1; independently named by fable as the
highest-value missing test). Under a throwaway `$HOME`: seed a `Broken` entry, launch the real app,
read status through real IPC, click **Fix**, assert the artifact healed on disk. **This is the only
layer that exercises the real command through the real capability ACL in the real binary** — L5 mocks
both sides, so a forgotten `allow-repair-autostart` grant, a missing `build.rs` manifest entry, or
serde camelCase drift ships green through L1–L6.

**Scope corrected in r4 (fable).** r3 said "Windows never runs WebDriver" — that is true only of the
`built-debug` leg; the dev-mode matrix *does* include `windows-latest` (`accelerator.yml:449-456`).
L7 covers macOS + Linux by **choice**, because a throwaway `$HOME` cannot isolate `HKCU` (same reason
as L3), not because Windows is impossible. L6 carries the Windows end-to-end proof. Two further
interactions the spec must expect: the app's own launch runs the startup rearm, which on macOS
patches the seeded plist's `KeepAlive` *before* Fix is clicked, and on Linux may shell out to a real
`systemctl --user`.

**L8 — native Windows updater-window barrier** (owner-approved scope, r3). Codex's highest-value
missing test, and the direct end-to-end proof of its Fork B counterexample: a real N−1→N update that
stops at a barrier after the disarm, launches an **alternate executable from a different directory**
while the installed target is absent, and asserts it **neither heals nor rearms**; then releases the
barrier and asserts N clears the marker and the Run value still targets the installed exe.

**The barrier mechanism — r4. r3 specified this in `updater-smoke-windows.ps1`, and both audits
independently proved that cannot work:** `perform_update` runs disarm → marker → `install()` within
milliseconds, and `install_inner` hands off to NSIS and calls `std::process::exit(0)`
(`tauri-plugin-updater-2.10.1/src/updater.rs:788-867`). There is no process left for PowerShell to
hold, and a Rust-side pause *before* `install()` leaves `P` present — which does not test the
counterexample at all.

**Adopted mechanism (codex): a test-only sentinel in the synthetic N−1's `NSIS_HOOK_POSTUNINSTALL`.**
The dispatch path builds N−1 itself, so it can inject a hook the production installer never carries.
During an upgrade the new installer invokes the old uninstaller, which removes the old files — so
POSTUNINSTALL is *exactly* the "`P` absent, new files not yet written" state. The hook signals ready,
waits on a release file, and the smoke asserts `P` is absent before launching `Q`. This needs **no
production code**, unlike fable's env-gated-pause-in-`perform_update` alternative, which would put a
test lever in the shipped updater. `hooks.nsi:44` is the existing precedent for hook-driven behaviour
tests.

**The subject must also be asserted not to start its own update (fable).** `Q` is a real app copy
sharing `~/.aztec-accelerator`; the smoke pre-seeds `auto_update:true`
(`updater-smoke-windows.ps1:196`), the feed still serves N, and the first check fires 5 s after launch.
Without D22, `Q` would kick off a *second concurrent NSIS run* inside the barrier — so L8 asserts three
properties, not one: `Q` does not heal, does not rearm, **and does not start an install**. That third
assertion is the regression test for D22.

**Trigger plumbing.** `workflow_dispatch` was *deliberately* removed (recon §E;
`windows-disarm-proof-2026-06-04/plan.md:80-81`) for a structural reason: `n-artifact` defaults to
`accelerator-windows-x86_64`, **an artifact produced by the same run's `build` job**, so standalone the
workflow has nothing to install. The dispatch path must build N from the checked-out ref in-job — which
means **two** full Windows Tauri builds (pubkey-patched N−1 + N) against a `timeout-minutes: 40` budget
sized for one (`_e2e-updater-windows.yml:50`). Raise it, and expect the runtime cost.

**Trigger-split design (r3):**

| Trigger | N | N−1 | Signing key |
|---|---|---|---|
| `workflow_call` (release, unchanged) | this run's `build` artifact | **real `accelerator-v1.0.7`** — replacing the synthetic 0.0.1 | prod |
| `workflow_dispatch` (new — the marker gate) | built in-job from the ref | synthetic | **ephemeral throughout** |

The workflow header's claim *"There is no prior Windows STABLE release to download as N−1 yet"* has
been **false since April** — seven stable releases ship a Windows `setup.exe`
(`accelerator-v1.0.0`…`v1.0.7`) — and the file's own comment says to switch to the real-N−1 pattern
once one exists. Doing it here also delivers the fixture the queued `AztecAccelerator` rename needs.

**Real-N−1 preflight (both audits — r3 called this path "unchanged", which is false; the fixture
changes materially).** Before enabling updates, assert: the installed N−1's `/health` version, with N
strictly greater (F-004's monotonic floor rejects any N ≤ 1.0.7, and the `accelerator-v1.0.7` *tag
source* reads `1.0.7-rc.1` because release builds patch the version in-job — **verified**); that
v1.0.7's embedded pubkey equals the current prod key; that the endpoint host matches what the
redirected feed impersonates; and that the current `SignedEnvelope` v1 / `config_version:1` pre-seed is
accepted. Fail early and loudly — an unverified precondition here turns the release gate red *on
release day*.

**Ephemeral signing, specified properly (codex — r3 under-specified it).** Merely swapping the private
key fails verification, because the current workflow deliberately keeps the committed **production**
pubkey inside N−1 (`_e2e-updater-windows.yml:102-104`). The generated ephemeral **public** key must be
patched into `tauri.conf.json` before building *both* N−1 and N, and the same private key must sign N's
artifact **and** its manifest.

**Security correction (codex).** r3 claimed the trigger split keeps the prod key off the dispatch path.
**That claim was wrong**: `workflow_call.secrets` is not an isolation boundary — repository secrets stay
addressable in a `workflow_dispatch` run of the same YAML, and this file already references the
production key. The real control is structural: put the dispatch and production paths in **separate
jobs**, with the secretless one requesting no production secrets; event-guard every production step;
and protect the production key behind a **release-only environment**. Note also that `contents: read`
prevents publishing a release but does **not** prevent uploading a production-signed Actions artifact.

---

## 7. Phases and gates

| # | Work | Gate |
|---|---|---|
| 1 | `autostart.rs` pure layer + L1/L2 | `cargo test autostart`; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` → **Rust Tests** (`:88`) |
| 2 | Platform readers/writers, `heal_if_broken`, `set_enabled`, `intent_enabled`; remove the plugin; `build.rs` + trust-boundary command sets | `cargo test` on Linux **and** `windows-build` (`:535`); `bun run --cwd packages/accelerator test:unit` |
| 3 | `update_marker.rs` (D18) + **production `NSIS_HOOK_POSTINSTALL` completion token (D21)** + **marker-aware `perform_update` (D22)** + the removal call site in `main.rs` startup (fable: r3's change map had no remover) | `cargo test marker` — the state table must cover every stranded state in §3.2 and show a defined exit; `windows-build` green; `test:nsis` still green with the new hook |
| 4 | `main.rs` call site + `repair_autostart` + `AutostartStatus` + `settings.js` (D17) | `test:e2e:ui` → **Desktop UI Tests** (`:413`); existing `settings.spec.ts:159` still passes |
| 5 | L3 + L7 + CI wiring (incl. the macOS bare `cargo test`) | `cargo test --test autostart_heal -- --ignored` on all three `cert-trust` legs; the macOS log shows `patch_plist_*` **running**; `test:e2e:webdriver` green; `bun run lint:actions` |
| 6 | L4 container harness | `test:autostart` exits 0 locally; `bun run lint:shell` clean |
| 7 | L6 Windows smoke extension **+ cleanup** | **Windows Build Smoke** (`:518`) green, twice consecutively (F-B1 is a flake, so one green proves little) |
| 8 | L8: `workflow_dispatch` path + real-N−1 switch + the barrier scenario | `gh workflow run _e2e-updater-windows.yml --ref <branch>` green on the marker scenario; `bun run lint:actions` clean; the release `workflow_call` path re-validated unchanged |

Full local gate before push: `bun run test` + `bun run lint:actions`.

**Phase 8 is the one phase with no local gate** — it can only be validated by dispatching the workflow
on a pushed branch. Budget for iteration there, and expect the first runs to fail on workflow wiring
rather than on the marker logic.

---

## 8. Assumptions

**Facts (source-verified; both reviewers independently re-verified the r1 set at the cited lines):**
the plugin cannot report the stored path (`tauri-plugin-autostart-2.5.1/src/lib.rs:49-72`); macOS
`enable()` recreates the plist and strips `KeepAlive` (`auto-launch/macos.rs:70-132`); `is_enabled()`
is existence-only on macOS/Linux and existence + `StartupApproved` on Windows (`windows.rs:73-95`);
the plugin writes unquoted on Windows (`windows.rs:37-43`) and unquoted `Exec=` on Linux
(`linux.rs:39`); macOS stores a canonicalized path (`lib.rs:202`); the plugin prefers
`app.env().appimage` on Linux (`lib.rs:214-222`); `acquire_updater_lock` is a real `fs2` cross-process
lock held across the disarm window (`updater.rs:44,272,379,434`); Windows `install()` exits the
process (`updater.rs` comment + `tauri-plugin-updater/src/updater.rs:865`); `productName` is
`"Aztec Accelerator"`; `plist 1.8.0` is already an unconditional dependency via `tauri-utils:149`;
`winreg` is in the lock twice; `build.rs:143` is all-or-nothing for the ACL; no repo test asserts
autostart artifact content; no macOS CI job runs a bare `cargo test`; the frontend never invokes
`plugin:autostart|*`.

**Inferences (challenge these):** Finder's drag-to-`/Applications` is a move, so the old path stops
resolving — the trigger. "Path resolves" means canonicalizable regular executable file. A valid
alternate copy is functional and must not be auto-repointed. An installer-written `POSTINSTALL` token
is trustworthy evidence that NSIS finished. `NSIS_HOOK_POSTUNINSTALL` during an upgrade is a point
where the old target is genuinely absent (this is what L8's barrier rests on).

**Two inferences have now been disproven in review and are no longer load-bearing** — recorded because
the pattern matters more than either instance:

- r1: *"the heal is safe inside the Windows NSIS window"* — disproven by codex's alternate-copy
  interleaving. D18 replaces it.
- r3: *"version + path + successful rearm is sufficient evidence NSIS finished"* — disproven by codex
  (the new exe can be launched while NSIS is still copying other files) and **withdrawn by its own
  author**. D21's completion token replaces it. Likewise r3's *"a file-age TTL is a safe backstop"*:
  it is a liveness heuristic only, and a hung installer past the TTL reopens the very race.

**Asks:** one, **Q1** (§11) — now a scope-size question, not a design question.

---

## 9. Security & Adversarial Considerations

**Live finding, shipped today.** The Windows Run value is written unquoted
(`auto-launch/windows.rs:37-43`): `…\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe` makes
`CreateProcess` try `…\AppData\Local\Aztec.exe` **first** — a user-writable directory under
`installMode: currentUser`. Any same-user process can drop a binary there and be launched at every
login. **Not** a privilege-boundary crossing, so it is a **persistence-hijack primitive, not an EoP**.
Both reviewers independently confirmed this scoping as neither over- nor under-stated. D7 fixes it at
enable time rather than after breakage.

**Never resurrect.** `Absent` and `Unreadable` never heal. Scoped precisely (criterion 4): no Aztec
path can resurrect an entry that was `Absent` at its locked read; D19 makes our own OFF and our own
heal non-interleaving. A foreign deleter racing our locked read→write is out of scope and stated as
such.

**Never widen crash recovery.** The heal touches only the autostart artifact, and D13 keeps the rearm
keyed to *intent*, so an OS-level OFF (`StartupApproved` / `Hidden` / launchd `Disabled`) is never
overridden.

**Injection.** Malformed artifact content must never become plist, `Exec=`, or command-line injection.
Every transform is fail-closed; anything off-shape is `Unreadable`, which never writes.
`autostart_path_is_safe` (C6) now runs on the heal path. No shell is invoked; reads are size-bounded.

**New surface: `stored_path` into the webview (F-B2).** Settings receives a bool today; it will now
receive an attacker-influenceable string. Rendered with `textContent`, never `innerHTML` — the same
treatment the origin rendering received in PR #421.

**TOCTOU / symlink.** Refuse a symlinked plist or `.desktop`; same-dir temp + `rename`, `0600`. The
NSIS-window TOCTOU codex identified is closed by D18.

**Auditability, and privacy (D24, r4).** Every heal logs `from`→`to` at `info` — but codex is right
that `textContent` (F-B2) solves *injection*, not *privacy*. Both `stored_path` in the UI and the
transition log expose full usernames and home paths that §5's copy does not need. Redact to the
basename plus an elided ancestor.

**The marker is a new attack surface (D23, r4 — both audits).** It is same-user-writable and its whole
purpose is to *suppress* healing and crash-recovery rearming, so an attacker who can write it gains
**renewable suppression** of both. This is an **availability** effect, not privilege escalation — the
same actor could simply delete the Run value — and it is stated that way rather than dressed up.
Controls: owner-private ACLs, bounded reads, reparse-point/symlink refusal, defined handling for
future-dated values, and an absolute deadline stored **inside** the validated payload so mtime forging
cannot extend it.

**Least privilege on the new dispatch trigger — r3's claim was WRONG, corrected in r4 (codex).** r3
asserted that using an ephemeral key on the dispatch path keeps the production signing key away from a
manually-triggerable workflow. `workflow_call.secrets` is **not** an isolation boundary: repository
secrets remain addressable in a `workflow_dispatch` run of the same YAML, and this file already
references the production key. The real control is structural — separate jobs, the secretless one
requesting no production secrets, every production step event-guarded, and the production key behind a
release-only environment. `contents: read` blocks publishing a release but not uploading a
production-signed Actions artifact.

---

## 10. Known limits (stated, not solved)

- **"Resolves" ≠ "will launch."** D20 reads the three first-class disable overrides, but desktop
  environments can suppress a `.desktop` in ways we do not model, and launchd has session/domain state
  beyond the `Disabled` key. The taxonomy is about the *stored pointer*, not a launch guarantee.
- **macOS session gap** (§4.7): the loaded launchd job is not reloaded. Repaired at next login.
- **L7 covers macOS + Linux only**; Windows end-to-end rests on L6 + L8.
- **L8 is dispatch-gated, not PR-gated.** It proves the updater-window property on demand, not on
  every PR — *two* full Windows Tauri builds per run is far too heavy for the PR gate. Running it is a
  release-checklist item.
- **The marker is a same-user availability lever** (D23): anyone who can write the user's home can
  suppress healing and crash-recovery rearming until the deadline. No privilege boundary is claimed —
  the same actor could delete the Run value outright.
- **The `AztecAccelerator` rename must ship after this**, not before: it changes the install dir and
  exe name, which is precisely the marker's "expected path" (fable).

---

## 11. Scope questions — resolved by the owner (r3)

**Q1 — the updater marker: IN this PR.** Codex's counterexample proved a heal inside the NSIS window
can silently point autostart at a transient copy — the very failure this plan exists to eliminate — and
the marker is the only available signal, because the updating process has already exited. Owner chose
(a) over shipping with a documented hole. D18 stands; Phase 3 implements it.

**Q2 — the Windows updater E2E gap: add the `workflow_dispatch` path here.** Owner chose to build the
gate now rather than defer it, which also delivers the real-N−1 fixture the queued `AztecAccelerator`
rename needs. Phase 8; design in §6 L8.

No open asks remain.

---

## 12. Revision log

**r2 — post-contradiction-check (codex + fable, in parallel, neither seeing the other).**

Reversed: **Fork B** (marker now in scope — codex's counterexample broke r1's safety claim);
**D15** (`plist` in production — r1's supply-chain rationale was factually false, found by the main
agent via `Cargo.lock`, and independently reaffirmed by codex).

Fixed contradictions: **D13** intent≠presence (*both* reviewers); **D16** `prior_enabled` undefined and
forced to intent by C1 (fable, sharpened during folding); **D17** broken-state UI removed the user's OFF
control (codex); **§4.7** "heals crash recovery for free" was too strong (codex); **§2 criterion 4**
over-claimed (codex); **D19** lock scope (codex).

Added: **D20** per-platform disable overrides (codex); **L7** WebDriver (main agent, confirmed by
fable); **L6 cleanup** (fable — r1 would have shipped a CI flake generator); **F-B2** `textContent`
(fable); **F-B3** decode-before-resolve (fable); `build.rs` + `tauri-trust-boundary.test.ts` +
centralized app-name in the change map (codex).

Ledger honesty: **D4** overstated convergence (fable); the toggle-ON-vs-Fix-button fork was unledgered
(fable); the TM-disabled heal delta is now recorded (fable); r1's Fork B walk was incomplete even where
its conclusion happened to hold (fable) — and then wrong (codex).

**r3 — owner scope decisions.** Q1 → the marker ships in this PR (Phase 3). Q2 → `_e2e-updater-windows.yml`
gains a `workflow_dispatch` path now (Phase 8, **L8**), which also delivers the real-N−1 fixture the
queued `AztecAccelerator` rename needs and corrects a workflow header comment that has been false since
April.

**r4 — double audit (codex: reject, 6 blocking; fable: conditional-approve, 5 blocking; run in
parallel, neither seeing the other).** They converged on the two fatal areas.

*Both, independently:*
- **L8's barrier was not implementable.** `install()` hands off to NSIS and calls `process::exit(0)`,
  so nothing was left for `updater-smoke-windows.ps1` to hold. Replaced with codex's test-only sentinel
  in the synthetic N−1's `NSIS_HOOK_POSTUNINSTALL` — the one point where `P` is genuinely absent, and
  it needs no production code.
- **D18's removal rule was circular** ("no process rearms while live" vs "removal requires a successful
  rearm") and unsatisfiable under an OFF intent. Codex withdrew its own formulation. Now: version +
  path + **installer-completion token** + **recovery reconciled to intent**, with the removal
  transaction explicitly exempt from the no-rearm rule.
- **D22** — `perform_update` was not marker-aware, so a second instance could start a whole concurrent
  install inside the window the marker protects. r3 guarded the heal and left the larger hole open.

*Codex only:* rearm is not proof NSIS finished (→ **D21**, a new production `NSIS_HOOK_POSTINSTALL`
token); TTL is a liveness heuristic, not a safety backstop; the r3 signing-isolation claim in §9 was
**wrong** — `workflow_call.secrets` is not a boundary; ephemeral signing needs the pubkey patched into
both builds; real-N−1 needs a version preflight and the release path is *not* "unchanged"; **D24**
redact user paths.

*Fable only:* **D19 as adopted in r2 would have broken the Settings toggle** — `updater.lock` is held
across a multi-minute download — so a dedicated short-lived `autostart.lock` replaces it, and §3.4's
rejection of a second lock is reversed; L8's own subject would have started a second update mid-barrier;
the change map named **no marker removal call site**; the queued rename would strand every first-rename
marker; L3/L7 on Windows cannot isolate `HKCU` via `$HOME`; L7's Windows exclusion is a choice, not an
impossibility; **and the highest-value missing test — nothing anywhere actually *executes* a healed
entry**, so the quoting fix was proved only against our own model.
