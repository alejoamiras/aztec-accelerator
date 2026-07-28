# Final fresh-context pass — codex, plan revision 4

Session `019fa9ef-3a26-7fa1-be99-fcfd06721072`. A NEW session with no memory of the prior rounds, given
the plan plus the full decision trail (both planning legs, both contradiction-checks, both audits).

**Verdict: reject.** Headline recommendation — **split the work into three pieces** — is adopted.

## Blocking finding #1 is REFUTED (main agent, with repo evidence)

Codex claims Tauri's `/UPDATE` path "proceeds directly to installation without running the previous
uninstaller", and concludes L8's barrier sits at a hook production never invokes — and therefore that
the `P`-absent interval may not exist at all, so D18 might be solving a hypothetical.

**This is wrong, and this repo measured it.** `nsis/hooks.nsi:5-31` documents that Tauri runs the
PREVIOUS version's uninstaller when installing over an existing install, and enumerates the observed
command lines:

    uninstall.exe /S _?=<dir>            $EXEDIR == $INSTDIR   install-over
    uninstall.exe /S /UPDATE _?=<dir>    $EXEDIR == $INSTDIR   install-over
    uninstall.exe /S   (Add/Remove)      $EXEDIR != $INSTDIR   REAL uninstall

That is not a comment written from the docs. PR #375 shipped a guard built on an equally plausible
wrong assumption (`${GetOptions} $CMDLINE "_?="`), CI caught it, and the corrected behaviour was
measured under wine (`scripts/nsis-hook-test.sh`) **and** is pinned by a PR-gated test on a real
`windows-latest` runner ("NSIS uninstall-hook guard", `accelerator.yml`). The whole reason that hook
needs a guard is that it fires on every upgrade.

So the old uninstaller does run, the `P`-absent interval is real, the original Fork B counterexample
stands, and D18 remains justified. What survives from the finding is a fair methodological point,
adopted: **pin the premise with an assertion instead of a comment** — L8 asserts `P` is non-resolving
at the barrier rather than assuming it.

Findings #2, #3 and #4 are correct and are folded into r5.

---

## Verdict: reject

The core autostart design is strong, but the Windows updater extension rests on a false installer-lifecycle assumption. D18 is not concurrency-closed, and L8 does not exercise the production update path.

## Scope: split it

This is three pieces:

1. **Core autostart fix:** owned readers/writers, plugin removal, quoting, status/UI, `repair_autostart`, and `autostart.lock`, with L1–L7. This carries the actual user-facing fix.
2. **Windows updater transaction hardening:** marker, update suppression, completion protocol, recovery reconciliation. Do this only after proving an actual non-resolving-`P` interval in the generated installer.
3. **Updater-test infrastructure:** dispatch workflow, synthetic builds, real N−1 fixture, and signing isolation.

First perform the installer-lifecycle proof. If it demonstrates a real gap, land piece 2 before piece 1; otherwise delete D18 rather than solving a hypothetical state machine. Piece 3 should not enlarge the product PR.

## Blocking findings

1. **L8’s barrier is at a hook the production updater does not invoke.**

   The vendored updater passes `/UPDATE` before exiting ([updater.rs](~/.cargo/.../tauri-plugin-updater-2.10.1/src/updater.rs:801)). In Tauri’s update path, `/UPDATE` proceeds directly to installation without running the previous uninstaller; the installer overwrites files in place. Consequently, synthetic N−1’s `NSIS_HOOK_POSTUNINSTALL` does not supply the claimed barrier. The existing comment claiming every upgrade runs the old uninstaller is also wrong for in-app updates (`packages/accelerator/src-tauri/nsis/hooks.nsi:5`). See the [Tauri 2.10.1 NSIS template](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.10.1/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi).

   If the test forces an uninstall, it manufactures the `P`-absent interval and proves only marker behavior under fault injection—not that production has that interval. First trace the generated `/S /R /UPDATE` installer and establish whether `P` ever becomes non-resolving.

2. **D18 still has an owned race with no durable exit.**

   Candidate N reads current intent ON; concurrently the user turns it OFF, deleting Run and disarming the task; N then arms recovery from its stale read and removes the marker. Final state: intent OFF, but the every-minute crash-recovery task is armed. Later startups see OFF and do nothing, so nothing automatically disarms it.

   The removal transaction must hold `autostart.lock` continuously across current-intent read, recovery reconciliation, and marker removal. Updater disarm→marker creation needs the same lock order. Explicit ON must be rejected while a marker is live; OFF must remain allowed. None of this lock graph is specified.

3. **The completion-token protocol is neither transaction-bound nor safely live.**

   A POSTINSTALL token is evidence only that execution reached that point—not that the installer process exited. More importantly, the plan defines no nonce, token cleanup, or binding to one marker creation. A stale token from an earlier same-version attempt can satisfy a later retry. If token creation fails after a successful install, there is no safety-preserving exit; expiry merely reopens the race.

   The installer should atomically acknowledge a unique marker transaction ID. Its hook must be update-only and its failure/retry semantics explicit.

4. **The deadline contradicts success criterion 6.**

   If NSIS remains alive past the deadline, Q may heal, rearm, or begin another update during the exact window the marker promised to close. Calling TTL “only liveness” is honest, but criterion 6 remains absolute. Tests using normal installer duration will all pass.

## Non-blocking

- “Settings never shows ON for an entry that will not launch” remains too strong: Linux additionally specifies `OnlyShowIn`, `NotShowIn`, and `TryExec`; launchd disable state may live outside the plist.
- Remove `stored_path` from IPC or redact it in Rust. `textContent` prevents injection, not disclosure to the webview.
- The marker suppresses security updates as well as autostart recovery, so it is a broader availability lever than merely deleting the Run value.
- Rust no-follow checks do not protect the NSIS hook itself from a junction/reparse-point token destination.
- With zero install base, legacy unquoted-value interpretation can be test-only rather than production migration logic.
- Put dispatch and production signing in separate workflows; event guards are insufficient unless the key is actually moved to a protected environment.

## Genuinely sound

D1’s resolve-not-diff rule, in-place repair, plugin removal, quoted Windows enable, intent/health separation, OFF-capable UI, XDG/AppImage handling, the dedicated short-lived lock, native Run-value execution, real IPC coverage, and adding bare macOS `cargo test` are all well-founded.
