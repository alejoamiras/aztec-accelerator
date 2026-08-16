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

## Double-audit round (rev 2 → rev 3): codex 12 (1 BLOCKER) + fresh-fable 19 (4 HIGH)

- **D-C16 (fable H1, supersedes the rev-2 D-C4 resolution):** cooldown = **403 +
  `authorization_cooldown` code**, not 429. The 429 rationale only held for the NEW SDK's
  table; deployed old SDKs hard-throw on non-403/503, so 429 would break every existing dApp
  on desktop auto-update. 403 keeps old SDKs on denied→WASM; new SDK keys off the body code
  (no fresh `denied` phase). Standing rule extracted: **wire changes must stay 403/503-shaped
  for the deployed fleet.**
- **D-C17 (fable H4/M1):** PendingUpdate becomes a NEWTYPE (private inner; take_matching sole
  extractor) — the compile-time claim is now real; + command-layer mismatch test. Mismatch
  recovery = navigate the existing window (open_or_focus dedup makes close+re-show droppable).
- **D-C18 (fable H3):** future-config protection moved to a lenient `{config_version}`
  probe-parse BEFORE the fail-open full parse (v3 configs are by-policy unparseable by v2);
  test fixture must be non-v2-parseable.
- **D-C19 (fable H2-alt, adopted over codex's capture+polling):** previous-feed CAPTURE
  DROPPED. Verified: accelerator-v1.0.7 ships latest.json as a release asset — every stable
  does. Rollback = promote-only <older-version> using that release's own signed feed. Kills
  codex-r2 BLOCKER #1 (version confusion: promote-only is single-version; feed_version ==
  input enforced), #4 (staleness), and the H2 poisoning class. Stable asset gate = 17, RC 16.
