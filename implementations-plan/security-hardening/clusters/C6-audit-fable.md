# C6 / F-007 — FABLE dual-audit. VERDICT: conditional approve (4 conditions).

Defense-in-depth (plan) BEATS the marker-only outline decisively (marker-only makes download-bb.ts a
bandwidth-wasting no-op + moves detection to prove-time). Keep it. Findings:

- **B1 (High) — no implemented re-download trigger + a FAIL-OPEN fall-through.** `prove.rs:80`
  `needs_download = v!=bundled && !version_bb_path(v).exists()` → a legacy/tampered entry EXISTS →
  never re-downloads. `downloader.rs:24 download_bb` early-returns on exists() (no marker check).
  `download-bb.ts:68` skip-if-exists. WORST: find_bb rejecting a cache entry then FALLING THROUGH to
  sidecar/`~/.bb/bb`/`$PATH` = silent downgrade to a wrong/unverified bb. FIX: (a) find_bb cache-hit
  rejection = hard Err for the requested version, NO fall-through; (b) prove.rs:80 `needs_download` via
  `verify_cached_bb` not exists(); (c) download_bb:24 early-return only when the marker validates, else
  fall into install_version_dir (atomic wholesale replace); (d) download-bb.ts:68 skip only when marker
  present+matching.
- **B2 (High) — macOS codesign mutates the binary AFTER the marker is computed.** Both paths
  `codesign --force --sign -` post-extract → rewrites bytes → marker (hashed pre-codesign) mismatches on
  every Mac cache hit → reject loop on VALID entries. FIX: marker = sha256 of the binary AFTER ALL
  mutations (post finalize_downloaded_binary / post-codesign) in both paths + an ordering unit test. This
  also makes A3 (store binary sha256, not tarball digest) NECESSARY, not just preferable — the tarball
  digest cannot validate the re-signed binary offline.
- **M3 — re-hash in list_cached_versions is a status/tray hot-path DoS** (server.rs:292 + tray.rs:57,
  polled; N×100MB per poll). FIX: list_cached_versions checks marker EXISTENCE only (cheap stat); full
  re-hash confined to find_bb/verify_cached_bb. Then prove-path = one hash (~100-300ms, sha2 HW) vs
  multi-second proves → skip the per-process cache (dodges TOCTOU). Inherent hash→spawn TOCTOU is
  at-rest integrity, not exec-time — say so.
- **M4 — TS digest fetch fidelity + rate limits.** Mirror release_metadata.rs:83-120 EXACTLY
  (accept: application/vnd.github+json; match asset name; strip `sha256:`; non-2xx → Ok(None) → HARD
  error, not skip). Risks: unauthenticated api.github.com = 60/hr/IP → bulk script + CI trips it → add
  optional GITHUB_TOKEN auth (Rust too). The asset `digest` field is recent → old releases may lack it →
  Phase 4 verify bundled/pinned versions expose digests. REUSE copy-bb.ts's assertSha256 + capped-fetch.
- L5: marker residual framing correct (on-disk marker doesn't stop a local writer even storing GitHub's
  digest; needs online re-fetch or HMAC/ACL — future work). L6: Phase-1 gate wrong — scripts/ tests run
  via root `bun run test`, not `--cwd packages/accelerator`. L7: TS non-atomic install acceptable
  (fails closed later); optionally mirror install_version_dir's temp+rename.

Critical files: bb.rs, versions/downloader.rs, server/prove.rs, scripts/download-bb.ts, cache_layout.rs.
