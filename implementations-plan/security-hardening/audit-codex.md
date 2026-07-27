# Codex master-plan consult — gpt-5.6-sol @ xhigh

Consult prompt + verdict for the security-hardening campaign (D1–D8).

---

## Verdict

Change the strawman in five ways:

- Split F-012 from F-014.
- Split F-007 from F-008.
- Split F-006 from the broad F-015 pin churn.
- Add a CI-bootstrap cluster first.
- Make F-002 last and hard-dependent on F-001’s shared identity interface.

Do not move F-011 beside F-002. It is origin canonicalization, not installation identity.

Cut every branch from the newly updated `security-hardening` after the preceding cluster merges. No parallel stacked branches where workflows, updater release code, or frontend files overlap.

## Finalized order

| # | Branch | Findings / tier | Local gate on VPS | Required CI gate | Primary risk |
|---|---|---|---|---|---|
| C0 | `sechard/ci-integration-gates` | Bootstrap only — **light** | actionlint/YAML validation | Manually dispatch all four gate workflows on this ref | Bootstrap PR may not auto-trigger; do not rely on it |
| C1 | `sechard/workflow-input-hardening` | F-006 — **light** | actionlint plus adversarial tag-validator tests | `actionlint.yml` | Validation must execute before any token-bearing step |
| C2 | `sechard/core-request-safety` | F-003, F-009, F-011 — **mid** | core fmt/clippy/tests; concurrency/backpressure tests | `accelerator.yml` | Slow bodies, permission-at-creation, persisted dotted origins |
| C3 | `sechard/action-pinning` | F-015 — **mid** | actionlint plus “all remote uses are SHAs” checker | All four gates; `actionlint.yml` | Huge mechanical diff and unpinned downloaded tools |
| C4 | `sechard/updater-rollback` | F-004 — **deep** | Rust/feed unit tests; fixture-key rollback tests | `accelerator.yml`, `actionlint.yml`, new PR-safe Linux rollback smoke | Manifest canonicalization, floor corruption, signing-key scope |
| C5 | `sechard/infra-deploy-authz` | F-005 — **deep** | `tofu fmt/init -backend=false/validate`; IAM/ruleset semantic tests | `actionlint.yml` extended to validate ruleset JSON and IAM invariants | Staged role migration, landing sync deletion, ruleset not yet live |
| C6 | `sechard/bb-cache-integrity` | F-007 — **mid** | Bun fixture tests, Rust cache tests, safe-extraction tests | `accelerator.yml`; `actionlint.yml` if filters change | Digest marker must form a real verification chain |
| C7 | `sechard/bb-windows-provenance` | F-008 — **mid** | Manifest/provenance validator tests | Windows prebuild/build in `accelerator.yml`; `actionlint.yml` | No independent provenance means Windows release must fail closed |
| C8 | `sechard/desktop-platform-secrets` | F-010, F-016 — **mid** | systemd serialization tests; cert-generation tests | `accelerator.yml` plus targeted macOS cert test | systemd byte escaping; rcgen zeroization is incomplete |
| C9 | `sechard/authorize-popup-safety` | F-014 — **light** | helper tests and headless mocked Playwright | Accelerator desktop UI + WebDriver jobs | IDN/punycode display and hiding security-relevant subdomains |
| C10 | `sechard/tauri-trust-boundary` | F-012 — **deep** | frontend build, CSP lint, Rust command-policy tests, mocked UI | Accelerator WebDriver on Linux/macOS/Windows with negative IPC tests | Highest regression risk: IPC, popup and updater prompt |
| C11 | `sechard/incumbent-identity` | F-002 — **deep** | fake-identity, forged/replay challenge tests | Targeted Windows dual-instance/port-squatter integration | Must consume F-001 identity; no public-health fallback |

`app.yml` is not the desktop gate: it covers playground/SDK. F-012 and F-014 belong under [accelerator.yml](.github/workflows/accelerator.yml:5), not [app.yml](.github/workflows/app.yml:5).

## D3 — F-002 ruling

Defer implementation and land it last. Do not build a second per-install secret system, and do not drop F-002.

Require F-001’s team to expose a storage-independent, versioned identity contract:

