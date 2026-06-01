# CI/Release Overhaul — consolidated plan

**Status:** reviewed (3 parallel plans + final codex pass; 5 findings folded in) · pending approval · **Type:** infra / release-gating / CI-speed (cross-cutting) · **Tier:** A (main + codex + opus subagent)

Two coupled goals: **(A)** promote `update-smoke` to **release-blocking** (macOS arm64 + Intel + Linux AppImage); **(B)** **cut CI time** for the release pipeline + PR gates. They reconcile: parallelizing the pre-build gate more than pays for the added blocking gate.

## Locked decisions (from the user)
1. Hybrid gate: parallelize the WebDriver gate with build, **cancel-on-fail**.
2. update-smoke blocking on macOS arm64 + Intel + **Linux AppImage** (+ Intel DMG notarization check; + a new Linux updater path).
3. **Flat-cost first**; `macos-xlarge` earmarked for the Intel build leg, not default.
4. Scope = release pipeline **and** PR gates.

## Critical-path math (warm caches; from the rc.5 run)
```
TODAY:   validate 0.1 → gate 4.5 (SERIAL, ~20 cold) → build/Intel 7.6 → smoke 0.2 → tag+rel 0.5   ≈ 13m  (worse cold)
TARGET:  validate 0.1 → max(gate 4.5 ∥ build/Intel 7.6) → update-smoke 2.4 ∥ smoke → tag+rel 0.5   ≈ 10–11m  (cold: gate∥build hides the gate entirely)
```
Net: **faster even though we add a blocking updater gate** — because the gate stops being a serial pre-build step.

---

## Workstream A — promote update-smoke to release-blocking

