# Lessons — phases 1–2 (pure layer + I/O + plugin removal)

- **Phases 1 and 2 collapsed into one compile unit.** Removing `tauri-plugin-autostart` from
  Cargo.toml breaks `commands.rs`/`main.rs`/`updater.rs` immediately, so the rewiring cannot trail
  the module by a commit. The plan's phase *gates* still ran separately; the phase *boundary* was
  notional. Expected for D7-style removals — plan the commit that way next time.
- **Two test-expectation bugs, zero implementation bugs, caught by the gate:**
  - `%%U` contains `%U` as a substring — a `!quoted.contains("%U")` assertion can never hold for
    doubled field codes. The real assertion is "no lone `%`".
  - `C:\a.exe b` keeps its last-dot "extension" in the CreateProcess append heuristic — my invented
    expectation `C:\a.exe b.exe` contradicted the model I had just written.
- **`clippy -D warnings` vs the everywhere-compiled pure layer:** each platform sees the other two
  platforms' codecs as dead code. Resolved with `#[cfg_attr(not(<os>), allow(dead_code, reason =
  ...))]` per function group — the `reason` field documents the §4.1 design decision at the exact
  place a future reader would "clean it up".
- **The removed plugin's Windows `disable()` errored on an already-absent Run value**
  (`delete_value` → NotFound). Onboarding calls `set_autostart_inner(false)` on decline, so a fresh
  Windows install declining autostart got a spurious error result. The owned `remove()` treats
  already-absent as success (idempotent OFF). Behavioural delta, deliberate.
- **lint-staged spawns `rustfmt` from the hook's own PATH** — commit with `~/.cargo/bin` on PATH or
  the pre-commit hook ENOENTs (and its stash-backup/revert cycle runs; it cleans up after itself,
  but on this machine the stash stack is shared across worktrees — don't panic at the transient
  entry).
- `cargo test` in a fresh worktree needs `frontend:build` + `prebuild` (bb sidecar) first — the
  build.rs asserts both.

# Lessons — phase 5 (L4/L7)

- **"HOME unset" is NOT the C8 trigger.** `dirs` falls back to `getpwuid` when `$HOME` is missing,
  so the first no-HOME harness leg (a bare `env -u HOME`) resolved the invoking user via passwd and
  **wrote a real `.desktop` into my actual profile** before failing its own wrong assertion. The
  real trigger is *home resolution unavailable*: HOME unset **and** a uid with no passwd entry (the
  k8s/random-uid container case) — now reproduced with `docker run --user 12345:12345` executing
  the prebuilt test binary. The test also self-skips loudly anywhere home still resolves, so the
  wrong-assertion class can't recur. Cleanup performed (`~/.config/autostart/*.desktop`).
- **WebKitGTK's WebDriver rejects `execute/async`** ("Origin header is not a valid URL") — caught
  pre-push only because trust-boundary.spec.ts documents it. L7's bridge invoke uses the same
  sync-execute-stash-then-poll pattern.
- **build.rs reaches OUTSIDE the crate** (`../package.json`, `../../../bun.lock`): a container that
  copies only `src-tauri` + `core` fails the F-012 manifest check. The harness mirrors the repo
  layout instead.
- **L7 cannot run on this headless host** (no X/Xvfb): its first execution is the CI WebDriver
  matrix — watch that job specifically on the PR.
- Background `run_in_background` tasks capture only what the command PIPES — `| tail -N` threw away
  the panic that mattered. Tee to a scratchpad file instead when the output will be diagnostic.

# Lessons — post-implementation review rounds

Three review rounds each found real defects **in the previous round's fixes**. Worth remembering
when sizing ceremony for piece 2.

- **Round 1 (workflow, 2.3M tokens):** one serious find — parse-strangeness and I/O failure collapsed
  into one `Err`, so a hand-edited artifact permanently disabled the Settings switch (worse than the
  plugin it replaced). Everything else was moderate.
- **Round 2 (same workflow, re-verified against the new HEAD):** my round-1 fix was **incomplete** —
  `status()` stopped Err-ing on the classification while `intent_enabled` still re-parsed the same
  file underneath and Err'd anyway. The Playwright test I wrote for exactly this case mocks the
  status object, so it passed while the bug was live. **A test at the wrong layer is worse than no
  test: it buys false confidence.**
- **Round 3 (codex):** my flag-byte parity fix was defeated by a `len < 8` guard left above it, and
  the `Exec` args guard covered only the quoted form. Both were half-applied fixes I had already
  reported as done.

**The expensive finding classes, ranked by what actually caught them:**
1. CI (cheapest) caught the two most severe production bugs — the Windows fresh-profile `Run` key
   (enable impossible) and the L7 fixture premise. Neither came from a reviewer.
2. Adversarial review caught the defect classes CI structurally cannot: a control that is *worse
   than what it replaced*, and tests that pass for the wrong reason.
3. Nothing caught my **destructive tests** except a reviewer explicitly asked to look for them —
   L3-Windows deleted the shared HKCU `Run` key (other apps' startup entries), and L7 clobbered the
   developer's real artifact while its header claimed a throwaway `$HOME`.

**Rules earned here:**
- Assert a test's precondition; never `return` out of it. The L4 no-home leg *skipped* when isolation
  failed, so a broken harness passed silently.
- When a fix spans a read path and its consumers, grep every consumer before declaring it done —
  twice now the consumer underneath re-did the work the fix removed.
- A comment claiming isolation ("throwaway $HOME", "scoped off the real profile") is a claim that
  must be verified, not decoration. Two were false.

## Accepted residuals (piece 1) — argued, not overlooked

Two verification findings are deliberately NOT fixed. Recorded so a later reader sees a decision,
not an oversight:

1. **Unquoted `Exec` with POSITIONAL arguments still heals and drops them.** Options are caught
   (a remainder token starting with `-`), positional args are not. Perfect disambiguation is
   impossible: the legacy `auto-launch` format is `{spaced path} `, whose remainder is a path
   fragment, so "any remainder ⇒ refuse" would break the primary heal case this work exists for.
   The dash heuristic covers what users actually write; the residual is a user with
   `Exec=/path/app somefile` losing `somefile` on relocation.
2. **`set_enabled` holds `autostart.lock` across crash-recovery arming** (`systemctl`/`schtasks`),
   which is unbounded. Narrowing it is not available on macOS, where arming patches the SAME plist
   the lock protects — so the critical section would have to cover it anyway. Mitigated instead by
   the bounded 10 s acquisition; the residual is a user-initiated toggle that can wait up to 10 s
   behind a concurrent operation.

Also settled: `platform_disabled` is tolerant of read failure BY CHOICE (erroring re-bricks the
Settings switch, the exact defect the Unreadable work fixed) and now logs rather than hides it.

**The lock is not reentrant** — the round-trip test's mutate closure self-deadlocked until the 10 s
bound when it called a lock-taking API. Production is right; test helpers must clobber artifacts
directly.
