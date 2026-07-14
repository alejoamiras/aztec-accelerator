# C6 / F-007 — bb-cache-integrity — plan (mid tier) — REVISED r2 (after final-codex reject)

## Summary
`scripts/download-bb.ts` fetches + untars bb into the runtime-trusted version cache
(`~/.aztec-accelerator/versions/{version}/bb`) with **zero digest verification**, and the Rust runtime
(`bb.rs::find_bb`, `server/prove.rs`) trusts a cache hit purely by `exists()` — it never re-runs the
verification `downloader.rs::verify_digest` does at download time, and on a rejected entry it would fall
through to the sidecar/`~/.bb`/`$PATH` bb (a silent wrong-version/unverified execution). A compromised or
tampered cache entry thus becomes a trusted bb that executes over the witness (`--ivc_inputs_path`).

Fix = the master-plan F-007 spec: **both download paths verify the GitHub release digest, extract into a
private staging dir, reject unsafe archive members, then atomically publish `bb` + a structured marker
(archive digest + final-binary digest); the runtime rehashes the cached `bb` against the marker on every
use; a missing/malformed/mismatched marker ⇒ fail closed + verified re-download, with NO wrong-version
fallback.**

## Decision ledger (audits folded)
Dual audit (Fable conditional-approve + Codex reject) folded in r1. Final fresh-context Codex pass on r1
returned **reject** with operational gaps; ALL folded here in r2:

- **R2-1 (bundled/requested control flow).** `find_bb(Option<&AztecVersion>)` cannot tell a bundled `Some(v)`
  from a non-bundled one, so "present-invalid ⇒ hard-Err" would either break explicit bundled requests or
  keep the hole. FOLD: `resolve_version` **normalizes a bundled request to `version: None`** (bundled ships
  as the sidecar, never the cache) — so `find_bb` only ever sees a genuinely non-bundled `Some(v)`, and the
  "present-invalid ⇒ hard-Err / absent ⇒ fall through" rule is unambiguous. (Design + Phase 2/3.)
- **R2-2 (`BB_BINARY_PATH`).** Priority-0 override, accepted by `exists()` only. It is env-controlled ⇒ set
  by the same principal that owns the process — a deliberate dev/CI/operator escape hatch. FOLD: keep it,
  but **explicitly scope it as a trusted, out-of-F-007-threat-model override** (Assumptions Ask A4) + a test
  pinning that it is honored. NOT verified against a marker (it points at an arbitrary unversioned bb by
  design). (Design + Assumptions.)
- **R2-3 (replacement is not portably atomic).** Renaming staging over a **non-empty** live dir is not one
  atomic op; the code must `remove(live)` then `rename`. FOLD: SPECIFY the publish as **fail-closed
  delete-then-rename**, NOT "atomic replacement": initial publish is atomic; a crash mid-replace leaves NO
  live entry (⇒ verified re-download next use) plus a `.tmp` staging dir (⇒ reaped). Documented as
  fail-closed, not atomic. (Design + Phase 2.)
- **R2-4 (staging namespace pollutes the inventory).** A marker-existence scan would surface active/stale
  staging dirs via `/health`, tray, retention. FOLD: staging = the existing **`.{version}.tmp`** dot-prefixed
  sibling (`install_version_dir` already uses it); `list_cached_versions` **skips dot-prefixed names AND
  requires the name parse as a version AND the marker exist**; stale `.tmp` reaped at install (existing) +
  a note. (Design + Phase 2.)
- **R2-5 (TS is unsafe in four ways).** (a) `arrayBuffer()` before the length cap ⇒ **use a bounded
  streaming reader** (`response.body.getReader()` + running byte cap, mirror Rust `download_tarball`); (b)
  codesign status ignored ⇒ **codesign failure aborts without publish**; (c) raw CLI version string in cache
  paths ⇒ **add `assertValidVersion` mirroring `is_valid_version`** (non-empty, ≤128, no leading/trailing
  dot, no `..`, `[A-Za-z0-9._-]`) BEFORE any path use; (d) `bb.exe` on Windows + TS platform maps Windows→
  linux ⇒ **download-bb.ts is explicitly Unix-scoped** (Windows bb ships via `copy-bb.ts` sidecar) — reject
  `process.platform === "win32"` with a clear error. (Phase 1.)
