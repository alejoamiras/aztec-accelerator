# c1-listener-ingress — Claude (Opus) raw findings

> **COORDINATOR NOTE (added by the main agent, not the cluster agent):** F-C1-1 overlaps prior finding
> **F-002 "Spoofable `/health` probe evicts the real accelerator (Windows)" (MEDIUM, 2026-07-09)**, which
> was NOT in the agent's already-known list. Phase 3 must adjudicate: re-report, regression, or
> escalation? The agent adds two sinks the prior finding did not cover (the update-floor ratchet and
> witness interception via the SDK's identical `/health` contract) and an unbounded-response-size issue.

## F-C1-1 — Unauthenticated liveness probe used as an identity check: a local process owning `:59833` makes the real app `exit(0)` (Windows) and ratchets the anti-rollback floor

**Impact.** Availability (the legitimate app removes itself), Confidentiality (the impersonator keeps
`:59833` and therefore keeps receiving `/prove` bodies — private witnesses — from the SDK, with NO
tray/error signal), Integrity (the monotonic floor advances to a version whose server never ran,
permanently blocking rollback to a good build). Blast radius: whole app + all dApps; persists across
reboots (floor on disk; squatter re-wins the port each logon). Vector local; complexity low; privileges
low (unprivileged same-user code, no admin); no user interaction.
**Confidence** high for forgeability and both sinks; moderate for the Windows `SO_REUSEADDR` hijack
refinement. **CWE-290** / CWE-345 / CWE-346, OWASP A07:2021; secondary CWE-400 (see instances).

**Trace.** Untrusted local process on `127.0.0.1:59833` returns
`{"status":"ok","api_version":1,"version":"<our version>"}` →
`core/src/server/probe.rs:14-17` `is_healthy_aztec_response` is the ENTIRE identity test (two constant
JSON fields) → `:24-45` `healthy_aztec_on_port()` GETs `/health` (`:25`), 2xx (`:38`), `resp.json()`
(`:41`), classify (`:44`) → **sink A** `src-tauri/src/main.rs:294-303`:
`if addr_in_use && cfg!(windows) && healthy_aztec_on_port().await { app_handle.exit(0) }` — the real app
terminates with status 0, chosen (comment `:290-292`) specifically so the supervisor will NOT retry →
`probe.rs:54-71` `healthy_aztec_version_on_port()` returns the responder's self-declared `version`
(`:68-70`) → **sink B** `main.rs:347-355`: three matching probes ⇒ `updater::commit_launch_floor()`.
Bind side: `core/src/server.rs:245-246` → `server/bind.rs:13-18,30` — `TcpListener::bind` with **no
`SO_EXCLUSIVEADDRUSE`** and no post-bind ownership assertion. Witness sink:
`packages/sdk/src/lib/accelerator-prover.ts:181-220` — the SDK consumes the same `/health` contract, then
POSTs the witness to `/prove` on whatever answered.

**Missing control.** No authentication of the responder anywhere: no per-install secret echoed in
`/health` (or HMAC challenge); no OS-level ownership check (Windows `GetExtendedTcpTable` → owning PID →
image path + Authenticode); no OS single-instance primitive (named mutex / abstract socket / lockfile)
instead of an HTTP probe; no `SO_EXCLUSIVEADDRUSE` on the Windows listener. The `version` echo in sink B
is treated as an identity assertion but is fully attacker-controlled.

**Exploit.** Malware as the user starts earlier (Run key / Scheduled Task) or hijacks the socket, serves
the expected `/health` shape and accepts `/prove` (proxying to a real bb so proofs still succeed and
nothing looks broken) → accelerator loses the bind, probes, gets `true`, logs "redundant", `exit(0)`
(`main.rs:301`) with no tray icon, no error, no restart → every dApp's SDK finds a "healthy accelerator"
and ships private witnesses to the attacker → the floor tracker (which also runs on macOS/Linux) sees
three version-matched probes and permanently raises the rollback floor.

**Preconditions.** Unprivileged local code execution as the user. Sink A additionally needs Windows + a
lost bind (attacker starts first, or socket hijack: Rust's `TcpListener` does not set
`SO_EXCLUSIVEADDRUSE` on Windows (`bind.rs:30`) and Windows permits a second `SO_REUSEADDR` bind to steal
such a socket for the same user). Sink B needs no port loss at all and fires on any OS.

**Why mitigations fail.** `is_healthy_aztec_response`'s doc (`probe.rs:11-13`) claims it stops "an
arbitrary process answering on :59833" — it checks two constants that are part of the PUBLIC `/health`
contract, observable by probing the real app; it distinguishes nothing an attacker cannot satisfy.
`main.rs:333-336` claims matching `/health.version` "proves we are observing our own server" — that
inference only rules out an HONEST different-version incumbent; it is vacuous against a hostile responder
echoing the expected string. The Host guard and Origin gate protect OUR listener from browsers; they do
nothing for an OUTBOUND probe that trusts whatever answers. SEC-05 gating does not apply — the probe
sends no Origin, so it takes the "local, non-browser caller" branch (`server.rs:296-300`), and the
responder can mirror the full shape anyway.

**Instances.** `probe.rs:14-17` (forgeable predicate); `probe.rs:24-45` → `main.rs:294-303` (bow-out);
`probe.rs:54-71` → `main.rs:340-366` (floor); `bind.rs:30` (no `SO_EXCLUSIVEADDRUSE`, no ownership
assertion after a lost bind). Secondary CWE-400, same source: `probe.rs:41` and `:64` call
`resp.json::<serde_json::Value>()` with **no response-size cap** — the only bound is the 3 s client
timeout, and over loopback a hostile responder can stream multiple GB into the heap in that window.
(serde_json's 128-level recursion limit covers the nesting variant; reqwest has no gzip/br feature here so
there is no decompression amplification.) Same class as the accepted updater residual but on a separate,
undocumented path.

## F-C1-2 — Unbounded authorization-prompt flooding: the pending cap is global and un-grouped, so one page's sub-origins starve every legitimate site of the consent path

**Impact.** Availability — the ONLY mechanism by which a legitimate dApp can ever be approved is
unavailable while the attacker page is open; every legitimate `/prove` gets 429. Secondarily
Confidentiality via consent fatigue: an endless stream of near-identical approval windows is a known route
to a wrongful "Allow", and an Allow here is UNCONDITIONALLY PERMANENT (`authorization.rs:181-188`) and
grants private-witness access. Blast radius: the whole authorization subsystem, both listeners (shared
`AppState`). Vector network (any page); complexity low; no privileges; user interaction required (victim
keeps an attacker page/ad-frame open).
**Confidence** moderate (mechanism certain from code; the consent-fatigue step is behavioural).
**CWE-770** / CWE-799, OWASP A04:2021.

**Trace.** `prove.rs:233` → `auth.rs:24-34` attacker-controlled `Origin` → `:64-67`
`auth_manager.request(&origin)`, `Err` ⇒ `TooManyRequests` → `authorization.rs:331-342` piggyback (same
origin shares one slot, capped `MAX_PIGGYBACK_SENDERS` `:218`) → `:344-346` the ONLY global limiter:
`if st.by_request.len() >= MAX_PENDING_ORIGINS (10) { Err }` — first-come-first-served, **no per-site
(registrable-domain) grouping, no reservation for anyone else** → `auth.rs:69-75` every NEW origin fires a
real OS window (`windows.rs:197-218`) → **sink A** `server.rs:449-453` the victim's real dApp gets 429 and
never reaches a prompt → **sink B** `auth.rs:118-121` `AuthDecision::Deny` returns `OriginDenied` and
**records nothing** — no deny-list, no cooldown, no per-origin rate limit anywhere; the same origin may
immediately re-`request()` and get a fresh popup.

**Missing control.** No grouping of the pending cap by eTLD+1 (`a.evil.com … j.evil.com` are ten
independent tenants of a ten-slot table); no negative caching/backoff after Deny, so prompt rate is
bounded only by how fast the user dismisses windows; no fairness reservation guaranteeing an unseen origin
a slot; no cap on total prompts per unit time.

**Exploit.** Victim loads `evil.com` embedding ten iframes on ten sub-origins, each POSTing a 1-byte
`/prove` → ten `PendingRequest`s, table full → the victim's real dApp gets 429 (`server.rs:449-453`), is
never prompted, and appears broken → each attacker popup auto-denies after 60 s (`commands.rs:238-253`),
freeing a slot; the iframe's `.catch()` immediately re-fires and, because no denial is remembered, a fresh
popup reclaims it → the table never drains, the victim faces an endless queue, and one mis-click on an
attacker sub-origin permanently grants witness access (`auth.rs:89-116`).

**Preconditions.** Desktop build (headless denies instantly, `auth.rs:58-61`). The victim's dApp not
already approved and `auto_approve_localhost` off for it (shipping desktop default, SEC-04). Attacker
needs one open tab or ad iframe; ten hostnames under one wildcard DNS record suffice.

**Why mitigations fail.** `authorization.rs:206-209` claims the cap "prevents popup/memory spam from a
malicious site generating many subdomains" — it bounds CONCURRENT slots to ten, not prompts over time, and
is precisely the mechanism that converts the attacker's subdomains into a DoS against everyone else.
`MAX_PIGGYBACK_SENDERS` bounds fan-out WITHIN one origin only. The C9 single-active-popup arbiter
(`:238-240`, `:274-286`) reduces click-hijacking but LENGTHENS the drain to ~60 s per queued origin,
strengthening the availability impact. `AUTH_QUEUE_BACKSTOP` (`server.rs:56-61`) bounds one request's wait,
not refills. `MAX_INFLIGHT_PROVE`/`prove_waiters` don't apply — `authorize_origin` runs BEFORE `try_enter`
(`prove.rs:233` vs `:238`), so unapproved requests never touch that cap.

**Instances.** `authorization.rs:344-346`, `:206-209`; `auth.rs:64-67`, `:118-121`.

## NON-FINDINGS (the flagged keystones — recorded so they aren't re-audited)

**Loopback Host/`:authority` guard (`server/host.rs`) — no bypass found.** Attempted: HTTP/1.1
absolute-form vs `Host` disagreement (both present + byte-unequal ⇒ fail closed `:57-64`); absent-both
(fail closed); HTTP/2 `:authority` conflicting with `host` (fail closed); userinfo in either position
(`:23-25` pre-reject; `Authority::host()`'s last-`@` split would be safe anyway); IPv4-mapped/decimal/hex/
`0.0.0.0` alternate forms (`Authority` parsing keeps them textually distinct; `:41` matches exact literals);
trailing dots and case (normalised `:36-40`); `*.localhost` sub-labels (rejected — exact match); port
confusion (`:32-34` exact match against the per-listener `expected_port`, correctly wired to `HTTPS_PORT`
at `src-tauri/src/server/tls.rs:31`); trailing `/?#` (Authority::from_str errors); `+`-prefixed ports.
Duplicate `Host` headers select the first, but no browser can emit them and no downstream code re-reads
`Host`, so no smuggling differential. The guard is the OUTERMOST layer (`server.rs:282-284`) and axum 0.8's
`Router::layer` also wraps fallback/catch-all, so no route or 404/405 path bypasses it.

**Origin canonicalization (`authorization.rs:21-89`) — no collision or smuggle.** Default-port elision
uses `Url::port()` (None for scheme defaults) not `port_or_known_default()`, so no cross-scheme aliasing;
trailing-dot hosts rejected outright rather than collapsed (F-011) and `is_auto_approved` (`:398-411`) is
deliberately consistent; IDN → punycode for tuple schemes; extension IDs get an exact ASCII grammar check
BEFORE lowercasing (right order for opaque-host schemes `url` does not IDNA-normalise); `null`, `blob:`,
`file:`, `data:`, sandboxed-iframe opaque origins all fail `parse` → `InvalidOrigin` (400), fail-closed,
and can never enter `approved_origins`.

**Browser-forced Origin omission on `/prove` — not reachable.** Per Fetch, a non-GET/HEAD request always
carries `Origin`; referrer-policy/`no-referrer` downgrades set it to `null`, not absent, and `null` is
rejected. So the accepted "absent Origin auto-approves" residual (`auth.rs:29-33`) is NOT browser-reachable
— it remains local-process-only.

**`/health` detail gating (`server.rs:294-317`) — leak not realizable.** The comment at `:296-300`
("no Origin → local, non-browser caller") is factually WRONG: a browser can produce a no-Origin
cross-origin GET (`mode:'no-cors'`, `<img>`, `<script>`) which takes the detailed branch — but such a
response is opaque and unreadable, and any readable (CORS-mode) request necessarily carries an Origin and
hits the gate. No `Vary: Origin`, but the response has no `Cache-Control`/`Expires`/`Last-Modified` so it
is not heuristically cacheable and no cache-reuse path could be constructed. Both worth a comment fix;
neither meets the bar.

**Authorization races — fail closed under every constructed sequence.** Client-abort mid-popup orphans the
pending entry, but the desktop layer resolves it from three independent paths (60 s activation-armed timer,
window-`Destroyed` listener, `respond_auth`) and even handles window-BUILD failure by resolving Deny and
promoting (`windows.rs:209-218`); a lost Allow on an aborted request simply fails to persist (safe).
`resolve_active` (`authorization.rs:371-385`) + the unspoofable label binding in `respond_auth`
(`commands.rs:155-160`) prevent a queued popup from resolving itself or another request.
**One real defect flagged to `/harden bugs`, not security:** `auth.rs:84` calls `auth_manager.resolve(...)`
on backstop expiry and DISCARDS the returned promoted request id, unlike every other call site
(`commands.rs:245-251,269-271`, `windows.rs:214-216`), so a promotion there would never be raised or armed.
Unreachable in practice (the 60 s popup timer always fires long before the 660 s backstop; headless never
reaches it) and its outcome is denial.
