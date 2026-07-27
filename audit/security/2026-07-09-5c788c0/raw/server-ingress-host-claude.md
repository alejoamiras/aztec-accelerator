# Cluster: server-ingress-host — Claude findings

Scope: `packages/accelerator/core/src/server.rs`, `packages/accelerator/core/src/server/bind.rs`,
`packages/accelerator/core/src/server/host.rs`, `packages/accelerator/core/src/server/probe.rs`.

## Summary

The loopback `Host`/`:authority` guard (`host.rs`) was tested adversarially against every angle
listed in the cluster brief — absolute-form request URI vs `Host` disagreement, HTTP/2 `:authority`
handling, trailing dot, case, IPv6 brackets/zone-id, userinfo smuggling, decimal/hex/IPv4-mapped
IPv6 forms, `0.0.0.0`/`127.x` range, missing port — and confirmed (via the `http` crate v1.4.1
source for `Authority::host()`/`from_str`) that every one of these either fails closed or is a
strict *narrowing* of the accepted set, never a widening. Layer ordering (`router_for_port`) was
also verified against axum's own semantics: the last-added `.layer()` is outermost and therefore
runs before CORS/body-limit/handler for every method including `OPTIONS`, matching the code
comment; a request rejected by the guard never reaches the CORS layer, so it isn't even
CORS-readable by the rebinding page. No bypass of SEC-01a was found. `CorsLayer::allow_origin(Any)`
without `allow_credentials` is required by the architecture (any dApp origin must be able to talk to
the local accelerator; the real gate is the per-origin authorization check, out of this cluster's
file scope) and no concrete bypass ties it to a privacy/witness leak from these files alone.

One concrete finding was found in the `bind.rs`/`probe.rs`/(sink in `main.rs`) redundant-instance
classifier: the trust decision for "is the incumbent on `:59833` really our own healthy instance"
is based entirely on two unauthenticated JSON field values that are public (visible in this
open-source repo) and trivially forgeable by any local process, and a positive match causes the
legitimate accelerator to silently `exit(0)` with no tray error. Reported below.

---

## Finding 1: Windows redundant-instance health probe is unauthenticated and forgeable, enabling a silent local self-DoS

**1. Title:** Forgeable `/health` "is this really us" check causes the legitimate accelerator to silently exit when a squatter answers first (Windows-only)

**2. Impact factors:**
- Confidentiality: not violated (no witness/proof/origin data involved in this path).
- Integrity: not violated.
- Authorization: not violated.
- **Availability: violated** — the legitimate accelerator process terminates itself (`exit(0)`)
  with no error surfaced, leaving the user with no running accelerator on that machine/session,
  for as long as the squatter holds the port (persistently, if the attacker's process restarts
  itself or wins the race on every logon).
