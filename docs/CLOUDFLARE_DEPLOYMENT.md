# Cloudflare deployment

The landing page and playground use Cloudflare Workers Static Assets. The signed desktop update
manifest is served by a small Worker from KV, keeping feed promotion separate from site deploys.
No OpenTofu, S3, CloudFront, or build server is required.

## Fork setup

1. Run `wrangler login` and finish the browser OAuth flow. Verify with `wrangler whoami`.
2. Create a namespace with `wrangler kv namespace create RELEASE_FEED` and replace the namespace ID
   in `packages/release-feed/wrangler.jsonc` and `.github/workflows/release-accelerator.yml`.
3. Change the Worker names and custom-domain routes in each `wrangler.jsonc` for the fork's account.
4. Create `CLOUDFLARE_ACCOUNT_ID`, `CLOUDFLARE_DEPLOY_API_TOKEN`,
   `CLOUDFLARE_RELEASE_FEED_DEPLOY_API_TOKEN`, and `CLOUDFLARE_RELEASE_FEED_API_TOKEN` GitHub Actions
   secrets. Keep the promotion token limited to KV read/write. Keep the release-feed deployment token
   separate from the site deployment token and scope it to that Worker where the account supports it.
   Worker route deployment also needs Workers Routes edit and Zone read access.
5. Seed `latest.json`, remove any conflicting DNS-only CNAME, deploy the landing Custom Domain so a
   proxied record exists, then deploy the release-feed route and playground Custom Domain. Keep the
   cutover window short and verify all three public endpoints immediately.

The release-feed Worker deploy is manual and uses the protected `release-feed` GitHub environment.
Feed content promotion remains a separate workflow operation with a KV-only credential.

GitHub Actions runs the Vite builds, so build-time variables such as `AZTEC_NODE_URL` are compiled into
the publicly downloadable bundle before Wrangler uploads `dist`. Treat them as public configuration;
never place credentials or private values in Vite build-time variables. They are not Worker runtime
variables.

Every Worker deployment creates an immutable version and a `workers.dev` preview URL. For a named
preview, use `wrangler versions upload --preview-alias <name>` with the package config. Production
deployments and rollbacks remain explicit Actions/CLI operations rather than Cloudflare's Pages Git
integration.
