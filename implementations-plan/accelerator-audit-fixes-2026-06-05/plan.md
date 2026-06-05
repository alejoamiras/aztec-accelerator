# Accelerator audit fixes (#99) — download OOM, `..` guard, stderr panic + simplifications

**Tier:** `/plan mid` (codex + opus dual audit → final codex). Verdicts inline.
**Status:** drafting → dual audit → approval gate.

## Scope (owner-confirmed)
The net-new findings from the full-depth audit (#98), **fixes + the safe simplifications**, one contained
Rust PR. All in `packages/accelerator/src-tauri/src/`. Cap = **32 MB**; testing = **unit tests +
streaming code-review**.

---

## Phases

### Phase 1 — [HIGH] `download_bb` OOM: stream + bounded counter
`versions.rs:262-342`. Today: a `MAX_DOWNLOAD_BYTES = 500MB` guard checks only the *advertised*
`content_length()` (skipped when absent), then `response.bytes().await?` (`:293`) buffers the **entire**
body into memory. A CDN omitting `Content-Length` can stream GBs → OOM (triggerable by an approved
origin's `x-aztec-version` → runtime download).
**Fix:** lower `MAX_DOWNLOAD_BYTES` to **32 MB** (owner's deliberate tighter choice; bb is ~5 MB — note:
`copy-bb.ts:87` uses 64 MB, so this is *tighter*, not a mirror; a future bb past 32 MB fails loudly →
one-line bump) and stream via **`response.chunk()`** (no new crate — avoids `futures-util`), aborting the
moment the buffer exceeds the cap:
```rust
const MAX_DOWNLOAD_BYTES: usize = 32 * 1024 * 1024;
// keep the advertised-length early-reject (fail fast) ...
let mut response = response;               // chunk() needs &mut
let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024 * 1024);
while let Some(chunk) = response.chunk().await? {
    if buf.len() + chunk.len() > MAX_DOWNLOAD_BYTES {
        return Err(format!("bb v{version} download exceeded {MAX_DOWNLOAD_BYTES} bytes — aborting").into());
    }
    buf.extend_from_slice(&chunk);
}
// digest + extract are unchanged — sha256_hex (versions.rs:201) and extract_bb_from_tarball (versions.rs:399)
// both take &[u8], so pass &buf directly (no bytes::Bytes wrapper).
```
Deps: none new (`reqwest` already has `stream`, Cargo.toml:46; `Response::chunk()` is in the base API).
Mid-stream abort leaves no temp dir (extraction starts later). Confirmed no other `response.bytes()` in src-tauri.

### Phase 2 — [MED] reject `..` at the version ingress gate
`server.rs:282` `is_valid_version` allows `.`, so bare `..` passes (the existing
`is_valid_version_rejects_path_traversal` test only covers slash inputs, rejected by the charset). Then
`version_bb_path("..")` → `versions/..` and `download_bb`'s `remove_dir_all(version_dir)` could target
`~/.aztec-accelerator/` itself. Currently unreachable (fail-closed digest rejects `..` first) but one
refactor from data-loss.
**Fix:** add to `is_valid_version`: `&& !version.starts_with('.') && !version.contains("..")` — rejects
`.`, `..`, `1..2`, leading-dot; accepts `5.0.0`, `5.0.0-rc.1`, `5.0.0-nightly.20260301`,
`4.2.0-aztecnr-rc.2` (the charset already rejects `/`, `\`, abs paths, unicode dots). Extend the test to
assert bare `".."`, `"."`, `".foo"`, `"1..2"` are rejected + the valid formats still pass.
**Plus defense-in-depth at the SINK (codex):** `download_bb` + `version_bb_path` are *public* helpers, and
`download_bb` does `remove_dir_all(version_dir)`. Both auditors confirmed `resolve_version` (server.rs:428)
is the only runtime caller today, but to be safe regardless of caller, **add the same reject-`.`/`..` guard
at the top of `download_bb`** (return Err before touching the filesystem). Cheapest centralization: a small
`fn is_safe_version_component(&str) -> bool` shared by `is_valid_version` and `download_bb`.

### Phase 3 — [LOW] `bb.rs` stderr slice panic
`bb.rs:133` guards `if stderr.len() > 500` then slices `&stderr[..500]` (`bb.rs:136`) — which panics if
byte 500 lands mid-codepoint (`stderr` is `from_utf8_lossy`, valid UTF-8, but a multibyte char can
straddle 500). **Fix (both condition + slice must be char-based — codex):** truncate with
`stderr.chars().take(500).collect::<String>()` and gate the `[truncated]` label on
`stderr.chars().count() > 500` (a sub-500-*char* but >500-*byte* string must NOT be mislabeled truncated).
Unit-test with a >500-char multibyte string (repeated `é`/emoji) — asserts no panic + correct labeling.

### Phase 4 — Simplifications (safe, same files)
- **hex dedup:** `versions.rs:201` `sha256_hex` and `commands.rs:124` `sanitize_window_label` both
  hand-roll a `write!("{b:02x}")` hex loop. Add `hex = "0.4"` (tiny, ubiquitous) and use `hex::encode(...)`
  in both (`hex::encode(Sha256::digest(data))` / `hex::encode(&hash[..6])`).
- **`versions_to_evict`** (`versions.rs:148-181`): **KEEP `effective_limit`** — both auditors caught that
  it's **load-bearing**, not dead: the bundled item counts toward the tier retention limit, so when bundled
  is in-tier the cap is `limit-1`. Dropping the `-1` over-retains and breaks `bundled_version_never_evicted`
  (4 nightlies, limit 2, bundled-in-tier → must evict 2, not 1). The ONLY safe simplification is replacing
  the O(n²) `while versions.len() > effective_limit { remove(0) }` with one drain:
  `to_evict.extend(versions.drain(0..versions.len().saturating_sub(effective_limit)).cloned());`. All
  eviction tests must stay green (verify against `bundled_version_never_evicted`, `evict_excess_nightlies`,
  `mixed_tiers`, `rc_versions_sort_numerically`).
- (`open_in_browser` → `tauri-plugin-opener` — explicitly **out of scope**, deferred.)

---

## Assumptions

### Facts (verified)
- `download_bb` buffers via `response.bytes()` after a content-length-only guard: `versions.rs:284,293`.
- `is_valid_version` charset allows `.`: `server.rs:282`; existing tests `server.rs:1367,1375` don't cover
  bare `..`. `version_bb_path` joins the version: `versions.rs:84`. `remove_dir_all(version_dir)` in
  `download_bb` (`~:341`).
- `&stderr[..500]`: `bb.rs:133`. Hand-rolled hex: `versions.rs:201`, `commands.rs:124`.
  `versions_to_evict`: `versions.rs:148-181`.

### Inferences (attack these)
- `Response::chunk()` is in the base reqwest API (no new crate); digest + extract take `&[u8]` so operate
  on `&buf` directly. VERIFIED: reqwest has `stream` (Cargo.toml:46); both auditors found no other
  `response.bytes()` in src-tauri; the response is bound `mut` for `chunk()`.
- 32 MB is ample for bb (~5 MB, ~6× headroom); a future bb past 32 MB fails loudly → one-line bump.
  (copy-bb.ts:87 is 64 MB; 32 is the owner's deliberate *tighter* choice, not a mirror.)
- Adding `hex` is acceptable (Rust crate, not npm; the 7-day min-age is an npm policy).
- `versions_to_evict`'s `effective_limit` (`-1` when bundled is in-tier) is **load-bearing** (both audits);
  only the O(n²) `remove(0)`→`drain` is the safe simplification.

### Asks (resolved)
- Scope (fixes + safe simplifications), cap (32 MB), testing (unit + review) — all decided.

---

## Security & Adversarial Considerations
- **Threat model:** an *approved* web origin (or compromised CDN) drives a runtime `bb` download via the
  `x-aztec-version` header. The OOM (unbounded buffer) and the `..` (path escape → `remove_dir_all` on the
  data dir) are the two real surfaces; both are gated behind origin approval + the fail-closed digest, but
  defense-in-depth closes them.
- **Supply chain:** the integrity anchor is unchanged — GitHub's API `digest` verified before use
  (fail-closed; no-digest → reject). The streaming fix preserves the digest check (it runs on the fully
  buffered, size-capped `Bytes`). No new download trust assumptions.
- **Least privilege / input validation:** `is_valid_version` is the single ingress validator; tightening it
  is the right layer. `hex` is a pure-compute dep (no I/O, no unsafe in the used path).
- **Crypto:** sha2 (existing) for the digest; `hex` only formats. No hand-rolled crypto.

---

## Audit verdicts
- **Codex (xhigh):** approve-with-changes — **all adopted**: `response.chunk()` (no `futures-util`),
  defense-in-depth guard in `download_bb`, keep `effective_limit`, char-based stderr condition, 32 MB is
  ours-not-copy-bb.ts's. Transcript: `audit-codex.md`.
- **Opus subagent (Plan):** needs-rework → **all adopted** (same core findings: `effective_limit`
  load-bearing, deps wouldn't compile, `bb.rs:136` cite). Transcript: `audit-opus.md`.
- **Final fresh-context codex pass:** _pending (on the revised plan)._

---

## Seeds

### /goal
```
/goal All 4 phases ✓ in implementations-plan/accelerator-audit-fixes-2026-06-05/plan.md (download_bb streams with a 32MB bounded counter — no full-body buffer; is_valid_version rejects bare `..`/`.`/leading-dot/`..`-sequence with extended tests; bb.rs stderr truncates char-safely; hex::encode dedup + versions_to_evict simplified with eviction tests still green); per phase `LESSONS_FILE=implementations-plan/accelerator-audit-fixes-2026-06-05/lessons/phase-N.md` printed; `/code-review max --fix` applied + committed; codex post-impl audit clean (or high/critical addressed); `cargo test` + `bun run test` + `bun run lint:actions` exit 0 in transcript.
```

### /loop
```
/loop Each turn: read implementations-plan/accelerator-audit-fixes-2026-06-05/plan.md + lessons/; git status; open PR? gh pr view --json statusCheckRollup. CI on HEAD? gh run watch ≤10min. Failed? triage+fix, /codex xhigh if non-trivial, commit+push; stop after 5 fails. Phase green? mark ✓, file lessons/phase-N.md, print LESSONS_FILE=…, advance (HIGH OOM → `..` → stderr → simplify). Nothing in flight? next pending step (edit → cargo test + bun run test → bun run lint:actions → commit → push). All ✓? /code-review max --fix → commit → codex post-impl audit (adversarial+security) → address high/critical → stop. NEVER merge to main autonomously — surface the PR.
```
