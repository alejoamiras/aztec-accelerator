# Plan — SDK 5.2.0 npm release (publish → promote latest)

- **Tier**: `/blueprint light` (owner-requested; rubric: irreversibility HIGH — npm publish is
  append-only — every other dimension low/moderate; 5th publish cycle on proven machinery).
- **Success criterion** (owner, 2026-08-26): `@alejoamiras/aztec-accelerator@5.2.0` published on
  npm with provenance, `testnet` AND `latest` dist-tags pointing at it, playground live at
  playground.aztec-accelerator.dev serving the 5.2.0 bundle. Accelerator app release: OUT of scope.
- **Acceptance bar** (owner): trust the pipeline gate — promote follows publish verification
  immediately; no separate acceptance-smoke phase. (This waives the per-cycle manual smoke
  convention; `promote-latest.yml`'s header comment describes that convention, and this plan
  records the owner's explicit waiver for this cycle.)
- **Dispatch authority** (owner, verbatim answer "Run everything autonomously"): plan approval IS
  the standing authorization for THIS release's dispatches, exhaustively enumerated (codex R1:
  the failure protocol must never need an unauthorized action): (1) `publish-testnet.yml`;
  (2) `promote-latest.yml -f version=5.2.0`; (3) rollback `promote-latest.yml -f version=5.0.1`
  on failed verification; (4) repair-only `publish-testnet.yml -f skip_sdk_publish=true`
  (playground redeploy, npm-untouched) if the deploy leg alone fails. EVERYTHING else — manual
  tag/release creation, any `-revision.N` decision, token handling, accelerator release, npm
  deprecations — is STOP-and-surface to the owner, no exceptions.
- **Hardening**: owner declined a pre-release `/harden` (stays queued as the standing follow-up).
- **Codex audit** (`audit-codex.md`, session 01a03ebc-ee7e-7f60-8823-c65730b7114d): R1 conditional
  (1 blocking + 3 HIGH) → R2 conditional → R3 conditional → **R4 `approve`, no new or unmet
  findings**. Every condition adopted; dispositions inline above.
- **eli5_mode**: artifact — https://claude.ai/code/artifact/fb93437f-ec30-4f0c-a452-f503e04919b7
  (source: `implementations-plan/sdk-5-2-0-release/eli5.html`; republish the same path to update).
- **Worktree/branch**: `sdk-5-2-0-release` / `worktree-sdk-5-2-0-release`, base `8ab7df9`.

---

## Phase 1 ✓ — Docs-truth PR — GREEN 2026-08-26: PR #479 merged (squash) with rollup
`SUCCESS×20 SKIPPED×17`, zero failures; **Tarball Consumer pass (1m19s)** and SDK E2E pass at the
PR head. `bun run lint` exit 0 locally at every round. Codex loop: reject (3 blocking: shipped-README
temporal falsity, missing promote-failure triage, wrong-manifest local recipe) → reject → conditional
→ **approve**. Lessons: `lessons/phase-1.md`. CI-trigger gotcha recorded there: the push+`gh pr create`
raced and produced ZERO workflow runs; an empty commit was needed to trigger them.

The stale dist-tag story ships inside the tarball if uncorrected (`packages/sdk/README.md` is
packed by npm always; its line 18 still says "Aztec 5.0 line (`5.0.0-rc.x`), install `testnet`").
One small PR: four doc files (below) plus this plan directory — see Delivery. Close-out edits ride
a separate trailing PR.

1. `packages/sdk/README.md` — replace the rc-era install callout: bare install / `@latest` = the
   current stable Aztec line (5.2.0 after this release); `@testnet` = the newest published build
   (may equal `latest` between cycles). No rc.x references.
2. Root `README.md:67` — same correction, same wording (these two callouts must not drift apart
   again; keep them textually identical where they overlap).
