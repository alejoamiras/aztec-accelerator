# Phase 6 — Land the bump PR (2026-06-18)

PR #363 (`chore/aztec-5.0.0-rc.1` → main). Reached **green** on head 37e346f: all 31 checks pass, zero failures — incl. SDK E2E (native+WASM on local 5.0), playground Local Network E2E, all 3 WebDriver platforms, Windows prebuild/build, Rust/Clippy, smokes. `main` is branch-protected → branch + PR + auto-merge (squash); commits unsigned via `git -c commit.gpgsign=false`.

## CI iteration log (what each head fixed)
- c5bf0a3 → first push; conflicted with main (1.0.7-rc.1 / 1.0.6 lessons advanced) → merged main (129b91a).
- c9d94db → **Windows Prebuild Smoke** fail-closed (the lockfile bump made resolveAztecBb() key on 5.0.0-rc.1; no pinned Windows bb SHA). Fixed by pinning the v5.0.0-rc.1 bb.exe SHA-256.
- bc35ab7 → **Local Network E2E** real fail (`Failed to get a note`); codex-verified `from: NO_FROM` tag-sender fix. Two Playwright `install-deps` flakes (exit 124) cleared on re-run.
- 37e346f → codex post-impl nits (sdk remote-test endpoint + stale deploy guard). **Full green.**

## Standing improvement
`sdk.yml` e2e now runs `build_accelerator: true`, so the native :59833 proving path is a permanent gate on every SDK PR — future aztec bumps auto-re-verify the SDK-only guarantee.

**Gate:** PASS — PR green; auto-merge (squash) enabled.

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-6.md
