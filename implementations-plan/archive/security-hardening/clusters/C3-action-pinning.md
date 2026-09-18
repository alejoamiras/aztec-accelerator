# C3 — action-pinning (F-015 + C0 shellcheck-glob) · /blueprint mid

**Cluster:** C3 · **Branch:** `sechard/action-pinning` off `security-hardening` · **Tier:** mid (mechanical but repo-wide across all workflows; supply-chain sensitive).

## Summary
Third-party AND GitHub-owned actions are pinned by **mutable tags** (`actions/checkout@v6`, `oven-sh/setup-bun@v2`, `aws-actions/configure-aws-credentials@v6`, …). A compromised/retagged action runs in our CI with whatever permissions that job holds — including the job that mints the AWS deploy session. Fix (F-015): SHA-pin every remote `uses:` to a full commit SHA with a `# vX.Y.Z` comment; pin the mutable toolchain selectors (`bun-version: latest`, `rust-toolchain@stable`); pin the actionlint installer download; and fix the C0-discovered `shellcheck infra/*.sh` glob that errors when no `.sh` exists.

## Tier justification (Phase 0.5)
Novelty L · Blast-radius **H** (every workflow; the AWS-cred + publish jobs) · Irreversibility L · Migration L · External-coupling **H** (many third-party actions) · Security-sensitivity **H**. Mechanical fix but 3 highs + repo-wide → **mid** (codex + fable dual plan-audit).

## Distinct refs to pin (survey)
`actions/checkout@v6` (46), `oven-sh/setup-bun@v2` (22), `actions/cache@v5` +`/save`+`/restore` (22), `actions/upload-artifact@v7` (7), `actions/download-artifact@v8` (6), `dorny/paths-filter@v4` (6), `aws-actions/configure-aws-credentials@v6` (4), `actions/setup-node@v6` (2), `Swatinem/rust-cache@v2` (2), `dtolnay/rust-toolchain@stable` (2), `opentofu/setup-opentofu@v1` (1), `foundry-rs/foundry-toolchain@v1` (1). Already SHA-pinned: `actions/create-github-app-token@bcd2ba49…`.

## Phases
**Phase 1 — SHA-pin every remote `uses:`.** For each distinct `owner/repo@tag`, resolve the tag → full commit SHA via `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` (deref annotated tags to the commit), then replace `@tag` → `@<sha> # <tag>` everywhere. GitHub-owned included (GitHub's own guidance: a full SHA is the only immutable ref). `actions/cache`, `cache/save`, `cache/restore` share the cache repo SHA.
- *Gate:* `bun run lint:actions` exit 0; grep proof `! grep -rE 'uses: [^ ]+@v[0-9]' .github` (no version-tag refs remain except a documented allowlist if any); every pinned line carries a `# vX` comment.

**Phase 2 — kill mutable toolchain selectors.** `oven-sh/setup-bun` `bun-version: latest` → a pinned `bun-version: <x.y.z>` (match the repo's current bun). `dtolnay/rust-toolchain@stable` → pin the ACTION to a SHA (Phase 1); for the CHANNEL, pin to the repo's current Rust toolchain version if a `rust-toolchain.toml` exists, else keep `stable` with a documented rationale (pinning the channel can break on new stables — decide with the audit).
- *Gate:* actionlint 0; the touched setup workflows still resolve; CI green.

**Phase 3 — pin the actionlint installer.** `actionlint.yml` downloads actionlint via `bash <(curl … download-actionlint.bash)` at a version tag → pin the script by commit SHA + verify a checksum of the downloaded binary (or vendor a pinned version). No `curl | bash` of a moving ref.
- *Gate:* actionlint.yml still lints; CI green.

**Phase 4 — fix the shellcheck glob (C0 discovery).** `shellcheck infra/*.sh` errors when `infra/` has no `.sh` (only bites on workflow_dispatch, but latent). Make it nullglob-safe: e.g. `shopt -s nullglob; files=(infra/*.sh); [ ${#files[@]} -gt 0 ] && shellcheck "${files[@]}" || echo "no infra shell scripts"` (or `find infra -name '*.sh' -print0 | xargs -0r shellcheck`).
- *Gate:* dispatch actionlint.yml (or reason) no longer fails on the shellcheck step; actionlint 0.

## Security & Adversarial Considerations
- **Threat model:** supply-chain — a maintainer of any referenced action (or an attacker who compromises one) retags a version to malicious code; our CI runs it with the job's token (worst case: the `aws-actions/configure-aws-credentials` job → AWS deploy session, or a publish job → NPM_TOKEN). SHA-pinning removes the mutable-tag attack; only a hash-collision or a force-pushed SHA (git prevents) could bypass.
- **Least privilege:** unchanged; pinning is orthogonal to token scopes (C5/F-005 handles scoping).
- **Supply chain:** this IS the supply-chain hardening. Residual: SHA-pinning freezes VERSIONS (no auto security patches) → note that a manual bump process (or Dependabot pinned-SHA updates) is the maintenance counterpart; out of scope to add here.
- **Crypto:** the actionlint download should verify a checksum (integrity of the fetched binary).

## Assumptions
**Facts:** (1) survey above enumerates all `uses:` refs (grep of `.github/`). (2) `create-github-app-token` is already SHA-pinned (precedent for the format). (3) local gate `bun run lint:actions`; CI `actionlint.yml` (+ this touches all workflows, so accelerator/sdk/app gates also run on the PR). (4) GitHub resolves annotated tags via `git/ref/tags/<tag>` → may need a second deref to the commit (`^{}`).
**Inferences:** pinning GitHub-owned actions to SHAs won't break (GitHub supports it); `actions/cache*` sub-actions share one repo SHA. Attack in audit.
**Asks:** none blocking — but the Rust-channel-pinning decision (Phase 2) is surfaced for the audit to resolve (pin channel vs keep `stable`).

## Seeds
Campaign-level `/goal` + `/loop` (../plan.md) drive this; C3 is one loop iteration.

## Dual-audit fold (GATE 1 complete) — both Codex + Fable: conditional approve (convergent)
**Decision ledger:**
- SHA-pinned all 122 remote `uses:` refs (incl GitHub-owned; cache/save/restore share one SHA). Comments keep the major tag `# vN`.
- dtolnay/rust-toolchain: pinned to master SHA `fa04a145…` + explicit `with: toolchain: stable` at both sites (pinning alone would break the channel). Kept channel `stable` (no rust-toolchain.toml; Rust ships from signed infra — different trust class).
- Remaining GATE-2 items (this branch, not yet done): [a] `packageManager: bun@1.3.14` in root package.json + `bun-version-file` at setup-bun sites (incl the 3 implicit-latest credentialed ones); [b] `node-version: 24`→`24.18.0`; [c] actionlint installer → download exact release + verify hardcoded SHA-256, verify on cache-hit, target 1.7.12 (current CI mislabels 1.7.11→1.7.10); [d] shellcheck → `find … -print0 | xargs -0r shellcheck`; [e] `.github/dependabot.yml` (github-actions weekly, directories incl `/.github/actions/*`).
- Deferred/residual (documented): publish-*/deploy global `contents/id-token: write` (→ C5); setup-aztec unverified `install.aztec.network` curl; SHA-pin-without-bump staleness (mitigated by [e]).
