# CI Dedup — extract shared release-build setup into `setup-accelerator`

**Status**: APPROVED v2.2 (user chose to implement first); Phase 9 hardening tracked as separate follow-up PR.
**Date**: 2026-05-28
**Type**: Tier B (contained refactor + observable dry-run validation)
**Triggered by**: codex post-impl follow-up #1 from the release-2026-05-27 work — PR #228 fixed a real drift bug where `build-headless` had its own minimal setup that fell behind the `setup-accelerator` composite used by PR-gate.

## v2 changes from v1

Major revisions after dual audit (codex + opus subagent):

| What | Why | Source |
|------|-----|--------|
| **Keep Rust-first / cache-first ordering** | Swatinem/rust-cache derives the auto cache key from `Cargo.toml` + `Cargo.lock`. Patching `Cargo.toml` before `rust-cache` runs would make every release version a unique cache key → cold caches every release. | codex (must-fix) |
| **Move version-patching steps inline in the callers, NOT in the composite** | Original v1 design put version-patching inside the composite, which (a) bundles environment+source-mutation into wrong abstraction, and (b) creates an invalid 2-input state space (`cargo-version` + `tauri-conf-version`). Cleanest fix: composite stays env-only; callers do their own patching after the composite. | codex (should-fix design) |
| **Add `_e2e-webdriver.yml` to scope** | It has its own duplicated `rust-toolchain` + `rust-cache` + `bun setup` + `apt` + `prebuild` (verified). If drift prevention is the goal, this is the other drift surface. | codex (must-fix plan) |
| **Fix factual error about release-smoke** | `release-smoke` runs on `ubuntu-latest`, not `macos-latest`. The apt-guard fix becomes "defense-in-depth for future macOS callers" not "fixes existing silent failure". | opus + codex |
| **Add Phase 2 sanity check: `bun install` postinstall** | Reordering Rust later means a `bun install` postinstall invoking `cargo` would fail. Verified no current postinstall does — but add an explicit grep check before merging. | opus |
| **Tighten `build` job permissions to `contents: read`** | `build` doesn't push/write. Workflow-level `id-token: write`, `pull-requests: write`, `contents: write` is over-granted. Move them down to the specific steps/jobs that need them. | codex + opus |
| **Add SHA-pinning + CODEOWNERS as Phase 6** | Centralizing third-party actions inside the composite increases blast radius if a tag is retargeted. `dtolnay/rust-toolchain@stable` excepted (documented usage). | codex + opus |
| **Validation: do a `-rc` AND a stable dry-run** | Prerelease dry-run skips `latest.json` + S3 OIDC + `bump-source`. A stable release is needed to validate the full path. | codex |

## Goal

Eliminate the drift class that caused PR #228 by collapsing the inline environment setup steps in `release-accelerator.yml`'s `build` + `build-headless` jobs **and** `_e2e-webdriver.yml` into the existing `.github/actions/setup-accelerator/action.yml` composite. After this, **all** accelerator builds share one source of truth for: Linux apt deps, bun setup + cache + install, Rust toolchain + cache.

**Out of composite (kept in callers)**: version patching (so `rust-cache` can use Cargo manifest for auto-keying), the actual `tauri build` / `cargo build` invocation with its specific secrets, job-specific apt packages (e.g. xvfb for E2E).

**Success criteria**:
1. `release-accelerator.yml`'s `build` and `build-headless` jobs use `uses: ./.github/actions/setup-accelerator` for shared env setup; only version-patching + actual build remain inline.
2. `_e2e-webdriver.yml` uses the same composite + a job-specific apt install for xvfb/stalonetray/dbus.
3. Dry-run of `release-accelerator.yml` with a `1.0.2-rc.2` succeeds on all 7 platform combinations.
4. A subsequent **stable** `1.0.2` release validates `latest.json` + S3 OIDC + `bump-source` path.
5. PR-gate workflows (`accelerator.yml`) still pass.
6. Released artifacts byte-identical in structure (file names, contents) to the last release.

## Non-goals