- Blast radius: single user / single machine (the session where the attacker's process runs).
- Data sensitivity: none (availability-only; no sensitive data disclosed or corrupted).
- Attack vector: **local** (requires a co-resident process able to bind `127.0.0.1:59833` before/
  instead of the legitimate app).
- Attack complexity: **low** (a ~10-line HTTP responder that echoes two static JSON fields).
- Privileges required: **low** — any unprivileged local code-execution capability (malware already
  present in the user's own session, or a scheduled task under a different account racing the
  bind, depending on the OS's loopback port-sharing rules) is sufficient; no admin/elevation needed.
- User interaction: none beyond normal system logon (which is what races the autostart entry).

**3. Evidence confidence:** high — full call chain read and traced statically; the `cfg!(target_os
= "windows")` gate, the classifier's field checks, and the silent-exit sink are all unambiguous in
source.

**4. OWASP category + CWE:** OWASP A04:2021 – Insecure Design (a security-relevant decision —
"should I self-terminate because a peer already owns this port" — is made from unauthenticated
data). CWE-306: Missing Authentication for Critical Function (no authentication/identity proof is
required of the "incumbent" before the check treats it as trusted); secondarily CWE-290:
Authentication Bypass by Spoofing (the two-field JSON body is spoofable by design intent violation
— the code's own comment claims to exclude "a foreign process squatting on the port" but the check
cannot actually distinguish that case).

**5. Trace (source → sink):**
- Precondition/source: attacker-controlled local process binds `127.0.0.1:59833` before the
  legitimate accelerator starts (e.g. races the Task-Scheduler/autostart entries at Windows logon)
  and serves a fixed `/health` response `{"status":"ok","api_version":1}`.
- `packages/accelerator/core/src/server/probe.rs:24-45` — `healthy_aztec_on_port()` issues a plain,
  unauthenticated `GET http://127.0.0.1:{PORT}/health` (no shared secret, token, TLS client cert,
  or OS-level process/PID identity check of any kind).
- `packages/accelerator/core/src/server/probe.rs:14-17` — `is_healthy_aztec_response()` is the
  *entire* trust decision: `status=="ok" && api_version==1`. Both values, and the exact JSON shape
  expected, are visible in this public repository.
- `packages/accelerator/core/src/server/bind.rs:13-18,22-56` — `bind_with_retry` retries for 5s
  then returns `AddrInUse` once it gives up (the squatter never releases the port), which is the
  condition that triggers the classifier at all.
- `packages/accelerator/src-tauri/src/main.rs:224-247` (`spawn_http_server`, direct caller of the
  two functions above — cited for the sink, since the harm only manifests here): on
  `addr_in_use && cfg!(target_os = "windows") && healthy_aztec_on_port().await` (main.rs:238-240),
  the code calls `app_handle.exit(0)` (main.rs:245) — with **no** call to `status.set_text(..)` or
  `tray.set_tooltip(..)`, unlike the sibling `else` branch two lines below (main.rs:249-255) which
  does surface `"Error: port 59833 in use"` / `"Error: server failed"` to the tray for every other
  failure mode. The tray icon (already constructed earlier in `.setup()`) simply disappears with no
  diagnostic.

**6. Missing control:** no authenticated/unspoofable proof that the incumbent on `:59833` is
actually a prior instance of this same application — e.g. a per-install shared secret sent as a
header/query param and checked by the real server before it would ever echo it, a signed nonce,
inspecting the *owning process's* PID/exe path/signature via an OS API, or replacing the whole
scheme with a named-mutex/lock-file single-instance guard (the standard Windows single-instance
pattern) instead of trusting an HTTP response body.

**7. Exploit / violation scenario:**
1. Attacker code (already running as any local user — e.g. pre-existing malware, or another
   account's scheduled task) starts at/near logon and binds a minimal HTTP server on
   `127.0.0.1:59833` that always answers `GET /health` with `{"status":"ok","api_version":1}` and
   nothing else (it does not need to implement `/prove`).
2. The legitimate Aztec Accelerator's autostart entry also fires at logon and calls
   `server::start` → `bind_with_retry` (`bind.rs`), which retries for up to 5s and then returns
   `AddrInUse` because the attacker's squatter still holds the port.
3. `spawn_http_server` (`main.rs:224`) classifies the error as `AddrInUse`, is running on Windows,
   and calls `healthy_aztec_on_port()`, which succeeds against the forged body.
4. `main.rs:245` calls `app_handle.exit(0)`. The legitimate process — the only one that can
   actually download/run `bb` and produce proofs — terminates immediately and silently.
5. The user has no working accelerator, no error in the tray, and no indication of *why* (they
   would need to inspect the process list or `netstat -ano` to discover the impostor holding
   `:59833`). If the attacker's squatter persists across reboots (e.g. as its own
   autostart/service), this is a durable, repeatable denial of service every time the user logs in.

**8. Preconditions:** Windows only (`main.rs:239`, `cfg!(target_os = "windows")`); attacker needs
pre-existing local code-execution capability able to bind loopback `:59833` before/ahead of the
legitimate app and keep the bind held across the 5s retry window.

**9. Why existing mitigations fail:** The surrounding comments make the intended guarantee
explicit — `probe.rs:1-5`: "a redundant instance should bow out, but ONLY if the incumbent is
really us and not a foreign process squatting on the port", and `main.rs:232-237` repeats the same
claim. The implemented check does not meet this bar: it validates two constant, publicly-documented
JSON field values with a plain unauthenticated HTTP GET, which is precisely the capability a
"foreign process squatting on the port" needs to pass. This is not a parsing bypass of a strong
check (the kind of bypass the base prompt asks for) — the check itself provides no identity
guarantee at all, so no bypass technique is needed; reproducing the two fields *is* the entire
"attack." Nothing else in this cluster (the Host/`:authority` guard in `host.rs`) mitigates this,
because the redundant-instance probe is a client (not a request into the guarded server) and is
never subject to the Host guard.

**10. Instances (same root cause):**
- `packages/accelerator/core/src/server/probe.rs:14-17` (`is_healthy_aztec_response` — the
  forgeable classifier)
- `packages/accelerator/core/src/server/probe.rs:24-45` (`healthy_aztec_on_port` — unauthenticated
  probe using that classifier)
- `packages/accelerator/src-tauri/src/main.rs:238-247` (the sink: silent `exit(0)` gated solely on
  the forgeable check, contrasted with the error-surfacing `else` arm at `main.rs:248-255`)

---

## Non-findings (checked, no concrete bypass — noted for audit completeness, not separately certified)

- `host.rs` `host_is_trusted`: userinfo-smuggling pre-filter (`authority.contains('@')`,
  `host.rs:24-26`) is actually redundant-but-harmless given `http::uri::Authority::host()` already
  strips userinfo via `rsplit('@')` before returning the host substring (verified against the
  `http` v1.4.1 source) — no bypass, and the explicit check only makes the guard *more*
  restrictive.
- IPv6 zone-id (`[::1%25eth0]:59833`) and IPv4-mapped/decimal forms: parsed host string retains the
  extra suffix/format after bracket-stripping and lowercasing, so `matches!(host, "127.0.0.1" |
  "localhost" | "::1")` fails closed — no bypass.
- Absolute-form request-target vs `Host` header disagreement: `guard()` (`host.rs:57-64`) fails
  closed on any disagreement and fails closed when both are absent; only an exact, single
  loopback-literal-on-the-right-port value passes, matching what every real client (browser
  same-origin fetch to a rebound domain, or a genuine loopback client) can produce — a DNS-rebound
  page's `Host` is necessarily the attacker's domain, which is rejected.
- Layer ordering (`server.rs:207-232`): confirmed against axum's documented semantics that the
  last-added `.layer()` (the host guard) is outermost and runs before CORS/body-limit/handler for
  every HTTP method, including `OPTIONS` preflight, and that a guard-rejected response never enters
  the CORS layer (so it isn't even CORS-exposed to the rebinding page). No gap found.
- `/health` detail-gating (`server.rs:240-263`, SEC-05): cross-origin fetch/XHR cannot spoof the
  browser-set `Origin` header, so an unapproved cross-origin caller cannot forge an approved-looking
  Origin to obtain the detailed body; the `None`-Origin → detailed-body branch is the
  already-documented curl/script accepted caveat and discloses only low-sensitivity data (version
  strings, cached-version list, a boolean) — no witness/proof/origin-list leak.
- `bind.rs` retry/backoff: bounded (100ms × up to 5s), no panics, propagates non-`AddrInUse` errors
  immediately; a persistent squatter that does *not* answer the forged `/health` body correctly
  falls through to the safe, error-surfacing branch (`main.rs:248-255`) — only the forgery in
  Finding 1 turns this into a silent failure.
