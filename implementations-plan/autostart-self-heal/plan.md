# Plan — autostart self-heal

`/blueprint deep`. Consolidated from three independent planning legs: `plan-codex.md` (codex,
`xhigh`), `plan-fable.md` (top-tier Claude planning subagent), and the main agent's own draft.
Grounded in `recon.md` (Phase 0.4), whose constraints are cited as **C1**–**C8**.

Status: **draft — pre-contradiction-check.** Not approved.

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
2. Settings never shows ON for an entry that will not launch. When it cannot self-repair, it says so
   and offers the action that will.
3. A freshly-enabled Windows entry is **quoted** (§9 — today's unquoted value is a live same-user
   persistence hijack).
4. The heal can never *create* an autostart entry, on any path, in any state.
5. Autostart artifact content is asserted by tests — on all three OSes — where today **nothing is**.

---

## 3. Decision ledger

The three legs converged on far more than they disputed. Convergence is recorded because two
independent agents reaching the same answer without seeing each other is the strongest signal this
protocol produces.

### 3.1 Converged (independent agreement, adopted)

| # | Decision | Source |
|---|---|---|
| **D1** | **Heal iff the stored target does not resolve.** Never "differs from my `current_exe()`." | codex + fable, independently — both rejected the main agent's framing |
| **D2** | Never call the plugin's `enable()` in a heal; patch the artifact **in place** | codex + fable |
| **D3** | `get_autostart_enabled` returns a **structured status**, not a bare bool | codex + fable |
| **D4** | Own the per-platform readers/writers; atomic same-dir temp + `rename`; quote and escape everything | codex + fable |
| **D5** | Add a bare `cargo test` to a macOS CI leg — recon's **BONUS BUG** (no macOS job compiles `#[cfg(test)]`, so three `patch_plist_*` tests have never run anywhere) | codex + fable |
| **D6** | **Drop the wine-Rust spike as a merge gate** (recon measured exit 53) | codex + fable, same reasoning: `windows-latest` already tests production code faithfully |

**D1 is the load-bearing decision.** The main agent's diff-based framing creates a regression worse
than the bug: a leftover copy in `~/Downloads`, double-clicked once, repoints a healthy
`/Applications` entry at a file the user is about to delete. Resolve-based healing is a strict
repair — it only ever replaces a *dead* pointer with a *live* one. It is also **convergent**: every
writer writes a path that exists, so racing writers agree, which dissolves the flip-flop race
(recon §G) with no new coordination primitive.

It works for the target case precisely because Finder's drag is a **move** — the old path stops
resolving. If the user *copies* instead, both exist, nothing heals, and autostart still launches a
working app. Correct outcome, no write.

### 3.2 Forks resolved

**Fork A — keep the plugin (fable) vs remove it entirely (codex). → Remove. (D7)**

Codex's framing wins, but on an argument neither leg made explicitly:

> **The plugin's `enable()` *is* the unsafe serializer.** `auto-launch-0.5.0/src/windows.rs:37-43`
> writes `format!("{} {}", app_path, args)` — unquoted. Keeping the plugin for enable means the
> unquoted Windows Run value is written **at enable time, on every fresh install**, and healing only
> repairs it after the path has already broken. Fable's own security finding (§9) is therefore only
> half-fixed by fable's own plan.

Everything else follows: we must own three readers anyway (C: the plugin cannot report the stored
path — `AutoLaunchManager` exposes only `enable`/`disable`/`is_enabled`, and the in-memory
`get_app_path()` is rebuilt from `current_exe()` every launch and never forwarded). Once the readers
and safe writers are ours, what remains of the plugin is exactly: unsafe serialization, existence-only
status, and `dirs::home_dir().unwrap()` on Linux (C8). Verified cost of removal: **5 call sites**
(`commands.rs:51,55`, `commands.rs:445-446`, `main.rs:556`, `main.rs:609,614`), plus
`updater.rs:362-371`. The frontend never invokes `plugin:autostart|*` — grep-confirmed — and the
capability files already grant no autostart permissions (`capabilities/settings.json:4`), so removal
is **frontend-invisible** apart from the new status shape.

**Fork B — updater-window guard: non-blocking lock (fable) vs `update-disarmed.json` marker (codex).
→ Take the lock; explicitly do NOT claim it closes the Windows gap; defer the marker. (D8)**

Both legs are partly right, and the consolidated position disagrees with both.

