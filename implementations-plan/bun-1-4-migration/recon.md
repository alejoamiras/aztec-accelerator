# Recon — Bun 1.3.14 → 1.4 migration (2026-08-25)

Sources: the Bun 1.4 announcement (read in full), three parallel Sonnet recon agents
(runtime/scripts, test+CI, breakage-risk) over a fresh main checkout (`b605075`), and direct
empirical tests with the real 1.4.0 binary. Owner pushback reversed two initial
"not worth it" calls (ky, serve) — reflected below.

## Empirical facts (tested with bun 1.4.0, 2026-08-25)

- **F-E1**: Latest bun IS `1.4.0` — no patch releases yet.
- **F-E2 (the blocker)**: under 1.4.0, `new Worker` (node:worker_threads) crashes inside Bun's own
  `internal:worker/messaging` (`port.on is not a function` at `registerMainThreadPort`), triggered
  by pino's `thread-stream` transport worker. Reproduced on `packages/playground/src/aztec.test.ts`.
  No matching upstream issue found — we should file one with a minimal repro (pino transport under
  `bun test`).
- **F-E3**: `minimumReleaseAgeExcludes` wildcard (`@aztec/*`) is STILL silently ignored in 1.4.0
  (tested against a same-day @aztec nightly: blocked with the glob present). The 31-name exact list
  + `bunfig-aztec-excludes.test.ts` stay.
- **F-E4**: `expect.addEqualityTesters` is STILL undefined in 1.4.0's bun:test — all three shims stay.

## The worker-crash mechanics (agent C, verified in installed sources)

- `@aztec/foundation`'s pino logger builds `pino.transport({targets})` → thread-stream →
  `new Worker` **synchronously at module import** — importing any `@aztec/*` package under bun
  crashes the whole `bun test` invocation.
- **The only working knob is `process.env.JEST_WORKER_ID`** (any truthy value): an intentional
  early-exit in aztec's own logger that uses a sync fd destination (no worker). `LOG_JSON` is a red
  herring (still builds a transport worker). Workaround: set it first-line in the three existing
  preloads (`packages/sdk/src/test-setup.ts`, `packages/sdk/e2e/e2e-setup.ts`,
  `packages/playground/src/happydom.ts`). Cosmetic cost only (no pretty logs/OTLP in tests).
- **Crash-affected suites**: sdk `test:unit` (accelerator-prover + public-contract import the chain;
  drags accelerator-transport down with them), all sdk e2e, playground `test:unit` (aztec.test.ts
  drags its dir). NOT affected: accelerator scripts tests, root scripts tests, landing (no @aztec
  runtime imports).
- **Second, independent worker path**: `@aztec/bb.js`'s NODE factory does a real
  `new Worker(main.worker.js)`. Unit tests never reach it (createChonkProof is spyOn-mocked;
  WASMSimulator wraps acvm which has no workers). **SDK e2e's WASM fallback leg DOES reach it.**
  Whether bun 1.4.0's Worker bug also breaks this pattern is UNKNOWN — the empirical spike decides;
  if broken, the runtime bump of e2e-running CI holds until upstream fixes.
- kv-store's worker is OPFS/browser-only (unreachable under bun); msgpackr none; piscina absent;
  workerpool only inside wdio's own tooling (not under bun test).

## TLS tightening surface (1.4: servername-from-host for IPs, earlier checkServerIdentity)

- Our code never touches node:tls / Bun.connect / Bun.listen directly; all TLS via fetch/Bun.serve.
- SDK unit tests: fetch fully mocked (no sockets). Frontend probes to `https://127.0.0.1:59834`
  run in browsers (unaffected).
- **The one real site**: SDK e2e under bun — `AcceleratorTransport.probeHealth()` ALWAYS dual-probes
  HTTP+HTTPS; with the desktop accelerator running, a genuine bun-TLS handshake hits
  `https://127.0.0.1:59834` against the self-signed leaf. The leaf already carries
  `IpAddress(127.0.0.1)/IpAddress(::1)/DnsName(localhost)` SANs (`certs.rs:227-229`) → likely fine;
  verify empirically in the spike.
- `updater-feed-server.ts` is a Bun.serve TLS SERVER impersonating a hostname via /etc/hosts —
  untouched by client-side changes.

## Version-pin topology (agent B)

- **23 `setup-bun` sites**: 21 × `1.3.14` inline in workflows, 2 composites
  (`setup-aztec:15`, `setup-accelerator:94`) — full file:line table in the agent report;
  plus `@types/bun ^1.3.11` (root package.json). No `engines.bun`/`packageManager` anywhere.
- **`_publish-sdk.yml:65` = `bun-version: latest`** — the ONE floating site (slipped through the
  security-hardening C3 pass). The publish pipeline is ALREADY exposed to 1.4.0 today. Traced: it
  never runs `bun test` (install/tsc/two import-free scripts) so it won't hit the worker crash, but
  it must be pinned regardless.
- Prior codex recommendation (C3 audit, unadopted): centralize via one source. Owner decision
  2026-08-25: **adopt `.bun-version` + `bun-version-file:`** (setup-bun supports it) — 23 sites → 1.