3. `docs/RELEASE_RUNBOOK.md` — (a) fix the same stale callout (line ~93); (b) NEW subsection
   documenting `promote-latest.yml`: dispatch form, precondition (owner-defined acceptance bar per
   cycle), and the SDK rollback lever (re-dispatch with the prior version; honest limit quoted from
   the workflow header: a tag move repairs nothing already installed); (c) soften the "never
   override min-age" line to name the standing first-party `@aztec/*` `minimumReleaseAgeExcludes`
   (owner decision 2026-08-18) so the doc stops contradicting bunfig.
4. `implementations-plan/index.md` — add this plan's entry.

Deliberately OUT: fixing the `deploy-app` needs-vs-comment discrepancy and swap-sdk's inaccurate
"exact artifact" comment (both real, both harmless warts on other lines — ledgered below, not this
cycle); any workflow edit (an edited publish workflow's first real run would be its first test —
release-1.0.5 lesson — so this release deliberately dispatches UNEDITED workflows).

**Assumptions (phase)**: docs-only diff ⇒ `sdk.yml`'s changes-filter still triggers on
`packages/sdk/**`, so the PR re-runs the tarball-consumer job at this merge commit — a free,
current consumer-fidelity proof (recon fact 6). Verify it actually ran green before calling the
phase done; if the filter skips it (docs-only path exclusion), run the local equivalent in Phase 2
pre-flight instead and note it.

**Validation gate**
- `bun run lint` exit 0; PR CI green on all required checks; merged to main.
- Layers: lint · CI required checks.

## Phase 2 ⛔ BLOCKED (owner action required) — pre-flight all green, publish rejected by npm

