# C7 / F-008 — bb-windows-provenance — plan (mid tier) — REVISED after dual audit (both REJECT)

## Summary
Two concrete defects + one deeper structural one around the Windows `bb.exe` supply-chain pin:
1. **Auto-pin footgun.** `update-aztec-version.ts::pinWindowsBbChecksum` (:80) downloads the Windows release
   asset, SHA-256s it, and auto-writes that hash into `copy-bb.ts::WINDOWS_BB_CHECKSUMS` labeled
   "auto-pinned" — a circular pin (just "the bytes that arrived"). The bump workflow does NOT commit
   `copy-bb.ts` (`_aztec-update.yml:139` = `git add packages/*/package.json bun.lock`), so the automated PR
   is UNPINNED — but the auto-write is a live hazard for local/manual bumps and any future broadened
   `git add`, and it is the mechanism by which a circular pin would ever enter the tree.
2. **Immediate-merge.** `_aztec-update.yml` on `auto_merge: false` runs `gh pr merge --squash` (:239) — an
   IMMEDIATE merge that lands the (unpinned) bump BEFORE CI runs, bypassing the Windows gate.
3. **No independent evidence (structural).** EMPIRICALLY CONFIRMED: AztecProtocol/aztec-packages publishes
   NO build-provenance attestation (`gh attestation verify` → 404) and no signed upstream checksum
   (`copy-bb.ts:7-11`). The release TAG commit is GitHub-signed but that does NOT bind the tarball. So every
   current pin is circularly sourced ("verified on windows-latest" = CI re-downloaded + hashed = circular).
   The master plan says "block the version if no independent evidence" (`plan.md` F-008) — so the honest
   outcome is: the pin is a HUMAN-REVIEWED CHANGE-DETECTOR, its provenance must be machine-enforced (not a
   comment), and the automation must never mint one.

Fix: **remove auto-pin; leave the bump PR open (fix immediate-merge); make pin provenance a structured,
resolver-enforced field; delete the ungated nightlies bump path; document the change-detector policy + the
deferred upstream-signing residual (= F-007 SEC-02).**

## Decision ledger — dual audit (codex REJECT + fable REJECT), folded
- **Both — advisory/comment "flag" is insufficient.** A legacy/unverified entry left in the map is still
  fully trusted (the resolver reads only the hash). FOLD: structured `{ sha256, provenance }` where
  `resolveWindowsBbChecksum` throws on a missing/unrecognized provenance — machine-enforced, not a comment.
- **Both — no independent evidence exists today** (confirmed empirically). FOLD: provenance enum =
  `manual-review` (the best available: a human downloaded, inspected the release + diffed vs the prior asset,
  and recorded it) | `attestation` (future). All current entries → `manual-review` with an honest note; the
  residual (not proof vs a malicious upstream publisher) = SEC-02, documented + deferred. NOT "flag but
  continue" — the provenance field is REQUIRED and typed.
- **Both — remove auto-pin entirely** (never fetch/write the hash). FOLD (Phase 1).
- **Both — `auto_merge:false` ⇒ leave PR open.** FOLD: replace the ambiguous boolean with
  `merge_mode: none|auto` (default `none`); `none` → create PR + STOP; `auto` → `gh pr merge --auto --squash`
  (CI-gated). (Phase 2.)
- **fable F1 / codex nightlies-gap — the nightlies bump targets base `nightlies`, which no `pull_request`
  workflow gates** (`accelerator.yml:6` = `[main, security-hardening]` only). FOLD: **DELETE
  `aztec-nightlies.yml`** — the `nightlies` branch is UNUSED (campaign directive; C5 already deleted
  `publish-nightlies.yml`). This removes the only ungated caller; only `aztec-stable → main` remains, which
  HAS the Windows gate + the required `Accelerator Status` check. Reviving nightlies needs a reviewed source
  change + a gate on the nightlies base. (Phase 2.)
- **fable F2 / codex — required-check + bypass are LIVE config, not proven by the repo.** The checked-in
  `main-branch-protection.json:35` requires `Accelerator Status` (strict) and `accelerator-status` aggregates
  the Windows jobs (`accelerator.yml:488`) — but per the master plan that JSON is DESIRED STATE requiring
  human apply + readback (`plan.md:48`; C5-runbook applies it). So the required-check AND the bypass-actor
  list are the SAME live ruleset: neither is "verified" by the repo. FOLD: C7's fail-closed on `main` is a
  DOCUMENTED human-gated DEPENDENCY on the C5 ruleset being live + the bot absent from bypass — a live
  readback (enforcement active, main target, strict, `Accelerator Status` required, zero bypass actors), not
  a claimed fact. (Phase 3 runbook + Assumptions Inference.)