### Phase A1 — Hybrid gate (parallelize + cancel-on-fail) [FREE; biggest single win]
Today `build`/`build-headless` `needs: [validate, e2e-webdriver]` — the gate (4.5m warm / ~20m cold) runs fully *before* any build. Rewrite:
- `build` + `build-headless` → `needs: [validate]` only (start in parallel with the gate). `e2e-webdriver` stays `needs: [validate]`.
- **Correctness guard (do NOT drop):** keep `e2e-webdriver` in the `needs` of every post-build job — `smoke`, `smoke-intel`, `update-smoke-*`, `tag`, `release`. So even if the watchdog no-ops, **no ungated release can tag.** (opus's belt-and-suspenders.)
- **INVARIANT (codex final review):** GitHub cancellation is *best-effort* — the watchdog is **cost-control only, never the safety property.** The `needs`-chain is the only guarantee. **Any future job with side effects** (tag/publish/deploy/notarize-and-ship) **MUST transitively `needs` the gate**, or it reopens an ungated-ship hole. Document this next to the DAG.
- **`gate-watchdog` job** (cost optimization only): `needs: [e2e-webdriver]`, `if: ${{ always() && needs.e2e-webdriver.result == 'failure' }}`, `runs-on: ubuntu-latest`, `timeout-minutes: 2`, `permissions: { actions: write }` (scoped to *this job only*; workflow default stays `contents: read`). One step: `gh run cancel ${{ github.run_id }}` with `GH_TOKEN: ${{ github.token }}`. **No checkout, no third-party action, no secrets** — minimal supply-chain surface (chosen over codex's `actions/github-script` precisely to avoid pinning another action).
- **PR-side bonus (codex):** add `concurrency: { group: <workflow>-<pr/ref>, cancel-in-progress: true }` to `accelerator.yml`, `app.yml`, `sdk.yml` so a new push cancels the superseded in-flight run (real minutes saved on rapid pushes — felt repeatedly this session).
- **Risk:** build now starts before the gate proves green → on the rare gate failure we spend some build minutes (the explicit trade the user chose). The watchdog bounds it; build (~8m) usually finishes before the cold gate (~20m) even resolves, so there's often nothing to cancel.
- **Validation:** break one WebDriver test on a throwaway branch, dispatch a release; confirm builds start, gate fails, watchdog cancels within ~30s, no `tag`/`release` runs.

### Phase A2 — update-smoke → blocking, 3 platforms [mostly wiring]
- **macOS arm64 + Intel positive → blocking now.** Add `update-smoke-macos-arm64` + `update-smoke-macos-intel` to `tag.needs`. They run after `build`, parallel with `smoke`, finishing inside the Intel-build envelope → **no critical-path inflation** beyond ~2.4m which overlaps. Drop the "ADVISORY" framing.
- **Intel DMG notarization check.** `smoke` verifies notarization only on the **arm64** DMG today. Split into `smoke-macos-arm64` (mount + `codesign --verify --deep --strict` + `stapler validate` + launch + `/health`) and `smoke-macos-intel` (`runs-on: macos-15-intel`, verify-only: `codesign --verify` + `stapler validate` on the Intel `.app`). Both in `tag.needs`. **Skip `spctl --assess`** on the blocking path — it can hit Apple's network and flake (advisory at most). *(opus + me; codex suggested spctl "if stable" — rejected for the blocking path due to network dependence.)*
- **Linux AppImage updater-smoke (the new, riskiest piece):**
  - New `updater-smoke-linux.sh`; **reuse `updater-feed-server.ts` verbatim** (it's pure Bun, OS-agnostic — verified). Parameterize `_e2e-updater.yml` with `os: macos|linux`, `n1-asset-glob`, `artifact-kind`.
  - CA trust: `cp ca.pem /usr/local/share/ca-certificates/<run-unique>.crt && sudo update-ca-certificates` (honored because the updater's TLS uses `rustls-platform-verifier` → OS store). `/etc/hosts` override as on macOS. Cleanup: remove cert + `update-ca-certificates --fresh`.
  - Faux desktop: reuse `_e2e-webdriver.yml`'s `Xvfb + stalonetray + dbus-launch` (don't test updater semantics shell-only).
  - Install N-1 `.AppImage` (`gh release download`, `chmod +x`, writable stable path), launch with `APPIMAGE=<path>` + `DISPLAY=:99`, preseed `auto_update:true`, poll `/health.version == N`, assert a `/releases/download/` hit **and** that the writable AppImage's checksum changed (proves the in-place swap).
  - **⚠️ Spike FIRST — two make-or-break unknowns (codex final review + both planners):**
    - **(1) Artifact format [primary].** With `createUpdaterArtifacts: "v1Compatible"`, Tauri's Linux updater may expect an **`AppImage.tar.gz`** (compressed) artifact — but the repo's `latest.json` currently points `linux-x86_64` at the **raw `.AppImage`** + `*.AppImage.sig` (release-accelerator.yml:31). **Verify what `tauri build` actually emits for Linux** (raw `.AppImage` + `.AppImage.sig`, vs `.AppImage.tar.gz` + `.tar.gz.sig`) **and what the shipped updater downloads/applies** — the smoke must serve the artifact the real app fetches. **If they mismatch, Linux auto-update is already broken in production**, and the smoke's value is surfacing that (fix the release `latest.json`/upload first). This reframes the Linux work: it may be a *bug-fix + gate*, not just a gate.
    - **(2) FUSE / native execution [secondary].** Tauri's AppImage self-update replaces the file in place and needs native execution; if `ubuntu-latest` lacks FUSE → `--appimage-extract-and-run` tests the wrong path. Install `libfuse2`; confirm native launch + `$APPIMAGE` set. **Resolve (1) before (2)** — no point FUSE-testing the wrong artifact.
  - **Rollout:** macOS arm64+Intel blocking immediately; **Linux stays advisory through exactly one green `-rc` dry-run, then flips into `tag.needs`.** Never block real releases on an unproven native-AppImage path.
- **Negative (teeth) leg — BLOCKING on stable too (codex final-review correction):** keep **one** arm64 negative leg (tamper the artifact → must reject; proves *signature enforcement*, arch-independent, so no per-platform negatives). Put it in **`tag.needs` for both `-rc` and stable.** Earlier draft kept it rc-only/stable-advisory; the final review correctly flagged the gap: releases are `workflow_dispatch` on **repo state**, so a green rc does NOT guarantee the stable commit didn't regress signature enforcement afterward. Blocking on stable is safe because the negative test serves the tampered artifact from the **local feed (no CDN)** — its flake surface ≈ the (blocking) positive leg's, so it won't add wedge risk. *(opus wanted a Linux negative leg too — rejected as redundant: crypto enforcement is arch-independent.)*
- **Validation:** one `-rc` dry-run green on all 3 positive legs + the negative leg red-when-fed-bad; confirm `tag` is gated.

---

## Workstream B — CI speed (caching + PR gates) [FREE]

### Phase B1 — Rust cache that actually shares
Verified: PR jobs call `setup-accelerator` with **no `rust-cache-key`** (empty), release `build` uses `release-<target>`, `build-headless` uses `headless-<target>`, `_e2e.yml` uses `e2e-accelerator` — **four non-overlapping scopes**, so nothing cross-warms. Fixes (codex's specifics — adopt):
- Refactor `setup-accelerator` to use `Swatinem/rust-cache` **`shared-key`** (replaces the auto job-hash prefix → caches actually shared across jobs) instead of the append-only `key`. Per-(component × target) keys: `accelerator-desktop-<target>`, `accelerator-server-<target>`.
- **Split cache workspaces by consumer:** desktop jobs cache only `src-tauri -> target`; server jobs (`build-headless`, `smoke`, `release-smoke`, `_e2e.yml`) cache only `server -> target`. (Today the composite caches both everywhere → bloated, evicted sooner.)
- Replace `_e2e.yml`'s bespoke `e2e-accelerator` key with the shared **server** key → SDK E2E, `smoke`, `release-smoke` all hit one server cache.
- `rust-components: ""` in `test`/`smoke`/`release-smoke` (only `clippy` needs clippy/rustfmt) — skip a component download.
- **Keep the release version-patch AFTER the cache step** (verified it already is — rust-cache hashes `Cargo.toml`; moving it earlier would cold every version).
- **Profile caveat + honest framing (my finding + codex final review):** PR builds are *debug*, release is *release* profile — they coexist in one cache dir but compile deps at different opt levels, so cross-profile sharing is partial. The win is real *within* a profile (server debug across smoke/_e2e; release across release-smoke) and via the warmer below. **Net: this is *better/more reliable reuse*, NOT literally "guaranteed hits"** — debug-vs-release, `webdriver`-vs-non features, and GitHub's 7-day eviction all still apply; B1b (the warmer) is what actually approaches residency.
- **Optional B1b — `warm-rust-cache.yml` on `main` + weekly schedule** (the only way to *guarantee* residency past GitHub's 7-day idle eviction — same pattern as the Playwright warmer we just shipped). Recommended but separable.

### Phase B2 — PR-gate speed
- **Path-filter granularity (codex) — with a guardrail (codex final review):** split `accelerator.yml`'s `changes` into `desktop_relevant` vs `integration_relevant`, so a **pure `packages/sdk/src/**` change** keeps SDK E2E but does **not** trigger macOS WebDriver / Desktop UI / clippy / release-smoke. **`desktop_relevant` MUST retain ALL shared-infra paths** it already depends on — `.github/actions/**`, `.github/workflows/accelerator.yml` + `_e2e*.yml`, `package.json`, `bun.lock`, `biome.json`, `tsconfig*` — so a workflow/composite/lockfile/config change still reruns the desktop gates. **Only the pure-sdk-src case is narrowed** (final-review catch: under-narrowing → silent skips of gates that should run). Big win — those macOS/Rust jobs are the PR long-poles.
- **Playwright:** the cache composite + warm-on-main (just shipped) cover all 4 install sites incl. `local-network-e2e` — verified. No further work; it's the model.
- Concurrency cancellation already added in A1.

---

## Phase C — paid lever (optional, measured)
`macos-xlarge` (~2× cost, ~1.5–2× faster) on **`build-macos-intel` only**, *after* B lands and we re-measure. It's the one paid lever that hits the 7.6m Intel long-pole + cold compiles. Cross-compiling Intel on an arm64 xlarge is blocked by the composite's host==target assertion (separate effort). `mold` (Linux-only linker, free) is a low-priority experiment — doesn't move the Intel pole. `sccache` rejected (poisoning surface + ops complexity).

---

## Security & Adversarial Considerations
- **Watchdog `actions: write`** — powerful (can cancel/re-run any workflow). Isolated to one tiny job, **no checkout / no secrets / no third-party action / no `gh` beyond `run cancel`**. Correctness never depends on it (gate stays in post-build `needs`), so a subverted/no-op watchdog can't ship an ungated release.
- **SHA-pin third-party actions in the release workflow** (codex) — it handles signing/notarization secrets, so pin `actions/*`, `Swatinem/rust-cache`, `oven-sh/setup-bun`, etc. by full commit SHA. Higher bar than PR CI. (Matches the supply-chain ethos.)
- **Updater local feed stays secretless** — serves the already-prod-signed N artifact; N-1 verifies against its embedded minisign pubkey. **No CI signing key** for Linux or macOS, ever. `contents: read`.
- **Fake-CA / MITM hygiene** — the `/etc/hosts` + trusted-CA is a deliberate MITM of `aztec-accelerator.dev` *inside an ephemeral VM*. Use a **run-unique CN/filename** (not the fixed `updater-smoke-local-CA`) on both macOS and Linux so a self-hosted runner can't lose an unrelated trust entry; mirror the anchored-cleanup rigor. (Also a retro-hardening of the existing macOS script.)
- **Cache poisoning** — sharing Rust/Playwright caches is safe *only* because GitHub scopes PR-created caches to the merge ref; a PR cannot write `main`'s scope. Keep the shared/default cache written only by trusted `main`/release code. This is the decisive reason to reject `sccache`.
- **Flaky blocking gate wedging releases** — the genuine Phase A2 risk. Mitigations: rc-shadowing (Linux advisory until proven); negative leg non-blocking on stable; `fail-fast: false` (set); per-arch artifact independence (an Intel flake can't wedge arm64). **No inline bypass flag.** If emergency escape is ever needed, a **separate owner-only break-glass workflow with explicit approval** — never a default-path skip.

## Sequencing
1. **A1** (gate + concurrency) — biggest win, unblocks the DAG for A2.
2. **B1** (Rust cache) + **B2** (path filters) — free speed, independent of A.
3. **A2** (update-smoke blocking: macOS now, Linux rc-shadow→blocking).
4. **C** (xlarge) only if re-measurement still shows Intel as the ceiling.
Each phase: its own PR, `actionlint` + a `-rc` dry-run where it touches the release path. Lessons in `lessons/phase-N.md`.

## Provenance
- **opus:** 5-phase structure, belt-and-suspenders gate guard, AppImage FUSE risk + rc-shadow rollout, feed-server-is-OS-agnostic, profile caveat framing.
- **codex (1st input):** `shared-key` + workspace split, `rust-components` trim, `_e2e` key unify, PR concurrency cancel, path-filter split, SHA-pin release actions, run-unique CA, critical-path math correction, `gh run cancel` vs `github-script`.
- **codex (final review, 5 catches folded in):** (1) **Linux AppImage artifact-format mismatch** — v1Compatible may need `.AppImage.tar.gz`, repo `latest.json` points at raw `.AppImage` → spike-first + possible pre-existing-bug surface; (2) **negative leg → blocking on stable** (rc-green ≠ stable commit, since release is `workflow_dispatch` on repo state) — *reversed* the earlier non-blocking call; (3) watchdog is cost-control only, needs-chain is the guarantee, future side-effect jobs MUST need the gate; (4) path-filter `desktop_relevant` must retain shared-infra paths or silent skips; (5) "guaranteed hits" → "better reuse".
- **main (me):** verified the cache-key divergence + profile nuance + the Linux `latest.json`-points-at-raw-AppImage fact; chose `gh run cancel` (leanest supply chain).
- **Rejected:** Linux negative leg (redundant — crypto is arch-independent); `spctl` on the blocking path (network flake); `sccache` (poisoning); moving the version-patch before the cache (cold every version).
