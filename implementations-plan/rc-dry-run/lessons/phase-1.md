# Lessons — rc dry-run (1.0.8-rc.1), phase 1

Purpose: execute residual #1 from the arc bug hunt — the **v1.0.7 → renamed-N release-call-path
boundary**, the one path no local gate, dispatch smoke, or PR CI can reach. Prereleases skip the S3
`latest.json` upload and `bump-source` and are marked `--prerelease`, so this is a rehearsal that
still signs with the production updater key.

## Attempt 1 — auth_probe (run 30640775782): PASS

`-f auth_probe=true` on 1.0.8-rc.1. `Validate Version` + `Release AWS trust preflight` green;
every build/e2e/tag/release job correctly skipped. Cheap insurance done first: an OIDC trust drift
would otherwise have surfaced ~40 minutes into a signing run.

## Attempt 2 — full prerelease pipeline (run 30640841649): in flight

Pre-state recorded for evidence:
- no `accelerator-v1.0.8*` tag existed remotely (`git ls-remote --tags` count 0)
- source version on main is already `1.0.8-rc.1` (so `bump-source` being skipped leaves source and
  tag consistent)
- N−1 fixture is the real `accelerator-v1.0.7` release asset, built BEFORE the rename, so its
  installed exe is `aztec-accelerator.exe` while N ships `AztecAccelerator.exe`; the call path
  passes `-N1BinaryName "aztec-accelerator.exe"` to the smoke for exactly this run
- `Bun.semver.order('1.0.8-rc.1','1.0.7') === 1`, so preflight check (b) must pass


### Attempt 2 result (run 30640841649): **residual #1 CLOSED**, one NEW product bug found

**The point of the exercise succeeded.** Both Windows updater legs passed against the REAL
pre-rename fixture:
- `Updater Smoke (windows-x86_64 / positive)` ✅ — v1.0.7 (`aztec-accelerator.exe`) → 1.0.8-rc.1
  (`AztecAccelerator.exe`): 4-point preflight, N−1 launch proof, old exe deleted, new exe present,
  Run value healed to the quoted new path, no transaction file, recovery re-armed.
- `Updater Smoke (windows-x86_64 / negative)` ✅ — tampered artifact still rejected across the
  rename boundary.
Also green: all 7 builds, all three pre-release WebDriver E2E legs, and every mac/linux updater
smoke (darwin-aarch64 pos+neg, darwin-x86_64 pos, linux-x86_64 pos).

