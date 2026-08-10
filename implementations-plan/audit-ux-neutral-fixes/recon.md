# Recon — UX-neutral open findings from audit 2026-07-31-9c4cb0c

Read against `df38283` (current `main`), which is the base this worktree branches from.

> **Why that sentence matters.** The first recon pass read the canonical clone's working tree, which
> was checked out at `5d0efa4` — fifteen commits behind. `autostart.rs` and `update_marker.rs` do not
> exist at that commit, so F-12 appeared to have no implementation at all. Every line reference below
> is from the worktree's own checkout.

## Scope

In: **F-13**, **F-08a**, **F-12**, **F-11**, **F-03**.
Out: **F-07**, **F-10** (change user-visible consent behaviour — owner decision pending).
Out: **F-09** (owner deferred: its fix reintroduces F-04's permanent-update-lockout shape).

Owner constraint governing everything below: **anything we cannot write a test for is not implemented
unless it is innocuous.**

---

## F-13 — `schtasks.exe` resolved through `%SystemRoot%`

- **Defect**: `crash_recovery.rs:370` `schtasks_exe()` builds the path from `$SystemRoot` / `$windir`
  with a `C:\Windows` string fallback. No hardcoded-System32 preference.
- **Reuse as-is**: `trust/windows.rs:36` `certutil_exe()` is the correct sibling — try
  `C:\Windows\System32\<tool>.exe` first, `is_file()`, fall back to the env only if absent. Four lines.
  Its own doc comment cites this as the pattern, and the audit's F-13 entry says the hardening was
  applied to one and not the other.
- **Collision risk**: none. Two independent functions; no shared helper exists to fight over. A shared
  `system32_tool(name)` helper is possible but couples two modules for eight lines — call it in the plan.
- **Test shape**: **neither resolver has a test today.** New. Must set `SystemRoot` to a bogus value
  and assert the hardcoded path still wins. `#[cfg(windows)]`, so it only runs on the Windows CI leg.
- **Test must actually EXECUTE**: `tests/autostart_heal.rs` is run on the Windows runner via
  `cargo test --test autostart_heal -- --ignored --nocapture --test-threads=1`. A `#[cfg(windows)]`
  unit test inside `crash_recovery.rs` runs in the `--lib` suite — confirm that suite runs on the
  Windows leg, or place the test where it demonstrably does. (This is the `NTE_NOT_FOUND` lesson:
  compiled-on-Windows is not run-on-Windows.)

## F-08a — witness residue

- **Defect**: `bb.rs:120` `create_prove_tempdir()` returns a `tempfile::TempDir` whose `Drop` is the
  only deletion. Quit / auto-update restart skip it. No startup reaper exists.
- **Reuse as-is**: `versions/downloader.rs:236` `reap_stale_stages()` is the same job one directory
  over — enumerate a parent, match a prefix, age-gate, remove. Its test at `:513`
  (`reap_stale_stages_spares_recently_active_stages`) is the shape to copy, including how this repo
  backdates mtimes: `filetime::set_file_mtime(&p, FileTime::from_system_time(long_ago))`.
- **Adapt with changes**: the sweep target is `prove_tmp_parent()` (the `0700` `prove-tmp` dir), and
  the prefix is `prove-`, not `.{version}.tmp.`.
- **Dependency gap**: `filetime` is a dev-dep of **core only** (`core/Cargo.toml:62`). `bb.rs` is in
  core, so the sweeper and its test land there and the dep is already available. No new dependency.
- **Age gate**: an active proof's directory must survive. The `recently_active` / active-window idea
  already exists in `versions/downloader.rs`.
- **"At startup nothing is in flight" is FALSE — correction.** `accelerator-server` (headless) depends
  on `accelerator-core` and calls `server::start` (`server/src/main.rs:16`), so it reaches the same
  `bb.rs` → the same `dirs::data_local_dir()/aztec-accelerator/prove-tmp`. **Two processes share the
  directory**, and the desktop app's startup sweep can delete a workspace the headless server is
  actively proving into. This is the F-06 lesson repeating one directory over, so the age gate is
  load-bearing, not hygiene.
- **Sweep scope**: only `prove_tmp_parent()`. `create_prove_tempdir` falls back to the OS temp dir on
  non-Windows when no data-local dir resolves (`bb.rs:144-146`); prefix-matching `prove-*` in a shared
  `/tmp` risks deleting a stranger's directory. That residue stays unreaped, by choice.

## F-12 — AppImage `$APPDIR` provenance

- **Defect**: `autostart.rs:1197` `exe_c.starts_with(&dir_c)`. `APPDIR=/` contains every absolute path.
- **Reuse as-is**: the function at `:1186` is **already pure and injectable** —
  `appimage_self(appimage, appdir, exe)` — and its doc comment says so explicitly: *"Pure so the
  provenance rule is table-tested; both consumers go through it so they cannot diverge."* There is an
  existing table test at `:1789` (`appimage_trusted_only_when_our_exe_lives_under_appdir`) covering
  the genuine case, the inherited-parent case, and the missing/empty halves.
- **Test shape**: extend that table with the `APPDIR=/` case. Cheapest possible test surface.
- **Design constraint (load-bearing)**: the audit's suggested fixes — device-ID comparison, or a
  `/proc/self/mountinfo` mountpoint check — both read ambient filesystem state, which would break the
  purity the table test depends on. **Whatever we choose must arrive as a parameter** (an injected
  `is_mountpoint` predicate, or mountinfo content as a string) so the table test still works. This is
  the main design question for F-12 and should be argued in the audits.
- **Consumers to keep aligned**: `crash_recovery.rs:249,262,560,592` all defer to `appimage_self` and
  say so in comments — the fix stays in one place, which is why this is cheap.

## F-11 — SDK `/prove` response is unbounded

- **Defect**: `accelerator-transport.ts:638` `postProve()` returns the raw `Response`; the body is read
  by the caller with no cap, deadline, or abort path. ky's `timeout` stops counting at headers.
- ~~**Reuse — with a correction to the audit**: the audit says *"reuse `readJsonBounded`"*. That
  function parses JSON; the `/prove` response is a binary proof, not JSON.~~
  **RETRACTED — this was wrong, and both auditors caught it independently.** `/prove` returns
  `axum::Json(json!({"proof": encoded}))` (`core/src/server/prove.rs:372`) and the SDK reads it with
  `(await res.json()) as { proof: string }` (`accelerator-prover.ts:565`). The body **is** JSON,
  carrying a base64 string. The audit's advice was correct; my "correction to the audit" was the
  error. The fix is therefore *smaller* than this section claimed: parameterize `readJsonBounded`'s
  hardcoded `HEALTH_BODY_MAX_BYTES` / `HEALTH_BODY_TIMEOUT_MS` (`accelerator-transport.ts:26-27`), and
  additionally bound the base64 string before `Buffer.from` (`accelerator-prover.ts:567`) — a capped
  JSON body still decodes to ~0.75× its size in a fresh buffer. See `decision-ledger.md` E-1.
- **Adapt with changes**: `readJsonBounded` already implements the deadline + byte-cap + single-read
  discipline against a `Response`; factor its transport-level half out, or write the sibling next to it
  with a comment binding them.
- **Cap sizing is an empirical question, not a guess**: too low and proving breaks for everyone. The
  SDK has `test:e2e` and `test:e2e:remote` (live testnet) — a real proof through the real path is the
  measurement. Sizing must be justified by an observed proof size with headroom, recorded in the plan.
- **Test shape**: the SDK suites already stub `globalThis.fetch` (see `accelerator-prover.test.ts`'s
  `mockFetch`), so over-cap and stalling responses are straightforward to simulate.

## F-03 — forgeable `/health` treated as process identity

> **Correction (after reading `findings/verified.md:263-355`).** The paragraph below treats F-03 as
> one thing. It is **three sinks**, and they have wildly different cost and risk. Treating them as one
> was wrong and it made the finding look undoable.
>
> - **Sink A — Windows bow-out** (`main.rs:294-303`): the only sink carrying the Medium. Hard. See
>   `plan.md` §"F-03 sink A: why it is not here" — the audit's own recommended fix (reuse a
>   per-install token) is refuted twice over: no such token exists (verified: `grep token` in
>   `accelerator-transport.ts` is empty — the shipped F-01 fix was httpsOnly + URL validation), and a
>   same-user attacker reads any `0600` secret we own, so no file-based token could work regardless.
> - **Sink B — floor ratchet** (`main.rs:340-360`): verified as **impact-free security-wise**
>   (`commit_launch_floor` commits `env!("CARGO_PKG_VERSION")`, never the probed value). It is a
>   **correctness** fix, cheap, UX-neutral, and testable as a pure function. **In scope.**
> - **Sink C — unbounded probe read** (`probe.rs:41,64`): a two-line cap, same defect class as F-11.
>   **In scope**, paired with F-11 so the two caps cannot drift.

- **Defect**: `server/probe.rs:12-16` `is_our_accelerator()` tests two public constants
  (`status=="ok"`, `api_version==1`) and that verdict drives whether the real app exits.
- **Existing quality**: the predicate is already pure and unit-tested (`probe.rs:82-85`). The problem
  is not the function, it is that an HTTP answer is the wrong evidence for "am I redundant?".
- **Reuse**: none — this is new code. Three platform implementations.
- **Owner constraint applies hardest here.** Not innocuous: the failure mode is *the app will not
  start*. It qualifies only under a design where that failure is impossible by construction:
  **kernel-released primitives only** (named mutex on Windows, abstract unix socket on Linux, `flock`
  on macOS) — all released by the OS on process death, so there is no stale state to strand a restart.
  If the design drifts toward a manually-managed lock file with its own PID/staleness logic, F-03 stops
  and defers.
- **Test shape**: contention is testable per-OS by spawning a helper process that tries to acquire and
  reports. Needs a real second process — an in-process second acquire does not exercise the primitive.
- **Blast radius note**: `main.rs` currently exits with a success code deliberately chosen so the
  supervisor does not retry. Whatever replaces the decision must preserve that contract or the app
  gains a restart loop.
