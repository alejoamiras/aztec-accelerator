# Competing outline — fold the window into `updater-state.json`

The alternative shape `/blueprint mid` requires: instead of a NEW `update-in-progress.json`, extend
the EXISTING `updater-state.json` schema with an optional `window` object:

```json
{ "schema": 2, "floor": "1.0.8", "pending": "1.0.9",
  "window": { "txn": "…", "expected_install_path": "…", "intent_at_disarm": true,
              "deadline_unix": 1785300000 } }
```

Token handoff (NSIS) is unchanged — this outline differs only in where the marker state lives.

## What it buys

- **One file, one loader, one atomic writer.** Reuses `write_state`/`load_state` verbatim; no new
  `update_marker.rs` IO layer; the schema bump rides the existing canonical-round-trip machinery.
- **No duplicated candidate version.** `pending` already IS the candidate; removal rule #1 reads it
  instead of a second copy. Kills the two-files-asserting-overlapping-facts smell outright.
- **One fewer file in `~/.aztec-accelerator`** to reason about, secure, and clean up.

## Why the main plan rejects it

1. **Blast radius.** `updater-state.json` is the CROSS-PLATFORM anti-rollback floor, loaded
   fail-closed (`Corrupt` ⇒ all updates blocked). Coupling a seconds-scale Windows install window
   into it means a torn/corrupt window write can poison the floor file — turning a marker glitch
   into "updates permanently blocked until manual deletion" on every OS. The marker's own corrupt
   exit (delete + one-launch suppression) is only safe BECAUSE the file is disposable; the floor
   file is precisely not.
2. **Schema migration on a security-critical file.** `deny_unknown_fields` + `schema: 1` means a
   `window` field requires a schema bump and forward/backward handling for a file that gates
   anti-downgrade. Piece 1's "zero install base" argument does NOT apply — 1.0.7 installs in the
   wild already write schema-1 files, and an old binary reading a schema-2 file goes `Corrupt` ⇒
   fail-closed ⇒ a downgrade-then-recover path bricks updates. The separate marker is invisible to
   old binaries.
3. **Divergent lifecycles under one lock discipline.** `pending` clears on healthy launch commit
   (floor tracker, minutes later, updater.lock); the window clears in the startup removal
   transaction (autostart.lock, seconds). One file written under two different lock regimes at two
   different cadences reintroduces exactly the cross-lock coupling the D19 split was built to avoid.
4. **`pending` outlives the window by design** (it keeps gating downgrades after the install
   completes), so removal rule #1 would need "pending at window-creation time" captured anyway —
   i.e. the window object still ends up storing its own version copy, erasing benefit #2.

Verdict: the buy is real but small (one loader), the risks land on the most safety-critical file
in the app. Separate file stands. This outline goes to both auditors alongside the main plan so
the choice is contested, not assumed.
