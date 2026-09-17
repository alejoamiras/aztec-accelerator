# Phase 3 — P5: real-artifact Windows updater-smoke (Option B, staged)

## What shipped (the rework)
Reworked _e2e-updater-windows.yml to test the REAL prod-signed N (this run's `build` artifact)
instead of a synthetic ephemeral-signed N — true parity with mac/linux (codex's core P5 objection:
the old smoke flipped to blocking would gate releases on a synthetic test, not the shipped artifact).
- N   = downloaded `accelerator-windows-x86_64` (-setup.nsis.zip + .sig — prod-signed with
        TAURI_SIGNING_PRIVATE_KEY, embeds the committed prod pubkey).
- N-1 = synthetic 0.0.1 built in-job embedding the COMMITTED prod pubkey (NOT patched to ephemeral);
        built with a throwaway key only to satisfy tauri's updater-artifact signing (N-1's own .sig
        is never used). So N's prod sig verifies against N-1's prod pubkey.
- mac/linux download a real N-1 STABLE; Windows has no stable yet → bootstrap N-1 synthetically.
- Wiring (release-accelerator.yml): update-smoke-windows[-negative] `needs: [validate, build]`, pass
  `n-version=${{ needs.validate.outputs.version }}`, gate on build success. Timeout 40 (one in-job
  build + smoke, vs 60 for the old two-build smoke).
- KEPT ADVISORY (staged, per plan + final codex): prove the real-artifact path on a green rc dry-run,
  THEN flip to blocking (add both to tag.needs + release.needs). Revert = drop from needs.