- **R2-6 (validation gate still not real).** Root `bun run test` → `test:unit` → accelerator `bun test
  scripts/` runs `packages/accelerator/scripts/`, NEVER root `scripts/`. FOLD: add root `"test:scripts":
  "bun test scripts/"`, fold into root `test:unit`, AND an explicit `accelerator.yml` step + extend the
  `desktop`/`integration` paths-filters with `scripts/download-bb.ts`. Every cargo gate keeps
  `--manifest-path packages/accelerator/core/Cargo.toml` (there is NO root `Cargo.toml`). (Phases 1/2/3/4.)
- **R2-7 (marker semantics underspecified).** FOLD: a **schema constant** (`aztec-accelerator/bb-cache-marker@1`);
  `verify_cached_bb` **rejects an unknown schema and binds `version` + `platform`** (not just `binary_sha256`).
  Store archive + binary digests. (Design + Phase 2.)
- **R2-8 (self-inflicted eviction race).** `cleanup_old_versions` is spawned (detached) right after download
  (`prove.rs:269`) and can evict the just-downloaded version before `bb::prove` runs it — with the new
  hard-Err this becomes a visible spurious failure. FOLD: **exempt the in-use requested version from
  eviction** (pass it as protected to `versions_to_evict`/`cleanup_old_versions`). (Phase 3.)
- **R2-9 (shared cross-language fixture named but never created).** FOLD: Phase 1 creates a committed
  `scripts/__fixtures__/bb-cache-marker.json` + a release-metadata JSON that BOTH TS and Rust tests load
  (contract pin against drift).
- **R2-10 (false inferences).** "one hash per prove" is wrong (resolve + find_bb = **≤2 hashes**); GitHub
  `digest` is generated-at-upload but **not immutable unless the release uses the immutable-releases
  feature**. Corrected in Assumptions.

## Design (folded r2)
**Both download paths** (`downloader.rs`, `download-bb.ts`), in order:
1. Fetch the tarball with a **bounded streaming reader** (running byte cap; Rust already does — TS switches
   off `arrayBuffer()`); verify `sha256(tarball)` == the GitHub release asset's published digest (mirror
   `release_metadata.rs::fetch_github_asset_digest`; non-2xx/missing/malformed ⇒ hard error). TS validates
   the version string first (`assertValidVersion`).
2. Extract into a PRIVATE STAGING dir (`.{version}.tmp`, 0700), accepting ONLY a single regular file named
   `bb_binary_name()` (`bb`/`bb.exe`) — reject symlinks/hardlinks, dirs, `..`/absolute paths, extra members,
   duplicate `bb`, wrong-platform name, and enforce compressed + decompressed size caps.
3. Finalize in staging: `chmod 0755`; on macOS `xattr -cr` + `codesign --force --sign -` — **codesign
   failure aborts, nothing is published**.
4. Compute `sha256(the FINAL staged bb)` (post-codesign).
5. Write the marker `bb.sha256.json` in staging (0600):
   `{schema:"aztec-accelerator/bb-cache-marker@1", version, platform, archive_sha256, binary_sha256}`.
6. PUBLISH fail-closed: if a live dir exists, `remove` it, then `rename(staging → live)`. Initial publish is
   atomic; replacement is delete-then-rename (a crash ⇒ no live entry ⇒ verified re-download next use; a
   leftover `.tmp` is reaped). NOT claimed as atomic replacement.

**Runtime** (`core`):
- `verify_cached_bb(version) -> Result<PathBuf, CacheIntegrityError>`: read the marker; reject an unknown
  `schema`; require `version`+`platform` match; re-hash the live `bb`; require `binary_sha256` == the live
  hash; else `Err` (+ `SECURITY:` log). One streamed hash (no whole-file load).
