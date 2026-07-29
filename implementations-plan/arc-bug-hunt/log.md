# Post-merge bug hunt — pieces 3 + rename + publisher (owner-requested, until-clean loop)

Scope: the merged combined diff `08a4670..main` (#425, #426, #427) plus piece-2 product code in
its post-rename context. Loop: fresh codex hunt at xhigh → every finding verified against the
code → real bugs fixed through normal gates → resumed "what did you miss" → repeat until a round
survives with zero verified findings. No style/architecture findings admitted (owner rule).

## Round 1 (session `019fb002-ae00-79d1-a50c-c88ef364c8cb`): 3 findings, 0 release-blocking

1. **CONFIRMED (Medium, fixed)** — identity-guard routing hole: the four grep-bearing workflows
   were in the `desktop` paths-filter but NOT `integration`, and `test:scripts` (which runs
   tauri-identity.test.ts) lives in the integration-gated job — a workflow-only PR reverting a
   rename site would skip the exact guard that pins it. Fixed: paths mirrored into `integration`.
2. **CONFIRMED (Medium, fixed)** — D22 post-milestone false-pass window: the download count was
   sampled once, immediately after the rejection log line; a regression that logs the decision
   but still schedules the download asynchronously could pass before the request reached the
   feed. Fixed: bounded 5s settle between milestone and sample (distinct from the pre-milestone
   sleep race fixed in #425 — the milestone stays the primary proof).
3. **ACCEPTED residual (Low)** — fixture preflight checks the TAG's conf, not the downloaded
   asset's provenance (asset-from-commit-A vs tag-at-B divergence). Our release assets are
   pipeline-uploaded from the tagged commit; manual asset tampering on our own repo is outside
   the fixture's threat model, and the N−1 launch proof (`/health == N1Version`) catches
   functional mismatches. Documented, not coded.

Checked-and-clean (codex, round 1): both name-regimes of the ps1 (dispatch same-name, call
split-name) incl. Q/$expectedHeal/boundary asserts; rename-aware marker reconciliation;
publisher-flip silent-install default-dir resolution without registry continuity; sentinel
injection; signing-key cleanup; identity assertions as written.

## Round 2 (resumed, same session): 2 findings, BOTH release-blocking, both product bugs

Self-critique it offered first: round 1 framed Q only as a hostile observer inside an existing
marker window, so it never looked at a copy's ORDINARY startup or a copy that OWNS an update.

**F1 — CONFIRMED (High, fixed): any copied instance steals the crash-recovery task.**
`startup_rearm` gates on lock + no-live-marker + `intent_enabled_now()`, but intent only means
"an entry exists and isn't platform-disabled" (`autostart.rs:1236`) — it never asks WHO this
process is. `enable_crash_recovery()` then serializes `current_exe()` into the task XML
(`crash_recovery.rs:384`) / systemd ExecStart. So launching a leftover `Downloads` copy once —
even while the installed app is healthy — re-points recovery at the copy, which then exits on the
port check. Delete the copy, crash the real app: recovery launches a deleted path forever. Piece 1
forbade exactly this theft for the Run value (`points_elsewhere`: "a healthy entry is never
silently stolen by whichever copy launched last") — the TASK had no such rule. The barrier smoke
could not see it: its Q runs inside a live marker, where rearm is suppressed anyway.
**Fix**: `implicit_arm_allowed` (pure, table-tested) + `implicit_arm_gate` (effectful) — implicit
arming paths (startup rearm, reconcile's arm callback, the updater guard's rearm) require
`Healthy { points_elsewhere: false }`; explicit user toggles/repair stay ungated (they mean "arm
THIS exe"); macOS exempt (its recovery patches the stored entry itself and cannot steal).
Rename-boundary safety: at reconcile the stored value is still Broken (old exe deleted), so the
gate declines there and the same-launch `startup_rearm` arms after the heal makes us the owner.

**F2 — CONFIRMED (High, fixed): a copy that owns an update poisons `expected_install_path`.**
`updater.rs` recorded `current_exe` with a comment asserting in-place replacement — false when a
copy drives the install: the updater passes `/UPDATE` and NO `/D`, so NSIS resolves `$INSTDIR`
itself and installs at P. N then sees `expected != exe_canon` with the copy still present (absence
unprovable) → reconciliation suppressed → heal/rearm/updates blocked and recovery left disarmed
until deadline + another launch.
**Fix (codex option C, chosen over an ownership gate on update-initiation)**: record what the
INSTALLER will use — a pure mirror of `RestorePreviousInstallLocation` (`installer.nsi:873-877`):
MANUPRODUCTKEY default value when non-empty, else `%LOCALAPPDATA%\<productName>`, joined with the
binary name. Registry NotFound ⇒ default; any OTHER registry error ⇒ ABORT the update rather than
publish a guessed marker. Option A (gate initiation) was rejected because it leaves
autostart-OFF users and copies that legitimately own an entry unprotected; option B (accept as
TTL residual) leaves ON users with recovery disarmed until another launch.
Pre-#427 installs wrote the OLD manufacturer namespace, so they resolve NotFound → default —
which is exactly where those (default-dir, dev-only) installs live, so the mirror stays faithful.

**Tests added** (codex's required list, minus the one it asked for that we judged covered):
resolver decision table (registry / blank / missing / undeterminable); implicit-arm decision
table; `tauri-identity.test.ts` pins every input to the destination mirror (Rust consts vs
productName + publisher + binary name, `installMode: currentUser`, no `/D` or installerArgs) —
and that new assert was MUTATION-TESTED (drift the registry path ⇒ 1 fail; restore ⇒ 11 pass);
new dispatch-only smoke mode **`copy-initiator`** — the copy drives the update with the copy
retained and autostart OFF, asserting N lands in the install dir, the transaction is fully
reconciled away, the copy still exists (so proven-absence can't mask a wrong path), and NO
crash-recovery task was armed. That single mode is the end-to-end regression proof for both F1
(OFF-intent implicit arm) and F2.

Windows-only code could not be compiled locally (cross-compiling the C deps — ring/aws-lc-sys —
needs a Windows C toolchain we don't have). Mitigation: the winreg API was verified against the
vendored 0.55 sources (`io::Result` on both calls, empty-name default-value read) and the exact
idiom already proven on Windows in `autostart.rs`; CI's Windows legs compile it.

### Validation run 1 (30499816033): the regression test's OWN assert was malformed

`copy-initiator` failed — on my assert, not the product. Order matters: every earlier assert
passed, including the two that carry the proof (transaction files GONE ⇒ N reconciled the
copy-initiated marker, i.e. the F2 fix works end-to-end; and N present in the install dir). The
failure was `Test-Path $QDir\AztecAccelerator.exe` → "N was installed BESIDE the copy", which is
trivially TRUE because on the dispatch path the copy has the SAME file name as N. A name-existence
check cannot distinguish "the installer wrote N here" from "the copy I made is still here".
Replaced with a SHA-256 before/after comparison of the copy's own bytes — the property actually
being claimed. Lesson: when two things share a name, identity asserts must key on content, not
existence; the dual name-regime introduced by the rename makes this a recurring trap in this
script.
