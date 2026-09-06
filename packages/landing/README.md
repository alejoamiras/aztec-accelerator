# Aztec Accelerator Landing Page

Static landing page for the Aztec Accelerator project.

Production now serves the script-free `retirement/index.html` migration page linking to
[Presto](https://presto.build). The source and `dist/` build below remain regression fixtures;
Wrangler uploads only `retirement/`, never the old download/detection UI.

Accelerator detection is HTTPS-first and never treats its witness-free HTTP diagnostic as proving
availability. The page can explain how to enable **Encrypted Connection** or repair certificate
trust, but it never persists consent or enables plaintext proving. Deliberate, current-tab-only HTTP
fallback belongs to an integrating dApp such as the playground.

## Live Site

[aztec-accelerator.dev](https://aztec-accelerator.dev)

## Development

```bash
cd packages/landing
bun run dev       # Start dev server
bun run build     # Build for production (output: dist/)
bun run preview   # Preview production build locally
```

## Deployment

Auto-deployed on push to `main` via the [`deploy-landing.yml`](../../.github/workflows/deploy-landing.yml) workflow. Hosted with Cloudflare Workers Static Assets.

## License

[AGPL-3.0](../../LICENSE)