- `resolve_version`: **bundled request ⇒ `version: None`**; a non-bundled request ⇒ `needs_download =
  verify_cached_bb(v).is_err()` (re-download on absent OR present-but-invalid).
- `bb.rs::find_bb`: `BB_BINARY_PATH` stays priority-0 (scoped trusted override, A4). The version-cache
  branch: if the entry is PRESENT, `verify_cached_bb` → Ok returns it, Err ⇒ **hard `Err`** (no
  sidecar/`~/.bb`/`$PATH` fallthrough); if ABSENT, fall through (this is the bundled/`None` path → sidecar).
- `downloader.rs::download_bb` (+ `download-bb.ts`) skip-if-exists ONLY when `verify_cached_bb` passes;
  else proceed to the verified staged download (fail-closed replace).
- `cache_layout.rs::list_cached_versions`: skip dot-prefixed names; require the name parse as a version AND
  the marker EXIST (cheap stat) — NO re-hash. Feeds `/health` + tray.
- `cleanup_old_versions`: **never evict the in-use requested version** (protected arg).

## Phases

### Phase 1 — `scripts/download-bb.ts`: verify + stage + tar-safety + marker + atomic publish (+ shared fixture)
- Reject Windows (Unix-scoped tool). Add `assertValidVersion` (mirror `is_valid_version`). Reuse
  `copy-bb.ts`'s `assertSha256`; add `fetchAssetDigest` (mirror `release_metadata.rs`: exact asset name,
  `sha256:` strip, non-2xx/missing ⇒ throw; optional `GITHUB_TOKEN`). Bounded streaming download (no
  `arrayBuffer()`); verify tarball digest; extract into `.{version}.tmp` accepting only a single regular
  `bb` (size caps, no symlink/`..`); chmod; codesign (abort on failure); write the JSON marker
  (archive+final-binary digests, schema/version/platform); fail-closed publish. Skip only when the existing
  marker validates. Migrate `listCachedVersions` to require the marker. Create
  `scripts/__fixtures__/bb-cache-marker.json` (+ a release-metadata JSON) for the cross-language contract.
- **Validation gate:** `bun test scripts/download-bb.test.ts` (root fixture test — cases below) +
  `bun run lint`. Add root `"test:scripts": "bun test scripts/"`. Layers: unit + lint.
- **Required negative cases:** digest mismatch ⇒ throw + nothing published; non-2xx / missing-digest /
  malformed-digest / wrong-asset metadata ⇒ throw; absent/lying Content-Length over the streamed cap ⇒
  abort; symlink `bb` / `..` member / extra member / duplicate `bb` ⇒ reject; codesign-fail ⇒ no publish
  (mock); invalid version string ⇒ reject before any fs op; skip only on a valid marker; legacy/tampered
  entry ⇒ re-download; marker parses against the shared fixture.

### Phase 2 — core: marker helpers + `downloader.rs` reorder + inventory
- `cache_layout.rs`: `version_bb_marker_path`, `write_bb_marker` (atomic 0600), `read_bb_marker` (reject
  unknown schema), `verify_cached_bb` (bind version+platform, streamed rehash vs `binary_sha256`; fail-closed).
  `list_cached_versions` → skip dot-prefixed + name-parses-as-version + marker-exists.
- `downloader.rs`: `verify_digest` returns the verified hex. Reorder `download_bb`/`install_version_dir` so
  extract→chmod→finalize/codesign→marker all happen IN `.{version}.tmp`, THEN fail-closed publish (marker is
  post-codesign, pre-publish). `download_bb` skip gated on `verify_cached_bb`.
- **Validation gate:** `cargo test --manifest-path packages/accelerator/core/Cargo.toml` (marker
  round-trip; unknown-schema/version-mismatch/platform-mismatch rejected; verify accepts valid, rejects
  missing+mismatch; marker computed post-finalize; publish replaces a stale entry and leaves no `.tmp`;
  list skips `.tmp`/unmarked; loads the shared fixture) + `cargo clippy --manifest-path
  packages/accelerator/core/Cargo.toml --all-targets -- -D warnings` + `cargo fmt --manifest-path
  packages/accelerator/core/Cargo.toml --check`. Layers: unit + lint.

