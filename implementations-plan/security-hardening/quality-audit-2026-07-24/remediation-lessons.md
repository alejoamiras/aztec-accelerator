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
| 5 | Updater rollback-race + bounded streaming | pending | |
| 6 | win_acl owner not verified | ✓ done | `fix(f-003): set + verify object OWNER == current user` |
| 7 | C8 rollback destroys recovery | pending | |
| 8 | C9 arbiter promote-before-build | ✓ done | `fix(c9): build auth popup before deciding active-slot` |
| 9 | Release-CI dispatch-ref / tag-verify | pending (infra: commit+validate only, human applies) | |

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

## Notes
- `semver = "1"` was already a core dependency — no new dep.
- Only `resolve_version_flags_uncached_for_download` used a default (unknown-bundled) state with a
  real version; updated it to a proper bundled floor. No full-path prove test sends a valid
  non-bundled version, so the new 403 path doesn't disturb existing prove tests.
