# Plan — autostart self-heal

## 0. The one decision everything hangs on

**Stale means the stored target does not resolve. It never means "differs from my `current_exe()`."**

Diff-based healing is the framing in the prompt and in the rejected prior rounds, and it is wrong. It creates a regression worse than the bug: a leftover copy in `~/Downloads`, double-clicked once, repoints a perfectly good `/Applications` entry at a file the user is about to delete. Resolve-based healing is a strict repair — it only ever replaces a dead pointer with a live one.

This choice is load-bearing. It dissolves hard part 3 (two instances cannot both see an unresolvable path pointing at each other; the operation is idempotent and *convergent*, so racing writers agree), it removes the need for any instance-coordination primitive, and it makes the drag-to-`/Applications` case work exactly because Finder's drag is a **move** — the old path is gone. If the user *copies* instead, both exist, nothing heals, and autostart still launches a working app. Correct outcome, no write.

Second decision: **never call `manager.enable()` in a heal.** The heal patches the existing artifact in place. That is the whole answer to hard part 1 — macOS `KeepAlive` cannot be stripped by an operation that never recreates the plist. And because `ProgramArguments[0]` is shared by `RunAtLoad` (autostart) and `KeepAlive` (crash recovery), patching it **fixes hard part 2 for free, with zero edits to `crash_recovery.rs`**. That is the argument for taking the macOS early-return in scope: it isn't extra work, it's the same byte.

## 1. Correction to recon §F

Recon says "no instance-coordination primitive exists." **False.** `updater.rs:44` `acquire_updater_lock()` is a real cross-process `fs2` exclusive lock on `updater.lock`, and `perform_update` holds it from `:272` across the Windows disarm at `:379` through `app.restart()` at `:434`. Promote it to `pub(crate)` and have the heal take it non-blocking, skipping when held. Hard part 4 becomes a five-line guard with an existing, already-reviewed primitive — no new lock, no new stale-lock failure mode.

## 2. Architecture

New module `packages/accelerator/src-tauri/src/autostart.rs`, added to `lib.rs`. Pure string layer is **not** `#[cfg]`-gated, so all three platforms' parsers/serializers compile and unit-test on Linux CI.

```rust
pub enum StoredTarget {
    Absent,                                   // no entry — NEVER heal (never resurrect)
    Resolvable { program: PathBuf, raw: String },
    Broken { raw: String },                   // entry exists, nothing resolves
    Unreadable { reason: String },            // I/O or parse failure — NEVER heal
}
pub enum HealOutcome { NotNeeded, Healed { from: String, to: String }, Skipped(&'static str), Failed(String) }

pub fn read_stored_target() -> StoredTarget;            // #[cfg] dispatch
pub fn heal_if_broken() -> HealOutcome;                 // the one-shot; also the repair command body
fn write_program_path(exe: &Path) -> Result<(), String>;// #[cfg] dispatch, in-place

// pure, compiled everywhere, closure-injected `exists` per repo convention (cf. enable_transaction):
pub(crate) fn plist_program(xml: &str) -> Option<String>;
pub(crate) fn plist_rewrite_program(xml: &str, new_escaped: &str) -> Option<String>;
pub(crate) fn desktop_exec(ini: &str) -> Option<String>;
pub(crate) fn desktop_rewrite_exec(ini: &str, new_quoted: &str) -> Option<String>;
pub(crate) fn run_value_candidates(v: &str) -> Vec<String>; // CreateProcess search order
pub(crate) fn resolve_first(c: &[String], exists: &dyn Fn(&str) -> bool) -> Option<String>;
pub(crate) fn xml_escape(s: &str) -> String;            // & < > " '
pub(crate) fn desktop_quote(s: &str) -> Option<String>;  // freedesktop Exec quoting
pub(crate) fn run_value_quote(s: &str) -> Option<String>;// "…" — fixes today's unquoted value
```

`heal_if_broken()` sequence, single-entry, one-shot:

