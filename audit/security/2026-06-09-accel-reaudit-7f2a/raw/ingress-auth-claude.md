# Ingress + Authorization Trust-Boundary Re-Audit — Claude

**Scope:** ingress (loopback Host guard, CORS, `/health` tiering) + `/prove` origin authorization
(`AuthorizationManager`, popup flow) + Safari-HTTPS toggle gate, of the aztec-accelerator
desktop/headless app. Read against working tree on branch `feat/verified-sites`.
Resolved deps: `axum 0.8.9`, `http 1.4.0`, `hyper 1.9.0`, `hyper-util 0.1.20`, `url 2.5.8`,
`uuid 1.23.0` (v4 feature), `tower-http 0.6.8`.

---

## VERDICT: CLOSED-CLEAN

All five originally-reported findings (SEC-01, SEC-04, SEC-05, SEC-06) and both post-impl fixes
(M1/SEC-08, L3) are closed in the current code, source→sink. No regression and no new concrete
ingress/auth vulnerability found. Concrete-finding count: **0** (Critical/High/Medium/Low).

One non-actionable residual is noted at the end (a documented, deliberate property of localhost
services — not a finding, not new, and not in scope to "fix"): a non-browser local process that omits
`Origin` and sends a loopback `Host` is auto-approved. This is the explicit, tested design (auth.rs
None-arm; pinned by `prove_allows_no_origin_only_with_trusted_loopback_host`) and is the correct
trust model for a loopback IPC surface — any local process already has user-level code execution.

---

## Concrete findings

None.

---

## Confirmed-closed (each item attacked, with the file:line that proves it)

### SEC-01 — Loopback Host/`:authority` allowlist, outermost layer (CLOSED)

Wiring: `core/src/server.rs:204-228` builds the router; the host guard is added **last**
(`.layer(from_fn(host::guard))` at `server.rs:225-227`), so it is the **outermost** layer and runs
**before** CORS and both routes. Both listeners pass their own expected port: HTTP `start()` →
`router(state)` → `router_for_port(state, PORT=59833)` (`server.rs:188,196-198`); HTTPS
`tls.rs:23` → `router_for_port(state, HTTPS_PORT=59834)`. Headless binary uses the same `start()`
(`server/src/main.rs:98`). There is exactly **one** `Router::new()` in the whole tree
(`server.rs:214`) with two routes — no `merge`/`nest`/`route_service` escapes the guard.

Both listeners bind **loopback-only**: HTTP `SocketAddr::from(([127,0,0,1], PORT))` (`server.rs:188`),
HTTPS `([127,0,0,1], HTTPS_PORT)` (`tls.rs:24`). The kernel drops any non-loopback-destined packet,
so the *only* browser-reachable attack is DNS-rebinding (browser dials 127.0.0.1 but `Host: evil`),
which the guard blocks.

`host_is_trusted` (`host.rs:22-42`) attacked against the **actual `http 1.4.0` parser**
(`~/.cargo/.../http-1.4.0/src/uri/authority.rs`):
- **DNS-rebinding:** `Host: evil.com:59833` → `host()` returns `evil.com` ≠ loopback literal → 403.
  Pinned by `host.rs:97-100` + the end-to-end `prove_rejects_forged_host_dns_rebinding`
  (`server.rs:1099-1131`: 403 `invalid_host`, popup never fires).
- **Userinfo smuggle:** guard rejects any `@` *before* parsing (`host.rs:24`). This is load-bearing —
  the http crate's free `host()` fn does `rsplit('@').next()` (`authority.rs:429-433`), so
  `evil.com@127.0.0.1:59833` would otherwise yield host `127.0.0.1`. Pre-rejection closes it
  (`host.rs:121-126`).
- **`[::ffff:127.0.0.1]`:** parses as a valid bracketed authority; `host()` returns the bracketed
  literal; guard strips brackets → `::ffff:127.0.0.1`, not in `{127.0.0.1,localhost,::1}` → 403
  (`host.rs:116`).
- **Decimal/hex IP (`2130706433`, `0x7f000001`):** host string is `2130706433`, not a loopback
  literal → 403 (`host.rs:115`). url/Authority does not canonicalize these to dotted-quad here.
- **`0.0.0.0`:** not in the literal set → 403 (`host.rs:114`).
- **Trailing dot (`localhost.`):** `trim_end_matches('.')` normalizes → accepted (`host.rs:36,93`) —
  a real form, correctly *not* 403'd.