- Restructuring matrix shapes.
- Replacing `dtolnay/rust-toolchain@stable` with a tag-pinned action (`@stable` is the documented usage for this action — it's a branch selector for the toolchain channel, not a version tag).
- Refactoring `release-smoke` (already uses the composite).
- Touching `validate`, `tag`, `release`, `bump-source`, or `smoke` jobs.
- Changing `tauri.conf.json` schema or signing scheme.

## Current state (verified by reading source)

### `.github/actions/setup-accelerator/action.yml` (45 lines, no inputs)

```yaml
name: Setup Accelerator
runs:
  using: composite
  steps:
    - Install Linux deps                       # ← UNCONDITIONAL apt
    - oven-sh/setup-bun@v2
    - actions/cache@v5 (~/.bun/install/cache)
    - bun install --frozen-lockfile
    - Copy bb sidecar (prebuild)
    - dtolnay/rust-toolchain@stable             # ← with clippy, rustfmt
    - Swatinem/rust-cache@v2 (no key suffix)
```

### Composite callers (verified)

All current consumers are in `.github/workflows/accelerator.yml`:
- `clippy` (line 11, ubuntu-latest)
- `test` (line 42, ubuntu-latest)
- `lint` (line 57, ubuntu-latest)
- `smoke` (line 71, ubuntu-latest)
- `release-smoke` (line 139, **ubuntu-latest** — codex caught me listing this wrong)
- `e2e` (line 162, ubuntu-latest)

**No current macOS caller** of the composite. The unconditional apt install in the composite has never run on macOS to date. The `runner.os == 'Linux'` guard is still added as defense in depth (for future macOS callers including the `build` job we're about to refactor).

### `release-accelerator.yml` `build` job (lines 77–125)

Inline steps:
1. `actions/checkout@v6`
2. `dtolnay/rust-toolchain@stable` with `targets: ${{ matrix.target }}`
3. **`Swatinem/rust-cache@v2` with `key: release-${{ matrix.target }}`** ← cache key derives from `Cargo.toml` content as-of-this-step
4. `oven-sh/setup-bun@v2`
5. `actions/cache@v5` for `~/.bun/install/cache`
6. `bun install --frozen-lockfile`
7. Linux apt install (gated on `matrix.platform == 'linux-x86_64'`)
8. **Patch `tauri.conf.json` version** ← after rust-cache
9. **Patch `Cargo.toml` version** ← after rust-cache
10. `bun run --cwd packages/accelerator prebuild` (calls `bun scripts/copy-bb.ts` — verified at `packages/accelerator/scripts/copy-bb.ts` — copies bb binary, writes `AZTEC_VERSION` file, does NOT read Cargo.toml/tauri.conf.json)
11. `bunx tauri build --target ${{ matrix.target }}` with signing secrets

### `release-accelerator.yml` `build-headless` job (lines 175–227)

Same as `build` except:
- Step 3 uses `key: headless-${{ matrix.target }}`
- Step 7 gates on `runner.os == 'Linux'` (correct future-proof predicate)
- Step 8 (`tauri.conf.json` patch) is omitted
- Step 11 is replaced by `cargo build --release --bin accelerator-server --target X` + tar + shasum

### `_e2e-webdriver.yml` (verified at lines 28–64)

Same env setup pattern + additional apt deps (xvfb, stalonetray, dbus-x11, etc.) for the WebDriver-on-CI flow. Has its own inline `rust-toolchain` + `rust-cache` + `setup-bun` + `bun install` + `prebuild`.

### `Cargo.toml` cache-key behavior

Confirmed via `Swatinem/rust-cache@v2` README ("the `key` is *additive*, not a replacement"): distinct prefixes (`release-` vs `headless-` vs `""`) provide cache isolation; the **automatic** part of the key already includes a hash of `Cargo.toml` + `Cargo.lock`. If `Cargo.toml` is mutated before rust-cache runs, the auto key changes → cold cache every release. **This is the v1 bug codex caught.**

## Design v2

### Composite — env-only, no source mutation

```yaml
name: Setup Accelerator
description: Install system deps, Bun, Rust toolchain, copy bb sidecar — environment setup for accelerator CI jobs.

inputs:
  rust-target:
    description: Optional Rust target triple for cross-toolchain install (e.g. `x86_64-apple-darwin`). NOTE: prebuild (`copy-bb.ts`) chooses the bb sidecar from the HOST `process.platform`/`process.arch`; cross-compile callers must run on a matching host. This input only configures the dtolnay/rust-toolchain `targets:` field.
    required: false
    default: ""
  rust-cache-key:
    description: Extra cache key suffix for rust-cache. Use to isolate caches between build kinds. Append-only (Swatinem/rust-cache adds it after its automatic key).
    required: false
    default: ""
  rust-components:
    description: Comma-separated Rust components. Default 'clippy,rustfmt' preserves PR-gate behavior.
    required: false
    default: "clippy,rustfmt"

runs:
  using: composite
  steps:
    - name: Assert host matches rust-target      # ← fail-fast guard against cross-compile misuse
      if: inputs.rust-target != ''
      shell: bash
      env:
        REQUESTED_TARGET: ${{ inputs.rust-target }}
        RUNNER_OS: ${{ runner.os }}
        RUNNER_ARCH: ${{ runner.arch }}
      run: |
        # Derive host triple from runner facts.
        case "$RUNNER_OS-$RUNNER_ARCH" in
          Linux-X64)   HOST="x86_64-unknown-linux-gnu" ;;
          Linux-ARM64) HOST="aarch64-unknown-linux-gnu" ;;
          macOS-ARM64) HOST="aarch64-apple-darwin" ;;
          macOS-X64)   HOST="x86_64-apple-darwin" ;;
          *) echo "::error::Unsupported runner os/arch: $RUNNER_OS-$RUNNER_ARCH"; exit 1 ;;
        esac
        if [ "$HOST" != "$REQUESTED_TARGET" ]; then
          echo "::error::rust-target ($REQUESTED_TARGET) does not match host triple ($HOST). Prebuild chooses bb sidecar by host arch; cross-compile is not supported by this composite. Use a matching runner."
          exit 1
        fi
        echo "Host=$HOST matches rust-target=$REQUESTED_TARGET"

    - name: Install Linux system dependencies
      if: runner.os == 'Linux'                  # ← gated (defense for future macOS callers)
      shell: bash
      run: |
        sudo apt-get update
        sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libgtk-3-dev

    - uses: oven-sh/setup-bun@v2
      with:
        bun-version: latest

    - uses: actions/cache@v5
      with:
        path: ~/.bun/install/cache
        key: ${{ runner.os }}-bun-${{ hashFiles('bun.lock') }}
        restore-keys: ${{ runner.os }}-bun-

    - run: bun install --frozen-lockfile
      shell: bash

    - uses: dtolnay/rust-toolchain@stable
      with:
        components: ${{ inputs.rust-components }}
        targets: ${{ inputs.rust-target }}

    - uses: Swatinem/rust-cache@v2              # ← BEFORE any Cargo.toml mutation (preserves auto key)
      with:
        workspaces: packages/accelerator/src-tauri -> target
        key: ${{ inputs.rust-cache-key }}

    - name: Copy bb sidecar                      # ← prebuild stays after toolchain (matches PR-gate composite today)
      shell: bash
      run: bun run --cwd packages/accelerator prebuild
```

**Three inputs, all optional**, all backwards-compatible with PR-gate callers (defaults preserve current behavior).

### Caller pattern (canonical)

```yaml
steps:
  - uses: actions/checkout@v6
  - uses: ./.github/actions/setup-accelerator
    with:
      rust-target: ${{ matrix.target }}
      rust-cache-key: release-${{ matrix.target }}
      rust-components: ""
  - name: Patch versions (release jobs only)
    env:
      RELEASE_VERSION: ${{ needs.validate.outputs.version }}
    run: |
      cd packages/accelerator/src-tauri
      sed -i.bak "s/^version = \".*\"/version = \"$RELEASE_VERSION\"/" Cargo.toml
      rm -f Cargo.toml.bak
      bun -e "
        const conf = JSON.parse(await Bun.file('tauri.conf.json').text());
        conf.version = process.env.RELEASE_VERSION;
        await Bun.write('tauri.conf.json', JSON.stringify(conf, null, 2) + '\n');
      "
  - name: Build Tauri bundle
    run: bunx tauri build --target ${{ matrix.target }}
    working-directory: packages/accelerator
    env:
      TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
      # ... rest of signing secrets unchanged
```

The version-patching runs AFTER the composite (after `rust-cache`), so cache key stays stable across releases. Yes, this means rebuilds after Cargo.toml mutation will recompile changed crates — but the **cache hits** still work (just the changed crate gets rebuilt, which is correct behavior).

### `build-headless` — same composite call, no tauri.conf patch

```yaml
- uses: ./.github/actions/setup-accelerator
  with:
    rust-target: ${{ matrix.target }}
    rust-cache-key: headless-${{ matrix.target }}
    rust-components: ""
- name: Patch Cargo.toml version
  env:
    RELEASE_VERSION: ${{ needs.validate.outputs.version }}
  run: |
    cd packages/accelerator/src-tauri
    sed -i.bak "s/^version = \".*\"/version = \"$RELEASE_VERSION\"/" Cargo.toml
    rm -f Cargo.toml.bak
- name: Build accelerator-server
  ...
```

### `_e2e-webdriver.yml` — composite call + extra apt

```yaml
- uses: ./.github/actions/setup-accelerator      # ← shared env (incl. Linux core apt deps)
- name: Install E2E-specific apt deps            # ← job-specific extras
  if: runner.os == 'Linux'
  run: |
    sudo apt-get install -y xvfb stalonetray dbus-x11 ...
```

This kills the second drift surface codex flagged.

## Permissions tightening (separate concern, in this PR)

Move from broad workflow-level to per-job least privilege:

```yaml
# Workflow level (today):
permissions:
  id-token: write       # only needed by AWS step in release job
  contents: write       # only needed by tag, release, bump-source jobs
  pull-requests: write  # only needed by bump-source

# After (workflow level):
permissions:
  contents: read

# Per job:
build:          permissions: contents: read
build-headless: permissions: contents: read  # (already had this)
smoke:          permissions: contents: read
validate:       permissions: contents: read
tag:            permissions: contents: write
release:        permissions: { contents: write, id-token: write }    # id-token: write for OIDC to AWS
bump-source:    permissions: { contents: write, pull-requests: write }
```

Rationale: `id-token: write` should be present only on the job that does `aws-actions/configure-aws-credentials@v6`. `pull-requests: write` is only needed by `bump-source` (which calls `gh pr create`). `contents: write` is only needed by jobs that push (`tag`, `release` uploads, `bump-source` commits).

## Phases

### Phase 1 — Inspection + sanity checks

1. Confirm inventory above by re-reading composite + workflows + `copy-bb.ts`.
2. **`bun install` postinstall sanity check**: search every `package.json` in the workspace for `"postinstall"` / `"preinstall"` / `"prepare"` scripts. Confirm none invoke `cargo`, `rustc`, or `rustup`. If any do, reordering Rust later breaks. (Today: confirmed bun cleanly via current PR-gate composite which already runs `bun install` before Rust on the PR-gate path.)
3. Document findings in `lessons/phase-1.md`.

### Phase 2 — Extend composite (env-only)

1. Edit `.github/actions/setup-accelerator/action.yml` per the Design v2 above.
2. **Crucial check**: composite has NO `Cargo.toml` mutation, NO `tauri.conf.json` mutation. Source mutation is caller responsibility.
3. PR-gate green on draft PR (default inputs preserve current behavior).
4. `bun run lint:actions`.

### Phase 3 — Refactor `build` job

1. Edit `release-accelerator.yml` `build` job: replace inline env steps with composite call, keep version-patching + `tauri build` inline.
2. PR-gate green.

### Phase 4 — Refactor `build-headless` job

1. Same pattern. No tauri.conf patch.
2. PR-gate green.

### Phase 5 — Refactor `_e2e-webdriver.yml`

1. Replace inline env setup with composite + job-specific apt install for xvfb/stalonetray.
2. PR-gate green (E2E runs as PR gate).

### Phase 6 — Permission tightening

1. Remove broad workflow-level grants.
2. Add per-job `permissions:` blocks per Design.
3. PR-gate green.

### Phase 7 — Prerelease validation (`1.0.2-rc.2`)

1. Merge feature PR to main.
2. Trigger `release-accelerator.yml` with `version: 1.0.2-rc.2`.
3. Watch: all 3 `build` matrix + 4 `build-headless` matrix + `smoke` + `tag` + `release` green.
4. Compare release-files structure to `accelerator-v1.0.1-rc.3` — byte-identical naming.
5. Spot-check DMG: launches, health-checks.
6. What this validates: shared composite + build matrix on all 7 platform combos, signature flow (.sig present), smoke gate. What it does NOT validate: `latest.json` generation, AWS OIDC, S3 upload, CloudFront invalidation, `bump-source` PR — all are stable-only paths.

### Phase 8 — Stable cut (`1.0.2`) — this IS production

> ⚠️ This is **not a dry-run** — there is no non-prod equivalent for the stable-only paths (`latest.json` → S3 → auto-updater feed; `bump-source` PR). Phase 7 is the dry-run; Phase 8 is the real cut. Treat it as a regular release.

1. After Phase 7 passes, trigger with `version: 1.0.2` (real stable).
2. Watch: `latest.json` generation, AWS OIDC, S3 upload, CloudFront invalidation, `bump-source` PR opens, auto-merge fires.
3. Compare `latest.json` content vs last stable release (`1.0.1` → `1.0.2` version bump, signatures populated for 3 platforms).
4. Verify auto-updater path on a `1.0.1` install (existing user).
5. Verify `bump-source` PR lands source at `1.0.3-rc.1`.

### Phase 9 — Supply-chain hardening (SEPARATE FOLLOW-UP PR)

This is intentionally a **separate PR** so the Phases 1–8 refactor can be reviewed and released without the unrelated diff noise. To be opened after `1.0.2` ships clean.

1. SHA-pin third-party actions inside composite — `oven-sh/setup-bun`, `Swatinem/rust-cache`, `actions/cache`. Leave `dtolnay/rust-toolchain@stable` as-is (documented usage for that action — `stable` is the toolchain channel selector, not a version tag).
2. Add `.github/CODEOWNERS` (or extend if exists) for `.github/actions/**` + `.github/workflows/**` requiring curator approval.
3. Optional: pin `actions/checkout`, `actions/upload-artifact`, `actions/download-artifact`, `aws-actions/configure-aws-credentials` to SHAs too.

### Phase 10 — Cleanup (post-Phase-8)

1. Leave `1.0.2-rc.2` prerelease intact (matches policy from release-2026-05-27 for immutable history; auto-updater unaware because prereleases skip `latest.json`).
2. Update `CLAUDE.md` if any commands or job graph changed.
3. Update `implementations-plan/index.md` with completion marker.
4. Final `lessons/phase-N.md` summary.
5. Open Phase 9 hardening PR.

## Test strategy

Workflow refactor — no new unit tests. Validation matrix:

| Test | How | Phase |
|------|-----|-------|
| `bun install` has no Rust-invoking hooks | grep all `package.json` files for `cargo`, `rustc`, `rustup` in install scripts | Phase 1 |
| PR-gate (all 7 jobs) still green | Feature PR draft | Phase 2/3/4/5/6 |
| Release prerelease end-to-end | `1.0.2-rc.2` workflow_dispatch | Phase 7 |
| Release stable end-to-end (incl. `latest.json` + S3 + bump-source) | `1.0.2` workflow_dispatch | Phase 8 |
| Cache reuse (incremental) | Verify in Phase 8 logs that `rust-cache` reports a partial cache hit between `-rc.2` and `1.0.2` runs | Phase 8 |

## Security & Adversarial Considerations

### Threat model

1. **Composite poisoning**: same threat as today — composite already lives in the repo. Mitigations: CODEOWNERS (added Phase 9), branch protection on `main`, PR review discipline. Centralizing third-party actions inside the composite **does** increase blast radius if a transitive action tag is retargeted → mitigated by SHA-pinning in Phase 9.
2. **Cache poisoning across build kinds**: `rust-cache-key` input adds isolation suffix (`release-` / `headless-` / `""`). The default empty maintains PR-gate isolation via Swatinem's automatic job-id keying. Validated as additive per README.
3. **Secrets boundary preserved**: TAURI signing secrets remain at the `bunx tauri build` step inside `build` job only. Composite has no `secrets.*` references — confirmed by reading the composite YAML above.
4. **Least privilege improved**: Phase 6 moves over-broad workflow-level perms down to specific jobs. Net reduction in attack surface (each job exposes only what it needs to a token thief).
5. **`dtolnay/rust-toolchain@stable`**: not a SHA, by design — `stable` is the toolchain channel selector (not a tag). Documented usage. Accepted exception to SHA-pinning.
6. **Supply chain attack surface**: `oven-sh/setup-bun@v2`, `Swatinem/rust-cache@v2`, `actions/cache@v5` — major-tag pinned today. Phase 9 SHA-pins them, reducing tag-retarget risk.

### What an attacker would target

- **The composite itself** (silent change to add a `curl | sh` line, or modify `bun install` to run a postinstall script). Mitigations: CODEOWNERS + review.
- **A third-party action** retargeting their tag to a malicious version. Mitigations: SHA-pin (Phase 9).
- **The `bun install` step**: a malicious dep landing in `bun.lock` would run. Mitigated by frozen-lockfile + commit `bun.lock` discipline (already in place).

### Domain checklist

- **Supply chain**: `bun.lock` committed, `--frozen-lockfile`, `minimumReleaseAge=604800` configured in `bunfig.toml` (verify in Phase 1). SHA-pin actions Phase 9.
- **Least privilege**: tightened Phase 6.
- **OIDC**: unchanged — already used for AWS via `aws-actions/configure-aws-credentials@v6`.

## Rollback

Workflow refactor with no DB/external state. Revert the PR. Last-known-good: commit `5d46f53` (post-1.0.1 source-bump).

For partial rollback (composite works but release jobs need to revert): revert only the release-job changes; composite-level changes remain backwards-compatible (defaults preserve PR-gate behavior).

## Open questions (resolved before approval gate)

| Q | A |
|---|---|
| Does `prebuild` read `Cargo.toml` / `tauri.conf.json`? | **No** — `copy-bb.ts` only resolves `@aztec/bb.js` and copies the binary + writes `AZTEC_VERSION`. Verified. |
| Swatinem rust-cache `key:` semantics? | **Additive**. Distinct prefixes prevent cache pollution. Verified via README + codex confirmation. |
| Any current macOS caller of the composite? | **No**. All current consumers are ubuntu-latest. Apt-guard is defense-in-depth for the soon-to-be-added macOS callers from `build`. |
| Does `bun install` invoke Rust via postinstall? | **Sanity-check in Phase 1**. Current PR-gate composite already runs bun install before Rust without issue. |
| `_e2e-webdriver.yml` in scope or accepted drift? | **In scope** (Phase 5) — drift-prevention story is incomplete without it. |
| Tighten `build` job perms to `contents: read`? | **Yes** (Phase 6) — `build` doesn't push. |
| SHA-pin third-party actions? | **Yes** (Phase 9). Except `dtolnay/rust-toolchain@stable` which is documented branch usage. |

## Estimated scope

- Composite v2: ~25 line diff (3 inputs added; apt gate added)
- `build` job: net -25 lines (env into composite, keep patch + build inline)
- `build-headless` job: net -25 lines
- `_e2e-webdriver.yml`: net -20 lines + 5 lines for E2E-specific apt
- Permission tightening: +30 lines per-job, -3 lines top-level
- SHA-pinning + CODEOWNERS (Phase 9): ~15 lines composite + new CODEOWNERS file
- Total: 1 PR, ~120 lines diff
- Implementation: ~45 min coding + ~25 min PR-gate validation + ~30 min `-rc.2` dry-run + ~30 min `1.0.2` stable dry-run + ~15 min Phase 9 hardening
