# Phase 3 — auto-update wiring (researched while blocked on the P2 push)

**Goal:** add `windows-x86_64` to the auto-update feed (`latest.json`) so Windows users get
minisign-verified N-1→N updates, on par with mac/linux. P3 (feed wiring) and P5 (release
build-matrix integration) both live in `release-accelerator.yml` and are best done together.

## release-accelerator.yml map (verified)
- **Build matrix** (~line 90): 3 Tauri targets — `aarch64-apple-darwin`, `x86_64-apple-darwin`,
  `x86_64-unknown-linux-gnu`. **No Windows.** → add `x86_64-pc-windows-msvc` on `windows-latest`.
- **Build step**: mac does `--bundles app,dmg` with a structure assert; linux a single
  `tauri build`. → Windows arm: `tauri build --bundles nsis` (real `TAURI_SIGNING_PRIVATE_KEY`
  from secrets, NOT the smoke ephemeral key) → `*-setup.exe` + `*-setup.nsis.zip` + `.nsis.zip.sig`.
- **Artifact upload** (~209-216): dmg/deb/AppImage/macos *.tar.gz/*.sig. → add
  `bundle/nsis/*-setup.exe` + `bundle/nsis/*-setup.nsis.zip` + `bundle/nsis/*.sig`.
- **Rename + collect** (~558): renames assets to friendly names. → add a Windows friendly name,
  e.g. `Aztec-Accelerator-${VERSION}-Windows-x86_64-setup.exe` (first-install) AND the
  `.nsis.zip` (updater).
- **latest.json gen** (~578-618): reads `*.app.tar.gz.sig` / `*.AppImage.sig`, asserts all
  present (~592), `jq` builds `platforms: {darwin-aarch64, darwin-x86_64, linux-x86_64}`.
  → add `WINDOWS_X86_64_SIG=$(cat the *-setup.nsis.zip.sig)`, add it to the sig-present assert,
  add `"windows-x86_64": { signature: $win_sig, url: $win_url }` where **url → the `.nsis.zip`**
  (the Tauri v1Compatible Windows updater artifact), NOT the `-setup.exe`.
- **verify-live-feed** (~767): `has("darwin-aarch64") and ... and has("linux-x86_64")` →
  add `and has("windows-x86_64")`.
- **Release-notes table** (~667): add a Windows download row.

## Key facts / decisions
- **Updater URL = `.nsis.zip`**, not `-setup.exe`. createUpdaterArtifacts `v1Compatible` makes
  the Windows updater download the `.nsis.zip`, extract the installer, run it silently
  (installMode currentUser ⇒ no UAC). First-install users download `-setup.exe` from the GH release.
- **Signing = the real minisign key** (release secret) ⇒ the `.nsis.zip.sig` verifies against
  the embedded pubkey `B371381E…` in tauri.conf ⇒ Windows auto-update is integrity-gated exactly
  like mac/linux. (The P2 build-smoke used a throwaway key — that's smoke-only.)
- **Headless server matrix** (~219, the 4 `accelerator-server` targets) — leave Windows OUT for
  v1 (the headless server is for CI consumers; not in scope per the locked decisions).
- **Stable-only steps** (S3 upload of latest.json, CloudFront invalidation, `--latest`,
  bump-source) are gated on `is_prerelease=='false'` — the P6 rc dry-run skips them, so the rc
  proves the build+feed-gen without touching prod. Good.

## Validation path
- P4 adds the Windows updater-smoke (a synthesized feed, local CA, hosts impersonation) that
  proves N-1→N click-free auto-update on windows-latest — the make-or-break, independent of this.
- P6 rc dry-run exercises THIS release-accelerator.yml path end-to-end (builds the Windows
  installer + emits a latest.json with the windows-x86_64 key) without prod upload.

## Status — IMPLEMENTED (release-accelerator.yml)
All 9 edits applied + actionlint-clean: matrix `windows-x86_64`, the build step's Windows
NSIS branch (`shell: bash` added so the bash build script runs under Git Bash on Windows),
the nsis artifact upload, the Windows rename + EXPECTED assert, the `latest.json`
windows-x86_64 key (url=`.nsis.zip`, sig=`.nsis.zip.sig`) + the all-sigs assert, the
create-release file collection, the verify-live-feed key check, and a release-notes row +
SmartScreen note.

**Validation caveat:** release-accelerator.yml is `workflow_dispatch`-only, so a PR runs only
actionlint (the `changes` filter doesn't list it → the accelerator gate skips). End-to-end
proof = the P6 rc dry-run (builds the Windows installer + the rc path) — but `latest.json`
gen is `is_prerelease=='false'` so the rc does NOT emit it; the windows-x86_64 KEY is only
exercised in a stable cut. The **auto-update** proof is independent: P4's Windows
updater-smoke (synthesized feed). So P3 = "a release WOULD include Windows + a windows feed
key"; P4 proves the update mechanism; the prod feed key lands with the first stable cut.

The release pipeline already has updater-smoke jobs (darwin + linux, lines ~445-492) gated
into `tag.needs` — **P5 integrates a Windows updater-smoke there** (after P4 builds it).
