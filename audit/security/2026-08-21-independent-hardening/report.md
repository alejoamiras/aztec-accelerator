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
**Recommendation**: document as an explicit trust boundary in README/security docs. A real fix
(shared secret minted at first-run, or SDK-side pinning of the accelerator's leaf cert) is a
protocol change — worth a roadmap item, not a patch.

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
