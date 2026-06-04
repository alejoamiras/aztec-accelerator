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
