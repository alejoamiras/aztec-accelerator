# Claim ledger — prior findings treated as UNVERIFIED CLAIMS

Nothing here is accepted as truth. Each row gets a verdict in Phase 1/2:
`CONFIRMED-FIXED` (fix is sound at source) · `FIX-INCOMPLETE` · `REGRESSION` (fix introduced a hole) ·
`REFUTED` (finding was wrong/overstated) · `STILL-OPEN` · `STALE` (superseded by later work).

## Run 2026-07-31 (`9c4cb0c`) — security, 13 findings

| ID | Claim | Claimed state | Verdict |
|---|---|---|---|
| F-01 H | CONFIRMED-FIXED (server side): `/health` stays deliberately-public contract; origin-tiered minimal body; NO server-side auth token exists — SDK↔server identity (old F-001) remains an open design gap, tracked | ✔ |
| F-02 H | CONFIRMED-FIXED — structural `validate_ca_profile` (critical nameConstraints ⇒ loopback-only) + `vetted_copy_of` random-name 0600 copy; same-UID swap window is an honestly-documented RISK ACCEPTANCE bounded by the loopback constraint (certs.rs:139-216,520-617) | ✔ |
| F-03 M | CONFIRMED-FIXED all sinks — sink A kernel-mediated four-tuple→PID→image-path (`server/owner.rs`, Guarded(ACCESS_DENIED)⇒Foreign, Unknown-exits polarity pinned); sink C chunk-wise 64KiB cap (`probe.rs:21-46`); one-connection tie defeats the two-connection bypass | ✔ |
| F-04 M | CONFIRMED-FIXED — floor clamped/repaired to running build on successful launch, `pending` expires (updater_state.rs:279-318); lockout shape gone | ✔ |
| F-05 M | CONFIRMED-FIXED — removal post-condition readbacks fail closed on BOTH backends (windows.rs:239-270 Present/Unknown/unrun-delstore ⇒ reported failure; macos.rs remove-all-by-CN + post-check) | ✔ |
| F-06 M | CONFIRMED-FIXED — denylist checked BEFORE bundled short-circuit, digest-verified download (64MiB stream cap + 512MiB decompression bomb cap), lease-guarded eviction incl. pre-permit lease (prove.rs:325-334). Residual: denylist empty by default = any well-formed version downloadable (by design, digest-pinned) | ✔ |
| F-07 M | CLOSED-BY-LATER-WORK (#446 B2): 30s per-origin post-deny cooldown (64-entry evict-oldest map) + MAX_PENDING_ORIGINS=10 queue bound + 60s activation-relative auto-deny. Residual: unlimited-distinct-origin flood still adds bounded consent latency; starvation claim now OVERSTATED | ✔ |
| F-08 M/L | Witness residue RAII-only + never-executed stderr containment | Fixed both facets (#441 reaper, #447) | ☐ |
| F-09 M | STILL-OPEN (owner deferral sound): closure would reintroduce the F-04 permanent-lockout shape; replay needs feed/signing-key control; revisit when upstream provides release revocation | ✔ |
| F-10 M | CLOSED-BY-LATER-WORK (#446 B2): click-steal guard DEFAULT-ON (bridge.js:21-24), zero `guard:false` opt-outs on consequential buttons (authorize/renewal/update-prompt/onboarding all wired guarded) | ✔ |
| F-11 M | CONFIRMED-FIXED body facet: 50MiB `to_bytes` cap + 30s whole-body deadline + inflight-shed (429) + permit-only-around-proving (prove.rs:132-246, A1 ordering auth→inflight→cap→buffer→permit). Disconnect-abort rides the bb Guard drop chain (tree-kill). Phase-ordering facet SUPERSEDED by A1 | ✔ |
| F-12 M | `appimage_self` containment accepts any ancestor as `$APPDIR` | Fixed (#438) | ☐ |
| F-13 L | `schtasks_exe()` resolves via `SystemRoot`, not hardcoded System32 | Fixed (#438) | ☐ |

## Run 2026-07-09 (`5c788c0`) — security, 16 findings → campaign C0–C10 merged

| ID | Claim | Claimed state | Verdict |
|---|---|---|---|
| F-001 | SDK↔server identity contract missing | BLOCKED → C11/F-002 blocked on it | ☐ |
| F-002 | Spoofable `/health` evicts real accelerator (Windows) | Escalated into 07-31 F-03 | ☐ (via F-03) |
| F-003 | CONFIRMED-FIXED — 0700 dirs / 0600 witness at creation syscall; Windows owner-only DACL fail-closed readback; no OS-temp fallback on Windows | ✔ |
| F-004 | Updater rollback: feed version not bound to signed artifact | Campaign claim: fixed (Layer A/B) | ☐ |
| F-005 | Deploy trust reaches update feed (wildcard OIDC, whole-bucket write) | Human-applied tofu/ruleset — verify repo state | ☐ |
| F-006 | `_publish-sdk.yml` dist_tag shell injection | Campaign claim: fixed | ☐ (CI, low priority) |
| F-007 | download-bb.ts poisons runtime-trusted cache | Campaign claim: fixed | ☐ |
| F-008 | Windows bb.exe checksum TOFU | Accepted residual (upstream unsigned) | ☐ |
| F-009 | `/prove` buffers full body before semaphore (memory DoS) | Overlaps 07-31 F-11 | ☐ (via F-11) |
| F-010 | CONFIRMED-FIXED — systemd serializer fails closed on all injection classes, `%%`+quoted-`:` token, round-trip proven | ✔ |
| F-011 | CONFIRMED-FIXED — trailing-dot origins REJECTED (not stripped) at parse, ingress + persistence + config-deser all enforce | ✔ |
| F-012 | Global Tauri IPC + CSP gaps | Campaign claim: fixed (per-page caps) | ☐ |
| F-013 | Headless auto-approves localhost | Accepted (headless out of scope) | ☐ |
| F-014 | Popup overflow + Remember pre-checked | REVERSED by owner (#421 permanent-Allow) — documented decision | ☐ sanity-check copy |
| F-015 | Mutable major-tag Action pins | Campaign claim: fixed | ☐ (CI) |
| F-016 | CA signing key not zeroized | Safari-TLS plan deleted mint-any-cert primitive (#288) | ☐ confirm dead |

## GitHub issues (open)

| # | Claim | Verdict |
|---|---|---|
| #343 | Pin bb artifact integrity once upstream signs (SEC-02) | upstream-blocked — confirm still true |
| #344 | macOS Keychain negative-binding manual smoke (SEC-09) | manual runbook owed |
| #345 | Updater memory-DoS: preflight size cap defeated by manifest tampering | needs independent download-path bound — re-assess merit |
| #351 | Dedupe loopback-literal sets across 3 matchers | refactor, low |
| #352 | `/health` unknown-version sentinel → Option | refactor, low |

## Other residuals

| Source | Claim | Verdict |
|---|---|---|
| arc-bug-hunt | 1 Low: packaged-E2E fixture provenance | ☐ |
| closeout-followups plan | Windows ACL tail (owner-only cross-user), C9 popup arbiter, C8 autostart rollback | blueprint awaiting approval — fold into Phase 1/2 targets |
| v2 train | B1 Authenticode DEFERRED; Windows composed-HTTPS-proof = manual runbook; playground prod-deploy pending | Phase 4 inputs |
