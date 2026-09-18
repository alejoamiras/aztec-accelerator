# Phase 1 lessons — bump + migrate + typecheck-enablement + lockfile (2026-07-13)

## Gate result: ✅ all green

- `bun run aztec:update 5.0.0`: 24 pins rewritten, **zero skips** (grep: all pins exactly `5.0.0`), CRS auto-bumped. Windows checksum **soft-failed auto-insert** (as the plan anticipated) but the tool printed the fetched sha; inserted manually into `copy-bb.ts` after independently confirming it against the GitHub release asset digest (`gh release view v5.0.0 … .digest` → `ec58f1d0…`, exact match).
- 5 `createSchnorrAccount` migrations + `Fq` imports: done; repo-wide grep shows every call passing a signing key. `tsc` (sdk + playground + scripts) caught no further 5.0.0 breaks — the changelog's other items genuinely miss our surface.
- `ephemeral: true` coverage **verified in installed 5.0.0** (`wallets/dest/embedded/entrypoints/browser.js`): deletes `pxeConfig.dataDirectory` AND routes walletDB to `openTmpStore(true)`. `openTmpStore(true)` still boots the SQLite worker but on a `:memory:` database — no OPFS dir, no origin-wide lock (second-tab concern gone at the storage layer).
- Lockfile: one local `bun install --minimum-release-age=0`; **lock diff had ZERO non-`@aztec` changed lines** (cleanest possible outcome — no stranger-package `npm view time` checks needed); `bun install --frozen-lockfile` exit 0.
- Playground typecheck enablement: `"types": ["bun", "vite/client"]` in tsconfig closed both config gaps (`bun:test`, `*.css`). That exposed **7 pre-existing type errors** in test files, fixed here: (a) `globalThis.fetch = mock(…)` fails Bun's `typeof fetch` (now has `preconnect`) → `setFetchMock` helper with one cast; (b) a literal-narrowing bug (`state.uiMode = "local"` narrows the union so the later `toBe("accelerated")` can't typecheck) → use `setUiMode("local")` instead of direct assignment. New scripts: playground `typecheck`, root `test:typecheck` now = sdk + playground src + playground scripts.
- Gate note: the "28 Playwright UI mock tests" figure in CLAUDE.md counts all Playwright specs; the **mocked project is 8 tests** — that's the layer this gate runs (8/8 passed).

## The real fight: kv-store sqlite-opfs vs the Vite dev server (3 attempts)

5.0.0's `@aztec/kv-store` sqlite-opfs backend (new browser default, pulled in via `@aztec/wallets`) broke the dev server twice-over. Production build was fine throughout — dev-only, but P3b's pre-publish smoke runs on the dev server, so it had to work.

1. **Attempt 1 — `resolve.alias` for `#msgpackr`/`#ordered-binary`: FAILED both ways.** The dep-optimizer scan ignores the alias (still `Could not resolve "#msgpackr"`), and the `#ordered-binary` alias to a deep package path broke the production rollup build (`dest/…` is not an exported subpath). Reverted.
2. **Attempt 2 — `optimizeDeps.exclude: ["@aztec/kv-store"]`: FAILED (whack-a-mole).** Exclusion raw-serves kv-store's whole transitive graph; every CJS dep breaks on named ESM imports in sequence — first `util` (`does not provide an export named 'inspect'`), then `pino`. Each fixable individually (e.g. adding `util` to nodePolyfills), but the shape is wrong: you'd be chasing the graph. Reverted.
3. **Attempt 3 — LANDED, two surgical pieces:** (a) an esbuild `onResolve` plugin under `optimizeDeps.esbuildOptions` mapping the two `#` specifiers to their browser-condition targets (Vite's optimizer can't resolve package-internal subpath imports — known limitation; production rollup resolves them natively so the plugin is dev-only by construction); (b) the existing `bbWorkerPlugin` middleware extended to also redirect kv-store's `worker.js` — same `new Worker(new URL(…, import.meta.url))`-in-a-prebundled-dep disease bb.js already had. Matching switched from substring to **exact basename** because `"worker.js"` is a substring of `"main.worker.js"`.

After (3): dev server boots, wallet init completes (the "SQLite worker crashed: undefined" ×3 = `MAX_INIT_ATTEMPTS` exhausting against the missing worker file — gone), only expected accelerator-health `ERR_CONNECTION_REFUSED` remains in console. `@aztec/sqlite3mc-wasm` itself is bundler-aware (static `new URL('../vendor/jswasm/sqlite3.wasm', import.meta.url)` — their comment says it exists precisely for bundlers), so no wasm asset handling was needed.

## Probe-tooling gotchas (not product bugs; logged to save future-me an hour)

- The Bash tool's cwd drifts between calls; a probe run from the worktree ROOT made `bunx vite` fall back to a **fetched vite 8.0.1** (the version this repo has blocked!) serving the wrong directory — looked like a regression, was scaffolding. Always spawn the workspace's own `node_modules/.bin/vite` with an explicit `--port`, from the right cwd, with absolute paths.
- Bare `playwright` isn't installed — import `chromium` from `@playwright/test`; and a probe script in `/tmp` can't resolve workspace bare specifiers (run it from inside the package).

LESSONS_FILE=implementations-plan/aztec-5.0.0-stable-2026-07-13/lessons/phase-1.md