## Adoption map (owner-calibrated)

**In scope — safe wave (no 1.4 binary required):**
1. Pin `_publish-sdk.yml` (hotfix-grade).
2. `.bun-version` centralization + guard extension (no inline `bun-version:` reintroduction —
   extend `scripts/action-pins.test.ts`-style checking or a sibling test).
3. **Isolated linker** (`[install] linker = "isolated"`): shipped in 1.3.14, matured in 1.4. Zero CI
   cache changes needed — the global store lives under `~/.bun/install/cache/` which every cache
   step already covers. Wins: up to 7× warm installs × ~18 CI install sites; thinner worktrees
   locally. Verify: packaged-e2e swap-sdk script (release-gating; analysis says linker-agnostic —
   `rm -rf` symlink + materialize real dir — but smoke it), tsconfig.e2e.json's vite path override
   (becomes unnecessary, not broken), and multi-worktree concurrent-install contention (open
   question; empirical).
4. `JEST_WORKER_ID=1` in the three preloads (harmless under 1.3.14, required under 1.4).

**In scope — bump wave (needs 1.4 binary, gated on spike):**
5. Flip `.bun-version` to 1.4.0 (all CI + local) + `@types/bun` bump + one isolated
   lockfile-regen commit (expect format-only churn; zero version movement).
6. `bun run --parallel` for the `lint` chain (sequential inside 3 live CI jobs: biome/pkg/shell/rust)
   and the local `test:unit`/`test:typecheck` chains. NOT the root `test` chain (fail-fast ordering
   is intentional). tsc-concurrency caveat: verify on CI-class hardware before committing playground's
   3-tsc chain.
- 7. Per-test `{retry: 1-2}` on the four live-network bun:test surfaces ONLY (sdk e2e connectivity/
  proving/remote-network network-bound tests, playground live-node block). Explicit trap: NEVER on
  `release-contract.test.ts` (it unit-tests retry logic itself — double-retry masks bugs).
8. `bun test --parallel` (WITHOUT --isolate): empirical try per suite — preloads apply per worker
  process, so should be safe; adopt where results byte-identical, else drop. Seconds-scale win;
  rides because it's free to verify.
9. **ky → raw fetch in the published SDK** (owner reversal): the transport test suite (zero @aztec
  imports) pins behavior; `AbortSignal.timeout()` + existing `AcceleratorHttpError` replace ky's
  surface; removes a runtime dep from the published package. Gate: transport suite green, zero
  public-API change, timeout semantics preserved (header-vs-body deadline comments).
10. **`serve` devDep → Bun.serve dir-serving** (owner reversal; 1.4's `routes: {"/*": {dir}}` made
  it ~6 lines): swap `bunx serve -l 3456` in accelerator's playwright webServer; verify ETag/
  Content-Type parity for the packaged flow; delete the dep.
11. `Bun.Archive` for `copy-bb.ts` Windows path (kills the System32-bsdtar workaround; tarball
  already SHA-pinned pre-extraction, only a name/count canary to keep). `download-bb.ts` ONLY if
  Archive proves per-entry type inspection + bounded output (F-007 safety walk with regression
  tests — do not swap blind; keep zlib `maxOutputLength` gzip bound regardless).
12. `--no-orphans` on the two Playwright webServer commands (accelerator playwright.config.mjs:16,
  playground playwright.config.ts:9).
13. Comment hygiene: bunfig "verified on 1.3.14" notes → re-verified-on-1.4 wording (F-E3/F-E4).

**Out of scope (recon-ledgered, reasons in agent reports):** `--shard` (suites too small),
`--changed` (paths-filter already solves it coarser+cheaper), audit-fix/dedupe/auto-update
automation (against the human-reviewed-bumps philosophy), Bun.WebView (mature Playwright suites),
JSON5/JSONC/XML/TOML/markdown/PTY/ANSI helpers (no such surface exists), `--isolate` (preload-time
global mutation semantics unresolved), `--no-env-file` (fpc scripts need .env autoload),
cpu/heap-prof + memoryPressure (no long-running bun daemon), flattening root `test` chain
(fail-fast ordering), `prepare-sdk-publish`/`helpers` fs→Bun.file style nits.

## Lockfile / install-behavior deltas

- No github:/tarball/file:/link: deps, no trustedDependencies keys anywhere → the 1.4
  trustedDependencies-scoping and integrity-hash changes are non-events here (hash mechanism also
  predates us: 1.3.10).
- Linker flip alone must produce ZERO bun.lock diff (materialization, not resolution). First 1.4
  install may re-stamp lockfile format — keep it an isolated commit for bisectability.

## Sequencing insight

Safe wave (1-4) is land-now at 1.3.14. The spike (workers × 2, TLS, --parallel semantics) decides
whether the bump wave lands now or holds behind an upstream bun fix (file the Worker bug either
way). Owner validation gates: full PR CI + a release-path exercise + a local live-testnet smoke
under 1.4.
