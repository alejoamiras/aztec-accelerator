Changes requested. I found no CRITICAL/HIGH path that executes an unverified non-bundled cache entry, but two MEDIUM blockers remain.

## Findings

### MEDIUM — Bun extraction is post-hoc and not decompression-bounded

The 64 MiB limit applies only to compressed HTTP bytes ([download-bb.ts:198](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:198>)). The script then extracts the entire archive using system `tar` before inspecting its contents ([download-bb.ts:308](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:308>)).

Consequences:

- A small gzip bomb or huge extra member can consume unbounded disk/inodes before `findSingleBb` runs.
- The 512 MiB limit applies only to the selected `bb`, after extraction ([download-bb.ts:244](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:244>)).
- Ordinary non-`bb` files are silently accepted at lines 247–264.
- Hardlink identity is lost after extraction; `lstatSync(...).isFile()` accepts the resulting inode, so the claimed hardlink rejection is not implemented.
- Hashing also buffers the entire cached/final binary with `readFileSync` ([download-bb.ts:179](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:179>), [download-bb.ts:335](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:335>)); a permitted 512 MiB binary can cause a large synchronous allocation.

Stock GNU tar has default traversal and symlink safeguards, so I do not classify this as a demonstrated arbitrary-write primitive on the Linux runner. Those safeguards do not provide the promised cumulative size or single-member policy. [GNU tar security documentation](https://www.gnu.org/software/tar/manual/html_section/Security.html)

Fix by validating/streaming archive entries during extraction, enforcing a cumulative decompressed-byte cap, accepting exactly one regular `bb`, and streaming file hashes.

### MEDIUM — The security test gate omits most promised negative properties

CI correctly runs the test file, but the tests do not prove the claimed boundary:

- Streaming tests cover declared oversize and a normal body, not an absent/lying `Content-Length` whose actual stream crosses the cap ([download-bb.test.ts:210](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.test.ts:210>)).
- Archive tests cover a symlink, duplicate `bb`, and missing `bb`, but not hardlinks, traversal/absolute names, directory `bb`, generic extra members, wrong-platform names, or cumulative decompression ([download-bb.test.ts:226](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.test.ts:226>)).
- End-to-end tests do not cover codesign failure/no-publish, post-codesign mutation, staging/marker modes, strict stage exclusivity, Windows rejection, or invalid-version-before-filesystem behavior ([download-bb.test.ts:257](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.test.ts:257>)).
- Rust’s `find_bb` test proves only the absent-cache case, not tampered/unreadable cache with a planted sidecar/PATH fallback or a valid marked cache entry ([bb.rs:368](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/bb.rs:368>)).
- The install test proves ordering only where `finalize_downloaded_binary` is a no-op; it does not exercise actual macOS codesign mutation/failure ([downloader.rs:447](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/downloader.rs:447>)).
- Rust loads only `bb-cache-marker.json`; `github-release-metadata.json` is not cross-language-tested ([cache_layout.rs:376](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/cache_layout.rs:376>)).

This is blocking for a security gate because the missing cases include the unimplemented decompression/hardlink behavior above.

### LOW — Eviction protection closes only the triggering request’s race

The just-downloaded version is passed into the detached cleanup and exempted ([prove.rs:281](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:281>), [version_policy.rs:224](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/version_policy.rs:224>)). That closes the named download→cleanup→exec race for that request.

It does not protect against:

- A delayed cleanup spawned by an earlier request.
- Another accelerator instance.
- Concurrent `download-bb.ts` retention cleanup.

Those can delete a version during hash→spawn. The result remains fail-closed—hash/spawn failure, not unverified execution—but proof availability can still race.

### LOW — Crash-stale staging is never reaped, and TS creation is not strictly exclusive

Both implementations remove only their current stage on a handled error. A process crash leaves the unique stage indefinitely; the next install creates a different name instead of scanning stale stages ([downloader.rs:215](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/downloader.rs:215>), [download-bb.ts:308](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:308>)). Repeated crashes can accumulate hidden cache data.

Additionally, Bun uses `mkdirSync(..., { recursive: true })`, which succeeds if the randomly named stage already exists rather than failing on collision. Random collision is negligible and hostile precreation is within the excluded same-UID model, but it is not strict unique/exclusive creation.

Documentation also incorrectly calls replacement atomic, although it is delete-then-rename ([README.md:110](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/README.md:110>)).

## Confirmed security properties

- `resolve_version` normalizes an explicit configured bundled version to `None`; the resulting value is passed unchanged through download and into `bb::prove` ([prove.rs:50](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:50>), [prove.rs:263](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:263>), [prove.rs:315](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:315>)).
- Except for the documented `BB_BINARY_PATH` override, `find_bb(Some(v))` returns directly from `verify_cached_bb`; absent/tampered entries cannot reach sidecar, `~/.bb`, or PATH ([bb.rs:26](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/bb.rs:26>)).
- Download failure returns before proving ([prove.rs:299](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:299>)).
- Both download paths skip only after marker verification ([downloader.rs:19](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/downloader.rs:19>), [download-bb.ts:290](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:290>)).
- Rust marker validation correctly binds schema, version, platform, and canonical lowercase digests and streams the binary rehash ([cache_layout.rs:82](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/cache_layout.rs:82>), [cache_layout.rs:139](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/cache_layout.rs:139>), [cache_layout.rs:175](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/cache_layout.rs:175>)).
- Codesign runs before final hashing/marker creation, and codesign failure precedes live-directory deletion in both paths ([downloader.rs:227](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/downloader.rs:227>), [download-bb.ts:324](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:324>)).
- Rust creates the stage with Unix mode `0700` in the creation call; markers use `0600` ([downloader.rs:248](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/downloader.rs:248>), [cache_layout.rs:123](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/cache_layout.rs:123>)).
- Version validation, Windows CLI rejection, inventory exclusions, and protected TS retention are correctly wired ([download-bb.ts:86](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:86>), [download-bb.ts:373](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:373>), [download-bb.ts:408](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:408>), [download-bb.ts:436](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:436>)).
- CI paths include source, test, and fixtures; root tests and Windows core tests are genuinely invoked ([accelerator.yml:32](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/.github/workflows/accelerator.yml:32>), [accelerator.yml:191](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/.github/workflows/accelerator.yml:191>), [accelerator.yml:416](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/.github/workflows/accelerator.yml:416>)).

The marker is an unsigned digest record, not an authentication signature. Same-UID writers can forge/replay `{bb, marker}`, and hash→exec remains a same-UID TOCTOU. GitHub’s API digest also shares the artifact’s control plane, so an upstream account/release compromise can replace both; this is accurately documented at [release_metadata.rs:73](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/release_metadata.rs:73>) and is an accepted deferred trust boundary.

The named `c6-gate3.diff` was absent; I audited the clean four-commit implementation range `649bafe..0622df5`. Runtime tests could not execute because the supplied sandbox makes `/tmp` read-only, and `cargo` is unavailable; `actionlint` and `git diff --check` passed.

VERDICT: changes-requested (unbounded/post-hoc Bun extraction; incomplete fail-closed security test gate)