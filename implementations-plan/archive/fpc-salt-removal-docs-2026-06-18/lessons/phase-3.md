# Phase 3 — Refresh docs (2026-06-18)

Six docs refreshed. Each claim re-verified against source/installed types before writing (the plan flagged the SDK example as an Inference; codex flagged the headless caveat).

- **`packages/sdk/README.md`** — the Quick Start `getSchnorrAccount(pxe, secretKey, signingKey, Fr.ZERO, prover).getWallet()` was dead: `grep` of installed `@aztec/accounts` shows **no standalone `getSchnorrAccount`** in 5.0 (only `getSchnorrAccountContractAddress`/`...Artifact`). 5.0 injects the prover via the PXE (`pxe.proverOrOptions`), not as a `getSchnorrAccount` arg. Rewrote Quick Start to construct the prover + point at the existing (codex-verified-valid) `EmbeddedWallet.create(url,{pxe})` section. Did **not** touch `EmbeddedWallet.create("http://localhost:8080",…)` — `string | AztecNode` still valid (our own e2e passes a node client).
- **`packages/accelerator/README.md`** — (a) added a **Version Model** section (runtime-bb ⇒ `@aztec` bump = SDK-only, no app re-release). (b) Fixed the stale headless caveat: it claimed the build "still pulls Tauri in transitively" + needs `libwebkit2gtk-4.1`/`libgtk-3`. **Verified false**: `server/Cargo.toml` deps `accelerator-core` (path `../core`), core is Tauri-free, and `_e2e.yml:48` installs only `libssl-dev`. (c) Same staleness at `:183` ("depends on the desktop crate's library target") — also corrected to `accelerator-core`. (d) `ACCELERATOR_VERSION` example `1.0.2`→`1.0.6` (latest stable per `gh release list`).
- **`docs/RELEASE_RUNBOOK.md`** — added a Release-Types table (SDK vs accelerator) + a **Releasing the SDK to npm** section. Verified the publish auth before writing: `_publish-sdk.yml` uses static `NPM_TOKEN` **with** `npm publish --provenance` (Sigstore via `id-token: write`), `--tag testnet`, `latest:false` — did NOT claim "OIDC/no static token" (would have been false).
- **`README.md`** (root) — accelerator row now mentions the headless CI server; added a 5.0/`testnet`-dist-tag + runtime-bb note.
- **`packages/playground/README.md`** — dropped the `SPONSORED_FPC_SALT` env row; noted the canonical salt=0 FPC default; fixed the stale "deployed via `app.yml` on push to main" (app.yml is now a pure PR gate; deploy is a `publish-testnet.yml` dispatch — #364 decoupling).
- **`implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-0.md`** — appended an UPDATE correcting the recon-time "salt=0 NOT published on v5" (we deployed+funded it, claim block 1387).

**Gate:** PASS (manual review) — no `SPONSORED_FPC_SALT` in refreshed prose; no absolute local paths; cross-doc anchors (`#version-model-…`, `#embedded-wallet-browser-dapps`) resolve to real headers; diff focused (6 files, +49/-13).

LESSONS_FILE=implementations-plan/fpc-salt-removal-docs-2026-06-18/lessons/phase-3.md
