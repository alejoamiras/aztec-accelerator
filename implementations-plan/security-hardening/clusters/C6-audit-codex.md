The chosen defense-in-depth direction is safer than runtime-marker-only, but the plan is not implementable as “fail closed + redownload” yet.

1. **CRITICAL — rejection does not cause redownload and may execute the wrong bb.** The plan assumes an invalid marker “falls through” and the caller redownloads ([C6-plan.md](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/clusters/C6-plan.md:36)). In reality, `resolve_version` decides solely from `bb.exists()` ([prove.rs](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:80)), and `download_bb` also returns immediately on existence. Then `find_bb` can fall through to bundled, `~/.bb`, or `$PATH` ([bb.rs](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/bb.rs:30))—potentially executing a different version over the witness. Missing/malformed/mismatched markers must be treated as a cache miss by resolution and downloader, replaced through a verified download, with no generic fallback for a requested non-bundled version. Add an end-to-end test proving legacy/tampered cache → one redownload → requested cached path, and download failure → no execution.

2. **HIGH — the plan drops the campaign’s safe-publication invariant.** The master requires private staging, unsafe-member rejection, and atomic publication of binary plus a marker containing archive and binary digests ([master plan](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/plan.md:50)). C6 instead retains Bun’s unbounded `arrayBuffer()` and whole-archive extraction directly into the live directory ([download-bb.ts](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:87)). Rust currently renames the directory before chmod, macOS signing, and marker creation. Both paths should stage privately, extract only one regular `bb` with compressed/decompressed limits, finalize/sign, write the structured marker, then publish the complete directory atomically.

3. **HIGH — the proposed validation gate is not real.** `packages/accelerator` has `test:unit`, not `test`; its tests cover `packages/accelerator/scripts/`, while `download-bb.ts` is in root `scripts/` ([package.json](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/package.json:7)). Specify an explicit root fixture test and ensure `accelerator.yml` invokes it. Required cases: unsafe archive members/symlinks, size limits, partial/crash publication, malformed/oversized marker, legacy redownload, mismatch redownload, no wrong-version fallback, and marker computed after macOS signing.

4. **MEDIUM — hashing every listed version makes `/health` an I/O amplifier.** `list_cached_versions()` feeds every detailed health request ([server.rs](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server.rs:291)); hashing every binary there can synchronously read gigabytes, especially with unlimited mainnet retention. Fresh streamed hashing immediately before each prove is reasonable relative to proving cost; benchmark cold/warm disk and avoid loading the whole binary into memory. Do not add a per-process execution cache absent evidence: it expands the local-write/TOCTOU window from one hash-to-exec interval to the process lifetime. Listing should use a bounded/background verification cache or cheap marker state; execution remains freshly verified.

5. **MEDIUM — marker semantics need precise claims.** Store both verified archive digest and final binary digest because that records provenance and satisfies the master invariant. But merely storing GitHub’s tarball digest does **not** make binary replacement require a collision: tarball and extracted binary are different objects, and a local writer can edit the binary-digest field too. Collision resistance against that attacker requires an authenticated publisher-provided binary digest/signature or re-derivation from a retained authenticated archive. The same-UID local-writer residual is acceptable for F-007 if explicitly excluded; otherwise this design does not close it. Fresh hashing also retains a narrow hash-to-`exec` TOCTOU.

