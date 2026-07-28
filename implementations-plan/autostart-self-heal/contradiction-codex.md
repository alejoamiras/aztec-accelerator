# Contradiction-check — codex (gpt-5.6-sol, xhigh, read-only)

Session `019fa9c8-74a0-7b73-846d-e96a4e238006`. Reviewed plan.md r1 + the decision ledger.
Headline: **broke r1's Fork B safety claim with a concrete counterexample**, and conceded two of its
own prior positions (D11 canonicalization, the fail-closed marker).

Verdict: The consolidation is directionally strong but not internally consistent; Fork B, D13, D15, and the broken-state UI need correction before approval.

## Contradictions

1. **`plan.md` §3.3 D13 vs §§4.2, 4.5, 5.** Raw entry presence is not intent on Windows. `auto-launch/windows.rs:73-94` combines Run-value presence with `StartupApproved`; the plan’s `entry_present()` would rearm crash recovery after the user disabled startup in Task Manager, while `enabled = present && resolves` would show it ON. Define `intent_enabled`: presence plus platform override state. A `Broken` entry can then be intent-on but operationally unhealthy.

2. **§2 criterion 4 vs §§4.4–4.5.** “Heal can never create an entry” is not guaranteed. After the locked re-read, another app instance can process OFF before `rename`/`set_value`; the heal then recreates the entry. `set_enabled()` is not specified to take the same lock. All owned mutations must share it and revalidate. An absolute guarantee against external registry/file deletion is impossible and should be narrowed.

3. **§§4.8 and 5.** `Broken` renders an unchecked switch, yet D13 considers intent still enabled and rearms recovery. The user cannot turn it OFF; clicking the unchecked switch sends ON, while `can_repair_now:false` offers no action. Add an explicit Remove/Disable action and define broken-state transitions. Otherwise “status honesty” makes control worse.

4. **§4.7 vs §4.5’s “No launchctl.”** Editing the plist repairs next-login autostart, but does not update an already-loaded launchd job. Then `enable_crash_recovery()` sees `KeepAlive` in the file and returns early, leaving the current-session job targeting the old executable. “Heals crash recovery for free” is false without reload handling or an explicitly documented until-next-login gap.

## Wrongly rejected / wrongly adopted

- **D15 is backwards; my original `plist` position wins.** `plist 1.8.0` is already locked and is a runtime dependency through Tauri/Tauri Utils. The claimed new supply-chain surface is therefore absent. A hand scanner adds parser ambiguity and rejects binary plists merely to preserve formatting byte-for-byte—stronger than the actual requirement to preserve keys semantically. Use `plist` in production with atomic replacement.

- **D11: my canonicalized-write position was wrong.** Writing raw absolute `current_exe()` and canonicalizing only for identity is safer. Rust canonicalization can introduce `\\?\`; Microsoft explicitly warns that shell components may not interpret extended paths, while Run commands are limited to 260 characters. The plan should say compatibility is unproven, not categorically that Explorer rejects it, and add native execution proof. [Microsoft path guidance](https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation)

- **D7 is right, but the cost accounting is incomplete.** There is no hidden bundler dependency, no granted plugin ACL, and direct LaunchAgent handling preserves `MacosLauncher::LaunchAgent`. However, the new command also requires `build.rs`’s app-manifest command list and `scripts/tauri-trust-boundary.test.ts`’s static command sets; the app-name derivation must be centralized to retain `package_info().name` semantics.

- The marker amendment is only half-right. Permanent corrupt-marker fail-closed was my mistake. But path-only removal is unsafe: old and new versions normally run at the same expected install path, so a surviving old process could remove the marker mid-NSIS. Require candidate version **and** canonical expected path **and** successful rearm; expire corrupt markers using filesystem age plus a conservative TTL.

## Blind spots

- Highest-value missing test: a native Windows N−1→N update with a barrier after disarm, launching an alternate executable while the installed target is absent/stale. Assert it cannot heal or rearm, then assert N clears the marker and preserves the installed Run target. L1–L6 prove no updater-window concurrency property.

- “Resolves” still does not mean “will launch”: Linux `Hidden=true`/desktop-environment overrides and macOS launchd disable overrides are outside the taxonomy.

## Fork B verdict

**Put the marker in this PR.** Concrete interleaving: NSIS removes installed path `P`; Run still targets `P`; a second copy `Q` starts from Downloads; startup reconciliation sees `P` broken and live `Q`, writes `Q`; NSIS restores `P`; later `Q` is deleted, leaving autostart broken. The plan considered only starting from old `P`, not another live path. Additionally, the desired-path existence check and registry write are not atomic against NSIS deletion. Thus the marker protects autostart itself, not merely crash recovery.

## What looks fine

D1, D2, D7, D9, D12, and `can_repair_now` are sound. Section 9 is correctly scoped: the unquoted command creates a same-user binary-hijack/persistence opportunity, but not an elevation of privilege. [CreateProcessW security guidance](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw)