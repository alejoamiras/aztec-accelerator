# Maintenance — 2026-05-27 (revised after codex review)

Light plan, three tracks. Codex review complete; plan revised against findings (see `Revisions from codex review` at the bottom).

## Tracks → PRs

- **PR 1** — Track A1 (npm bumps + cargo patches) + Track B (delete `dependabot.yml`).
- **PR 2** — Track A2 (Tauri 2.10→2.11 alone — its own rollback boundary).
- **PR 3** — Track C (headless tarballs as release artifacts + CI-only docs).

Close `#217 (vite 8)` separately with comment referencing `project_vite8_blocked.md`.

---

## PR 1 — Track A1 + Track B

### A1 scope

**npm root devDeps:**
- `lint-staged` 16.4.0 → 17.0.5
- `@commitlint/cli` 20.5.3 → 21.0.1
- `@commitlint/config-conventional` 20.5.3 → 21.0.1
- `vite-plugin-node-polyfills` 0.26.0 → 0.28.0 (group)

**cargo (`packages/accelerator/src-tauri/Cargo.toml`):**
- `tauri-build` 2.5.6 → 2.6.0
- `axum` 0.8.8 → 0.8.9
- `tracing-appender` 0.2.4 → 0.2.5
- `tokio` 1.51.1 → 1.52.1

`tauri` 2.11 deferred to PR 2.

### B scope

Delete `.github/dependabot.yml` entirely. User has accepted the trade-off (no automated CVE PRs; manual periodic maintenance). The repo-level Dependabot toggles are also currently OFF (`vulnerability-alerts` → 404, `automated-security-fixes` → `{enabled:false}`); we are not enabling them. Documenting this explicitly in the PR body so it's not a silent regression.

### Steps

1. Branch `chore/bump-deps-and-drop-dependabot-2026-05-27`.
2. `bun add -D lint-staged@^17 @commitlint/cli@^21 @commitlint/config-conventional@^21 vite-plugin-node-polyfills@^0.28` (root; check workspaces if any leak).
3. Edit `packages/accelerator/src-tauri/Cargo.toml` for the four cargo pins; `cargo update -p tauri-build -p axum -p tracing-appender -p tokio` to refresh `Cargo.lock`.
4. Delete `.github/dependabot.yml`.
5. **Validate locally:**
   - `bun run lint` (biome + pkg + cargo fmt)
   - `bun run test` (typecheck + unit)
   - `bun run lint:actions`
   - `cd packages/accelerator/src-tauri && cargo check --all-targets --features webdriver`
   - `cd packages/accelerator/src-tauri && cargo build --bin accelerator-server` then start it, `curl http://127.0.0.1:59833/health`, expect `200 + status:"ok"`.
   - `git stash -- some-file && bunx lint-staged` smoke to confirm commitlint 21 + lint-staged 17 still pass on the hooks.
