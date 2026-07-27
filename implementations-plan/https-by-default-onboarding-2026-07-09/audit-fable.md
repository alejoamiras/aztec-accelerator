# Fable audit — HTTPS-by-default + onboarding plan

Two legs: (0) an independent planning leg folded into plan.md during consolidation; (1) a fresh hostile audit of the consolidated plan (below), run with no prior context to avoid framing anchoring.

## Round 1 — fresh hostile audit

**Verdict: conditional approve.** Conditions: (1) guard the NSIS POSTUNINSTALL hook with Tauri's `$UpdateMode` + assert trust survives an update — must be right in the FIRST hooked release; (2) SDK "HTTPS wins iff `response.ok`" with a test for https-non-OK + http-OK; (3) re-scope the macOS real-trust CI leg — spike I7, user-domain trust can't be set non-interactively; (4) surface the Linux no-OS-dialog consent as an explicit Ask; (5) add real chain validation (`certutil -V` / `certutil -verify`) to trust CI + disclose snap/flatpak Chromium.

### CRITICAL
- **C-1 — NSIS uninstall hook nukes trust on every auto-update.** Tauri runs the previous version's uninstaller when installing over an existing install (that's why the template exposes `$UpdateMode`). Unguarded, `NSIS_HOOK_POSTUNINSTALL` deletes the anchor + certs dir on every update → `MissingCertsReset` → HTTPS silently off after each update. The hook ships baked into release N's uninstaller, so the first hooked release must be correct. `update-smoke-windows` asserts update success, not trust state. Fix: guard with `${If} $UpdateMode <> 1` + assert anchor/certs survive an update.

### HIGH
- **H-1 — macOS real-trust CI leg mis-designed.** `add-trusted-cert` records trust in the user trust-settings DOMAIN (needs interactive auth), not the keychain; a temp keychain doesn't dodge it → `errSecAuthorizationDenied`/hang on headless runners. mkcert's precedent is `sudo ... -d` (admin domain, System keychain) = different code path from the app's login-keychain/user-domain flow. Invert confidence: spike I7, not just I3; declare a fallback (admin-domain-under-sudo test-only switch, or arg-construction + runbook).
- **H-2 — SDK "HTTPS wins if fulfilled" too coarse.** ky `throwHttpErrors:false` → a 404/500 counts as fulfilled; a foreign server squatting fixed port 59834 that answers non-2xx HTTPS deterministically beats the accelerator's healthy HTTP `/health`. Rule must be "HTTPS wins iff `response.ok`". Also the "250 ms grace only paid by our own stalled listener" claim is false — any foreign process on 59834 that stalls the handshake charges 250 ms every re-probe (per 10 s cache TTL). Core zero-latency claim (I4) otherwise survives (127.0.0.1 literal, no happy-eyeballs; loopback RST instant).

### MEDIUM
- **M-1 — Linux has NO consent ceremony**, contradicting §1's "OS dialog is the consent ceremony." One click on a pre-checked toggle silently writes a root anchor into every NSS store + Firefox profile — weakest consent on the OS with the most writes. Resolution is a product decision → new Ask.
- **M-2 — snap/flatpak Chromium blindness.** Handles snap/flatpak Firefox but not sandboxed Chromium (Ubuntu chromium is snap-only, confined). I1's core claim is correct (Chrome Root Store still honors locally-managed anchors via NSS user DB), but `trust_status` must enumerate/disclaim sandboxed Chromium like it does snap Firefox.
- **M-3 — real-trust CI never validates a chain.** `certutil -L`/`-verifystore` only prove the anchor is present, not that a leaf validates through the name-constrained anchor. Add `certutil -V -u V -n <leaf>` (NSS) + `certutil -verify localhost.pem` (Windows) to convert I5 from "assume" to pinned. Acceptance risk itself Low (macOS ships this CA in prod today = existence proof).
- **M-4 — D4: delete by SERIAL, not thumbprint/SHA-1.** Old anchor's serial is parseable from old `ca.pem` via `x509-parser` (already a direct dep) — locale-proof, no `sha1` crate, no bookkeeping drift. certutil accepts a serial as certID. Supersedes both listed D4 options.
- **M-5 — every existing user gets the wizard retroactively** (`onboarding_version` default 0). Re-interrupting the whole installed base is a product call → new Ask.

### LOW
- **L-1** — `CONFIG_VERSION` 1→2 bump is decorative (nothing reads `config_version`; the alias does all the work); "fail-safe" framing on duplicate-key configs should say "matches existing malformed-config policy," not imply added safety.
- **L-2** — Windows re-adding an already-present root shows no dialog (consent edge, reachable only via residue). `complete_onboarding` marking done even when every action failed suppresses the wizard forever → make it a stated decision.
- **L-3** — §2 "launch stays prompt-free" vs §7 renewal window at launch: OS-dialog invariant holds, but a launch-time window is a "prompt" to users; copy overstates.
- **L-4** — release-mode 3-OS WebDriver is unproven flake/cost surface (only macOS has run release-mode).
- **L-5** — citation nits: SDK file is `packages/sdk/src/lib/accelerator-transport.ts`; `installMode:currentUser` is `tauri.conf.json:51`, not the release workflow. Substance correct.
- **L-6** — Firefox: cert9.db needs restart (I6 correct); Windows Firefox 120+ imports OS-store roots by default → CurrentUser Root covers Windows Firefox (add to coverage matrix).

### §9 buckets
- Facts: all verified; two citation nits (L-5).
- Inferences: I1 partially unsafe (M-2); I2 asks the wrong question (C-1); I3 rightly spiked; I4 safe with non-OK carve-out (H-2); I5 safe in direction, untested functionally (M-3); I6 correct; I7 unsafe (H-1); I8 plausible but bundled-content ≠ XSS-proof.
- Asks: add Linux-consent-ceremony + retroactive-wizard; resolve A5 as delete-by-serial.
