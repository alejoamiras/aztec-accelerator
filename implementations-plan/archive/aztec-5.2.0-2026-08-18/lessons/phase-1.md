# Phase 1 lessons — bump + Windows pin (2026-08-19)

**Outcome: ✓ green on first pass. Commit `2780336`.**

- `bun run aztec:update 5.2.0`: only skip = the lockstep `@aztec-foundation/aztec-standards`
  (expected, A1: held at 5.0.1). Every `@aztec/*` package had a 5.2.0 → I2 confirmed.
- bunfig exact-name exclusion (31 names seeded from the old lock): `bun install` resolved in ONE
  pass — no min-age blocks, so 5.2.0 introduced no new @aztec transitives. Post-install check:
  excludes list == new lock's @aztec graph exactly (31/31, nothing to prune). I1 convergence
  confirmed trivially.
- Lockfile review: 108 changed lines (54+/54−), **all 108 contain `@aztec`** — zero non-@aztec
  movement. Strongest possible pass of the per-line criterion.
- `bun install --frozen-lockfile`: clean (CI parity).
- Windows pin: live bb.js == `5.2.0` (I3 confirmed). Two-channel verification (A3-as-resolved):
  downloaded-file sha256 `17fe17e1…d2fe` EQUALS GitHub API digest; size 5,970,432 both channels;
  asset id `RA_kwDOJAQCos4e3ffa` recorded in the pin note. `check-windows-bb-pin.ts` ✓.
- Prebuild proof: `copy-bb.ts` extracted Linux bb from the installed bb.js, `AZTEC_VERSION` →
  5.2.0.
- Gate `bun run test`: exit 0 (biome 4 warnings pre-existing, non-failing; cargo fmt, 3-graph
  typecheck, all unit suites green). **Zero API-drift fallout — no source changes needed
  anywhere.** I4 confirmed at the typecheck+unit layer.
- Env quirk: `cargo` not on the tool shell's default PATH — prefix
  `export PATH="$HOME/.cargo/bin:$PATH"` for any command that reaches `lint:rust`.
- Working tree after: exactly the canonical bump set (6 files) — same-commit guard satisfied.

LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-1.md