```text
InstallationIdentity
  expected_identity() -> trusted local identity/key id
  answer_challenge(nonce, context) -> authenticated response
  verify_challenge(nonce, context, response) -> verified/rejected
```

Wire protocol requirements:

- Fresh 32-byte nonce.
- Domain-separated context such as `aztec-accelerator/incumbent/v1`.
- Bind the response to nonce, API version and port.
- Verify against identity loaded from the trusted local provider—never a key supplied only by the response.
- Legacy/missing/malformed/replayed response means “foreign process”: remain resident and surface port-in-use.
- Only a verified incumbent permits Windows `exit(0)`.

The provider may use the F-001 team’s certificate key, asymmetric install key, or HMAC token. F-002 must not read their files or assume their crypto directly.

Standalone implementation risks split-brain identity, incompatible rotation/migration and duplicate sensitive storage. Deferral leaves F-002 exposed temporarily, but that is better than institutionalizing a second identity system. If F-001 is not ready, the other clusters may land, but the campaign cannot claim F-002 closed.

## D4 — F-005 IaC boundary

Your authority boundary is correct:

- Commit tofu, workflows, rulesets, policy tests and an apply/read-back runbook.
- Run `fmt`, `init -backend=false`, `validate` and deterministic policy tests locally and in CI.
- Do not put production credentials into PR CI.
- A trusted human runs `tofu plan`, reviews it, then applies.
- Claude does not apply.

