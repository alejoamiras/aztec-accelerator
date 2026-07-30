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

### Validation run 2 (30500599861): copy-initiator GREEN on real Windows

"copy-driven update installed at the install dir, transaction fully reconciled with the copy still
present, and no crash-recovery task armed" — the end-to-end regression proof for F1 (OFF-intent
implicit arm) and F2 (destination mirror). Note this run predates the round-3 fixes below; re-run
after them.

## Round 3 (resumed): 5 findings, 3 release-blocking — ALL introduced by my round-2 fixes

The loop's most valuable round: the fixes themselves were the bug source.

**r3 #1 — CONFIRMED (High, fixed): the gate could strand recovery permanently.**
`gated_enable_crash_recovery` returned `Ok(())` when it DECLINED, so reconciliation treated a
decline as a successful arm and deleted the marker. On the rename boundary with a read-only/ACL'd
autostart entry (readable, not writable): entry is `Broken` (old exe deleted) → gate declined →
marker removed → heal failed to write → every later rearm kept declining the still-`Broken`
entry. Crash recovery gone permanently, nothing left to converge it. Same for a parse-unreadable
artifact.
**Fix — the rule is now narrowed to exactly the theft it must stop**: decline ONLY
`Healthy { points_elsewhere: true }` (another live binary provably owns a working entry). `Broken`
arms (nobody owns a working entry; whoever launched becomes owner — precisely what the heal writes
next, so this matches piece 1's audited resolve-based semantics). `Unreadable`/`Absent` arm (no
proof of theft; declining would silently drop recovery for endpoint-managed users). This mirrors
piece 2's proven-absence epistemics: act only on what can be PROVEN, and never let an unprovable
state become permanent.

**r3 #2 — CONFIRMED (High, fixed): the new abort path ran after the global disarm.**
`nsis_install_destination()` was called inside the critical section — after `record_pending` and
after `disable_crash_recovery()`. An unreadable registry therefore aborted with an installed
instance's recovery task already deleted and no marker to reconcile it back (and the guard's
ownership-gated rearm correctly refuses when a copy is the initiator). **Fix**: hoist the
destination resolution above the critical section — it is a read-only registry+env lookup, so an
error now aborts before ANY state changes.

**r3 #3 — CONFIRMED (High, fixed): AppImage owners could never pass the gate.**
Linux autostart deliberately stores the real `.AppImage` path while `current_exe()` is the
ephemeral `/tmp/.mount_*` squashfs (`desired_path`, D12) — so my gate computed
`points_elsewhere: true` on EVERY AppImage launch. After a logout destroyed the mount recorded in
the systemd unit, no launch would ever rewrite it. **Fix**: the gate takes the ownership
`reference` as a parameter; the Linux/macOS caller passes `desired_path(app)` (which resolves
`$APPIMAGE`), Windows callers pass `current_exe()` (no AppImage indirection there). The pure
decision table never modelled this identity split — the lesson is that a "pure core" only tests
the logic, not whether callers feed it the right identity.

**r3 #4 — CONFIRMED (Medium, fixed): copy mode could pass without a marker ever existing.**
It only asserted the transaction files were ABSENT afterwards, which also holds if marker creation
was skipped entirely. `updater.rs` now logs `update window marker armed` on successful create
(D24-safe: version only) and the smoke requires that line before trusting the absence assert.

**r3 #5 — CONFIRMED (Medium, fixed): the `/D` pin missed runtime builder args.**
A `.installer_arg("/D=…")` on the updater builder would redirect NSIS while both conf-level
assertions stayed green. The identity test now greps all Rust sources for `installer_arg` —
mutation-tested (inject the string ⇒ 1 fail; remove ⇒ 12 pass).

Checked clean (r3): the currentUser destination mirror vs the 2.8.1 template; the old publisher
key ignored by both sides; 8.3 names, casing, junctions, trailing separators converge through
canonicalization; fresh explicit ON; macOS and Linux non-AppImage; the SHA-based copy assert.

### Validation runs 3+4 (30501446852 copy-initiator, 30501460093 barrier): both GREEN on the r3 code

