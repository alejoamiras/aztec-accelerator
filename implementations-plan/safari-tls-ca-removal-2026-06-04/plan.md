# Safari-HTTPS: remove the CA *minting key* (audit HIGH, certs.rs) — discarded-key design

**Tier:** `/plan deep` — 3 parallel drafts → consolidated → final codex audit (needs-rework) → **reworked
to the discarded-CA-key approach** (owner-chosen). Verdicts inline.
**Status:** ✅ **IMPLEMENTED** (branch `feat/tls-discard-ca-key`). All phases done; codex post-impl
(high-critical) all addressed; `cargo test --lib certs` 6 passed, `bun run test` + `lint:actions` exit 0.

### Phase checklist
- [✓] **Phase 1** — `certs.rs` generates CA+leaf, **discards the CA key** (never written); `ca_params`/
  `leaf_params` dedup; `certs_exist` validity-checked; atomic `0o600` `write_pem_file`.
- [✓] **Phase 2** — staging-swap `rotate()` (fresh keyless CA, pre-expiry ≤30d, add-new-anchor → verify →
  atomic swap → remove-old-anchor by SHA-1; fail-closed); renewal runs off the startup path (non-blocking);
  rcgen `zeroize`.
- [✓] **Phase 3** — `migrate_legacy_ca_key()` wired into startup (deletes legacy `ca.key`); `try_start_https`
  recovers (reset Safari) on a broken/mismatched cert set instead of silently wedging HTTPS.
