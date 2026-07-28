# Audit — codex (gpt-5.6-sol, xhigh, read-only), plan revision 3

Session `019fa9c8-74a0-7b73-846d-e96a4e238006` (resumed from the contradiction-check, so it holds the
full history of its own reversed positions).

**Verdict: REJECT.** Six blocking findings. Two of them break things the owner had just approved:
L8's barrier is not implementable as specified, and D18's removal rule is circular — codex withdraws
its own "successful rearm" formulation as wrong when adopted literally.

Reject — the core autostart design is coherent, but D18 is not a closed state machine and L8 is not implementable as currently specified.

## Blocking findings

1. **L8 has no actual barrier mechanism (§6 L8 / §4.6 / Phase 8).**  
   Editing only `_e2e-updater-windows.yml` and `updater-smoke-windows.ps1` cannot pause after disarm while `P` is absent. `packages/accelerator/src-tauri/src/updater.rs:425` launches NSIS and exits; PowerShell is outside both processes and can only race-observe the window. A Rust barrier before `install()` leaves `P` present and therefore does not test the counterexample.

   Make the synthetic N−1 installer test-only: add a sentinel barrier to its `NSIS_HOOK_POSTUNINSTALL`, signal “ready” after removal, wait for “release,” and assert `P` is absent before launching `Q`. The repository already has `packages/accelerator/src-tauri/nsis/hooks.nsi:44`. Without such an installer hook, L8 is aspirational.

2. **D18 is circular and mishandles OFF (§3.2 / §4.4).**  
   “While live, no process rearms” conflicts directly with “removal requires successful rearm.” There must be a narrowly defined matching-candidate owner transition allowed to reconcile recovery.

   More seriously, if intent was OFF—or the user turns it OFF during NSIS—rearming is forbidden, so the marker can never meet its removal rule. Store/recompute intent and require **recovery reconciled to current intent**: armed when ON; confirmed disarmed when OFF. I withdraw my original “successful rearm” formulation; adopted literally, it was wrong.

3. **Version + path + rearm is not proof NSIS finished (§8 inference).**  
   Once the new executable has been copied to `P`, it can be launched manually while NSIS is still copying other files. It matches candidate and path and can successfully rearm, yet installation is not complete. This can reintroduce the half-installed-launch hazard the disarm exists to prevent. Use installer-produced completion evidence—ideally a production `POSTINSTALL` completion token—or at minimum prove the installer process has exited. Rearm itself is not completion evidence.

   Likewise, TTL is a liveness heuristic, not a “safe backstop”: an installer hung longer than TTL reopens the exact `P`-absent healing race.

4. **The marker does not serialize subsequent updates.**  
   The updater lock dies when the initiating process exits. A second copy can then acquire it and start a higher permitted update, overwriting or racing the first marker. `perform_update` must reject a live foreign marker under the updater lock, and marker creation must be compare-and-create, not unconditional replacement.

   Stranded states include: install failure/no candidate launch; downgrade or manual reinstall at another version; matching version at another path; transient rearm failure with no retry; OFF intent; corrupt/future-dated marker. Exact-version sideways reinstall at the expected path can recover; higher/lower reinstall cannot until expiry.

5. **The dispatch signing design is underspecified and its security claim is too strong (§6/§9).**  
   Ephemeral signing is coherent only if the generated public key is patched into `tauri.conf.json` before building both N and N−1, and the same private key signs N’s artifact and manifest. The current workflow deliberately keeps the committed production pubkey in N−1, so merely changing the private key will fail verification.

   Also, `workflow_call.secrets` is not an isolation boundary: repository secrets remain addressable in a `workflow_dispatch` run, and this YAML already references the production key. Separate secretless dispatch and production jobs/workflows, event-guard every production step, and protect the production key with a release-only environment. `contents: read` prevents publishing a release, but not uploading a production-signed Actions artifact.

6. **Real v1.0.7 needs a version preflight.**  
   F-004 rejects any N ≤ 1.0.7. Moreover, the `accelerator-v1.0.7` tag’s source says `1.0.7-rc.1`; release builds patch it in-job. Assert the installed N−1’s `/health` version before enabling updates, and fail early unless N is strictly greater. The redirected feed prevents the old updater contacting production, and a fresh runner avoids residual floor/StartupApproved state. Calling this path “unchanged” is nevertheless false—the fixture changed materially.

## Non-blocking

- A writable marker gives same-user malware renewable suppression of healing/recovery. That is availability, not privilege escalation, but TTL can be defeated by refreshing or future-dating mtime. Require owner-private ACLs, bounded reads, reparse-point refusal, and defined future-time handling.
- `textContent` solves injection, not privacy. `stored_path` and `info` logs expose full usernames/paths although the proposed UI does not need them. Remove/redact them.
- Highest-value missing case after L8: update while autostart intent is OFF, proving the candidate clears the marker without arming recovery. Also test returned install failure and a second updater attempt.
- launchd’s plist `Disabled` key is only part of macOS disable state; the plan correctly lists domain/session state as a known limit.

## What looks fine

The new `plist`, dual-`winreg`, and app-manifest Facts check out. D7, D11–D17, D19, and the qualified D20 remain sound. The Windows unquoted-Run finding is correctly scoped as same-user persistence hijacking, not EoP.