1. `current_exe()` → **must itself exist on disk** (`try_exists`). A running-but-relocated process has a stale `current_exe()`; healing from it writes a bad path. `Skipped("own path unresolvable")`.
2. `crash_recovery::autostart_path_is_safe(&exe)` — constraint 6, now enforced on the heal path too. Fail ⇒ `Failed`.
3. macOS only: `exe.canonicalize()` — constraint 7, matches `tauri-plugin-autostart/src/lib.rs:202`; comparing raw would false-positive on symlinks.
4. `app.autolaunch().is_enabled()`. `Ok(false)`/`Err` ⇒ `Skipped`. This respects Windows `StartupApproved`: a Task-Manager-disabled entry reads `false` and is never touched.
5. `read_stored_target()`. Only `Broken` proceeds.
6. `acquire_updater_lock()` — `None` ⇒ `Skipped("updater active")`.
7. `write_program_path(&exe)` — in place, preserving every other key.

Per platform, step 7:

- **macOS** `~/Library/LaunchAgents/Aztec Accelerator.plist`: read, find `<key>ProgramArguments</key>` → next `<array>` → first `<string>…</string>`, replace its contents with `xml_escape(path)`. Everything else — `KeepAlive`, `ThrottleInterval`, `RunAtLoad`, `Label` — byte-identical. **No `launchctl unload/load`**: a `load` with `RunAtLoad` immediately spawns a second instance, and `patch_plist_with_keepalive` already sets the precedent of file-only patching.
- **Linux** `$HOME/.config/autostart/Aztec Accelerator.desktop` (constraint 8: `$HOME`, *not* `$XDG_CONFIG_HOME` — mirror `auto-launch`'s `dirs::home_dir()`, but `ok_or_else` instead of its `.unwrap()`): rewrite the `Exec=` line's program token, preserving args and every other key/line.
- **Windows** `HKCU\…\Run` value `Aztec Accelerator`: `set_value` to `run_value_quote(path)`. Registry value writes are atomic.

macOS/Linux writes are **temp-file-in-same-dir + `fs::rename`**, with a symlink refusal on the target. Atomic rename is the actual serialization answer to concurrent healers, and it closes a TOCTOU/symlink-swap on a world-readable dotfile path.

**Call sites.** `main.rs` — replace the block at 607-625 so the heal runs *before* the existing `enable_crash_recovery()` rearm (heal the path, then arm against the healed artifact), log-and-continue, `#[cfg(not(feature = "webdriver"))]`-gated (recon §G: the current block isn't gated and runs in E2E; gate the new work rather than inherit the defect). Plus a new `#[tauri::command] repair_autostart(window, app) -> Result<AutostartStatus, String>` calling the same function.

**What I would not do.** No `plist`/`serde-plist` dependency (new supply-chain surface for a job the repo already does with `rfind("</dict>")`). No `tauri-plugin-single-instance` (app-wide launch-semantics change, out of scope, unnecessary given convergence). No `launchctl`. No periodic/watcher heal — one-shot per process is what makes flip-flop structurally impossible. No migration code. No changes to `crash_recovery.rs` logic. No wine spike (§4).

## 3. Status honesty

`get_autostart_enabled` returns a struct, not a bool. It is a label-guarded IPC surface with one in-repo consumer; honesty is worth one frontend edit.

```rust
#[derive(serde::Serialize)] #[serde(rename_all = "camelCase")]
pub struct AutostartStatus { pub enabled: bool, pub broken: bool, pub stored_path: Option<String> }
```

`enabled = entry_present && target_resolves`. A `Broken` entry reports `enabled: false, broken: true`. An `Unreadable` entry still `Err`s (preserves the codex #7 behaviour at `commands.rs:52-57` — the switch stays disabled on unknown state).

`settings.js:52-59`: `checked = status.enabled`. When `broken`, show a warning row — *"Start on login is set up, but points to a file that no longer exists"* — with a **Fix** button invoking `repair_autostart`, then re-render.

**Report-false alone is a trap:** the user toggles ON, `set_autostart_inner` reads `prior_enabled = true` from the plugin, `enable_transaction` correctly *skips* `plugin_enable` (constraint 1), and nothing changes. A silent dead end. **Auto-heal-then-report-true is strictly better** for the case it covers, because after a successful heal `enabled: true` is *true*. The two are not alternatives: heal at startup for the silent-fix path, report honestly for the residual (heal skipped, heal failed, or relocation *while running* — the common one, since users drag the app to `/Applications` with it already open).

## 4. Test plan

**L1 — pure unit, `cargo test`, Linux, every PR** (`#[cfg(test)] mod tests` in `autostart.rs`, repo convention §H). All three formats' parse+rewrite: plist with nested dicts and `KeepAlive` present (assert KeepAlive survives byte-identically); `&`/`<` in paths round-trip; unquoted `.desktop` `Exec=/a b/c ` (trailing space, exactly what `auto-launch:39` writes) resolves via longest-existing-prefix; `run_value_candidates("C:\\Program Files\\Aztec Accelerator\\Aztec Accelerator.exe ")` yields the documented CreateProcess order; injected `exists` closure returning nothing ⇒ `Broken`. **Catches:** every parser/serializer defect, on every OS, in 200ms. Nothing today tests any of this.

**L2 — golden fixtures pinning the crate contract.** Byte-exact expected artifacts for all three platforms, derived from `auto-launch-0.5.0` source, asserted parseable by our readers. **Catches:** a `tauri-plugin-autostart`/`auto-launch` bump silently changing the format so our reader sees `Unreadable` forever. Runs on Linux.

**L3 — real-OS integration, `src-tauri/tests/autostart_heal.rs`, `#[ignore]`d, throwaway `$HOME`** (exact `trust_linux.rs` shape). Fixture is written by **`auto_launch::AutoLaunchBuilder` itself** — the plugin's own crate — then relocated (delete the file it points at), then healed by our code, then re-read. Wired into the existing PR-gated `cert-trust` matrix (all three OSes) as `cargo test --test autostart_heal -- --ignored --nocapture`. On macOS this also fixes the **BONUS BUG**: add a bare `cargo test` to that leg, which finally compiles the three dead `patch_plist_*` tests. **Catches:** real registry semantics including `StartupApproved`, real plist/`.desktop` I/O, `$HOME` divergence — free, on infrastructure that already exists.

**L4 — Docker, `packages/accelerator/scripts/autostart-heal-test.sh`**, modelled line-for-line on `nsis-hook-test.sh` (inline heredoc Dockerfile, `docker run --rm -i`, `bash -s`; auto-covered by `lint:shell`; add `"test:autostart": "bash scripts/autostart-heal-test.sh"`). Container = `rust:bookworm`, runs L3's Linux leg under a *hermetic* `HOME=/h` with `XDG_CONFIG_HOME=/decoy` deliberately set to a different directory. **Catches:** constraint 8 — a heal that silently watches the wrong dir. On the dev host the two paths coincide, so only a container proves it. Also catches the `dirs::home_dir()` panic (run once with `HOME` unset; expect `Skipped`, not abort).

**L5 — Playwright**, `e2e/settings.spec.ts` + `e2e/tauri-mock.js`: mock `get_autostart_enabled` returning `{enabled:false,broken:true}` ⇒ warning row visible, switch unchecked, Fix button invokes `repair_autostart`. **Catches:** the UI lying, which is half the reported bug.

**Wine: drop it.** Recon measured our cross-compiled binary failing under Debian's wine (exit 53). Chasing a WineHQ build buys a Windows *registry* proof we already get for free and more faithfully from `cert-trust (windows)` on `windows-latest`, and a CreateProcess-search proof that wine's heuristic may not even reproduce — a green wine test there would be *misleading*. L1 covers the search-order logic; L3 covers real registry I/O. Spending the spike is negative value.

## 5. Phases and gates

| # | Work | Gate (real commands, PR-observable) |
|---|---|---|
| 1 | `autostart.rs` pure layer + L1/L2 tests | `cd packages/accelerator/src-tauri && cargo test autostart` — new tests pass; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check`. Job **Rust Tests** (`accelerator.yml:88`, PR-gated). |
| 2 | Platform readers/writers, `heal_if_broken`, `pub(crate) acquire_updater_lock` | `cargo test` green on Linux **and** `cert-trust (windows)`/`windows-build` (`cargo test`, `:535`). |
| 3 | `main.rs` call site + `repair_autostart` + `AutostartStatus` + `settings.js` | `bun run --cwd packages/accelerator test:e2e:ui` (job **Desktop UI Tests**, `:413`) — new broken-state specs pass, existing `settings.spec.ts:159` still passes. |
| 4 | L3 integration test + CI wiring (incl. bare `cargo test` on the macOS `cert-trust` leg) | `cargo test --test autostart_heal -- --ignored` green on all three `cert-trust` legs; the macOS leg's log shows `patch_plist_*` tests **running** (they never have). `bun run lint:actions`. |
| 5 | L4 container harness | `bun run --cwd packages/accelerator test:autostart` exits 0 locally; `bun run lint:shell` clean. Local-only, exactly like `test:nsis` — not a CI job, and I do not claim it is one. |

All gate jobs are `pull_request`-triggered under the `desktop` paths filter (`accelerator.yml:2-4, 24-40`) and aggregate into **Accelerator Status** (`:606`). No `workflow_dispatch`-only or release-only job is named.

## 6. Assumptions

**Facts (source-verified):** `enable()` recreates the macOS plist (`macos.rs:70-132`); `is_enabled()` is existence-only on macOS/Linux (`macos.rs:164`, `linux.rs:71`) and existence+`StartupApproved` on Windows (`windows.rs:73-83`); plugin writes `format!("{} {}", path, args)` with `args` empty ⇒ trailing space in the Run value and `Exec=`; macOS path is canonicalized (`lib.rs:202`); `perform_update` holds the updater lock across the disarm window (`updater.rs:272,379`); no repo test asserts autostart artifact content; `nsis-hook-test.sh` is the container precedent and is not in CI.

**Inferences:** Finder drag-to-`/Applications` is a move, so the old path stops resolving (the trigger). Windows `CreateProcess` prefix search makes today's unquoted spaced value *work* but *hijackable*. Racing healers converge because every writer writes a path that exists.

**Asks:** (a) Confirm changing `get_autostart_enabled`'s return type is acceptable versus adding a second command. (b) Confirm the Fix button should be a distinct `repair_autostart` rather than reusing `set_autostart(true)` — it must be, given the `prior_enabled` skip.

## 7. Security

- **Never resurrect.** `Absent` and `Unreadable` never heal. The heal can only rewrite a program path inside an entry the user already opted into; it can never create one. This is the primary abuse boundary — an autostart writer that can create entries is a persistence primitive.
- **`autostart_path_is_safe` on the heal path** (constraint 6) — currently toggle-time only.
- **Quoting is a fix, not just hygiene.** Today's `…\Aztec Accelerator\Aztec Accelerator.exe` (unquoted, spaced) makes Windows try `…\Aztec Accelerator\Aztec.exe` first — a user-writable directory under `installMode: currentUser`. That is a live same-user persistence hijack. Healed entries are quoted; the L1 test asserts it.
- **Symlink/TOCTOU:** refuse a symlinked plist/`.desktop`; write via same-dir temp + `rename`, `0600`.
- **Never widen crash recovery.** The heal touches only the autostart artifact; on macOS that incidentally heals `KeepAlive`'s path because it is the same file, but it never arms recovery that wasn't armed.
- **Log the transition** (`from`/`to`) at `info` — an unexplained startup-entry rewrite must be auditable in the shipped log.

### Critical Files for Implementation
- packages/accelerator/src-tauri/src/autostart.rs (new)
- packages/accelerator/src-tauri/src/commands.rs
- packages/accelerator/src-tauri/src/main.rs
- packages/accelerator/src-tauri/src/updater.rs
- packages/accelerator/src-tauri/frontend-src/settings.js
- .github/workflows/accelerator.yml
