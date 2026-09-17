# Lessons

Gotchas that already bit us and would bite again on unrelated work. **Read before starting a task.**

One line each, promoted when a plan closes. Budget ~8 KiB, so a promotion is also a pruning pass: deduplicate, retire
what your entry supersedes, date-stamp anything tied to a tool version. A stale workaround here is worse than no file.
Sections are stable: append inside one, never re-sort. _Seeded 2026-09-17 from the 28 plans archived that day._

## Aztec upgrades

- **Every `@aztec` bump needs a hand-pinned Windows bb SHA** — the bump workflow only touches package.jsons and the
  lockfile; `WINDOWS_BB_CHECKSUMS` in `copy-bb.ts` must be edited by hand or the Windows build fails closed with no
  fallback. It has bitten every bump so far. `archive/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-2.md`
- **Every `@aztec` bump moves the SponsoredFPC address** — it derives from the artifact bytecode hash, so it must be
  rederived, redeployed and refunded, not just re-registered. `archive/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-3.md`
- **Aztec node health needs JSON-RPC, not REST** — `GET /status` returns 405 on a 5.0 node. Only the `node_getNodeInfo`
  POST is a reliable reachability check; the REST form silently broke the live playground.
  `archive/aztec-5.0.0-stable-2026-07-13/lessons/phase-3.md`
- **`DeployAccountMethod` with `from` set breaks note discovery** — passing a funded signer instead of `NO_FROM` tags the
  new account's constructor notes to the signer, so it cannot find its own notes.
  `archive/aztec-5.0.0-2026-06-18/lessons/phase-3.md`

## Build, bundling and dependencies

- **Bun's isolated linker exposes undeclared transitive deps** — hoisting hid them, 1.4 does not; it surfaced in five
  packages at once. A worktree nested in this clone still walks up into the root's hoisted `node_modules`, masking the
  bug while you reproduce it. `archive/bun-1-4-migration/lessons/phase-3.md`
- **Vite's dev optimizer breaks any dep that spawns a worker** — anything doing `new Worker(new URL(...))` internally.
  `resolve.alias` and `optimizeDeps.exclude` are both dead ends; the fix is an exact-basename worker redirect plus an
  esbuild `onResolve` shim. `archive/aztec-5.0.0-stable-2026-07-13/lessons/phase-1.md`
- **`Arc::make_mut` on a clone diverges from shared state** — the mutation landed only on that clone, so readers such as
  `/health` never saw it. Use a shared `Arc<AtomicBool>`. `archive/quality-refactor-2026-06-05/lessons/phase-7.md`
- **`/prove` errors must be `text/plain`** — the bodies are JSON-shaped, but the SDK's `ky` client keys `HTTPError.data`
  parsing on Content-Type, so `axum::Json` would silently break the SDK.
  `archive/quality-refactor-2026-06-05/lessons/phase-0.md`
- **`ky` is not a drop-in for `fetch`** — without `{retry: 0, throwHttpErrors: false}` it retries GETs three times and
  throws on non-2xx, which flipped a real 500 into a mis-mapped "offline" state.
  `archive/quality-fixes-2026-06-08/lessons/phase-4.md`

## Tests

- **`bun test src/` from the repo root skips the bunfig preload** — test-setup and the equality-tester patch never load,
  giving ~28 spurious failures. Run scoped per package. `archive/quality-fixes-2026-06-10/lessons/phase-1.md`
- **`cargo test --lib` never compiles the bin crate** — errors that only exist across the lib/bin boundary pass silently.
  `cargo check --bin` catches them. `archive/quality-refactor-2026-06-05/lessons/phase-1.md`
- **Bun 1.4 hands fetch mocks a `Request`, not a URL string** — handlers written against the 1.3 string form match
  nothing and the test times out instead of failing. `archive/bun-1-4-migration/lessons/phase-5.md`
- **A dev build's update check hijacks WebDriver** — the check is ungated even in dev and webdriver builds. Once the dev
  version is at or below the latest release it pops a window, steals focus, and fails later specs.
  `archive/ci-reliability-2026-05-29/diagnosis.md`
