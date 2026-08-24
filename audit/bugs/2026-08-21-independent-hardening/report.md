# Bugs Report — independent-hardening

**Date**: 2026-08-21 · **Auditor**: ox-alpha (solo) · **Base**: main @ `9eff8dc`

## Findings

### IH-BUG-1 · Info — host.rs comment/code divergence on trailing dots
`server/host.rs:36` uses `trim_end_matches('.')` (strips ALL trailing dots) while the doc comment
says "strip one trailing dot". `Host: 127.0.0.1..:59833` is accepted (live-verified). Security
impact: none — the result is still a loopback literal. Fix: use `strip_suffix('.')`-once semantics
or correct the comment.

### IH-BUG-2 · Info — duplicated cfg attribute on win_acl module
`core/src/win_acl.rs:18` carries an inner `#![cfg(windows)]` while `lib.rs:19` already gates the
module with `#[cfg(windows)]`. Clippy (x86_64-pc-windows-gnu) warns "duplicated attribute".
Harmless today, but this is exactly the cfg-hygiene class the repo has been bitten by before
(a detached cfg silently changing what compiles). Fix: drop one of the two gates and add a
comment stating which is authoritative.

### IH-BUG-3 · Low — remote-controlled version download stalls a prove request
**AMENDED 2026-08-21 (post-report):** the original "indefinitely-ish" wording was wrong — the
downloader's shared HTTP client already bounds fetches at **300 s total / 30 s connect**
(`versions/release_metadata.rs::http_client`). Worst case is therefore roughly **10 minutes**
(tarball download + release-metadata/digest fetch are sequential, each individually bounded), not
unbounded. Two caveats keep this from being called a hard guarantee: the fallback
`unwrap_or_else(|_| reqwest::Client::new())` when the builder fails drops those explicit
deadlines, and the bound exists to protect legitimate slow downloads of large bb archives.
Closed as documented: no code change warranted.
Live observation that prompted the finding: `POST /prove` with `x-aztec-version: 1.0.0`
(uncached) triggered a GitHub download inside the request path; the client observed no response
within a 4 s window while the fetch ran. Bounded in aggregate (cache-size cap, digest
verification, single-flight via lease), but any approved origin can force the server onto network
fetches of attacker-chosen well-formed versions, extending that request's latency by
seconds-to-minutes and consuming bandwidth. Not a DoS of other requests (inflight cap + separate
download phase), purely latency/bandwidth — and bounded as above.

**STATUS 2026-08-21**: IH-BUG-1 and IH-BUG-2 FIXED in the `ih-hygiene` PR; IH-BUG-3 closed as
documented (amendment above).
Consider a configurable allowlist of downloadable versions or a short download budget per request.

## Verified-clean correctness checks

- Bind retry: hard-deadline tested; non-AddrInUse errors propagate immediately.
- Body read decoupled from prove permit; slowloris bounded by absolute deadline + inflight cap.
- Saturating size math in eviction paths (no debug panics / release wrap on sparse files).
- Crash recovery / config parsing: production paths error-or-read-only on malformed input;
  XML generation escaped; plist heal parses real XML before patching ProgramArguments[0] only.
- Config schema migration fails safe to read-only mode; strict typed deserializers throughout.

## Platform verification

- macOS (local): full TS suites + 263 core tests green.
- Windows-GNU cross-check: core crate clean (`cargo check --target x86_64-pc-windows-gnu
  --lib --all-targets`) — surfaced IH-BUG-2. src-tauri target check requires mingw-w64 (absent
  locally); covered by CI's windows-build lane.
- Linux: source-level reasoning only (no container verified available).
