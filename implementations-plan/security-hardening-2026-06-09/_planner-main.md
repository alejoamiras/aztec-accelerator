# Planner — main (independent plan #1 of 3)

Scope: SEC-01,03,04,05,06,07,08,09 (SEC-02 deferred + tracked). My independent design; strongest-pieces consolidated later.

## Central thesis
**The `Host`-header allowlist is the keystone.** DNS rebinding works because the rebound page's `Host` is `evil.com:59833` while the server only ever looks at `Origin`. A single ingress check — reject any request whose `Host` host-component ∉ {`127.0.0.1`,`localhost`,`[::1]`} — defeats rebinding *regardless of Origin*, on `/prove` AND `/health`, desktop AND headless. Everything else is defense-in-depth layered behind it. So PR-1 leads with the Host check; the absent-Origin and headless-default questions get *easier* once it's in.

## PR phasing (5 PRs, risk-ordered)
- **PR-1 — SEC-01 origin gate (HIGH, core + headless).** Host-allowlist tower middleware; fail-closed absent-Origin on `/prove`; headless deny-by-default + `--allow-all`/`ACCEL_ALLOW_ALL` opt-in; docs. The keystone PR.
- **PR-2 — SEC-05 + SEC-04 (origin-gate adjacencies, core).** `/health` minimal-vs-detailed split; configurable localhost-trust narrowing (default = today's zero-config). Same router/auth surface as PR-1 → sequence after it.
- **PR-3 — SEC-03 + SEC-07 (download/resource, src-tauri + core).** Updater size-cap (replace `update.download()` with a capped streaming download); tarball decompression cap. Carries the **SEC-02 deferral note**.
- **PR-4 — SEC-08 + SEC-09 (cert hardening, src-tauri).** Legacy `ca.key` fail-closed + retry + surface; rotation leaf-chains-to-CA verify + atomic staging-dir swap.
- **PR-5 — SEC-06 (IPC per-request token, src-tauri + popup frontend).** Server-issued opaque request id; cross-package, standalone, last.

## Per-finding design

### SEC-01 (PR-1) — the keystone
1. **Host allowlist** — a tower `middleware`/layer in `core/src/server.rs` wrapping the router, running before the auth gate, for ALL routes. Extract `Host` (HTTP/1.1 `Host` header; for HTTP/2 the `:authority`); take the host-component (strip `:port`, strip a bracketed IPv6's brackets consistently); **case-insensitive exact match** against {`127.0.0.1`,`localhost`,`[::1]`/`::1`}. Anything else → `403` + log. Missing Host → `403` (HTTP/1.1 requires it; legit clients always send it).
   - **Bypass enumeration (must test):** `0.0.0.0` → reject; decimal/octal/hex IP literal (`2130706433`, `0x7f.1`) → reject (we match literals, not normalized — legit clients send canonical literals; an attacker serving on `http://2130706433:59833` sends `Host: 2130706433` → rejected); `localhost.` trailing dot → normalize (trim one trailing dot) then match; `[::1]` vs `::1` vs `0:0:0:0:0:0:0:1` → accept the first two literal forms (what browsers/SDK emit), reject exotic expansions; `Host:` absent → reject; a page rebound+served on `:59833` with `Host: localhost` → **this is the one literal-localhost bypass** (attacker must serve on the loopback Host literal) — but to send `Host: localhost` the browser must have loaded the page from `http://localhost:59833`, i.e. the attacker already controls a localhost origin (SEC-04 territory, local foothold), not remote rebinding. Document this residual.
2. **Absent-Origin on `/prove` → fail closed.** Verified: the SDK (`accelerator-transport.ts`) calls `/prove` via `ky.post` from the dApp page — a **cross-origin** request to `127.0.0.1`, so the browser **always** attaches `Origin` (HTTP and Safari-HTTPS alike). Therefore no legit browser `/prove` omits Origin. Flip `None => Ok(())` to `None => deny`. **Contract flip:** `prove_skips_auth_when_no_origin_header` (`core/src/server.rs:908`) becomes `prove_denies_when_no_origin_header`; update any e2e/test harness that POSTs to `/prove` without an Origin to send one. (Defense-in-depth behind the Host check, which already blocks the rebinding no-Origin case.)
3. **Headless deny-by-default.** `server/src/main.rs`: unset `ALLOWED_ORIGINS` currently → `(None, None)` (open). New: unset → behave like **present-but-empty** (build `auth_manager` + empty approved list ⇒ gate on, deny non-localhost, localhost still auto-approved). Restore the old open behavior only under `--allow-all` (CLI) **or** `ACCEL_ALLOW_ALL=1` (env) → `(None, None)`. Emit a one-line `WARN` when running `--allow-all`. **Docs:** the `//!` module doc (`server/src/main.rs:6-7`), README headless section, and any CLAUDE.md mention.

### SEC-05 (PR-2) — `/health` tension (real constraint)
The SDK probes `/health` **cross-origin** to detect the accelerator + read `aztec_version`/`available_versions` (for `needsDownload`). So `/health` must stay cross-origin-readable or detection breaks. Resolution: **two-tier health.** Unapproved/unknown origin → minimal body (`{status:"ok", protocol}`) with permissive CORS (preserves detection + the SDK can still attempt `/prove` → approval flow). Approved origin (or same-origin) → full detail (versions, `bb_available`). Shrinks the cross-site fingerprint (version/cache no longer world-readable) without breaking zero-config detection. **Verify** the SDK still computes `needsDownload` correctly when it only gets the minimal body pre-approval (it falls back to attempting prove; the accelerator downloads on demand — acceptable).

### SEC-04 (PR-2) — localhost-trust knot (resolved)
With the Host check defeating *remote* rebinding, SEC-04's residual is a *local-foothold* attacker (malicious process serving a page on `localhost:PORT`). Do **not** break the playground/dev zero-config (`localhost:5173` etc. must keep auto-approving). Resolution: add an **opt-in `restrict_localhost` setting** (default OFF = today's behavior). When ON, non-self localhost origins get the approval prompt instead of auto-approve. Ship the knob, keep the safe-default; document the trade-off. (Rejecting a hard narrowing: it breaks zero-config for every dev for a local-foothold threat that already implies machine compromise.)

### SEC-03 (PR-3) — updater size cap (plugin constraint)
`tauri-plugin-updater`'s `update.download()` buffers the whole body into a `Vec` before signature verify, and its progress closure `FnMut(usize, Option<u64>)` **cannot abort the loop**. So a callback-only guard can't truly cap. Fix: **reject pre-flight on advertised `content_length` exceeding a ceiling (e.g. 500 MB), AND** — for robustness against a content-length-omitting server — replace `update.download()` with our own `reqwest` streaming download that enforces a cumulative byte cap, then `update.install(bytes)`. Ceiling derived from a constant (the DMG/AppImage are tens of MB). Pin the source URL to the expected host.

### SEC-07 (PR-3) — decompression cap
`core/src/versions/downloader.rs:240` `entry.unpack` is unbounded. Wrap the `GzDecoder`/entry reader in a byte-counting reader with a hard ceiling (e.g. 1 GB for `bb`), or check `entry.header().size()` before unpack and abort if it exceeds the cap. Test with a crafted small-compressed/large-uncompressed fixture.

### SEC-08 (PR-4) — legacy ca.key fail-closed
`certs.rs:181-198`: after the `remove_file` attempt, **verify the key is actually gone**; on persistent failure retry once, then **surface** (a tray/log error the user sees) and — per the audit — **fail closed for Safari HTTPS** (don't bring HTTPS up while the legacy mint-any-cert key + trusted anchor coexist). Idempotent; safe on installs that never had a legacy key.

### SEC-09 (PR-4) — cert rotation chain check + atomic swap
Before `swap_into`, run `security verify-cert -c <staged leaf> -r <staged ca>` (leaf-chains-to-anchor), not just the anchor-trust check. Replace the 3 non-atomic renames with an **atomic staging-dir swap** (write the full set into a temp dir, then a single directory rename / or use `rename` on a symlinked `current` dir) so a crash can't leave a ca/leaf mismatch.

### SEC-06 (PR-5) — per-request token (cross-package)
Server issues an opaque `request_id` (UUID v4) per pending approval; the popup payload carries it; `respond_auth(request_id, decision)` resolves the pending map **by id**, not by origin string. Keep the origin in the payload for display. Update the popup frontend JS to round-trip the id. Removes the malformed-Deny no-op + the same-origin shared-decision race. Preserve backward-compat: version the IPC or update both sides atomically (single PR).

## Test plan
- **SEC-01:** unit — Host-allowlist accepts {127.0.0.1, localhost, [::1]}:port, rejects evil.com / 0.0.0.0 / decimal-IP / absent-Host; **DNS-rebinding regression** (forged `Host: evil.com` + no Origin → 403); absent-Origin-on-/prove → deny; headless unset→deny-non-localhost, `--allow-all`→open. Flip `prove_skips_auth_when_no_origin_header`.
- **SEC-03/07:** size-cap rejects oversized download / decompression (crafted fixtures).
- **SEC-08/09:** ca.key-delete-fails → HTTPS not started + surfaced; rotation leaf/ca-mismatch → rejected before swap.
- **SEC-06:** malformed-origin Deny actually denies; two concurrent same-origin requests get independent decisions.
- **e2e:** the headless e2e leg + WebDriver — ensure they set `ALLOWED_ORIGINS` or `--allow-all` (else deny-by-default breaks them); confirm the SDK (HTTP + Safari HTTPS) still proves end-to-end through the Host check.

## Migration & docs
Headless deny-by-default is the one operator-facing break: opt-in flag + WARN + README/module-doc/CLAUDE.md updates + a CHANGELOG/release-note line ("headless `ALLOWED_ORIGINS` unset now denies non-localhost; set `--allow-all` to restore"). Auto-update reaches real users → stage the 1.0.x rollback (latest.json kill-switch) as with prior releases; the gate change is covered by WebDriver E2E.

## SEC-02 deferral (tracked)
Do NOT implement a pinned-digest manifest (nightlies ship daily → perpetually stale). Leave: (a) a `// SECURITY (SEC-02, deferred):` comment at `core/src/versions/mod.rs` `fetch_github_asset_digest` explaining the circular-trust gap + the nightlies constraint + the intended fix (verify an upstream Aztec publisher signature once `bb` releases are signed); (b) a tracking entry in this plan's index + recommend opening a GitHub issue; (c) a note in the `/harden` re-audit scope so it's re-checked.

## Open knots for consolidation/audit
1. Absent-Origin on `/prove`: hard-deny (my pick) vs allow-if-loopback-Host. Audit should stress the e2e/curl impact.
2. `/health` two-tier: does the SDK's pre-approval detection truly survive the minimal body? Verify against `accelerator-transport.ts`.
3. SEC-03: pre-flight content-length reject (simple) vs full self-managed streaming download (robust but more code + reqwest surface).
4. SEC-09 atomic swap mechanism (dir rename vs symlink-swap) on macOS/Windows.
