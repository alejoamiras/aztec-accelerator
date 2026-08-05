# Harden Report: security

**Repo:** `alejoamiras/aztec-accelerator` @ `9c4cb0c`
**Date:** 2026-07-31 (verification 2026-08-01)
**Effort:** `medium` tier shape, with owner-requested model upgrades (see Methodology)
**Run ID:** `2026-07-31-9c4cb0c`
**Models:** Phase 1 maps — Opus ×2 · Phase 2 — Opus + Codex xhigh per cluster (16 agents) ·
Phase 3 reduce — Opus · Phase 4 verify — Opus
**Scope:** `packages/accelerator/src-tauri`, `packages/accelerator/core`,
`packages/accelerator/src-tauri/frontend-src`, `packages/sdk`. Excluded by the owner:
`packages/accelerator/server` (headless crate), `packages/playground`, `packages/landing`,
`.github/workflows`, `infra/tofu`. Generated/vendored paths excluded as non-eligible.

**Stakeholder-facing version:** `report.html` (per-finding one-sentence summary + plain-language
explanation, technical trace collapsed). **Full detail:** `findings/consolidated.md` (reduce) and
`findings/verified.md` (verification). **Raw corpus:** `raw/` — 16 cluster reports + 2 repo maps.

## Executive summary

Thirteen findings: **2 High, 10 Medium, 1 Low, no Critical**. Nothing here is unauthenticated-remote;
every finding needs a local foothold, control of the update feed, or a user click. That matches the
prior audit's own ceiling.

The two High findings share a shape: **a check that was written for one purpose silently became
load-bearing for another.** In the SDK (F-01), a two-constant JSON shape check — whose own comment
says it is "collision resistance, not authentication" — is what decides where a complete private
transaction witness is sent. In the desktop app (F-02), `leaf_matches_ca()`, written to detect a torn
file swap, is the only thing standing between "our certificate authority" and "someone else's" before
one gets installed into the OS root store.

The single most actionable item is not an attack at all. **F-04 is a live, non-adversarial bug:** a
user who downgrades to an older build — the normal response to a bad release — trips
`running_below_floor` and is permanently and silently cut off from *every* future update, including
the fix. Nothing in the tree repairs or deletes `updater-state.json`, and the Windows uninstaller
only removes `certs/`, so reinstalling does not clear it. The fix is hours of work.

Two of the previous audit's remediations became findings here, which is the strongest argument for
auditing remediation code rather than treating a closed finding as closed ground.

## Methodology

Map-reduce, 8 clusters drawn by **entrypoint** and **sink family** (see `raw/CLUSTERS.md`), two
independent agents per cluster from different model families, then a coordinator reduce and a
verification pass.

**Deviations from the formal `medium` spec — stated explicitly:**
- **Models upgraded at the owner's request.** Spec says Sonnet for Phase 1 and the Phase 2 Claude
  legs; this run used **Opus** for all Claude legs (maps, clusters, coordinator, verifier) and
  **codex xhigh** rather than codex medium.
- **No Phase 2.5 cross-rebuttal** (correct for the `medium` tier): the two models stayed blind to each
  other within a cluster. Convergence is therefore genuine independent agreement.
- **Hierarchical outer mapper skipped.** With only two in-scope packages the inventory was already
  known; two per-package mappers ran directly.
- **Verification limited to 5 findings** by severity bucket (all High, then 3 Mediums chosen for the
  largest gap between claimed impact and evidence). Eight findings carry reduce-stage bands only.
- **One cross-model leg missing at reduce time.** The codex agent for the persistence cluster did not
  finish before Phase 3; it was captured afterwards and is in `raw/c6-persistence-codex.md`, but it
  did **not** inform the consolidated document. F-12 and F-13 therefore rest on a single family.
- **Context cap** ~4 functions of inter-procedural trace, with handoff-edge escalation, per the skill.

**Threat-model decision, applied uniformly:** same-user local code execution is **in scope**, because
the codebase itself repeatedly treats it as such (`certs.rs:239-275` refuses HTTPS bring-up over a
same-user-*readable* key; `win_acl.rs` applies owner-only DACLs against same-user processes;
`update_marker.rs:17-18` notes mtime is forgeable by the same user). But **scope is not severity**: a
local finding only counts when it yields something the attacker does not already have — durability
past the foothold, consent laundering, or crossing into another trust domain. One raw finding was
dropped for failing that test, and it is why F-12 is Medium rather than High.

