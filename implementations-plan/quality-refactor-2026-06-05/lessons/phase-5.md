# Phase 5 — `download_bb` split (Q11) + codex post-impl audit of Q3/Q10

## Codex post-impl audit of Q3 (AztecVersion) + Q10 (ServerStatus) — session 019e9a1e
Ran `/codex xhigh` (read-only) on the merged Q3+Q10 refactors. **Verdict: "at-risk — no currently
reachable app-level behavior/security break, but Q3 narrows the safety guarantee and weakens the
regression net, so 'strictly behavior-preserving' is too strong."**

**Looks fine (codex confirmed):**
- No `is_valid_version` vs `AztecVersion::parse` divergence — `parse` is a straight wrapper; `Display`/
  `Deref`/`AsRef` emit identical bytes.
- No unvalidated path into `remove_dir_all`: `/prove` parses once at ingress; `download_bb` only takes
  `&AztecVersion`; `cleanup_old_versions` only removes `read_dir` names (can't synthesize `/` traversal).
- Q10 `display_text()` strings exact; `is_busy()` matches the old spinner for the only emitted states.

**Findings + disposition:**
- **Med-1 (raw-path helpers not fully encapsulated):** `version_bb_path`/`find_bb` are still `pub` +
  take raw `&str`. Non-destructive today (they build paths; the destructive sink is `download_bb`, now
  typed). *Accepted* — closing this means typing the whole module surface; out of Q3's behavior-
  preserving scope. Logged for a future hardening pass.
- **Med-2 (sink-boundary regression net weakened):** the old `download_bb_rejects_unsafe_version_at_sink`
  exercised the destructive sink with unsafe input; retargeting it to the ctor lost that. *Partially
  mitigated already* by `resolve_version_rejects_invalid_version` (ingress 400) + `AztecVersion`'s
  **private fields** (so `parse` is the ONLY constructor — an unchecked one can't be added externally).
  *Accepted* for the merged Q3; no live bug.
- **Low (crate-API breaks):** `download_bb(&str)→(&AztecVersion)`, `StatusCallback` signature. Expected —
  this lib isn't published as a Rust crate (it's the app's internal lib + the headless server consumes it,
  which we recompiled). Not behavior-preserving at the *crate* boundary, but is at the *app* boundary.

**Direct relevance to Phase 5:** codex's Med-1 (raw-path sink helpers) is exactly why the new
`install_version_dir(version_dir: &Path, bytes)` is kept **private + single-caller** — `download_bb` only
ever passes `versions_base_dir().join(version)` for a validated `AztecVersion`, so its `remove_dir_all`
never sees an attacker-derived path. (Taking `&Path` rather than `&AztecVersion` is purely for tempdir
unit-testability; the safety comes from the private/single-caller invariant, documented on the fn.)

## Q11 part 1 — extract `install_version_dir` (#TBD)
First Extract-Method unit: the extract-to-tmp + 2 `remove_dir_all` cleanup arms + atomic rename, pulled
out of `download_bb` into a private `install_version_dir(&Path, &[u8])`. Byte-identical: the temp dir is
still `.{name}.tmp` where `name = version_dir.file_name()` (== the validated version). Char test
`install_version_dir_replaces_stale_and_extracts_atomically` (plan's "atomic-rename-cleanup first") pins:
fresh extract, stale-entry wholesale replace, temp-dir cleanup — using the existing GzEncoder+tar::Builder
fixture pattern + a `tempfile::tempdir()` so it never touches the real `~/.aztec-accelerator`. 127 lib
tests + clippy -D warnings green.

## Q11 part 2 — extract `download_tarball` + `verify_digest` (#TBD)
Extracted the two cfg-free async units: `download_tarball(&str) -> Vec<u8>` (bounded-streaming GET with
the 64 MB cap) and `verify_digest(&str, &[u8])` (GitHub asset SHA-256 fetch + fail-closed compare).
`download_bb`'s orchestrator is now a clean sequence: `download_tarball → verify_digest →
install_version_dir → postprocess`, preserving the **verify-BEFORE-install ordering**. Pure byte-identical
extractions — covered by the gated real-download integration test. 127 lib tests + clippy -D warnings green.

**Deliberately left `postprocess` inline** (NOT extracted to `postprocess_unix`/`postprocess_macos` as the
plan sketched): the chmod is `#[cfg(unix)]` and the xattr+codesign is `#[cfg(target_os = "macos")]`, so
extracted fns would have **unused params on non-macOS** (`version_dir`/`version` only used by the macOS
cleanup-on-failure) → `clippy -D warnings` failures across the platform matrix, the exact cfg trap the
Q15 bin-crate lesson warned about. The inline `#[cfg]` blocks are already clearly delimited; extracting
them trades real cross-platform risk for cosmetic gain. Logged as a conscious scope cut.

## Next
Q11 done (modulo the noted postprocess decision) → `/code-review max --fix` on the complete `download_bb`
split → then the Q3-followup (`versions_to_evict` → `&[AztecVersion]`, using the precomputed
`tier()`/`sort_key()`) → Phase 6 (Q4 crash-recovery trait, SAFETY-CRITICAL, needs the rc dry-run).
LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-5.md
