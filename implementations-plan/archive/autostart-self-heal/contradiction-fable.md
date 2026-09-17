# Contradiction-check — fable (top-tier Claude planning subagent, fresh context)

Reviewed plan.md r1 + the decision ledger, in parallel with codex and without seeing it.
Headline: three source-verified defects, two of them in NEITHER original planning leg.
On Fork B it reached the opposite verdict to codex (defer the marker) — codex won, on a case
fable had not considered either.

**Verdict:** Internally consistent except for one real cross-phase contradiction (StartupApproved vs D13), one stale justification (repair_autostart), and one incomplete proof (Fork B's walk). Fork A/B/C resolutions survive attack; every checked Fact is source-accurate.

## 1. Contradictions

**C1 — D13/§4.6 vs §4.5 vs recon §C: "presence" is not intent on Windows.** Verified `auto-launch/windows.rs:73-95`: `is_enabled() = Run-value-present &amp;&amp; StartupApproved-not-disabled`. Today (`main.rs:614`) a Task-Manager-disabled entry reads `false` → no crash-recovery rearm. The plan rekeys the startup rearm *and* the pre-update capture (`updater.rs:364-365`) to `entry_present`. If that means bare Run-value presence, a TM-disabled user gets the schtasks relauncher armed at every launch and re-armed after every update — arming auto-relaunch against an explicit user OFF, contradicting §4.5's own "Task-Manager-disabled entry stays disabled" and recon's "off stays off has a real mechanism to respect." Fix is one sentence: define `entry_present` on Windows as *present AND not StartupApproved-disabled*. D13's core claim is otherwise verified correct — health-keyed rearm would stop protecting exactly the Broken-entry users whenever a heal fails; and on macOS/Linux presence == today's existence-only `is_enabled()`, so it's clean there.

**C2 — §4.8's justification for `repair_autostart` is stale under D3.** "User toggles ON, `prior_enabled = true`, enable skipped, dead end" was true of the *plugin's* existence-only read. Under the plan's own semantics a Broken entry reports `enabled:false` → `prior_enabled=false` → toggle-ON writes a fresh quoted entry and repairs. The dead end survives only if `prior_enabled := entry_present` — never stated. Related silent resolution: codex made toggle-ON the repair path ("Turn it on to repair"); fable invented the Fix button. That fork is in neither the ledger nor Rejected. The button is still fine UX; the ledger entry and the `prior_enabled` definition are missing.

**C3 — Fork B's stated walk is incomplete (conclusion survives).** "Once NSIS has removed the old exe, no process can start from it" ignores the pre-broken-entry case: entry already Broken before the update, user launches the install-dir exe mid-NSIS → heal **does write inside the window**. It writes the install path — which NSIS is repopulating in place — so the value is exactly the post-update target. Safe, but for a reason §3.2 doesn't state.

Also: ledger **D4** overstates convergence — fable owned readers/heal-writers but kept the plugin's *enable-time* writer; "own the writers" was in truth the Fork A dispute, not a converged point.

## 2. Wrongly rejected / adopted

**Fork A — concede, decisively.** Verified `windows.rs:37-43`: `enable()` is the unquoted serializer, and fable's §7 "Healed entries are quoted" tacitly admits fresh ones aren't. Grep confirms the full plugin surface is exactly the plan's list (`commands.rs:51,55,445-446`, `main.rs:23,556-557,609,614`, `updater.rs:364-365`); capabilities grant no `plugin:autostart` (only app commands `allow-get-autostart-enabled`/`allow-set-autostart`); no JS guest package; `tauri-mock.js:33` returns a bare bool (in the change map). Nothing unlisted breaks. **D12 — concede:** verified `tauri-plugin-autostart/lib.rs:214-222` prefers `env.appimage`; fable's step 7 writes `&amp;exe`, i.e. the ephemeral `/tmp/.mount_*` path under AppImage — a genuine bug in the prior leg. **D14 — concede:** macOS `current_exe()` is the exec-time `_NSGetExecutablePath` snapshot; moved-while-running → stale, so fable's Fix silently no-ops. (Linux `/proc/self/exe` follows same-fs moves, so `can_repair_now` is usually true there — consistent.) **Fork C — concede** as ledgered. **One undocumented drop:** fable's heal step 4 skipped healing when `is_enabled()==false` (TM-disabled). The consolidated heal rewrites TM-disabled Broken entries — defensible given §4.5 never touches StartupApproved, but it's an unledgered behavioral delta, and its rearm-side twin is C1.

## 3. Blind spots (neither leg)

**B1 — L6 arms real crash recovery on the runner.** Seeding the Run value means the production build's heal-then-rearm creates a real schtasks task; the smoke then `Stop-Process -Force` (accelerator.yml:596) — a "crash" — and the task relaunches the app during/after assertions. Flake generator. L6 needs schtasks + Run-value cleanup before the kill.

**B2 — `stored_path` is a new untrusted string into the webview.** Today Settings receives a bool; now an attacker-influenceable (same-user) artifact string reaches the DOM. §9 never says "render as textContent, never innerHTML."

**B3 — reader-side decode.** L2's oracle validates *writes* only. L1's "round-trip" must explicitly assert read-side XML-unescape (`&amp;amp;`→`&amp;`) and `.desktop`/Run unquote *before* the resolve check, else a healthy `&amp;`-path misclassifies Broken and gets rewritten (identically) every launch while status lies.

## 4. Fork B verdict

**Defer the marker; the heal is safe in this PR.** I tried the interleavings: old-exe process pre-deletion sees a resolving entry (no write); post-deletion it cannot start; the only write-in-window case (C3) writes the install path NSIS is itself converging on; catastrophic NSIS failure leaves a Broken-again entry that the next launch heals — never a resurrection, because only `Broken` writes and the entry pre-existed. The hazard in that window is the *rearm*, which is today's bug, untouched. Q1's framing — marker fixes crash-recovery, TTL + path-based removal instead of codex's fail-closed brick — is right. Ship without it; take Q1 to the owner as its own PR.

## 5. Tests

**Highest-value missing test:** one *unmocked* IPC round-trip — a Linux webdriver spec (built-debug leg, exists today, zero autostart coverage): seed a Broken `.desktop`, open real Settings, assert the broken row, click Fix, assert the artifact healed on disk. A forgotten `allow-repair-autostart` ACL entry or serde camelCase drift ships green through all of L1-L6, because L5 mocks both sides. Note the webdriver gate only excludes the *startup* heal, not the command path — this test is possible. **L4 is not ceremony** — it is the only hermetic proof of divergent `XDG_CONFIG_HOME` and unset-`HOME` no-panic — but it's thin; keep it because it's cheap and the owner asked for Docker.

## 6. Fine

All §8 Facts verified at cited lines (lock at `updater.rs:44/272`; "Windows never reaches here" comment; no bare macOS `cargo test` — only `:109`/`:535`; cert-trust runs `--test` only, `:157/161`). D1, D11, D15 sound. §9's Windows finding is correctly scoped: persistence-hijack, same-user, not EoP — neither over- nor understated.
