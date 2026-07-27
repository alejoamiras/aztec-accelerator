# Phase 2 — SDK https-preferred probe + httpsOnly

## Scope
Rewrite `AcceleratorTransport.probeHealth` to prefer HTTPS only when healthy (plan §4 / audit R2, H-2); add `httpsOnly` strict mode + `AZTEC_ACCELERATOR_HTTPS_ONLY`. Docs (SDK README + skill).

## Design (plan §4 / R2)
- **Healthy = 2xx + parseable JSON object.** `#isHealthy` clones the Response before parsing (R2 — never consume the body the prover's classifier needs downstream).
- **Two-race structure** in `#probePreferHttps`:
  1. *Leading edge*: `Promise.race([healthy-https-or-never, http-settled])`. A non-healthy HTTPS maps to a never-resolving promise so it can't beat a still-pending HTTP (fixes the "500-on-https squatter beats healthy http" bug, H-2). A healthy HTTPS wins the instant it appears — even before HTTP settles, and even if HTTP is hung.
  2. *Grace*: once HTTP settles OK, `Promise.race([httpsHealthy, delay(250)])`. A HTTPS that already settled (refused→null / unhealthy→null) short-circuits with no wait; only a *pending* HTTPS costs up to 250ms.
- **Zero added latency** when HTTPS absent: refused HTTPS → `httpsHealthy` resolves null ~0ms → grace returns immediately → HTTP wins. Asserted by a wall-clock < 150ms test.
- **Strict `httpsOnly`**: probeHealth fires only the https URL; `baseUrl` is always https (never constructs http, even pre-negotiation). Unreachable → rejects → caller maps offline.

## Notes
- Kept the existing 4 probeHealth tests — they still pass (their unparseable "ok" bodies now mean "https not healthy → http wins", same asserted protocol).
- No `https-fallback` phase event added: the adopted D6 (fable dual-probe) self-corrects the pin each probe, so there's no sticky-wrong-pin to signal. The old main-leg's demote-once/event idea was not the adopted design.

## Validation (all local — pure TS)
- ✅ SDK typecheck (`tsc --noEmit`): clean.
- ✅ SDK unit: **53 pass** (8 new: healthy-https-wins, latency-neutral <150ms, stall-grace ~250ms, https-500+http-ok→http, https-200-malformed+http-ok→http, clone-readable, httpsOnly-no-http-url, httpsOnly-offline-rejects).
- ✅ Full `bun run test`: biome (exit 0), pkg-sort, cargo fmt --check, SDK 53 + playground 73 + scripts 6.
- ⏳ SDK E2E (`test:e2e`, live accelerator): CI-only (sdk.yml).

## Branch hygiene
Discovered feat/ was branched off the unmerged LNA docs commit 46c34ee (PR #373). Excised it via `git rebase --onto origin/main 46c34ee` so PR #375 is self-contained and doesn't duplicate #373. Own draft branch, force-push-with-lease (allowed — no other human touched it).
