# Competing outline — "fix the instrument before you take the measurement"

An alternative sequencing of the same scope. Written to be argued against `plan.md`, not to be a
strawman. Audits should say which is right and why.

## Thesis

`plan.md` sequences by *risk of the change*: safe things first, the rename last, and the Windows
updater-smoke fixture last of all (Phase 4) because it is "the blind spot".

That ordering has the dependency backwards. **The fixture is not a deliverable, it is the
instrument.** `_e2e-updater-windows.yml` builds its N-1 from the current checkout rather than
downloading a real prior release (recon B2 #4). That means it does not test "can users upgrade from
the last shipped version" — it tests "can this build upgrade to itself". For the rename specifically
it is *structurally incapable* of catching the failure it exists to catch, because N-1 inherits the
new binary name too.

So under `plan.md`, Phase 3 (the rename) is gated by a Windows check that will fail for a reason that
has nothing to do with whether the rename is safe, and the only way through is to change the
instrument anyway — but by then it is being changed under pressure, on the critical path, to unblock
a release. That is the worst moment to touch a test fixture.

## Proposed order

**Phase A — Repoint the Windows updater-smoke N-1 at a real prior release.**
Match what macOS (`updater-smoke.sh`, downloads a real past DMG) and Linux already do. No product
change at all. Prove it by running `_e2e-updater-windows.yml` on an *unmodified* HEAD: it must still
pass. A fixture change that is proven green before any product change is a fixture change you can
trust afterwards.

*Gate*: `workflow_dispatch` of `_e2e-updater-windows.yml` on this branch, unmodified product code,
both positive and negative legs green.

**Phase B — `mainBinaryName` + the five lockstep CI fixes.**
Now the rename is validated by an instrument that can actually observe it: N-1 is a real old build
with the *old* binary name, N has the new one, and the upgrade either works or does not. This is the
only phase in the whole plan whose correctness we currently cannot prove, so it goes as early as the
instrument allows.

*Gate*: as `plan.md` Phase 3, **plus** the Windows updater smoke now being a meaningful signal
rather than a fixture artifact.

**Phase C — Bundle metadata.**
Config-only, zero binary-path impact. Deliberately *after* the rename here: if metadata and rename
are separated, and the rename has already landed and been proven, then any bundling failure in this
phase is unambiguously attributable to the metadata.

*Gate*: as `plan.md` Phase 2.

**Phase D — Remove "allow once" + the decision record.**
Last, not first. It is the only change in this plan with *semantic* risk — it alters a security
property and reverses an audited decision. It deserves the most reviewer attention, and it gets that
attention if it is not competing for it with a release-pipeline scare. It also has zero coupling to
anything in A–C, so nothing is blocked by deferring it.

*Gate*: as `plan.md` Phase 1.

**Phase E — Version in Settings** (optional, unchanged).

## Where this is genuinely better

- The one thing we cannot currently verify (Windows rename-across-upgrade) becomes verifiable
  **before** it is needed, rather than being converted into a documented blind spot (`plan.md`'s
  ASK-1 option (b)/(c)).
- It removes the possibility of Phase 3 failing CI for a fixture reason and someone "fixing" the
  fixture hastily to get green — the exact failure mode that produces a test which passes without
  testing anything. This session already hit that class of bug twice: a `docker run` without `-i`
  that exited 0 having run nothing, and an NSIS guard that could never fire.
- The riskiest *mechanical* change lands early, when there is still time to revert before a release.

## Where this is genuinely worse

- **Phase A may be large or impossible.** Downloading a real prior Windows release requires one to
  exist with a compatible installer, plus network access in CI and a pinned known-good version.
  If that turns into a project of its own, the whole plan stalls behind it — whereas `plan.md`
  delivers the popup change and the metadata regardless.
- **It front-loads work the owner did not ask for.** The ask was two items; this outline makes CI
  fixture engineering the first thing that happens.
- **It delays the owner-visible wins.** The popup and the Publisher fix are what the owner will
  actually see; under this ordering both land last.
- The argument assumes the fixture is *worth* fixing. If Windows upgrades are rare relative to
  fresh installs, a documented blind spot may be the correct engineering trade.

## The question the audits should settle

Is `_e2e-updater-windows.yml`'s synthetic N-1 a **defect to fix now** (this outline), or a **known
limitation to document and route around** (`plan.md` Phase 4 / ASK-1)? Everything else in the two
orderings follows from that one answer.