- Fable is right that the primitive exists: recon §F's "no instance-coordination primitive exists" is
  **false**. `updater.rs:44 acquire_updater_lock()` is a real cross-process `fs2` exclusive lock, held
  from `:272` across the Windows disarm at `:379` through `app.restart()` at `:434`.
- Codex is right that **the lock provably dies on Windows** — the only platform with a disarm window.
  `tauri-plugin-updater`'s `install()` calls `std::process::exit(0)` after dispatching the installer,
  and the repo already knows it: `updater.rs` comments *"Windows never reaches here — install()
  dispatched the installer and exited the process."* The lock releases at process exit while NSIS
  keeps mutating files.
- **But the marker is fixing a different bug.** What is hazardous inside that window is the
  **crash-recovery rearm** at `main.rs:607-625` — a *pre-existing* defect (recon §G), untouched by
  this plan. The **heal itself is safe in that window** by construction: it writes only a path that
  (a) exists at write time and (b) belongs to a live process, and it is convergent. Walking the
  Windows update sequence: a process starting mid-NSIS from the old exe finds the entry still
  resolving (no heal); once NSIS has removed the old exe, no process can start from it at all.

So: promote `acquire_updater_lock` to `pub(crate)`, take it non-blocking around the read-modify-write
(genuine serialization on macOS/Linux, cheap everywhere), and **state plainly that it does not close
the Windows post-`install()` gap**. Codex's marker is the right fix for the crash-recovery rearm race
and is recorded as **scope question Q1** (§11) rather than silently absorbed or silently dropped.

Note also that codex's "corrupt markers fail closed" is a permanent brick: a catastrophic NSIS failure
would leave crash recovery disarmed forever. If Q1 is taken, the removal rule must be *"any process
whose own path canonicalizes to the marker's expected install path may remove it"* — not
version-matching — plus a TTL backstop.

**Fork C — Linux config dir: mirror `auto-launch`'s hardcoded `$HOME/.config` (fable) vs honour
`XDG_CONFIG_HOME` (codex). → Honour XDG. (D9)**

This fork was never independent: fable was *forced* to mirror the hardcoded path only because it kept
the plugin, and reading a different file than the plugin writes would be a correctness bug. Removing
the plugin (D7) frees the choice, and then `dirs::config_dir()` is right on all three counts — it
matches the freedesktop autostart spec, it matches what `crash_recovery.rs:246` already does for the
systemd unit, and it removes C8's `.unwrap()` panic. **Test isolation must set both `$HOME` and
`XDG_CONFIG_HOME`**, since on a normal dev host they coincide and a wrong-directory bug is invisible.

**Fork D — state taxonomy. → Codex's sharper taxonomy, collapsed to one healing branch. (D10)**

```
Absent      no entry                              → NEVER heal (never resurrect)
Healthy     stored target resolves to an executable → never heal
              └ points_elsewhere: bool  (canonicalized ≠ ours — drives Settings copy ONLY)
Broken      parsed fine, target does not resolve   → THE ONLY HEALABLE STATE
Unreadable  I/O or parse failure                   → never heal, never write
```

