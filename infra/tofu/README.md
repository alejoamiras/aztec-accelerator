# Infrastructure — OpenTofu

Static site hosting for `aztec-accelerator.dev` via S3 + CloudFront.

## Architecture

```
aztec-accelerator.dev              → CloudFront → S3 /landing/
playground.aztec-accelerator.dev   → CloudFront → S3 /playground/
```

A CloudFront function routes requests to the correct S3 prefix based on the `Host` header. SPA fallback (no file extension → `index.html`) is handled in the same function.

## Resources

| Resource | Purpose |
|----------|---------|
| S3 bucket (`aztec-accelerator-site`) | Static files (`/landing/` + `/playground/` prefixes) |
| CloudFront distribution | CDN with COOP/COEP headers, HTTPS, subdomain routing |
| CloudFront function | Host-based request routing + SPA fallback |
| ACM certificate | TLS for `aztec-accelerator.dev` + `*.aztec-accelerator.dev` (us-east-1) |
| IAM OIDC role | GitHub Actions → AWS for deploy |

## Bootstrap

Before `tofu init`, create the S3 state bucket:

```bash
aws s3api create-bucket --bucket aztec-accelerator-tfstate --region us-east-1
aws s3api put-bucket-versioning --bucket aztec-accelerator-tfstate \
  --versioning-configuration Status=Enabled
aws s3api put-public-access-block --bucket aztec-accelerator-tfstate \
  --public-access-block-configuration \
  BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
```

Then create the ACM certificate:

1. AWS Console → ACM → Request certificate (us-east-1)
2. Domain: `aztec-accelerator.dev` + `*.aztec-accelerator.dev`
3. Validation: DNS
4. Add the CNAME record in Cloudflare (proxied=false)
5. Wait for validation, copy the ARN

Set up Cloudflare DNS:

```bash
# Point both domains to CloudFront (after tofu apply)
# CNAME aztec-accelerator.dev            → <cf-domain>.cloudfront.net (DNS only)
# CNAME playground.aztec-accelerator.dev → <cf-domain>.cloudfront.net (DNS only)
```

## Usage

```bash
cp terraform.tfvars.example terraform.tfvars
# Fill in acm_certificate_arn

tofu init
tofu plan
tofu apply
```

## Deploy flow (CI)

```bash
# Landing page
bun run --cwd packages/landing build
aws s3 sync packages/landing/dist/ s3://aztec-accelerator-site/landing/ --delete
aws cloudfront create-invalidation --distribution-id $CF_DIST_ID --paths "/landing/*"

# Playground
bun run --cwd packages/playground build
aws s3 sync packages/playground/dist/ s3://aztec-accelerator-site/playground/ --delete
aws cloudfront create-invalidation --distribution-id $CF_DIST_ID --paths "/playground/*"
```

## GitHub Secrets (from tofu output)

After `tofu apply`, set these in the repo's GitHub settings:

| Secret | Value |
|--------|-------|
| `AWS_ROLE_ARN_LANDING` | `tofu output -raw landing_deploy_role_arn` |
| `AWS_ROLE_ARN_RELEASE` | `tofu output -raw release_feed_role_arn` |
| `AWS_ROLE_ARN_PLAYGROUND` | `tofu output -raw playground_testnet_role_arn` |
| `AWS_REGION` | `us-east-1` |
| `S3_BUCKET_NAME` | `tofu output s3_bucket_name` |
| `CLOUDFRONT_DISTRIBUTION_ID` | `tofu output cloudfront_distribution_id` |

**F-005 (C5) per-pipeline deploy roles.** The single `AWS_ROLE_ARN` (from `ci_role_arn`) is replaced by
three least-privilege, per-pipeline roles, each trusting ONLY its own workflow (the OIDC `workflow` NAME
claim) on `main`. Only the release pipeline may write `landing/releases/latest.json` (the F-004 update
feed); the landing role is explicitly denied it. **Cutover is staged + human-applied** — see the C5
runbook (`implementations-plan/security-hardening/clusters/C5-runbook.md`): apply roles (legacy retained,
trust-narrowed) → set the 3 new secrets → land the workflow cutover on `main` → smoke (incl. a negative
cross-role AssumeRole test) → delete the legacy role + `AWS_ROLE_ARN` in a separate PR. `deploy-landing`
must keep `--exclude "releases"`/`"releases/*"` on its `sync --delete` (the IAM Deny enforces it too).
Adding a second release-feed object requires updating the release role's exact-object policy.
