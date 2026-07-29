# /goal seed — autostart pieces 2 & 3, then the AztecAccelerator rename

Paste as `/goal`. Derived from the piece-1 session (PR #422, `921d5ab`) and its lessons.

---

/goal Ship the remaining autostart arc in `alejoamiras/aztec-accelerator`, in this order, each as
its own worktree + blueprint + PR merged green before the next starts:

**(1) Piece 2 — the Windows update-window marker.** `/blueprint mid` (NOT deep — the surface is
known and the design already survived three adversarial rounds; the ledger is in
`implementations-plan/autostart-self-heal/plan.md` §3.2 Fork B + D18/D21/D22). Implement:
`update-in-progress.json` carrying a transaction-ID nonce, candidate version, canonical expected
install path and an in-payload deadline; a NEW production `NSIS_HOOK_POSTINSTALL` completion token
(`hooks.nsi` has only POSTUNINSTALL today — this is new installer code and a release cannot fix its
own installer, so it must be right first time); marker-aware `perform_update` (compare-and-create,
reject a live foreign marker); the removal call site in `main.rs` startup, requiring version AND
canonical path AND completion token AND recovery-reconciled-to-intent, with the removal transaction
exempt from the no-rearm rule and holding `autostart.lock` across intent-read → reconcile → remove.
Then REMOVE the `not(target_os = "windows")` gate on the startup heal in `main.rs` and flip the L6
smoke assertion from "seeded value unchanged" to "healed to the exactly quoted installed exe".

**(2) Piece 3 — the Windows updater dispatch gate + L8.** `/blueprint light`. Give
`_e2e-updater-windows.yml` a `workflow_dispatch` path that builds N in-job (its `n-artifact` input
defaults to the same run's `build` output, which is why the trigger was removed); switch the
`workflow_call` release path's N−1 to the real `accelerator-v1.0.7` with the preflight in plan.md
§6 L8; put the barrier sentinel in the SYNTHETIC N−1's `NSIS_HOOK_POSTUNINSTALL` — the one moment
the installed target is absent. Codex's claim that `/UPDATE` skips the old uninstaller is REFUTED
by wine-measured evidence in `nsis/hooks.nsi:5-31`; do not re-adopt it. Assert `P` is non-resolving
at the barrier rather than assuming it. Separate jobs for dispatch vs production signing —
`workflow_call.secrets` is not an isolation boundary — and keep the prod key behind a release-only
environment.

**(3) The `AztecAccelerator` rename.** `/blueprint light`. Must follow piece 2: it changes the
install dir and exe name, which IS the marker's expected path. `mainBinaryName: "AztecAccelerator"`
(no space — a spaced name breaks Windows autostart and Linux `Exec=`), 8 lockstep CI sites, and the
N−1 fixture piece 3 delivers.

## How to work

- **Worktree per piece**, named for it (`autostart-update-marker`, `updater-dispatch-gate`,
  `binary-rename`), off updated `main`, registered via `agent-worktree`.
- **Tests inline with each change, at the layer where the bug can actually exist.** Piece-1 lessons
  that are now rules: assert a test's precondition, never `return` out of it; a test that accepts
  two outcomes must assert WHICH path ran; never let a test mutate shared machine state it cannot
  prove it can restore (HKCU `Run`, the real autostart artifact); when fixing a read path, grep
  every consumer before calling it done.
- **Review loop per PR:** local gate (`bun run test` + `bun run lint:actions`) → push → CI green →
  ONE codex audit at `xhigh` → fix → ONE **resumed** codex verification pass asking only "are your
  own findings fully closed, or half-applied?" (that second call is cheap and is what caught real
  defects three times) → merge. Do not run a third fresh sweep.
- **Do not over-engineer.** No new abstraction until something is duplicated three times; no
  restructuring for testability beyond the established `*_at()` AppHandle-free-core pattern; a
  finding that requires rework or expands scope gets DOCUMENTED as a residual in
  `lessons/phase-N.md` with the argument, not implemented.
- **Log lessons as you go** in `implementations-plan/<plan>/lessons/phase-N.md`; keep
  `implementations-plan/index.md` current; update `CLAUDE.md` test counts in the same PR.

## Come back to me for

Release tags, publishing, force-pushes, history rewrites, prod credentials — never autonomously.
Also stop and ask if: a piece turns out to need product-code changes beyond its plan, three
attempts on the same step fail, or a review finding would change the piece split.

---

## Paired /loop seed

/loop Work the current piece's next unfinished phase. Run its gate before moving on; if it fails,
fix and re-run rather than continuing. Log every non-trivial attempt in
`implementations-plan/<plan>/lessons/phase-N.md`. After three failures on the same step, stop and
report instead of trying a fourth approach.