6. **MEDIUM — tighten the Facts/Inferences.** GitHub currently documents `digest: "sha256:…"` and says release-asset digests are generated at upload and immutable ([GitHub release-asset documentation](https://docs.github.com/en/rest/releases/assets), [GitHub announcement](https://github.blog/changelog/2025-06-03-releases-now-expose-digests-for-release-assets/)). Mirror the existing helper in `release_metadata.rs`, including exact asset matching and fail-closed non-2xx/missing/malformed handling ([release_metadata.rs](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/release_metadata.rs:83)). API availability and rate limits remain failure modes. Also, ordinary platform TLS validation is not “certificate pinning,” and a GitHub release/account/CI compromise can replace both asset and GitHub-computed digest.

**Competing outline:** reject runtime-marker-only. It delays detection until first prove, is broken by today’s `exists()` short-circuits, and leaves `bb:download` reporting success for an unusable/unverified cache. The small TS/Rust metadata-fetch duplication is worth paying; share contract fixtures to prevent drift.

Fail-closed legacy behavior is the right default, but explicitly document that offline unmarked caches will fail/fallback until an online verified redownload. Current Rust downloads already require this GitHub digest, so the newly affected population is principally Bun-produced and legacy caches.

VERDICT: reject (blocking findings: invalid-cache redownload/fallback control flow; missing staged safe atomic publication and real marker chain)
---

## Round 2 — final fresh-context pass on the REVISED plan (VERDICT: reject)

The prior findings are visibly folded into the prose, but several folds are not operationally complete. I would not approve this blueprint yet.

### 1. Re-download control flow

The intended chain is stated correctly in [C6-plan.md:58](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/clusters/C6-plan.md:58): `verify_cached_bb`, `needs_download`, marker-gated skips, and hard error instead of sidecar fallback.

Two holes remain:

- `find_bb` cannot implement the promised bundled exception. Its API receives only `Option<&AztecVersion>` ([bb.rs:21](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/bb.rs:21)); it does not know whether `Some(v)` equals the bundled version. `resolve_version` keeps bundled requests as `Some(version)` while merely setting `needs_download = false` ([prove.rs:75](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:75), [prove.rs:85](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:85)). Therefore either every `Some` hard-errors—breaking explicit requests for the bundled version—or every `Some` may fall through—retaining the vulnerability. The plan must normalize bundled requests to `None`, pass bundled context, or redesign the resolved-path API.

- `BB_BINARY_PATH` remains a priority-zero bypass, accepted solely by `exists()` before any version-cache verification ([bb.rs:22-27](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/bb.rs:22)). After a verified re-download, a requested non-bundled proof still executes that possibly wrong-version binary. If this is an intentional trusted override, the plan must explicitly scope and test it; currently it contradicts “NO wrong-version/unverified execution.”

The Rust and TS skip-if-exists changes themselves are properly specified at C6-plan.md:63-66.

### 2. Safe publication

The desired ordering is now correct: finalize/codesign, hash the resulting binary, write the marker, publish ([C6-plan.md:45-55](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/clusters/C6-plan.md:45)). The post-codesign fold is real.

Remaining publication gaps:

- “Rename staging → live, replacing any prior” is not a portable atomic operation for a non-empty destination directory. The current Rust implementation has to delete live first and then rename ([downloader.rs:206-210](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/downloader.rs:206)). The plan supplies no replacement/quarantine, locking, rollback, or crash-recovery protocol. Initial publication can be atomic; replacement of a bad cache is not specified.

- The TS “capped-fetch” reference is unsafe. `copy-bb.ts` calls `arrayBuffer()` before enforcing the actual-length cap when `Content-Length` is absent or false ([copy-bb.ts:110-120](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/scripts/copy-bb.ts:110)); current `download-bb.ts` does the same at line 87. A true bounded streaming reader, like Rust’s downloader at lines 130-150, must be required.

- Current TS codesigning ignores the subprocess status ([download-bb.ts:111-114](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:111)). “Finalize+codesign” plus an ordering test does not expressly require codesign failure to abort without publication.

- “Exactly one regular file named `bb`” is wrong for Rust’s Windows path, where `bb_binary_name()` is `bb.exe` ([cache_layout.rs:15-21](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/cache_layout.rs:15)). Either use the platform binary name or explicitly exclude unsupported platforms. TS’s current platform detection also maps every non-Darwin platform, including Windows, to Linux ([download-bb.ts:26-29](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:26)).

- TS versions remain raw CLI strings used in cache paths ([download-bb.ts:44-45](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:44), [download-bb.ts:203-206](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:203)). The plan never adds Rust-equivalent version validation. A traversal value can escape the cache, and error cleanup currently recursively deletes the derived directory. This becomes more dangerous once replacement publication is added.

### 3. Validation gate

The TS gate is still not real.

Root `bun run test` executes root lint/typecheck and then package-local unit scripts; its `test:unit` invokes only SDK, playground, and `packages/accelerator` tests ([package.json:24-28](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/package.json:24)). `packages/accelerator` runs `bun test scripts/` relative to that package ([packages/accelerator/package.json:12](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/package.json:12)); it will not discover root `scripts/download-bb.test.ts`. The plan must add `bun test scripts/download-bb.test.ts` to a root script or invoke it explicitly in CI.

Additionally:

- Phase 2 uses the correct core manifest command, but Phases 3 and 4 regress to bare `cargo test` ([C6-plan.md:89](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/clusters/C6-plan.md:89), [C6-plan.md:97](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/clusters/C6-plan.md:97)). There is no repository-root `Cargo.toml`; every gate must retain `--manifest-path packages/accelerator/core/Cargo.toml`.

- The current `accelerator.yml` filter excludes root `scripts/**` ([accelerator.yml:32-41](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/.github/workflows/accelerator.yml:32)). Phase 4 says to fix relevant paths, but it must add both root script/test paths and run the actual root test command above.

Missing negative gates include:

- absent/lying `Content-Length` exceeding the streamed compressed cap;
- cumulative decompressed-cap violation;
- symlink, hardlink, absolute path, `..`, extra member, duplicate `bb`, and wrong platform filename independently;
- codesign non-zero ⇒ no marker/no publish;
- wrong schema/version/platform and malformed/noncanonical digest;
- non-2xx, missing digest, malformed digest, and wrong-asset metadata responses;
- replacement failure/crash and concurrent publisher behavior;
- cross-language marker and release-metadata contract fixtures. The ledger mentions a shared fixture at C6-plan.md:21, but no phase actually creates or gates it.

### 4. `list_cached_versions`

The main hot-path fold is correct: Rust listing performs marker-existence checks only, while full hashing remains in `verify_cached_bb`; `/health` therefore avoids binary rehashing.

However, the proposed staging layout defeats that inventory rule. Staging is named `{version}.staging-<rand>` and receives its marker before publication ([C6-plan.md:48-53](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/clusters/C6-plan.md:48)). A marker-existence-only scan will expose active and crash-stale staging directories through `/health` ([server.rs:291-295](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server.rs:291)), the tray, and retention. Accumulated stale stages also restore an unbounded stat-scan amplifier. The plan must reserve and explicitly exclude a staging namespace and define race-safe stale-stage cleanup.

The TS `listCachedVersions` also remains based on `bb.exists()` ([download-bb.ts:48-59](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/scripts/download-bb.ts:48)); Phase 1 does not explicitly migrate its listing/retention behavior to committed-marker existence.

There is no `/health` full-hash amplification, but there are two hashes per normal non-bundled proof: proposed `resolve_version` hashes to determine `needs_download`, then `find_bb` hashes again before execution. Thus the “one-hash-per-prove” inference at C6-plan.md:140 is false. A per-request verified path/capability could avoid duplication without introducing a per-process cache.

### 5. Assumptions and residuals

Offline-unmarked behavior and the hash-to-exec limitation are documented in the right direction. The same-UID residual is recognized, but its bounding is inaccurate:

- A same-UID writer is not “higher privilege”; it is the same OS principal.
- A directory ACL ordinarily cannot stop another process running as that same UID. C6-plan.md:113-117 should not name ACLs as a solution unless publication moves to a distinct owner/service principal.
- The TOCTOU residual is not only a hostile local-writer case: the application’s own cleanup is spawned before `bb::prove` ([prove.rs:269-299](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/server/prove.rs:269)) and deletes cache directories ([version_policy.rs:210-215](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/packages/accelerator/core/src/versions/version_policy.rs:210)). It can therefore race verified-path execution without any attacker.

The GitHub `digest` field is generated for an uploaded asset, but “immutable” is not generally true. GitHub provides a separate immutable-release feature; only releases with that protection prevent asset modification/deletion. The release API even exposes an `immutable` state. See [GitHub’s immutable-release documentation](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases) and [REST release schema](https://docs.github.com/en/rest/releases/releases). Phase 4 checks digest presence, not release immutability. The residual account/CI-compromise statement is therefore right; the inference at C6-plan.md:138 is not.

Silent Asks remain around `BB_BINARY_PATH`, Windows support, marker schema evolution, and the portable replacement/recovery protocol.

### 6. New second-order risks

The most serious are:

- An old requested nightly can be downloaded and then immediately selected for eviction as the oldest excess version. Cleanup is spawned before execution, creating a real delete-versus-hash/spawn race.
- Active and crash-stale staging directories contain markers and are consequently treated as committed caches.
- Concurrent TS/Rust publishers and cleanup have no per-version coordination.
- `{schema,...}` is written, but verification only promises to compare `binary_sha256` ([C6-plan.md:58-59](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-bb-cache-integrity/implementations-plan/security-hardening/clusters/C6-plan.md:58)). There is no schema constant, rejection policy for unknown schemas, or binding check for marker `version` and `platform`.
- Crash safety is overstated: without a defined existing-directory replacement protocol, a crash can leave no live entry plus abandoned staging/backup directories. It should fail closed, but it is not the claimed atomic replacement.

VERDICT: reject (root TS tests do not run; requested/bundled and BB_BINARY_PATH control flow remains unresolved; portable atomic replacement and staging/retention coordination are unspecified; TS streaming caps/version validation and marker-schema/tar/codesign negative gates are incomplete)