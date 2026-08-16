# v2-release-train — decision ledger

Running record of decisions, their sources, rejected alternatives, and unresolved disputes.
Owner-ratified decisions D-R1..D-R5 live in `brief.md` and are binding without re-litigation.

## Phase-0 decisions (pre-plan)

- **D-P0.1 — Aztec target: FROZEN on current (@aztec 5.0.1 axis).** Brief's freeze rule checked
  2026-08-16: no open PRs; aztec-stable automation's last scheduled success was 2026-05-27 and a
  manual dispatch failed 2026-06-11 — nothing newer is "ready and green". Consequence: SDK ships
  as the next `-revision.N` on the existing X.Y.Z. Backlog note: the aztec-stable schedule looks
  dormant since late May — investigate post-v2.
- **D-P0.2 — Clarifying questions are pre-answered by the brief** (owner set success criterion,
  scope, quality bar = production, validation layers, decision delegation, and hardening posture
  in the goal + brief). Recorded here so the deep protocol's Phase 0 is auditable.

## Plan-space decisions (consolidation of 3 legs, rev 1)

Convergent across all three legs (adopted without dispute): guard default-on in `wireButton`;
F7 rearm-on-transition latch; F8 version-echo; F9 cooldown-in/fairness-defer; F4 cap-and-continue
stderr; F5 validate inside bb::prove; renew_cert refuse-pattern kept; sync panic sink; Show Logs
in prod; F1 hybrid uninstall with NSIS-native inline + no PREUNINSTALL; F3 keep user
config/origins/floor; F15 MIGRATION.md shipped+attached; exports rewrite → committed script
shared by CI and publish; tarball consumer fixtures (Node 24); F16 landing prerelease filter;
F17 publish-testnet → promote-latest after desktop GA; runbook+CLAUDE.md fixed together;
fix-forward-only recovery; full gates per RC; soak = repeated gate cycles ≥2h.

- **D-C1 (F6)** bb child death = Unix process_group(0) + group-SIGKILL registry, Windows Job
  Object KILL_ON_JOB_CLOSE, explicit terminate at tray-quit + pre-install (abort update on
  unconfirmed reap; never block quit), Linux PDEATHSIG extra. Source: fable+codex majority.
  Main's kill-registry-only rejected: misses grandchildren + Windows NSIS-handoff exit(0).
- **D-C2 (F2)** Uninstall removes autostart/recovery UNCONDITIONALLY (main+fable): surviving
  copied install self-heals via #429 reconcile on next launch; codex's ownership-aware removal
  rejected as corner-case machinery — DISSENT RECORDED, revisit if a real dual-install report
  ever lands.
- **D-C3 (F13)** DISPUTED → evidence-gated: lean exact-pinned PEERS (codex) because the
  aztec-tracking version scheme semantically means "for Aztec X.Y.Z", README already promises
  peers, and loud conflicts beat silent nested dups; main+fable prefer keep-deps
  (supply-chain-story continuity). B7 cohort task 1 = import audit + both fixture variants;
  the fixture evidence commits the decision in-cohort.
- **D-C4 (F9 wire)** Cooldown response = immediate `OriginDenied` (main+fable), NOT codex's 429:
  denial semantics must keep dApps on the WASM-fallback path; 429 would surface a typed error
  and change dApp behavior for a user-denial situation.
- **D-C5 (F12)** Promote/recovery live INSIDE release-accelerator.yml as dispatch modes
  (codex's IAM-trust argument: OIDC role trust may be workflow-bound and is unverifiable from
  the repo); previous-latest.json captured at publish (codex) + release-asset sourcing (fable's
  correction that latest.json already ships as a stable-release asset). Freeze = promote:false
  input only. GitHub-Latest badge marked post-promote in a separate contents:write job. Stable
  re-dispatch refuses if release exists; RC keeps clean-slate.
- **D-C6 (F10/F11)** Combined per-OS installed-product gates EXTENDING the updater smokes
  (codex+fable) instead of a parallel harness; browser-trust matrix: Linux NSS+Chromium,
  Windows LocalMachine\Root+Chromium, macOS Firefox-NSS (keychain spike as fallback, curl
  --cacert as floor); app's own interactive trust dialogs stay a manual-runbook residual on
  mac/win (codex concurrence required at audit).
- **D-C7 (F14)** Fallback table refined via server error CODES: recognized
  capacity/denial/version codes (403-family incl. version_not_allowed→"version-mismatch",
  408/413/429/503, 500 w/ download_failed|prove_failed, malformed-2xx, network) → WASM
  fallback; caller-bug codes (400 invalid_version/invalid_origin) + unrecognized → typed
  `AcceleratorHttpError`. Preserves the original "don't mask misconfig" rationale for
  unrecognized responses. take_matching deletes the blind-take path (compile-time consent
  binding, fable).

