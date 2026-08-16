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

## Disputes (open for contradiction-check + audits)

- D-C2: codex dissent on unconditional autostart removal.
- D-C3: peers-vs-deps awaiting fixture evidence; auditors asked to attack BOTH outcomes.
- D-C4: codex dissent on cooldown response code.
- Main's dissent on F6 scope (wanted Job-Object-free minimalism) — recorded, outvoted.