6. Push PR. CI gates: `accelerator.yml`, `sdk.yml`, `app.yml`, `actionlint.yml`.
7. After merge: comment-and-close the superseded dependabot PRs (#216, #215, #207, #204, #189, #181, #179, #178). Close #217 separately citing `project_vite8_blocked.md`. Close #188 (tauri 2.11) saying "superseded by PR 2".

### A1 risks

- `commitlint 21` / `lint-staged 17` majors — config-shape regressions possible. Local smoke at step 5 + husky-driven CI hook execution on the PR itself catches it.
- `cargo` patch/minor (excluding tauri) — tiny surface, low risk.

### B risks (user-accepted)

- **No CVE PR pipeline.** Both `bun audit` (npm-only, doesn't open PRs) and manual review are the residual mitigations. User explicitly accepted.
- GitHub's Security tab still surfaces Dependabot alerts independently of the config file, but only if the repo-level toggle is on — it currently isn't. Document this in the PR body.

### Adversarial check

- **Supply-chain freshness:** no min-age gate exists in the repo (`bunfig.toml` does not exist). The `--frozen-lockfile` check is integrity, not freshness. Accept residual risk on each bump. If we want freshness gating later, add a `bunfig.toml` with `[install] minimumReleaseAge = 604800` as a follow-up.
- **Cargo deps:** `cargo audit` is not in CI today. Out of scope for this PR; track as a follow-up issue.
- **Hook bypass via commitlint/lint-staged majors:** step 5 smoke catches obvious breakage; CI runs hooks on every commit.

---

## PR 2 — Track A2 (Tauri 2.11)

### Scope

`tauri` 2.10.3 → 2.11.0 in `packages/accelerator/src-tauri/Cargo.toml`.

### Steps

1. Branch `chore/bump-tauri-2.11`.
2. Edit `Cargo.toml`, `cargo update -p tauri`.
3. Validate locally:
   - `bun run test`, `bun run lint`
   - `cargo check --all-targets --features webdriver`
   - `cargo build --bin accelerator-server` + `/health` curl
   - **Most important: WebDriver E2E gates this PR.** `accelerator.yml` PR gate runs the WebDriver suite — that's the real check that tray, popup, and Tauri runtime survived the bump.
4. PR description links to Tauri 2.11 release notes and our WebDriver test count.

### Risks

- Tauri minor bumps include runtime / wry / tray-icon updates, not just API surface. Tray-icon and updater-plugin breakages are the historical landmines.
- WebDriver E2E + post-build DMG smoke on the eventual release are our safety nets.

### Adversarial check

- Tauri runtime is a privileged surface — any regression in IPC handling or origin enforcement would be exploitable. WebDriver tests do not cover that surface directly; mitigation is reading the changelog before approving and trusting our cautious pin-to-minor policy.

---

## PR 3 — Track C (headless tarballs as release artifacts)

### Goal — explicitly bounded

Ship cross-compiled `accelerator-server` tarballs as GitHub release assets so external Aztec dApp CI can install and run the accelerator to speed up E2E proving tests. **Strictly for CI test acceleration. Not production. Not for shared/self-hosted runners.** Doc language must enforce this.

### Why this scope (and not the bigger one)

- The headless server's "auth" (`ALLOWED_ORIGINS`) only gates browser callers (`server.rs:218-222` auto-approves any request without an `Origin` header). On a single-tenant CI runner (GitHub-hosted) this is fine — the localhost-only binding IS the boundary. On a shared/self-hosted runner, any other process can hit `/prove` regardless of `ALLOWED_ORIGINS`.
- We are NOT adding bearer-token auth in this PR. We are NOT making the server's port configurable in this PR (README claim at L62 is incorrect today; PR 3 documents the actual behavior — fixing the README lie about `AZTEC_ACCELERATOR_PORT` is a freebie one-line edit, but no code change to the server).
- We are NOT signing/attesting the tarballs in this PR. SHA-256 sidecars only.
- These limits are why the docs must be very explicit about intended use.

### Scope

1. **Build matrix in `release-accelerator.yml`** — a new `build-headless` job, parallel to existing `build` (Tauri), gating the existing `release` job:

    | Target triple | Runner | Tarball |
    |---|---|---|
    | `aarch64-apple-darwin` | `macos-latest` | `accelerator-server-${VERSION}-macos-arm64.tar.gz` |
    | `x86_64-apple-darwin` | `macos-15-intel` | `accelerator-server-${VERSION}-macos-x86_64.tar.gz` |
    | `x86_64-unknown-linux-gnu` | `ubuntu-latest` | `accelerator-server-${VERSION}-linux-x86_64.tar.gz` |
    | `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | `accelerator-server-${VERSION}-linux-arm64.tar.gz` |

    Per-step:
    - `actions/checkout@v6`
    - `dtolnay/rust-toolchain@stable` with target
    - `Swatinem/rust-cache@v2` keyed `headless-${target}` (separate from Tauri caches)
    - **Linux only**: `sudo apt-get update && sudo apt-get install -y libssl-dev` (required because `reqwest` in `Cargo.toml:33` keeps default features and pulls native-tls per `Cargo.lock`). macOS uses Security framework — no apt step.
    - Patch version: same `Cargo.toml` `sed` the Tauri build does.
    - `cargo build --release --bin accelerator-server --target ${matrix.target}`
    - Locate binary at `target/${matrix.target}/release/accelerator-server`
    - `tar -czf accelerator-server-${VERSION}-${platform}.tar.gz accelerator-server`
    - Checksum portably: `shasum -a 256 accelerator-server-${VERSION}-${platform}.tar.gz > accelerator-server-${VERSION}-${platform}.tar.gz.sha256` (`sha256sum` is not on macOS; `shasum -a 256` is on both).
    - `actions/upload-artifact@v7` named `accelerator-server-${platform}`.

2. **`release` job updates:**
    - Add `needs: [validate, build, build-headless, smoke]` (currently `[validate, build, smoke]`).
    - Download new artifacts in the existing `actions/download-artifact` step.
    - In the flatten step, move tarballs + sha256 sidecars into `release-files/`.
    - Add 8 new entries to `EXPECTED` (4 tarballs + 4 sidecars). Fail-fast on missing.
    - Extend the `gh release create` file glob to include `*.tar.gz` and `*.tar.gz.sha256` (careful: the Tauri updater also produces `*.app.tar.gz` — the existing find already matches it; the new headless tarballs are named `accelerator-server-*.tar.gz` so they coexist without collision).
    - Extend `/tmp/release-notes.md` with a new section (see below).

3. **NO `latest.json` update.** The auto-updater is GUI-only.

4. **Release-notes addendum** (CI-only language):

    ```markdown
    ### Headless server for CI test acceleration

    > **For CI test acceleration only.** Not production. Listens on `127.0.0.1`
    > only. Tested only on GitHub-hosted (single-tenant) runners. The built-in
    > `ALLOWED_ORIGINS` access control only gates browser-driven callers; on
    > shared / self-hosted runners, other processes on the same host can bypass
    > it. Do not deploy this binary as a service.

    | Platform | File |
    |---|---|
    | macOS (Apple Silicon) | `accelerator-server-${VERSION}-macos-arm64.tar.gz` |
    | macOS (Intel)         | `accelerator-server-${VERSION}-macos-x86_64.tar.gz` |
    | Linux (x86_64)        | `accelerator-server-${VERSION}-linux-x86_64.tar.gz` |
    | Linux (ARM64)         | `accelerator-server-${VERSION}-linux-arm64.tar.gz` |

    Each tarball ships with a `.sha256` sidecar. Verify before extracting.

    Install example for a GitHub-hosted Linux runner:

    ```yaml
    - name: Install aztec-accelerator headless server
      run: |
        VERSION=${VERSION}
        curl -sSfL "https://github.com/alejoamiras/aztec-accelerator/releases/download/accelerator-v${VERSION}/accelerator-server-${VERSION}-linux-x86_64.tar.gz" -o server.tar.gz
        curl -sSfL "https://github.com/alejoamiras/aztec-accelerator/releases/download/accelerator-v${VERSION}/accelerator-server-${VERSION}-linux-x86_64.tar.gz.sha256" -o server.tar.gz.sha256
        sha256sum -c server.tar.gz.sha256
        tar -xzf server.tar.gz
        sudo mv accelerator-server /usr/local/bin/
    - name: Start headless accelerator
      run: accelerator-server > accelerator.log 2>&1 &
      env:
        ALLOWED_ORIGINS: http://localhost:5173
    ```
    ```

5. **README updates** in `packages/accelerator/README.md`:
    - Add a "Headless Server for CI Test Acceleration" section mirroring the release-notes language. Lead with the warning, then install, then config.
    - **Fix the existing lie at L62** about `AZTEC_ACCELERATOR_PORT` — current text claims it works on both sides; only the SDK honors it. Change to: "The default port is `59833`. The SDK reads `AZTEC_ACCELERATOR_PORT` to override the client-side target; the server itself does NOT currently honor this and always binds 59833. If you need to change the port, this is tracked as a follow-up."
    - No code change to `server.rs` in this PR.

6. **Pre-merge validation** — Track C edits a workflow that's `workflow_dispatch`-only and tags before building. To validate before merge:
    - Add a `build-headless-smoke` job to `accelerator.yml` (PR gate). Cross-compile only `x86_64-unknown-linux-gnu` (the cheapest, on the host runner) using the exact same `cargo build --release --bin accelerator-server` command + tar + shasum. Don't upload, just confirm the build path works. **This is the pre-merge smoke.**
    - First post-merge `workflow_dispatch` trigger should be against an `-rc.X` tag to validate end-to-end before any stable release.

### What we are NOT doing in PR 3

- No bearer-token auth.
- No `AZTEC_ACCELERATOR_PORT` plumbing into the server.
- No `reqwest` → rustls-only switch.
- No artifact attestation / Sigstore / minisign.
- No Windows target.
- No npm wrapper package.
- No headless-binary code signing (macOS Gatekeeper warnings on a raw binary are acceptable for CI-only use).

Track these as follow-up issues if there's demand.

### PR 3 risks

- Cross-compile flakiness on `ubuntu-24.04-arm` — first time we use this runner. The PR-gate smoke (linux-x86_64 only) doesn't exercise the ARM64 path; we'll only learn at release time. Acceptable because: (a) we can manually trigger a throwaway `-rc` release to validate, (b) if ARM64 fails, x86_64 still ships.
- Release-tagging happens before headless build (existing workflow order at `release-accelerator.yml:49-68`). If `build-headless` fails, the tag is already pushed. Same failure mode as the existing Tauri `build` job — known limitation. Re-running the workflow is idempotent on the tag.
- `release` job's `gh release create` file glob: making sure `accelerator-server-*.tar.gz` doesn't collide with `*.app.tar.gz` (Tauri updater). The current `find` uses `-name '*.app.tar.gz'` for updater files; adding `-name 'accelerator-server-*.tar.gz'` separately avoids collision. Verify during implementation.

### Adversarial check — being honest

- **Release-asset tampering.** SHA256 sidecars are forgeable by anyone who can write to the GitHub release (us, or an attacker with write access). Not a defensive layer against repo compromise. Mitigation: explicit warning in docs + follow-up issue for `actions/attest-build-provenance`. **Accepted limitation.**
- **Bypass of `ALLOWED_ORIGINS`.** Documented openly in the warning block. **Accepted limitation for CI-only use.**
- **`bb` download trust loop.** `versions.rs:287-310` verifies SHA-256 of downloaded bb against the GitHub API's `digest` metadata field — but both come from the same GitHub source. Pre-existing limitation; `versions.rs:205` has a `TODO` for upstream signatures. Not Track C's problem.
- **Port hardcoded → collision on multi-job CI hosts.** Documented. If a consumer has two jobs hitting the same host, only one will succeed. Accepted for CI-only single-tenant runners.
- **Workflow permission bloat.** New `build-headless` job needs `contents: read` at most. Inherits from workflow-level (`id-token: write, contents: write, pull-requests: write`) — does not need writes. Add a job-level `permissions:` override scoping it down: defensive least-privilege.

---

## Sequencing

1. Land **PR 1** (Track A1 + Track B). Validate via CI gates.
2. Land **PR 2** (Track A2 — Tauri 2.11). WebDriver E2E is the gate.
3. Land **PR 3** (Track C). Trigger a throwaway `-rc.X` release after merge to validate the headless build matrix end-to-end before any stable.

After PR 3 lands, close the open dependabot Tauri PRs that point to obsolete versions (#188 if not already closed in PR 2 cleanup).

## Out of scope (explicit)

- `@aztec/*` bumps — handled by `_aztec-update.yml`.
- Vite 8 retry — blocked per memory.
- npm wrapper package for the headless binary.
- Code signing / attestation for the headless tarball.
- Server-side `AZTEC_ACCELERATOR_PORT` support.
- Bearer-token auth on `/prove`.
- `cargo audit` in CI.
- Windows headless build.

---

## Revisions from codex review (vs. v1 of this plan)

- ❌ Dropped fabricated `bunfig.toml` 7-day-min-age claim — file doesn't exist.
- ❌ Dropped "bun audit covers CVEs" claim for Track B — `bun audit` doesn't open PRs and doesn't cover cargo/actions. User has explicitly accepted no automated CVE coverage.
- ✅ Split Tauri 2.11 into its own PR (own rollback boundary).
- ✅ Track C scope narrowed from "release binary + npm wrapper + maybe Docker" to "release tarballs only, CI test-acceleration only, no code changes." Explicit warnings in release notes + README replace the misleading auth claims.
- ✅ Acknowledged hardcoded port + Origin-bypass openly in docs instead of glossing.
- ✅ Fixed binary name (`accelerator-server`, not `aztec-accelerator-server`).
- ✅ Switched `sha256sum` → `shasum -a 256` for macOS portability.
- ✅ Added linux libssl apt step (because `reqwest` pulls native-tls per Cargo.lock).
- ✅ Added pre-merge smoke build to `accelerator.yml` PR gate (linux-x86_64 cross-compile) so PR 3 isn't merging blind against a `workflow_dispatch`-only release pipeline.
- ✅ Added job-level `permissions: contents: read` to the new `build-headless` job (defensive least-privilege).
- ✅ Will fix the existing lie in `packages/accelerator/README.md:62` about `AZTEC_ACCELERATOR_PORT` as part of PR 3's docs work.