- **D-C20 (codex r2 #3 + fable M2/M3/M4/M5):** containment hardened — RAII Drop = group-kill
  (client-disconnect drop covers grandchildren); reap/kill lock discipline (no pgid-reuse
  kill); Windows assign+IsProcessInJob+fail-prove-if-unassigned (suspended-spawn REFUSED —
  bb is our marker-verified sidecar; window documented); wiring tests through the REAL prove
  spawn path on both OSes; call-site fns testable; **macOS crash-orphan explicitly a
  residual** (bb exits naturally, F-08a reaps, PROVE_TIMEOUT bounds app-alive).
- **D-C21 (codex r2 #5/#6 + fable M12):** NSIS ownership = exact canonicalized token compare
  (Run value quoted-exe token; task XML <Command> unescaped), FindStr /L /C: only; harness
  fixtures incl. spaces-in-path and $INSTDIR-deleted; template-ordering question resolved
  in-cohort against tauri-bundler 2.8.1. Foreign detection ALSO skips shared certs/trust
  removal.
- **D-C22 (codex r2 #7/#8/#12 + fable M6/M7 + L1/L3):** append-only release policy
  (--clobber banned, --latest=false pinned, contract-tested); burned-stable rule (failed
  held regate ⇒ fix-forward X.Y.Z+1, never re-dispatch); single full 3-OS journey once
  against held published bytes; staging-key rehearsal pre-GA + live 1.0.7→2.0.0 drill
  post-GA; bump_source only on organic GA promote; mark-Latest re-badges the PROMOTED
  release (rollback keeps landing/feed consistent).
- **D-C23 (fable M8/M9/M10 + codex r2 #9/#11):** bounded restart-mid-proof L3 replaces the
  dropped full-app automation (ledgered substitute); CA-trust-survives-upgrade assertion
  added where seedable (brief-mandated; mac/win residual ledgered); nomenclature checklist
  re-run against the RC tree as a signoff artifact attached to the codex release review;
  exact npm version pinned for peer evidence + publish.
- **D-C24 (fable M11):** peer criterion (d) patch-ahead host, predeclared verdict "hard
  refusal is the contract" — ledgered before adoption; surprise ⇒ keep-deps.
- **D-C15 note:** cooldown map eviction = drop-new at cap (fable L2).

## Error log (continued)

- **E-6:** rev 2's 429 cooldown would have broken every deployed dApp — I evaluated the wire
  change only against the NEW SDK's table (fable H1).
- **E-7:** rev 2 claimed a compile-time consent binding that a std `Option::take` trivially
  bypasses; no named test failed on revert (fable H4).
- **E-8:** rev 2's future-config guard could never fire (v3 is by-policy unparseable; load is
  fail-open) and its named test would have passed against a strawman fixture (fable H3).
- **E-9:** rev 2's previous-feed capture had no version pinning and a post-flip re-capture
  poisoning path; and promote-only source:previous conflated release vs feed version
  (codex r2 #1 BLOCKER + #4, fable H2).

## Final fresh-context codex pass (rev 3 → rev 4): 7 findings, 2 BLOCKERs, 2 trims + a RULING

Session 01a00cef-d10b-7e01-85bb-9b81a80e02fe. All folded; codex was right on every point.

- **RULING on the HTTPS dispute: codex REFUSED the residual — ACCEPTED.** UntrustedSkip is
  affirmative proof the packaged app did not serve HTTPS, so the rev-3 package proved
  components, not the composed path. **D-C25**: B4 restructured to non-interactive MANDATORY
  trust spikes reaching the app's OWN store on all 3 OSes (Linux NSS; macOS login-keychain +
  set-key-partition-list; Windows raw registry write to HKCU\...\SystemCertificates\Root\
  Certificates\<thumbprint>, which bypasses the CryptoAPI prompt that froze CI) → full
  composed browser proof (packed SDK → installed app → HTTPS → native bb, downgrade+WASM
  disabled) per OS. Genuine spike failure ⇒ STOP-and-surface to owner before RC dispatch —
  NOT a residual, NOT a manual ceremony. Resolves BLOCKER 1 (no human in the autonomous path).
  Codex's stated evidence bar is now the literal gate assertion.
- **D-C26 (BLOCKER 2):** stable gates as a GitHub **DRAFT** (no tag, non-public assets); pass
  ⇒ publish draft (atomic tag creation); fail ⇒ delete draft + re-dispatch same 2.0.0. Keeps
  failed bytes private (codex: `--latest=false` is still downloadable) and stays inside the
  goal's 2.0.0-only authorization. A bad PUBLISHED 2.0.0 ⇒ 2.0.1 = STOP-and-surface (exceeds
  authorization; the goal grants only rc.N + 2.0.0). E-11.
- **D-C27 (finding 3):** B5 F1 flipped to brief-literal — PREUNINSTALL invokes
  `--prepare-uninstall` (+ POSTUNINSTALL native fallback). The newer binding brief overrides
  the F-05 precedent, and B4's installed-gate + NSIS harness dissolve F-05's untestability
  objection. Cohort task 0: confirm PREUNINSTALL exists in the tauri-bundler 2.8.1 template;
  absent ⇒ owner-deviation STOP. Reverses part of D-C21's "PREUNINSTALL refused".
- **D-C28 (finding 4):** config future-schema protection type-bound via a `PersistCapability`
  token minted only by a current/migratable load and required by EVERY save path — not
  "convention that saves are rejected" (which repeated the E-7/E-8 enforcement mistake). E-10.
- **D-C29 (finding 5):** peer evidence tests DEFAULT `npm install` (not just
  `--strict-peer-deps`, which is opt-in and proves nothing about real consumers); keep-deps
  unless DEFAULT npm refuses the duplicate. Revises D-C13/D-C24. Likely outcome: keep-deps.
- **D-C30 (finding 6):** landing derives its download version from the SIGNED S3 FEED, not the
  GitHub Latest badge, so a mark-Latest partial failure can't desync landing from the updater;
  PushNotification fires immediately after the S3 flip. Supersedes D-C11's "/releases/latest
  endpoint" (which was ambiguously the GitHub API).
- **D-C31 (trims):** (a) post-GA LIVE prod rollback drill CUT — the staging-key rehearsal now
  exercises BOTH candidate and previous sourcing without ever serving 1.0.7 to real users;
  (b) Linux PDEATHSIG CUT (thread-scoped, redundant, two auditors converged — overrides D-C8).
- **Cooldown eviction:** evict-oldest, not drop-new (finding 7 reverses fable L2 — drop-new
  lets a flooder disable cooldown for others).

## Error log (continued)

- **E-10:** rev 3's "ALL saves rejected" for future configs was enforcement-by-convention —
  the same class as E-7 (take_matching) and E-8 (config guard). A std `save()` stayed callable.
  Caught by the final pass #4. Fixed by the PersistCapability newtype.
- **E-11:** rev 3's burned-stable → 2.0.1 recovery silently exceeded the goal's authorization
  (rc.N + 2.0.0 only) and relied on "`--latest=false` = invisible" which is false (still
  downloadable). Caught by final-pass BLOCKER 2. Fixed by draft-gating.
- **E-12:** rev 3's HTTPS residual tried to substitute component evidence for the composed
  security path the brief mandates; UntrustedSkip actually proves the negative. Caught by the
  final-pass RULING. Fixed by non-interactive composed-path spikes + owner escalation.
- Meta-pattern across E-7/E-8/E-10: I repeatedly asserted an invariant as prose that the type
  system didn't enforce. Rev 4 standing rule: **any "cannot happen" security invariant must be
  type-bound (newtype/capability) with a compile-or-mutation proof, never a convention.**

## Remaining EXECUTION-TIME verifications (not plan defects; owner-escalation paths defined)

- B4 trust spikes landing non-interactively per OS (else STOP-and-surface before RC dispatch).
- B5 PREUNINSTALL macro existing in the actual bundler template (else owner-deviation STOP).
- D-C29 peer evidence outcome (default-npm behavior decides peers-vs-deps in-cohort).
- D-C8 PDEATHSIG now CUT (dispute closed).
