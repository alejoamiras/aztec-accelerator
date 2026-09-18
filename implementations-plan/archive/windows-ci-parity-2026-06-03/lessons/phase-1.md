# Phase 1 — #95: Cargo.lock parity + bump-source fix

## What it was (corrected by audits)
My draft premise (src-tauri/Cargo.lock stale) was incomplete. Codex+opus caught the actually-stale
lock is server/Cargo.lock; final codex sharpened it: server/Cargo.lock has TWO stale local stanzas
(accelerator-server line 7 + the aztec-accelerator path-dep line 259, both 1.0.2-rc.1). AND the
COMMITTED src-tauri/Cargo.lock was ALSO stale at 1.0.2-rc.1 — the audits read the working tree,
which carried an uncommitted 1.0.4-rc.1 from an earlier local build. So both committed locks were
stale. Cosmetic (server + tauri builds use no --locked; the rc proved builds fine), but the lock
should record the truth; bump-source was the drift source (seds the 3 Cargo.tomls, never the locks).

## What shipped
1. Synced BOTH locks to 1.0.4-rc.1: server/Cargo.lock's two stanzas (perl -i; only the 2 local
   crates were at 1.0.2-rc.1, verified by count==2) + src-tauri/Cargo.lock's aztec-accelerator stanza
   (the working-tree change, version-line-only per `git diff --stat`).
2. Patched bump-source "Update source files" to bump every local-crate stanza in BOTH locks after
   the Cargo.toml seds: `sed "/^name = \"$crate\"$/{n;s/^version = .../}"` over
   {src-tauri,server}/Cargo.lock × {aztec-accelerator,accelerator-server}; a crate absent from a lock
   simply doesn't match. Added both locks to the "Create PR" git add.

## Validation
bump-source's lock-sed runs only during a real STABLE release's bump (NOT exercised by this PR's CI),
so validated LOCALLY with real GNU sed (gsed): seeded the local stanzas to 1.0.0-OLD, ran the exact
loop, confirmed all stanzas in both locks → the new version. actionlint clean.
