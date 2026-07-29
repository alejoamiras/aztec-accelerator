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
