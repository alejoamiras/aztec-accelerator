# Lessons

Gotchas that already bit us and would bite again on unrelated work. **Read before starting a task.**

One line each, promoted when a plan closes. Budget ~8 KiB: every promotion is also a pruning pass — deduplicate, merge,
retire what it supersedes. A stale entry is worse than none. Sections are stable; append, never re-sort.
_Seeded 2026-09-17 from 43 plans._

## Aztec upgrades

- **Every `@aztec` bump needs a hand-pinned Windows bb SHA** — the bump workflow touches only package.jsons and the
  lockfile, so `WINDOWS_BB_CHECKSUMS` in `copy-bb.ts` must be edited by hand or the Windows build fails closed. It has
  bitten every bump. `archive/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-2.md`
- **Every `@aztec` bump moves the SponsoredFPC address** — it derives from the artifact bytecode hash, so it must be
  rederived, redeployed and refunded, not just re-registered. `archive/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-3.md`
- **Aztec node health needs JSON-RPC, not REST** — `GET /status` returns 405 on a 5.0 node; only the `node_getNodeInfo`
  POST is reliable. The REST form silently broke the live playground.
  `archive/aztec-5.0.0-stable-2026-07-13/lessons/phase-3.md`
- **`DeployAccountMethod` with `from` set breaks note discovery** — a funded signer instead of `NO_FROM` tags the new
  account's constructor notes to the signer. `archive/aztec-5.0.0-2026-06-18/lessons/phase-3.md`

## Build, bundling and dependencies

- **Bun's isolated linker exposes undeclared transitive deps** — hoisting hid them, 1.4 does not; five packages at once.
  A worktree nested in this clone still walks up into the root's hoisted `node_modules`, masking the bug as you
  reproduce it. `archive/bun-1-4-migration/lessons/phase-3.md`
- **npm 12's `allowScripts` default blocks native postinstall builds** — the failure surfaces at runtime as a missing
  `.node` binding, not at install, and a probe using a prebuilt-binding package misses it.
  `archive/aztec-5.2.0-2026-08-18/lessons/phase-3.md`
- **Vite's dev optimizer breaks any dep that spawns a worker** — anything doing `new Worker(new URL(...))` internally.
  `resolve.alias` and `optimizeDeps.exclude` are both dead ends; the fix is an exact-basename worker redirect plus an
  esbuild `onResolve` shim. `archive/aztec-5.0.0-stable-2026-07-13/lessons/phase-1.md`
- **`/prove` errors must be `text/plain`** — the bodies are JSON-shaped, but `ky` keys `HTTPError.data` parsing on
  Content-Type, so `axum::Json` silently breaks the SDK. A test mocking it with `Response.json()` passed while the real
  branch was dead code. `archive/quality-refactor-2026-06-05/lessons/phase-0.md`
- **`ky` is not a drop-in for `fetch`** — without `{retry: 0, throwHttpErrors: false}` it retries GETs three times and
  throws on non-2xx, which flipped a real 500 into a mis-mapped "offline" state.
  `archive/quality-fixes-2026-06-08/lessons/phase-4.md`

## Tests

- **The Rust lib/bin split hides three error classes from `--lib` checks** — `pub(crate)` items are invisible to
  `main.rs` (E0603), a `Send` bound is only enforced at the downstream `spawn`, and `cargo test --lib` never compiles the
  bin. Behind `#[cfg(target_os = "windows")]` local checks stay green while every Windows CI leg fails; `cargo check
  --bin` catches them. `archive/autostart-update-marker/lessons/phase-1.md`, `archive/v2-release-train/lessons/b3.md`
- **`bun test src/` from the repo root skips the bunfig preload** — test-setup and the equality-tester patch never load,
  giving ~28 spurious failures. Run scoped per package. `archive/quality-fixes-2026-06-10/lessons/phase-1.md`
- **A dev build's update check hijacks WebDriver** — the check is ungated even in dev and webdriver builds. Once the dev
  version is at or below the latest release it pops a window, steals focus, and fails later specs.
  `archive/ci-reliability-2026-05-29/diagnosis.md`