## Contradiction-check round (rev 1 → rev 2): 20 findings (codex 12, fable 8), dispositions

- **D-C2 FLIPPED — ownership-aware uninstall removal (codex was right).** The "self-heal"
  rationale was FALSE: autostart.rs:17-18/:1563 — Absent entries are NEVER resurrected
  (verified in code). CLI uses #429 probes (Absent ⇒ success; foreign/unreadable ⇒ leave +
  report); NSIS gates deletes on $INSTDIR match. E-1 in the error log below.
- **D-C4 RESOLVED codex's way — new `AuthorizationCooldown` → 429 `authorization_cooldown`.**
  D-C7's own table makes recognized 429 fall back to WASM, dissolving the "would surface"
  objection, and OriginDenied would have emitted a false `denied` phase for a cached decision.
  E-2 below.
- **D-C8 (fable dissent, kept):** Linux PDEATHSIG retained with documented thread-scope caveat;
  group-kill/Job Object primary. Fable would drop to residual; benefit judged worth one caveated
  line.
- **D-C9:** previous-latest.json captured + uploaded by the PUBLISH job (contents:write), from
  the public CDN pre-flip; promote job stays id-token+read (codex c-4 + fable c-1 — rev-1's
  shape was a permissions impossibility). Asset gates 16 RC / 18 stable (codex c-5).
- **D-C10:** Total release immutability — RC delete-recreate removed (codex c-7); failed
  publish ⇒ rc.N+1; partial-publish recovery = runbook additive-upload + promote-only hard
  asset-URL pre-flight (fable c-4). Codex's `resume-existing` mode REJECTED (extra machinery).
- **D-C11:** Stable flow = publish-with-hold → regate on published bytes → promote-only
  (fable c-3 — rev-1's "full regate on stable" was unimplementable with in-run promote).
  verify-live-feed/mark-Latest/bump-source move to the promotion path. promote:false = "hold";
  drill = promote-only source:previous → restore (codex c-8). Landing switches to
  /releases/latest endpoint (fable c-2; subsumes F16).
- **D-C12:** B4 trust matrix corrected (codex c-3, VERIFIED: classify_launch_https →
  UntrustedSkip): Linux = full HTTPS browser proof via the app's own NSS store; macOS =
  keychain spike else residual; Windows = residual (widening the app trust predicate for
  testability REFUSED). Fresh-install stage restored as its own gate stage (fable c-5 — rev-1
  silently merged a brief deliverable). Full package removal in uninstall legs (codex c-12).
  Uninstall assertions scoped to app-owned stores (fable c-6).
- **D-C13:** F13 acceptance criteria predeclared (codex c-10): exact-host green + npm ls
  singleton; conflicting-host ERESOLVE (npm version + --strict-peer-deps pinned); host-absent
  is NOT a criterion (npm auto-installs peers). Any ambiguity ⇒ keep-deps.
- **D-C14:** Future-schema configs protected at the SAVE path (read-only flag), not just load
  (codex c-11). Version PRs lockstep all four manifests incl. core (codex c-9) + B6
  declaration-equality gate.
- **D-C15:** wdio real-timing guard spec dropped (fable c-8, flake-prone duplicate);
  DEFAULT_GUARD_MS pinned by unit test instead.

## Error log (my consolidation errors caught by auditors)

- **E-1:** I asserted (and fable repeated) that a surviving copied install "self-heals" after
  unconditional autostart removal — false; never-resurrect is a documented invariant. Caught by
  codex c-1, verified in code.
- **E-2:** I created a D-C4/D-C7 internal contradiction (rejected 429 for "would surface"
  while making 429 a fallback code). Caught by codex c-2.
- **E-3:** I gave promote-feed an asset-upload duty its own declared permissions forbid.
  Caught by codex c-4 + fable c-1 independently.
- **E-4:** I promised "full regate on the stable build" in a flow where promote happens in-run
  before any external gate could run. Caught by fable c-3.
- **E-5:** rev-1's B4 browser-trust matrix could not make the app serve HTTPS on mac/win at
  all (UntrustedSkip) — the "HTTPS proof on 3 OSes" claim was unimplementable as written.
  Caught by codex c-3, verified in code.

## Disputes still open for the double audit

- D-C3/D-C13 peers-vs-deps: evidence-gated; auditors asked to attack BOTH outcomes and the
  criteria themselves.
- D-C8 PDEATHSIG caveat (fable would drop it).
- Main's original F6 minimalism dissent (outvoted; Job Object stands).
