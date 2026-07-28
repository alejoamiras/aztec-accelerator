# Plan — autostart self-heal

`/blueprint deep`. Consolidated from three independent planning legs: `plan-codex.md` (codex,
`xhigh`), `plan-fable.md` (top-tier Claude planning subagent), and the main agent's own draft.
Grounded in `recon.md` (Phase 0.4), whose constraints are cited as **C1**–**C8**.

**Revision 3** — post-contradiction-check, with both owner scope decisions folded (§11). See §12 for
the change log. Status: **draft — pre-double-audit.** Not approved.

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

**Design (both reviewers' corrections folded).** Windows-only — macOS/Linux `perform_update` holds
the lock across `app.restart()`, so they are already covered; the marker exists *only* because
Windows `install()` calls `std::process::exit(0)` and the lock dies with the process.

- Written atomically after confirmed disarm, before `install()`, at
  `~/.aztec-accelerator/update-in-progress.json` (renamed from codex's `update-disarmed.json`, since
  it now guards two things). Contains the candidate version and the canonical expected install path.
- While a marker is live, **no process heals and no process rearms**.
- **Removal requires all three:** matching candidate version **AND** canonical expected install path
  **AND** a successful rearm. r1 proposed path-only removal; codex correctly rejected it — on Windows
  the old and new versions normally run from the *same* install path, so a surviving old process could
  remove the marker mid-NSIS.
- **Corrupt or stale markers expire** on filesystem age against a conservative TTL. Codex's original
  "corrupt markers fail closed" is withdrawn by its own author — permanent fail-closed bricks crash
  recovery forever after a catastrophic NSIS failure.

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
| **D19** | **Every owned mutation takes the same lock, `set_enabled` included.** | New in r2, from codex. r1 specified the lock only around the heal. Without it, another instance can process an OFF between our locked read and our `rename`/`set_value`, and the heal resurrects the entry — violating success criterion 4. |
| **D20** | **Read each platform's disable override, not just Windows'.** | New in r2, from codex's "resolves ≠ will launch". Linux `.desktop` has `Hidden=true`; macOS launchd has a `Disabled` key. These are the direct analogues of Windows `StartupApproved`, and reading all three makes `intent_enabled` uniform instead of Windows-special. Known limit retained in §10. |

### 3.4 Rejected

| Rejected | Why | Whose |
|---|---|---|
| Diff-based healing (`stored != current_exe()`) | §3.1 D1 — regression worse than the bug | main agent's original framing |
| `manager.enable()` / disable→enable to heal | C1: macOS `enable()` recreates the plist, stripping `KeepAlive`/`ThrottleInterval` | prior rejected rounds |
| `launchctl unload`/`load` after patching | `load` with `RunAtLoad` immediately spawns a second instance | fable — but see the §4.7 gap this leaves |
| Toggle-ON as the repair path | Unledgered fork in r1 (codex proposed it, fable proposed the Fix button). D16 settles it: toggle-ON on a `Broken` entry hits the `prior_enabled` skip. Off→on remains the correct route for `points_elsewhere`, where recreation is desired. | codex |
| `tauri-plugin-single-instance` | App-wide launch-semantics change, out of scope | fable |
| A new dedicated `desktop-state.lock` | `updater.lock` already exists and is reviewed; a second lock adds a second stale-lock failure mode | codex |
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
| `src-tauri/src/update_marker.rs` | **new** (D18) — write / read / expire / remove, Windows-only behaviour, pure state machine |
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
non-executable, permission error, malformed, `points_elsewhere`. Plus the D18 marker state machine,
every early return, including expiry.

**L2 — differential oracle.** Structural assertions against `plist`-parsed output (now the production
parser, so the oracle checks *intent*, not self-consistency).

**L3 — real-OS integration, `tests/autostart_heal.rs`, `#[ignore]`d, throwaway `$HOME`.** Enable →
relocate (delete the target) → heal → re-read, on each OS. Wired into the **existing PR-gated
`cert-trust` matrix** (`accelerator.yml:113-136`, three runners, already builds the crate — nearly
free). Windows asserts real registry semantics, `StartupApproved` preservation, quoting. macOS asserts
`KeepAlive` survives a real patch. Same step adds the bare `cargo test` that finally compiles the three
dead `patch_plist_*` tests (**D5**).

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

**L7 — WebDriver, `e2e-webdriver/autostart.spec.ts`** (S1; independently named by fable as the
highest-value missing test). Under a throwaway `$HOME`: seed a `Broken` entry, launch the real app,
read status through real IPC, click **Fix**, assert the artifact healed on disk. **This is the only
layer that exercises the real command through the real capability ACL in the real binary** — L5 mocks
both sides, so a forgotten `allow-repair-autostart` grant, a missing `build.rs` manifest entry, or
serde camelCase drift ships green through L1–L6. Covers **macOS + Linux only**: Windows never runs
`built-debug` WebDriver (recon §E), so L6 carries the Windows end-to-end proof.

**L8 — native Windows updater-window barrier** (owner-approved scope, r3). Codex's highest-value
missing test, and the direct end-to-end proof of its Fork B counterexample: a real N−1→N update that
stops at a barrier after the disarm, launches an **alternate executable from a different directory**
while the installed target is absent, and asserts it **neither heals nor rearms**; then releases the
barrier and asserts N clears the marker and the Run value still targets the installed exe.

This requires giving `_e2e-updater-windows.yml` a `workflow_dispatch` path. That trigger was
*deliberately* removed (recon §E; `windows-disarm-proof-2026-06-04/plan.md:80-81`), and the reason is
structural, not arbitrary: the workflow's `n-artifact` input defaults to `accelerator-windows-x86_64`,
**an artifact produced by the same run's `build` job**, so standalone it has nothing to install. The
dispatch path must therefore build N from the checked-out ref in-job.

**Trigger-split design (r3):**

| Trigger | N | N−1 | Signing key |
|---|---|---|---|
| `workflow_call` (release, unchanged) | this run's `build` artifact | **real `accelerator-v1.0.7`** — replacing the synthetic 0.0.1 | prod |
| `workflow_dispatch` (new — the marker gate) | built in-job from the ref | synthetic | **ephemeral throughout** |

Two things fall out of this. First, the workflow header's claim *"There is no prior Windows STABLE
release to download as N−1 yet"* has been **false since April** — seven stable releases ship a Windows
`setup.exe` (`accelerator-v1.0.0`…`v1.0.7`) — and the file's own comment says to switch to the real-N−1
pattern once one exists. Doing it here also delivers the fixture the queued `AztecAccelerator` rename
already needs, so it is built once rather than twice.

Second, and deliberately: the dispatch path uses an **ephemeral key for both ends**, never the prod
updater key. Real-N−1 forces prod signing because `v1.0.7` embeds the committed prod pubkey — but the
marker test does not need real-signature verification, only real updater-window behaviour. Keeping the
prod key exclusively on the release path means a manually-triggerable workflow can never be induced to
produce a prod-signed artifact (§9).

---

## 7. Phases and gates

| # | Work | Gate |
|---|---|---|
| 1 | `autostart.rs` pure layer + L1/L2 | `cargo test autostart`; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` → **Rust Tests** (`:88`) |
| 2 | Platform readers/writers, `heal_if_broken`, `set_enabled`, `intent_enabled`; remove the plugin; `build.rs` + trust-boundary command sets | `cargo test` on Linux **and** `windows-build` (`:535`); `bun run --cwd packages/accelerator test:unit` |
| 3 | `update_marker.rs` + updater integration (D18) | `cargo test marker`; `windows-build` green |
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
alternate copy is functional and must not be auto-repointed. Matching candidate version **and**
canonical expected path **and** a successful rearm is sufficient evidence NSIS finished. A conservative
file-age TTL is a safe backstop for a corrupt marker.

*(r1's weakest inference — "the heal is safe inside the Windows NSIS window" — was **disproven** by
codex and is no longer load-bearing; D18 replaces it.)*

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

**Auditability.** Every heal logs `from`→`to` at `info`.

**Least privilege on the new dispatch trigger (r3).** Making `_e2e-updater-windows.yml`
manually-dispatchable widens *who can trigger a workflow that signs updater artifacts*. The dispatch
path therefore uses an **ephemeral key for both N and N−1** and never requests
`TAURI_SIGNING_PRIVATE_KEY`; the prod key stays bound to the `workflow_call` release path. A
manually-triggered run can never emit a prod-signed artifact.

---

## 10. Known limits (stated, not solved)

- **"Resolves" ≠ "will launch."** D20 reads the three first-class disable overrides, but desktop
  environments can suppress a `.desktop` in ways we do not model, and launchd has session/domain state
  beyond the `Disabled` key. The taxonomy is about the *stored pointer*, not a launch guarantee.
- **macOS session gap** (§4.7): the loaded launchd job is not reloaded. Repaired at next login.
- **L7 covers macOS + Linux only**; Windows end-to-end rests on L6 + L8.
- **L8 is dispatch-gated, not PR-gated.** It proves the updater-window property on demand, not on
  every PR — a full Windows Tauri build per run is too heavy for the PR gate. Running it is a
  release-checklist item.

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
April. Added the least-privilege split that keeps the prod updater signing key off the manually
triggerable path (§9), and the note that Phase 8 has no local gate.
