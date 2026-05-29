# Phase — decouple + harden the updater gate (post-codex)

**Trigger:** the `1.0.3-rc.2` dry-run never exercised `update-smoke`. The
`macos-15-intel` build leg flaked **3× in a row** on Tauri's `bundle_dmg.sh`
(zero output, ~11s; the app signed + notarized fine — only the DMG wrapper
died). arm64 built its DMG fine from the same commit; the 1.0.1 release hit the
same Intel flake. Because `update-smoke needs: [validate, build]` (the whole
matrix), one Intel DMG flake skipped **both** update-smoke legs — including the
healthy arm64 one. "Re-run and pray" was failing.

## Codex review (session 019e74dd-…, xhigh, read-only)

Asked for an adversarial pass on whether the gate has teeth + the flake. Verdict:
"useful signal, but not a release-grade guard; partly real coverage, partly
theater." Verified each claim against the repo:

1. **[HIGH / NO-TEETH]** N-1 (`1.0.2`) and N (`1.0.3-rc`) are *both post-fix*, so
   green only proves good→good relaunches — it never proves the gate would
   *catch* a 1.0.1-style hang. The deterministic guard for the exact 1.0.1
   regression is the **bundle-shape invariant**, not this smoke. A real teeth
   check needs a *validly-signed-but-bad* bundle, which needs the prod key we
   don't have → only a CI-trust-domain (ephemeral CI key) could do it. **Cheaper
   partial teeth we CAN do with no key: a corrupt-`.sig` negative control.**
2. **[HIGH / FALSE-PASS]** Stripping quarantine + bare-exec is less faithful than
   a real quarantined/Finder launch — BUT codex also noted (and I verified
   against tauri `process.rs`) that Tauri's `restart()` is a **bare `Command::
   spawn` of the bundle executable, not LaunchServices `open`**. So the relaunch
   (the actual 1.0.1 hang point) is faithful as-is. See decision below.
3. **[HIGH / CORRECTNESS]** Workflow coupling is wrong: `update-smoke` (and
   `smoke`) depend on the whole `build` matrix though each arm64 leg only
   consumes arm64 artifacts; and `.app.tar.gz`/`.sig` are produced in the same
   `tauri build` step as the flaky DMG.
4. **[MEDIUM / FALSE-PASS]** `_e2e-updater.yml` picks N-1 = "latest stable" at
   runtime; once `1.0.3` is stable, a re-run gets N-1 == N and `/health == N`
   passes trivially.
5. **[MEDIUM]** `macos-15-intel` flake is *not* Tauri-specific — generic GH-runner
   `hdiutil`/`create-dmg` flakiness (actions/runner-images#7522). Retry the DMG
   step, not the whole build.
6. **[LOW]** Cleanup never removes the trusted test CA (fine on ephemerals only).

Verified-feasible: `tauri build --no-bundle` / `--bundles` and a standalone
`tauri bundle --bundles dmg --verbose` all exist in our CLI (`@tauri-apps/cli ^2`).

## Decisions (user picked "Pragmatic")

Implemented on `ci/updater-gate-decouple-harden`:

- **Split macOS build** (`release-accelerator.yml`): `tauri build --bundles app`
  (compile + sign + **notarize once** + emit `.app.tar.gz`/`.sig`, no `hdiutil`)
  → bundle-shape invariant → `tauri bundle --bundles dmg --verbose` wrapped in a
  **×3 retry** (only the flaky step retries; no re-notarization). Guard asserts
  `--bundles app` actually emitted the updater artifacts (fail fast if my
  assumption is wrong). Linux unchanged. Fixes #3 + #5.
- **Decouple update-smoke** (#3): `if: ${{ !cancelled() && needs.validate.result
  == 'success' }}` so the arm64 leg validates even when the Intel build leg
  fails. (Did NOT split artifact names — the `release` job requires dmg+tar.gz in
  one artifact; splitting risked the release path for little gain once the DMG
  self-heals.)
- **Rerun guard** (#4): `_e2e-updater.yml` aborts if N-1 tag matches the version
  under test.
- **Feed-log assertion** (#4-ish): feed server logs served requests;
  `updater-smoke.sh` positive mode requires both `latest.json` + `download/`
  were hit (no no-op pass); negative mode requires `latest.json` was hit (no
  vacuous pass).
- **Negative control / partial teeth** (#1): new `negative` mode + a 3rd
  update-smoke matrix leg (arm64). Serves a `rev`-corrupted `.sig`; asserts the
  updater **rejects** it (/health never reports N). Proves the gate *can* fail.
- **CA cleanup** (#6): cleanup trap now `security delete-certificate` the test CA.

### DEVIATION from codex — dropped the headless `open`/quarantine faithfulness leg

Codex suggested adding a quarantined + LaunchServices-`open` leg. I did **not**,
and the reasons are verified, not lazy:

- CI's N-1 is `gh release download`-ed → it carries **no** `com.apple.quarantine`
  xattr to begin with (quarantine is applied by browsers/LaunchServices, not by
  `gh`). There is no real quarantined state to reproduce without *synthesizing*
  one.
- Tauri `restart()` is a **bare spawn** (verified in `process.rs`), so the
  relaunch hits amfid identically whether or not the first launch used `open`.
  The hang point is faithful already.
- The existing `smoke` job *and* the WebDriver E2E both deliberately launch via
  bare exec, not `open`, because LaunchServices/Gatekeeper is unreliable on
  GUI-less hosted runners.

Net: an `open` leg adds flake risk for ≈zero coverage gain over what the bundle-
shape invariant + the bare-spawn relaunch already cover. Full teeth (catching a
*validly-signed* bad bundle) would need an ephemeral CI signing key + a CI-keyed
N-1 — deferred; noted as the only thing that converts this from "plumbing +
sig-rejection" to "proven detection of a 1.0.1-class hang."

## Validation
- `shellcheck` (updater-smoke.sh), `actionlint` (release + _e2e-updater), `biome`
  (feed server) all clean locally.
- Next: PR → re-trigger `1.0.3-rc.3` dry-run → confirm Intel build self-heals,
  both positive update-smoke legs go green, and the negative leg goes green
  (rejection). Bound: 3 dry-run attempts per root cause.
