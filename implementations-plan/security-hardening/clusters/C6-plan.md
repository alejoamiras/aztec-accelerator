# C6 / F-007 — bb-cache-integrity — plan (mid tier) — REVISED after dual audit

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

## Decision ledger (dual audit: codex REJECT + fable CONDITIONAL-APPROVE, both folded)
- **DEF-1 — defense-in-depth over runtime-marker-only.** Both audits reject marker-only (it makes
  `bb:download` a no-op that fails at first prove, and moves detection off the network-MITM point). KEEP
  verify-on-download in BOTH paths + the runtime marker. The ~30-line TS/Rust metadata-fetch duplication
  is worth it; share a contract fixture to prevent drift.
- **B1/C1 (Critical) — the re-download must be actually WIRED + no fail-open fallback.** My first draft
  asserted "legacy→re-download" with no implementing edit, and `find_bb`'s reject would fall through to
  sidecar/`~/.bb`/`$PATH`. FOLDED into Phases 2/3.
- **B2 (Critical) — marker is the FINAL binary digest (post macOS codesign).** Both paths sign/mutate the
  binary AFTER extract; hashing before codesign → every Mac cache hit mismatches. Marker = sha256 of the
  published binary after ALL mutations. This makes storing the binary digest (not just the tarball digest)
  NECESSARY. FOLDED (Phase 1/2) + an ordering test.
- **M3/M4 (Med) — `list_cached_versions` must NOT full-hash** (it feeds `/health` `server.rs:291` + the
  tray `tray.rs:57`, polled → GB of I/O). Listing checks marker EXISTENCE (cheap stat); the full re-hash
  lives ONLY in `find_bb`/`verify_cached_bb` (exec-adjacent). NO per-process exec cache (it widens the
  hash→exec TOCTOU window to the whole process lifetime). FOLDED (Phase 3).
- **M4/M6 (Med) — TS digest fetch mirrors `release_metadata.rs` EXACTLY** (accept header, exact asset
  match, strip `sha256:`, non-2xx/missing/malformed ⇒ HARD error not skip) + optional `GITHUB_TOKEN`
  (60/hr/IP unauth trips the bulk script + CI). Reuse `packages/accelerator/scripts/copy-bb.ts`'s
  `assertSha256` + capped-fetch. FOLDED (Phase 1/4).
- **C2 (High) — honor the master safe-publication invariant** (`plan.md` F-007): private staging + reject
  unsafe archive members (ONE regular `bb`, no symlinks/`..`, compressed+decompressed size caps) + atomic
  publish + structured marker (archive + binary digest). FOLDED (Phases 1/2).
- **C3/L6 (High) — real validation gate.** VERIFIED: root `bun run test` = `lint + test:typecheck +
  test:unit`, and `test:unit` runs sdk/playground/accelerator `test:unit` — accelerator's is
  `bun test scripts/` = `packages/accelerator/scripts/`. NONE of them run ROOT `scripts/`, where
  `download-bb.ts` (and its new `.test.ts`) live. So the fold is: add a root `test:scripts`
  (`bun test scripts/`) npm script, fold it into root `test:unit`, AND add a dedicated
  `accelerator.yml` step + extend the `desktop`/`integration` paths-filters with `scripts/download-bb.ts`
  (root `scripts/**` is otherwise not a trigger). FOLDED into Phase 1 + Phase 4.

## Design (folded)
**Both download paths** (`downloader.rs`, `download-bb.ts`) do, in order:
1. Fetch the tarball; verify `sha256(tarball)` == the GitHub release asset's published digest
   (mirror `release_metadata.rs::fetch_github_asset_digest`; non-2xx/missing/malformed ⇒ hard error).
2. Extract into a PRIVATE STAGING dir (`{version}.staging-<rand>`, 0700), accepting ONLY a single regular
   file named `bb` — reject symlinks, dirs, `..`/absolute paths, extra members, and enforce compressed +
   decompressed size caps (bb is tens of MB).
3. Finalize in staging: `chmod 0755`; on macOS `xattr -d com.apple.quarantine` + `codesign --force --sign -`.
4. Compute `sha256(the FINAL staged bb)` (post-codesign).
5. Write the marker `bb.sha256.json` in staging (0600): `{schema, archive_sha256, binary_sha256, version, platform}`.
6. Atomically PUBLISH: rename staging → the live `{version}/` dir (replacing any prior). Publish is
   all-or-nothing (a crash leaves a `.staging-*` dir, never a half-written live entry).

