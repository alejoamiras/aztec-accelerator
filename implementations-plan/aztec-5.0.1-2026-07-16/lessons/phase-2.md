# Phase 2 lessons — landing the PR (2026-07-16)

- PR #395 (bump commit + swap commit kept separate) merged 16:38Z; all CI green: sdk.yml native-bb e2e (`transmit`), app.yml with the 3-graph typecheck + the NEWLY-ENABLED local-network token spec (passed 2nd run after the sessionAddresses fix; see phase-3 lessons for the find), the new `test:scripts` step, actionlint.
- First run failed CORRECTLY: the enabled token spec caught the genesis-sender bug — exactly what re-enabling it was for. Job time with the WASM token flow: ~4.5 min total (within budget; spec stays enabled).
- Local runtime-download gate vs bb 5.0.1: `download_and_verify_bb ... ok` (download + digest + extract).

LESSONS_FILE=implementations-plan/aztec-5.0.1-2026-07-16/lessons/phase-2.md
