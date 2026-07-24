# Remediation lessons — quality-audit-2026-07-24 → sechard/quality-remediation

Executing the RELEASE-BLOCKING set from `CONVERGED-SCOPE.md`. Branch off
`origin/security-hardening`. Each item validated locally (cargo fmt/clippy + touched-crate
tests; frontend build + biome; Playwright/WebDriver are CI-only) and committed.

## Progress

| # | Item | Status | Commit |
|---|------|--------|--------|
| 1 | C9 authorize Remember unguarded + poll correctness | ✓ done | `fix(c9): gate authorize Remember…` |
| 2 | /prove per-origin piggyback sender cap | ✓ done | `fix(prove): cap per-origin piggyback senders` |
| 3 | Version-downgrade policy (x-aztec-version) | ✓ done | `fix(prove): enforce a safe-default bb version-downgrade policy` |
| 4 | CWD cache fail-open | ✓ done | `fix(cache): fail closed when home dir is unresolvable` |
| 5 | Updater rollback-race + bounded streaming | ✓ done | `fix(updater): record install intent BEFORE install; retryable-intent gate` |
| 6 | win_acl owner not verified | ✓ done | `fix(f-003): set + verify object OWNER == current user` |
| 7 | C8 rollback destroys recovery | ✓ done | `fix(c8): autostart rollback restores prior recovery + surfaces failures` |
| 8 | C9 arbiter promote-before-build | ✓ done | `fix(c9): build auth popup before deciding active-slot` |
| 9 | Release-CI dispatch-ref / tag-verify | ✓ YAML done; repo-settings → owner runbook | `fix(release): pin tag to github.sha + verify pre-existing tag` |

## Codex consults

### #3 version-downgrade policy (2026-07-24, gpt-5.6-sol xhigh)
Asked: is `requested >= bundled` the right floor; reject nightly/devnet even when newer; holes
in strict-semver-parse; anything missing for a production-safe default.

Codex verdict (adopted):
- **Use `cmp_precedence`, require STRICTLY GREATER** (not `>=`): SemVer's Rust `Ord` includes
  build metadata; precedence ignores it. Exact-bundled is normalized upstream, so equal precedence
  reaching the gate is a sidegrade → refuse.
