# Headless Slim Follow-ups — release build-headless + e2e GUI-libs

**Tier:** `/blueprint mid`. Follow-up to `headless-ci-slim-2026-06-08` (the Phase-3b composite flags +
version-only resolver, now merged). **Split into two PRs per the owner:** Part A (e2e) merges on its PR gate;
Part B (release build-headless) is rc-gated. No `/harden`.

## Summary
Finish the headless slim by extending it to the two legs Phase 3b deferred:
- **Part A (PR1, PR-gate-validated):** drop the desktop GUI libs (WebKit/GTK) from `_e2e.yml`'s inlined apt
  install — it only `cargo build`s the headless server, so it never needs them. KEEP its prebuild (the SDK e2e
  needs the real `bb` via `BB_BINARY_PATH`).
- **Part B (PR2, rc-gated):** slim the release `build-headless` job (4-platform headless tarballs) via the
  Phase-3b composite flags (`install-tauri-system-deps: "false"` + `run-prebuild: "false"`), and remove its now-
  dead `src-tauri/Cargo.toml` version patch + stale comment. Validated by an owner-dispatched rc dry-run.

## Verified facts (recon)
- `build-headless` (release-accelerator.yml:237) matrix = {aarch64/x86_64 darwin, x86_64/aarch64 linux} on
  matching runners; uses `./.github/actions/setup-accelerator` with `rust-target: ${{ matrix.target }}` +
  rust-cache-key/workspaces/components. So the Phase-3b boolean flags apply directly.
- Its "Patch version" step (release yml:34–42 of the job) patches BOTH `src-tauri/Cargo.toml` AND
  `server/Cargo.toml`. Post-core-extraction the server builds against `accelerator-core` (NOT src-tauri), and
  `/health.version` is the INJECTED server `CARGO_PKG_VERSION` (server/main.rs) — so the `src-tauri` patch is
  **dead** (src-tauri isn't compiled here) and its comment ("/health reports via env! from the shared lib") is
  stale. `accelerator-server --version` correctly reads the `server` crate version (still patched). Removing the
  src-tauri patch is safe; the desktop `build` job patches src-tauri on its own runner.
- With `run-prebuild: "false"`, the composite's host/target assert is skipped (Phase-3b gating). For
  `build-headless` that's safe: the matrix pairs each target with a matching-arch runner, so host==target is
  guaranteed by runner selection, and `cargo build --target` would fail on a genuine mismatch anyway. The assert
  only ever protected host-selected sidecar copying (the prebuild), which is now off.
- `_e2e.yml:49` installs `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libgtk-3-dev`
  then (line 53) runs the prebuild + (line 82–86) `cargo build`s ONLY the headless server. So WebKit/GTK is waste;
  `libssl-dev` (reqwest) + the prebuild (bb for the e2e) must stay.

## Approach A — MAIN: surgical inline edits (provisional pick)
- **Part A:** edit `_e2e.yml:49` → `sudo apt-get install -y libssl-dev` (drop the 5 desktop packages; keep the
  prebuild step + BB_BINARY_PATH + the AZTEC_BB_VERSION hook untouched).
- **Part B:** add `install-tauri-system-deps: "false"` + `run-prebuild: "false"` to `build-headless`'s composite
  `with:`; delete the `src-tauri/Cargo.toml` sed line + fix the comment to reflect injected-server-version.

## Approach B — COMPETING: route `_e2e.yml` through the composite
Instead of editing `_e2e.yml`'s inline apt, replace its hand-rolled setup (apt + bun-install + rust-toolchain +
cache) with a `setup-accelerator` call using `install-tauri-system-deps: "false"` + `run-prebuild: "true"` (keep
the bb prebuild). **Pros:** DRY — `_e2e` stops duplicating the composite's setup (kills the 3-copy apt drift the
Phase-3b audit flagged). **Cons:** bigger diff on a release-adjacent reusable workflow; `_e2e` sets
`BB_BINARY_PATH`/`AZTEC_BB_VERSION` + has its own step ordering that must interleave with the composite's prebuild
— more interactions to get right. **Provisional pick: A** — Part A is meant to be the *trivially-safe, merge-now*
half; the composite-routing is a nice cleanup but raises its risk/diff for marginal gain. Note B as a future
de-drift follow-up. The audit should weigh whether B's drift-kill is worth folding in now.

