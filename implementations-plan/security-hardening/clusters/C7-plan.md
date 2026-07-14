# C7 / F-008 — bb-windows-provenance — plan (mid tier)

## Summary
The Windows `bb.exe` supply-chain anchor is **self-referential**. `update-aztec-version.ts::pinWindowsBbChecksum`
(:80) downloads `barretenberg-amd64-windows.tar.gz` from the GitHub release, computes its SHA-256, and writes
that hash into `copy-bb.ts::WINDOWS_BB_CHECKSUMS` as the "pin" — labeled `(auto-pinned)`. This provides ZERO
independent verification: the pin is just "whatever bytes arrived." An attacker who compromises the upstream
release (or MITMs the fetch) has their malicious hash auto-pinned. Worse, `_aztec-update.yml` (:239) on
`auto_merge: false` runs `gh pr merge --squash --delete-branch` — an **immediate merge**, not "leave the PR
open" — so a bump can auto-accept a poisoned Windows sidecar with no human review.

The pin is supposed to be the supply-chain integrity ANCHOR (the in-repo, review-gated hash the Windows
Prebuild/Build Smoke gates re-verify). Auto-pinning + immediate-merge defeats that anchor entirely.

Fix (master-plan F-008): **remove auto-pinning** (a twice-downloaded asset is not independent evidence);
require a pin only from independently-verifiable evidence + human review; leave the bump PR **open** so a
human adds/reviews the pin; and **fix the `auto_merge: false` immediate-merge** to actually leave the PR open.

## Facts (verified)
- `pinWindowsBbChecksum` fetches the release asset, `crypto.subtle.digest` SHA-256, and auto-inserts the
  hash into `copy-bb.ts` before `resolveWindowsBbChecksum` (`update-aztec-version.ts:79-98`); `main()` calls
  it unconditionally on every bump (`:137`).
- `copy-bb.ts::resolveWindowsBbChecksum` THROWS if no pin exists for a version (`:76-86`) — the Windows
  Prebuild/Build Smoke gates re-fetch + re-verify against the pin (fail-closed at CI). `WINDOWS_BB_CHECKSUMS`
  mixes human-verified entries ("verified on windows-latest") with `(auto-pinned)` ones.
- `_aztec-update.yml`: `auto_merge: true` → `gh pr merge --auto --squash --delete-branch` (waits for CI);
  `auto_merge: false` → `gh pr merge --squash --delete-branch` (**immediate merge**) (`:220-239`). Callers:
  `aztec-stable.yml`, `aztec-nightlies.yml` (both `workflow_dispatch` only, no auto-merge per CLAUDE.md).
- The macOS/Linux bb comes from the `@aztec/bb.js` npm package (`copy-bb.ts:184-193`); only Windows fetches
  a GitHub release asset + pins a hash. So F-008 is Windows-specific.

## Inferences (verify in impl)
- Removing the auto-write means a NEW version with no pin fails the Windows CI gate (`resolveWindowsBbChecksum`
  throws) until a human adds an independently-verified pin — the desired fail-closed behavior.
- The current LIVE pin (the `@aztec/bb.js` version the lockfile resolves — 5.0.0-rc.2) must be revalidated as
  independently-verified, not auto-pinned residue.

## Asks (defaults chosen — flag to override)
- A1: replace auto-pin with a fail-closed **advisory** (print "MANUAL PIN REQUIRED" + independent-verification
  steps; never write the hash). The Windows CI gate enforces the block. — chosen.
- A2: `auto_merge: false` ⇒ create the PR and STOP (leave open). `auto_merge: true` keeps `--auto --squash`
  (CI-gated) — but since a missing pin fails the Windows gate, even `--auto` can't merge an unpinned bump. —
  chosen.
- A3: record the provenance of each live `WINDOWS_BB_CHECKSUMS` entry (independent evidence vs legacy
  auto-pin) in a comment; flag any that lack independent evidence. — chosen.

## Design (draft)
1. **`update-aztec-version.ts`**: delete `pinWindowsBbChecksum`'s download+write. Replace with
   `reportWindowsBbPinStatus(version)` — reads `copy-bb.ts`, and:
   - if a pin already exists for `version` → "✓ Windows bb.exe pin present (verify its provenance)";
   - else → a loud "⚠️ MANUAL PIN REQUIRED" block: the exact steps to obtain INDEPENDENT evidence
     (upstream signed checksum / attestation / reproducible build) and add the entry, and a note that the
     Windows Prebuild/Build Smoke CI gate will stay red until then. NEVER fetches or writes the hash.
2. **`_aztec-update.yml`**: on `auto_merge: false`, do NOT run `gh pr merge` — leave the PR open (echo the
   URL). Keep `auto_merge: true` → `--auto --squash` (opt-in, CI-gated). Update the input `description`.
3. **`copy-bb.ts`**: annotate each `WINDOWS_BB_CHECKSUMS` entry with its provenance; the current live pin
   revalidated + labeled independently-verified (or flagged). Keep `resolveWindowsBbChecksum` fail-closed.

## Phases

### Phase 1 — remove auto-pinning (`update-aztec-version.ts`) + tests
- Delete the fetch+write; add the fail-closed advisory reporter. Inline tests in `update-aztec-version.test.ts`:
  advisory when pin missing (no fetch, no file write), "present" when pin exists, no network in either path.
- **Validation gate:** `bun test scripts/update-aztec-version.test.ts` + `bun run lint`. Layers: unit + lint.

### Phase 2 — `_aztec-update.yml` leave-PR-open + `copy-bb.ts` provenance
- Fix the `false`-branch immediate-merge; update the input description. Annotate `WINDOWS_BB_CHECKSUMS`
  provenance; revalidate the live pin.
- **Validation gate:** `bun run lint:actions` + `bun test packages/accelerator/scripts/` (copy-bb tests still
  green) + `bun run lint`. Layers: lint + unit.

### Phase 3 — docs + CI wiring
- Doc the Windows-pin provenance policy (how to add an independently-verified pin) in the accelerator README
  / a SECURITY note. Confirm the paths-filters trip on the touched files.
- **Validation gate:** full `bun run test` + `bun run lint:actions`. Layers: lint + unit.

## Security & Adversarial Considerations
- **Threat model:** a compromised/MITM'd upstream Windows release asset becoming a trusted, shipped sidecar.
  Auto-pin + immediate-merge = full auto-accept. Closed by removing auto-pin (pin needs independent human
  evidence) + leaving the PR open + the fail-closed CI gate.
- **Residual:** the pin is still a HASH, not an upstream signature — it authenticates "the reviewed bytes"
  but a human must obtain independent evidence to trust those bytes. Aztec does not yet sign `bb` releases
  (same SEC-02 circular-trust residual as F-007); documented. A malicious maintainer with merge rights can
  still add a bad pin — out of scope (repo-authority threat).
- **Least privilege:** the bump workflow already uses `gh` with the default token; leaving the PR open
  removes its merge action on the `false` path.

## Seeds (draft)
- `/goal`: F-008 fixed — auto-pin removed (advisory only, no fetch/write), `_aztec-update.yml` `false` leaves
  the PR open, live Windows pin provenance revalidated + documented; each phase's gate green; post-impl codex
  xhigh audit folded; PR into security-hardening CI green.
- `/loop 15m`: drive C7 — remove auto-pin → fail-closed advisory; leave-PR-open on auto_merge:false;
  provenance-annotate WINDOWS_BB_CHECKSUMS. After each edit run the touched test+lint. Commit/push. Consult
  codex on the provenance policy.