- **`[::1]`:** brackets stripped → `::1` matched (`host.rs:90`). Real Safari/IPv6 form preserved.
- **Wrong port (`:59834` on the `:59833` listener and vice-versa):** `port_u16() != Some(expected)`
  → 403 (`host.rs:32`, `host.rs:103-106`). Per-listener port threading is what makes this hold.
- **Port-absent / `127.0.0.1` bare:** `port_u16()` is `None` → 403 (`host.rs:108-109`). No
  "port-absent" loophole.
- **`127.0.0.1:59833:extra` / malformed:** `validate_authority_bytes` returns `InvalidAuthority`
  for `colon_cnt > 1` outside brackets (`authority.rs:554-557`) → parse fails → 403 (`host.rs:133`).
- **h1-Host vs h2-`:authority` disagreement:** both read, fail-closed on mismatch
  (`host.rs:57-64`). In h1, `req.uri()` is origin-form so `authority()` is `None` → uses `Host`
  header; in h2, `:authority` populates `req.uri().authority()`. Neither-present → fail-closed
  (`host.rs:62-63`).
- **Header-injection / whitespace smuggle:** `HeaderValue::to_str` rejects any non-visible-ASCII
  byte (control/CR/LF/high) (`http-1.4.0/src/header/value.rs`, `is_visible_ascii` loop) → a CRLF- or
  tab-laced Host never reaches the parser as a passing value.
- **Duplicate `Host`:** `headers().get(HOST)` returns the first value; both copies route to the same
  single handler (one backend, no intermediary, no proxy in the repo — confirmed no
  `proxy_pass`/`X-Forwarded` anywhere), so there is no smuggling differential to exploit. A
  comma-folded duplicate would fail `Authority::parse` (comma invalid) → 403 (fail-closed).
- **Real-client compatibility (no false 403):** `127.0.0.1:59833`, `127.0.0.1:59834`,
  `localhost:59833`, `[::1]:59834`, case-insensitive, trailing-dot all accepted
  (`host.rs:83-94`) — the exact SDK/Safari/curl/Node forms. HTTP proving and the Safari-HTTPS h1
  path are not broken.

### SEC-04 — localhost prompt-once (desktop default) (CLOSED)

`config.rs:60-61` defaults `auto_approve_localhost = false`; `Default` impl sets it false
(`config.rs:72`); a pre-existing on-disk config lacking the field deserializes to false via
`#[serde(default)]` (secure-by-default on upgrade). `is_approved`
(`authorization.rs:269-276`) gates the auto-approve branch on `auto_approve_localhost && ...`, so
with the flag off **no** `http://localhost:*` / `http://127.0.0.1:*` / `http://[::1]:*` origin is
silently approved — it falls through to the popup (auth.rs:76-92). Pinned by
`is_approved_checks_both` (`authorization.rs:339-343`: flag-off localhost → not approved). The
headless binary deliberately sets it `true` (no popup available) and scopes via `ALLOWED_ORIGINS`
(`server/src/main.rs:76-79`) — out of the desktop-default threat model.

### SEC-05 — `/health` Origin-tier fingerprint starvation (CLOSED)

`health_is_detailed` (`server.rs:237-260`): **no Origin → detailed** (local non-browser tool, line
242); **Origin present but malformed → minimal** (line 245); **Origin present + gated config →
detailed only iff `is_approved`** (lines 247-256). The detailed body (`version`, `aztec_version`,
`available_versions`, `bb_available`, `https_port`) is built only after this gate
(`server.rs:267-269` early-returns `{status:"ok", api_version:1}` otherwise). So an unapproved
cross-origin browser page gets liveness only — no version/bb/cache fingerprint. After SEC-01 every
caller already carries a loopback Host, so Origin (not Host) is the correct discriminant here. Pinned
by `health_minimal_for_unapproved_cross_origin` (`server.rs:857-918`).

### SEC-06 — per-request opaque-id resolution (CLOSED)

