# Audit — fable (fresh context, no prior exposure), plan revision 3

Ran in parallel with the codex audit, seeing neither it nor the earlier rounds beyond the two
contradiction-check records it was given as "do not repeat this" context.

**Verdict: conditional-approve.** Converges with codex on the two fatal areas (L8's barrier is not
implementable; D18's removal rule is circular) and adds four findings codex missed — most importantly
that D19, adopted from codex in r2, would break the Settings toggle during any background update.

---

**Verdict: conditional-approve** — Phases 1–2/4–7 are sound and source-verified; Phase 3 (D18) and
Phase 8 (L8) each contain a design hole that must be fixed on paper before implementation.

## Blocking

**1. §6 L8 / Phase 8 — the barrier is not implementable as specified.** `perform_update` runs disarm →
marker → `record_pending` → `install()` in-process within milliseconds; `install_inner` ShellExecuteW's
the extracted NSIS exe and `std::process::exit(0)`s (verified, vendored
`tauri-plugin-updater-2.10.1/src/updater.rs:788-867`). Nothing can "hold after disarm": the only
candidate holders are the app (which exits) or NSIS (no external pause hook). The change map
(`plan.md` §4.6) puts the barrier in `updater-smoke-windows.ps1`, but the ps1 has no process to hold.
Implementable only via (a) an env-gated pause hook inside `perform_update` — production code, absent
from the change map and unmodeled in §9 — or (b) a stub installer wrapped in the ephemeral-signed N
zip (delete target → wait on file → chain real setup.exe), possible only on the dispatch path and
described nowhere. The plan must pick one and cost it. Also: the dispatch path builds **two** full
Windows Tauri apps (pubkey-patched N−1 + N from ref) against a `timeout-minutes: 40` budget sized for
one (`_e2e-updater-windows.yml:50`).

**2. L8's subject sabotages its own assertion, exposing a production gap.** The alternate exe is a real
app copy sharing `~/.aztec-accelerator`; the smoke pre-seeds `auto_update:true`
(`updater-smoke-windows.ps1:196`), the feed is still serving N, the first check fires 5s post-launch,
and the updater lock died with the exited N−1 — so the process being asserted as "neither heals nor
rearms" starts **its own N update and a second NSIS run mid-barrier**. Underneath: the marker gates
heal and rearm but `perform_update` itself is not marker-aware, so a second instance can launch a
whole concurrent install inside the very window D18 protects. §2 criterion 6 holds while a strictly
worse operation stays permitted.

**3. D18's removal rule is circular and leaves unremovable states.** (a) Removal requires "a successful
rearm" while "no process rearms while a marker is live" — deadlock unless the removal transaction is
exempt; unstated, and a transient schtasks failure at N's first launch then suppresses heal+rearm
until TTL. (b) `install()` Err path (`updater.rs:436-442`): the marker is already written; the writer
(N−1) fails the candidate-version match, so it cannot clean up its own failed transaction — and
`CrashRecoveryGuard::drop` rearms immediately, either contradicting "no process rearms" or (if
suppressed) regressing today's Err-path rearm. (c) The change map has **no removal call site**:
`update_marker.rs` is "pure", `updater.rs` removes "around the disarm", but the actual remover is N's
startup in `main.rs` — absent from §4.6. (d) The queued `AztecAccelerator` rename — cited twice by the
plan itself — changes install dir and exe name, so renamed N runs from a path ≠ the marker's "canonical
expected install path": removal unsatisfiable, TTL limbo on the first rename release. (e)
Downgrade/manual reinstall after an NSIS catastrophe: version mismatch → same limbo; TTL value and
mtime-backward-clock behavior unspecified.

**4. D19 breaks the Settings toggle during updates.** `set_enabled` takes `updater.lock`, which
`perform_update` holds across the entire multi-minute download (`updater.rs:272`). Non-blocking
acquire → toggle hard-fails whenever a background update is downloading; blocking → UI hang.
Unspecified either way; §3.4's rejection of a dedicated lock never weighed this cost.

**5. Real-N−1 switch preconditions unverified.** `workflow_call` with `v1.0.7` requires: v1.0.7's
embedded pubkey == current prod key, endpoint host == `aztec-accelerator.dev` (the hosts/CA
impersonation), acceptance of the current `SignedEnvelope` v1 and `config_version:1` pre-seed. None are
checked; a mismatch turns the release gate red on release day. The dispatch leg also silently
reintroduces the pubkey-patched N−1 the current workflow deliberately abandoned
(`_e2e-updater-windows.yml:102-104`).

## Non-blocking

- §9 models `stored_path` and the dispatch trigger but not the marker itself: same-user-writable,
  mtime-forgeable (TTL bypass), perms unspecified. No new privilege vs deleting the Run value, but say so.
- The marker duplicates `record_pending`'s candidate version (`updater.rs:407-423`) — two files,
  divergent lifecycles; never discussed.
- L7 rationale inaccurate: the dev-mode WebDriver matrix includes `windows-latest`
  (`accelerator.yml:449-456`); excluding Windows is a registry-isolation choice, not impossibility.
  Also L7's launch runs the ungated startup rearm, which patches the seeded macOS plist (KeepAlive)
  before Fix — assertions must expect it; Linux may hit real `systemctl --user`.
- L3-Windows writes the real HKCU Run value — throwaway `$HOME` does not isolate the registry; needs
  serialized tests + panic-safe cleanup.
- `updater.rs:362-371`'s Err→assume-enabled rearm-safe default isn't carried into the `intent_enabled`
  spec.

## Highest-value missing test

**Execute a healed entry the way the OS would.** L1–L8 all assert stored bytes; none launches from
them — the quoting claim rests on `run_value_candidates`, a model checked against itself. Cheapest and
highest-yield: in L6, after the equality assertion, spawn the raw Run value via `Win32_Process Create`
(CreateProcess semantics — what Run processing actually does) and health-probe. D11 promises exactly
this ("prove the written value natively (L6)"); L6 as written doesn't do it.

## Genuinely fine

D1/D2/D7/D9/D12–D17/D20 are coherent against source; §5 is consistent with D13/D16/D17 (OFF always
available). §4.7's "until-next-login gap" is honest — the heal strictly improves persisted state and
leaves session state exactly as today. §8 Facts spot-checked true: `winreg` 0.10.1 + 0.55.0 both in
`Cargo.lock`; `plist` unconditional at `tauri-utils-2.9.2/Cargo.toml:149`; `exit(0)` at vendored
`updater.rs:865`; `build.rs:143` all-or-nothing; `commands.rs:462-464`; `main.rs:23/556/609/614`;
`settings.js:54` is the sole frontend caller, so the `AutostartStatus` shape change is fully covered.
