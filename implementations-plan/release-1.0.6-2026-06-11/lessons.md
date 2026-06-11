# Release 1.0.6 — Aztec 4.3.1 bump + release (2026-06-11)

**Shipped:** @aztec 4.2.0 → 4.3.1 (SDK + playground + bundled bb), accelerator 1.0.6 stable, SDK
`4.3.1` published (npm latest + testnet). Three distinct landmines, all root-caused with durable fixes.

## 1. The aztec bump tooling covers npm deps only
`aztec-stable.yml` updates package.jsons + lockfile. Manual completions every bump:
- **`WINDOWS_BB_CHECKSUMS`** in `copy-bb.ts` — pin the new version's Windows tarball sha256 (fail-closed by design).
- `AZTEC_VERSION` self-heals via copy-bb (gitignored, generated) — no action, despite looking stale locally.

## 2. Foundry 1.7 broke aztec's L1 deploy wrapper (E2E down 3 legs)
Aztec 4.3.1 switched its local-network L1 deploy to `forge script` (4.2.0 used viem — never spawned
forge). Foundry 1.7 made `--batch-size` require the new `--batch` flag; aztec's wrapper passes
`--batch-size` standalone → "required arguments were not provided: --batch". Two-layer fix in
`setup-aztec`: pin foundry-toolchain to **v1.4.1** (the lineage aztec bundles) AND rename the
aztec-bundled forge aside (it's 1.7-lineage too and wins PATH otherwise). The rename step's
"forge now resolves to" log line is what isolated the real cause — instrument first.
**Unpin when aztec's `forge_broadcast.js` supports the 1.7 CLI.**

## 3. The SDK's GitHub release poisoned N-1 resolution (4/6 updater smokes red)
The SDK publish creates a metadata-only GitHub release; it (a) stole the repo's **Latest** badge
from `accelerator-v1.0.5` and (b) became the newest non-prerelease release, which the updater
smokes' N-1 resolver (`gh release list --exclude-pre-releases --limit 1`) picked → "no assets to
download". Windows survived only because it bootstraps N-1 synthetically. Fixes (#360): resolvers
filter `accelerator-v*`; `_publish-sdk` always `--latest=false` (the `latest` input keeps its npm
dist-tag meaning). Proven same-day: the 4.3.1 SDK release sits newest by date while 1.0.6 holds Latest.
**Meta-lesson: a new release family in the repo means auditing every `gh release list` consumer.**

## Carry-overs that paid off
Rollback staged before each cut; full-verification watcher (feed poll → completeness incl. SEC-03
sizes → asset HEADs); rerun-failed-only discipline; What's-new prepend per behavior-visible release.