**Runtime** (`core`):
- `verify_cached_bb(version) -> Result<PathBuf, CacheIntegrityError>`: read the marker; re-hash the live
  `bb`; require the marker present + `binary_sha256` == the live hash; else `Err` (+ `SECURITY:` log).
- `bb.rs::find_bb`: the version-cache branch calls `verify_cached_bb`. A REQUESTED (non-bundled) version
  whose cache entry is present-but-invalid ⇒ hard `Err` — do NOT fall through to sidecar/`~/.bb`/`$PATH`
  (only the bundled/no-version case may use those).
- `server/prove.rs:80`: `needs_download = v != bundled && verify_cached_bb(v).is_err()` (re-download when
  unverified, not merely when absent).
- `downloader.rs::download_bb` (+ `download-bb.ts`) skip-if-exists ONLY when `verify_cached_bb` passes;
  otherwise proceed to the verified staged download (which atomically REPLACES the bad entry).
- `cache_layout.rs::list_cached_versions`: filter on marker EXISTENCE (cheap stat) only — NO re-hash.

## Phases

### Phase 1 — `scripts/download-bb.ts`: verify + stage + tar-safety + marker + atomic publish
- Reuse `packages/accelerator/scripts/copy-bb.ts`'s `assertSha256` + capped-fetch. Add `fetchAssetDigest`
  (mirror `release_metadata.rs`: GH releases API, exact asset name, `sha256:` strip, non-2xx/missing ⇒
  throw; optional `GITHUB_TOKEN`). Verify tarball digest; extract into a private staging dir accepting only
  a single regular `bb` (size caps, no symlinks/`..`); finalize+codesign; write the JSON marker with
  archive+final-binary digests; atomically rename staging→live. Skip only when an existing entry's marker
  validates.
- **Validation gate:** `bun test scripts/download-bb.test.ts` (new root fixture test: verify-mismatch
  throws + nothing published; unsafe archive member/symlink rejected; single-`bb` publish; marker written
  post-codesign; skip only on a valid marker; legacy/tampered → re-download — mock fetch + build fixture
  tarballs) + `bun run lint` (biome). Also add the root `test:scripts` script here. Layers: unit + lint.

### Phase 2 — core: marker helpers + both `downloader.rs` and the runtime re-verify
- `cache_layout.rs`: `version_bb_marker_path`, `write_bb_marker` (atomic 0600), `read_bb_marker`, and
  `verify_cached_bb(version)` (rehash live bb vs marker `binary_sha256`; fail-closed). `list_cached_versions`
  → marker-existence filter only.
- `downloader.rs`: stage privately, reject unsafe members, finalize/sign, write the marker (archive digest
  from `verify_digest` + final-binary digest), atomic publish. `download_bb` skip-if-exists gated on
  `verify_cached_bb`.
- **Validation gate:** `cargo test --manifest-path packages/accelerator/core/Cargo.toml` (marker
  round-trip; verify_cached_bb accepts valid / rejects missing+mismatch; marker computed post-finalize) +
  `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check`. Layers: unit + lint.

### Phase 3 — core: `find_bb` + `prove.rs` fail-closed, NO wrong-version fallback
- `bb.rs::find_bb`: version-cache branch uses `verify_cached_bb`; a present-but-invalid REQUESTED version ⇒
  hard `Err` (no sidecar/`~/.bb`/`$PATH` fallthrough for a requested non-bundled version). `prove.rs:80`
  `needs_download` via `verify_cached_bb`.
- **Validation gate:** `cargo test` (find_bb: valid marker → cached path; invalid/missing marker for a
  requested version → Err, NOT sidecar; prove resolve_version re-downloads on invalid; end-to-end:
  legacy/tampered → one verified re-download → requested path; download failure → no execution) + clippy +
  fmt. Layers: unit + integration.

### Phase 4 — CI wiring + docs
- Root `package.json`: add `"test:scripts": "bun test scripts/"` and fold it into `test:unit` (so root
  `bun run test` now covers root `scripts/`). `accelerator.yml`: add a `bun run test:scripts` step to the
  lint/test job and extend the `desktop` + `integration` paths-filters with `scripts/download-bb.ts`
  (root `scripts/**` is otherwise not an accelerator trigger). Confirm the pinned/bundled bb versions
  expose a GitHub asset digest (else they'd be un-downloadable — aligns with the already-fail-closed Rust
  path). Doc note (README/CLAUDE): `bb:download` verifies; legacy unmarked caches re-download on first use;
  offline unmarked caches fail closed until an online re-download.