Run [32993796578](https://github.com/alejoamiras/aztec-accelerator/actions/runs/32993796578),
dispatched 2026-08-26 on `main`, `headSha=acb3d317…` == the Phase-1 merge commit (assert passed).

- Pre-flight 1–5 ALL GREEN: registry had no `5.2.0` (`latest=5.0.1`, `testnet=5.0.1-revision.1`);
  tag `@alejoamiras/aztec-accelerator@5.2.0` absent; no queued/in-flight run in any of the three
  npm-mutating workflows; HEAD == merge commit; derived version = bare **5.2.0**; local
  tarball-consumer proof at `acb3d31` exit 0 ("packed tarball resolves + typechecks; exact-host
  @aztec graph is a singleton"), manifest restored clean.
- Jobs: `Assert main ref` ✓ · `e2e / SDK E2E` ✓ (native-bb leg) · `deploy-app` ✓ ·
  **`publish-sdk` ✗**.
- **npm rejected the publish**: `npm error 404 The requested resource
  '@alejoamiras/aztec-accelerator@5.2.0' could not be found or you do not have permission to
  access it.` npm packed the tarball first (contents logged), so the failure is at the registry
  PUT, not in our build.
- **Registry state re-read FIRST per protocol** (quoted in transcript): `5.2.0` ABSENT, dist-tags
  UNCHANGED (`latest=5.0.1`, `testnet=5.0.1-revision.1`). Nothing was published; no `-revision.N`
  risk; the tag/GitHub-release step never ran.
- Classification: version-absent + auth-shaped E404 = the documented 2026-05-27 incident shape
  (`implementations-plan/release-2026-05-27/lessons/phase-3b.md`) — a credential problem, NOT a
  code problem. Per protocol: STOPPED, zero retries, token untouched, surfaced to owner.
- **Playground DID deploy** (`deploy-app` has no `needs:` edge on `publish-sdk` — recon fact 1):
  live bundle serves `VITE_AZTEC_SDK_VERSION:"5.2.0"`. The 5.2 playground half of the release is
  DONE; only the npm publish is blocked.
- **Cause identified (high confidence): the token expired.** `gh secret list` (metadata only)
  shows `NPM_TOKEN` last updated 2026-05-27 — the previous rotation. This failure is day 91; the
  last successful publish (2026-08-18) was day 83, consistent with a 90-day granular-token
  lifetime. Nothing in the repo changed between those two dates.
- **Unblock**: owner mints a fresh granular, package-scoped npm token and updates the `NPM_TOKEN`
  secret, then re-dispatch — the derived version is still bare `5.2.0`
  because nothing was published.

### Original phase definition (unchanged — re-run from pre-flight on unblock)

Pre-flight (read-only, from the dispatch-intended main HEAD):
1. `npm view @alejoamiras/aztec-accelerator dist-tags versions --json` — assert `5.2.0` absent,
   tags as expected (`latest=5.0.1`, `testnet=5.0.1-revision.1`). If `5.2.0` already exists, STOP
   and surface (someone else published; the version script would mint `-revision.1`).
2. Tag-absence check (codex: the workflow's squat guard runs only AFTER npm publish):
   `git ls-remote origin 'refs/tags/@alejoamiras/aztec-accelerator@5.2.0'` must return EMPTY.
   A pre-existing tag = STOP/owner (append-only forbids the manual-repair path).
3. No in-flight/queued npm mutations: `gh run list` with explicit `--status queued` and
   `--status in_progress` for ALL THREE of `publish-testnet.yml`, `_publish-sdk.yml` (its direct
   dispatch shares the `publish-npm` group), and `promote-latest.yml`.
4. Confirm main HEAD SHA == the Phase-1 merge commit (nothing unexpected landed between) —
   established BEFORE step 5, since "same SHA" is what decides whether step 5 is needed at all.
5. Local tarball-consumer proof at that commit — ONLY if `sdk.yml`'s tarball-consumer job did not
   already run green at this same SHA (codex: don't duplicate it). Run it as a **script file**
   (not an `&&` chain — cleanup must survive a mid-way failure), mirroring CI's cwd exactly
   because the rewrite script's manifest argument defaults to a RELATIVE `package.json`:
   ```sh
   set -euo pipefail
   cd packages/sdk
   git diff --quiet HEAD -- package.json   # refuse to start dirty (staged changes included)
   trap 'git checkout HEAD -- package.json; rm -f "${TARBALL:-}"' EXIT
   bun run build
   bun ../../scripts/prepare-sdk-publish.ts 5.2.0
   TARBALL="$PWD/$(npm pack --silent)"
   bash ../../scripts/sdk-tarball-consumer.sh "$TARBALL"
   ```
   Pass = exit 0 (singleton stdlib graph + tsc + runtime import of the packed dist).

Dispatch + watch:
6. `gh workflow run publish-testnet.yml --ref main` → confirm the run actually started
   (`gh run list`, concurrency-group caveat) → assert the run's `headSha` == the approved merge
   SHA (`gh run view --json headSha`; codex: local-HEAD checks don't close the ref-resolution
   race) → `gh run watch <id>` to completion.
7. Verify — THIS step is the phase gate's "verify bullets" (the run being green is necessary but
   not sufficient):
   - `npm view @alejoamiras/aztec-accelerator dist-tags --json` → `testnet=5.2.0`, `latest` STILL
     `5.0.1` (untouched by design).
   - `npm view @alejoamiras/aztec-accelerator@5.2.0 version dist.integrity` resolves AND
     provenance attached: `npm view @alejoamiras/aztec-accelerator@5.2.0 --json` shows
     `dist.attestations` (url + provenance predicate).
   - Git tag `@alejoamiras/aztec-accelerator@5.2.0` exists at the dispatch SHA; GitHub release
     exists, is NOT marked Latest, carries MIGRATION.md.
   - Playground live (CloudFront invalidation is async — bounded retry, up to 10 min):
     `curl -fsSL https://playground.aztec-accelerator.dev/` → extract the absolute bundle URL →
     `curl -fsSL <asset>` → grep `5.2.0`.

**Failure protocol (embedded; codex R1 hardened)**: registry state FIRST, classification second —
on ANY failure of a publish or promote run, before any other action, run and QUOTE
`npm view @alejoamiras/aztec-accelerator versions dist-tags --json` (a "failed" npm command can
have succeeded server-side; a red promote may have moved the tag before its trailing query died).
Then: 5.2.0 absent + auth-shaped error (E401/E403/E404) → the 2026-05-27 `NPM_TOKEN` incident
shape; STOP, surface to owner for rotation, never blind-retry, never touch the token. 5.2.0
LIVE + a later leg failed → never redispatch the publish; the ONLY authorized repair is the
playground-only redeploy (authorization item 4) — a missing tag/GitHub-release or any
`-revision.N` decision is STOP/owner. Conflicting tag discovered at any point → STOP/owner.

**Assumptions (phase)**: `NPM_TOKEN` secret is valid (unverifiable from outside — the #1 historical
failure mode; mitigated by the failure protocol, not prevented). GitHub Actions + npm registry
healthy (probe ops, not status flags, if in doubt).

**Validation gate**
- Dispatch run green end-to-end (headSha == approved merge SHA) AND all four verify bullets in
  step 7 pass.
- Layers: CI e2e (sandbox, native-bb) · registry state · live-site check.

## Phase 3 ⛔ NOT STARTED — blocked upstream by Phase 2

`promote-latest.yml` refuses any version not already on npm, and `5.2.0` was never published, so
this phase is not merely unstarted but **structurally unreachable** until Phase 2 completes. No
promote was dispatched; `latest` remains `5.0.1` — its correct, untouched pre-release value.

**Terminal state of this release arc**: docs shipped (Phase 1 ✓), playground live on 5.2.0
(Phase 2's `deploy-app` leg), npm publish blocked on an owner-only credential repair, promotion
correctly not attempted. Resuming needs exactly one external action — see Phase 2's unblock note
and `lessons/phase-2.md` — after which Phase 2 re-runs from pre-flight (derived version is still
bare `5.2.0`) and Phase 3 proceeds as written below, unchanged.

### Original phase definition (unchanged — runs once Phase 2 clears)

1. `gh workflow run promote-latest.yml -f version=5.2.0` → confirm started → watch.
2. Verify: `npm view @alejoamiras/aztec-accelerator dist-tags --json` → `latest=5.2.0`,
   `testnet=5.2.0`. npm badge on the README will follow automatically.
3. Post-promote registry sanity (cheap, non-gating for the promote itself but gating for
   close-out): fresh scratch dir, `npm i @alejoamiras/aztec-accelerator` (bare ⇒ resolves the new
   `latest`; assert the installed version IS 5.2.0), then ESM import:
   `node -e 'import("@alejoamiras/aztec-accelerator").then(m => { if (typeof m.AcceleratorProver !== "function" || typeof m.ACCELERATOR_API_VERSION !== "number") process.exit(1); })'`.
   If this fails → IMMEDIATE rollback: `gh workflow run promote-latest.yml -f version=5.0.1`,
   WATCHED to completion, then `npm view … dist-tags` quoted showing `latest=5.0.1` restored,
   then surface. (Rollback moves the tag only; 5.2.0 stays published — append-only — and
   `^5.0.x`-range consumers still resolve it; rollback protects bare installers only.)
4. Close-out bookkeeping: mark phases ✓ here with outputs quoted; lessons file; index.md status →
   released; `agent-worktree status sdk-5-2-0-release "done: 5.2.0 live on latest"`; update the
   project memory (release state + any new lesson).

**Assumptions (phase)**: none beyond Phase 2's outcomes.

**Validation gate**
- Both dist-tags = 5.2.0 (command output quoted) AND the step-3 install/import exits 0.
- Layers: registry state · consumer install/runtime.

---

## Architecture & Implementation (compact — light tier)

- **Reuse**: everything (recon reuse map) — the plan adds ZERO pipeline code. The only shipped
  change is Phase 1's doc corrections; everything else is dispatch → verify → dispatch → verify,
  with the failure protocol as the only branching logic.
- **Touched files**: `packages/sdk/README.md`, `README.md`, `docs/RELEASE_RUNBOOK.md`,
  `implementations-plan/index.md`, plus this plan dir (Phase-1 PR) and the close-out edits to
  `plan.md`/`index.md`/`lessons/` (trailing PR).
- **Critical flow**: Phase-1 merge SHA becomes the dispatch SHA; `_publish-sdk.yml` derives
  `5.2.0` from the registry gap; testnet tag lands first; `latest` moves only via the second
  dispatch. `^5.0.x`-range consumers pick up 5.2.0 at PUBLISH time (ranges ignore dist-tags) —
  the real exposure event is Phase 2, which is why the tarball proof runs at its exact commit.
- **Simpler alternative considered**: skip Phase 1 and publish immediately (docs later). Rejected:
  the stale README ships INSIDE the 5.2.0 tarball forever (append-only registry); one small PR
  fixes it permanently and re-triggers the consumer-fidelity CI proof for free.
- **Alternative rejected**: adding an in-pipeline tarball-consumer step to `_publish-sdk.yml`
  (recon adapt candidate). Right idea, wrong moment — editing a publish workflow means its first
  real run tests the edit (release-1.0.5 lesson). Ledgered as a follow-up for a non-release PR.

## Security & Adversarial Considerations

- **Threat model**: the release moves public trust anchors (npm dist-tags). Attack surfaces:
  registry auth (static `NPM_TOKEN` — if exfiltrated, an attacker publishes as us; standing
  ledgered gap, trusted-publisher OIDC migration deliberately NOT this cycle); the dispatch ref —
  HONESTLY: `assert-main` lives only in `publish-testnet.yml`, while `_publish-sdk.yml`'s own
  `workflow_dispatch` remains directly invocable from ANY ref with the token in scope, skipping
  assert-main AND the e2e gate (codex HIGH; write-access compromise → malicious-branch publish
  with `prepublishOnly` executing under the token). Accepted residual THIS cycle (owner declined
  hardening; we never edit publish workflows in a release cycle) — closing it is ledgered below;
  tag squatting (pre-flight ls-remote absence check added; the in-workflow guard fires only
  post-publish, so a conflicting tag = STOP/owner, not self-repair); a compromised dependency
  entering the artifact (frozen lockfile in the publish job; note the 7-day min-age gates OUR
  lockfile-regen PRs, not the publish job's frozen install — protection happened at bump time).
- **Least privilege**: no new credentials; `id-token: write` scoped to the two jobs needing it
  (Sigstore provenance, AWS OIDC); I never handle `NPM_TOKEN` (failure = surface to owner).
- **Provenance**: `npm publish --provenance` attaches a Sigstore attestation binding artifact ↔
  this repo ↔ this run — consumers can verify the 5.2.0 tarball came from CI, not a laptop.
- **Supply chain (consumer side)**: publishing 5.2.0 exposes it instantly to `^5.0.x` rangers;
  the min-age gate protects OUR installs of others, not others' installs of us — their protection
  is the provenance + the append-only discipline.
- **Blast radius control — stated honestly (codex MED)**: `^5.0.x`-range consumers resolve 5.2.0
  the moment the PUBLISH lands, regardless of dist-tags, and neither the rollback lever nor a
  `-revision.N` (a semver prerelease their ranges won't match) can un-expose them — testnet-first
  and the rollback protect bare/`@latest` installers ONLY. The real range-consumer protection is
  everything that gated the content itself (PR gates, live-testnet proofs at bump time).
  `latest` move is tag-only and reversible in one dispatch; append-only everywhere.
- **Input validation**: both workflows validate their inputs (dist-tag allowlist; version regex +
  published-existence check).

## Assumptions

**Facts (verified in recon, sources in recon.md)**
1. npm live state: `latest=5.0.1`, `testnet=5.0.1-revision.1`, no `5.2.0*` published — recon
   agents 2+3 ran `npm view` independently, 2026-08-26.
2. The version script mints bare `5.2.0` for an unpublished base (`get-sdk-publish-version.ts` +
   its unit test's exact case).
3. `packages/sdk/package.json` pins every `@aztec/*` dep at exact `5.2.0` (read).
4. `publish-testnet.yml`'s e2e gate = local-sandbox native-bb sdk e2e (`_e2e.yml` defaults, read
   in full) — NOT live testnet; the live-testnet proof for this content ran 2026-08-24/25
   (aztec-5.2.0 Phase 4 + bun-1-4 Phase 4/5 gates, 10/10 sdk e2e, smoke 2/2).
5. `promote-latest.yml` is tag-only, verifies existence, and is the sanctioned rollback lever
   (file read in full).
6. `sdk.yml`'s tarball-consumer job runs on any `packages/sdk/**` PR and exercises the REAL
   rewritten manifest via plain npm on Node 24 (workflow + script read in full; whether it is
   branch-protection-REQUIRED is server-side config — codex nuance — the phase gate checks it ran
   green, which suffices).
7. The sdk README ships in the tarball (npm packs README.md unconditionally) and is stale about
   dist-tags (line 18 read).
8. Full-repo suite green on exactly this content: merged-main local full chain 2026-08-25 + #478
   CI green.

**Inferences (unverified — attack these)**
1. `NPM_TOKEN` is currently valid (unverifiable without publishing; last rotation ~2026-05-27
   post-incident; the failure protocol covers invalidity). Its SCOPE is not an inference — it is
   an open Ask below.
2. Nothing else lands on main between Phase-1 merge and Phase-2 dispatch (single-operator repo;
   pre-flight step 4 + the dispatched run's `headSha` assert close the race regardless).
   (The former inference "docs PR triggers sdk.yml" was verified by codex as a fact —
   `packages/sdk/**` matches unconditionally — and moved out of this bucket.)

**Asks — RESOLVED at the approval gate (2026-08-26, owner: "1 and 2: yes and ayes")**
- **`NPM_TOKEN` scope**: owner CONFIRMS it is a granular, package-scoped token. The failure
  protocol still covers invalidity; no rotation needed pre-Phase-2.
- **`_publish-sdk.yml` direct-dispatch bypass** (codex HIGH — skips `assert-main` + the e2e gate
  from any ref): owner ACCEPTS as a residual for this cycle. Never edit publish workflows
  mid-release; removal ledgered as a follow-up PR.

**Approval**: `approve` (owner, 2026-08-26) — both open asks answered affirmatively; standing
authorization for the four enumerated dispatches is live.

**Asks — resolved at clarify (2026-08-26)**
- Scope: publish + promote, accelerator app excluded — RESOLVED (owner).
- Acceptance bar: trust the pipeline; no separate smoke phase — RESOLVED (owner; the cheap step-3
  registry sanity in Phase 3 is retained as the rollback trigger, not a smoke phase).
- Hardening: declined for this cycle — RESOLVED (owner).
- Dispatch authority: standing authorization for these two workflows this cycle — RESOLVED
  (owner, "Run everything autonomously").

## Delivery

Single-arc: ONE PR (Phase 1) via `gh pr create` off `worktree-sdk-5-2-0-release`, carrying the
four doc edits PLUS this entire plan directory (codex: state where artifacts land); Phases
2–3 are dispatches with no PRs. Close-out bookkeeping (phase ✓ marks, lessons, index status) lands
as one tiny trailing docs PR at the end. No stack ceremony.

## Post-implementation

Owner's standing review override applies (/code-review max only on explicit request — memory
2026-08-25): after Phase 1's diff is ready, (1) inline self-review of the docs diff; (2) codex
xhigh pass over the diff + this plan with the adversarial/security + assumption-attack +
implementation-critique asks, PLUS verbatim: "Report bugs and small, targeted improvements only.
Do not propose speculative abstractions, extra configuration surface, new layers, or rewrites —
the smallest change that fixes each real problem. If code works and is clear, leave it alone." and
"Audit the comments for value per character. Flag any comment that narrates what the code visibly
does, restates its line, references implementation plans / phases / reviews, or spends a paragraph
where a sentence works — and flag places where a non-obvious invariant or constraint deserves a
comment it doesn't have."; (3) iterate until a resumed round yields no new material findings (3+
churning rounds → stop, surface); (4) ONLY THEN `gh pr create`, watch checks, merge on green
(merge authorized: it is a precondition of the authorized dispatches); (5) proceed to Phase 2.

## Ledger — deferred follow-ups (not this cycle)

- **Remove `_publish-sdk.yml`'s direct `workflow_dispatch`** (codex HIGH: it bypasses assert-main
  + the e2e gate from any ref with the token in scope) — normal PR, NOT during a release.
- In-pipeline tarball-consumer step in `_publish-sdk.yml` (+ making swap-sdk's pack post-rewrite
  so its comment becomes true) — do in a normal PR, NOT a release.
- `deploy-app` needs-edge vs. comment discrepancy in `publish-testnet.yml`.
- npm trusted-publisher OIDC migration (kills the static `NPM_TOKEN`) — standing security item.
- `/harden security` — still queued (owner).
- `@types/bun ^1.4.0` trailing commit after min-age (~2026-08-28) — unrelated to this release.

## Seeds

(Recommended: /goal — completion is transcript-observable.)

/goal All three phases marked ✓ in implementations-plan/sdk-5-2-0-release/plan.md with each phase's validation-gate outputs quoted in the transcript (Phase 1: PR merged + CI green; Phase 2: publish run green with its headSha quoted matching the approved merge SHA + all four verify bullets incl. `npm view` showing testnet=5.2.0 with latest still 5.0.1, `dist.attestations` present, and the playground bundle serving 5.2.0; Phase 3: `npm view` showing latest=5.2.0 AND the scratch bare `npm i` resolving 5.2.0 with the ESM import exit 0), `LESSONS_FILE=implementations-plan/sdk-5-2-0-release/lessons/phase-N.md` printed per phase, the codex loop on the Phase-1 diff converged (resumed pass quoting no new material findings), and `bun run lint` exit 0; OR the blocked path: a failure surfaced with its protocol followed — on ANY failed publish/promote run, `npm view @alejoamiras/aztec-accelerator versions dist-tags --json` output quoted BEFORE any further action; token-shaped failure → owner surfaced, zero retries; promote verify failed → rollback dispatched, watched, and `latest=5.0.1` re-verification quoted — and the state honestly recorded in plan.md.

/loop 15m Drive implementations-plan/sdk-5-2-0-release forward per plan.md. Never idle. Each firing: read plan.md + lessons (authoritative), `git status`, check any in-flight `gh run` (watch ≤10 min, stuck → log + reassess). Phase gates are as written in plan.md — quote outputs, mark ✓, print LESSONS_FILE. Hard limits — the authorization is EXACTLY four dispatches and nothing else: (1) publish-testnet.yml; (2) promote-latest.yml -f version=5.2.0; (3) rollback promote-latest.yml -f version=5.0.1; (4) publish-testnet.yml -f skip_sdk_publish=true for a playground-only repair. Manual tag/release creation, any -revision.N decision, token handling, accelerator release → STOP and surface. On ANY failed publish or promote run, quote `npm view @alejoamiras/aztec-accelerator versions dist-tags --json` BEFORE any other action (a failed command may have succeeded server-side); classify only after that quote. Token/auth-shaped failures → stop and surface, never retry, never touch NPM_TOKEN; no scope beyond plan.md. Stuck or 5 same-step failures → codex xhigh, log the consult, act within scope.
