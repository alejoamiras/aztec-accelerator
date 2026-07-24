# C6 / F-007 — bb-cache-integrity — lessons

Cluster: `sechard/bb-cache-integrity` · Finding: F-007 · Tier: mid (codex + fable dual audit + final codex)

## The bug
`scripts/download-bb.ts` fetched + untarred `bb` into the runtime-trusted version cache with ZERO digest
verification, and the Rust runtime (`bb.rs::find_bb`, `server/prove.rs`) trusted a cache hit purely by
`exists()` — never re-running `downloader.rs::verify_digest`, and on a rejected entry falling through to the
sidecar/`~/.bb`/`$PATH` bb (a silent wrong-version / unverified execution over the private witness).

## Key discovery (research)
The Rust DOWNLOAD path already implemented ~80% of the master F-007 invariant — `verify_digest`
(fail-closed), `install_version_dir` (`.tmp` staging + atomic rename), `extract_bb_from_tarball`
(symlink/decompression-bomb/declared-size rejection), and `finalize_downloaded_binary` (codesign). What was
missing was the **marker**, the **runtime re-verification**, the **fail-closed no-fallback**, and reordering
so codesign+marker happen pre-publish. The TS path lagged far behind (no verify, no staging, no marker).

## GATE 1 — blueprint (mid): codex consults

Dual audit + two fresh final-codex passes; each round caught real defects. Codex is advisory; every finding
here was legitimate and folded.

- **Dual audit (r1):** Fable = conditional-approve; Codex = **reject**. Both converged on: (a) the plan had
  no ACTUAL re-download trigger and a fail-OPEN `find_bb` fallthrough (silent downgrade); (b) marker must be
  the FINAL binary hash (post macOS codesign). Codex added: the plan DROPPED the campaign's own
  safe-publication invariant (private staging + unsafe-member rejection + atomic publish + structured
  marker with archive+binary digests) and the validation gate wasn't real (`download-bb.ts` is root
  `scripts/`, not covered by `packages/accelerator test:unit`).
- **Final-codex r2:** **reject** — 10 operational gaps. Most important: `find_bb(Option<&AztecVersion>)`
  can't tell a bundled `Some(v)` from a non-bundled one, so "present-invalid ⇒ hard-Err" would either break
  bundled requests or keep the hole. FOLD: `resolve_version` normalizes a bundled request to `version: None`
  (bundled ships as the sidecar), making every `Some(v)` unambiguously non-bundled. Plus: `BB_BINARY_PATH`
  scoped as a trusted override; fail-closed delete-then-rename (not "atomic replacement"); `.tmp` staging
  excluded from inventory; TS bounded-stream + codesign-abort + version-validation + Windows-reject; real
  gate (root `test:scripts` + `--manifest-path` on every cargo gate); marker schema/version/platform
  binding; in-use eviction exemption; shared cross-language fixture; corrected hash-count + digest wording.
- **Final-codex r3 (confirming):** **reject** — converged to completeness nits + ONE control-flow
  tightening: once bundled→None, `find_bb(Some(v))` is ALWAYS non-bundled, so an ABSENT cache entry must
  ALSO hard-error (r2's "absent ⇒ fall through" would keep a wrong-version fallback on a between-resolve-
  and-exec eviction or a direct `bb::prove(Some(v))`). Also: per-run-unique `.{v}.tmp.<rand>` staging (a
  fixed `.tmp` is shared by Rust+TS → concurrent stomp); TS listing must also skip dot-prefix + invalid
  names; canonical-hex marker digests + 0700/0600-at-creation; expanded negative tests; CI filters add the
  test + fixtures; Windows CI runs core tests.

**DECISION:** closed GATE 1 after r3 rather than a 4th planning pass — the design was settled (dual audit + 2
fresh finals, converging to spec-completeness), and the remaining items are naturally enforced by the phase
gates + GATE 3 (post-impl audit on the ACTUAL diff). Codex is advisory; diminishing returns vs the real-code
audit. **Lesson (reinforces C5):** fold findings into the OPERATIVE plan text (Design/Phases/Assumptions),
not amendment-only sections — r2/r3 both re-checked that the operative bodies matched the ledger.

