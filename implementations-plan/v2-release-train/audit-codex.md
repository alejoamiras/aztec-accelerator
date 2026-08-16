# audit-codex.md — codex audit trail, v2-release-train

Planning-leg + contradiction + double-audit session: `01a00cc4-0918-7603-94ed-12851a926021`.
Final fresh-context pass: separate session (verdict recorded at the bottom when it lands).
Round 1 (independent planning leg) full text: `plan-codex.md`.

## Round 2 — contradiction-check on plan.md rev 1 → 12 findings

1. **D-C2's self-healing rationale is false.** After unconditional removal, the surviving copy
   sees `StoredTarget::Absent`; reconciliation deliberately never recreates absent autostart.
   Fix: ownership-aware removal; absent = idempotent success. [ADOPTED — verified in code,
   D-C2 flipped, E-1]
2. **D-C4 contradicts D-C7.** D-C4 rejected 429 as "would surface", but D-C7 makes recognized
   429 fall back. Fix: AuthorizationCooldown → 429. [Adopted in rev 2; SUPERSEDED in rev 3 by
   fable-fresh H1 → 403 + code (old-SDK fleet compat), D-C16]
3. **B4's mac/win trust matrix cannot make the app SERVE HTTPS.** LocalMachine\Root / Firefox
   NSS don't satisfy the app's own CurrentUser/login-keychain predicates. [ADOPTED — verified
   (UntrustedSkip); B4 matrix rebuilt, E-5; residual dispute packaged for final pass]
4. **promote-feed can't attach previous-latest.json with contents:read.** [ADOPTED via
   capture-in-publish in rev 2; capture DROPPED entirely in rev 3 (D-C19)]
5. **Asset counts wrong per channel.** 16 RC / 18 stable. [Adopted; now 16/17 after capture
   drop]
6. **Stable refusal dead-ends a recoverable partial run.** Proposed resume-existing mode.
   [REJECTED as extra machinery; replaced by additive-repair runbook + promote-only full
   pre-flight, D-C10]
7. **RC clean-slate delete violates the no-delete rule.** Every release immutable; failed RC ⇒
   rc.N+1. [ADOPTED, D-C10]
8. **Recovery terminology/drill overclaim.** hold vs freeze; drill must exercise
   previous-asset re-point. [ADOPTED: hold semantics; staging rehearsal + live drill, D-C22]
9. **RC version PR contradicts declaration-equality.** Lockstep all manifests. [ADOPTED,
   D-C14]
10. **Host-absent peer fixture can't decide F13** (npm auto-installs peers). Criteria:
    exact-host green, conflicting-host ERESOLVE, npm ls singleton. [ADOPTED, D-C13]
11. **Future-version configs not protected at the save path.** [ADOPTED rev 2; deepened rev 3
    by fable-fresh H3 probe-parse, D-C18]
12. **B4 mac/linux "uninstall" leg doesn't uninstall the package.** [ADOPTED — full package
    removal, D-C12]

## Round 3 — double audit on plan.md rev 2 → 12 findings (1 BLOCKER)

1. **BLOCKER: promote-only source:previous has two incompatible versions** (container 2.0.0 vs
   feed 1.0.7) driving downstream jobs wrong. [RESOLVED BY DESIGN in rev 3: single-version
   promote-only + feed_version==input + bump_source flag + mark-Latest re-badges the promoted
   release, D-C19/D-C22]
2. **rev 2 no longer fulfills B4's 3-OS HTTPS proof.** Demanded manual evidenced proof or
   owner exception. [OPEN DISPUTE — residual package + brief-clause invocation put to the
   final pass for ruling]
3. **Job Object spawn-before-assignment race.** Suspended-spawn proposed. [PARTIAL: assign+
   IsProcessInJob+fail-prove-if-unassigned; suspended-spawn refused (bb is our marker-verified
   sidecar; window documented), D-C20]
4. **Previous-feed snapshot proves authenticity, not currency.** [DISSOLVED by capture drop,
   D-C19]
5. **NSIS substring ownership is not ownership.** Exact parsed-token compare + deceptive
   fixtures. [ADOPTED + fable M12 hardenings, D-C21]
6. **Foreign-install protection stops at autostart** (certs/trust still removed). [ADOPTED —
   foreign ⇒ skip shared-state removal too, D-C21]
7. **Additive repair bypasses the asset gate.** Append-only policy; no --clobber; full
   verifier after repair. [ADOPTED, D-C22]
8. **N+1 rehearsal unrehearsed.** [ADOPTED via staging-key rehearsal + live drill, D-C22]
9. **Nomenclature checklist unowned at release time.** [ADOPTED — RC-tree re-run +
   signoff artifact into the release review, D-C23]
10. **Two B3 mutation claims not decisive** (helper-level tests). [ADOPTED — tests through
    real prove_inner path + test-only observations, D-C20]
11. **Peer gate must pin npm exactly.** [ADOPTED, D-C23]
12. **Stable runs the expensive gate twice.** [ADOPTED — once, against held published bytes,
    D-C22]

## Final fresh-context pass

(to be appended when the verdict lands)