- [✓] **Phase 4** — unit tests: `generation_writes_no_ca_key` (the security invariant) +
  `migrate_deletes_legacy_ca_key_but_keeps_certs`. macOS keychain integration validated by the **owner's
  Mac recipe** (touches the real keychain + prompts → can't be safely automated in CI).

## Problem
For Safari localhost HTTPS, `certs.rs` makes a root **CA** + a leaf signed under it, and trusts the CA in
the macOS keychain (`add-trusted-cert -r trustRoot`, certs.rs:244). **`ca.key` is a readable plaintext
`0o600` file** — a same-user process can read it and **mint browser-trusted certs for any domain**. The
*persistent CA signing key on disk* is the HIGH-severity primitive.

## Goal & approach (reworked after the codex final audit → needs-rework)
The original "no CA at all / self-signed leaf trusted directly" design was **rejected**: Apple's current
local-network-TLS guidance recommends a *CA*, and Safari may not honor a directly-trusted non-CA leaf
(`verify-cert` is only a preflight, not proof). So instead of removing the CA, **remove the CA's persistent
key**: keep generating a CA + leaf and trusting the CA (the proven, Safari-certain path), but **discard the
CA private key in-memory right after it signs the leaf — never write `ca.key` to disk.** Result: the
trusted anchor is a CA cert that *cannot sign anything* (no key exists), so the mint-any-cert primitive is
gone, with **zero Safari-acceptance risk**.

## Owner decisions
Discarded-CA-key (keep CA cert, drop its key) · forward-only for the *legacy* keychain anchor (but DELETE
the on-disk legacy `ca.key`) · key material = hardened files · automated cert-flow testing only.

## Key fact (opus research, HIGH confidence)
Apple's **825-day TLS validity cap applies** even to user-trusted certs → leaf validity **824 days**;
rotation is required ~every 2 years (a fresh CA+leaf each time, since no CA key is kept). Sources: Apple
103769, certkit.io, Špaček thread.

---

## Phases

### Phase 1 — certs.rs: generate CA+leaf, then DISCARD the CA key
- `ca_params()` + `leaf_params(now)` helpers (dedup the SAN/EKU/validity blocks at certs.rs:67-87 vs the
  rotation copy at 200-228; leaf validity = **824d**).
- `generate_and_save()`: generate CA keypair **in memory** → CA cert → leaf keypair → leaf cert signed by
  the CA → write **ca.pem (cert only) + localhost.pem + localhost.key**. **Never write `ca.key`** (drop the
  CA `KeyPair` at end of scope). The trusted anchor (`ca.pem`) is now keyless.
- Hardening (fold in the MEDs/LOW): `write_pem_file` atomic (temp + `0o600` + `rename`); `certs_exist()`
  parses the leaf + checks `not_after` is future (not just `.exists()`); `0o600` unconditional; reuse the
  parse helper in `load_rustls_config()`.

### Phase 2 — Trust + rotation (anchor cleanup + pre-expiry)
- `install_ca_trust` / `is_ca_trusted` **unchanged in mechanism** — `-r trustRoot` of `ca.pem` is correct
  (it IS a CA) and already Safari-proven. No `trustAsRoot`, no make-or-break.
- `regenerate_leaf_if_expiring()` → **rotate** (codex #2/#3): it can no longer re-sign under the old CA
  (no key), so it regenerates a **fresh CA+leaf** (discard new CA key), then: **prompt + install the new CA
  anchor → verify → swap the served cert → REMOVE the old CA anchor** (`security remove-trusted-cert`/
  `delete-certificate`) so anchors don't accumulate. **Pre-expiry:** trigger at **≤30 days remaining**
  (while the old leaf still serves), re-prompt periodically; if the user defers, keep serving the old
  still-valid leaf — only true expiry stops HTTPS. Never silent, never serve an untrusted cert
  (generate→trust→verify→serve, add-new-before-remove-old, fail-closed on cancel).

### Phase 3 — Migration (neutralize the HIGH at rest, incl. existing users)
`migrate_legacy_key()` early in startup: if `ca.key` exists on disk, **delete it** (the readable minting
key — closes the HIGH for existing installs; their CA cert keeps serving the existing leaf, now keyless
until the next rotation regenerates under the new model). Leave the legacy keychain anchor (owner) +
opt-in Settings "Remove legacy Aztec CA". Correct the prior assumption: the CA *key* was read by
`regenerate_leaf_if_expiring` (certs.rs:204) — that reader is replaced by fresh regeneration.

### Phase 4 — Tests (automated cert-flow only — no Safari gamble needed now)
Unit: `ca_params`/`leaf_params` (824d, NoCa leaf, SANs); generate writes ca.pem+leaf but **NOT ca.key**;
leaf loads into rustls (`with_single_cert`); `certs_exist` false on expired/corrupt/mismatched; atomic
write + `0o600`; `migrate_legacy_key` deletes ca.key, keeps serving; rotation produces a new CA fingerprint
+ removes the old anchor. macOS integration (`#[cfg(target_os="macos")]`, serial): install `trustRoot` →
`verify-cert` passes (now a *confirmation*, not a make-or-break) → rotate → old anchor gone, new trusted.
E2E: the macOS updater feed-server/smoke is a *separate* trust boundary (CA-backed fake prod host, per
codex #5) — leave it, but add a cert-flow regression that the served leaf has no `ca.key` on disk.

---

## Assumptions

### Facts (verified)
- CA+leaf+trust + silent rotation + leaf-served: certs.rs:46,71,108,147,182,244. The CA **key** is read by
  `regenerate_leaf_if_expiring` (certs.rs:204) — corrected from the prior draft (codex #4). Call sites:
  commands.rs:146/148, main.rs:70/75. Linux/Windows trust = stubs.
- 825-day cap applies (opus research). rcgen ECDSA-P256, rustls (existing deps).

### Inferences (attack these)
- Discarding the CA key in-memory after signing leaves a fully-valid leaf + a keyless trusted CA anchor;
  Safari trusts the CA root exactly as today (no behavior change — only the on-disk key disappears).
- Rotation regenerating a *new* CA each 2yr + removing the old anchor avoids accumulation without breaking
  a live session (add-new-before-remove-old).

### Asks (resolved)
- Approach (discarded-CA-key), forward-only legacy anchor + delete legacy key, hardened files, automated
  testing — all decided. No open unknowns (the Safari make-or-break is gone — we keep a real CA).

---

## Security & Adversarial Considerations
- **Win:** no CA signing key is ever written to disk → no readable mint-any-cert primitive, for new AND
  migrated installs (delete legacy `ca.key`). Safari trust unchanged (real CA, `trustRoot` — Apple-aligned).
- **Residuals (documented):** (a) a `ca.key` copied *before* upgrade still works until the legacy keychain
  anchor is removed (opt-in cleanup); (b) rotation leaves keyless CA anchors — mitigated by removing the
  old anchor on each rotation. (c) The brief in-memory CA key during signing is never persisted (zeroize
  best-effort; rcgen `KeyPair` drop).
- **Rotation hygiene:** pre-expiry (≤30d) prompting, non-silent, add-new-before-remove-old, fail-closed —
  no outage cliff, never serves an untrusted cert.
- **Crypto:** rcgen ECDSA-P256, rustls — no hand-rolled crypto. **Least privilege:** keys `0o600` atomic,
  dir `0o700`; leaf EKU `ServerAuth`, SAN localhost only.

---

## Audit verdicts
- **3 parallel drafts:** converged on bare-leaf + delete-ca.key + the discarded-CA-key fallback.
- **Final fresh-context codex (b1xr297zd):** **needs-rework** — bare-leaf is risky (Apple recommends a CA;
  `verify-cert` ≠ Safari proof), fallback anchor-accumulation, rotation cliff, the CA-file-reader
  assumption error. **All addressed by this rework** (flip to discarded-CA-key = codex's safer path; +
  anchor cleanup, pre-expiry prompting, corrected assumption). Transcript: `audit-codex.md`.
- **Confirming codex pass on the reworked plan:** _offered (recommended before implementation)._

---

## Seeds

### /goal
```
/goal All phases ✓ in implementations-plan/safari-tls-ca-removal-2026-06-04/plan.md (generate_and_save writes ca.pem+leaf but NEVER ca.key — CA key discarded in-memory after signing; trustRoot unchanged; rotation regenerates a fresh keyless CA, pre-expiry ≤30d, add-new-before-remove-old-anchor, fail-closed; migrate deletes legacy on-disk ca.key; certs_exist validity-checks + atomic 0o600 write + ca_params/leaf_params dedup; cert-flow unit incl. "no ca.key on disk" + macOS integration verify-cert/rotation green); per phase LESSONS_FILE printed; /code-review max --fix applied + committed; codex post-impl audit clean (or high/critical addressed); bun run test + bun run lint:actions exit 0. No stable release.
```

### /loop
```
/loop Each turn: read implementations-plan/safari-tls-ca-removal-2026-06-04/plan.md + lessons/; git status; PR? gh pr view --json statusCheckRollup. CI? gh run watch ≤10min. Failed? fix, /codex xhigh if non-trivial, commit+push; stop after 5 fails. Phase green? mark ✓, file lessons/phase-N.md, print LESSONS_FILE, advance. Next pending? edit → cargo test + bun run test → bun run lint:actions → commit → push. All ✓? /code-review max --fix → commit → codex post-impl audit (adversarial+security) → address high/critical → stop. NEVER merge to main or release autonomously. Security invariant: ca.key is NEVER written to disk; rotation removes the old CA anchor; verify with `ls ~/.aztec-accelerator/certs` (no ca.key) + an integration test.
```
