# Cluster: origin-authz-config — security audit findings

Scope: `packages/accelerator/core/src/authorization.rs`, `packages/accelerator/core/src/server/auth.rs`,
`packages/accelerator/core/src/config.rs`, `packages/accelerator/src-tauri/src/verified_sites.rs`,
`packages/accelerator/verified-sites.json` (plus direct call sites read for context: `server.rs`
health-gate reuse of `is_approved`, `windows.rs`/`commands.rs` popup wiring, `server/main.rs` headless
gating, `server/host.rs` loopback guard).

## NO FINDINGS

Rationale — each specific angle named in the cluster brief was traced to a concrete mechanism and no
bypass could be demonstrated:

- **RFC 6454 canonicalization** (`authorization.rs:21-58`, `canonicalize_origin`): scheme/host
  lowercased by `url::Url`, default port elided per-scheme (verified ws=80/wss=443/http=80/https=443
  distinctly — not conflated), trailing dot stripped, path/query/fragment/userinfo rejected, opaque
  extension schemes matched by exact scheme + lowercased ID with no port. `CanonicalOrigin` (a newtype,
  `authorization.rs:66-145`) makes canonical-by-construction the only way to obtain one — both the
  approved-origins list and the incoming request origin are this type, so no non-canonical string can
  reach the comparison in `is_approved` (`authorization.rs:291-298`). IDN/homograph is punycoded by the
  underlying `idna`/`url` crates before comparison (own test `canonical_origin_idn_punycode_no_homograph_collision`,
  `authorization.rs:602-614`, confirms non-collision with the ASCII form) — no in-repo logic re-derives
  or weakens this. Percent-encoded / numeric-IP / IPv6-compressed forms normalize the same way a real
  browser's own origin computation would (WHATWG host parsing), so two strings that canonicalize
  identically also correspond to the same real network origin — not a false-equivalence.
- **Absent-Origin auto-approve** (`server/auth.rs:24-34`): already documented/accepted per the audit
  brief. Checked for a *new* bypass angle (can a browser cross-origin POST omit Origin?) — modern
  browsers attach `Origin` on cross-origin fetch/XHR/form POSTs; the one case that can strip it
  (cross-origin redirect) produces `Origin: null`, and `CanonicalOrigin::parse("null")` returns `None`
  (fails `Url::parse`), which is routed to the *reject* branch (`server/auth.rs:38-42`,
  `ProveError::InvalidOrigin`) — fail-closed, not auto-approved. No new bypass found.
- **`approved_origins` persistence** (`config.rs:136-177`): write-tmp-then-rename is atomic; the tmp
  file is created with `OpenOptions::mode(0o600)` at creation time (not chmod-after), so there is no
  window where the final `config.json` is more permissive than 0600. The one narrow TOCTOU — the parent
  dir briefly exists at the default `create_dir_all` mode before the explicit `chmod 0700`
  (`config.rs:146-152`) — only occurs on the very first-ever save, before any file has been written
  into it, and is re-asserted (self-healing) on every subsequent save; the only information an attacker
  could win from that race is that the directory exists, which is not sensitive and requires
  already-privileged same-user code to even observe. Not a crossable boundary — dropped as a
  non-finding per the "theoretical, no exploit path" instruction.
- **Verified-sites badge borrowing** (`verified_sites.rs:119-124`, `commands.rs:101-109`,
  `windows.rs:87-93`): the origin string shown to `get_verified_info` is not attacker-suppliable — it
  is built by native (trusted) code from the already-validated `CanonicalOrigin` that gated the
  authorization request (`server/auth.rs:36-42` → `auth.rs:71` → `windows.rs:89-93`, urlencoded into
  the popup's own URL, decoded back out by `authorize.html`, and only then handed to the Tauri
  command). `lookup()` re-canonicalizes with the same total function used to build the registry
  (`verified_sites.rs:121-123`), so a genuinely different origin cannot collide onto a verified entry's
  key without an underlying `canonicalize_origin` collision (see canonicalization bullet above — none
  found). `description`/`curated_by`/`added_at` are deliberately excluded from the DTO
  (`commands.rs:94-99`), so no extra curator metadata crosses the IPC boundary either.
- **`request_id` binding / remember-approval** (`authorization.rs:236-268`, `server/auth.rs:64-103`,
  `commands.rs:111-139`): resolution is keyed by an unguessable UUID v4 (128-bit, `uuid::Uuid::new_v4`),
  not by origin string — `origin` on the `respond_auth` payload is diagnostics-only and does not
  influence which pending request resolves. A wrong/guessed `request_id` is a no-op
  (`authorization.rs:261-268`, exercised by `resolve_ignores_wrong_request_id`). The window label is a
  truncated SHA-256 of the request_id (`commands.rs:145-149`), used only for window addressing, not for
  authorization. `lock_mutate_save`'s dedup check (`config.rs:185-195`) runs entirely under one writer
  lock acquisition per call, so N piggybacked handlers resolving the same `Allow{remember:true}`
  decision cannot double-append the same origin — verified by tracing the lock scope, not merely
  assumed.
- **`AuthorizationManager` lock/race**: a single `parking_lot::Mutex<PendingState>` guards both the
  `by_origin`/`by_request` indexes via `insert`/`remove`, which update both maps in one call
  (`authorization.rs:181-207`) — no window where the two indexes disagree.
- **`MAX_PENDING_ORIGINS` cap** (`authorization.rs:162`, `250-252`): considered as a bounded (≤60s),
  single-session availability effect of the cap itself (an attacker controlling ≥10 distinct origins
  could transiently block a new legitimate popup) but this is the cap's own documented, intentional
  trade-off (comment at `authorization.rs:161-162`) rather than a bypass of authorization, confidentiality,
  or integrity — not reported per "do not manufacture findings to fill space."

No file:line trace in this cluster's scope demonstrates crossing a confidentiality/integrity/
authorization boundary that isn't already covered by an existing SEC-0N/q7e3 control or an inherent,
non-bypassable browser/OS guarantee.