Codex named `healthy-different` explicitly ("a healthy-different entry is not silently stolen by
whichever copy launched last"); fable implied it via D1 without naming it. Adopted as a **flag on
`Healthy`, not a fifth state** — it changes no behaviour, only what Settings says.

### 3.3 Decisions the consolidation made against both legs

| # | Decision | Why |
|---|---|---|
| **D11** | **Windows writes `current_exe()` verbatim, never canonicalized.** Canonicalize only for comparison. | Codex specified "canonicalized `current_exe()`" for Windows. On Windows `canonicalize()` yields an extended-length `\\?\C:\…` path; writing that into the Run key is not a value Explorer should be handed. `current_exe()` is already absolute. |
| **D12** | **The existence precondition applies to the *desired* path, not `current_exe()`.** | Fable's step 1 checks `current_exe()`. Under a Linux AppImage those differ: `tauri/src/process.rs:48-51` `current_binary()` prefers `env.appimage`, and `current_exe()` points inside the ephemeral `/tmp/.mount_XXXX` squashfs that vanishes at exit. Checking the wrong one would let the heal write a path guaranteed to break. (The plugin itself already uses `app.env().appimage` — `tauri-plugin-autostart/src/lib.rs:214-222` — so this is matching existing behaviour, not inventing it.) |
| **D13** | **The startup crash-recovery rearm keys off entry *presence* (intent), not health.** | With D3, a `Broken` entry reports `enabled: false`. Naïvely feeding that to `main.rs:614` would **stop** rearming crash recovery for exactly the users whose entry is broken — a regression introduced by the honesty fix. Rearm iff the entry is present; `Unreadable` warns and skips, matching today's `Err` branch byte-for-byte. |
| **D14** | **Status carries `can_repair_now`.** | Fable correctly identifies relocation-*while-running* as the common case, and correctly reports it honestly — but its **Fix** button would silently no-op there, because our own desired path doesn't resolve either (macOS `_NSGetExecutablePath` returns the original, now-dead path). The UI must show **Fix** only when repair can actually succeed, and otherwise *"Reopen Aztec Accelerator from its new location to repair."* |
| **D15** | **No `plist` runtime dependency; hand-rolled fail-closed transform, with `plist` as a *dev*-dependency used as a differential oracle in tests.** | Codex wanted `plist = "1.8"` in production; fable refused any new dep (supply-chain surface, and `crash_recovery.rs:168-183 patch_plist_with_keepalive` is existing precedent). Split the difference: no new runtime attack surface, but a real independent parser asserting in tests that what we write is valid plist and round-trips — which is exactly the oracle that catches escaping bugs, and which neither leg had. |

### 3.4 Rejected

| Rejected | Why | Whose |
|---|---|---|
| Diff-based healing (`stored != current_exe()`) | §3.1 D1 — regression worse than the bug | main agent's original framing |
| `manager.enable()` / disable→enable to heal | C1: macOS `enable()` recreates the plist from scratch, stripping `KeepAlive`/`ThrottleInterval` | prior rejected rounds |
| `launchctl unload`/`load` after patching | `load` with `RunAtLoad` immediately spawns a second instance | fable |
| `tauri-plugin-single-instance` | App-wide launch-semantics change, out of scope, and unnecessary once healing is convergent | fable |
| A new dedicated `desktop-state.lock` | `updater.lock` already exists, is reviewed, and adding a second lock adds a second stale-lock failure mode | codex |
| Periodic / filesystem-watcher healing | One-shot per process is what makes flip-flop structurally impossible | fable |
| SMAppService (macOS) | Larger product/distribution change; does not solve the cross-platform problem | codex |
| Migration / legacy-format code | Owner: zero install base | owner, Phase 0 |
| Wine-Rust spike as a merge gate | Recon measured exit 53; `windows-latest` tests production code faithfully, and a green wine result on `CreateProcess` heuristics would be *misleading* | codex + fable |

---

## 4. Architecture & Implementation

### 4.1 Proposed architecture

One new module, `packages/accelerator/src-tauri/src/autostart.rs`, owning the whole surface the
plugin used to own. Split in two layers:

- **Pure layer — not `#[cfg]`-gated.** All three platforms' parsers, serializers and classifiers
  compile and unit-test on Linux CI. This is the single biggest testability win: today *no* test on
  *any* platform asserts what the app writes to an autostart entry.
- **I/O layer — `#[cfg]`-dispatched.** Thin: locate artifact, read bytes, write bytes atomically.

Matches repo convention §H (pure logic factored out, closure-injected, inline `#[cfg(test)] mod
tests`) — the same shape as `enable_transaction`, `systemd_exec_start`, `classify_launch_https`.

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
    pub enabled: bool,           // entry present AND target resolves
    pub broken: bool,            // entry present, target does not resolve
    pub points_elsewhere: bool,  // resolves, but to another copy — informational only
    pub can_repair_now: bool,    // D14: our desired path resolves, so Fix would succeed
    pub stored_path: Option<String>,
}

