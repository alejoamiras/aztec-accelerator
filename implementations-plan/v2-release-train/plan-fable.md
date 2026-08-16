# plan-fable.md — fable planning leg (verbatim final report, 2026-08-16)

Leg run as a Plan subagent on Fable; verified load-bearing recon claims against code before
drafting. Notable: caught rev-0 recon error — `latest.json` IS stored as a GitHub release asset
on every stable release (FILES glob includes it), which became the backbone of the rollback
design.

## 1. Cohort boundaries + order

Accept D-R3's default: B2 → (B3 | B5 | B7) → B6 → B4-last, with two sharpenings: (a) F16
(landing prerelease filter) assigned to B6 — release mechanics, must be on main before
2.0.0-rc.1 or the soak exposes the RC to real visitors; (b) B7's files are disjoint from B3/B5
so the parallel wave is conflict-free; B6 after the wave because it rewrites
release-accelerator.yml which B4's gate dispatches against. Each cohort: one worktree, one
plan, one stacked PR.

## 2. Per-cohort design (positions on F1–F17)

### B2 (1–2d)
Guard default-on in `wireButton` (one change; settings.js bypasses wireButton so cannot
regress). F7: export `rearmClickGuard`; `wasActive` latch, rearm only on false→true. F8:
version-echo, TYPE-ENFORCED — `PendingUpdate::take_matching(&displayed)` replaces the blind
`take()` (compile-time wiring proof beats mutation testing); on mismatch log SECURITY, close,
re-show with pending version so the user isn't stranded until the next 12h poll. F9: deny
cooldown recorded in resolve/resolve_active (single choke point), checked after `is_approved`
in server/auth.rs; const beside MAX_PENDING_ORIGINS, injectable Duration; prune on insert;
fairness DEFER (AUTH_QUEUE_BACKSTOP's 660s bound assumes strict FIFO). settings.js guard
DEFER — tray-initiated window, remote origin cannot pop it under the cursor.

Tests (5): parameterized Playwright guard spec (200ms override, immediate-click absent /
waited-click fires); authorize rearm-on-promotion spec; take_matching unit (match/mismatch/
retained); update-prompt payload carries URL version; arbiter cooldown test (deny → immediate
re-request denied, no new pending → after test-cooldown popup again).

### B3 (3–5d)
F4 cap-and-continue (fail-closed is wrong: stderr volume is not an integrity signal; aborting
a valid proof over chatty diagnostics is self-inflicted DoS). F5 validate inside bb::prove
(new ProveError variant buys nothing). F6: Unix process_group(0) + registry (StatusGuard
idiom) + `terminate_inflight()` group-SIGKILL (also in the timeout branch — kill_on_drop only
reaps the direct child); Windows Job Object KILL_ON_JOB_CLOSE in core (one static handle, every
bb child assigned; OS kills bb on ANY exit incl. NSIS-handoff internal exit(0) and crashes,
zero per-exit-site code). Call sites: tray quit + perform_update explicit-before-no-return.
renew_cert refuse-pattern untouched (renewal optional; quit/update are not). Timeout
injectable via prove_inner. Panic hook: synchronous fs append (WorkerGuard cannot be trusted
to flush before abort) + best-effort tracing. Show Logs: construct in both branches.

Tests (6, serial, fake-bb): stderr-spam ≤64KB; empty/33B/64B proof triple; group-kill incl.
`sleep 300 &` grandchild; Windows job-object cfg test (verify it EXECUTES in windows-build job
log — repo lesson); 100ms injected timeout; updater pre-restart helper unit. Panic sink unit;
abort path ships innocuous. Full restart automation refused.

### B5 (3–5d)
F1 HYBRID and not a compromise: `--prepare-uninstall` (exact --remove-ca-trust shape) wrapping
`prepare_uninstall() -> Report` for all OSes; NSIS POSTUNINSTALL gains inline native cleanup
(schtasks /Delete + /Query re-verify, DeleteRegValue Run, marker deletes) — exe is already
deleted at POSTUNINSTALL and F-05 bars a new one-shot PREUNINSTALL bet. F2 accept+document
[NOTE: overturned in contradiction round — never-resurrect makes self-heal false]. F3: REMOVE
certs/markers/trust; KEEP config.json (with README security note: approved origins survive →
document full-purge path), updater-state.json (anti-downgrade floor), locks. deb/DMG/AppImage:
documented flows (ratified S6). updater-lock check before mutating.

Tests (3): L3 per-OS via CARGO_BIN_EXE (arm→run→gone→run→still-gone; establishes the missing
argv-branch pattern, runs in 3-OS cert-trust legs); NSIS harness seeds real task+Run value →
uninstall removes, update-mode preserves; CA regression green.

### B7 (2–4d)
F13 KEEP exact-pinned deps (pin is load-bearing for vetted-once-frozen-forever; peers with ^
reopen the fresh-window; instanceof risk mitigated by docs + detected by tarball consumer)
[NOTE: superseded by evidence-gated exact-PEERS criteria in consolidation]. F14 one
`AcceleratorHttpError` at the two escape sites only; fallback semantics untouched; README
status→behavior table incl. all four 403 variants; correct SKILL.md. AcceleratorStatus gains
appVersion/apiVersion (additive). F15 MIGRATION.md into `files` + release asset + pack assert.
Extract `node -e` rewrite into `scripts/prepare-sdk-publish.ts` shared by publish AND the
consumer job (else the job tests the wrong package shape). Consumer fixtures: node-tsc
(nodenext) + vite, Node 24. F17 publish-testnet chain after desktop GA; latest moved only by
explicit promote-latest dispatch; document that `^` consumers never auto-receive -revision.N —
latest promotion is the only distribution lever.

Tests (4): mocked-ky error table (400 throws typed, 403 falls back); health-parse carries
versions; public-contract additions; the consumer job itself.

### B6 (2–4d)
F12 split at :981: promote-feed job `needs:[validate,release]`, gated
`is_prerelease=='false' && inputs.promote=='true'` (default true — D-R1 autonomous; false =
publish-without-promote freeze mode); sources signed feed from the `signed-update-feed`
artifact; AWS creds REMOVED from the release job (real secret-scoping win). Rollback lever
`promote-accelerator.yml` (name distinct from SDK's promote-latest.yml): downloads the
release's own latest.json asset (the previous feed IS durably stored — every stable ships it),
verifies with the production verifier, flips, curl-asserts. Three roles: fix-forward promote,
re-point to N-1, idempotent re-promote (the rehearsal). Runbook: replace delete-and-restore
with freeze/fix-forward/re-point; fix stale :28-36 job map + CLAUDE.md together; add the manual
trust-install pre-release checklist that trust_{macos,windows}.rs dangle on. Layer-B honesty:
re-pointing protects not-yet-updated users only. F16: `!r.prerelease` filter in landing.
[NOTE: several elements revised in contradiction round: capture-in-publish, same-workflow mode
instead of second workflow, /releases/latest endpoint, asset counts.]

Validation: actionlint; auth_probe graph dry-run; RC chain validates publish path; idempotent
re-promote validates promote end-to-end with zero feed change.

### B4 (4–7d + soak)
Config migration FIRST via raw serde_json::Value pre-pass (NOT serde alias — recreates the
documented duplicate-field reset edge): safari_support→https_enabled only when absent, strip
legacy key, CONFIG_VERSION=2 and READ it, migrate v1, don't overwrite unknown-future configs.
Rewrite the two drop-pinning tests to pin the migration (owner-ratified reversal). F11 extend
the updater smokes (they already do real installs; a parallel harness duplicates the most
fragile logic); port Windows' pinned-n1 4-point preflight to mac/linux; seed a genuine 1.0.7
legacy profile (safari_support:true, approved origin, speed, auto_update) — reconciling the
scripts' safari_support/https_enabled drift; after update assert version/https/origins/floor/
health/HTTPS-port [NOTE: HTTPS-port assert per-OS-gated after UntrustedSkip verification].
F10: Linux NSS (proven); Windows LocalMachine\Root import (proven pattern; avoids CurrentUser
freeze) [NOTE: overturned — app won't SERVE HTTPS untrusted; residual instead]; macOS
Firefox-NSS via certutil [NOTE: same]. Playground installed-accelerator spec: packed SDK →
fresh playground copy → HTTPS-only → one real browser proof, no WASM fallback. Uninstall leg
appended per OS. Nomenclature sweep = checklist execution, no automation.

## 3. Release execution
Freeze confirmed (automation dormant since May). Version PR 2.0.0-rc.1 → RC dispatch (publish
path only, feed untouched) → full gate suite per RC → ≥2h soak (≥4 green e2e-gate cycles) →
codex clean round → version PR 2.0.0 → stable dispatch (promote default true) [NOTE: revised
to hold→regate→promote-only] → drill = idempotent re-promote → SDK publish-testnet chain →
fresh-dir npm verify → promote-latest → post-release 3-OS live smokes → close-out.
Rollback: pre-promote harmless; post-promote re-point via release-asset feeds; updated clients
fix-forward-only (Layer-B floor); crash-on-start = communicated manual reinstall; never delete.

## 4. Top risks
macOS browser-trust unproven (spike day 1, curl-cacert fallback, residual documented); Windows
Job Object compiles-but-never-runs (verify execution in windows-build log); update-race fix
strands users (re-show on mismatch); gate-suite flake burns RCs (bounded waits, infra-attributed
retries, 3-strikes → codex); -revision.N invisible to ^ consumers (dist-tag policy documented;
latest promotion is the lever).

## 5. Refusals
Queue fairness (invalidates documented 660s bound); settings.js guard (threat model
unreachable); bb supervisor frameworks; full restart-mid-proof automation; PREUNINSTALL hook;
ownership checks in prepare-uninstall [overturned]; --purge / auto-deleting origins; deb
postrm; S3 versioning (feeds durable as release assets); macOS Keychain automation beyond one
spike; SDK error taxonomy beyond one class; independent semver / Trusted Publisher / TUF /
telemetry / B1; nomenclature automation; Mainnet cache LRU, tray error states, single-instance
guard, update-failure UI → post-v2 backlog.

---

## Contradiction-check round contribution (same agent, resumed)

Checked D-C4/D-C7 (consistent as consolidated) and D-C1 vs NSIS-handoff (sound) first; found 8
findings elsewhere: (1) promote-feed permissions contradiction (upload needs contents:write) —
fix: capture in publish job from public CDN; (2) freeze doesn't freeze the landing page — fix:
switch landing to /releases/latest endpoint (promotion-aligned once Latest is marked
post-promote); (3) "full regate on stable" unimplementable with in-run promote — fix: stable =
hold → gate on published assets → promote-only; (4) partial-publish wedge — fix: promote-only
hard-verifies asset URLs + runbook additive-upload recovery; (5) fresh-install never gated
pre-promote (silently dropped brief deliverable) — fix: fresh-install stage per OS leg +
ledger the merge; (6) Windows uninstall assertion would test the wrong store (LocalMachine
import is test tooling) — fix: scope asserts to app-owned stores, separate teardown; (7)
PDEATHSIG is thread-scoped — keep only with documented caveat (re-raised drop-to-residual,
conceded if documented); (8) wdio click-within-700ms spec is a flake-prone duplicate — drop;
Playwright + unit pin suffice.
