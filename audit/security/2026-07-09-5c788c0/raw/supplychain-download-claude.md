# Cluster: supplychain-download — Claude findings

Scope: `packages/accelerator/core/src/versions/downloader.rs`, `packages/accelerator/core/src/versions/cache_layout.rs`, `scripts/download-bb.ts`, `packages/accelerator/scripts/copy-bb.ts`, `packages/accelerator/scripts/bb-version.ts` (+ the CI/IaC files that determine whether the Windows checksum-pin in `copy-bb.ts`/`update-aztec-version.ts` is actually review-gated, per the cluster brief).

Reviewed and cleared (no finding, with reasoning):
- `downloader.rs::verify_digest` — fail-closed on every path (missing digest `Ok(None)` → `Err`, fetch error → `Err`, mismatch → `Err`). No fall-through.
- `downloader.rs::extract_bb_from_tarball_capped` — decompression bomb: 64 MB compressed cap upstream (`download_tarball`) + 512 MB cumulative-decompressed cap enforced by `CappedReader` wrapping the *entire* archive stream (not just the matched entry), plus a per-entry declared-size pre-check. Symlink/hardlink entries named `bb` are rejected (`entry_type() != Regular`). Tested (`extract_bb_rejects_symlink_entry`, `capped_reader_trips_on_cumulative_decompressed_bytes`).
- Path traversal in extraction — the destination is always `dest.join(bb_binary_name())`, a name built by the code, never `entry.path()`. The archive entry's internal path is used only to *match* the file name, never to construct a write path. No traversal possible regardless of what a malicious tarball puts in the entry header.
- `cache_layout.rs::version_bb_path` — takes `&AztecVersion`, which can only be constructed via `AztecVersion::parse` → `is_valid_version` (no `..`, no leading/trailing `.`, ASCII alphanumeric/`.`/`-`/`_` only, ≤128 chars). Traversal-safe by construction; `versions_to_evict`/`cleanup_old_versions` re-validate directory names read off disk through the same gate before any use.
- `install_version_dir` temp-dir-then-rename — verified in-memory bytes are the exact bytes extracted (no re-read-from-disk TOCTOU between digest check and extraction). A same-UID race between two concurrent `download_bb` calls for the same version can corrupt the shared `.{version}.tmp` sibling, but this doesn't cross a privilege boundary (only the same OS user can write under their own `~/.aztec-accelerator/`, and that user already controls everything else the app touches) — not a finding.
- macOS `xattr`/`codesign` — invoked via `std::process::Command` with `final_path` as a discrete `arg()`, not through a shell; no injection surface.

---

## Finding 1: Windows `bb.exe` checksum pin is computed trust-on-first-use and merged with zero required human review — the "review-gated pin" claim is false under the live repo configuration

