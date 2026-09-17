# Phase 5 lessons — Arc D, ky removal (2026-08-25)

**Outcome: ✓ GREEN — PR #476 (`bun14-arc-d`). ky out of the published SDK; `TransportHttpError`
internal contract; F14 taxonomy frozen by tests before the swap.**

## What made this safe to do at all
The rewrite only stayed 1:1 because the F14 classification tests were written FIRST against the
ky behavior, then the implementation was swapped under them. The subtle contract ky was carrying:
`HTTPError.data` arrives PRE-PARSED (json for json media types, text otherwise) and the prover's
network-vs-HTTP branch keys off `instanceof`. Two places that would have silently drifted:
1. **Body-read failure on a non-2xx response** — naive `await res.json()` in a catch-all would
   have re-thrown as a generic error and DEMOTED an HTTP error to the network path, which is what
   activates the HTTPS→HTTP downgrade retry. Classification invariant (codex, security): status is
   preserved even when the body stalls, overflows the bound, or is malformed; `data` becomes
   undefined, class stays `TransportHttpError`. 6 adversarial tests pin both primary and
   downgrade-retry paths.
2. **Timeout scope** — ky's timeout spans the whole exchange; `AbortSignal.timeout()` on fetch
   does too, but the header/body budgets differ: `fetchHeaderBounded` clears its timer at headers
   so a slow body is charged to the BODY budget only. Proven by a real-socket test (Bun.serve
   fixture dribbling 1.2s headers + 1.2s body — under a shared 2s budget it would flake).

## Gotchas
- Mocked `fetch` handlers must normalize `input instanceof Request` — bun 1.4 passes a `Request`
  where 1.3 passed a string URL; tests written against the string form break silently (handler
  matches nothing, test times out).
- Media-type check uses the ESSENCE (`content-type` up to `;`) and `+json` suffixes — matching
  ky's parse decision; `application/problem+json` bodies parse, `text/plain; charset=utf-8`
  stays raw string.
- Off-main stacked PRs get ZERO `pull_request` runs (workflows filter `branches: [main, ...]`) —
  every CI-equivalent gate was hand-run locally under the scratch-pinned 1.4.0: full chain exit 0,
  sdk units 136/136, live e2e 10/10 (29.9s, testnet + headless accelerator), playground mocked
  8/8. Env-var trap while smoking: the sdk e2e config reads `ACCELERATOR_URL`, NOT
  `AZTEC_ACCELERATOR_URL` — the wrong name silently SKIPS all accelerator legs (7 pass/3 skip
  looks green; the skip count is the tell).

LESSONS_FILE=implementations-plan/bun-1-4-migration/lessons/phase-5.md