- ps1 unchanged (serves whatever N is handed; the #96 crash-recovery arming carries over).

## Validation constraint (important)
This can ONLY run in the RELEASE pipeline — the `build` artifact exists only there, so it is NOT
workflow_dispatch-testable (unlike #96). So codex-review is the main pre-rc check; then an
OWNER-DISPATCHED rc dry-run is the validation. Iterations are expensive (rc-gated) — get it right.

## Codex P5 review (ship-with-changes)
- HIGH (throwaway key echo'd to GITHUB_ENV) = FALSE POSITIVE: the old smoke used the byte-identical
  pattern and #96's armed-smoke ran green hours ago using it (its key-dependent builds succeeded), so
  `tauri signer generate`'s key is single-line. Kept as-is (don't change a proven path before an rc).
- MEDIUM (build-gate divergence) = FIXED: dropped `&& needs.build.result=='success'` to match
  update-smoke-linux (gate on validate only) so an unrelated mac/linux leg failure doesn't skip the
  Windows validation; the download-artifact step fails cleanly if the windows artifact is missing.
- Trust chain, artifact flow, version/feed (no live 9.9.9), negative leg: all confirmed fine.

## rc.2 caught a flat-glob bug (fixed)
rc.2's Windows smoke failed at "Stage N updater artifacts": `cp n-dl/*-setup.nsis.zip` → No such
file. upload-artifact NESTS the nsis files in `n-dl/nsis/` (the build's path globs span dmg/deb/
appimage/macos/nsis → common ancestor is `bundle/`, so the subdir is preserved). Fixed with a
recursive `find` (mirrors the linux smoke's `find n1 -name '*.AppImage'`) + a clear error that lists
the artifact tree. The trust chain / update logic wasn't reached — re-validate on the next rc.

## rc.3 GREEN — real-artifact smoke validated (the staged advisory proof)
1.0.4-rc.3 dry-run: the recursive-find fix (#280) unblocked the staging step that failed on rc.2,
and the smoke then exercised the full REAL trust chain end-to-end:
- **Updater Smoke (windows-x86_64 / positive): success** — the real prod-signed `-setup.nsis.zip`
  installed as a click-free minisign-verified auto-update FROM the synthetic N-1 (which embeds the
  committed prod pubkey, so N's prod signature verifies). Asserted /health == N.
- **Updater Smoke (windows-x86_64 / negative): success** — tampered artifact rejected vs the prod pubkey.
- Create Git Tag + Create GitHub Release: success — the run TAGGED + RELEASED (not wedged).
This proves Option B's central claim (a synthetic N-1 embedding the prod pubkey verifies the real
prod-signed N) on a real release run. The advisory stage did its job.

## Flip to blocking (#281, merged)
With rc.3 green, executed the staged flip: added `update-smoke-windows` + `update-smoke-windows-negative`
to `tag.needs` + `release.needs` in release-accelerator.yml. `tag` is pure needs-gated (no `if:`), so a
failed Windows smoke now skips `tag` → blocks `release` — identical blocking semantics to the already-
working mac/Linux gates. The Windows smoke's own `if: !cancelled() && needs.validate.result=='success'`
introduces no new wedge path (validate is already a shared need). actionlint clean; PR #281 squash-merged.

**Owner decision (logged):** skip a confirming blocking-rc (rc.4). rc.3 already proved the smoke green
(pos+neg) against the real artifact; the flip only wires it into needs (mechanically identical to the
proven mac/Linux gates). A green rc.4 would merely re-confirm green-passes — it can't prove block-on-
failure without a deliberate red smoke (undesirable in a release dispatch). The goal's P5 condition
("in tag/release needs AND proven by a green rc that isn't wedged") is satisfied: in needs via #281,
green proof via rc.3.

## Outcome
P5 COMPLETE. Windows updater-smoke is a BLOCKING release gate testing the REAL prod-signed artifact —
full parity with mac/Linux. North star met: Windows reaches the same release-pipeline assurance.

## Post-impl reviews (loop step 6)
**`/code-review max --fix` → clean (no fixes).** Inline expert pass on the b166504..53d40e0 diff
(CI YAML + ps1 + 2 lockfile lines). Verified directly: #95 lock completeness (accelerator-server is
genuinely ABSENT from src-tauri/Cargo.lock — tauri links only the lib; both locks have zero
version-qualified local-crate refs → the bump-source sed is drift-safe), the blocking-flip wiring,
the trust chain, `$Exe` null-guard (ps1 L159) before the #96 arm step, the re-arm assertion's teeth.

**Codex post-impl audit (session 019e9378…, xhigh adversarial) → verdict: minor-only.** No
release-bypass, no trust-chain break. Confirmed fine: real-artifact trust chain (synthetic N-1 keeps
the committed updater pubkey → Tauri verifies N's bytes against it), #95 in-tree fix + sed safety,
#96 arming realism (the HKCU Run-key write hits the SAME `is_enabled()` predicate the app uses at
startup, main.rs:259).
- **LOW (fixed):** the two windows-smoke job comments were stale — said "(advisory)" post-flip and
  under-specified the gate. Rewrote to state `!cancelled()` overrides the implicit needs-success gate
  (so a build failure does NOT skip the job; a missing windows artifact fails the download → blocks
  tag/release). NOTE: codex misread this as "build-failure → skip"; the `!cancelled()` idiom is the
  canonical override, so the job RUNS even when build fails — the new comment makes that explicit so
  the next reader doesn't repeat the misread. Either way codex agreed it CANNOT wrongly-pass a release.
- **MEDIUM (deferred → task #97):** the #96 crash-recovery check is END-STATE-ONLY (asserts the task
  is present after the update); it does NOT prove the disarm-before-install actually ran. A no-op
  regression of `disable_crash_recovery()` (crash_recovery.rs:281, an schtasks /Delete OS-call NOT
  directly unit-tested) would pass green. Real residual gap in a now-BLOCKING gate. Fix needs a
  disarm-success log in updater.rs + app-log capture in the smoke + grep — out-of-scope for the docs
  wrap-up (touches the update hot path + logging infra). Filed as task #97; surfaced to owner.

## Gates (transcript evidence)
- `bun run lint:actions` → exit 0 (actionlint).
- `bun run test` → all stages green: biome (51 files, 1 pre-existing non-failing warning unrelated to
  this diff), sort-package-json ✓, cargo fmt --check ✓, sdk lint ✓, tsc ✓, units 23+73+5 = 101 pass / 0 fail.