- **Playwright installs trap CI both ways** — its CDN can serve ~150 MB at under 250 KB/s and blow a 600s timeout, so
  use `--only-shell chromium`; and "does not support chromium on <os>" is not a dead end, since a cached revision may
  still be on disk for `executablePath`. `archive/updater-validation-2026-05-29/lessons/phase-afk.md`

## CI and release

- **GitHub Actions inputs are always strings** — a boolean-looking `inputs.foo` in a composite action or
  `workflow_call` is a string, so bare `if: inputs.foo` is always truthy. Compare against `'false'`.
  `archive/headless-ci-slim-2026-06-08/lessons/phase-2.md`
- **Stacked PR branches get zero CI** — `pull_request`-filtered workflows never fire off main, so a green stack branch
  can hide a red gate. Run every equivalent gate by hand. `archive/bun-1-4-migration/lessons/phase-5.md`
- **The release flatten step empties `artifacts/`** — binaries move to `release-files/` before `latest.json` is built,
  so anything reading sizes or hashes there sees only `.sig` files. RC dry runs never catch it: that step is gated on
  `is_prerelease == 'false'`. `archive/release-1.0.5-2026-06-11/lessons.md`
- **npm answers 404, not 403, when a publish is unauthorized** — it will not confirm a scoped package exists to an
  unauthorized caller, so "not found" means an expired or rotated token, not a naming bug.
  `archive/sdk-5-2-0-release/lessons/phase-2.md`
- **`npm dist-tag add` reads back stale** — the write lands, yet `npm view` can report the old tag a minute later.
  Trust the mutation's own output, and use `--prefer-online` after a promote or the install resolves the previous version
  from cache. `archive/sdk-5-2-0-release/lessons/phase-3.md`
- **A new release-producing workflow steals the "Latest" badge** — even a metadata-only GitHub release is returned by
  `gh release list --exclude-pre-releases --limit 1`. Filter by tag prefix, never recency.
  `archive/release-1.0.6-2026-06-11/lessons.md`
- **Foundry ≥1.7 breaks aztec's forge wrapper** — it omits `--batch`, which 1.7 requires with `--batch-size`. Pin
  `foundry-toolchain` *and* move aztec's bundled forge aside so it cannot shadow the pin.
  `archive/release-1.0.6-2026-06-11/lessons.md`

## Platform, packaging and trust

- **Tauri's `bundle.licenseFile` is global, not per-platform** — on macOS it makes the bundler embed a UDIF EULA, so
  every `hdiutil attach` blocks on interactive acceptance, CI included. Ship license text via `bundle.resources` when
  only Windows needs it. `archive/rc-dry-run/lessons/phase-1.md`
- **`mainBinaryName` only renames on tauri-driven builds** — any path running plain `cargo build`, such as WebDriver E2E,
  still emits the old name. Rename at the Cargo `[[bin]]`/`default-run` layer so every build path agrees.
  `archive/binary-rename/lessons/phase-1.md`
- **`bundle.publisher` namespaces the NSIS upgrade registry key** — it feeds `${MANUFACTURER}`, which keys the entry
  restoring a custom `$INSTDIR`. Changing it alongside install-path changes strands custom-directory installs with a
  live old exe. `archive/binary-rename/lessons/phase-1.md`
- **Apple's 825-day certificate ceiling applies to user-trusted certs too** — any locally generated leaf must stay under
  it or macOS silently refuses it. `archive/safari-tls-ca-removal-2026-06-04/lessons/phase-1.md`
- **Intel DMG bundling flakes on GitHub's runner** — `hdiutil` and `create-dmg` fail randomly on `macos-15-intel`, a
  runner-images issue, not a Tauri one. Notarize the `.app` once, then retry the DMG in its own step.
  `archive/updater-validation-2026-05-29/lessons/phase-decouple-harden.md`
- **The updater's progress callback cannot abort a download** — `tauri-plugin-updater` buffers regardless, so the only
  defence against an oversized artifact is checking the feed's advertised `size` first.
  `archive/security-hardening-2026-06-09/lessons/phase-4.md`

## Local tooling

- **1Password signing failures and SSH transport failures look alike and are not** — a dying signing agent ("failed to
  fill whole buffer") wants `-c commit.gpgsign=false`; a dropped GitHub SSH connection wants a push over HTTPS through
  `gh auth git-credential`. `archive/quality-refactor-2026-06-05/lessons/phase-4.md`