`AuthorizationManager` is a dual map (`authorization.rs:171-177`): `by_origin: origin→request_id`
(piggyback routing only) and `by_request: request_id→PendingRequest` (the authority). `resolve`
keys **only** on `request_id` (`authorization.rs:238-246`) — `by_request.remove(request_id)`; an
unknown/guessed/stale id is a no-op. `request_id` is `uuid::Uuid::new_v4()` (`authorization.rs:223`)
— 122 bits CSPRNG, unguessable. **Every** production `.resolve()` call passes a `request_id`, never
an origin: `auth.rs:98` (timeout), `commands.rs:124` (`respond_auth`), `windows.rs:125`
(popup timeout). No origin-keyed resolve path remains anywhere.
- **Forged/guessed id resolving a different concurrent request:** impossible — id is the map key and
  is 122-bit random. Pinned by `resolve_ignores_wrong_request_id` (`authorization.rs:374-386`).
- **id leak:** `request_id` never appears in any network-reachable response (`/health`, `/prove`).
  It flows generate → `show_popup` callback → local Tauri window URL
  (`authorize.html?requestId=…`, `windows.rs:89-93`) → in-process `invoke("respond_auth")` IPC
  (`authorize.html:37,53`). The popup is a local `tauri://` window, not a network endpoint, so a
  remote page cannot read another request's id.
- **Piggyback (`is_first`) race:** only the first pending request per origin shows a popup
  (`auth.rs:88-92`); repeats share the same `request_id` + decision (`authorization.rs:213-217`),
  bounded by `MAX_PENDING_ORIGINS=10` (`authorization.rs:220-221`).
- **Leaked/never-cleaned pending entry:** `resolve` clears **both** maps
  (`authorization.rs:240-242`); the 60s server timeout force-resolves Deny on no response
  (`auth.rs:94-103`) and the popup-side 60s timeout also resolves Deny (`windows.rs:118-127`), so a
  pending entry cannot linger past the timeout.

### M1 / SEC-08 — Safari toggle runs fail-closed CA-key migration before HTTPS (CLOSED)

`enable_safari_support` (`commands.rs:151-184`) calls `certs::migrate_legacy_ca_key()?`
(`commands.rs:162-164`) **before** `certs::generate_and_save`, `install_ca_trust`,
`load_rustls_config`, and `spawn_https` (`commands.rs:166-180`). On `Err` it returns early
(propagated to the Settings UI) and HTTPS never starts. This **mirrors the startup path**: `main.rs`
`match certs::migrate_legacy_ca_key() { Ok(()) => try_start_https(&state), Err(e) => …NOT started }`
(`main.rs:424-428`). The migration is genuinely fail-closed: `migrate_legacy_ca_key_at`
(`certs.rs:189-217`) removes `ca.key` (1 retry for a transient lock), then **re-checks** and returns
`Err` if it still exists (`certs.rs:205-211`). So a Settings off→on toggle cannot bring up Safari
HTTPS while a readable legacy `ca.key` (the mint-any-cert primitive) persists on an upgraded install.
Idempotent + absent-key-Ok (`certs.rs:192-193`). HTTP is unaffected.

### L3 — popup window keyed by request_id, not origin (CLOSED)

Popup label is `format!("auth-{}", sanitize_window_label(request_id))` (`windows.rs:87`), and the
60s timeout closes **that** label (`windows.rs:121-123`) and resolves **that** `request_id`
(`windows.rs:125`). `respond_auth` closes the same `auth-{sanitize(request_id)}` label
(`commands.rs:131-134`). `sanitize_window_label` is a truncated SHA-256 of the key
(`commands.rs:141-145`), collision-resistant. Because the label is derived from the 122-bit-random
`request_id` (unique per pending request) and only the first request per origin spawns a popup
(piggyback gate, `auth.rs:88-92` / `windows.rs:106`), a resolved request's stale timeout can no
longer close the **live** window of a newer same-origin request, and two distinct requests cannot
collide on a label (different UUIDs → different SHA-256 prefixes). `origin` is retained on the
`respond_auth` payload and timeout for diagnostics/logging only (`commands.rs:130`,
`windows.rs:126`) — it is not a trust input.

---

## Notes on residual (not a finding)

- **No-Origin local caller auto-approve** (`auth.rs:25-35`, None-arm `return Ok(())`): a local
  process that omits `Origin` and sends a loopback `Host` reaches `/prove` without a popup. This is
  the documented, tested, correct trust model for a loopback IPC service (CORS/Origin is a
  browser-only mechanism; any local process already has user-level execution). The DNS-rebinding
  no-Origin variant is 403'd at the Host guard. Pinned by
  `prove_allows_no_origin_only_with_trusted_loopback_host` (`server.rs:1065-1093`) and
  `prove_rejects_forged_host_dns_rebinding`. Out of scope to change; flagged only for completeness.