## Findings

Full entries — trace, instances, why-mitigations-fail, fix, effort — are in
`findings/consolidated.md`; verification notes for the five marked ✔ are in `findings/verified.md`.

| ID | Band | Finding | Verified | Prior audit |
|---|---|---|---|---|
| F-01 | **High 8.1** | SDK sends the private witness to an endpoint it never authenticates, in cleartext by default | ✔ partial | Escalation of F-001 |
| F-02 | **High 7.8** | `install_ca_trust()` installs whatever cert set is on disk into OS root stores, unvalidated and unscoped | ✔ confirmed | New |
| F-03 | Med ~5.0 | Forgeable `/health` shape treated as process identity (Windows self-eviction) | ✔ partial | Escalation of F-002 |
| F-04 | Med 6.5 | One local write permanently and silently disables the update channel; survives reinstall | ✔ confirmed | New |
| F-05 | Med 6.1 | Trust *removal* fails open on macOS/Windows — failure reported as success | — | New |
| F-06 | Med 6.3 | `x-aztec-version` is unbounded remote control over which native binary is executed, and over cache growth | — | New |
| F-07 | Med 5.9 | Authorization-prompt flooding starves every legitimate origin of the consent path | — | New |
| F-08 | Med/Low | Witness residue (RAII-only deletion) + `bb` stderr containment that has never executed | ✔ partial | New |
| F-09 | Med 5.9 | Updater accepts a byte-for-byte replay of a withdrawn but signed release | — | Escalation of F-004 |
| F-10 | Med 5.6 | Consent clicks accepted before the anti-click-steal guard applies; 3 of 4 windows opt out entirely | — | New |
| F-11 | Med 5.3 | SDK reads the `/prove` body with no size cap, deadline, or abort path | — | New |
| F-12 | Med 5.4 | `appimage_self` containment test accepts any ancestor as `$APPDIR` | — | New (see flag) |
| F-13 | Low 3.8 | `schtasks_exe()` resolves through `SystemRoot` without its sibling's hardcoded-System32 preference | — | New (resolves map flag #5) |

### What verification changed

- **F-01 — rationale replaced, band held.** The report claimed the plaintext-by-default posture was
  "not covered by any accepted residual". That is **false** — it is documented and accepted
  (`https-by-default-onboarding-2026-07-09/plan.md:18,131`). What actually justifies High is an
  argument the report never made: **the cross-user case**. Any local account can bind
  `127.0.0.1:59833`. Facet B (`host`) drops to **Low** — there is no environment-variable ingress.
- **F-03 — sink B refuted.** `commit_launch_floor` commits `env!("CARGO_PKG_VERSION")`, not the probed
  value, and `candidate_allowed` enforces `candidate > current` independently of the state file, so a
  forged probe yields `floor == current`: a no-op. The self-eviction sink is confirmed and was
  *under*-argued — it is what defeats the SDK's HTTPS preference on Windows, i.e. it opens F-01's
  window. Band 6.8 → ~5.0.
- **F-08 — split.** Facet B's dead control is proven from `tokio-1.52.3/src/process/mod.rs:1442-1466`,
  but the disclosure is asserted rather than shown; the certain harm is total loss of `bb` diagnostics
  (Low as security, high as a bug). Facet A's "accumulates over months" magnitude is unsupported.
- **F-02, F-04 — confirmed**, F-04 including the claim the verifier tried hardest to refute.

No finding was refuted outright and no band crossed a tier boundary.

### Remediation order

1. **F-04, items 1–2 (hours).** Fall back to `floor = current_running_version` on corrupt or
   impossible state and surface it in Settings. Removes a permanent, silent, *accidentally* triggerable
   lockout. Safe because `candidate_allowed` already enforces `candidate > current` before consulting
   the file — verified.
2. **F-05 (hours).** Port the `Present::{Yes,No,Unknown}` tri-state from `trust/linux.rs:328-350` to
   macOS and Windows, and make `nsis/hooks.nsi` call the verified `--remove-ca-trust` CLI instead of a
   bare unchecked `certutil`. Also bounds F-02's blast radius.