## Phases
**Part A — ✓ DONE — PR1 (`_e2e.yml`, PR-gate):**
1. Edit `_e2e.yml:49` → `libssl-dev` only. `bun run lint:actions`. Self-proves on the PR gate (the SDK E2E runs
   with `build_accelerator: true` → builds + runs the headless server + proves). → `lessons/phase-a.md`.

**Part B — PR2 (`build-headless`, rc-gated):**
2. `build-headless` (release-accelerator.yml:263+): **(a)** add `install-tauri-system-deps: "false"` +
   `run-prebuild: "false"` to the composite `with:`; **(b)** delete the dead `sed` on `src-tauri/Cargo.toml` (:277)
   + its `.bak` rm; **(c)** fix the version-patch comment (:274-276) — only the SERVER crate is patched;
   `/health.version` + `--version` both read the injected server version; **(d) fix the stale `--locked` comment
   (:287-290)** — both auditors: it cites the now-gone src-tauri patch; the REAL reason `--locked` stays off is
   that patching `server/Cargo.toml` (:278) stales the server lock's own `accelerator-server` stanza (else a future
   reader "re-adds `--locked` since src-tauri is gone" → breaks the release build). `bun run lint:actions`. → `lessons/phase-b.md`.
3. **rc validation — a REAL prerelease PUBLISH, not a dry-run (both auditors).** Owner dispatches
   `release-accelerator.yml --ref <branch> -f version=1.0.5-rc.N` with a **FRESH rc.N each run** (the tag step skips
   if the tag exists + the release step deletes/recreates → reusing an rc.N attaches new assets to an old tag;
   release:550-559/705-709). It pushes a public `accelerator-v1.0.5-rc.N` tag + a GitHub **prerelease** (4-platform
   headless tarballs + desktop builds). It SKIPS S3 / `latest.json` / live-feed / `bump-source`
   (`is_prerelease=='false'`-gated) → the **prod feed + real 1.0.x users are untouched**.
   **What rc-green proves for the slimmed `build-headless` (final codex — precise):** it compiled, self-reported
   the right version (`--version` assert @release:293-302), packaged+checksummed, and got included in the
   prerelease. It does **NOT** prove the headless tarball's extracted-runtime / `/health` — the BLOCKING
   updater-smokes consume only the **DESKTOP** artifacts (`_e2e-updater*.yml` download the `n-artifact` desktop
   builds, not the `accelerator-server-*` tarballs). Headless runtime/`/health` stays covered by the PR-gate Smoke
   job (on the dev build); this slim doesn't change it. Merge once the rc is green.

## Security & Adversarial Considerations
- **Release-pipeline blast radius (Part B):** the build-headless tarball ships to external CI consumers. The CODE
  change only REMOVES install steps (WebKit/GTK + prebuild). But the **validation blast radius is real** (codex):
  the rc is a real prerelease PUBLISH using real signing/notary + `contents: write` — it pushes a public rc tag + a
  GH prerelease. It does NOT touch the prod S3 feed / `bump-source` (prerelease-skipped) → real 1.0.x users
  unaffected, but it's a real publish (fresh rc.N each run), not a sandbox.
- **Least privilege:** no new secrets/permissions; both legs install strictly less. The release job keeps its
  scoped OIDC/token set untouched.
- **Supply chain:** `bun install --frozen-lockfile` stays (Part A keeps the e2e's install; Part B's composite
  keeps it). The pinned-checksum bb gate is Windows-only + untouched (release build-headless is mac/linux). The
  removed `src-tauri` patch is dead code, not a control.
- **Dead-code removal risk:** removing the `src-tauri` version patch is safe ONLY if build-headless truly never
  compiles src-tauri — verified (server → core, Phase 2). The rc dry-run confirms `--version` + `/health` still
  correct.