- **Reject nightly/devnet independently** — SemVer doesn't understand tiers (`5.1.0-nightly >
  5.0.0-rc.2`). Implemented as a stronger **channel rule**: above the floor, allow only stable OR
  the bundled baseline's exact prerelease channel. Subsumes dev-build rejection AND blocks
  stable→unknown-prerelease and foreign channels (`alpha`, …).
- **Reject `+build` metadata** (ambiguous precedence). Added `HasBuildMetadata`.
- Validate allowlist entries are strict semver (unit test asserts it).

Codex also recommended (NOT yet done — noted for the owner / future work): a deny/revocation
mechanism (a newer authentic release can still regress), per-network/origin local pinning so the
remote header only *selects within* local policy. Logged; out of scope for the safe default.
Rate-limit downloads / cap cache / atomic install — partially covered by existing caps + item #5.

## Pre-PR gate (whole branch, so far)
- `bun run lint` → exit 0 (one PRE-EXISTING biome warning: unused `firstCallMs` in
  `accelerator-prover.test.ts`, not from this work; warnings don't fail).
- `bun run test:typecheck` → exit 0.
- `bun run lint:actions` → clean (release-accelerator.yml change).
- Per-crate: core 185 tests + clippy + fmt clean; src-tauri crash_recovery/updater tests pass
  (sidecar-stubbed); win_acl via `cargo check --target x86_64-pc-windows-gnu`; tauri bin
  `cargo check --features webdriver` clean (sidecar-stubbed).
- Playwright + WebDriver = CI-only (unsupported OS locally).

## Codex consults (cont.)

### #5 updater record_pending ordering (2026-07-24, gpt-5.6-sol xhigh) — RESOLVED, must-fix
Codex's decisive finding (verified against the pinned plugin source): on **Windows**,
`tauri-plugin-updater 2.10.1`'s `install()` dispatches the external NSIS/MSI installer and
`std::process::exit(0)`s — it **never returns**. So `record_pending` in the post-install `Ok`
branch NEVER ran on Windows → the downgrade window was a CERTAINTY there, not a rare fail-open.
Must-fix, not a documentable residual.

Adopted (acting on codex's stronger argument):
- **Record intent BEFORE `install()`, fail-closed** (abort if it can't be recorded / path unresolved).
- **Retryable-intent gate**: `candidate_allowed` = strictly-above `current`+`floor` AND `>= pending`
  (equal allowed). This is why record-before doesn't poison a version — the exact intent can be
  retried; a lower still-signed version stays blocked. Codex's `artifact_id` refinement (match the
  signed artifact identity on retry, not just the version) was NOT implemented: Layer A already binds
  version→signed-artifact, so a same-version retry can only be the legitimately-signed one absent
  signing-key misuse — noted as a possible future hardening.
- On `install()` Err the intent is KEPT (an Err isn't proof no mutation happened — codex).
- **Buffering / feed-size points**: codex CONFIRMED they're correctly accepted as availability-only
  residuals #345/M6 — the plugin buffers an unbounded `Vec` then verifies minisign; bounding
  bytes-read needs the R3-rejected hand-rolled downloader (would make hand-written verify the sole
  authenticity control). Ed25519 integrity unaffected. Future fix = upstream/pinned-fork byte limits
  inside the plugin's own download+verify loop.

## Round-2 codex re-audit (2026-07-24, gpt-5.6-sol xhigh) — 7 findings, all fixed
Re-audited the whole remediation diff. Confirmed SOUND: #2 piggyback cap, #4 cache fail-closed,
#5 updater intent-before-install, #6 win_acl owner, #9 release-CI tag pin. Found + fixed:
1. **(High)** authorize Remember shipped ENABLED → pre-JS click could pre-check it. Fix: HTML
   `disabled` attr + JS `.checked=false` on init.
2. **(High regression)** version policy fail-closed on unparseable bundled BRICKED headless (no
   `AZTEC_BB_VERSION` → "unknown" → every version 403). Codex caught it. Fix: unparseable bundled ⇒
   NO floor (headless has no baseline to downgrade from); desktop always has a compile-time baseline.
   Dropped `BundledUnknown`.
3. **(Medium)** macOS: re-running the autostart plugin enable recreates the LaunchAgent plist and
   strips KeepAlive → my round-1 "skip disarm" didn't save it. Fix: `enable_transaction` skips
   `plugin_enable` entirely when `prior_enabled`.
4. **(Medium)** `focus_on_create:false` only gated a post-build set_focus; tao builds focused by
   default → queued popups stole focus. Fix: `.focused(config.focus_on_create)` on the builder.
5. **(Medium)** updater `rearm_crash_recovery_if_enabled` used `is_enabled().unwrap_or(false)` →
   a read error after disarm left recovery OFF. Fix: fail SAFE (re-arm on unreadable state).
6. **(Medium)** settings UI: `get_autostart_enabled` error rejected the whole `Promise.all`, leaving
   the switch at its false "off" default. Fix: fetch independently; on error disable + hint.
7. **(Low)** `HasBuildMetadata` 403 unreachable over HTTP (`is_valid_version` rejects `+` → 400).
   Documented (kept for direct callers).

**Meta-lesson**: codex's round-2 catch of #2 (headless brick) shows round-1's fail-closed was an
over-application — the fix for a desktop-downgrade threat mustn't brick a baseline-less mode. Also:
NEVER put backtick-wrapped tokens in a double-quoted `git commit -m` under zsh — they run as command
substitution and silently gut the message. Use `-F <file>` / heredoc.

## Round-3 codex re-audit (2026-07-24, gpt-5.6-sol xhigh) — 3 residuals, all fixed
Re-audited the round-2 fixes (no-explore prompt after the first attempt was infra-killed at 1856
lines). SOUND: Remember-disable, plugin-enable-skip, popup `.focused`, build-metadata doc. Fixed:
- **(Medium)** version_policy: my no-floor early-return ALSO skipped request strict-semver +
  build-metadata validation → headless could pass `latest`/`+build`. Reordered: validate request
  first (always), skip only floor+channel when bundled is unparseable.
- **(Low)** updater rearm armed recovery even if autostart was off (is_enabled Err → true). Capture
  `was_recovery_enabled` ONCE before disarm; all re-arm decisions use that bool (no late re-read).
- **(Low)** settings autostart switch not disabled while state unknown / a preceding Promise.all
  reject skipped the catch. Now ships `disabled`, enabled ONLY on get_autostart_enabled success.

Convergence: round-1 remediation (9) → round-2 found 7 → round-3 found 3. Round-4 verifying.

## Round-4 codex re-audit (2026-07-24, gpt-5.6-sol xhigh) — 1 latent Low, fixed
SOUND: updater state-capture, settings-switch. Found: `VETTED_OLDER_VERSIONS` was checked BEFORE the
request semver/build-metadata validation → an allowlisted entry could bypass well-formedness (latent;
allowlist empty). Fix: validate the request FIRST (unconditional), allowlist bypasses only floor/channel.
Convergence: 9 → 7 → 3 → 1. Round-5 verifying (expected clean).

## Notes
- `semver = "1"` was already a core dependency — no new dep.
- Only `resolve_version_flags_uncached_for_download` used a default (unknown-bundled) state with a
  real version; updated it to a proper bundled floor. No full-path prove test sends a valid
  non-bundled version, so the new 403 path doesn't disturb existing prove tests.

## Round-5 codex re-audit (2026-07-24, gpt-5.6-sol xhigh) — SOUND
No findings. Well-formedness enforced before every successful path; allowlist bypasses only
floor/channel; desktop + headless baseline behaviour unchanged. **Codex satisfied — converged
(9 -> 7 -> 3 -> 1 -> 0).**