## GATE 2 — implementation notes
- `find_bb(Some(v))` now returns `versions::verify_cached_bb(v)` directly — ANY failure (absent/tampered/
  unreadable) is a hard `Err`; the sidecar chain is reachable ONLY from `find_bb(None)`.
- `verify_cached_bb` refactored to a path-parameterized `verify_bb_entry` / `read_bb_marker_at` so it is
  unit-testable WITHOUT the home-bound cache dir (Rust does NOT honor `BB_VERSIONS_DIR` like the TS side —
  only the TS tool does, and only for tests; both default to `~/.aztec-accelerator/versions` in production).
- Eviction exemption extracted to a pure `evictions()` helper (testable without home-binding) instead of a
  fragile HOME-swap test.
- clippy `-D warnings` caught the now-unused `version_bb_path` import in downloader.rs (moved to a
  test-scoped import).

## Validation (per-phase, all local)
- Phase 1: `bun test scripts/download-bb.test.ts` — 29 tests (verify/stage/tar-safety/marker/publish/skip/
  retention). Phase 2/3: `cargo test --manifest-path packages/accelerator/core/Cargo.toml` — 171 tests;
  clippy `-D warnings` + fmt clean. Phase 4: `bun run lint:actions`.
- GATE 4: full `bun run test` (exit 0: 54 root-scripts + 45 sdk + 73 playground + 6 accelerator) + 171 core
  + actionlint. Verified recent Aztec releases (rc.1/rc.2) expose the GitHub asset `digest` (download path
  is live for the versions users request; pre-June-2025 releases fail closed — documented).

## GATE 3 — post-impl codex audit (on the real 4-commit diff)
VERDICT: **changes-requested** (2 MEDIUM). Codex confirmed the core security properties (no CRITICAL/HIGH
unverified-execution path): fail-closed `find_bb(Some(v))`, marker schema/version/platform binding +
streamed rehash, codesign-before-hash ordering, staging 0700 / marker 0600, bundled→None, CI wiring all
verified. Blockers folded:
- **MEDIUM-1 (real):** the TS path was extract-then-inspect via system `tar -xzf` with only the COMPRESSED
  64 MB cap — a gzip bomb / huge member could exhaust disk before `findSingleBb`, and hardlink identity is
  lost after extraction (the claimed hardlink rejection wasn't implemented). FOLD: `gunzipSync` with a
  cumulative `maxOutputLength` cap (verified Bun enforces it → `ERR_BUFFER_TOO_LARGE`) BEFORE extraction;
  `findSingleBb` rejects `nlink>1`; `sha256File` streams the hash. Now mirrors the Rust `CappedReader`.
- **MEDIUM-2:** the test gate omitted most promised negatives. FOLD: added hardlink / dir-`bb` /
  wrong-platform / invalid-version-before-fs / corrupt-archive / mode / gzip-bomb-primitive TS tests + the
  Rust `github-release-metadata.json` cross-language fixture test.
- **LOW:** crash-stale `.{v}.tmp.*` stages now reaped before each install (both paths); TS stage mkdir made
  strict/non-recursive; README "atomic" → "fail-closed delete-then-rename". The multi-instance / concurrent-
  publisher eviction race stays fail-closed (verify-on-use catches it; advisory lock deferred, A7).

**Lesson:** GATE 3 (audit on the ACTUAL diff) caught a real gap the plan-stage audits missed — the plan SAID
tar-safety + decompression caps, but the TS *implementation* leaned on system `tar` which doesn't enforce a
cumulative decompressed cap or expose entry types. Plan-level "reject unsafe members" is not self-executing;
the impl must actually stream+bound. This is exactly why GATE 3 exists.

## Git pre-flight
`origin/security-hardening` unchanged since branch point (merge-base == its HEAD) → no rebase/conflicts.
