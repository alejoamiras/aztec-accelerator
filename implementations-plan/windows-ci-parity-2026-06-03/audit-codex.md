# Codex audit (round 1) — verdict: REWORK
Session: 019e... (codex-RLQWJD9n). Run 2026-06-03.

---

**Verdict:** rework.

1. **#95 targets the wrong lockfile and cites the wrong build path.** The plan says [`src-tauri/Cargo.lock`](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/Cargo.lock:258>) is stale, but it already records `aztec-accelerator 1.0.4-rc.1`; the stale lock is [`server/Cargo.lock`](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/Cargo.lock:258>). The `--locked` comment it cites is in the server build path, not the Tauri build ([`release-accelerator.yml`](</Users/alejoamiras/Projects/aztec-accelerator/.github/workflows/release-accelerator.yml:287>)). Follow the plan literally and you fix the wrong file.

2. **P5 would make a synthetic test release-blocking, not parity-blocking.** The Windows smoke still builds synthetic N-1 and synthetic N with an ephemeral key and only `needs: [validate]` ([`_e2e-updater-windows.yml`](</Users/alejoamiras/Projects/aztec-accelerator/.github/workflows/_e2e-updater-windows.yml:32>), [`release-accelerator.yml`](</Users/alejoamiras/Projects/aztec-accelerator/.github/workflows/release-accelerator.yml:510>)). That does not prove the shipped Windows artifact or prod-signing path the way mac/Linux do. Also the “real N-1” idea is misstated: current mac/Linux logic explicitly excludes prereleases, so `accelerator-v1.0.4-rc.1` is not a usable baseline without changing semantics.

3. **#96 Level 1 is not implementable as written.** There is no `autostart=true` field in `config.json`; config only stores `auto_update` ([`config.rs`](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/config.rs:41>)). Autostart/crash-recovery are OS/plugin side effects via `set_autostart()` and startup checks ([`commands.rs`](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:31>), [`main.rs`](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:259>)). So the plan’s core arming mechanism does not exist.

4. **Timeout-tightening is a band-aid, not a wedge fix.** With `concurrency.group=release-accelerator` and `cancel-in-progress:false`, a blocking Windows smoke still monopolizes the only release slot until timeout. Reducing 60→35/40 minutes lowers blast radius, but repeated flakes still self-DoS releases. It also promotes a privileged job that mutates `LocalMachine\Root`, `hosts`, Defender, and Task Scheduler into the release-critical boundary.

5. **The Level-2 SYSTEM spike needs stricter safety assumptions.** The evidence it cites is from `windows-2025`, but the smoke runs on `windows-latest`. On self-hosted runners, orphaned SYSTEM tasks or failed cleanup become real risk. “Run-unique name + finally” is not enough; require verified cleanup and GH-hosted-only.

**What looks fine:** the updater guard itself is well-defended: download first, refuse install if disarm can’t be verified, and re-arm on every app-still-running path ([`updater.rs`](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:64>)). The current Windows negative smoke also has real crypto teeth: it tampers the artifact, not the signature, and proves a download happened before calling rejection ([`updater-smoke-windows.ps1`](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/scripts/updater-smoke-windows.ps1:93>)).

---

# Codex audit (round 2, final fresh pass) — verdict: still-needs-rework → all fixes adopted

**Verdict:** `still-needs-rework`

**A vs B:** `B` is the right fork. The core trust-chain claim is sound: the real Windows build already emits prod-signed `*-setup.nsis.zip/.sig` from the real signing key in [`release-accelerator.yml`](/Users/alejoamiras/Projects/aztec-accelerator/.github/workflows/release-accelerator.yml:154), and the updater verifies against the app’s embedded pubkey in [`updater.rs`](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:61). A synthetic N-1 that embeds that same prod pubkey can therefore validate the real N. The only sensible “third option” is staged B: prove it advisory on the next rc, then make it blocking. Not A.

**Ranked concerns**
1. `#95` is still factually incomplete. [`server/Cargo.lock`](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/Cargo.lock:6) has **two** stale local package stanzas: `accelerator-server 1.0.2-rc.1` and [`aztec-accelerator 1.0.2-rc.1`](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/Cargo.lock:258). The revised plan only patches the first in [`plan.md`](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/windows-ci-parity-2026-06-03/plan.md:48). If executed literally, drift remains, and `bump-source` still misses the server lock’s path-dependency stanza.

2. `#96` Level 1 arming is still unsafe as written. Pre-creating only `HKCU\...\Run\aztec-accelerator` in [`plan.md`](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/windows-ci-parity-2026-06-03/plan.md:75) is likely a no-op. The autostart plugin uses app name = `productName` (`Aztec Accelerator`), not crate name, per [`tauri-plugin-autostart`](/Users/alejoamiras/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-plugin-autostart-2.5.1/src/lib.rs:178) and [`tauri-codegen`](/Users/alejoamiras/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-codegen-2.6.2/src/context.rs:268). Worse, `is_enabled()` also checks `StartupApproved\\Run`, not just `Run`, per [`auto-launch/windows.rs`](/Users/alejoamiras/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/auto-launch-0.5.0/src/windows.rs:37). Safe path: drive `set_autostart(true)` in-harness, or replicate both registry writes exactly.

3. Option B must **not** keep a synthetic fallback once blocking. [`plan.md`](/Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/windows-ci-parity-2026-06-03/plan.md:115) says to fall back if the build artifact is unavailable; that weakens the gate precisely when the real shipped artifact is missing.

4. Level 1 cleanup is missing. Current smoke cleanup in [`updater-smoke-windows.ps1`](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/scripts/updater-smoke-windows.ps1:57) does not disable autostart or delete the crash-recovery task. Armed-state tests need explicit disarm + verified deletion on exit.

**What’s fine**
- The B parity rationale is correct.
- Keeping Level 2 as a non-blocking spike/documented gap is reasonable.
- GH-hosted-only, revert runbook, and timeout headroom are the right release-safety controls.