barrier passing matters as much as the new mode: it proves the ownership gate did not break the
marker-removal transaction's own rearm path.

## Round 4 (resumed): 4 findings, 1 release-blocking — the High is a correction to r3's correction

**r4 #1 — CONFIRMED (High, fixed): allowing `Unreadable` to arm reopened the original theft.**
Concrete Windows path, verified in the source: endpoint management writes a WORKING Run value with
arguments (`"C:\Installed\AztecAccelerator.exe" --managed`); `run_value_candidates` rejects
trailing arguments as "not the owned format" (`autostart.rs:556`) ⇒ `Unreadable`, while
`artifact_present` reads presence WITHOUT parsing ⇒ intent still ON. So my r3 "allow Unreadable"
let a stray copy arm recovery at itself — the exact F1 bug, reachable via startup rearm and via the
updater guard.
**Fix**: `Unreadable` DECLINES again; `Broken` still arms (that is what fixed r3 #1's stranding,
and it is the rename-boundary case that affects every user). The final rule: arm iff proven-ours,
Broken, or Absent; decline iff proven-foreign or unknowable.
**Disagreement with codex, deliberate**: it proposed a per-caller policy matrix (allow unknown at
reconcile, decline at startup/guard). Rejected as machinery for a rare configuration — a single
rule kills the theft in all three contexts and keeps the boundary convergence. The cost is stated
as an ACCEPTED RESIDUAL in the code: unparseable/managed entries get no implicit rearm, so after an
update their recovery task stays disarmed until an explicit toggle or Repair. Safety over
availability, matching the module's standing "never act on what we don't understand" doctrine (the
heal refuses `Unreadable` too). Noted for later: arming recovery at the INSTALL DESTINATION rather
than `current_exe` would give both properties, but it changes audited crash-recovery serialization
on all three platforms — not worth it now.

**r4 #2 — CONFIRMED (Medium, fixed): the "before any state change" abort still wrote `pending`.**
`record_pending` ran before the destination lookup, so a bad install-location value aborted with
`pending = candidate` recorded. F-004's `candidate_allowed` then requires `candidate >= pending`
(`core/src/updater_state.rs:113-130`) — a WITHDRAWN release would block the fixed, lower-but-newer
one forever. Fix: the lookup now precedes `record_pending` too, so the abort truly mutates nothing.

**r4 #3 — ACCEPTED residual (Medium): cached destination vs a concurrent registry writer.**
Between our read and NSIS's own `.onInit` re-read, another installer/management agent could
rewrite the install-location key; the marker would then predict A while NSIS installs at B, and
with A still present, proven-absence cannot rescue it. Unfixable without cooperation from the
installer (the window exists regardless of where we read), rare (requires a concurrent writer),
and BOUNDED: the marker expires on its in-payload deadline and the next launch removes it via the
Expire path. Documented, not coded.

**r4 #4 — CONFIRMED (Medium, fixed): the marker-armed proof was neither fresh nor run-scoped.**
It asserted PRESENCE across a persistent daily log — and copy mode launches the installed N−1
first (for the launch proof), whose own 5-second update poll can arm a marker before it is killed.
So P's line could satisfy a check meant to prove Q armed one. Fix: baseline the count before the
copy launches; require an increase. (Same class as round 1's D22 milestone fix — presence asserts
in append-only logs need baselines.)

Checked clean (r4): AppImage compares against `$APPIMAGE`; Windows callers use `current_exe`;
macOS canonicalization agrees with plist classification; the destination mirror is correct absent an
external writer; the installer_arg pin catches singular and plural builder methods.

**Loop judgement (codex, asked explicitly): CONTINUE for one focused round**, targeting the
unknown-owner policy across the three arming contexts, then STOP — after that, residual risk should
be dominated by real-release/real-fleet behaviour rather than more static rereading. Round 5 is
scoped exactly to that.

## Round 5 (resumed, scoped to the target codex itself named): 3 findings, 2 release-blocking

