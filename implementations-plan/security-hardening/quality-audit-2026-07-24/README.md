# security-hardening — pre-main-merge codex xhigh quality audit (2026-07-24)

User-requested: several iterations of codex gpt-5.6-sol @ xhigh over the WHOLE security-hardening branch
(vs main, 169 files / ~16.3k insertions) before it merges to main. Each cluster C0–C10 + closeout already
had its own blueprint + audit during the campaign; this is a fresh whole-branch pass for merge confidence.

Codex can't audit 16k lines in one run (infra-kills), so chunked by concern; each chunk = one tight
read-only codex run. Verdicts + folded findings logged here. Loop drives chunk-by-chunk, then re-audits.

## Chunks (round 1)
1. origin-auth + request-safety — core: authorization.rs, server.rs, server/auth.rs, server/host.rs, config.rs
2. bb-cache + Windows-ACL FFI — core: bb.rs, downloader.rs, win_acl.rs, versions.rs
3. updater + certs (crypto) — core: update_manifest.rs, updater_state.rs; src-tauri: certs.rs, updater.rs
4. tauri boundary — src-tauri: commands.rs, windows.rs, crash_recovery.rs, main.rs
5. frontend — frontend-src: authorize.js, settings.js, update-prompt.js, bridge.js
6. scripts — scripts/*.ts (download-bb, copy-bb, bb-version, build-frontend, check-windows-bb-pin)
7. CI — .github/workflows/* (OIDC, SHA-pinning, publish, _aztec-update)
8. infra — infra/tofu/*.tf + infra/rulesets

## Findings log
(appended per chunk below)

### Round 1 · Chunk 1 (origin-auth + request-safety) — REJECT
- **HIGH** — `authorization.rs:315-320` / `server.rs:123-126` / `server/auth.rs:80-87`: same-origin piggyback
  appends UNBOUNDED senders (`req.senders.push`) before any waiter cap → one unapproved origin firing/aborting
  many `/prove` requests accumulates senders (memory/connection DoS), each waiting up to the ~660s backstop.
  FIX: cap senders-per-pending-request; reject (429/deny) past the cap.
- **MEDIUM** — `server/auth.rs:80-85` / `authorization.rs:337-343`: the /prove backstop resolves the active
  request but discards the promoted id (core can't reach the window layer), so if the 60s activation timer
  ALSO failed, the promoted popup is server-active but unraised/unarmed. FIX: propagate promotion (add an
  on_promote callback core→src-tauri, or equivalent).

### Round 1 · Chunk 2 (bb-cache + Windows-ACL FFI) — REJECT
- **HIGH** — `bb.rs:38-39,191-206,229`: cache verify (`verify_cached_bb` rehash) and execute (`Command::spawn`)
  are path-separated → TOCTOU: swap the cached binary between verify and spawn and the replacement gets the
  witness. FIX (hard): verify+exec the same fd (Linux fexecve / hold an open handle), or accept as a
  documented same-user residual. [C6/F-007 pre-existing]
- **MEDIUM/HIGH** — `win_acl.rs:164-256`: `verify_owner_only` checks only the DACL ACEs, NOT the object OWNER.
  A foreign-owned object retains implicit WRITE_DAC and can rewrite the DACL. FIX: verify (and/or set) the
  object owner == current-user SID (OWNER_SECURITY_INFORMATION).
- **COVERAGE GAP** — I named `downloader.rs`/`versions.rs` (don't exist). Re-run chunk 2b on the REAL
  download/decompress/tar-safety + version-policy files (see ls output).

### Round 1 · Chunk 2b (versions/ download + policy) — REJECT
- **HIGH** — `version_policy.rs:67-85,187-196` / `downloader.rs:19-48`: no trusted-pin/downgrade policy — a
  caller can request an OLD vulnerable official bb release; it passes GitHub digest verify + runs. TRIAGE:
  likely bounded by threat model (the bb version is chosen by the LOCAL same-user SDK, not a remote origin) —
  candidate for min-version-floor OR documented-accepted. Needs user threat-model call.
- **HIGH** — `cache_layout.rs:9-13,134-188` / `downloader.rs:28-31`: home-resolution failure falls back to
  CWD with no ownership/privacy check → attacker-writable CWD can preseed a malicious bb + self-authored
  matching marker → verify passes, download skipped. FIX: remove the CWD fallback (fail closed, like F-003).
- **MED** — `downloader.rs:194-212,238-262`: stale-stage reaping + delete-then-rename are unsynchronized →
  concurrent installers can delete each other's active stage. FIX: per-version lock/serialization.

### TRIAGE NOTE
Most findings so far are PRE-EXISTING campaign code (C2/C6/C7), in scope for the whole-branch audit but a
large re-hardening. Split: clear fixes (piggyback cap · CWD-fallback fail-closed · win_acl owner-verify ·
staging lock) vs threat-model/design calls needing the user (version-downgrade policy · verify/exec TOCTOU
same-user · backstop-no-promote rare). Complete chunks 3–8, then present the full triaged set for the
remediation-scope decision.

### Round 1 · Chunk 3 (updater + certs) — REJECT
- **HIGH** — `updater.rs:381-402`: if `record_pending` fails after install(v3), it restarts + releases the
  lock anyway → a still-running v1 can pass the unchanged floor and install signed v2 over v3. FIX: on
  record_pending failure, don't release/restart into a lowered-floor state. [C4/F-004]
- **HIGH** — `certs.rs:187-225`: migration deletes `ca.key` but retains the trusted CA anchor → a
  pre-migration copied/backed-up key still mints trusted certs. TRIAGE: rotating the CA breaks existing
  trust (user re-trust) — threat-model call. [F-016]
- **MED** — `updater.rs:298-355`: size cap checked AFTER buffering the body → OOM before rejection. FIX:
  bound the stream during download.
- **MED** — `updater.rs:210-227`: unsigned multi-GB feed `notes`/`platforms` buffered+parsed before manifest
  verify → periodic-check DoS. FIX: cap feed response size pre-parse.
- **MED** — `certs.rs:71-353`: cert rotation non-transactional (leaf cert replaced before key on crash) →
  persistent mismatch accepted by presence/expiry check. FIX: transactional swap or validity-check the pair.
- Codex confirmed SOUND: sig envelope, version→artifact binding, VerifiedUpdate, localhost-SAN, TLS params,
  keyless-CA-on-disk.

## PLAN: complete chunks 4–8 (tauri · frontend · scripts · CI · infra), THEN present the full triaged
## findings to the user for the remediation-scope + threat-model decisions before any large re-hardening.

### Round 1 · Chunk 4 (tauri boundary) — REJECT  [⚠ several in MY closeout code]
- **BLOCKING (my C8 bug)** — `commands.rs:83-90` / `crash_recovery.rs:84-99`: enable_transaction rollback runs
  `disable_crash_recovery()` unconditionally even when prior_enabled=true → a transient arm failure DELETES
  pre-existing recovery, downgrading a fully-enabled install to launcher-only. FIX: on prior_enabled, do NOT
  disarm on rollback (restore prior fully).
- **BLOCKING** — `commands.rs:92-95` / `main.rs:334-336`: set_autostart(false) + Quit discard
  `disable_crash_recovery()==false` → deletion-failure reported as success; surviving task relaunches. FIX:
  surface/propagate the confirmed-disarm bool.
- **HIGH** — `commands.rs:52,83` / `main.rs:516`: `is_enabled().unwrap_or(false)` → I/O error treated as
  disabled → rollback may disable a real launcher / startup skips rearm. FIX: treat unknown conservatively.
- **MED (my C9 arbiter race)** — `windows.rs:132-153` / `commands.rs:236-242`: A can promote B before B's
  window is built → arm_active_popup finds no window, B builds stale unfocused/non-topmost. FIX: order
  build-before-promote, or re-raise on build.
- **BLOCKING** — `crash_recovery.rs:277-288` / `main.rs:238-256`: Linux `systemctl enable` doesn't START the
  unit; .desktop + systemd both launch, no dup-instance exit → C8/F-010 supervision design. TRIAGE: design.

### Round 1 · Chunk 5 (frontend) — REJECT  (no XSS; textContent used)
- **MED** — `update-prompt.js:3-6`: versions from URL params, not backend-confirmed → fake-update display.
  FIX: route through a server-authoritative pending-update (mirror the authorize get_pending_auth fix). [pre-existing]
- **MED (my C9)** — `authorize.js`: inactive-state + click-guard cover Allow/Deny but NOT the Remember
  checkbox → stolen click pre-arms persistent auth. FIX: disable+guard Remember while inactive.
- **MED (my C9)** — `authorize.js`/`bridge.js`: `decided` stops the poll before respond_auth succeeds; on
  failure, buttons re-enable but poll never resumes → stale request falsely actionable. FIX: resume poll on
  failure / don't latch decided until success.
- **LOW/MED (my C9)** — `authorize.js`: overlapping polls — a delayed older active:false can overwrite a
  newer active:true. FIX: sequence/guard stale poll results.

### Round 1 · Chunk 6 (build/prebuild scripts) — REJECT  (mostly BUILD-TIME → lower runtime severity)
- copy-bb.ts (C7 build): `:163-170` arrayBuffer before 64MiB check (chunked→OOM); `:175-191` verify→extract
  TOCTOU (same-user swap of the temp archive); `:191-201` no expanded-size/member-type bound (gzip bomb /
  symlink bb.exe followed); `:250` bare `xattr` shell exec (planted-PATH / shell-subst). Threat = build-machine.
- **build-frontend.ts (my closeout)**: `:41-55` manifest omits build-recipe/tool-version → bundler-opt change
  without source change passes the SHA guard; `:68-103` hash-time source-swap race. Threat = build-machine.
- copy-bb resolver had NO provenance fail-open. download-bb.ts/check-windows-bb-pin.ts gone at HEAD.

### Round 1 · Chunk 7 (release/publish CI) — REJECT  (all workflow_dispatch-gated → insider/CI threat)
- release-accelerator.yml: `:40-53` multiline-version → $GITHUB_OUTPUT injection; `:734-740` preflight-fail
  treated as skipped→acceptable (release created/deleted before AWS auth); `:109` auth_probe=true doesn't stop
  build/sign/notarize/feed-sign; `:601` existing tag accepted without verifying it points to github.sha.
- _publish-sdk.yml: `:148` tag create/push failures ignored → gh release on stale/auto-created tag (provenance).
- release/_publish-sdk/publish-testnet/aztec-stable `:4`: unrestricted dispatch refs run unreviewed branch code
  with signing keys / NPM_TOKEN / App key exposed (no protected environment).
- INTACT: no mutable third-party action refs (F-015 holds); landing/playground --delete scoped (F-005 holds).

### Round 1 · Chunk 8 (infra/OIDC/ruleset) — CONDITIONAL APPROVE
- **HIGH (already tracked)** — `iam.tf:263,283`: legacy `aws_iam_role.ci` checks aud+sub but NOT `workflow`,
  grants bucket-wide put/delete → any main-branch id-token workflow can assume it + clobber latest.json /
  sibling objects, bypassing the 3 new roles. This IS the documented F-005 human-gated retirement (C5-runbook
  R6: retire legacy role after cutover). NOT new. Infra otherwise SOUND.

## ROUND 1 SYNTHESIS (all 8 chunks) — triage by ACTUAL threat model
**A. Real RUNTIME bugs in the CLOSEOUT I just shipped (fix now — mine):**
  C8: destructive re-enable rollback (disarm on prior_enabled); disarm-failure swallowed (disable/quit);
  is_enabled()→false-on-unknown. C9 arbiter: promote-before-build race. C9 frontend: Remember unguarded;
  decided-stops-poll-before-success; poll-overlap. win_acl (F-003 closeout): owner-SID not verified.
**B. Real runtime findings in PRE-EXISTING campaign code (fixable; scope expansion):**
  unbounded piggyback senders (C2 DoS); CWD cache fallback fail-open (C6); updater rollback-race (C4);
  concurrent staging (C6); streaming size-cap + feed-DoS (C4); cert-rotation non-atomic (F-016);
  update-prompt unverified version.
**C. Threat-model / design calls (need USER decision — fixes have trade-offs / out of stated model):**
  version-downgrade policy (bb version is local-SDK-driven); cache verify/exec TOCTOU (same-user, hard);
  legacy-CA anchor retained (rotating breaks trust); Linux systemd supervision effectiveness.
**D. Build-time / dispatch-gated (different threat model: build-machine or repo-write compromise):**
  copy-bb build-time (buffering/TOCTOU/gzip-bomb/xattr-PATH); build-frontend manifest guard gaps; release/
  publish CI (output-injection, auth_probe, tag≠sha, dispatch-ref secrets); legacy-role (F-005 tracked).