### Phase 3 — core: `find_bb`/`prove.rs` fail-closed (no wrong-version fallback) + eviction exemption
- `resolve_version`: bundled ⇒ `version: None`; else `needs_download = verify_cached_bb(v).is_err()`.
- `bb.rs::find_bb`: present-but-invalid non-bundled version ⇒ hard `Err` (no fallthrough); absent ⇒ fall
  through. `BB_BINARY_PATH` stays (A4).
- `cleanup_old_versions`/`versions_to_evict`: exempt the in-use requested version; wire the protected arg
  through the `prove.rs:269` spawn.
- **Validation gate:** `cargo test --manifest-path packages/accelerator/core/Cargo.toml` (find_bb: valid
  marker→cached; invalid/missing marker for a present requested version→Err NOT sidecar; absent→fall
  through; bundled request→None→sidecar; BB_BINARY_PATH honored; resolve re-downloads on invalid; the
  just-downloaded requested version is NOT evicted; end-to-end legacy/tampered → one verified re-download →
  requested path; download failure → no execution) + clippy + fmt (same manifest-path). Layers: unit + integration.

### Phase 4 — CI wiring + docs
- Root `package.json`: fold `test:scripts` into `test:unit`. `accelerator.yml`: add a `bun run test:scripts`
  step + extend `desktop`/`integration` paths-filters with `scripts/download-bb.ts`. Confirm the
  pinned/bundled bb versions expose a GitHub asset digest (else un-downloadable — aligns with the
  fail-closed Rust path). Doc note (README/CLAUDE): `bb:download` verifies; legacy unmarked caches
  re-download on first use; offline unmarked caches fail closed until an online re-download;
  `BB_BINARY_PATH` is a trusted override.
- **Validation gate:** `bun run lint:actions` + full `bun run test` (now incl. `test:scripts`) + full
  `cargo test --manifest-path packages/accelerator/core/Cargo.toml`. Layers: lint + unit.

## Security & Adversarial Considerations
- **Threat model:** F-007 = a MITM'd/compromised tarball at download + the runtime trusting the cached
  binary + a fail-open fallback to a wrong/unverified bb. Closed by verify-on-download (both paths, digest +
  tar-safety) + the marker rehash-on-use + hard-Err no-fallback + bundled→None normalization.
- **Residual — same-UID local writer** can rewrite BOTH the binary and its marker (the marker is a digest
  record, NOT a signature) → the rehash still matches. This attacker is the **SAME OS principal**, not a
  higher-privilege one; a directory ACL does NOT stop a same-UID process, so ACLs are NOT a fix — only a
  distinct owner/service principal or an authenticated upstream publisher signature would close it. EXCLUDED
  from F-007's network threat model; documented as future work.
- **Residual — `BB_BINARY_PATH`** is a trusted env-controlled override (A4): whoever sets it already owns
  the process environment. Out of scope; documented + tested-as-honored.
- **Residual — retention-vs-exec TOCTOU** is not only a hostile-writer case: the app's OWN
  `cleanup_old_versions` races `bb::prove`. The Phase-3 in-use exemption removes the self-inflicted race;
  the residual hostile-writer window (marker-hash → exec) remains at-rest integrity, not exec-time. NO
  per-process exec cache (it would widen the window to process lifetime).
- **Residual — GitHub API/supply chain:** rate limits (optional `GITHUB_TOKEN`); the `digest` is
  generated-at-upload but **mutable unless the release uses immutable-releases**, and a GitHub
  release/account/CI compromise can replace asset AND digest (TOFU — identical to the existing Rust path).
  Old releases lacking a digest are un-downloadable (fail-closed).
- **Crypto:** SHA-256 via `sha2` (dep) + Bun `crypto`/`copy-bb.ts` — no rolled crypto. Ordinary platform TLS
  (NOT certificate pinning).
