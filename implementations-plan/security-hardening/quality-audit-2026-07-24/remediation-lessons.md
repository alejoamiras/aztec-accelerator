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

## Notes
- `semver = "1"` was already a core dependency — no new dep.
- Only `resolve_version_flags_uncached_for_download` used a default (unknown-bundled) state with a
  real version; updated it to a proper bundled floor. No full-path prove test sends a valid
  non-bundled version, so the new 403 path doesn't disturb existing prove tests.
