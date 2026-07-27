# Post-implementation: code review + codex audit — 2026-07-16

## /code-review max --fix (workflow wf_e4d99a43, 31 findings verified → 15 net)

Applied 13, skipped 2. The commit (`fix: code-review findings from the 5.0.1 bump (max review)`, PR #397) carries the full itemization. Highlights:

- **A real bug the new typecheck caught immediately**: `check-aztec-update.ts` passed `process.argv[2]` (`string | undefined`) into `getLatestVersion(tag: string)` — the new `tsconfig.scripts.json` gate (finding 9) flagged it on its first run. The gate paid for itself before it landed.
- **A silently-dead auto-insert**: `pinWindowsBbChecksum` anchored on `"};\n\nexport function resolveWindowsBbChecksum"`, but `windowsBbReleaseTag` has followed the map since it was added — every bump since then quietly took the "add it manually" path. Now anchors on the CHECKSUMS map's own closing brace; insert simulated against the real file before committing.
- **Stale-log-proof e2e**: the 500/500 balance assertion is occurrence-counted (before vs after the click) so serial specs sharing one `#log` can't pass vacuously.
- **Skipped (2)**: Bob-deploy timing skew (pre-existing live-network behavior, itemized in the steps breakdown) and the deep-path standards import (ledgered residual — upstream has no exports map).

## CI failure on PR #397 round 1 — self-inflicted cross-file interaction

`initSharedPage` (fullstack.helpers.ts:55) asserted `#token-flow-btn` **enabled** right after wallet init; the review fix in main.ts now gates that button on `state.sessionAddresses.length > 0`. Every local-network spec died in `beforeAll` (`unexpected value "disabled"`).

- **Why local gates missed it**: mocked e2e + production smoke don't exercise `initSharedPage`'s post-init assertions against a real wallet — only the local-network suite does, and it needs a sandbox. The 5.0.0-cycle lesson repeats: **the local-network CI job is the only automated behavioral gate; treat any change to main.ts button state or e2e helpers as untested until it runs.**
- **The irony worth keeping**: the codex audit prompt's ask #5 explicitly named "main.ts button gating vs aztec.ts state resets vs e2e helper assumptions about button state" as the cross-file class per-file reviewers miss. CI proved the class real before codex answered.
- **Fix**: init now expects the button disabled (with the why in a comment); `ensureSessionAccount` deploys on demand; `deployAndAssert`'s tail enabled-assertion stays (sessionAddresses is populated by then). Amended into the same code-review commit (`a6668a0`), lease-pushed with the explicit remote SHA (bare-URL pushes have no tracking ref — `--force-with-lease` needs `branch:sha` form; read the SHA from `git ls-remote`, never retype it by memory).

## Codex post-impl audit

Fresh-context xhigh session over both artifacts (`36994ec` net diff + `a6668a0` review commit) + plan/ledger, with correctness/adversarial/assumption/residual-risk asks.

- **Verdict**: FIX-BEFORE-MERGE (#397) — arrived after #397 auto-merged, so adopted items landed as a follow-up PR. No critical; 11 findings (2 high / 5 medium / 4 low). Session `019f6c18-5648-7992-bc2e-b4a987ca6edb`; transcript in `audit-codex.md`.
- **It caught the CI failure independently** (finding 1 == our round-1 `beforeAll` break) and confirmed the fix — the audit and CI converged on the same cross-file class its ask #5 named.
- **Best genuinely-new find (medium #5)**: `_aztec-update.yml` stages only `packages/*/package.json bun.lock` — the bot has been silently discarding the updater's CRS-version write to `aztec.ts` AND would discard the Windows checksum auto-pin, making the anchor repair dead in the bot path. Root cause of a whole class of "nightly bump built but Windows release failed at resolveWindowsBbChecksum" futures. Fixed: stage both derived files + `git diff --exit-code` guard after commit.
- **Also adopted**: sdk.yml scripts typecheck gate (`typecheck:scripts` — Bun test transpile accepts what tsc rejects, so `test:scripts` alone was not a type gate) + `tsconfig.scripts.json` path-filter entry + workflow-level `permissions: contents: read`; plan.md Facts labeled as pre-implementation snapshot.
- **Stood by (no new evidence)**: the rebuild-and-byte-compare demand (thrice-dispositioned reproducibility trade) — while withdrawing the over-claim it attacked ("only our code executes"); the negative-authorization e2e (P1 ledgered: the demo UI can't express Bob-initiated actions).
- **Re-dispositioned honestly**: Bob timing skew — "pre-existing" was provenance, not a correctness argument; now a named follow-up (deploy Bob outside the timer) instead of a skip.
- **Named follow-ups recorded in plan.md**: lockstep fail-open (404-vs-error conflation), dist-tag rollback = "upgrade" + direct-merge path in `_aztec-update.yml`, checksum single-control-plane + unbounded buffer, wallet-retry partial reset.
