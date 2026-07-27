Bottom line: reject. The plan improves workflow semantics, but it does not yet make independent provenance machine-enforced, and its CI-gating assumption is false for `nightlies` and unverified for branch protection.

## 1. Does the three-part fix close auto-accept?

Only conditionally for new versions targeting `main`.

Two important corrections:

- The current automated updater writes `copy-bb.ts`, but the workflow commits only `packages/*/package.json` and `bun.lock` ([`_aztec-update.yml:139-141`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:139)). Therefore, the auto-generated pin from [`update-aztec-version.ts:91-94`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/scripts/update-aztec-version.ts:91) is not actually included in this workflow’s PR. The plan’s compound “auto-pin + auto-merge” narrative at [`C7-plan.md:4-10`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:4) is thus misstated. The auto-pin remains dangerous for local/manual use or any future broader `git add`, but this workflow currently creates an unpinned PR.

- The current checksum map contains no `(auto-pinned)` entries ([`copy-bb.ts:56-70`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:56)). The entries are still circularly sourced from release downloads, but the plan’s “mixes human-verified and auto-pinned entries” fact is false.

For a new `main` bump, a missing pin does fail both Windows jobs: `bun.lock` and `packages/sdk/package.json` select the desktop filter ([`accelerator.yml:32-39`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/accelerator.yml:32)), the Windows prebuild invokes the real fetch path ([`accelerator.yml:357-375`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/accelerator.yml:357)), and `resolveWindowsBbChecksum` throws on absence ([`copy-bb.ts:76-85`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:76)).

Residual auto-accept paths remain:

- A legacy/unverified entry remains fully trusted. “Flagging” it in a comment changes nothing because the resolver reads only the hash. This directly contradicts “block the version if no independent evidence” in [`security-hardening/plan.md:51`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/plan.md:51).
- The nightly PR targets `nightlies` ([`aztec-nightlies.yml:26-30`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/aztec-nightlies.yml:26)), but Accelerator CI listens only to `main` and `security-hardening` ([`accelerator.yml:5-6`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/accelerator.yml:5)). SDK and App have the same branch restriction. The Windows gate is completely absent there.
- Whether `Accelerator Status` is required, and whether the release-bot App can bypass the ruleset, is external configuration not proven by this repository.

## 2. Merge semantics and `--auto`

Yes: `auto_merge: false` should mean “create/update the PR and stop.” Both documented callers explicitly select false ([`aztec-stable.yml:25-30`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/aztec-stable.yml:25), [`aztec-nightlies.yml:25-30`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/aztec-nightlies.yml:25)). Better still, replace the ambiguous boolean with `merge_mode: none|auto` and default to `none`; the current reusable default is `true` ([`_aztec-update.yml:23-27`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:23)).

For `auto_merge: true`:

- On `main`, a missing pin blocks only if `Accelerator Status` is a required check and the bot cannot bypass it. The aggregate does correctly propagate Windows failures ([`accelerator.yml:488-516`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/accelerator.yml:488)).
- GitHub auto-merge waits for required checks, not every advisory check. That required-check configuration must be audited explicitly; code comments claiming it is required are not evidence. [GitHub’s semantics](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request) confirm this.
- On `nightlies`, no Windows check is emitted, so an auto-enabled or manually merged PR can bypass the pin gate entirely.
- GitHub Apps can be placed on ruleset bypass lists. Verify the release-bot App is absent from all bypass lists; do not infer this from requested token permissions.

## 3. Is advisory-only sufficient?

It is correct that the updater must never write the hash. No other bump-time path needs the map.

The map is read only when the Windows copy path starts: `fetchWindowsBb` resolves it before downloading ([`copy-bb.ts:100-103`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:100)), and that path runs only on Windows ([`copy-bb.ts:182-184`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:182)). A human must eventually commit a trusted hash before Windows CI/builds can pass.

But stdout advisory alone is not sufficient enforcement:

