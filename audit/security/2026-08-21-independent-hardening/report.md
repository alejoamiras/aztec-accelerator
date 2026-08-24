# Security Report — independent-hardening

**Date**: 2026-08-21 · **Auditor**: ox-alpha (solo) · **Base**: main @ `9eff8dc`
**Method**: fresh adversarial pass — no prior audit artifacts consulted; source tracing + live
loopback red-team against a locally built headless server.

## Scope & coverage

| Area | Depth |
|---|---|
| C2 ingress (`server.rs`, bind/host/auth/probe/prove) | full read + live matrix |
| C3 authorization (`authorization.rs`) | full read of canonicalization/approval + live probes |
| C6 bb supply chain (`bb.rs`, `versions/*`) | targeted deep reads (spawn, paths, digests) |
| C4 PKI (`certs.rs`, `trust/linux.rs`) | targeted (CA lifecycle, Linux backend); mac/windows backends spot-checked |
| C5 updater (`updater.rs`, `update_manifest.rs`) | targeted (verify ordering, manifest binding) |
| C7 IPC/state (`commands.rs`, `crash_recovery.rs`) | targeted (label gates, parse robustness) |
| C8 OS edges (`autostart.rs`, config) | targeted (plist heal, strict deserialization) |
| SDK transport/prover | targeted (host/port validation, body caps, probe logic) |

Not exercised live: HTTPS listener (desktop-only), Windows/Linux-specific backends at runtime
(source-level only), src-tauri Windows-GNU compile (requires mingw toolchain absent on this Mac;
CI's windows-build lane covers it). Core crate compiles clean for x86_64-pc-windows-gnu.

## Findings

### IH-SEC-1 · Low (inherent design boundary) — port-squat witness capture
**Confirmed live (PoC).** While no accelerator instance holds 127.0.0.1:59833/59834, any local
process — any user on multi-user machines — can bind the port, answer `/health` with
`{"status":"ok","api_version":1}`, and receive `/prove` bodies containing private witness data.
Demonstrated byte-for-byte capture (`poc/ih-sec-1-squat.py`). The health check is collision
resistance, not authentication (the code documents this itself). Same-user malware needs no squat;
HTTPS does not change the same-user calculus (leaf key user-readable; CA installable by the user).

**RESOLUTION 2026-08-21 — accepted as a documented trust boundary.**
Residual exposure, stated plainly: *if the accelerator app is down and a dApp proves anyway,
the plaintext witness goes to whatever answers on 59833.* Rated Low because it requires downtime
plus a hostile local process; it remains true until one of the two levers below ships.

Future levers, recorded for when either becomes worth its cost (both were evaluated and
deliberately deferred):

1. **httpsOnly-by-default in the SDK** — the browser fix. TLS is the only server-auth mechanism
   the web platform offers, so this is the ONLY real mitigation for dApp consumers. Cost: Node/Bun
   users without `NODE_EXTRA_CA_CERTS=~/.aztec-accelerator/certs/ca.pem` get offline→WASM until
   they wire the CA (browsers need nothing — the CA is already in their trust store). Declined for
   now because that friction lands on real users to close a Low-severity, narrow-window risk.
2. **Session-auth challenge-response** — works only where files are readable (headless operators'
   tooling, future Node CLI consumers). Core would write an owner-only per-run session key;
   clients verify `HMAC(key, nonce)` on `/health` before trusting the endpoint. **Not implementable
   for browsers** — page JS cannot read the filesystem, so there is no client-side enforcer for
   the primary consumer. Deferred because no such non-browser consumer exists today; building
   crypto for a hypothetical caller is the over-engineering trap.

A third observation for whoever revisits this: the SDK already pins HTTPS once seen healthy and
refuses downgrade without opt-in (`accelerator-transport.ts::allowsHttpDowngrade`) — the residual
window is exactly "accelerator fully down", not "HTTPS briefly unavailable".

### IH-SEC-2 · Info (documented tradeoff) — absent Origin ⇒ auto-approved /prove
Confirmed live. Local non-browser callers bypass origin authz by omitting Origin; localhost
variants auto-approve case-insensitively (`http://LOCALHOST:9999` passed). Consistent with
IH-SEC-1's boundary; correctly denied `evil.localhost`, `null`, garbage origins. No action.

### IH-SEC-3 · Info — CORS `allow_origin(Any)` read surface
Unapproved cross-origin callers get the minimal `/health` body only (fingerprint starvation
verified live); pre-approval `/prove` errors echo only attacker-chosen inputs. No leak found.

## Verified-clean highlights (adversarially checked, held)

- Host guard fails closed on missing/disagreeing authority; rejects userinfo, hex/decimal IP,
  wrong-port, rebinding names — all re-confirmed live.
- Origin canonicalization: IDNA/punycode, trailing-dot rejection, extension-ID grammar pinning,
  exact-match approvals.
- Content-Length RFC 7230 discipline (malformed/conflicting/conflicting-list → 400/413, verified).
- Version grammar blocks path traversal at the type level (live-verified with `../../etc` payload).
- bb spawn: fixed args, per-user 0o700 tempdir, process-group containment, capped stderr.
- Cache integrity: fail-closed home resolution, marker rehash on every cached execution,
  digest-verified downloads, revocation denylist checked before bundled short-circuit.
- CA: keyless-on-disk generation; structural profile validation (CN, CA:true, keyCertSign,
  critical loopback nameConstraints) before any OS trust install; Linux backend resolves certutil
  to safe absolute paths and bounds Firefox profiles under canonicalized $HOME.
- Updater: signed manifest binds version→artifact set before download; monotonic floor ANDed with
  non-forgeable bind ownership; unique-URL matching defeats old-artifact confusion.
- IPC: window-label gating on every command; respond_auth bound to request-id label + server-side
  active-slot arbiter.

## Verdict

No exploitable vulnerability found above the inherent unauthenticated-localhost boundary
(IH-SEC-1). One roadmap-worthy protocol improvement suggested; everything else documentation-level.