Final-codex pass on the revised plan (both prior legs rejected) → **reject** with operational blockers, all
folded (design direction confirmed sound):
- **R2-1 (ordering):** the updater runs BEFORE `bun install` (`_aztec-update.yml:116-119`) but
  `resolveAztecBb()` needs installed deps → pin-status moved to a dedicated POST-install
  `check-windows-bb-pin.ts` step; schema migration (copy-bb.ts) is now Phase 1 (BEFORE the reporter uses it).
- **R2-2 (overclaim):** "required-check VERIFIED" downgraded to a runbook live-readback dependency (above).
- **R2-3 (least-priv concrete):** `none` never merges; stale cleanup `gh pr close` WITHOUT `--delete-branch`;
  drop unneeded caller `pull-requests:write`; `contents:write` retained only for the push (documented).
- **R2-4:** resolver accepts ONLY `manual-review` today; `attestation` is reserved + throws until
  `gh attestation verify` (repo/signer/source binding) is implemented — a recognized string is not
  verification. Current entries get honest legacy notes (no invented provenance).
- **R2-5:** delete-nightlies leaves a stale `CLAUDE.md:13` reference → removed. `updateCrsCacheVersion` also
  keys on argv (not bb.js) — correctness debt, NOT a Windows bypass; noted out-of-scope.
- **Reconciliation:** master-plan "block if no independent evidence" (`plan.md:51`) = block the AUTOMATION
  from silently accepting (no auto-pin; human must add + review) — NOT delete all Windows support (which
  would break the app with no path forward until Aztec signs). Explicit (A4).

DECISION: GATE 1 closes here. The final-codex pass CONFIRMED the design direction (auto-accept closed after
delete-nightlies; manual-review the correct availability choice; merge_mode/validation/delete-nightlies
sound; residuals honest) and its "reject" was operational-concreteness (ordering, live-verification framing,
least-priv specificity) — all folded above into the operative Design/Phases. Proceeding to implementation
rather than a 4th planning pass (direction settled; concrete fixes; GATE 3 post-impl codex on the ACTUAL
diff backstops — same discipline as C6).
- **codex — validate the forced version input BEFORE `$GITHUB_OUTPUT`** (`_aztec-update.yml:64`). FOLD (Phase 2).
- **codex/fable F6 — least privilege.** After removing merge, the create-pr job needs `contents:write` mainly
  for stale-branch cleanup. FOLD: drop `pull-requests: write`-for-merge reliance; scope the token / stop
  auto-deleting branches, or document the retained capability. (Phase 2.)
- **fable F7 — the advisory must key on `resolveAztecBb().version` (bb.js)**, the SAME version the gate
  resolves, not the argv (aztec.js) version. FOLD (Phase 1).
- **Both — `copy-bb.test.ts` hard-codes `4.2.0`, not the live version.** FOLD: resolve the live version in
  the test. (Phase 1/2.)
- **fable F5 / codex — reframe the npm path.** The macOS/Linux npm bb shares the circular root cause
  (bun.lock integrity = whatever npm served) but is MITIGATED by `minimumReleaseAge=604800` (7-day
  quarantine) + `--frozen-lockfile`; Windows has NO min-age analog (arguably worse). Corrected framing +
  documented; a Windows min-age analog is a noted hardening (out of scope unless cheap). (Assumptions.)
- **codex misstatement corrections:** the workflow does NOT commit the auto-pin; the current map has no
  literal "(auto-pinned)" entries (all are circular via CI/manual download); "Aztec doesn't sign releases"
  → the tag is GitHub-signed, the artifact is not attested. Corrected throughout.

## Design (folded)
1. **`copy-bb.ts` (schema FIRST)** — `WINDOWS_BB_CHECKSUMS: Record<string, { sha256: string; provenance:
   'manual-review' | 'attestation'; note: string }>`. `resolveWindowsBbChecksum` returns `sha256` ONLY for
   provenance `'manual-review'` (the sole type usable today); missing entry OR any other/empty provenance ⇒
   throw (fail-closed). `'attestation'` is RESERVED but NOT YET accepted — enabling it requires implementing
   `gh attestation verify` (pinned repo `AztecProtocol/aztec-packages` + signer-workflow/source-ref/digest
   binding) first; until then an `attestation` entry throws. All current entries → `manual-review` with
   HONEST notes ("legacy pin, adopted as a change-detector — not independently verified"; mass relabeling
   would invent provenance). `fetchWindowsBb` unchanged (re-fetch + verify vs the resolved sha).
