# Lessons — #99 accelerator audit fixes (all 4 phases, one implementation pass)

The four findings touch overlapping files (`versions.rs` carries Phases 1, 2, 4), so they were
implemented + validated together rather than as four separate commits.

## Phase 1 — `download_bb` OOM → bounded streaming
- Swapped `response.bytes().await?` (whole-body buffer) for `response.chunk()` in a
  `while let Some(chunk) = response.chunk().await?` loop with a running `bytes.len() + chunk.len()`
  counter against a 32 MB cap. Bound `let mut response = response;` first (`chunk()` takes `&mut self`).
- **No new crate.** `Response::chunk()` is in reqwest's base async API (codex confirmed it on the pinned
  `reqwest 0.12.28`, not gated by the `stream` feature). Avoided the `futures-util::StreamExt` +
  `bytes::Bytes` route an earlier plan draft used (those aren't deps → wouldn't have compiled).
- `bytes` is now `Vec<u8>`; `&bytes` coerces to `&[u8]`, so `sha256_hex(&bytes)` +
  `extract_bb_from_tarball(&bytes, …)` are unchanged. Digest verification still runs on the fully
  buffered, size-capped body — supply-chain anchor intact.
- Kept the advertised-`content_length()` early-reject as a fail-fast (the per-chunk counter is the real
  ceiling for chunked-encoding servers that omit it).

## Phase 2 — `..` path-traversal guard, centralized
- **Codex final-pass refinement (adopted):** the sink guard must encode the *full* invariant (charset +
  length + dots), not just dots — otherwise a *direct* `download_bb` caller could still pass
  slash/backslash traversal that only `server.rs` ingress blocked.
- So `is_valid_version` was **moved to `versions.rs` as `pub fn`** (single source of truth) with the new
  `!starts_with('.') && !contains("..")` clauses. `server.rs::resolve_version` now calls
  `versions::is_valid_version`; `download_bb` calls it as its **first line**, before `version_bb_path`,
  network, or fs (the `remove_dir_all(version_dir)` sink).
- Consolidated the two duplicate `is_valid_version` unit-test blocks that lived in `server.rs` into one
  canonical block in `versions.rs` (next to the fn), extended with `..`/`.`/`.foo`/`1..2`/leading-dot.
  Added `download_bb_rejects_unsafe_version_at_sink` (async) asserting the guard fires (err contains
  "invalid version") with no network — exercises the sink independently of the HTTP path (codex ask).

## Phase 3 — `bb.rs` stderr char-boundary panic
- Extracted `truncate_stderr(&str) -> String` (was inline in `prove`) so it's unit-testable in isolation.
- **Both** the condition and the slice are char-based: gate on `chars().count() > 500`, cut with
  `chars().take(500).collect()`. A sub-500-char but >500-byte string (e.g. 300 emoji = 1200 bytes) is left
  whole and **not** mislabeled `[truncated]` (the original `len() > 500` byte test would have mislabeled it).
- Test: 600×`é` (truncates, 600 chars), 300×emoji (untouched), 500×`x` (boundary, untouched).

## Phase 4 — simplifications
- Added `hex = "0.4"` (already present in `Cargo.lock` as a transitive dep → only the accelerator dep-list
  line changed; `[[package]] hex 0.4.3` block pre-existed → `--frozen-lockfile` safe). `hex::encode` is
  lowercase, byte-identical to the old `write!("{b:02x}")` loops, so the GitHub-API digest comparison is
  unaffected. Dedup'd `sha256_hex` (versions.rs) + `sanitize_window_label` (commands.rs).
- `versions_to_evict`: replaced the O(n²) `while versions.len() > effective_limit { remove(0) }` with
  `to_evict.extend(versions.drain(0..versions.len().saturating_sub(effective_limit)).cloned())`.
  **Kept `effective_limit`** (load-bearing — both audits caught that the bundled item counts toward the
  tier cap, so it's `limit-1` when bundled is in-tier). `drain` on `Vec<&String>` yields `&String`;
  `.cloned()` → `String`, matching the old `.remove(0).clone()`. Order preserved (front = oldest).

## Validation
- `cargo fmt --check` clean. `cargo test --lib`: **122 passed / 0 failed** (4 new tests green; all eviction
  tests — `bundled_version_never_evicted`, `evict_excess_nightlies`, `mixed_tiers`,
  `rc_versions_sort_numerically` — still green, confirming the `drain` refactor is behaviorally identical).
- `bun run test` exit 0 (TS unchanged). `bun run lint` exit 0 (biome's 1 warning is a pre-existing unused
  var in an SDK test, not this diff). `bun run lint:actions` clean (no workflow changes).