## Assumptions
**Facts (verified):** build-headless uses the composite w/ rust-target (4-platform matrix on matching runners);
its src-tauri version-patch is dead post-extraction (server builds core, not src-tauri); /health.version =
injected server CARGO_PKG_VERSION; run-prebuild:false skips the host-assert (safe — matrix guarantees host==target);
_e2e.yml installs WebKit/GTK but only builds the headless server; libssl-dev + the prebuild must stay in _e2e.
**Inferences (attack):** dropping WebKit/GTK doesn't break the _e2e headless build (high — same as Phase-3b
Smoke/Release-Smoke, no GUI tree); the matrix runner↔target pairing makes the host-assert skip safe (high);
removing the src-tauri patch has no effect on the desktop `build` job (high — separate job/runner/checkout).
**Asks (RESOLVED):** split into 2 PRs (e2e PR-gate, build-headless rc-gated); validation = PR-gate (A) + owner rc
(B); /harden = no. No open asks.

## Decision ledger
- **Approach A (surgical inline edits) — CHOSEN; both auditors.** B (route `_e2e` through `setup-accelerator`)
  would kill the 3-copy apt drift, BUT `_e2e` already gets Bun/cache/`--frozen-lockfile` from `setup-aztec` and
  hand-rolls a release-adjacent setup ordering (aztec-node + start-services before the accelerator block,
  BB_BINARY_PATH/AZTEC_BB_VERSION via `$GITHUB_ENV`, a matching rust-cache key) → folding into the composite layers
  a 2nd bootstrap for marginal gain. A's one-line `:49` edit is trivially safe + self-proves. **Rejected: B →
  future de-drift follow-up.**
- **`src-tauri` version-patch removal — VERIFIED safe** (both): `server/Cargo.lock` has no `aztec-accelerator`
  stanza; the server builds `core` only; `/health.version` + `--version` read the patched server version. Both
  stale comments (the `/health` "shared lib" one + the `--locked` rationale) fixed in Part B.
- **rc framing corrected** (codex): it's a real prerelease PUBLISH (tag + GH prerelease), not a dry-run; fresh rc.N
  per run; prod feed untouched (S3/bump-source prerelease-skipped).
- **No unresolved disputes.**

## Audit trail
- **Codex (xhigh):** `A; holds-with-changes` — 2 HIGH (rc is a real prerelease publish, not a dry-run; rc.N must be
  fresh per run) + dead-src-tauri-patch + the `--locked` comment + host-assert-via-matrix-discipline. ALL folded.
- **Opus subagent:** `A; holds-with-changes` — verified the src-tauri patch dead (no `aztec-accelerator` in
  server/Cargo.lock); the stale `--locked` comment; A>B (e2e setup ordering); prebuild/host-assert safe. ALL folded.
- **Final fresh-context codex:** `conditional approve` — "everything else lines up with the folded fixes." 1
  condition, FOLDED: correct Part B's rc-evidence wording (the updater-smokes validate the DESKTOP artifacts, not
  the headless `accelerator-server-*` tarballs; rc-green proves build/package/`--version`/prerelease-inclusion for
  headless, not extracted-runtime/`/health`). Explicitly verified: Part A merge-green on PR gate; the 4 Part-B
  edits (incl. `--locked` stays off because `server/Cargo.lock:31` has the `accelerator-server` stanza); fresh-rc.N
  (tag-skip + release-recreate); prerelease blast radius (no S3/feed/bump-source); host-assert skip safe by matrix.

## Seeds
**Recommended — `/goal`** (Part A is PR-gate-observable; Part B stops at the owner-only rc):
```
/goal Part A + Part B marked ✓ in implementations-plan/headless-slim-followups-2026-06-08/plan.md; for each, the agent printed `LESSONS_FILE=implementations-plan/headless-slim-followups-2026-06-08/lessons/phase-N.md` in the transcript; `bun run lint:actions` reports exit 0; Part A's PR shows the SDK E2E green and is mergeable; Part B is committed + pushed with the rc-validation SURFACED for the owner (do NOT dispatch the release rc — owner-only); `/code-review max --fix` applied+committed and codex post-impl audit clean (or high/critical addressed). Never merge to main without green CI; never dispatch a release.
```
**Alternative — `/loop`** (self-paced): see `eli5.html`.
_Use exactly one per session — they don't compose._