pub enum HealOutcome { NotNeeded, Healed { from: String, to: String }, Skipped(&'static str), Failed(String) }

pub fn desired_path(app: &tauri::AppHandle) -> Result<PathBuf, String>;  // D12
pub fn read_stored_target(app: &tauri::AppHandle) -> StoredTarget;
pub fn status(app: &tauri::AppHandle) -> Result<AutostartStatus, String>;
pub fn heal_if_broken(app: &tauri::AppHandle) -> HealOutcome;
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String>;  // replaces the plugin
pub fn entry_present(app: &tauri::AppHandle) -> Result<bool, String>;             // D13, drives the rearm

// pure — compiled on every OS, closure-injected `exists` per repo convention:
pub(crate) fn plist_program(xml: &str) -> Option<String>;
pub(crate) fn plist_rewrite_program(xml: &str, new_escaped: &str) -> Option<String>;
pub(crate) fn plist_render_fresh(label: &str, program: &str) -> String;
pub(crate) fn desktop_exec(ini: &str) -> Option<String>;
pub(crate) fn desktop_rewrite_exec(ini: &str, new_quoted: &str) -> Option<String>;
pub(crate) fn run_value_candidates(v: &str) -> Vec<String>;   // CreateProcess unquoted search order
pub(crate) fn resolve_first(c: &[String], exists: &dyn Fn(&str) -> bool) -> Option<String>;
pub(crate) fn xml_escape(s: &str) -> String;                  // & < > " '
pub(crate) fn desktop_quote(s: &str) -> Option<String>;       // freedesktop Exec quoting, incl. %%
pub(crate) fn run_value_quote(s: &str) -> Option<String>;     // "…" — §9
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

1. `desired_path()` → must exist on disk (`try_exists`). Else `Skipped("own path unresolvable")`.
   *(This is the relocated-while-running case: correct to skip, and D14 makes the UI say so.)*
2. `autostart_path_is_safe(&desired)` — C6. Fail ⇒ `Failed`.
3. `read_stored_target()`. Only `Broken` proceeds; `Absent`/`Healthy`/`Unreadable` return
   `NotNeeded`/`Skipped`.
4. `acquire_updater_lock()` non-blocking — `None` ⇒ `Skipped("updater active")`. **Does not close the
   Windows post-`install()` gap** (D8 / Q1).
5. Re-read under the lock, then `write_program_path(&desired)` in place.
6. Log the `from`→`to` transition at `info` — an unexplained startup-entry rewrite must be auditable.

### 4.5 Writers — in-place, atomic, fail-closed

- **macOS** `~/Library/LaunchAgents/Aztec Accelerator.plist` — locate `<key>ProgramArguments</key>` →
  next `<array>` → first `<string>…</string>`, replace its contents with `xml_escape(path)`.
  `KeepAlive`, `ThrottleInterval`, `RunAtLoad`, `Label` and any unknown keys stay **byte-identical**.
  No `launchctl`.
- **Linux** `${XDG_CONFIG_HOME:-$HOME/.config}/autostart/Aztec Accelerator.desktop` (D9) — rewrite the
  `Exec=` line's program token, preserving args and every other line.
- **Windows** `HKCU\…\CurrentVersion\Run` value `Aztec Accelerator` — `set_value` to
  `run_value_quote(path)`. Registry writes are atomic. **A heal never touches `StartupApproved`**; an
  explicit ON sets it (matching `auto-launch`'s `[0x02,0,…]`), and an explicit OFF deletes the Run
  value only — preserving the crate's exact semantics so a Task-Manager-disabled entry stays disabled.

macOS/Linux writes go through same-dir temp + `fs::rename`, `0600`, refusing a symlinked target.
Atomic rename is the real serialization answer to concurrent healers and closes a TOCTOU/symlink-swap
on a user-writable dotfile path.

### 4.6 File-level change map

| File | Change |
|---|---|
| `src-tauri/src/autostart.rs` | **new** — the whole module above |
| `src-tauri/tests/autostart_heal.rs` | **new** — `#[ignore]`d real-OS integration (shape copied from `tests/trust_linux.rs`) |
| `scripts/autostart-test.sh` | **new** — Docker harness, modelled line-for-line on `nsis-hook-test.sh` |
| `src-tauri/src/commands.rs` | `get_autostart_enabled` → `AutostartStatus`; `set_autostart_inner` drives `autostart::set_enabled`; **new** `repair_autostart` |
| `src-tauri/src/main.rs` | drop `tauri_plugin_autostart::init` (`:556`) and the `MacosLauncher` import (`:23`); replace `:607-625` with heal-then-rearm, `#[cfg(not(feature = "webdriver"))]`-gated (recon §G: today's block is ungated and runs in E2E — gate the new work rather than inherit the defect); register `repair_autostart` |
| `src-tauri/src/updater.rs` | `acquire_updater_lock` → `pub(crate)`; `:362-371` reads `autostart::entry_present` instead of the plugin |
| `src-tauri/src/crash_recovery.rs` | **no logic change** — ungate the pure plist transforms so they unit-test off-macOS |
| `src-tauri/Cargo.toml` | −`tauri-plugin-autostart`; +`winreg` (windows target); +`plist` (**dev**-dependency, D15) |
| `src-tauri/capabilities/settings.json` | +`allow-repair-autostart` |
| `frontend-src/settings.{js,html}`, `style.css` | broken-state row + **Fix** / reopen copy (D14) |
| `e2e/tauri-mock.js`, `e2e/settings.spec.ts` | status object; broken-state specs |
| `.github/workflows/accelerator.yml` | bare `cargo test` on the macOS `cert-trust` leg (**D5**); `--test autostart_heal -- --ignored` on all three legs |
| `packages/accelerator/package.json` | `test:autostart` script |

### 4.7 Non-obvious mechanics

**Windows `CreateProcess` search order.** An unquoted Run value containing spaces is tried
prefix-by-prefix. `C:\Users\x\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe` is attempted as
`C:\Users\x\AppData\Local\Aztec.exe` **first**. `run_value_candidates` models this exactly, so
`resolve_first` classifies a legacy unquoted value the same way Windows would actually launch it —
and the same table is what proves §9's finding in a unit test on Linux.

**Why the macOS plist patch heals crash recovery for free.** `crash_recovery.rs:196-201` and
`auto-launch/macos.rs:184-190` resolve to the **same file**. `ProgramArguments[0]` is shared by
`RunAtLoad` (autostart) and `KeepAlive` (crash recovery). Patching that one string fixes recon's
constraint-2 staleness — macOS `enable_crash_recovery()` early-returns on `KeepAlive` presence and
never compares the path — with **zero edits to `crash_recovery.rs`**. It is the same byte.

### 4.8 Trade-offs

- **Owning three readers/writers vs. one dependency.** Cost: ~3 parsers, ~3 serializers, real test
  surface. Bought: the only way to know what is stored (§3.2 Fork A), safe serialization at *enable*
  time, and XDG correctness. Mitigated by D15's differential oracle.
- **Heal-then-report vs. report-only.** Report-only is a trap: the user toggles ON,
  `set_autostart_inner` reads `prior_enabled = true`, `enable_transaction` correctly *skips*
  `plugin_enable` (C1), and nothing changes — a silent dead end. Hence a **distinct**
  `repair_autostart`, not `set_autostart(true)`.
- **One-shot at startup vs. continuous.** One-shot is what makes flip-flop structurally impossible.
  Cost: relocation-while-running isn't fixed until the next launch — covered by D14's copy.

---

## 5. Status honesty and Settings

`enabled = entry_present && target_resolves`. `Broken` ⇒ `enabled:false, broken:true`. `Unreadable`
still `Err`s, preserving the codex-#7 behaviour at `commands.rs:52-57` (the switch stays disabled on
unknown state). Opening Settings **never writes OS state**.

| State | Switch | Copy |
|---|---|---|
| `Healthy` | checked | — |
| `Healthy`, `points_elsewhere` | checked | "Start on Login points to another copy of Aztec Accelerator. Turn it off and on to use this one." |
| `Broken`, `can_repair_now` | unchecked | "Start on Login points to a file that no longer exists." + **Fix** → `repair_autostart` |
| `Broken`, `!can_repair_now` | unchecked | "Start on Login needs repair. Reopen Aztec Accelerator from its new location." (D14) |
| `Unreadable` | disabled | existing "state unavailable" treatment |

---

## 6. Test plan

The bar the owner set: *"a lot of tests… test on Docker too… professional."* Layered so each layer
catches something the others structurally cannot.

**L1 — pure unit, `cargo test`, Linux, every PR.** All three formats' parse + rewrite. Plist with
nested dicts and `KeepAlive` present (assert `KeepAlive` survives **byte-identically**); `& < > "` in
paths round-trip; malformed arrays fail closed. Linux `Exec=` encode/decode for spaces, quotes,
backslashes, `$`, backticks, `%`→`%%`, Unicode, controls, duplicate `Exec` fields, field-code
injection. Windows value encoding, trailing-argument rejection, and `run_value_candidates` yielding
the documented `CreateProcess` order. Injected `exists` closure ⇒ the full state table: absent, valid
symlink, dangling symlink, directory, non-executable file, permission error, malformed artifact,
`points_elsewhere`. *Catches every parser/serializer defect, on every OS, in milliseconds. **Nothing
today tests any of this.***

**L2 — differential oracle (D15).** Every plist our writer emits is parsed by the real `plist` crate
(dev-dep) and asserted structurally equal to intent. *Catches escaping bugs a self-consistent
hand-rolled round-trip cannot.*

**L3 — real-OS integration, `tests/autostart_heal.rs`, `#[ignore]`d, throwaway `$HOME`.** Exact
`trust_linux.rs` shape. Enable → relocate (delete the target) → heal → re-read, on each OS. Wired into
the **existing PR-gated `cert-trust` matrix** (`accelerator.yml:113-136`, all three runners, already
builds the crate on each — so this is nearly free). Windows asserts real registry semantics including
`StartupApproved` preservation and quoting. macOS asserts `KeepAlive` survives a real patch. The same
step adds the bare `cargo test` that finally compiles the three dead `patch_plist_*` tests (**D5**).

**L4 — Docker, `scripts/autostart-test.sh`.** `rust:bookworm`, inline heredoc Dockerfile,
`docker run --rm -i` + `bash -s` — modelled on `nsis-hook-test.sh`, auto-covered by `lint:shell`.
Runs the Linux leg under a hermetic `HOME=/h` with `XDG_CONFIG_HOME=/xdg` **deliberately different**.
*Catches D9/C8: a heal that silently watches the wrong directory. On a dev host the two paths
coincide, so only a container proves it.* Also runs once with `HOME` unset, asserting `Skipped`, not
abort — the `.unwrap()` panic C8 names. Local-only, exactly like `test:nsis`; **not claimed as a CI
job**.

**L5 — Playwright, `e2e/settings.spec.ts`.** Mocked `{enabled:false, broken:true, canRepairNow:true}`
⇒ warning row visible, switch unchecked, **Fix** invokes `repair_autostart`; and
`canRepairNow:false` ⇒ reopen copy, no Fix button. *Catches the UI lying, which is half the reported
bug.*

**L6 — Windows Build Smoke extension** (`accelerator.yml:518`, PR-gated). Before launching the
installed app, seed the production Run value with a missing spaced path; after launch, assert it
became the **exactly quoted** installed executable and resolves. *End-to-end proof on the real
shipped installer.*

Explicitly **not** claimed: `_e2e-updater-windows.yml` is `workflow_call`-only (recon §E — the
`workflow_dispatch` trigger was deliberately removed), so it is release-only and is not named as a
gate here.

---

## 7. Phases and gates

Every gate is a real command, and every named job is `pull_request`-triggered under the `desktop`
paths filter, aggregating into **Accelerator Status** (`accelerator.yml:606`).

| # | Work | Gate |
|---|---|---|
| 1 | `autostart.rs` pure layer + L1/L2 | `cargo test autostart`; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` → **Rust Tests** (`:88`) |
| 2 | Platform readers/writers, `heal_if_broken`, `set_enabled`; remove the plugin; `pub(crate) acquire_updater_lock` | `cargo test` green on Linux **and** `windows-build` (`:535`) |
| 3 | `main.rs` call site (webdriver-gated) + `repair_autostart` + `AutostartStatus` + `settings.js` | `bun run --cwd packages/accelerator test:e2e:ui` → **Desktop UI Tests** (`:413`); existing `settings.spec.ts:159` still passes |
| 4 | L3 + CI wiring (incl. the macOS bare `cargo test`) | `cargo test --test autostart_heal -- --ignored` green on all three `cert-trust` legs; the macOS log shows `patch_plist_*` **running** (they never have); `bun run lint:actions` |
| 5 | L4 container harness | `bun run --cwd packages/accelerator test:autostart` exits 0 locally; `bun run lint:shell` clean |
| 6 | L6 Windows smoke extension | **Windows Build Smoke** (`:518`) green with the seeded stale value |

Full local gate before push: `bun run test` + `bun run lint:actions`.

---

## 8. Assumptions

**Facts (source-verified in recon or during consolidation):** the plugin cannot report the stored path
(`tauri-plugin-autostart-2.5.1/src/lib.rs:49-72`); macOS `enable()` recreates the plist and strips
`KeepAlive` (`auto-launch/macos.rs:70-132`); `is_enabled()` is existence-only on macOS/Linux and
existence + `StartupApproved` on Windows (`windows.rs:73-95`); the plugin writes
`format!("{} {}", path, args)` unquoted on Windows (`windows.rs:37-43`) and unquoted `Exec=` on Linux
(`linux.rs:39`); macOS stores a canonicalized path (`lib.rs:202`); the plugin already prefers
`app.env().appimage` on Linux (`lib.rs:214-222`); `acquire_updater_lock` is a real `fs2` cross-process
lock held across the disarm window (`updater.rs:44,272,379,434`); Windows `install()` exits the
process (`updater.rs` comment, `tauri-plugin-updater/src/updater.rs:865`); `productName` is
`"Aztec Accelerator"` (`tauri.conf.json:3`); no repo test asserts autostart artifact content; no macOS
CI job runs a bare `cargo test`; the frontend never invokes `plugin:autostart|*`.

**Inferences (challenge these):** Finder's drag-to-`/Applications` is a move, so the old path stops
resolving — this is the trigger. "Path resolves" means canonicalizable regular executable file.
Racing healers converge because every writer writes a path that exists. A valid alternate copy is
functional and must not be auto-repointed. The heal is safe inside the Windows NSIS window because it
writes only live paths (§3.2 Fork B) — **this is the weakest inference in the plan and the one the
contradiction-check should attack hardest.**

**Asks:** one open scope question, **Q1** below. Nothing else blocks; no implementation phase carries
a silent ask.

---

## 9. Security & Adversarial Considerations

**Live finding, shipped today.** The Windows Run value is written unquoted
(`auto-launch/windows.rs:37-43`): `C:\Users\<u>\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe`
makes `CreateProcess` try `C:\Users\<u>\AppData\Local\Aztec.exe` **first** — a user-writable directory
under `installMode: currentUser`. Any same-user process can drop a binary there and be launched at
every login. **Not** a privilege-boundary crossing (same user, and same-user code can already write
HKCU), so it is a **persistence-hijack primitive, not an EoP** — but it is a real defect, it is ours,
and D7 is what actually fixes it, at enable time rather than after breakage. Asserted by an L1 unit
test and by L6 on the real installer.

**Never resurrect.** `Absent` and `Unreadable` never heal. The heal can only rewrite a program path
inside an entry the user already opted into; it can never create one. This is the primary abuse
boundary — an autostart writer that can create entries *is* a persistence primitive.

**Never widen crash recovery.** The heal touches only the autostart artifact. On macOS it incidentally
heals `KeepAlive`'s path because it is the same file, but it never arms recovery that was not armed
(D13 keeps the rearm keyed to entry presence, exactly as today).

**Injection.** Malformed attacker-controlled artifact content must never become plist, `Exec=`, or
command-line injection. Every transform is fail-closed: anything not matching the expected shape is
`Unreadable`, which never writes. `autostart_path_is_safe` (C6) now runs on the heal path too. No
shell is ever invoked; reads are size-bounded.

**TOCTOU / symlink.** Refuse a symlinked plist or `.desktop`; write same-dir temp + `rename`, `0600`.

**Auditability.** Every heal logs `from`→`to` at `info`. Paths are logged only under the user's own
home; no secrets.

**Threat model boundary explicitly not claimed:** same-user malware can already alter HKCU and user
startup files. This work does not claim to stop that. It claims that *our own writer* must not create
the hijack, and that our reader must not turn hostile input into injection.

---

## 10. Post-implementation

`/harden security` is already scheduled by the owner as a separate pre-release pass. After merge, the
queued chain resumes: **`mainBinaryName: "AztecAccelerator"`** (unblocked by this work — and note D7
makes the artifact name *ours*, so the rename becomes a plain constant change with a zero install
base), then bundle metadata, then `/harden`.

---

## 11. Open scope question (Q1)

**Does the `update-disarmed.json` marker come into this PR, or ship as its own?**

It fixes a *pre-existing* crash-recovery defect (`main.rs:607-625` can rearm the Windows recovery task
mid-NSIS, because the updater's lock dies at `install()`'s `process::exit(0)`), not an autostart one.
The heal is safe in that window by construction (§3.2 Fork B). Taking it in adds a cross-process state
file, a state machine, a removal rule (*"any process running from the marker's expected install path
may remove it"*, plus a TTL — **not** codex's version-matching fail-closed, which bricks recovery
permanently after a catastrophic NSIS failure), and its own tests.

Deferred in this draft, and named rather than dropped. Put to the contradiction-check first, then to
the owner at the approval gate.
