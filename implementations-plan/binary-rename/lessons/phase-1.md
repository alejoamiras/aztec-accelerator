# Lessons — binary-rename, phase 1

## The rename layer is a build-mode question, not a config question

The old pre-release-polish plan would have set conf `mainBinaryName` and updated all three
webdriver `APP_CMD` lines to the new name — shipping a breakage, because `_e2e-webdriver.yml`
builds dev/release with PLAIN `cargo build` (deliberate, F-012) and only tauri-driven builds
apply the conf rename. Renaming at the CARGO layer (`[[bin]] name` + `default-run`, crate name
untouched) makes every build path agree; conf `mainBinaryName` is then OMITTED (codex: tauri
applies it as an unconditional post-build file rename — same-name rename is at best redundant).
Verified empirically before pushing: `tauri build --debug --no-bundle` AND plain `cargo build`
both produce `target/debug/AztecAccelerator`; clippy --all-targets clean.

## Publisher is not "just metadata"

`bundle.publisher` feeds NSIS `${MANUFACTURER}`, which namespaces the registry key that restores
a custom `$INSTDIR` on upgrade. Changing it in the rename release would strand custom-directory
installs with a live old exe (the exact stale-exe loop the rename must avoid). Deferred, with a
pinning test whose comment says what deleting it requires.

## Upgrade-boundary certainty came from the lock history

"Every existing install deletes the old exe" rests on the MainBinaryName regkey existing in every
installed base — proven by checking `bun.lock` at EVERY release tag (all cli 2.10.1, whose
template has installer.nsi:666-674). The claim is about binaries users already have, so the
evidence had to come from git history, not HEAD.

## Post-impl audit (codex fresh session `019faf54-9c0d-7932-b596-6421c2449e85`): 1 blocking

Blocking: my updater-smoke.sh dual-name APP_BIN resolution sat at the ORIGINAL assignment
site — BEFORE the N−1 .app is installed — and under `set -euo pipefail` the find on a
missing .app kills a clean release runner instantly (or resolves a STALE leftover install).
Moved after ditto + quarantine-strip, with `|| true` on the find so the explicit FAIL line
reports it. PR CI cannot catch this class (the macOS release-updater gate doesn't run on
PRs) — exactly why the fresh-eyes diff audit exists.
Non-blocking adopted: identity test's "ONLY the fixture argument" claim is now a real
count assert (was toContain). Declined: schtasks XML <Command> assert (codex grades the
present→absent→present transition proof adequate; no more machinery).