2. **`update-aztec-version.ts`** — delete `pinWindowsBbChecksum` (no fetch, no write, no pin logic at all).
   The updater only touches `package.json` + CRS. Pin status is reported by a SEPARATE step (below), not the
   updater — so it doesn't depend on install order.
3. **Pin-status check (POST-`bun install`)** — a dedicated `check-windows-bb-pin.ts` (or a `copy-bb.ts`
   export) run as a `_aztec-update.yml` step AFTER `bun install`, resolving the live bb.js version via
   `resolveAztecBb()` (the SAME key the gate uses). Present manual-review pin → "✓ present"; else → a loud
   "⚠️ MANUAL PIN REQUIRED" block + the provenance-policy steps + a note that the Windows gate stays red
   until a human adds a reviewed pin. Read-only; never fetches/writes.
4. **`_aztec-update.yml`** — `merge_mode: none|auto` (default `none`); `none` → create/update PR, echo URL,
   STOP; `auto` → `gh pr merge --auto --squash`. Validate `inputs.version` in `check-update` BEFORE
   `$GITHUB_OUTPUT`. **Concrete least-privilege:** the `none` path never invokes `gh pr merge`; stale cleanup
   `gh pr close`s WITHOUT `--delete-branch` (drops the delete need; `contents:write` is retained only for the
   branch PUSH, unavoidable, documented); remove any unnecessary caller-level `pull-requests:write`
   (`aztec-stable.yml`). A separate conditional contents-write path is reserved for `auto` (no current
   caller). Update caller `aztec-stable.yml` to `merge_mode: none`.
5. **Delete `aztec-nightlies.yml`** (unused ungated caller) + remove its stale reference in `CLAUDE.md`.

## Phases

### Phase 1 — `copy-bb.ts` structured provenance (schema FIRST) + remove auto-pin (+ tests)
- Migrate `WINDOWS_BB_CHECKSUMS` to `{ sha256, provenance, note }`; `resolveWindowsBbChecksum` returns the
  sha ONLY for `manual-review`, throws on missing / `attestation` (reserved) / unrecognized / empty. Annotate
  all current entries `manual-review` with honest legacy notes. Delete `pinWindowsBbChecksum` from
  `update-aztec-version.ts` (no fetch/write/pin logic; remove its `main()` call). Fix `copy-bb.test.ts`'s
  hard-coded `4.2.0` → resolve the live version.
- **Validation gate:** `bun test packages/accelerator/scripts/` (resolver accepts manual-review; REJECTS
  missing / attestation-reserved / unrecognized / malformed-sha / empty-note; live-version test) +
  `bun test scripts/update-aztec-version.test.ts` (no pin logic remains; no fetch/write) + `bun run lint`.
  Layers: unit + lint.

### Phase 2 — pin-status check (post-install) + `_aztec-update.yml` leave-open + least-priv + delete nightlies
- Add `scripts/check-windows-bb-pin.ts` (read-only; resolves `resolveAztecBb().version`; prints
  present/MANUAL-PIN-REQUIRED; NEVER fetches/writes). `_aztec-update.yml`: run it as a step AFTER
  `bun install`; `merge_mode: none|auto` (default none) → `none` leaves the PR open; validate `inputs.version`
  in `check-update` before `$GITHUB_OUTPUT`; stale cleanup `gh pr close` WITHOUT `--delete-branch`; drop
  unneeded caller `pull-requests:write`. Update `aztec-stable.yml` → `merge_mode: none`. DELETE
  `aztec-nightlies.yml` + its `CLAUDE.md` reference.
- **Validation gate:** `bun test scripts/check-windows-bb-pin.test.ts` (present vs required; no network) +
  `bun run lint:actions` + a static assertion that the `none` path contains NO `gh pr merge` invocation +
  a `check-update` invalid-forced-input test/guard + `bun run lint`. Layers: unit + lint.

### Phase 3 — docs + provenance policy + runbook
- Document the Windows-pin provenance policy (how a human adds a `manual-review` pin: download the asset,
  verify the release page + tag signature, diff vs the prior pinned asset, record sha + honest note; the
  `attestation` type + its `gh attestation verify --repo AztecProtocol/aztec-packages` + signer/source
  binding is FUTURE work, reserved until Aztec ships build attestations). Runbook note: C7's fail-closed on
  `main` DEPENDS on the human having applied the C5 main ruleset (required `Accelerator Status`, strict) AND
  the release-bot App NOT being on any bypass list — add a live readback step (enforcement=active, main
  target, strict, `Accelerator Status` required, zero bypass actors). Confirm paths-filters trip on the
  touched files.
