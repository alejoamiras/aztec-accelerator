# Phase 3b — SDK publish: BLOCKED

## What happened

Triggered `gh workflow run publish-testnet.yml --ref main` (run id 26533045020) post PR B merge.

**Failure**: `publish-sdk / Publish SDK` job failed at the `npm publish --provenance --access public --tag testnet --workspaces=false` step.

**Error**:
```
npm notice publish Signed provenance statement with source and build information from GitHub Actions
npm notice publish Provenance statement published to transparency log: https://search.sigstore.dev/?logIndex=1646386279
npm error code E404
npm error 404 Not Found - PUT https://registry.npmjs.org/@alejoamiras%2faztec-accelerator - Not found
npm error 404
npm error 404  The requested resource '@alejoamiras/aztec-accelerator@4.2.0-revision.1' could not be found or you do not have permission to access it.
```

## Analysis

- The OIDC trusted publisher attestation SUCCEEDED (provenance was signed and published to Sigstore transparency log).
- The actual PUT to the npm registry was rejected with 404, which npm masks for 403 (no publish permission).
- Retried via `gh run rerun` — same result. NOT transient.
- The script fix (PR #226, A.2) is working — `get-sdk-publish-version.ts` produced `4.2.0-revision.1` correctly.

## Root cause hypothesis

One of:

1. **NPM_TOKEN expired or rotated** without updating the GitHub secret.
2. **Trusted publisher / OIDC configuration mismatch** on the npm side — provenance signing works but the publish path isn't whitelisted for this repo+workflow+branch combination.
3. **Repo-level npm publish settings** changed (e.g. 2FA-only mode).
4. **Package ownership** changed.

Last successful SDK publish was 2026-04-22 (`@alejoamiras/aztec-accelerator@4.2.0-rc.1`). No publishes happened between then and today's failed run (the 2026-05-27 successful run at 17:51 was triggered by a playground-only diff and skipped the publish-sdk job entirely).

## What's NOT broken

- The publish-version script (A.2 fix verified end-to-end).
- The publish-testnet.yml workflow_dispatch trigger.
- The e2e and deploy-app jobs (both succeeded).
- Playground deployed cleanly (per the user-approved decision to deploy on every dispatch).

## Action required

User needs to investigate npm side:

1. Confirm `NPM_TOKEN` repo secret is current and valid.
2. Check npm package trusted publisher configuration at `https://www.npmjs.com/package/@alejoamiras/aztec-accelerator/access`.
3. Verify package owner via `npm owner ls @alejoamiras/aztec-accelerator` (confirmed `alejoamiras <alejo.amiras@gmail.com>` — so ownership intact).
4. If trusted publisher is the issue, the OIDC subject claim needs to match the workflow's branch + path.

## Continuation strategy

Track 3c (accelerator 1.0.1 release) is fully independent of the SDK publish — the accelerator release pipeline builds Tauri bundles + headless `accelerator-server` binary, no npm interaction. Proceeding with 3c while the SDK publish remains blocked.

After user resolves the npm-side issue, re-trigger publish-testnet.yml or fall back to `npm dist-tag add @alejoamiras/aztec-accelerator@4.2.0 latest` (move tag to existing 4.2.0 publish — loses the "new code" aspect but ships).
