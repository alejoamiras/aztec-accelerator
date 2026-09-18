# Phase 4 — Proving parity = the SDK-only decision gate (2026-06-18)

## Verdict: GREEN → SDK-ONLY CONFIRMED
The accelerator-backed e2e ran against a **local 5.0 sandbox** in CI (`sdk.yml` e2e, now `build_accelerator: true`) and proved the native path end-to-end:
```
(pass) AcceleratorProver > Accelerated > should deploy account through the NATIVE accelerator path (not WASM fallback) [20115ms]
(pass) AcceleratorProver > Local (WASM) > should deploy account with local proving (WASM fallback path) [18427ms]
7 pass / 0 fail / 18 expect()
```
The CI job steps confirm the real path: *Build headless accelerator server → Start → Wait for accelerator → Run SDK E2E tests*, all ✓. So the **deployed accelerator's `bb prove --scheme --ivc_inputs_path` against runtime-fetched 5.0 bb produces a proof the 5.0 network mines** — the bb CLI/msgpack/proof interface is parity-stable. **Appendix C (accelerator rebuild) is NOT triggered.**

## Code changes that made the gate real (closed the false-green)
- `e2e-helpers.ts`: v5 `send()` resolves post-inclusion with a real receipt → assert `!receipt.hasExecutionReverted()` (mined != succeeded).
- `proving.test.ts`: positive native-path assertion via the phase trail — `transmit` present (native /prove to :59833) + `fallback` absent. A silent WASM fallback would mine too, so "it mined" alone was insufficient. WASM leg asserts `fallback` engaged (proves the discriminator is meaningful).
- `sdk.yml`: e2e now runs `build_accelerator: true` → the native path is a **standing** gate on every SDK PR (future aztec bumps re-verify SDK-only automatically).

## Scope correction (honest)
"Windows bb SHA is Appendix-C-contingency-only" was **wrong**. The 4.3.1→5.0.0-rc.1 lockfile bump makes `resolveAztecBb()` key on 5.0.0-rc.1, so the accelerator's **Windows Prebuild Smoke** PR-gate fail-closed (no pinned SHA) — even for an SDK-only release. Fixed by pinning the `v5.0.0-rc.1` `barretenberg-amd64-windows.tar.gz` SHA-256 `7fd01446…c4dd` (bb.exe-only; no-DLL canary holds; CI re-fetches + verifies independently = the second source). This makes the accelerator *build green*; it does NOT trigger an accelerator *release* (still SDK-only).

**Gate:** PASS — native 5.0 deploy mined via :59833 (CI), Windows prebuild green.

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-4.md
