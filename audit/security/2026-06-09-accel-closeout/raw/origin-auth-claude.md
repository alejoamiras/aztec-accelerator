# Raw — origin-auth trust boundary (Claude agent)

Run: 2026-06-09 post-quality-fixes closeout security audit. Scope: localhost server origin gate.

## Confirmed facts (traced)
- Server binds `127.0.0.1` only (server.rs:186, tls.rs:22) — not 0.0.0.0. DNS-rebinding is browser-mediated, not raw-network.
- **No `Host` header is ever read or validated** anywhere (exhaustive grep). Only the `Origin` header is gated (auth.rs:23-33).
- Missing `Origin` → `None => return Ok(())` auto-approve (auth.rs:30-33), pinned by test `prove_skips_auth_when_no_origin_header` (server.rs:908).
- Localhost origins auto-approved with no prompt (authorization.rs:213-223); `url::Url` normalizes `0177.0.0.1`/`2130706433`/unicode → `127.0.0.1` (genuinely loopback → acceptable).

## Findings
1. **[HIGH] DNS rebinding: no Host-header check; cross-origin page that omits Origin reaches /prove** — auth.rs:32. evil.com (TTL~1s → 127.0.0.1); attacker JS `fetch('http://127.0.0.1:59833/prove',{method:'POST',body})`. Browser may send NO Origin (no-cors form/navigation, or post-rebind effective host = loopback). `authorize_origin` reads only ORIGIN; absent → approve. No Host validation. Impact: arbitrary page offloads attacker proving (CPU abuse / proof-oracle / remote-trigger surface incl. attacker x-aztec-version download trigger). Missing control: Host-header allowlist (reject Host whose host ∉ 127.0.0.1/localhost/[::1]).
2. **[HIGH] Missing Origin treated as APPROVED (fail-open)** — auth.rs:30. curl/same-origin/form/no-cors/rebound page hit `None => Ok(())`. Comment frames "CORS is browser-only" but a malicious page CAN cause a no-Origin cross-site request → linchpin for #1. Missing control: fail-CLOSED on absent Origin (or a separate loopback-Host bypass for non-browser local scripts).
3. **[MEDIUM] Any local web server (other dev app / local foothold) silently auto-approved** — authorization.rs:213/222. Any `http(s)://localhost:<any-port>` page (2nd dev server, malicious npm postinstall listener, compromised local tool) → unprompted unlimited /prove. No port pinning / per-app distinction.
4. **[MEDIUM] respond_auth resolves on RAW (non-canonical) origin → cross-pending-resolution + dead path** — commands.rs:131. Deny on a malformed payload is a no-op vs the canonical-keyed pending map (real request only dies on 60s timeout). resolve() matches on origin string with no per-request token → any caller of respond_auth with a known canonical origin could resolve a different concurrent pending request as Allow{remember}. Bounded today by Tauri IPC (local UI). Missing control: server-issued opaque per-request id.
5. **[LOW] Global pending cap (10) = liveness DoS** — server.rs:130 / authorization.rs:162,195. 10 distinct sub-origins fill the global map → new legit origins get 429 + popup spam for ≤60s. Caps memory (good) but no per-origin fairness.
6. **[LOW] CORS allow_origin(Any) → no defense-in-depth behind the gate** — server.rs:194-211. Once past authorize_origin, any origin reads the proof body. By design (cross-origin SDK), but amplifies any #1/#2 bypass.

NOTE: all pre-existing (not introduced by quality-fixes F-01..F-09). F-02 hardened canonicalization but did not add a Host check or change the missing-Origin fail-open.
