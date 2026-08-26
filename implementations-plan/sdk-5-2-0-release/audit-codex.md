conditional approve (with conditions: fix the tarball recipe; make all npm-mutation failures registry-first; verify dispatch SHA, provenance, and rollback; reconcile repair authority)

## 1. Adversarial/security

- **[HIGH] Main-ref protection is bypassable.** `_publish-sdk.yml` remains directly dispatchable from another ref, skipping `assert-main` and E2E while receiving `NPM_TOKEN` ([workflow](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/sdk-5-2-0-release/.github/workflows/_publish-sdk.yml:7)). A write-capable compromised account could dispatch malicious branch content; `prepublishOnly` executes while the token is present. Correct the Security section’s claim that dispatch-ref risk is mitigated. Given the fixed no-hardening decision, record this as an explicitly accepted residual.

- **[HIGH] Tag squatting is detected too late, not mitigated.** npm publishes at line 118; the remote-tag guard runs afterward ([workflow](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/sdk-5-2-0-release/.github/workflows/_publish-sdk.yml:110)). A conflicting tag strands a live 5.2.0 without its canonical tag/release. The failure protocol’s “create tag/release manually” is impossible under the append-only rule when the tag points elsewhere. Add a preflight absence check, and classify any conflicting-tag result as STOP/owner escalation.

- **[HIGH] Failure classification is unsafe.** A publish command can fail after npm accepted the upload, including through a lost response. Therefore **every** publish-step failure—including E401/E403/E404—must first query versions and dist-tags before declaring “token failure.” Likewise, a red promotion may have moved `latest` before its trailing query failed; query tags before retrying or rolling back.

- **[MED] Blast-radius claims are overstated.** `testnet` first does not protect semver-range consumers; they resolve stable 5.2.0 immediately. Rolling back `latest` does not protect them, and `5.2.0-revision.N` cannot repair their range. Provenance authenticates origin only when consumers verify it; it does not prevent an off-workflow static-token publish. Frozen installs also do not apply the age gate at publish time, and `@aztec/*` is excluded.

## 2. Assumption attack

**Facts**

- **[LOW]** Fact 6 overstates “required-check”: the repository proves the job runs for `packages/sdk/**` PRs, but branch-protection configuration—not the YAML—determines whether it is required.
- **[LOW]** Public npm currently reports `latest` as 5.0.1; exact `testnet` and absence of 5.2.0 still need the planned live preflight. [npm package](https://www.npmjs.com/package/%40alejoamiras/aztec-accelerator)
- The remaining repository Facts check out.

**Inferences**

- **[MED]** Checking local/main HEAD before dispatch does not eliminate the ref-resolution race. Verify the started run’s `headSha` equals the approved merge SHA.
- **[LOW]** The README-trigger inference is actually a fact: `packages/sdk/**` unconditionally matches the file and enables the whole SDK workflow ([sdk.yml](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/sdk-5-2-0-release/.github/workflows/sdk.yml:28)).

**Asks**

- **[HIGH]** Repair authority is unresolved. The plan authorizes two dispatches plus rollback, while its failure protocol proposes manual tag/release creation and a third playground-only dispatch; the `/loop` explicitly forbids those. Make them STOP-and-request-owner actions or extend authorization explicitly.
- **[MED]** Owner confirmation should exist that the static token is granular, package-scoped, minimally privileged, and expected valid—not merely that some `NPM_TOKEN` secret exists.

## 3. Implementation critique

- **[LOW]** The three-phase ordering is sound under the fixed acceptance decision.
- **[LOW]** Phase-1 scope is correct: the shipped MIGRATION and Claude skill contain no additional release-version correction. Existing shipped source/tests are unrelated.
- **[LOW]** The local tarball run duplicates the Phase-1 job when the dispatch SHA is identical. Make it conditional on that CI job being skipped or the SHA changing.
- **[LOW]** “One four-file PR” conflicts with committing the currently untracked plan/recon and later modifying plan/index/lessons. State where close-out changes land.

## 4. Load-bearing mechanics

- **[BLOCKING]** The local recipe rewrites the wrong manifest: from repository root, `prepare-sdk-publish.ts` defaults to root `package.json`, while cleanup restores the SDK manifest ([script](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/sdk-5-2-0-release/scripts/prepare-sdk-publish.ts:44)). Run it from `packages/sdk` exactly like CI, capture an absolute `npm pack --silent` path, and install cleanup via a trap after asserting the manifest was initially clean.

- **[MED]** Preflight must also inspect direct `_publish-sdk.yml` runs and use explicit queued/in-progress filters. After dispatch, verify `headSha`.

- **[MED]** The playground command lacks `curl -f`, does not construct an absolute asset URL, and ignores asynchronous CloudFront invalidation. Use a bounded retry with `curl -fsSL`.

- **[MED]** Success requires provenance, but neither verification nor `/goal` checks `dist.attestations`. Add it. Specify the ESM import command, and require rollback to be watched and `latest=5.0.1` verified.

## Things that look fine

- Version derivation, manifest rewrite, publish tag, promotion workflow, and GitHub-release checks match the files.
- Exact 5.2.0 Aztec pins and playground build-time version injection are correct.
- The docs-before-publish rationale, accelerator exclusion, and append-only discipline are sound.

---

## Round 2 (resumed pass on rev 2)

conditional approve (with conditions: make cleanup executable, align the /loop seed, and keep token scope unresolved until confirmed)

- **[HIGH — unmet]** The `/loop` seed still contradicts both hardened rules: it omits the authorized playground-only repair dispatch and requires registry-first inspection only after “fail-past-publish,” not after **any** publish/promote failure. Mirror the exhaustive authorization and `versions dist-tags --json BEFORE any action` rule verbatim.

- **[MED — unmet]** The tarball recipe promises restore-on-failure, but the displayed `&&` chain installs no `EXIT` trap; a build/rewrite/pack/consumer failure can still leave `package.json` rewritten. Explicitly install the cleanup trap before mutation. Also move the approved-main-SHA confirmation before this proof so “same SHA” is established before deciding whether to skip/run it.

- **[MED — unmet]** Token scope is still an assumed Inference while “Asks — all resolved” remains. Move granular/package-scoped token confirmation into Asks as unresolved unless the owner actually confirms it.

- **[LOW — new consistency nits]** Phase 2’s gate says verification is step 6, but it is step 7. Phase 1/Architecture still say “four files”/“only diff,” while Delivery correctly includes the plan directory and a trailing docs PR. Update those descriptions only.

---

## Round 3 (resumed pass on rev 3)

conditional approve (with conditions: make the manifest-clean check include staged changes)

- **[MED — still unmet]** `git diff --quiet -- package.json` checks only unstaged changes. A staged manifest change passes, and cleanup restores that staged version. Use `git diff --quiet HEAD -- package.json` and restore from `HEAD`.

- **[LOW — new consistency nits]** The Inference still references preflight step 5 for the SHA check; it is now step 4. Phase 1/Delivery omit the newly synced `eli5.html` from their explicit plan-directory file lists. Add it or make both lists simply say “this plan directory.”

The two OPEN owner asks remain intentional approval gates, not plan defects.

---

## Round 4 (resumed pass on rev 4)

approve

No new or unmet findings. Rev 4 converges; the two OPEN owner asks remain explicit approval-time decisions as intended.