**Impact factors**: Integrity (primary) + Confidentiality + Availability, blast radius = **all Windows users** of the released Accelerator app (the pinned binary is bundled into the shipped Tauri sidecar, `src-tauri/binaries/bb-x86_64-pc-windows-msvc.exe`), data sensitivity = highest (the `bb` subprocess processes the private ZK witness — the app's own documented "crown jewel"). Exploitability: attack vector = network/supply-chain (attacker must control the exact `aztec-packages` GitHub release asset matching the version tag being bumped to), attack complexity = high, privileges required = none on this repo, user interaction = required (a maintainer must `workflow_dispatch` the update workflow — routine, non-suspicious).

**Evidence confidence**: high — verified against the live GitHub ruleset API (`gh api repos/alejoamiras/aztec-accelerator/rulesets/...`), not just committed config.

**OWASP category + CWE**: A08:2021 – Software and Data Integrity Failures; CWE-345 (Insufficient Verification of Data Authenticity), compounded by CWE-829 (Inclusion of Functionality from Untrusted Control Sphere — the CI pipeline pulls and trust-anchors an artifact from an external release with no independent check).

**Trace** (source → sink):
1. `scripts/update-aztec-version.ts:79-98` (`pinWindowsBbChecksum`) — fetches `barretenberg-amd64-windows.tar.gz` from the `aztec-packages` GitHub release for the target version (`update-aztec-version.ts:83-85`), computes `crypto.subtle.digest("SHA-256", ...)` **over the bytes it just fetched** (`update-aztec-version.ts:87-88`), and splices the resulting hex digest straight into `packages/accelerator/scripts/copy-bb.ts`'s `WINDOWS_BB_CHECKSUMS` map (`update-aztec-version.ts:89-93`). This is textbook trust-on-first-use: nothing external to this one HTTPS fetch is consulted.
2. `.github/workflows/_aztec-update.yml:116-117` — the CI job runs `bun scripts/update-aztec-version.ts "$NEW_VERSION"` unattended (no human in the loop at this step).
3. `.github/workflows/_aztec-update.yml:122-141` — commits the new pin under the `github-actions[bot]` identity and pushes a branch, still unattended.
4. `.github/workflows/_aztec-update.yml:187-225` — opens the PR via a GitHub App token (`create-github-app-token`, `_aztec-update.yml:158-165`), still unattended.
5. `.github/workflows/_aztec-update.yml:235-239` — merges the PR: `if AUTO_MERGE=true → gh pr merge --auto ...; else → gh pr merge ...` (immediate squash, just without waiting via `--auto`). **Both branches merge the PR from within the same automated workflow — there is no branch that stops and waits for a human to look at the diff.**
6. `.github/workflows/aztec-nightlies.yml:30` and `.github/workflows/aztec-stable.yml:29` both pass `auto_merge: false` — i.e. every real invocation takes the "immediate merge" branch at step 5, not the "wait for CI" branch.
7. The gate that's supposed to catch this — branch protection — is confirmed **live** via `gh api repos/alejoamiras/aztec-accelerator/rulesets/14138676` (main) to have `"required_approving_review_count":0` (matches the committed `infra/rulesets/main-branch-protection.json:19`). `gh api .../rulesets/15028728` (the **live** "Nightlies branch protection" ruleset, target branch `nightlies`, used by `aztec-nightlies.yml:target_branch`) has **no `pull_request` rule at all** — only `required_status_checks` — and this ruleset isn't even represented in `infra/rulesets/` (only `main-branch-protection.json` is committed; the nightlies one is undocumented in IaC).
8. `.github/CODEOWNERS:1-9` claims (`:4`) "Required reviews are configured via branch protection on `main`" — untrue per step 7 — and in any case does not list `packages/accelerator/scripts/copy-bb.ts` or `scripts/update-aztec-version.ts` as owned paths (only `verified-sites.*` are), so even if `require_code_owner_review` were on, it wouldn't fire here (it's explicitly `false` anyway per the ruleset's `rules[0].parameters`).
9. Sink: `packages/accelerator/scripts/copy-bb.ts:100-152` (`fetchWindowsBb`) later checks a real download **against exactly the value pinned in step 1**, so the fail-closed check is real but only as trustworthy as its own root of trust — which steps 2-8 show is never independently reviewed before landing on a release-shipping branch.

**Missing control**: an actual, enforced code-review gate (required approving reviewer count > 0, or at minimum a CODEOWNERS-enforced review) on the branches that receive an automated CI-computed cryptographic trust anchor for a binary that ends up inside the shipped product; alternatively, an independent verification source for the Windows `bb.exe` hash (e.g. a second fetch from a different vantage point, or waiting for Aztec to publish a signed manifest) instead of hashing the artifact against itself.

**Exploit/violation scenario**:
1. Attacker compromises (or social-engineers a maintainer/CI credential into) the `aztec-packages` release pipeline, or otherwise gets a malicious `barretenberg-amd64-windows.tar.gz` published under a version tag the accelerator repo is about to adopt (e.g. the next nightly or `rc.N`).
2. A maintainer runs `aztec-nightlies.yml` (or `aztec-stable.yml`) via `workflow_dispatch` — routine, expected activity, not itself suspicious.
3. `_aztec-update.yml` auto-detects the new version, downloads the malicious tarball, hashes it, writes that hash into `copy-bb.ts`, commits, opens a PR, and — because `auto_merge: false` takes the "else" branch at `_aztec-update.yml:238` — immediately squash-merges it. No human ever sees the new SHA-256 value or is asked to approve it (main requires 0 approvals; nightlies requires none at all).
4. On the next `prebuild` (`packages/accelerator/package.json:8`, `bun scripts/copy-bb.ts`) on Windows CI/release runners, `fetchWindowsBb` re-downloads the same malicious asset and validates it against the very hash that was derived from it — it "passes" by construction — and copies it into `src-tauri/binaries/bb-x86_64-pc-windows-msvc.exe`, which ships in the next Windows release.
5. Every Windows user who installs/updates gets a malicious `bb` subprocess that receives their private ZK witness on every proving request.

**Preconditions**: attacker needs the capability to get a matching-version-tag malicious release asset onto `AztecProtocol/aztec-packages` (the same precondition already accepted for SEC-02); no credentials needed on the `aztec-accelerator` repo itself; a maintainer must trigger the (routine, intentional) update workflow.

**Why existing mitigations fail**: This is explicitly a *different* artifact and *different* claimed mitigation than the documented SEC-02 residual. SEC-02 is scoped to the **runtime** Rust downloader (`downloader.rs`/`release_metadata.rs`) checking a GitHub-API-published digest against a GitHub-hosted binary — a documented, accepted circular-trust gap. This finding is about the **build-time** Windows-sidecar pin, whose code comment (`copy-bb.ts:9-11`) claims a *different* safety property — "this in-repo, **review-gated** pin is the supply-chain integrity anchor" — i.e. it explicitly relies on human review, not on an independent digest source, to be trustworthy. The live GitHub configuration (steps 7-8 above) shows that claimed review gate does not exist: 0 required approvals on `main`, no PR-review rule at all on `nightlies`, and CODEOWNERS doesn't cover the files in question. The `fetch → hash → pin` step (`update-aztec-version.ts:87-88`) and the `fetch → compare` step (`copy-bb.ts:100-152`) are also the same network round-trip pattern (fetch the same URL twice, from the same untrusted origin) with no diversity of vantage point, so even if a human *did* look at the PR diff, they'd see a hash with nothing to independently corroborate it against.

**Instances** (files sharing this root cause):
- `scripts/update-aztec-version.ts:79-98` (TOFU hash computation)
- `packages/accelerator/scripts/copy-bb.ts:9-11, 56-70, 76-86` (the pin table + the doc comment asserting the false "review-gated" property)
- `.github/workflows/_aztec-update.yml:116-141, 235-239`
- `.github/workflows/aztec-nightlies.yml:30`, `.github/workflows/aztec-stable.yml:29`
- `infra/rulesets/main-branch-protection.json:19` + live ruleset 14138676 (main, 0 required approvals)
- Live ruleset 15028728 ("Nightlies branch protection", not committed anywhere in `infra/rulesets/`) — no PR-review rule at all on `nightlies`
- `.github/CODEOWNERS:4` (inaccurate claim, and non-coverage of the files in question)

---

## Finding 2: `download-bb.ts` installs a `bb` binary with zero integrity verification directly into the path `downloader.rs` trusts forever once present

**Impact factors**: Integrity + Confidentiality (the cached `bb` handles the private witness once executed), blast radius = the local developer/machine that ran the script (not remotely triggerable), data sensitivity = highest if this cache is ever shared with a real proving session. Exploitability: attack vector = local/supply-chain, attack complexity = high (needs either a compromised upstream release matching the exact requested version+platform asset, or pre-existing local write access to `~/.aztec-accelerator/versions/`), privileges required = none-to-low, user interaction = required (someone must run `bun run bb:download <version>` or `bun scripts/download-bb.ts <version>`).

**Evidence confidence**: high for the code trace; moderate for real-world exploitation likelihood (requires the compounding condition below).

**OWASP category + CWE**: A08:2021 – Software and Data Integrity Failures; CWE-494 (Download of Code Without Integrity Check) for `download-bb.ts` itself, compounded by CWE-345 (Insufficient Verification of Data Authenticity) for `downloader.rs`'s unconditional cache-trust.

**Trace**:
1. `scripts/download-bb.ts:65-119` (`downloadBb`) fetches `https://github.com/AztecProtocol/aztec-packages/releases/download/v{version}/barretenberg-{platform}.tar.gz` (`downloadUrl`, `:32-34`), pipes it straight into `tar -xzf -` (`:92-94`), `chmod 0o755`s it (`:106`), and on macOS clears quarantine + ad-hoc re-signs it (`:111-114`) — **at no point does the file compute, fetch, or compare any SHA-256/digest.** Confirmed by grep: zero occurrences of `sha`/`digest`/`checksum`/`hash`/`verify` anywhere in the file.
2. It writes the result to `versionBbPath(version)` = `join(versionsBaseDir(), version, "bb")` (`download-bb.ts:44-46`), where `versionsBaseDir()` = `$BB_VERSIONS_DIR` or `~/.aztec-accelerator/versions` (`:40-42`) — **the identical directory convention** the production Rust code uses (`cache_layout.rs:8-13`, `versions_base_dir()` = `~/.aztec-accelerator/versions`; `version_bb_path()` = `versions_base_dir().join(version).join(bb_binary_name())`, `cache_layout.rs:27-31`).
3. Sink: `packages/accelerator/core/src/versions/downloader.rs:22-27` — `download_bb`'s very first action is `if bb_path.exists() { ...; return Ok(bb_path); }`. **This is an unconditional short-circuit: nothing about the digest-verification pipeline (`verify_digest`, `downloader.rs:157-176`) ever runs for a path that already exists.** Once *any* bytes land at `~/.aztec-accelerator/versions/{version}/bb` — by any means — the production accelerator will use and execute them indefinitely, with no periodic or on-launch re-verification anywhere in this cluster's files.
4. Confirmed reachable as a real, invokable command (not dead code): `package.json:13-14` — `"bb:download": "bun scripts/download-bb.ts"`, `"bb:list": "bun scripts/download-bb.ts --list"`.
5. Confirmed **not** wired into any CI/release path: `grep -rln "download-bb" .github/` → no hits; `grep -rn "download-bb" packages/accelerator` → no hits. It is genuinely dev-only tooling, invoked manually.

**Missing control**: `download-bb.ts` has no SHA-256 comparison against the GitHub release asset's `digest` field (the same `api.github.com/.../releases/tags/v{version}` lookup `release_metadata.rs::fetch_github_asset_digest` already performs for the Rust path could be trivially mirrored here); separately, `downloader.rs::download_bb` has no mechanism to distinguish "this cache entry was digest-verified when written" from "this cache entry exists for any other reason" — there's no marker file, no stored digest, no re-check.

**Exploit/violation scenario**: A developer runs `bun run bb:download 5.0.0-rc.2` to pre-seed their local cache for testing (or does so from a fork/branch where `downloadUrl`'s hardcoded upstream org could plausibly be repointed at a compromised mirror in a supply-chain-compromised dependency chain of the script itself). If, at that moment, the targeted GitHub release asset for that exact version+platform has been tampered with upstream (the same precondition SEC-02 already accepts as a residual risk), the script downloads, extracts, chmods, and (on macOS) codesigns the malicious binary with **zero comparison against any independent value** and drops it at the exact path the production Tauri app (run in dev mode, or a production build pointed at `$HOME` on that same machine) reads via `version_bb_path`. The next time anything on that machine calls `download_bb` for that version, `bb_path.exists()` is `true` and the function returns immediately (`downloader.rs:24-26`) — the malicious binary is trusted and executed as the proving subprocess indefinitely, with no digest check ever performed on it.

**Preconditions**: a human must run the script (or something must invoke it) targeting a version whose upstream release asset is compromised at fetch time; the resulting cache directory must subsequently be read by a real accelerator instance (dev-mode Tauri app on the same machine, most realistically).

**Why existing mitigations fail**: The documented SEC-02 caveat is about the *runtime* Rust downloader's digest check being circular (same GitHub trust plane) — it does not apply here at all, because `download-bb.ts` has *no* digest check to be circular about; it's strictly weaker than SEC-02's already-accepted floor. The fail-closed guarantee documented for `downloader.rs::verify_digest` ("Fail-closed: a missing digest ... or a fetch error is an error, not a skip — we never install unverified code", `downloader.rs:153-155`) is true only for the code path that runs `verify_digest` — it says nothing about, and does not defend against, bytes that arrive at the same cache path via a different writer that never calls it. The cache-existence check at `downloader.rs:24` has no way to tell these two provenances apart.

**Instances**:
- `scripts/download-bb.ts:65-119` (source: unverified write)
- `scripts/download-bb.ts:44-46` (path convention shared with production)
- `packages/accelerator/core/src/versions/downloader.rs:22-27` (sink: unconditional cache-hit trust)
- `packages/accelerator/core/src/versions/cache_layout.rs:8-13, 27-31` (the shared path-construction convention that makes the two "the same file")
- `package.json:13-14` (confirms it's a real, documented entry point: `bun run bb:download`)

---

Not flagged: `packages/accelerator/scripts/bb-version.ts` — a one-line wrapper around `resolveAztecBb()` from `copy-bb.ts`, printing the resolved version for CI. No download, no path/URL construction, no user input. No security-relevant surface of its own.
