# Post-impl `/code-review max --fix` — findings + dispositions

Workflow-backed max review (31 agents: 6 finders → per-location verifiers → synthesis). 23 candidates → 17 kept (15 reported at cap), 6 refuted. **Notably refuted** (verified correct): the NSIS `$UpdateMode` guard fires correctly for Tauri auto-update; `last_rotation_prompt_at` IS read (my codex-round throttle fix); the rotation-hooks "anchors accumulate" framing (rotate's non-removal is documented-intentional).

## Fixed
- **[High] Linux certutil ACE** (`trust/linux.rs`): the hardcoded `/usr/local/bin/certutil` was accepted via `is_file()` without the writability guard. Now every candidate (hardcoded + `which`) passes `is_writable_by_nonowner`; a writable location falls through to "not found".
- **[Confirmed] `#isHealthy` accepts arrays** (`accelerator-transport.ts`): `typeof [] === "object"`. Now excludes arrays AND requires a recognizable `/health` field (`status`/`api_version`/`aztec_version`/`available_versions`) — also raises the bar against the HTTPS-squatter (finding #2).
- **[High] remove_ca_trust left rotated anchors** (`trust/linux.rs` + `macos.rs`): remove now deletes ALL our anchors — Linux enumerates `aztec-accelerator-ca-*` nicks per store (`our_nicks_in_store`/`delete_all_ours`); macOS loops delete-by-CN until none remain. Makes rotate()'s "cleared by Remove-trust/uninstall" true.
- **[Confirmed] Onboarding re-prompt on upgrade** (`commands.rs`): `enable_https_inner` now short-circuits when `certs_exist() && is_ca_trusted()` — an upgrader clicking the wizard's pre-checked Start no longer gets a fresh Keychain dialog + redundant double-bind.
- **[Confirmed] `<24h` cert reported unusable** (`certs.rs`): `(not_after-now)/86400` truncated sub-day to 0 → `certs_exist()` false. Added `leaf_secs_remaining()`; `certs_exist` now checks seconds `> 0`.
- **[Cleanup] renew_cert sync** → `async` (matches enable_https siblings; doesn't block the webview loop on the OS dialog).
- **[Cleanup] get_onboarding_state ran unused trust_status** subprocesses on every wizard open → removed the field + the shell-outs.
- **[Cleanup] get_trust_status/remove_https_trust dead** → removed the unused `get_trust_status`; **wired `remove_https_trust` to a new Settings "Remove certificate trust" button** (delivers D5).
- **[Cleanup] stale docs**: `leaf_cert_days_remaining` mtime comment + `CertPaths::staged` "(macOS) trusted" corrected.

## Accepted / documented (not fixed)
- **[PLAUSIBLE #2] prefer-HTTPS favors a localhost HTTPS squatter.** Partially mitigated (the `#isHealthy` schema check raises the bar). The residual — a same-user process with its OWN browser-trusted cert (e.g. mkcert) squatting :59834 — is past the recorded SEC-04 line ("machine already compromised"); dApps needing a hard guarantee set `httpsOnly`. This is the plan-approved prefer-HTTPS design.
- **[Confirmed #6] onboarding non-HTTPS failure recovery.** Addressed by the codex-round fix that re-enables Start+Skip on any partial failure (Start = retry-all). The extra Retry affordance is HTTPS-specific by design.
- **[PLAUSIBLE #8] non-OK HTTP path awaits HTTPS up to 2s.** Intended: HTTP-non-OK + a real HTTPS endpoint (Safari) must be found; the 2s bound is the probe timeout. Rare stall only when a bound-but-stalled foreign process holds :59834.
- **[Cleanup] main.rs reads config 3× on startup / complete_onboarding 3 disk writes.** Minor I/O on cold paths; left for a follow-up (correctness fine).

Re-validated: Linux 24 lib + 7 main + real NSS integration test; clippy -D clean; Windows target clippy -D clean; SDK 53; `bun run test` + `bun run lint:actions` exit 0.
