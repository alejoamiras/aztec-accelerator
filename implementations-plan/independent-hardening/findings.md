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
`candidate` · confidence high (design-level)
While the accelerator is NOT running, any local process (any user on multi-user machines — TCP
binds are system-global) can hold 127.0.0.1:59833/59834. The SDK's probe accepts any responder
whose `/health` matches `{status:"ok",api_version:1}` (collision resistance, not authentication —
the code says so itself), then POSTs the private witness. Same-user malware needs no squat at all.
HTTPS does not help against same-user attackers (leaf key on disk; CA installable by user).
**Not fixable without client-cert/pinning redesign; document as trust boundary.**
Phase 2: demonstrate squat-and-capture live against headless server.

### IH-SEC-2 (Info, documented tradeoff) — absent Origin header ⇒ auto-approved /prove
`candidate` · confidence high
auth.rs:29-33. Non-browser local callers bypass origin authz by omitting Origin. Documented as
inherent (Origin is a browser mechanism). Combined with Host-guard this is "any local process may
prove" — consistent with IH-SEC-1's boundary. No action; keep documented.

### IH-SEC-3 (Info) — CORS `allow_origin(Any)` lets any website read /health minimal body + /prove errors
`candidate` · confidence med
server.rs:351-358. Fingerprinting mitigated by SEC-05 tiering (minimal body for unapproved).
/prove error variants echo attacker-chosen inputs (InvalidVersion echoes requested version,
OriginDenied echoes origin) — no obvious secret leakage pre-approval. Phase 2: verify no
pre-authentication info leak via error bodies/timing cross-origin.

### IH-BUG-1 (Info, comment/code divergence) — host.rs strips ALL trailing dots, comment says one
`candidate` · confidence high
host.rs:36 `trim_end_matches('.')` vs doc "strip one trailing dot". `Host: 127.0.0.1..:59833`
accepted. Still a loopback literal → no security impact; fix comment or tighten to strip_once.

### Verified-clean notes (explicitly checked, no finding)
- Dual-listener port confusion: each listener gates on its own expected_port (router_for_port);
  tests cover wrong-port rejection. Phase 2 re-verifies live.
- Downloader digest: fetched from GitHub release asset metadata (Aztec's release = trust root,
  same host as artifact — self-referential by design; marker rehash catches post-install drift).
- `x-aztec-version` remote-controlled downloads bounded by cache-size cap + revocation denylist
  checked before bundled short-circuit.
- prove tempdir: per-user data-local parent, owner-only, Windows refuses shared-temp fallback.
- Updater verify-before-download ordering enforced by VerifiedUpdate capability type.