- **Playwright browser installs are a CI trap both ways** — its CDN can serve the full ~150 MB Chrome for Testing under
  250 KB/s and blow a 600s timeout, so use `--only-shell chromium`; and its "does not support chromium on <os>" refusal
  is not a dead end, since a cached revision may still be on disk and `executablePath` will run it.
  `archive/updater-validation-2026-05-29/lessons/phase-afk.md`

## CI and release

- **Stacked PR branches get zero CI** — `pull_request`-filtered workflows never fire off main, so a green stack branch
  can hide a red gate. Run every equivalent gate by hand. `archive/bun-1-4-migration/lessons/phase-5.md`
- **The release flatten step empties `artifacts/`** — it moves binaries into `release-files/` before `latest.json` is
  generated, so anything reading sizes or hashes there sees only `.sig` files. Release-candidate dry runs never catch it,
  because that step is gated on `is_prerelease == 'false'`. `archive/release-1.0.5-2026-06-11/lessons.md`
- **A new release-producing workflow steals the "Latest" badge** — even a metadata-only GitHub release is returned by
  `gh release list --exclude-pre-releases --limit 1`. Filter by tag prefix, never recency.
  `archive/release-1.0.6-2026-06-11/lessons.md`
- **`upload-artifact` keeps the glob's common ancestor** — a multi-platform glob preserves the shared parent instead of
  flattening, so a flat `cp *.ext` after download matches nothing. Use a recursive `find`.
  `archive/windows-ci-parity-2026-06-03/lessons/phase-3.md`
- **actionlint rejects `continue-on-error` on a reusable-workflow caller** — put it on a step inside the called workflow
  and surface the failure through the step summary. `archive/ci-release-overhaul-2026-06-01/lessons/phase-a2b.md`
- **An empty `secrets:` mapping is invalid YAML** — removing a reusable workflow's last secret means deleting the whole
  mapping, in the declaration and every caller, in one commit.
  `archive/fpc-salt-removal-docs-2026-06-18/lessons/phase-2.md`
- **Pushing under `.github/workflows/` needs the `workflow` OAuth scope** — not granted by default; run
  `gh auth refresh -s workflow` first. `archive/https-by-default-onboarding-2026-07-09/lessons/phase-3.md`
- **Foundry ≥1.7 breaks aztec's forge wrapper** — the wrapper omits `--batch`, which 1.7 requires alongside
  `--batch-size`. Pin `foundry-toolchain` *and* move aztec's bundled forge aside so it cannot shadow the pin on PATH.
  `archive/release-1.0.6-2026-06-11/lessons.md`

## Platform, signing and trust

- **Apple's 825-day certificate ceiling applies to user-trusted certs too** — any locally generated leaf must stay under
  it or macOS silently refuses it. `archive/safari-tls-ca-removal-2026-06-04/lessons/phase-1.md`
- **Intel DMG bundling flakes on GitHub's runner** — `hdiutil` and `create-dmg` fail randomly on `macos-15-intel`, a
  runner-images issue rather than a Tauri one. Notarize the `.app` once, then wrap the DMG in its own retried step.
  `archive/updater-validation-2026-05-29/lessons/phase-decouple-harden.md`
- **The updater's progress callback cannot abort a download** — `tauri-plugin-updater` buffers regardless, so the only
  defence against an oversized artifact is checking the feed's advertised `size` first.
  `archive/security-hardening-2026-06-09/lessons/phase-4.md`

## Local tooling

- **1Password signing failures and SSH transport failures look alike and are not** — a dying signing agent ("failed to
  fill whole buffer") wants `-c commit.gpgsign=false`; a dropped GitHub SSH connection wants a push over HTTPS through
  `gh auth git-credential`. `archive/quality-refactor-2026-06-05/lessons/phase-4.md`
- **`git fetch <url> <branch>` never updates the tracking ref** — it writes `FETCH_HEAD` only, so
  `refs/remotes/origin/<branch>` can look stale when it is not. Cross-check with `git ls-remote`.
  `archive/fpc-salt-removal-docs-2026-06-18/lessons/phase-4.md`
- **`pkill -f` can match the script that runs it** — a cleanup pattern matched the invoking script's own argv and killed
  it mid-run. Anchor to the binary name, never a substring the launcher contains.
  `archive/ci-release-overhaul-2026-06-01/lessons/phase-a2b.md`
