# Repo Map — Frontend HTML, TS scripts, CI/CD, IaC

## A. Frontend HTML (served locally by Tauri, ./frontend)
`withGlobalTauri: true` (tauri.conf.json:11) => window.__TAURI__.core.invoke available to all pages. **No app.security.csp key set.**
- `authorize.html` — TRUST-DECISION surface. Query params `origin`, `requestId` from location.search. Renders origin via `.textContent` (authorize.html:38) — not innerHTML. Calls invoke `get_verified_info({origin})`; writes `display_name` via `.textContent`. Echoes decision via invoke `respond_auth({requestId,origin,allowed,remember})`. Resolves by opaque requestId (SEC-06). No postMessage/innerHTML/eval.
- `settings.html` — invoke get_config/get_autostart_enabled/get_system_info. approved_origins via `.textContent`. CPU from backend not navigator.
- `update-prompt.html` — query params current/version via `.textContent`.
- `tauri-bridge.js` — shared invoke wrapper; all strings via `.textContent`.
- `verified-sites.json` — 2 entries (Nulo Wallet, Playground). schema states "NOT a security guarantee".
- `capabilities/default.json` — Tauri capability grant list.

## B. Download/integrity surfaces
- **copy-bb.ts (Windows prebuild): VERIFIED.** SHA-256 pinned per-version WINDOWS_BB_CHECKSUMS, assertSha256 throws on mismatch, fail-closed on unknown version. Content-Length + 64MiB caps, post-extract "only bb.exe" canary, System32 tar.exe execFileSync no shell.
- **copy-bb.ts (macOS/Linux): no network** — copies bb from installed @aztec/bb.js npm package. macOS clears quarantine.
- **scripts/download-bb.ts (local version cache): NO checksum/signature.** fetch tarball, arrayBuffer, pipe to `tar -xzf -`. Only checks tar exit + bb existence. macOS clears quarantine + ad-hoc re-sign. (NOTE: runtime path is core downloader.rs which DOES verify via GitHub digest; download-bb.ts is dev/CI tooling — confirm reachability.)
- **update-aztec-version.ts:80-98**: fetches Windows tarball, computes SHA-256 via crypto.subtle, writes hash into copy-bb.ts (trust-on-first-pin, no independent source).
- **App auto-updater**: endpoint https://aztec-accelerator.dev/releases/latest.json, minisign pubkey in tauri.conf.json:18. Windows installMode quiet.
- **latest.json gen** (release-accelerator.yml:641-706): reads per-platform .sig + sizes, emits latest.json -> S3 landing/releases/latest.json 300s cache. verify-live-feed HEAD-checks.
- **curl|bash tooling, no checksum**: actionlint.yml:56 (version-pinned), setup-aztec/action.yml:66 (version-pinned).

## C. CI trigger & privilege matrix (key rows)
No pull_request_target anywhere; PR gates use pull_request (fork => read-only token, no secrets). create-github-app-token SHA-pinned.
- `release-accelerator.yml` — workflow_dispatch(version regex-validated :35). Secrets: TAURI_SIGNING_*, APPLE_*, RELEASE_BOT_*, AWS_*, S3, CLOUDFRONT. Signs/notarizes, gh release create, uploads latest.json, opens auto-merge bump PR. Per-job scoped perms.
- `_publish-sdk.yml` — workflow_call/dispatch. id-token:write (npm provenance)+contents:write. NPM_TOKEN. `npm publish --provenance`, tag+release.
- `publish-testnet.yml` / `publish-nightlies.yml` — dispatch. NPM_TOKEN, AWS_*, TESTNET_AZTEC_NODE_URL. publish SDK + S3 deploy playground.
- `deploy-landing.yml` — push main (paths landing), dispatch. AWS_* OIDC. S3 deploy.
- `_aztec-update.yml` — workflow_call. App token contents+PR+issues write. Opens/merges dep PRs (auto or squash-merge). Interpolates `${{ inputs.* }}` into run: blocks (inputs from release matrix/dispatch, not PR authors).
- `aztec-nightlies.yml`/`aztec-stable.yml` — dispatch. contents+PR write. RELEASE_BOT_*.
- PR gates (accelerator/app/sdk/actionlint.yml) — pull_request, execute PR code, NO secrets, read-only token.

## D. Infra authz (infra/tofu)
- **S3** (s3.tf): bucket aztec-accelerator-site. All 4 public-access-block flags true. Policy: single AllowCloudFrontOAC (Principal cloudfront.amazonaws.com, s3:GetObject, /*, Condition AWS:SourceArn=distribution). No public principal. State backend encrypt+lockfile.
- **CloudFront** (cloudfront.tf): OAC signing always sigv4. Response headers COOP same-origin + COEP credentialless; **no CSP header**. subdomain_router viewer-request function routes by Host. redirect-to-https, TLSv1.2_2021, sni-only. query_string false, cookies none. **No WAF / no access-logging.**
- **IAM** (iam.tf): GitHub OIDC provider, client_id sts.amazonaws.com, thumbprint from var. CI role trust: AssumeRoleWithWebIdentity, aud=sts.amazonaws.com + **sub StringLike restricted to refs/heads/main, refs/heads/nightlies, refs/heads/chore/aztec-nightlies-*, refs/heads/chore/aztec-stable-*** (two wildcarded branch patterns — anyone who can push a matching branch can assume role). Policy: S3Deploy (PutObject/DeleteObject/ListBucket/GetBucketLocation on bucket + /*) + CloudFrontInvalidation. No `*` action/resource. **Write scope = entire bucket (all prefixes incl releases/), not per-prefix.**
- **ACM** (acm.tf): import-only, prevent_destroy, ignore_changes=all.
- **Branch protection** (main-branch-protection.json): target main, enforcement active, bypass_actors []. PR rule dismiss_stale_reviews, **required_approving_review_count 0**, no code owner review, no last-push approval. Required checks SDK/App/Accelerator Status, strict false, do_not_enforce_on_create true.

## Secrets
NPM_TOKEN, RELEASE_BOT_APP_ID/PRIVATE_KEY (short-lived installation token, per-use scoped), TAURI_SIGNING_*, APPLE_*, AWS_ROLE_ARN/REGION (OIDC, no static keys), S3_BUCKET_NAME, CLOUDFRONT_DISTRIBUTION_ID, TESTNET_AZTEC_NODE_URL. No static cloud/signing keys committed.