- **Validation gate:** full `bun run test` + `bun run lint:actions`. Layers: lint + unit.

## Security & Adversarial Considerations
- **Threat model:** a compromised/MITM'd upstream Windows release becoming a trusted shipped sidecar via
  (a) a circular auto-pin or (b) an immediate-merge that skips the CI gate. Closed by removing auto-pin,
  leaving the PR open (CI-gated on main + required `Accelerator Status`), machine-enforced provenance, and
  deleting the ungated nightlies caller.
- **Residual — no upstream artifact signing (SEC-02, = F-007).** No attestation / signed checksum exists
  today; `manual-review` is a human-reviewed change-detector, NOT proof against a malicious upstream
  publisher. Deferred until Aztec ships build attestations (then `provenance: 'attestation'` +
  `gh attestation verify --owner AztecProtocol --repo aztec-packages` pinning repo/workflow/ref). Documented.
- **Residual — repo authority.** A maintainer with merge rights / the release-bot App key / a ruleset
  bypass can still add a bad pin — out of scope (repo-authority threat); the runbook's bypass-list check is
  the human lever.
- **Residual — npm path.** macOS/Linux bb shares the circular root cause but is mitigated by the 7-day npm
  min-age quarantine + `--frozen-lockfile`; Windows lacks a min-age analog (noted hardening).
- **Least privilege:** the bump token loses its merge action on the `none` path; scope/split retained.
- **Input validation:** `pinWindowsBbChecksum` deletion removes the only version→fetch-URL+file-write
  injection surface; `check-update` validates the forced input before emitting outputs; the strict
  `VERSION_PATTERN` gate is retained.

## Assumptions
### Facts (verified)
- `pinWindowsBbChecksum` fetch+auto-write (`update-aztec-version.ts:80-98`), called in `main()` (`:137`);
  the workflow commits only `packages/*/package.json bun.lock` (`_aztec-update.yml:139`) — auto-pin NOT
  committed by the workflow. `resolveWindowsBbChecksum` throws on a missing pin (`copy-bb.ts:76-85`); the map
  is read only by `fetchWindowsBb` on Windows (`copy-bb.ts:100,182`). `auto_merge:false` →
  `gh pr merge --squash` immediate (`_aztec-update.yml:239`); default is `true` (`:26`). Callers:
  `aztec-stable → main` (gated), `aztec-nightlies → nightlies` (UNGATED; branch unused). Main ruleset
  requires `Accelerator Status` strict (`main-branch-protection.json:35`); `accelerator-status` aggregates
  Windows jobs (`accelerator.yml:488`). No attestation for aztec-packages (empirical 404); no signed upstream
  checksum (`copy-bb.ts:7-11`); rc.2 pin == downloaded bytes (circular, empirical). `copy-bb.test.ts` hard-
  codes 4.2.0 (`:20`).
### Inferences (verify in impl)
- Removing auto-pin ⇒ a new-version bump PR is unpinned ⇒ the Windows gate throws ⇒ (on main) red
  `Accelerator Status` ⇒ blocked merge, PROVIDED the bot is not on the ruleset bypass list (runbook).
- `manual-review` is the strongest evidence available today; `attestation` becomes available only if Aztec
  adopts `actions/attest-build-provenance`.
### Asks (defaults chosen — flag to override)
- A1: pin provenance is a structured, resolver-enforced field (`manual-review`|`attestation`); resolver
  fail-closes on missing/unrecognized — chosen.
- A2: `merge_mode: none` default; `none` leaves the PR open — chosen.
- A3: DELETE `aztec-nightlies.yml` (unused ungated caller) rather than build a nightlies-base gate — chosen.
- A4: `manual-review` entries remain USABLE (Windows must build); the residual upstream-signing gap is
  documented + deferred, NOT a hard-block of all current versions — chosen (a hard-block would break Windows
  entirely with no path forward until Aztec signs).

## Seeds (draft)
- `/goal`: F-008 fixed — auto-pin removed (no fetch/write, keys on resolved bb.js version), `merge_mode:none`
  leaves the PR open, `WINDOWS_BB_CHECKSUMS` structured + resolver fail-closed on unrecognized provenance,
  `aztec-nightlies.yml` deleted, provenance policy + bypass-list runbook documented; each phase's gate green;
  post-impl codex xhigh audit folded; PR into security-hardening CI green.
- `/loop 15m`: drive C7 — remove auto-pin → read-only reporter; structured resolver-enforced provenance;
  merge_mode:none leave-open + validate-input-before-output; delete nightlies. After each edit run the
  touched test+lint (+ lint:actions for workflows). Commit/push. Consult codex on the provenance schema.