- **Fail-closed everywhere:** missing digest, unsafe archive member, codesign failure, missing/malformed/
  unknown-schema/mismatched marker, hash mismatch, fetch error ⇒ refuse; no fall-through for a requested
  non-bundled version.

## Assumptions
### Facts (verified)
- download-bb.ts unverified `arrayBuffer`+untar (`:87-114`), raw version in path (`:44,203`), Windows→linux
  platform map (`:26-29`); downloader.rs `verify_digest` fail-closed (`:157`), `install_version_dir` `.tmp`
  staging + delete-then-rename (`:188-213`), symlink/bomb/declared-size rejection (`:250-301`),
  codesign-on-publish (`:55`); find_bb trusts exists() then falls through (`bb.rs:21-67`); resolve_version
  needs_download by exists() + keeps bundled as `Some` (`prove.rs:80,85`); cleanup spawned before prove
  (`prove.rs:256-299`), evicts by retention (`version_policy.rs:203-217`); `is_valid_version` = non-empty,
  ≤128, no leading/trailing dot, no `..`, `[A-Za-z0-9._-]` (`version_policy.rs:187`); `fetch_github_asset_digest`
  fail-closed shape (`release_metadata.rs:83`); `bb_binary_name()` = `bb`/`bb.exe` (`cache_layout.rs:16`);
  `copy-bb.ts::assertSha256` (`:88`); list feeds `/health` (`server.rs:305`) + startup (`main.rs:562`);
  root `bun run test`→`test:unit`→accelerator `bun test scripts/` does NOT cover root `scripts/`
  (`package.json:24-28`, `packages/accelerator/package.json:12`); no root `Cargo.toml`.
### Inferences (verify in impl)
- Bundled version is never present in the versions cache in normal operation (ships as sidecar) ⇒
  normalizing bundled→None is behavior-preserving. Verify no path relies on `resolve_version` returning
  `Some(bundled)`.
- GitHub release-asset `digest` = `sha256:<hex>`, generated at upload; NOT immutable unless the release uses
  immutable-releases — mirror `release_metadata.rs` fail-closed. Old releases may lack it (Phase 4 check).
- ≤2 streamed hashes per non-bundled prove (resolve + find_bb), each ~100-300 ms (sha2 HW) — negligible vs
  multi-second proves; NO per-process cache. Threading a single verified path is a deferred micro-opt.
### Asks (defaults chosen — flag to override)
- A1: defense-in-depth (verify-on-download BOTH paths + runtime marker) — chosen.
- A2: legacy/unmarked ⇒ fail-closed + verified re-download; requested non-bundled invalid ⇒ hard Err, NO
  wrong-version fallback — chosen.
- A3: marker stores BOTH archive + final-binary digest + schema/version/platform; verify binds all — chosen.
- A4: `BB_BINARY_PATH` = trusted, unversioned dev/CI/operator override, out of F-007 scope — chosen.
- A5: `download-bb.ts` is Unix-only (Windows bb via `copy-bb.ts` sidecar) — chosen.
- A6: publish is fail-closed delete-then-rename (not atomic replacement); crash ⇒ re-download — chosen.

## Seeds (draft)
- `/goal`: All C6 phases ✓ in plan.md, each backed by its gate (`bun test scripts/download-bb.test.ts` +
  `cargo test --manifest-path packages/accelerator/core/Cargo.toml`/clippy/fmt + `test:scripts` in CI +
  `lint:actions`) reported in the transcript; post-impl codex xhigh audit folded; PR into
  security-hardening CI green; ledger DONE.
- `/loop 15m`: drive C6 — verify-on-download (bounded stream + digest + tar-safety) + private `.tmp` staging
  + structured marker (schema/version/platform + archive+binary digests, post-codesign) + fail-closed
  publish + runtime rehash (bundled→None; hard-Err no fallback; eviction exemption). After each edit run the
  touched package's test+lint (`bun test scripts/download-bb.test.ts` for the TS path). Commit/push. Consult
  codex on any marker/tar/publish-ordering detail.
