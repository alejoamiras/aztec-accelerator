# C7 / F-008 — Windows bb.exe pin provenance — operator runbook

Two human-gated procedures. C7's fail-closed guarantee on `main` DEPENDS on both.

## 1. Adding a Windows bb.exe pin (when a bb bump needs one)

Pins are **never auto-generated** (F-008 removed the circular auto-download+write). When `@aztec/bb.js`
bumps to a version with no pin, the Windows Prebuild/Build Smoke CI gate goes RED
(`resolveWindowsBbChecksum` throws) and the bump PR is **left open** (`merge_mode: none`). A human then:

1. **Download** the asset from the matching release:
   `https://github.com/AztecProtocol/aztec-packages/releases/download/v<version>/barretenberg-amd64-windows.tar.gz`
2. **Verify what evidence exists.** Today there is NO independent cryptographic evidence for aztec-packages
   Windows assets — checked: `gh attestation verify … --repo AztecProtocol/aztec-packages` → 404, and
   upstream publishes no signed checksum file. The release TAG commit is GitHub-signed, but that does NOT
   bind the tarball. So the strongest available evidence is a careful **manual review**:
   - Confirm the release/tag is the expected upstream (author, CI provenance on the release page).
   - Diff the archive layout + `bb.exe` against the PRIOR pinned version (size, the "bb.exe only / no-DLL"
     canary in `copy-bb.ts`); an unexpected shape is a red flag.
   - `sha256sum` the tarball.
3. **Add the entry** to `WINDOWS_BB_CHECKSUMS` in `packages/accelerator/scripts/copy-bb.ts`:
   ```ts
   "<version>": {
     sha256: "<64-lowercase-hex>",
     provenance: "manual-review",
     note: "Reviewed <date>: <what you checked> — change-detector, not independently verified (SEC-02).",
   },
   ```
   `manual-review` is the ONLY provenance the resolver accepts today. It is a **change-detector**, not
   proof against a compromised upstream publisher.
4. Push to the open bump PR; the Windows gate re-fetches + re-verifies against your sha and goes green.

**Future (`attestation`):** when AztecProtocol adopts `actions/attest-build-provenance`, add an
`attestation` code path (verify with `gh attestation verify <asset> --repo AztecProtocol/aztec-packages`
pinning the signer workflow + source ref/digest) and flip entries to `provenance: "attestation"`. Until that
verification is implemented, an `attestation` entry FAILS CLOSED (the resolver rejects it) — a recognized
string is not verification.

## 2. Ruleset bypass readback (the fail-closed dependency)

C7's "a missing/unreviewed pin blocks the merge on `main`" holds ONLY if the C5 main ruleset is live AND no
actor can bypass it. This is external, live config the repo cannot prove. Read it back (owner/admin):

```bash
REPO=alejoamiras/aztec-accelerator
gh api "repos/$REPO/rulesets" --jq '.[] | select(.name=="Main branch protection") | {id,enforcement}'
RID=<id from above>
gh api "repos/$REPO/rulesets/$RID" --jq '{
  enforcement,                                   # EXPECT "active"
  target: .conditions.ref_name,                  # EXPECT refs/heads/main
  strict: (.rules[] | select(.type=="required_status_checks") | .parameters.strict_required_status_checks_policy),
  checks: [.rules[] | select(.type=="required_status_checks") | .parameters.required_status_checks[].context],
  bypass: .bypass_actors                          # EXPECT [] — the release-bot App must NOT appear here
}'
```
EXPECT: `enforcement=active`, target `main`, strict `true`, `checks` includes **`Accelerator Status`** (which
aggregates the Windows Prebuild/Build Smoke jobs), and `bypass_actors` is empty (in particular the
`RELEASE_BOT` App id is absent). If the bot is on the bypass list, `gh pr merge --auto` (or a human) could
merge past a red Windows gate — STOP and remove it before trusting the fail-closed guarantee.

Note: the C5 ruleset itself is applied via `clusters/C5-runbook.md` (commit+validate only; a human applies).
