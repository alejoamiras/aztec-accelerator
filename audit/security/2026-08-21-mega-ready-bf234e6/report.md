# Security audit — mega-ready-audit run

**Repo:** `alejoamiras/aztec-accelerator` @ `bf234e6` (worktree `mega-ready-audit`)
**Date:** 2026-08-21 · **Model:** ox-alpha solo (+1 subagent batch that returned; 4 empty returns → manual passes)
**Scope:** GUI app (`src-tauri`, `core`, `frontend-src`). Headless server/SDK/CI excluded per owner.
**Method:** two-stage. Stage 1 re-verified EVERY prior finding as an unverified claim (fix diffs +
current source, evidence-cited). Stage 2 fresh adversarial read of all 9 clusters, weighted to the
unaudited v2 delta (#434–#442 fix arcs, #446/#447/#448/#451/#455 v2 train).

## Headline

**No new findings above Low.** Every claimed fix from runs 2026-07-09 and 2026-07-31 that was
re-verifiable at source held up — including the four fixes written by other models that had never
been independently re-reviewed. Two "open" findings from the 2026-07-31 report are actually CLOSED
by later work (#446): F-07 (prompt flooding) and F-10 (click-steal guard opt-outs). The prior
reports' quality is real; nothing needed refuting as false.

## Stage 1 — claim re-verification (details in ../../implementations-plan/mega-ready-audit/ledger.md)

| Finding | Verdict |
|---|---|
| 07-31 F-01 (server side) | CONFIRMED-FIXED surface facts; SDK↔server identity contract remains open by design (old F-001) |
| 07-31 F-02 | CONFIRMED-FIXED — structural loopback name-constraint validation + vetted random-name copy; same-UID swap window honestly documented as risk acceptance |
| 07-31 F-03 | CONFIRMED-FIXED all sinks — kernel-mediated owner identity (`owner.rs`), capped probe reads, bind-owned floor gate |
| 07-31 F-04 | CONFIRMED-FIXED — floor clamp/repair + pending expiry kills the lockout shape |
| 07-31 F-05 | CONFIRMED-FIXED — fail-closed removal post-condition readbacks on both backends |
| 07-31 F-06 | CONFIRMED-FIXED — denylist-first, digest-pinned downloads (64MiB/512MiB caps), lease-guarded eviction incl. pre-permit lease |
| 07-31 F-07 | **CLOSED-BY-LATER-WORK** (#446: 30s deny cooldown, MAX_PENDING_ORIGINS=10, activation-relative auto-deny); starvation claim now overstated |
| 07-31 F-08 | CONFIRMED-FIXED both facets (startup reaper w/ 24h floor + bind-win gating; concurrent 64KiB stderr drain; proof validation; full tree-kill matrix) |
| 07-31 F-09 | STILL-OPEN — deferral sound (closure would reintroduce F-04 lockout; needs upstream revocation story) |
| 07-31 F-10 | **CLOSED-BY-LATER-WORK** (#446: guard DEFAULT-ON, zero opt-outs on consequential buttons) |
| 07-31 F-11 | CONFIRMED-FIXED body facet (50MiB + 30s deadline + inflight shed + permit placement); disconnect-abort rides bb Guard drop chain; phase-ordering facet superseded by A1 ordering |
| 07-31 F-12/F-13 | CONFIRMED-FIXED (fuse.* mount proof w/ kernel mnt_id; hardcoded System32-first resolvers) |
| 07-09 F-003/F-010/F-011/F-016 | CONFIRMED-FIXED (0700/0600-at-syscall + Windows DACLs; systemd serializer fail-closed; trailing-dot rejection; no ca.key write path + Zeroizing) |
| #345 | Re-assessed: trigger = control of the SIGNED URL's content (release-infra/CDN compromise) — no Ed25519 key needed to force the buffering DoS; integrity unaffected (minisign rejects post-buffer). Keep as low-priority hardening |

## Stage 2 — fresh cluster audit (coverage + result)

| Cluster | Read | Result |
|---|---|---|
| C1 server surface | server.rs (router/CORS/host-guard layering/health tiering), host guard position, body limits consistent | Clean. CORS Any is safe behind origin auth; health tiering correct; backstop bounded (660s) |
| C2 consent/auth | auth.rs, authorization.rs cooldown/queue caps, commands.rs respond_auth/get_pending_auth label binding (SHA-256 128-bit labels), windows.rs build-then-arm arbiter, nav-guard tests | Clean. Server-side resolve_active arbiter; wrong-window resolution impossible without SHA collision |
| C3 certs/trust | validate_ca_profile structure, vetted_copy_of, rotate staging/fail-closed, trust backends' verify+remove | Clean (residual documented in-code) |
| C4 updater chain | updater.rs FULL: Layer A/B, proof-carrying VerifiedUpdate, install-time TOCTOU re-check under lock, quiesce+confirm before NSIS, marker transaction guards, legacy-exe prune scoping | Clean. Extensively defended; every abort path unwinds correctly |
| C5 bb supply chain | release_metadata.rs URL construction (charset-restricted AztecVersion ⇒ no traversal), digest fetch fail-closed, downloader stream/decompress caps, cache_layout verify_cached_bb | Clean. SEC-02 circular-trust residual honestly documented (#343) |
| C6 config/win_acl | load_with_cap_from fail-closed stages, migrate_value, PersistCapability typestate, acquire_config_write_lock (flock/LockFileEx) across stage→recheck→rename, win_acl.rs line-by-line | Sound. Lows below |
| C7 uninstall/lifecycle | uninstall.rs three-state oracle, task-local recovery check, autostart lock span | Clean; never-armed-copy residual documented |
| C8 frontend/IPC | windows.rs is_local_asset_url pinned tests, bridge.js default-on guard, per-page capabilities + tauri-trust-boundary.test.ts static guards (15) | Clean |
| C9 startup sequencer | main.rs CLI modes, desktop always wires Some(auth_manager) (None-auto-approve is headless-only), launch HTTPS gate classify/reset paths | Clean |

## New findings (all Low / hardening notes)

- **L-1 (hardening)** `core/src/win_acl.rs` (402 LOC) has zero inline unit tests. It IS exercised
  indirectly on the Windows CI lane via `bb.rs` DACL-readback tests, but the module's own edge table
  (foreign-owner, null-DACL FAT case, ACE-type mismatch) deserves direct tests. → Phase 3.
- **L-2 (availability-only, same-user)** `win_acl::secure_create_dir` create→open window: a same-user
  actor replacing the just-created directory with a regular file gets a hardened *file* where a dir
  was expected; subsequent child creations fail confusingly. No privilege gain. Not fixed (churn > risk).
- **L-3 (same-user only)** Unix `config::save_to` opens `config.json.tmp` with `.create(true)` — a
  pre-planted stale tmp keeps its old (looser) mode through truncate+write+rename. Requires write
  access to the 0700 dir ⇒ same-user only; Windows path is immune (remove+CREATE_NEW). Not fixed.

## Standing residuals (unchanged, correctly tracked)

#343 (upstream bb signing — blocked upstream), #344 (macOS Keychain negative-binding manual smoke),
F-09 (withdrawn-release replay), F-001 identity contract (SDK↔server), updater feed-body buffering
(plugin limitation), #351/#352 refactors.

## Verdict

The app's security posture is genuinely strong post-v2. The remediation arcs did what they claim;
the two "open" UX findings were closed by #446; remaining risks are all either upstream-blocked,
require signing-key compromise, or are same-user availability nits. No code changes required from
this phase.