Editing [main-branch-protection.json](infra/rulesets/main-branch-protection.json:1) is not operationally effective. A human with administration permissions must create/update the ruleset through the GitHub API or UI, then export/read back the live configuration. GitHub documents rulesets as repository state managed through those APIs, not automatically loaded from a checked-in JSON file: [ruleset management](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/managing-rulesets-for-a-repository), [REST rules endpoints](https://docs.github.com/en/rest/repos/rules).

Implement four roles, not one:

- Landing: exact `main` subject; `landing/*`, with explicit deny for `landing/releases/*`.
- Release feed: exact `main`; write only `landing/releases/latest.json`.
- Testnet playground: exact `main`; `playground/*`.
- Nightly playground: exact `nightlies`; `playground-nightly/*`.

Use prefix-conditioned `ListBucket` plus object-level permissions. CloudFront invalidation cannot be meaningfully prefix-scoped within one distribution; accept that residual availability risk or split distributions later. AWS explicitly recommends exact GitHub OIDC subject constraints: [AWS OIDC role guidance](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_create_for-idp_oidc.html).

Protect `main` and `nightlies`, require one review, and require all four status aggregators including `Actionlint Status`. Remove `chore/aztec-*` from OIDC trust entirely; do not protect those ephemeral source branches just to compensate for a wildcard that should not exist.

F-005 becomes:

- “Source/CI complete” after cluster CI is green.
- “Operationally remediated” only after human apply, secret cutover and live read-back.
- Not fully closed until that second condition is recorded.

Use a two-stage rollout: additive roles first; configure new secrets; merge workflow cutover to main; smoke each deployment; then remove the legacy broad role.

## D5 — CI mechanics

Yes: C0 changes each gate to:

```yaml
pull_request:
  branches: [main, security-hardening]
```

`pull_request.branches` matches the PR base branch, and PR jobs test the merge ref. [GitHub’s event documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows) confirms both behaviors.

Bootstrap procedure:

1. Open C0 into `security-hardening`.
2. Run local actionlint.
3. Manually dispatch `accelerator.yml`, `sdk.yml`, `app.yml` and `actionlint.yml` on the C0 ref.
4. Merge only after all four are green.
5. Then apply a temporary `security-hardening` ruleset requiring the four status aggregators.

Current internal `changes` jobs are suitable: the workflows still start and the aggregator passes legitimately skipped component jobs. Do not add top-level `paths` filters to required workflows; skipped required workflows can remain pending.

The existing concurrency keys are fine. `actionlint.yml` will lint its own trigger edit. Do not use `pull_request_target`.

Remove the temporary `security-hardening` trigger/ruleset during the final integration PR into `main`, after main-target CI has run.

## Non-negotiable implementation details

- **F-003:** create the Unix directory as `0700` and witness file as `0600` at creation—never write and chmod afterward. Use `cfg(unix)`; Windows relies on its ACL model. macOS validates the Unix path in CI.

- **F-009:** authorize first, acquire the proof permit, then read the body under the 50 MB limit and a body-read timeout. The timeout prevents a slowloris from monopolizing the only permit. Test oversized body, cancellation, permit release and that a second body is not polled early.

- **F-011:** reject trailing-dot origins in the canonical-origin constructor. Do not silently migrate them to undotted approvals. Invalid persisted entries are dropped with a warning. Keep Host-header normalization separate.

- **F-004:** sign a canonical manifest envelope embedded in the same `latest.json`. Cover version, publication time and each platform’s URL, artifact signature and size. Verify the envelope with the updater public key and require `Update.raw_json` to match it exactly. This binds version to signed artifact without fragile per-platform artifact introspection. Sign in a dedicated job with no AWS/GitHub write permissions.

  Persist the highest successfully running installed version in an atomic `0600` config write. Require candidate `> max(current, floor)` and update the floor only after the new build starts successfully. Corrupt floor means updater disabled/fail-closed, not reset.

  The now-signed size fixes the feed-only lie against the preflight cap. It does not bound actual bytes if the artifact host serves a huge object before signature rejection; keep that residual documented. Commit `9e0d742`’s Chrome-142 LNA note has no coupling to rollback logic.

- **F-005:** the current landing `aws s3 sync ... landing/ --delete` can delete `landing/releases/latest.json`. Add `--exclude 'releases/*'` and an IAM explicit deny. This is the hidden release-destruction landmine in [deploy-landing.yml](.github/workflows/deploy-landing.yml:37).

- **F-007:** both download paths verify the GitHub release API digest, extract into a private staging directory, reject unsafe archive members, and atomically publish the binary plus a marker containing the verified archive digest and extracted-binary digest. Runtime rehashes cached `bb` against the marker on every use. Missing/malformed legacy markers fail closed and force redownload.

- **F-008:** remove auto-pinning from `update-aztec-version.ts`. Accept a Windows pin only from signed upstream checksum/provenance, a verifiable artifact attestation, or a reproducible build, followed by human review. A GitHub asset downloaded twice is not independent evidence. Revalidate the current live pin; do not grandfather it blindly. If independent evidence does not exist, block that Windows version.

  Also fix `_aztec-update.yml`: `auto_merge: false` currently performs an immediate merge. It must leave the PR open.

- **F-010:** serialize the `ExecStart` path using systemd unit escaping over Unix path bytes; reject controls/newlines, escape quotes/backslashes and double `%`. Validate generated units with `systemd-analyze verify` without installing them.

- **F-012:** externalize inline scripts/styles, set `withGlobalTauri:false`, bundle imports from `@tauri-apps/api`, and establish a strict CSP with only the documented Tauri IPC connect sources. Tauri’s own guidance requires CSP to be explicitly configured and tightly scoped: [Tauri CSP](https://v2.tauri.app/security/csp/).

  Declare custom commands in `build.rs`, split capabilities by window label, and retain Rust-side caller-label checks as defense in depth. Tauri currently allows registered custom commands to all windows unless explicitly constrained: [Tauri capabilities](https://v2.tauri.app/security/capabilities/).

- **F-014:** preserve scheme, security-relevant subdomain context and the registrable domain; use a bundled PSL implementation, keep punycode canonical, and expose the full origin through accessible text/title. Make content scrollable, keep actions reachable, and default Remember unchecked.

- **F-015:** SHA-pin every remote `uses:`, including GitHub-owned actions, with version comments. GitHub calls a full commit SHA the safest action reference: [workflow syntax guidance](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax). Also pin the actionlint download by commit plus checksum and stop selecting mutable `bun-version: latest`/Rust `stable` in the touched setup paths. Otherwise the same supply-chain class remains.

- **F-016:** `Zeroizing<KeyPair>` around the CA key, with explicit early drop immediately after leaf signing. In rcgen 0.13, this provably scrubs serialized DER but does not prove every backend allocation is wiped; document that residual. The leaf key remains persistent by design.

No finding is dropped. F-008 is tightened to fail closed, F-015 is minimally widened to cover mutable downloaded tooling, and F-002 is delayed—not waived.