- **Validation gate:** `bun run lint:actions` + full `bun run test` (now incl. `test:scripts`) + full
  `cargo test`. Layers: lint + unit.

## Security & Adversarial Considerations
- **Threat model:** F-007 = a MITM'd/compromised tarball at download + the runtime trusting the cached
  binary + a fail-open fallback to a wrong/unverified bb. Closed by verify-on-download (both paths) + the
  marker rehash-on-use + hard-Err no-fallback.
- **Residual — same-UID local-cache writer** can rewrite BOTH the binary and its marker (the marker is a
  digest record, NOT a signature) → the rehash still matches. EXPLICITLY EXCLUDED from F-007's network
  threat model (that attacker already has local write, a higher privilege). Storing GitHub's digest does
  NOT change this (the writer edits the marker too; archive vs binary are different objects). Closing it
  needs an authenticated publisher signature or a dir ACL — future work; documented.
- **Residual — hash→exec TOCTOU:** the rehash is at-rest integrity immediately before spawn, not
  exec-time; a writer racing the window is the same local-writer case. NO per-process exec cache (it would
  widen this window to the process lifetime).
- **Residual — GitHub API:** rate limits (mitigated by optional `GITHUB_TOKEN`) + a GitHub
  release/account/CI compromise can replace the asset AND its digest (TOFU — identical to the existing
  Rust path). Old releases lacking the `digest` field are un-downloadable (fail-closed).
- **Crypto:** SHA-256 via `sha2` (already a dep) + Bun `crypto`/`copy-bb.ts` — no rolled crypto. Ordinary
  platform TLS (NOT certificate pinning — corrected from the draft).
- **Fail-closed everywhere:** missing digest, unsafe archive member, missing/malformed marker, hash
  mismatch, fetch error ⇒ refuse (no fall-through for a requested non-bundled version).

## Assumptions
### Facts (verified)
- download-bb.ts unverified fetch+untar (`:65-119`); downloader.rs `verify_digest` fail-closed (`:157`);
  find_bb trusts exists() then falls through to sidecar/`~/.bb`/`$PATH` (`bb.rs:21-50`); prove.rs
  needs_download by exists() (`:80`); no marker exists; `sha256_hex` + `fetch_github_asset_digest`
  (`release_metadata.rs:83`) exist; `copy-bb.ts` has `assertSha256`; the master F-007 spec matches this
  design (`plan.md` F-007 line).
- `list_cached_versions` feeds `/health` (`server.rs:291`) + tray (`tray.rs:57`).
### Inferences (verify in impl)
- GitHub release-asset `digest` = `sha256:<hex>`, generated at upload, immutable (GitHub docs 2025-06) —
  mirror `release_metadata.rs` exactly. Old releases may lack it → verify the pinned versions have it (Phase 4).
- One-hash-per-prove (~100-300 ms, sha2 HW) is negligible vs multi-second proves → fresh hash, no cache.
### Asks (defaults chosen — flag to override)
- A1: defense-in-depth (verify-on-download BOTH paths + runtime marker) — chosen (both audits agree).
- A2: legacy/unmarked ⇒ fail-closed + verified re-download; requested non-bundled invalid ⇒ hard Err, NO
  wrong-version fallback — chosen.
- A3: marker stores BOTH archive digest AND final-binary digest; runtime checks the binary digest offline —
  chosen (master invariant; binary digest is necessary post-codesign).

## Seeds (draft)
- `/goal`: All C6 phases ✓ in plan.md, each backed by its gate (root `bun run test` + `cargo test`/clippy/
  fmt + lint:actions) reported in the transcript; post-impl codex xhigh audit folded; PR into
  security-hardening CI green.
- `/loop 15m`: drive C6 — verify-on-download + private staging + tar-safety + structured marker + atomic
  publish + fail-closed runtime rehash (no wrong-version fallback); after each edit run the touched
  package's test+lint (root `bun run test` for scripts/); commit/push; consult codex on any marker/tar
  detail.