**FAILED: `Post-build Smoke` + `Post-build Smoke (Intel notarization)` — classification: (a)
PRODUCT BUG, mine, from the rename PR (#426).**
Evidence: both jobs die at `hdiutil attach "$DMG" -nobrowse -mountpoint …`, which printed the
entire AGPL text and ended with `hdiutil: attach canceled`. Cause: the `bundle.licenseFile`
I added embeds a **Software License Agreement into the macOS DMG**, and `hdiutil attach` on an
SLA-bearing image requires interactive agreement. This is NOT a CI artifact: every macOS user
downloading the DMG would now have to accept a license agreement before they could mount it.

Why nothing caught it earlier: PR CI never builds DMGs (only the release pipeline does), and the
metadata audit reasoned only about the NSIS consumer — codex explicitly checked "/S suppresses the
NSIS license page" and stopped there. **This is exactly the residual-#1 class paying for itself:
a first release rehearsal surfaced a shipped-UX regression that four static rounds could not.**

Fix decision: `bundle.licenseFile` is GLOBAL in Tauri v2 — verified against the installed
`config.schema.json`, there is no `nsis`/`dmg`/`macOS` per-bundle license key — so it cannot be
scoped to Windows. Cost/benefit is one-sided: it bought an NSIS license page on INTERACTIVE Windows
installs only (silent/updater installs skip it), and it costs a mandatory SLA gate for every macOS
download. Remove `licenseFile`; KEEP the `license: "AGPL-3.0-only"` SPDX string (metadata only, no SLA).
The AGPL governs distribution and needs no click-through assent.

**Audit correction (codex, blocking — I was wrong).** I wrote that "the license still ships in the
bundle". FALSE: there was no `bundle.resources` entry, and `bundle.license` is an SPDX IDENTIFIER,
not the text — so removing `licenseFile` would have shipped packages containing no copy of the
AGPL at all, which sections 4 and 6 require recipients to receive. Fixed by adding
`"resources": { "../../../LICENSE": "LICENSE" }` (map form: the list form preserves the source's
directory structure, wrong for an out-of-tree path). That ships the text with NO assent gate.
Verified it cannot break the release bundle-shape invariant: that check inspects
`Contents/MacOS/` (`release-accelerator.yml:250-255`) while resources land in
`Contents/Resources/`.

Codex also confirmed the causal chain conclusively: `settings.license_file()` adds `--eula` in the
2.8.1 DMG bundler, which base64-encodes it and calls `hdiutil udifrez` — a UDIF EULA resource, so
the diagnosis was right even though my justification was not. Rejected alternative it agreed with:
`-acceptlicense`/`yes |` in CI would merely hide a shipped-UX regression. Noted for later if the
Windows licence page is ever product-desired: `tauri.windows.conf.json` can merge `licenseFile`
into the WINDOWS config only — not legally necessary, so not done now.

Bookkeeping: `Create Git Tag` was SKIPPED (smoke is in `tag.needs`), so **no tag was created** and
1.0.8-rc.1 stays re-dispatchable. (A first `git ls-remote | grep -c "1.0.8"` printed 1 and looked
like a tag existed — false positive: unescaped dots matched a commit SHA. Escaped, it is empty.
Same vacuous-assert class this whole arc has been hunting; verify the shape of a check before
trusting its answer.)


## Attempt 3 — full prerelease pipeline (run 30644544360): **GREEN end-to-end**

Dispatched on `b294df1` (the licence fix), same version `1.0.8-rc.1` — legitimate because attempt 2
never reached `Create Git Tag` (it is gated behind the smoke), so no tag had to be deleted or moved.

23/23 jobs succeeded, 0 failed. Skipped, all by design for a prerelease: `Sign update feed`,
`Verify live updater feed`, `Bump source version` (no S3 `latest.json` publish, source stays at
1.0.8-rc.1), and `Cancel run if the pre-release gate failed` (the gate passed).

Evidence for residual #1, from the run log:
- `preflight OK: accelerator-v1.0.7 asset present, 1.0.8-rc.1 > 1.0.7, pubkey + endpoint unchanged`
  (both legs) — the 4-point fixture preflight
- `N-1 alive at 1.0.7` (both legs) — the launch proof, i.e. the PRE-RENAME binary
  (`aztec-accelerator.exe`) really ran before any update
- `SUCCESS — updated to 1.0.8-rc.1 via the local feed (artifact downloaded + relaunched)` — the
  positive leg's full tail, which only passes after: new-name exe present, OLD-name exe deleted by
  the installer's `OldMainBinaryName` logic, Run value healed to the quoted new path, no
  update-transaction file surviving, and crash recovery re-armed
- `Updater Smoke (windows-x86_64 / negative)` green — a tampered artifact is still rejected across
  the rename boundary
- `Post-build Smoke` + `Post-build Smoke (Intel notarization)` green — the DMG mounts without an
  assent gate again, and the licence ships as a resource
- Outputs: tag `accelerator-v1.0.8-rc.1` → `b294df1`; GitHub release created with
  `prerelease=true` and 16 assets

**Residual #1 is closed by measurement, not by argument.** Two attempts, one real product bug found
(the DMG SLA) plus one packaging defect caught in its fix (shipping AGPL software with no licence
text). Neither was reachable by any local gate, PR CI, or the seven static hunt rounds — PR CI never
builds DMGs, and no non-release job installs the real pre-rename fixture.