- It will be buried in workflow logs and is not included in the PR body.
- “Pin present; verify provenance” at [`C7-plan.md:49-53`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:49) accepts legacy/unverified pins without distinguishing them.
- Provenance represented only by comments cannot be validated.
- The existing unit test called “shipped version” is hard-coded to `4.2.0`, not the live dependency ([`copy-bb.test.ts:20-23`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.test.ts:20)).

Use a structured manifest with an explicit `verified` provenance type and make the resolver reject `legacy`, `unknown`, or missing evidence. Surface missing provenance in the PR body and as a dedicated required check.

## 4. Revalidating the live pin

The plan’s “annotate or flag” approach is not actionable. If `5.0.0-rc.2` is flagged but remains in `WINDOWS_BB_CHECKSUMS`, it still passes unchanged at [`copy-bb.ts:69`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:69). It must either gain verifiable evidence or become non-usable.

Current evidence appears insufficient:

- The repository itself says upstream publishes no checksum file ([`copy-bb.ts:7-11`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:7)).
- Aztec’s public [attestations page](https://github.com/AztecProtocol/aztec-packages/attestations) reports no attestations.
- The [rc.2 release page](https://github.com/AztecProtocol/aztec-packages/releases/tag/v5.0.0-rc.2) shows a GitHub-verified signature on the tag commit. That signature does not bind the Windows tarball.
- Re-downloading the asset in CI is not independent evidence. Neither is GitHub’s release-asset digest if the threat includes a compromised upstream publisher; it is useful against transit corruption or later mutation, not a malicious release at publication.
- The prior “reproducible” claim at [`phase-2.md:5-6`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-2.md:5) does not document an independent rebuild, toolchain, source commit, or deterministic comparison.

Actionable evidence would be one of:

1. An upstream build attestation verified with `gh attestation verify`, additionally pinning the expected repository, signer workflow, source tag/ref and source digest. [GitHub documents this verification model](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations).

2. A signed upstream checksum manifest whose signing-key fingerprint was obtained out of band.

3. A genuinely reproducible build from the signed source commit by at least two independent builders. Because gzip metadata may make tarballs nondeterministic, this likely requires pinning the extracted `bb.exe` hash as well as validating the archive layout.

Until one exists, the honest outcome is: block that Windows bb version, not “flag but continue.”

## 5. Facts, inferences and asks

Misstated Facts:

- The workflow does not commit the updater’s auto-pin.
- The current map contains no `(auto-pinned)` entries.
- “Aztec does not sign releases” is too broad: the tag commit is GitHub-signed, but the artifact is not signed/attested.
- “F-008 is Windows-specific” is true only for this direct GitHub-sidecar path, not for the broader circular-provenance risk.

Unsafe Inferences:

- “Missing pin fails the Windows CI gate” is true for a normal `main` PR, false for `nightlies`.
- “CI failure blocks `--auto`” requires proof that `Accelerator Status` is required and that the bot has no bypass.
- “Human review supplies independence” is false unless the evidence policy is defined and enforced.
- “Flagged provenance” is safe while the entry remains active is false.

Asks that must be explicit:

- Must `nightlies` run the Windows gate, or are Windows nightlies prohibited?
- Is `Accelerator Status` required on every relevant target branch?
- Is the release-bot App on any ruleset/branch-protection bypass list?
- Does lack of independent evidence block the entire Aztec bump or only Windows release support?
- Are all current entries disabled pending evidence?
- Is npm/macOS/Linux provenance in scope?
- Who is authorized to approve/sign provenance, and is two-person review required?
- Should `auto_merge: true` continue to exist at all?

## 6. Adversarial targets

- Upstream release credentials, release workflow and mutable assets.
- The release-bot App private key and its ruleset bypass status.
- Branch protection configuration, especially omission of `Accelerator Status`.
- The `nightlies` target branch, where Windows CI does not run.
- The provenance reviewer: social-engineer them into treating a second download or GitHub asset digest as “independent.”
- npm publishing credentials. macOS/Linux directly copy `bb` from installed `@aztec/bb.js` ([`copy-bb.ts:184-193`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:184)); the lockfile’s SHA-512 is auto-generated during the bump ([`bun.lock:191`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/bun.lock:191)). That protects later installs against mutation but is not independent provenance for malicious bytes initially published to npm.

Least privilege is under-addressed. The App token requests contents, PR and issues write ([`_aztec-update.yml:154-165`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:154)). After removing merge, the PR-creation job needs contents write mainly because stale cleanup deletes branches ([`_aztec-update.yml:169-185`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:169)). Split push, PR creation and stale-branch cleanup into separately scoped tokens/jobs, or stop deleting branches automatically.

Version-to-URL/source injection is mostly constrained: the strict regex excludes slashes, quotes, whitespace and shell metacharacters ([`update-aztec-version.ts:8-9`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/scripts/update-aztec-version.ts:8)), and validation precedes fetch/write ([`update-aztec-version.ts:109-114`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/scripts/update-aztec-version.ts:109)). One hardening gap remains: forced input is written into `$GITHUB_OUTPUT` before that validation ([`_aztec-update.yml:64-70`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:64)). Validate it in `check-update` before emitting outputs.

## 7. Competing outline

Use an attestation-gated manifest rather than a source-code hash table:

1. Store version, archive hash, extracted-executable hash, source commit, signer workflow and attestation identity in a structured manifest, preferably in a separately protected repository or as an offline-signed manifest.
2. A provenance workflow with no merge permission verifies the upstream artifact using `gh attestation verify` with pinned repository/workflow/ref policy.
3. Only that workflow or an offline two-person signer may add a `verified` entry.
4. `copy-bb.ts` verifies the manifest signature, provenance status, archive hash and extracted executable hash.
5. A platform-neutral required check validates manifest coverage for the live `@aztec/bb.js` version on every target branch, including `nightlies`.
6. The dependency-bump workflow has no merge capability and never generates either hashes or provenance.
7. Until Aztec publishes a qualifying attestation/checksum, block Windows bumps entirely.

This beats the proposed plan when bumps are recurring, multiple branches exist, or reviewers cannot reliably distinguish real provenance from circular checks. An attestation-only design is simpler still once Aztec publishes build attestations; today, the public evidence does not support it.

VERDICT: reject (legacy/unverified pins remain trusted; nightlies skip the Windows gate; required-check/bypass assumptions are unverified; no actionable independent-evidence enforcement)
---

## Final fresh-context pass on the REVISED plan (VERDICT: reject — operational blockers, design direction CONFIRMED)

Scope note: the requested C7 files were absent from the supplied C6 worktree, so I audited the sibling `sechard-bb-windows-provenance` worktree where they exist.

## Blocking findings

1. **The reporter runs before the dependency it resolves is installed.** The plan wires `reportWindowsBbPinStatus()` into the updater using `resolveAztecBb()` ([C7-plan.md:66](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:66)), but the workflow runs the updater before `bun install` ([\_aztec-update.yml:116](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:116), [\_aztec-update.yml:119](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:119)). `resolveAztecBb()` requires the installed `@aztec/bb-prover → @aztec/bb.js` tree ([copy-bb.ts:160](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/packages/accelerator/scripts/copy-bb.ts:160)). A clean runner will fail resolution; a local run with existing dependencies reports the old bb.js version. Move reporting to a post-`bun install` step or separate post-install command. Also migrate the map before the reporter: Phase 1 currently consumes the structured schema that Phase 2 creates ([C7-plan.md:83](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:83), [C7-plan.md:91](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:91)).

2. **The required-check fold is still not live-verified.** C7 calls the checked-in JSON “VERIFIED” ([C7-plan.md:44](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:44)), but the master plan explicitly says that file is desired state requiring human application and readback ([plan.md:48](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/plan.md:48)); the C5 runbook still contains that apply operation ([C5-runbook.md:32](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C5-runbook.md:32)). Treating the required check as proven while treating bypass actors as external is inconsistent: both reside in the same live ruleset. Require a live readback proving active enforcement, main targeting, strict status checks, `Accelerator Status`, and no bypass actors.

3. **Least-privilege remains an unresolved alternative, not an operational design.** The ledger still says “scope … / stop … or document” ([C7-plan.md:50](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:50)). Currently the App token has `contents:write` and stale cleanup deletes branches ([\_aztec-update.yml:154](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:154), [\_aztec-update.yml:169](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/_aztec-update.yml:169)). `contents:write` is also the permission used to merge PRs, so “none path has no merge capability” requires a concrete split. The sound design is: job-scope the default token; mint the `none` App token with PR/issues permissions but no contents-write; close stale PRs without deleting branches; mint a separate conditional contents-write token only for `auto`. Also remove caller-level `pull-requests:write` at [aztec-stable.yml:15](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/aztec-stable.yml:15). This matches GitHub’s documented [merge permission](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request) and the App-token action’s [default permission inheritance](https://github.com/actions/create-github-app-token#permission-permission-name).

## Answers

1. **Auto-accept:** after deleting nightlies, `stable → main` is the only repository caller, and `merge_mode:none` plus a missing live-version pin closes automatic acceptance. Package and lockfile changes select desktop CI ([accelerator.yml:32](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/accelerator.yml:32)); Windows invokes the real resolver ([accelerator.yml:357](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/accelerator.yml:357)); the aggregate includes both Windows jobs ([accelerator.yml:488](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/.github/workflows/accelerator.yml:488)). No other `_aztec-update.yml` caller exists. Residuals are manual/repository-authority merge and the unverified live ruleset, not another automated caller.

2. **Provenance:** usable `manual-review` is the correct availability choice under the explicitly downgraded change-detector threat model ([C7-plan.md:113](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:113), [C7-plan.md:149](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:149)). It knowingly retains circular initial trust but no longer disguises it as independent proof. During migration, current entries must either undergo the stated review or have notes explicitly saying they are legacy pins adopted only as change detectors; mass relabeling alone would invent provenance. Before first use of `attestation`, add an actual verification procedure and fields/record binding signer workflow, source ref and digest; a recognized string alone is not attestation verification. The sample command at [C7-plan.md:116](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/clusters/C7-plan.md:116) is also incomplete: `--repo` requires `AztecProtocol/aztec-packages`, is an alternative to `--owner`, and signer/source constraints need explicit flags.

3. **Workflow changes:** `merge_mode:none`, caller update, forced-input validation before output, and deleting nightlies are directionally sound. The unresolved permission/stale-cleanup design above prevents calling the fold complete. Add behavioral/static tests for invalid forced input and “none contains no merge invocation”; `actionlint` alone tests syntax, not these semantics.

4. **Version key:** using `resolveAztecBb().version` is correct, but only after installation. A remaining divergence exists in `updateCrsCacheVersion`: its comment says the cache tracks bb.js, while it is passed the argv/aztec.js version ([update-aztec-version.ts:70](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/scripts/update-aztec-version.ts:70), [update-aztec-version.ts:134](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/scripts/update-aztec-version.ts:134)). That is correctness debt, not a Windows-pin bypass.

5. **Assumptions:** the change-detector residual is honest, and the npm framing is bounded correctly: minimum age affects resolution, while frozen installs consume the lock ([bunfig.toml:7](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/bunfig.toml:7)). The misstated Fact is live ruleset verification. The manual-review availability override is explicit rather than silent, though the conflicting master-plan “block” requirement at [plan.md:51](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/implementations-plan/security-hardening/plan.md:51) should be reconciled.

6. **Second-order risks:** reorder schema migration/reporting; test malformed SHA and empty notes as well as provenance; test the reserved attestation branch before enabling it. Deleting nightlies leaves stale documentation at [CLAUDE.md:13](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-windows-provenance/CLAUDE.md:13). No additional resolver consumers were found beyond `fetchWindowsBb`, the tests, and the proposed reporter.

VERDICT: reject (reporter runs before bb.js is installed; live ruleset enforcement is not actually verified; none-path merge capability/stale cleanup are not concretely scoped)