# Plan — flip bundle.publisher to "Aztec Accelerator" (no migration)

`/blueprint light`, micro-piece. The rename PR (#426) deferred `publisher` because it feeds NSIS
`${MANUFACTURER}`, whose registry namespace anchors custom-`$INSTDIR` restore — a hazard **for an
existing fleet**. Owner confirmed today (2026-07-29): the install base is dev/test only. With no
fleet to strand, the two-release migration is vacuous — flip now, BEFORE a user base exists.

## Facts (verified, this worktree @ d905e19)

1. `installer.nsi:58` (tauri-bundler 2.8.1): `UNINSTKEY = "Software\Microsoft\Windows\
   CurrentVersion\Uninstall\${PRODUCTNAME}"` — **manufacturer-FREE**. The `MainBinaryName` regkey
   (old-exe delete on the rename boundary) and all uninstall-entry state live there; a publisher
   change does not touch them.
2. The ONLY manufacturer-keyed state is `MANUPRODUCTKEY = "Software\${MANUFACTURER}\
   ${PRODUCTNAME}"` (`:59-60`): custom-`$INSTDIR` memory (`:658`, read `:331`) and the MUI
   language choice (`:140`). Loss = a custom-dir dev install upgrades into the DEFAULT dir
   (old files left behind) and language prompt reappears. Accepted for a dev-only fleet;
   default-dir installs (everyone in practice) resolve to the same directory regardless.
3. CI self-consistency: both Windows smokes install fresh per run (no prior registry), so
   manufacturer changes are invisible to them; the CALL path's v1.0.7 N−1 → renamed-N old-exe
   delete rides UNINSTKEY (fact 1) — unaffected.
4. `tauri-identity.test.ts:49-54` currently pins publisher ABSENT, with a comment requiring the
   migration story to delete it. This piece IS that story's resolution (fleet = none), so the pin
   flips to the exact value and the comment records the decision + date.
5. Publisher surfaces: Add/Remove Programs "Publisher" (today: identifier-fallback "aztec"),
   NSIS registry namespace (fact 2). No macOS/Linux behavior change (deb maintainer/dmg unaffected
   by tauri `publisher`; only Windows NSIS consumes it as MANUFACTURER).

## Design
- `tauri.conf.json`: `bundle.publisher: "Aztec Accelerator"`.
- `tauri-identity.test.ts`: pin `publisher === "Aztec Accelerator"`; rewrite the comment — the
  deferral resolved 2026-07-29 with a confirmed empty fleet, and any FUTURE manufacturer change
  (including reverting this) re-opens the custom-$INSTDIR hazard against a then-real fleet.
- Memory/docs: drop the "publisher deferred / migration open item" note from the queue memory and
  the /harden pointer; lessons entry.
- NOT in scope: any NSIS/registry migration code (vacuous — no fleet); other metadata (shipped in
  #426).

## Gates
1. Local: `bun run test` + identity test; `lint:actions` n/a (no workflow edits expected).
2. PR CI green (Windows Build Smoke installs fresh — self-consistent per fact 3).
3. Post-merge dispatch burn-in (barrier) — exercises install-over with the new manufacturer in
   one run (N−1 and N both carry it; plus the call path proves v1.0.7-cross-manufacturer on the
   next release, same leg that proves the rename boundary).
4. ONE codex audit of this plan (fresh session) → fold → implement → ONE post-impl pass only if
   the audit finds substance (micro-piece; the diff is ~10 lines).

## Asks
None. (Owner input already taken: fleet = dev/test only, "flip now" chosen.)

## Audit fold (codex approve-with-changes, session `019fafb3-905a-75c0-b4e7-a9c46561e003`)

Design unchanged ("flip now" sound; the app-side registry mirror is REJECTED even as insurance —
it runs after installation, so it cannot protect the boundary; real insurance needs a prior
release, unjustified with no supported fleet). Three factual corrections:

1. **Fact 2 understated the dev-install disruption.** Interactive reinstall over an
   old-manufacturer install reads the NEW (empty) `MANUPRODUCTKEY` → `_?=$4` is appended EMPTY
   (`installer.nsi:331-335`) → the old uninstaller runs from a `~nsu*.tmp` copy →
   `$EXEDIR != $INSTDIR` → the hooks.nsi guard correctly classifies it a REAL uninstall and
   deletes the local CA/certs. A dev doing an interactive reinstall across the flip loses HTTPS
   trust until re-enabled (silent in-app updates are unaffected — they never run the old
   uninstaller). ACCEPTED, now stated.
2. **Fact 5 was false**: publisher also becomes the Debian `Maintainer` (Cargo.toml has no
   authors; config.schema.json:2102) and would feed MSI Manufacturer (unused — NSIS-only).
   DMG/plists/AppImage unaffected. Both cosmetic changes are wanted anyway.
3. Wording: "no SUPPORTED fleet" (not "confirmed empty"); smokes cover fresh/silent
   default-directory installs — they do NOT cover interactive reinstall or stale
   old-manufacturer keys (that's the accepted hole above, not an invisible change).
