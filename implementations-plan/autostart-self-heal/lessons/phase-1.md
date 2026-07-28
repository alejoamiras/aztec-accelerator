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
