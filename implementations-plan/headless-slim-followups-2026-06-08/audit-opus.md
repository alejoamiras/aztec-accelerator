# Opus subagent audit — headless slim follow-ups

**VERDICT: A; holds-with-changes.** A (surgical inline edits) correct; the `src-tauri` patch is genuinely dead
(verified `server/Cargo.lock` has NO `aztec-accelerator` stanza); host-assert skip + prebuild drop safe. Fix the
inherited stale `--locked` rationale + the rc-coverage framing.

1. **A > B.** B (route `_e2e` through the composite) kills the 3-copy apt drift, BUT `_e2e` hand-rolls a different
   setup ordering (setup-aztec/start-services BEFORE the accelerator block, then prebuild, BB_BINARY_PATH/
   AZTEC_BB_VERSION via `$GITHUB_ENV`, separately-keyed rust-cache). Folding in = more surface on a release-adjacent
   reusable wf. A's one-line `:49` edit is trivially safe + self-proves. B = future de-drift follow-up.
2. **`src-tauri` patch DEAD — verified:** `server/Cargo.lock` has no `aztec-accelerator` stanza; server deps only
   `accelerator-core` (build.rs-free). `/health.version` + `--version` both read the SERVER crate version (patched,
   kept). Removing the src-tauri sed (release:277) changes neither. Comment (274-276 "via env! from the shared lib")
   is factually wrong post-Phase-2.
3. **Host-assert skip (run-prebuild:false) SAFE** — it only guards bb-sidecar selection; the matrix pairs
   target↔runner + the `--version` assert executes the binary. Safety = matrix discipline.
4. **Prebuild drop SAFE** — the tarball is just the binary; version stamp = patched `server/Cargo.toml` (env! @
   compile), not the prebuild's AZTEC_VERSION. build-headless never sets AZTEC_BB_VERSION → `/health.aztec_version`
   already `"unknown"` on the release binary today (no regression). bb never bundled.
5. **rc-validation:** the rc DOES exercise build-headless + update-smoke* (prerelease-eligible); the smokes bootstrap
   N from the rc's own artifacts → validate the slimmed build output. GAP (out of Part B scope): verify-live-feed/
   S3/bump-source are `is_prerelease=='false'` → the FEED isn't exercised. **NEW: the `--locked` rationale
   (release:287-290) is ALSO stale** — it cites the src-tauri patch; the real reason `--locked` stays off is the
   `server/Cargo.toml` patch staling the server lock's own `accelerator-server` stanza. Part B MUST fix this comment
   too (else a future reader re-adds `--locked` + breaks the release build).
6. Assumptions/Security accurate except the `--locked`/"shared lib" framing. Security holds (install less, no new
   secrets, Windows checksum untouched — build-headless is mac/linux only, no windows leg).
