# Phase 3 lessons — promote `latest` (2026-08-26)

**Outcome: ✓ GREEN. `latest` = `testnet` = 5.2.0; a newcomer's bare install resolves and loads it.**

## Two stale-read traps, both of which faked a failure
1. **Registry read-back lags the write.** `npm dist-tag add` printed
   `+latest: @alejoamiras/aztec-accelerator@5.2.0`, and the workflow's own `npm view` **0.4 seconds
   later** still reported `latest: '5.0.1'` — as did a local `npm view` a minute after that. The
   write had landed; the read was served stale. A cache-bypassing packument fetch
   (`curl https://registry.npmjs.org/@scope%2fname`) showed the truth immediately. **The
   authoritative signal is the mutation's own output, not an immediate read-back** — treating the
   stale read as "promote failed" would have triggered a pointless rollback of a correct promote.
2. **npm's local packument cache silently resolves the OLD `latest`.** The first newcomer-install
   check installed **5.0.1** minutes after the promote and then failed on a missing export — a
   scary-looking result that was pure cache. `--prefer-online` forces revalidation and got 5.2.0.
   Any post-promote install check on a machine that queried the package recently MUST revalidate,
   or it silently verifies the previous release.

Both traps produce false negatives that look exactly like real regressions, and both cost a
rollback if believed. Verify a mutation from the mutation, then from a cache-free read.

## A harness bug worth keeping
The first version of the check did `require("@alejoamiras/aztec-accelerator/package.json")` and hit
`ERR_PACKAGE_PATH_NOT_EXPORTED`. That is **correct package behavior** — the published `exports` map
intentionally exposes only `"."` — so the harness, not the artifact, was wrong. Read the installed
manifest from `node_modules/...` directly instead. A verification script that reaches for
unexported subpaths will keep manufacturing failures against a healthy package.

## What the gate actually proved
Bare `npm install` (no version, no tag) → 5.2.0, and `import()` yielded a callable
`AcceleratorProver` plus `ACCELERATOR_API_VERSION=1` from the dist-only build. That exercises the
publish-time manifest rewrite end to end, from the registry, as a stranger — the one thing no
workspace build or PR gate covers.

LESSONS_FILE=implementations-plan/sdk-5-2-0-release/lessons/phase-3.md
