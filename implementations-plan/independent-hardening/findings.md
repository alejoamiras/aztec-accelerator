# Findings — independent-hardening (Phase 1 static)

Status key: `candidate` → verified/refuted in Phase 2. Confidence: high/med/low.
Namespace: IH-SEC-N (security), IH-BUG-N (correctness). No prior-audit artifacts consulted.

## Reviewed clusters

- **C2 ingress** (`server.rs`, `bind.rs`, `host.rs`, `auth.rs`, `probe.rs`, `prove.rs`): full read.
  Host guard fails closed (absent/disagreeing authority), exact-port loopback literals only,
  userinfo rejected; Content-Length RFC 7230 discipline; body caps + inflight semaphore(8) +
  slowloris deadline decoupled from prove permit; /health origin-tiered.
- **C3 authorization** (`authorization.rs`): full read of canonicalization + approval logic.
  IDNA/punycode via `url`, trailing-dot rejection, extension-ID grammar pinning, exact-match
  approvals, localhost auto-approve variants incl `[::1]`.
- **C6 bb supply chain** (`bb.rs`, `versions/{cache_layout,version_policy,release_metadata,downloader}.rs`):
  targeted deep reads. Version grammar blocks traversal at the type level; fail-closed home
  resolution; marker rehash on every cached execution; private per-user prove tempdir (0o700);
  process-group containment + stderr cap; saturating size math.
- **C4 PKI** (`certs.rs` targeted): CA private key never persisted ("keyless"); structural
  `validate_ca_profile` (CN/CA:true/keyCertSign/critical nameConstraints→loopback) gates OS trust
  install; staged→live atomic rename.
- **C5 updater** (`updater.rs` targeted): signed manifest binds advertised version to artifact set
  BEFORE download; monotonic floor ANDed with non-forgeable bind-ownership signal; cross-process lock.
- **C7 IPC** (`commands.rs` targeted): every command label-gated; `respond_auth` bound to
  request-id-derived window label + server-side active-slot arbiter.
- **C8 autostart/config** (targeted): plist heal parses real XML, replaces ProgramArguments[0] only;
  generation XML-escaped. Config: typed Speed enum, strict deserializers, read-only fallback.

## Candidates

### IH-SEC-1 (Low, inherent) — unauthenticated localhost service: port-squat witness capture
**CONFIRMED LIVE (Phase 2)** · confidence high · **CLOSED AS DOCUMENTED 2026-08-21** — accepted trust boundary; both future levers recorded in the security report
While the accelerator is NOT running, any local process (any user on multi-user machines — TCP
binds are system-global) can hold 127.0.0.1:59833/59834. The SDK's probe accepts any responder
whose `/health` matches `{status:"ok",api_version:1}` (collision resistance, not authentication —
the code says so itself), then POSTs the private witness. Same-user malware needs no squat at all.
HTTPS does not help against same-user attackers (leaf key on disk; CA installable by user).
**Not fixable without client-cert/pinning redesign; document as trust boundary.**
PoC: `poc/ih-sec-1-squat.py` — squatter answered the health shape check and captured the
witness POST byte-for-byte (`WITNESS CAPTURED BYTE-FOR-BYTE`, 2026-08-21 run).

### IH-SEC-2 (Info, documented tradeoff) — absent Origin header ⇒ auto-approved /prove
**CONFIRMED LIVE**: no-Origin POST /prove passed authz on headless server (reached prove stage,
500 bb-not-found); `http://LOCALHOST:9999` variant also passed (case-insensitive localhost
auto-approve); `evil.localhost` correctly denied; `null`/garbage → 400 invalid_origin.
Design-consistent; no action.

### IH-SEC-3 (Info) — CORS `allow_origin(Any)` lets any website read /health minimal body + /prove errors
**PARTIALLY VERIFIED LIVE**: unapproved Origin gets minimal `{api_version,status}` only
(fingerprint starvation working). /prove error bodies pre-approval echo only attacker-chosen
inputs (origin, requested version). No secret leakage found. No action.

### IH-BUG-1 (Info, comment/code divergence) — host.rs strips ALL trailing dots, comment says one
**CONFIRMED LIVE**: `Host: 127.0.0.1..:59833` → 200. **FIXED** in `ih-hygiene` PR: strip-once +
raw `]`+port check (Authority::parse silently discards post-bracket junk); multi-dot forms → 403.

## Phase-2 attack matrix summary (headless server, deny-by-default, 2026-08-21)
- Host guard: 8/10 malicious variants → 403 invalid_host; `[::1]`/uppercase/multi-dot accepted
  (all loopback-by-design — PRE-FIX behavior; multi-dot since rejected by the `ih-hygiene` PR).
  Hex/decimal IP, userinfo, wrong-port, rebinding names all rejected.
- Body limits: declared oversize → 413; malformed/conflicting Content-Length → 400.
- Version header traversal string → 400 invalid_version (type-level grammar holds at runtime).
- Remote-controlled download path observed live (`x-aztec-version: 1.0.0` triggered a GitHub
  fetch; client timed out at 4s) — bounded by cache cap + digest verification (static).
- TLS listener (59834): desktop-app-only, not exercised live this phase — static review only.

### Verified-clean notes (explicitly checked, no finding)
- Dual-listener port confusion: each listener gates on its own expected_port (router_for_port);
  tests cover wrong-port rejection. Phase 2 re-verifies live.
- Downloader digest: fetched from GitHub release asset metadata (Aztec's release = trust root,
  same host as artifact — self-referential by design; marker rehash catches post-install drift).
- `x-aztec-version` remote-controlled downloads bounded by cache-size cap + revocation denylist
  checked before bundled short-circuit.
- prove tempdir: per-user data-local parent, owner-only, Windows refuses shared-temp fallback.
- Updater verify-before-download ordering enforced by VerifiedUpdate capability type.
