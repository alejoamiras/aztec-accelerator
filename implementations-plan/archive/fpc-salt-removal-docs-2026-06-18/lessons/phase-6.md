# Phase 6 — Delete the repo secret (2026-06-20)

After P4 (merged, salt-less `main`) + P5 (live salt-less playground smoked OK), deleted the obsolete repo secret.

- **Pre-delete safety:** `git grep SPONSORED_FPC_SALT FETCH_HEAD -- .github packages .env.example` (excl. prose `.md`) → **zero references on `main`**, so a deleted secret breaks nothing (and an unset `${{ secrets.X }}` would render `""` anyway — but there are no references at all).
- `gh secret delete SPONSORED_FPC_SALT` → success.
- `gh secret list` → now shows only `TESTNET_AZTEC_NODE_URL`; `SPONSORED_FPC_SALT` gone.

**Gate — PASS:** secret absent from `gh secret list`; no missing-secret errors possible (zero references). Reversible if ever needed (value was `0x0`).

**Posture:** one fewer secret to leak/rotate. The salt=0 canonical FPC address was always publicly derivable from the public artifact, so the secret only ever added obscurity, not security — deleting it is a small net hardening.

LESSONS_FILE=implementations-plan/fpc-salt-removal-docs-2026-06-18/lessons/phase-6.md