3. **F-08 facet B (one line).** Pipe or explicitly discard the `bb` child's stderr so the existing
   truncation actually runs.
4. **F-02 (about a day).** One validation function called before every trust-store write, plus `-p ssl`
   on macOS and an EKU restriction on Windows.
5. **F-01 (hours–day for steps 1–2).** Default `httpsOnly` on when HTTPS is healthy; build URLs with
   `new URL()` and constrain `host` to loopback unless explicitly overridden.

## Dropped / not pursued

- **TLS-handshake DoS** (codex, c5) — a same-user attacker can already `SIGKILL` the app; a local
  availability-only finding must beat that bar.
- **bb integrity-marker forgery** (c3, self-rejected) — a same-user process can already read the
  witness from the 0700 workspace; the marker adds no capability. Left as a doc-accuracy nit:
  `cache_layout.rs:208` overstates it as "the single authority the runtime trusts".
- **npm tarball ships `src/**` incl. `test-setup.ts`** (c8, refuted) — `exports` is a plain string, so
  every exports-aware resolver refuses deep paths; `test-setup.ts` is unreachable from `index.ts`.
  Packaging hygiene for the quality run.
- **`@aztec/simulator/client` dynamically imported but only a devDependency** (c8, refuted at the
  security bar) — no attacker-controlled step; exploiting it requires already owning `node_modules`.
- **`auth.rs:84` discards the promoted request id** (c1) — real defect, unreachable in practice,
  outcome is denial. Routed to the bugs run.
- **`cleanup_old_versions` can evict a version an in-flight prove is using** (c2) — spurious prove
  failure; correctness, not security. Routed to the bugs run.
- **macOS LaunchAgent plist written with `std::fs::write`** rather than the atomic symlink-refusing
  writer used for the same file elsewhere (c6) — real asymmetry, no security property violated.

## Cross-cutting observations

**Hardening applied on the write path but not the read path.** The update marker is *created* with
owner-only Windows ACLs but a marker that is *read* is never checked for owner or ACL. The certificate
set is *written* 0600 with atomic renames but the set that is *installed* is never re-validated. In
both cases the protective effort went into producing the file and the consuming path trusts what it
finds.

**A control exists on one platform and not its siblings.** Linux has the tri-state trust-removal
result; macOS and Windows do not (F-05). Linux scopes the trust grant; macOS and Windows do not
(F-02). `trust/windows.rs` distrusts `%SystemRoot%`; `crash_recovery.rs` does not (F-13) — and the
former's comment cites the latter as the pattern it follows.

**Documentation asserts invariants the code does not enforce.** `bridge.js:38-39` states every
consequential control is click-guarded; three of four are not. `probe.rs:11-13` claims its check stops
an arbitrary process being accepted; it checks two public constants. `cache_layout.rs:208` calls an
unauthenticated JSON file "the single authority the runtime trusts". Each currently misleads a reader.

**Yesterday's fixes are today's findings.** F-004's remediation introduced `updater-state.json`
(F-04's primitive) and `commit_launch_floor`, which was built on the still-unfixed probe from F-002.
Remediation code deserves the same scrutiny as the code it remediates.

## Coverage gaps

- **Out of scope by request:** the headless server crate, playground, landing, all CI/CD workflows,
  and the Terraform infrastructure. F-09's real-world severity depends on prior F-005, which lives in
  that infrastructure and was not re-verified.
- **One missing cross-model leg at reduce time** (persistence cluster) — F-12 and F-13 have no
  second-family corroboration, in exactly the cluster the owner flagged as a priority.
- **Eight findings carry reduce-stage bands only** (see the table); only five were verified.
- **Map uncertainty flag #3 unresolved:** `snapshot_restore_roundtrip_for_tests`
  (`autostart.rs:1655`) is a `pub fn` in the production library that mutates real autostart state; no
  caller was found in `src/`, and no agent examined whether it is reachable in a shipped build.
- **F-12 is a re-take, not a discovery.** The `APPDIR=/` case was explicitly considered during the
  arc bug hunt earlier the same day and the risk was accepted under a correctness lens. It is listed
  here because the payoff — durable, self-repairing persistence attributed to a signed application —
  deserves re-weighing under a security lens, not because it was missed.
