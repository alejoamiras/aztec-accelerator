# C7 / F-008 — fable dual-audit leg — VERDICT: REJECT (3 blockers)

## Empirical (run during audit-fold, resolves fable F3)
- `gh attestation verify barretenberg-amd64-windows.tar.gz --repo AztecProtocol/aztec-packages` → **HTTP 404**
  (no build-provenance attestation). The attestations API 404s too. So `gh attestation verify` as a trust
  anchor is NOT available today — the competing "attestation-gate" approach is off the table.
- The downloaded v5.0.0-rc.2 Windows asset sha256 = `c0bf2429...a7842` == the pinned
  `WINDOWS_BB_CHECKSUMS["5.0.0-rc.2"]`. The current pin IS "the bytes we download" — circular confirmed.

## Blockers (fable)
- **F1 (BLOCKER):** the nightlies bump path has NO CI gate — `accelerator.yml` runs only on PRs into
  `[main, security-hardening]`, but `aztec-nightlies.yml` targets base `nightlies`, so the Windows Prebuild
  Smoke gate (the only thing that runs `resolveWindowsBbChecksum`) never executes there. Fail-closed
  enforcement is absent on that caller. (Mitigated in practice: the `nightlies` branch is UNUSED per the
  campaign — but the robust fix is branch-independent enforcement in the SCRIPT, not CI-location-dependent.)
- **F2 (BLOCKER):** fail-closed depends on an UNVERIFIED assumption that the Windows gate is a REQUIRED
  status check with admin-override off. `gh pr merge --auto` (retained auto_merge:true) merges as soon as
  REQUIRED checks pass; a non-required red Windows check doesn't block it (nor a human clicking merge).
- **F3 (BLOCKER):** "independent evidence" is likely unavailable (confirmed above: no attestation, upstream
  publishes no signed checksum). The advisory "human obtains independent evidence" then relocates the
  circular trust to a tired human running `sha256sum` of the same download. The fix must be honest: the pin
  is a human-reviewed CHANGE-DETECTOR; the residual upstream-signing gap = F-007's SEC-02, deferred.

## Also (medium/low)
- **F4:** every current pin ("verified on windows-latest") is CI-hashed = circular; none is independently
  verified. Don't relabel a re-run CI hash as "revalidated."
- **F5:** the macOS/Linux npm bb path is ALSO self-referential (bun.lock integrity = whatever npm served),
  but MITIGATED by `minimumReleaseAge=604800` (7-day quarantine) + `--frozen-lockfile`. Windows has NO
  min-age analog → arguably worse. Reframe "Windows-specific" → "Windows lacks the npm mitigations."
- **F6:** least-privilege claim is cosmetic — dropping the `gh pr merge` call doesn't drop the token's
  `contents:write`/`pull-requests:write`. Both live callers use auto_merge:false → could drop the merge
  capability, or scope it explicitly.
- **F7:** the advisory keys on the argv (aztec.js) version but the gate `resolveWindowsBbChecksum` keys on
  `resolveAztecBb().version` (bb.js) — equal today (lockstep) but a silent divergence risk. Advisory should
  resolve + check the SAME version the gate uses.
- **F8 (positive):** deleting `pinWindowsBbChecksum` removes the only version-string→fetch-URL+file-write
  injection surface; keep the "no network in either path" test.

## Competing approaches (fable)
- **Primary (attestation gate)** — UNAVAILABLE today (no attestation). Revisit if AztecProtocol adopts
  `actions/attest-build-provenance`.
- **Fallback (adopted direction):** make enforcement branch-INDEPENDENT — the bump SCRIPT hard-fails on a
  Windows-relevant bump with no pre-existing reviewed pin (visible on every caller, not CI-location-
  dependent), + make the accelerator status aggregator a REQUIRED check on main (human-applied). Honest
  hard-block, not a soft advisory that only lives in job logs.
