# audit-fable.md — fable audit trail, v2-release-train

Two fable roles: (a) the planning-leg agent, resumed once for the contradiction-check;
(b) a FRESH hostile auditor for the double audit (no prior context). Planning leg full text:
`plan-fable.md` (its contradiction contribution is appended there too).

## Contradiction-check on plan.md rev 1 (resumed leg) → 8 findings

1. promote-feed permissions contradiction (upload needs contents:write) → capture in publish
   job from the public CDN. [ADOPTED rev 2; capture dropped rev 3]
2. Freeze doesn't freeze the landing page → switch landing to /releases/latest (Latest badge
   is post-promote). [ADOPTED, D-C11]
3. "Full regate on stable" unimplementable with in-run promote → hold → gate on published
   bytes → promote-only. [ADOPTED, D-C11]
4. Partial-publish wedge → promote-only hard-verifies asset URLs; runbook additive-upload
   recovery. [ADOPTED, D-C10]
5. Fresh-install stage silently dropped (brief deliverable) → restored as its own gate stage;
   merge ledgered. [ADOPTED, D-C12]
6. Windows uninstall assertion would test the wrong store (test-tooling LocalMachine import)
   → scope asserts to app-owned stores. [ADOPTED, D-C12; largely mooted by the HTTPS-matrix
   rebuild]
7. PDEATHSIG is thread-scoped → keep only with documented caveat (re-raised drop). [KEPT with
   caveat, D-C8]
8. wdio click-within-700ms spec = flake-prone duplicate → drop; Playwright + unit pin
   suffice. [ADOPTED, D-C15]

## Fresh hostile double audit on plan.md rev 2 → 19 findings (4 HIGH, 12 MED, 3 LOW)

HIGH:
- **H1** 429 authorization_cooldown breaks every DEPLOYED old-SDK dApp (non-403/503 throws
  raw at accelerator-prover.ts:535); the "nothing surfaces" justification only held for the
  new SDK. → 403 + code. [ADOPTED, D-C16, E-6; standing rule: wire changes stay 403/503-shaped]
- **H2** Rollback lever integrity unpinned; post-flip re-capture poisons previous-latest;
  source:previous version semantics undefined. Simpler alternative: drop capture — N-1's own
  release asset is the lever. [ADOPTED (alternative), D-C19, E-9]
- **H3** Future-config protection can't fire: v3 is by-policy unparseable and load_from is
  fail-open, so config_version is never read; the named test would pass against a strawman.
  → lenient {config_version} probe-parse first; non-v2-parseable fixture. [ADOPTED, D-C18, E-8]
- **H4** take_matching "compile-time binding" false on a bare Mutex<Option<>>; no test fails
  on revert. → NEWTYPE with private inner + command-layer mismatch test. [ADOPTED, D-C17, E-7]

MED (all adopted; see D-C17..D-C23): M1 close+re-show race → navigate existing window;
M2 drop-path grandchildren → RAII Drop group-kill; M3 pgid-reuse lock discipline;
M4 Windows wiring test through prove path; M5 call-site testable fns + macOS residual
ledgered; M6 source:previous downstream gating [resolved by single-version design];
M7 burned-stable rule; M8 restart-mid-proof substitute ledgered + bounded L3;
M9 CA-trust-survives-upgrade assertion (brief-mandated); M10 nomenclature re-run at RC;
M11 patch-ahead peer criterion (d); M12 NSIS FindStr /L /C:, spaces-in-path +
$INSTDIR-deleted fixtures, template-ordering resolution.

LOW: L1 contract rows for --latest=false + --clobber ban [ADOPTED]; L2 cooldown eviction =
drop-new [ADOPTED]; L3 risk-7 wording "same tree modulo version manifests" [ADOPTED].

Explicitly verified SOUND by the fresh auditor: 429/cooldown is not a cross-origin oracle
(CORS Any + browsers can't forge Origin; non-browser callers bypass auth); ownership-gated
uninstall adds no attack surface; guard default-on breaks no non-consent flow; promote
verifier chain for the candidate path; publish-job re-run safety on the workflow path;
config read-only mode is not a local-DoS boundary regression; brief completeness for
B2/B5/B6/B7 + DONE block; the named decisive tests (with H3/H4 fixes) are decisive.
