# Piece-3 plan audit — codex (xhigh, read-only)

## Verdict: reject

### Blocking

1. **The event split misroutes every release.** The release workflow itself is `workflow_dispatch` ([release-accelerator.yml:3]). A called workflow’s `github` context belongs to its caller, so both calls at lines 582/599 see `github.event_name == 'workflow_dispatch'`. Thus the production job is skipped and the ephemeral dispatch job runs instead ([plan.md:35]). GitHub confirms this [caller-context behavior](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations). Keep `_e2e-updater-windows.yml` call-only and put the zero-secret manual path in a separate workflow.

2. **The stated pre-merge dispatch gate is unavailable.** GitHub requires a manually dispatched workflow to be configured on the default branch; adding `workflow_dispatch` only on this feature branch cannot reliably support plan gate 2 ([plan.md:87]; [GitHub documentation](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow?tool=cli)). Specify a bootstrap mechanism, temporary safe push trigger, or post-merge burn-in/revert procedure.

3. **Q is vacuous, including D22.** Every proposed assertion can pass if Q immediately exits ([plan.md:71]). Require: port initially unreachable; Q remains alive; `/health.version == N1Version`; an additional `latest.json` request occurs after Q starts; artifact-download count remains unchanged. Health is especially strong because it starts after marker reconciliation ([main.rs:601]).

4. **Barrier ownership is not proven until release.** A bounded missing-ready failure is required; `p-status` cannot fail if execution never reaches it. Also have the sentinel emit `timed-out`, and assert that absent immediately before writing release—otherwise its 120-second fail-open can silently end the measured window ([plan.md:58]). The existing negative leg remains vacuous after download if N−1 crashes; require final `/health == N1Version` or explicit rejection evidence ([updater-smoke-windows.ps1:245]).

### Non-blocking

- `N > 1.0.7` by SemVer is correct: F-004 rejects `candidate <= current`. The release artifact is patched to 1.0.7 even though tag source says `1.0.7-rc.1`.
- Compare the exact endpoint, not merely its host, and export `GH_TOKEN`.
- Use run-unique barrier files under runner temp and clean them; current names do not collide semantically but pollute product state.

### Sound

The ephemeral chain is complete: both binaries embed the ephemeral pubkey, Layer A reads that bundled key ([updater.rs:14]), and the same private key signs N’s artifact and manifest. No production key is needed.