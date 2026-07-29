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