**r5 #1 — CONFIRMED (High, fixed): the marker cleanup paths bypassed the gate.**
Three `post_create_failure_cleanup` call sites (`updater.rs`) passed the RAW
`enable_crash_recovery`. So a copy-initiated update whose marker publication / handoff write /
install FAILED would still re-arm recovery at the COPY on the way out — the F1 theft, reached
through the failure path instead of the happy path. The copy smoke only exercises a SUCCESSFUL
install, and the existing cleanup unit tests inject ordering counters, not ownership. Fixed: all
three cleanups now take `gated_enable_crash_recovery` (made `pub(crate)`).

**r5 #2 — CONFIRMED (High, fixed) — and PRE-EXISTING, not introduced by this work.**
My r3 AppImage fix corrected only the gate's COMPARISON path; `crash_recovery::enable_impl`
(Linux) still serialized `current_exe()` into `ExecStart`, which inside an AppImage is the
ephemeral `/tmp/.mount_*` squashfs. The recovery unit therefore pointed at a path that vanishes
exactly when recovery is needed — for every AppImage user, since before this arc. Fixed with a pure
`recovery_target(appimage, exe)` helper (prefers `$APPIMAGE`, ignores an empty value) + unit table.
This is the arc's clearest evidence for hunting the same code repeatedly: four rounds of ownership
work were needed before anyone looked at what the writer actually writes.

**r5 #3 — CONFIRMED (Medium, fixed): macOS lost its exemption.**
Parameterising the gate made the whole non-Windows branch gate, including macOS — but macOS
crash recovery patches KeepAlive into the app's own fixed plist and writes NO executable path, so
it cannot steal; gating it only widened the `Unreadable` residual (a managed plist using `Program`
instead of the owned `ProgramArguments` shape classifies Unreadable). macOS arms unconditionally
again; Linux keeps the AppImage-aware gate.

Checked clean (r5): in all three Windows arming contexts a declined reconcile arm still removes the
marker without double-arming; parse-unreadable users recover via OFF→ON. Honest limit it noted:
a TRULY ACL-unreadable entry cannot be repaired from inside the app (Repair skips Unreadable) —
management must restore access. Destination hoisting and the run-baselined marker proof hold.

Loop judgement: CONTINUE narrowly — audit only these two fix paths, then STOP.

### CI caught what every local gate structurally could not: a misplaced `cfg` boundary

PR #429's macOS legs failed to COMPILE (`enable_impl` defined multiple times; `SYSTEMD_NAME`,
`systemd_exec_start`, `recovery_target` not found). Cause: my r5 #2 edit inserted `recovery_target`
+ the reopened `fn enable_impl` header BETWEEN the linux `#[cfg(target_os = "linux")]` attribute and
the function it guarded. The cfg then applied to `recovery_target`, and `enable_impl` — one of THREE
platform variants — lost its gate entirely, so it was compiled on every target and collided with
the macOS/Windows definitions. On Linux everything still compiled, which is exactly why the local
gate passed. Second occurrence of this class in the arc (piece 2's `pub(crate)` E0603 was the
first): **anchor-based edits near cfg attributes can silently move an item out of its gate, and a
single-platform build cannot see it.**

Fix: restore attribute adjacency (each of the three `enable_impl`s verified gated by an awk pass
over the file).

**New local capability — a REAL second-platform compile gate.** Earlier in this session I claimed
Windows-only code "cannot be compiled locally" (the msvc target dies cross-compiling ring/aws-lc-sys
C deps). That claim was wrong for `x86_64-pc-windows-gnu`: mingw is present, and the only blocker
was tauri-build wanting a sidecar for the triple. With an empty, gitignored
`binaries/bb-x86_64-pc-windows-gnu.exe` placeholder:

    cargo check  --target x86_64-pc-windows-gnu --lib
    cargo clippy --target x86_64-pc-windows-gnu --all-targets   # warnings here are test-only noise;
                                                                # CI's clippy gate is ubuntu-only

**Mutation-proved live**: a deliberate type error inside the Windows-only
`nsis_install_destination()` makes the check fail; reverting restores green. So this gate really
does compile the `cfg(windows)` paths — it would have caught BOTH platform breaks in this arc.
Cost: ~5s incremental after the first build. This belongs in the local gate for any change touching
platform-gated code.
