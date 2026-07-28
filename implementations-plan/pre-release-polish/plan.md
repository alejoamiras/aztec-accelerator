# Pre-release polish: authorization model + app identity

**Tier**: `mid` · **Worktree**: `pre-release-polish` · **Base**: `main` @ `80c6f4c` · **eli5_mode**: Artifact
**Revision 2** — rewritten after codex + fable both returned `reject`. See `audit-codex.md`,
`audit-fable.md`, and the Adopted/Rejected log below. `recon.md` is the factual base.

Two owner-raised pre-release items. The audits turned this from "two small changes" into "two small
changes plus a latent upgrade bug they exposed" — the rename would have silently broken autostart for
every upgrading user, and no existing test could have seen it.

---

## What changed in revision 3 (read this first)

The fresh-context codex pass rejected R2 and found errors **in R2's fixes**. Corrections:

1. **`mainBinaryName` is `"AztecAccelerator"` — no space** (owner decision). A space is actively
   incompatible with the stack: `auto-launch-0.5.0/src/windows.rs:41-43` writes the Run value as
   `format!("{} {}", app_path, args)` with **no quoting**, so `Aztec Accelerator.exe` yields a Run
   command Windows parses as `C:\...\Aztec` + arg `Accelerator.exe` — autostart broken on **fresh
   installs**, not just upgrades. That crate is pinned by `tauri-plugin-autostart` and is not ours to
   fix. `main.desktop:6` (`Exec=env GDK_BACKEND=x11 {{exec}}`, unquoted) has the same bug and IS ours.
   Dropping the space removes both hazards; macOS still gains `AztecAccelerator` over
   `aztec-accelerator`.
2. **R2's "Release Smoke does not run on PRs" was WRONG and is retracted.** `accelerator.yml:382-385`
   defines a job named exactly `Release Smoke`, gated `desktop == 'true'`. R2 adopted that from the
   fable audit **without verifying it**. Recorded as a process failure, not just a fact fix.
3. **I5 is false and the codebase already knew.** `crash_recovery.rs:75-101` `enable_transaction`
   deliberately refuses to re-run enable when already enabled *because macOS recreation strips
   `KeepAlive`*. R2's "disable→enable is idempotent" self-heal is the exact operation that guard
   exists to prevent. **Phase 2 must be redesigned**, not just implemented.
4. **Phase 1's gate was logically impossible** — it asked for "old name absent after" while product
   code is unmodified, i.e. while the old name is still correct. And `workflow_dispatch` alone does
   not make `_e2e-updater-windows.yml` standalone: it needs `n-version` and an N artifact from its
   caller (`_e2e-updater-windows.yml:16-55,65-70`).
5. **Phase 4's gate named the wrong job** — "Production Build Smoke" is the playground/browser build
   (`app.yml:108-128`) and accelerator-only changes do not trigger its path filter.
6. **Second-instance / updater race** (codex HIGH): startup reconciliation can run before the second
   instance discovers the healthy incumbent (`main.rs:270-303`), recreating launcher/recovery state
   the updater deliberately disarmed (`updater.rs:357-396`). Phase 2 must specify coordination.
7. **The persistence test needs an injectable save sink** — `lock_mutate_save` hardcodes
   `config_path()` (`config.rs:221-228`); `config.rs`/server state joins the change map.
8. Owner decisions recorded: publisher `"Aztec Accelerator"`; copyright
   `"© 2026 Aztec Accelerator contributors"`; Phase 6 stays optional-and-last.

**Status: NOT ready for the approval gate.** Phase 1 and Phase 2 both need redesign before a fourth
audit round. See "Open work" at the bottom.

## What changed in revision 2

1. **The security rationale was wrong and is retracted.** R1 argued "Allow once isn't really a
   rate-limit because it persists nothing." Codex: *"Re-prompting on every later proof is exactly the
   audit's rate-limit… Its severity as UX friction proves it functions."* That is correct. The change
   is now framed as a **deliberate trade**, not a refutation, and it is paired with a disclosure line
   so the consent is informed.
2. **The rename breaks autostart for upgraders.** New Phase 2. Verified against
   `auto-launch-0.5.0/src/windows.rs:73-83`: `is_enabled()` only checks that the Run *value* exists,
   never the path it points to.
3. **The instrument is fixed before the measurement.** `competing-outline.md` won that argument. A
   real Windows N-1 exists (`accelerator-v1.0.7`), so it is a template copy, not a project.
4. **Two of R1's validation gates were fiction** — one named a job that doesn't run on PRs, one named
   a `workflow_dispatch` that doesn't exist. Both fixed.

---

## Architecture & Implementation

### Proposed architecture

Still no new components. Three kinds of work:

- **A latent-bug fix** (Phase 2): autostart entries must self-heal their stored path on launch, the
  way crash recovery already does. This is a prerequisite for the rename, not part of it.
- **Configuration** (Phases 3–4): `tauri.conf.json` only; `Cargo.toml` untouched (recon B1).
- **A subtraction plus a sentence** (Phase 5): delete the `remember` control, make persistence
  unconditional, and disclose permanence in the popup.

### The autostart defect (Phase 2) — mechanics

```
user toggles autostart ─► commands.rs:449 current_exe() ─► Run value "Aztec Accelerator" = <old path>
                                                            LaunchAgent "Aztec Accelerator.plist"
update installs N ─────► old binary deleted, new name written
next launch ───────────► main.rs:614 autolaunch().is_enabled()
                            └─ auto-launch/windows.rs:73-83 checks the VALUE EXISTS, not the path
                            └─ returns TRUE  ──► Settings shows autostart ON  ← a lie
                                             └─► enable_crash_recovery() re-arms from current_exe()
                                                 ── crash recovery SELF-HEALS (fable claimed it
                                                    breaks; it does not — codex agrees)
```

Net: **autostart silently points at a deleted file and the UI reports it as working.** macOS is worse
(codex): startup skips `autolaunch.enable()` when the plist exists and `enable_crash_recovery()`
returns early when `KeepAlive` exists, so neither self-heals there.

**Fix**: at startup, when autostart reads as enabled, re-write the entry to `current_exe()`
(disable→enable, preserving user intent) if the stored path differs. Idempotent, cheap, and it makes
*any* future path change self-healing — not just this rename.

### Key interfaces — D2 revised

**R1 chose**: drop `remember` from `respond_auth`, keep `AuthDecision::Allow { remember }`.
**Both audits called this a half-migration.** Codex's argument is decisive and inverts my rationale:

> *"A renderer-provided `false` is less privilege, so the 'IPC trust boundary' rationale is
> misleading."*

Correct — an untrusted renderer sending `false` asks for *less*, so removing its ability to do so is
not a privilege reduction. The real reason to change the boundary is that the value has one meaning.

**R2 chooses the full migration**: `AuthDecision::Allow` (no field); persistence unconditional in
`auth.rs`. The five direct construction sites (`authorization.rs:503,524,625,633`,
`server/tests.rs:680`) are cheap to update and doing so *encodes* the new invariant instead of
leaving a field that only ever holds one value.

### Data & control flow

Unchanged except that `remember` ceases to exist as a concept. One honest caveat that must reach the
UI copy: **`auth.rs` warns-and-continues on config-save failure** (deliberate — "a config-write error
must NOT fail an approved prove"). So persistence is best-effort. The disclosure line must not
promise more than that.

### File-level change map

**Phase 1 — instrument**
| File | Change |
|---|---|
| `.github/workflows/_e2e-updater-windows.yml` | add `workflow_dispatch` (today `workflow_call` only — R1's gate could not run); repoint N-1 at the real `accelerator-v1.0.7` setup.exe per the file's own TODO at L14 |
| same | add pre/post assertions: old binary name present before, absent after; new name present after; **autostart Run value resolves to an existing file** (codex: current script sets the Run key itself and only asserts `/health`) |

**Phase 2 — autostart self-heal**
| File | Change |
|---|---|
| `src-tauri/src/main.rs` (~L607-625) | when autostart reads enabled, re-point the entry at `current_exe()` if stale |
| `src-tauri/src/crash_recovery.rs` | macOS: don't early-return on existing `KeepAlive` when the stored path is stale |
| `src-tauri/src/commands.rs` | shared helper if needed |
| tests | unit test: stale stored path → re-armed to `current_exe()`, enabled state preserved |

**Phase 3 — `mainBinaryName` + lockstep CI (8 sites, not 5)**
| File:line | Change |
|---|---|
| `tauri.conf.json` | add `mainBinaryName: "Aztec Accelerator"` |
| `accelerator.yml:575,577` | PR-gate Windows filter |
| `_e2e-webdriver.yml:91,92,93` | **all three** `APP_CMD` assignments (92/93 identical — fixing one leaves a decoy) |
| `_e2e-webdriver.yml:96` | **quote `"$APP_CMD"`** — unquoted, a spaced name breaks it regardless of path |
| `_e2e-webdriver.yml:163,165` | `taskkill //IM` + `pkill -f` cleanup, narrow-anchored |
| `release-accelerator.yml:250` | bundle-shape invariant `EXPECTED` |
| `release-accelerator.yml:404,452` | DMG smoke `find` + cleanup pattern |
| `updater-smoke-windows.ps1:61,191` | installed-exe lookup + cleanup |
| `UPDATER_TESTING.md` | bundle invariant documented there too (codex) |
| `scripts/tauri-identity.test.ts` | **NEW** drift guard (pattern: `updater.rs:508-518`) |

**The space is load-bearing.** `"Aztec Accelerator"` contains one, and it propagates into
`.app/Contents/MacOS/`, NSIS `${MAINBINARYNAME}`, the `.deb`'s `/usr/bin/`, and `main.desktop`'s
`Exec=` (a Handlebars template neither R1 nor recon opened). Phase 3 must inspect `main.desktop`
quoting explicitly.

**Phase 4 — bundle metadata** — `tauri.conf.json` only.

**Phase 5 — popup**
| File | Change |
|---|---|
| `frontend/authorize.html` | delete the `.popup-remember` block; `"Allow once"` → `"Allow"`; **add the disclosure line** |
| `frontend-src/authorize.js` | delete `rememberEl` + its listener + the payload field; **remove the now-unused `isClickGuardActive` import** (codex) |
| `src-tauri/src/commands.rs` | drop the param; `AuthDecision::Allow` |
| `core/src/authorization.rs` | drop the enum field; update 4 construction sites + the misleading doc comment at L410 (fable) |
| `core/src/server/auth.rs` | persistence unconditional |
| `frontend/style.css` | **`.popup-remember` NOT deleted** (shared with `update-prompt.html:14`); harden `.origin-item` (L220-232) |
| `frontend-src/settings.js:91` | **bidi-isolate the approved-origins rendering** — fable HIGH: the surface this plan promotes to load-bearing has none of the popup's F-014 treatment |
| `e2e/authorize.spec.ts` | 5 touchpoints (`:53,58,73,77-92,111`), not 4 |
| `e2e-webdriver/auth-flow.spec.ts` | rewrite; `:193-196` asserts `isSelected()`; delete the "without remembering" spec |
| `core/src/server/tests.rs` | **ADD** persistence test — with an injected temp config path, reloaded **from disk** (codex: the naive version can pass while the save fails, and writes the developer's real `~/.aztec-accelerator/config.json`) |
| `README.md:121-132,375` | rewrite the model; fix the false "session" claim; `:375` hardcodes "WebDriver E2E tests (9)" |
| `decision-allow-once.md` | **NEW** — the documented reversal |

### Trade-offs & alternatives not taken

- **Full ephemeral option** ("Allow for 1 hour"): rejected — new grant lifetime, new state, more
  surface than removed.
- **Second confirmation for unverified origins**: offered to the owner, not chosen. Recorded so a
  future reader knows it was considered.
- **Keeping the enum field** (R1's D2): rejected on codex's argument above.
- **Synthetic old-name fixture** (codex's third option): rejected in favour of the real N-1, because
  a real prior release now exists and the workflow's own comment says to switch once it does. Codex's
  *assertions* are adopted regardless.

---

## Security & Adversarial Considerations

### The trade, stated honestly

**Retracted from R1**: the claim that "Allow once" is not a real control. It is one. Re-prompting
requires renewed user presence and intent; the friction *is* the mechanism.

**What we are actually choosing**: a control that works by attrition, against a control that works by
disclosure. The shipped one re-prompts on the *next proof* (recon A2 — it does not survive the popup
closing), so in practice a regular user of one dApp faces a prompt per proof forever. The predicted
outcomes are click-through habituation or ticking "Always allow" out of irritation — both landing on
a permanent grant by a worse path. **This prediction is not measured in this codebase** and is
recorded as a stated assumption, not a finding.

**In exchange** the popup must make permanence explicit at the moment of consent. That is the
property F-014 actually bought — fable put it exactly right: *the checkbox was the notification.*

### Threat model

- **Attacker**: a look-alike origin (homograph, subdomain, punycode) that gets one Allow click.
- **Gains, post-change** (codex, adopted): durable capability from one ordinary click — able to
  return on later visits, exploit a future XSS or dependency compromise on that origin, or retain
  access after the domain changes hands, all without a further prompt. The grant authorizes any
  witness that origin later obtains.
- **Unchanged**: loopback `Host` allowlist, deny-by-default, origin canonicalization at ingress,
  per-window capability ACL.
- **Pre-existing, unchanged, noted** (fable): `auth.rs:31-34` auto-approves a request with **no**
  `Origin` header (non-browser caller behind the loopback guard). The popup is not the whole boundary.

### Compensating controls — corrected

| Control | Status after the audits |
|---|---|
| **Disclosure line** (new) | The primary control. Must state persistence and where to undo it, and must NOT overpromise: `auth.rs` warns-and-continues on save failure, so persistence is best-effort. Pinned by a test. |
| Full origin display | Necessary, not sufficient — codex: humans parse long domains poorly and the registrable domain is not emphasized. Its F-014 hardening must survive Phase 5 unchanged. |
| Verified-sites badge | **Downgraded.** `VERIFIED_SITES.md` says explicitly it is *not* a security guarantee, and it survives DNS/site compromise. It is a convenience signal; it is no longer counted as a control. |
| Revocation via Settings | **Was overstated.** fable HIGH: `settings.js:91` renders origins with a bare `textContent` and `.origin-item` (`style.css:220-232`) has none of the popup's bidi isolation. **Phase 5 hardens it** — otherwise the plan promotes a surface F-014 skipped. |
| 700 ms click-steal guard | Real but narrow — prevents click-steal, not social engineering or a deliberate click. |

### Rename-specific

Narrow-anchor every `pkill`/`taskkill` pattern touched: a recorded incident
(`ci-release-overhaul-2026-06-01/lessons/phase-a2b.md:55`) had `pkill -f "aztec-accelerator"` match
the smoke script's own argv because the checkout path contains that string.

---

## Assumptions

### Facts (verified; corrections from R1 marked)

1. `remember == false` persists nothing (`core/src/server/auth.rs:89-115`).
2. `README.md:126`'s "approved for this session only" is false today.
3. F-014 / PR #392 made the checkbox default-unchecked in response to a written audit finding.
4. `.popup-remember` CSS is shared with `update-prompt.html:14`.
5. `mainBinaryName` needs no `Cargo.toml` change.
6. The rename is a **MOVE** — measured: `target/debug/` keeps only `Aztec Accelerator` + a `.d` file.
7. **CORRECTED (was "five sites")** — at least **eight** CI/script sites, plus `main.desktop`'s
   `Exec=` and `UPDATER_TESTING.md`. Three `APP_CMD` assignments exist, two identical.
8. **CORRECTED (F8/F9 overclaimed in R1)** — Tauri's `CFBundleExecutable` handling proves the
   *immediate macOS restart* only. It does **not** cover persisted launcher entries.
9. `auto-launch-0.5.0/src/windows.rs:73-83`: `is_enabled()` checks only that the Run value exists,
   never its path → **autostart breaks silently and the UI reports it as on**. Crash recovery
   self-heals on Windows (`main.rs:614-618`); on macOS neither does.
10. Windows Publisher renders as `"aztec"` (identifier-split fallback).
11. Icons complete; tray icon is a correct macOS template image.
12. **CORRECTED** — a real Windows stable N-1 **exists**: `accelerator-v1.0.7` ships
    `Aztec-Accelerator-1.0.7-Windows-x86_64-setup.exe`. `_e2e-updater-windows.yml:14` says to switch
    to the real-download pattern once one exists.
13. `_e2e-updater-windows.yml` exposes **only `workflow_call`** — R1's Phase 4 gate was unrunnable.
14. `_e2e-webdriver.yml:96` executes `$APP_CMD` **unquoted**.
15. `auth.rs` **warns and returns `Ok(())`** on save failure — deliberate; so "always persists" is false.

### Inferences (labelled for attack)

- **I1**: `mainBinaryName` fixes both macOS symptoms (name + missing icon). **Now a release gate on
  the owner's Mac, not a note** (codex).
- **I2**: dropping an IPC parameter needs no capability change — ACLs grant command names. (Both
  audits: sound.)
- **I3**: **RETRACTED.** Metadata is *not* inert — `licenseFile`, `category` and `bundle.icon` alter
  bundling and signature inputs and can fail platform builds.
- **I4**: **RETRACTED as stated.** Beyond built-debug, the move breaks stale launchers, and a spaced
  filename breaks the unquoted execution independently.
- **I5 (new)**: the autostart self-heal is safe to run on every launch because disable→enable is
  idempotent. Unverified — Phase 2 must test it, including that user intent (off stays off) survives.

### Asks

- **ASK-1 — RESOLVED.** Owner: real N-1 + old-name assertions + add `workflow_dispatch`.
- **ASK-2 — Phase 6 (version in Settings) in or out?** Still open; Low. The `/goal` seed is worded so
  an optional phase does not block completion (codex LOW).
- **ASK-3 — `copyright` holder string.** Proposal: `"Copyright © 2026 Aztec Accelerator
  contributors"`. **Must not be delegated to an autonomous loop** (codex MED): legal identity is the
  owner's to state. Phase 4 blocks on it.
- **ASK-4 — `publisher` string.** Proposal: `"Aztec Accelerator"`. Same constraint as ASK-3.
- **ASK-5 (new, codex HIGH) — explicit security risk acceptance.** The owner has chosen
  "Allow + disclosure line" over the audits' objection to a bare permanent Allow. That acceptance is
  recorded here and in `decision-allow-once.md`, and is part of what the approval gate approves.

---

## Phases

Order follows codex's recommendation: fix the instrument, then the latent bug, then the risky
mechanical change, then config, then the semantic change.

### Phase 1 — Make the Windows updater fixture executable and representative
Add `workflow_dispatch`; repoint N-1 at the real `accelerator-v1.0.7`; add assertions that the old
binary name is present before and absent after, the new one present after, and the autostart Run
value resolves to an existing file.

**Gate** · `gh workflow run _e2e-updater-windows.yml --ref worktree-pre-release-polish` on
**unmodified product code** → both positive and negative legs green. A fixture change proven green
before any product change is one you can trust afterwards.
Layers: e2e (real installer, real upgrade).

### Phase 2 — Autostart self-heal (latent bug; prerequisite for the rename)
Re-point a stale autostart entry at `current_exe()` on launch, preserving enabled/disabled state;
fix the macOS early-returns.

**Gate** · `cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml`;
`bun run test`. New unit tests: stale path → re-armed; **off stays off**; idempotent across launches.
Layers: typecheck/lint · unit.

### Phase 3 — `mainBinaryName` + lockstep CI
All eight sites in one commit, `"$APP_CMD"` quoted, `main.desktop` `Exec=` inspected, plus the
`tauri-identity.test.ts` drift guard.

**Gate** · `bun run test`; `bun run lint:actions`; `shellcheck packages/*/scripts/*.sh`; full PR CI.
**`E2E WebDriver (built-debug, linux)`, `E2E WebDriver (windows)` and `Windows Build Smoke` must all
be green**, plus the Phase-1 fixture re-run — which now actually observes the rename.
Layers: typecheck/lint · unit · integration · e2e.

### Phase 4 — Bundle metadata *(blocked on ASK-3 + ASK-4)*
`publisher`, `licenseFile`, `category`, `copyright`, `homepage`, the two unused icon sizes.

**Gate** · `bun run test`; `bun run lint:actions`; PR CI **`Windows Build Smoke` + `Production Build
Smoke`** green (**not** "Release Smoke" — R1 named a job that does not run on PRs). Because
`bundle.icon` and `licenseFile` change bundling inputs, a macOS bundle must be produced at least once
before release.
Layers: typecheck/lint · unit · integration.

### Phase 5 — Remove "allow once", disclose permanence, harden revocation
Full D2 migration; disclosure line; bidi-harden the Settings origin list; the corrected persistence
test; README; `decision-allow-once.md`.

**Gate** · `frontend:build`; `bun run test`; `test:e2e:ui`; both `cargo test`s;
`test:e2e:webdriver`. Specific criteria:
- a spec asserts the checkbox is **absent** and the disclosure copy is **present**;
- the F-014 origin-display specs still green;
- the new persistence test reloads config **from disk** via an injected temp path;
- `handlers.length === 18` pin still green.
Layers: typecheck/lint · unit · integration · e2e.

### Phase 6 — Version in Settings *(optional, ASK-2)*
**Gate** · `bun run test`; `test:e2e:ui` with a spec pinning the rendered version.

### Phase 7 — Re-run Phase 1 and Phase 5 gates together
fable MED: Phase 3 changes the launcher that Phase 5's WebDriver gate depends on, so a green Phase 5
before Phase 3 proves nothing afterwards. Final gate re-runs both.

**Gate** · full PR CI green on the merged branch + the Phase-1 dispatch re-run.

---

## Post-implementation hardening
`/harden security` scheduled as a separate pre-release pass (owner, Phase 0).

---

## Decision ledger

| # | Decision | Chosen | Rejected | Why |
|---|---|---|---|---|
| D1 | Popup shape | `[Deny] [Allow]` + disclosure line | bare Allow; pre-checked checkbox; Block list | Owner. Disclosure restores the informed-consent property both audits said a bare Allow destroys. |
| D2 | `respond_auth` / `AuthDecision` | **Full migration** — drop the param AND the enum field | R1's keep-the-field | Codex: a renderer-sent `false` is *less* privilege, so R1's rationale was backwards. 5 cheap edits encode the invariant. |
| D3 | Ordering | Instrument → latent bug → rename → metadata → popup | R1's popup-first | Both audits. A green synthetic same-name upgrade cannot validate a changed identity. |
| D4 | Fixture | Real `accelerator-v1.0.7` N-1 **+** codex's old-name assertions | synthetic-only; real-only | Owner. Real release exists (F12); assertions catch what `/health` alone misses. |
| D5 | Sweep scope | Identity + bundle metadata | + installer UX; + signing | Owner. |
| D6 | Verified badge as a control | **Demoted** to a convenience signal | counting it | `VERIFIED_SITES.md` says it is not a security guarantee. |
| D7 | Save-failure semantics | Keep warn-and-continue; make the **copy** accurate | fail the Allow | Existing behaviour is deliberate and documented; the defect was the plan's claim, not the code. |
| **Open** | ASK-2 / ASK-3 / ASK-4 | — | — | Owner input; ASK-3/4 explicitly not delegable. |

## Audit adopted / rejected log

**Adopted from codex**: security-rationale retraction; disclosure copy; durable-capability framing;
D2 full migration; launcher migration + upgrade assertions; corrected persistence test (temp path,
disk reload); `workflow_dispatch` + non-representative-gate finding; unquoted `$APP_CMD`; I3/I4
retractions; ASK-5 risk acceptance; ASK-3/4 non-delegation; `UPDATER_TESTING.md`; unused
`isClickGuardActive` import; verified-badge demotion; save-failure accuracy.

**Adopted from fable**: real Windows N-1 exists (decisive, verified); Settings origin-list lacks
F-014 hardening; silent-persistence/"the checkbox was the notification"; Phase-2 gate named a
non-PR job; `bundle.icon` changes signed contents; `_e2e-webdriver.yml:163,165` omitted; the
`APP_CMD:93` decoy; the space-in-name propagation incl. `main.desktop`; 5 not 4 spec touchpoints;
`auth-flow.spec.ts:193-196`; `README.md:375` test count; `authorization.rs:410` doc comment; Phase
ordering re-invalidating an earlier gate.

**Rejected, with reason**:
- *fable: "the rename breaks crash recovery too"* — **factually wrong**. `main.rs:614-618` re-arms
  from `current_exe()` on every launch when autostart reads enabled, and
  `auto-launch/windows.rs:73-83` returns `true` regardless of path staleness, so the re-arm fires.
  Verified directly; codex independently agrees ("although its recovery task is rewritten"). The
  macOS early-return *is* real and is adopted separately.
- *codex: synthetic old-name fixture instead of a real N-1* — the assertions are adopted, the
  synthetic approach is not: a real prior release now exists and the workflow's own comment (L14)
  says to switch to it.
- *codex: second confirmation for unverified origins* — offered to the owner, not chosen. Recorded.

---

## Seeds

*(DRAFT — finalized after approval)*

**`/goal`** — recommended.

```
/goal All NON-OPTIONAL phases marked ✓ in implementations-plan/pre-release-polish/plan.md (the per-phase headers in the file, not just the chat), each ✓ backed by its phase's validation gate as written in plan.md reported passing in the transcript; Phase 7's combined re-run green; for each phase the agent has printed `LESSONS_FILE=implementations-plan/pre-release-polish/lessons/phase-N.md` in the transcript; decision-allow-once.md exists and records the F-014 reversal, the retracted rate-limit argument, the disclosure line, and the owner's explicit risk acceptance; `/code-review max --fix` complete with findings applied and committed; codex post-impl audit complete with high/critical findings addressed; `bun run test` and `bun run lint:actions` both report exit 0 in the transcript. ASK-3 and ASK-4 must be answered by the owner — never decide the copyright or publisher string autonomously; stop and ask.
```

**`/loop`** — fallback.

```
/loop 15m Drive implementations-plan/pre-release-polish forward. Never idle waiting for my input. Each firing:
1. **Reality check**: read plan.md and lessons/ (authoritative — not the chat); `git status`; `git log --oneline -5`. PR exists → `gh pr view --json statusCheckRollup` (no --watch), else `gh run list --branch $(git branch --show-current) --limit 1 --json status,databaseId`.
2. **Waiting on CI is fine** — confirm it progresses (`gh run watch <run-id>` up to 10 min; stuck past that → inspect logs, log as blocked). Use the wait to review the diff or strengthen tests.
3. **No task in hand?** Take the next pending step from plan.md. After each meaningful edit run `bun run test` for the touched packages. Commit → push.
4. **Stuck or facing a judgement call?** `/codex xhigh`, reach a defensible decision, act, log the consult + verdict in lessons/phase-N.md. Hard limits: never merge to main, never publish/deploy, never expand scope. **ASK-3 (copyright) and ASK-4 (publisher) are NOT delegable — stop and ask me.**
5. **Same step failed 5 times?** Stop; reassess with codex; continue down the agreed path.
6. **Phase green?** "Green" = THE PHASE'S GATE as written in plan.md. Run it, paste the result, mark ✓, file lessons, print `LESSONS_FILE=implementations-plan/pre-release-polish/lessons/phase-N.md`, advance.
7. **All non-optional phases ✓?** `/code-review max --fix` → commit separately → codex post-impl audit → address high/critical → wrap-up report → stop.

Order matters here and is not negotiable: Phase 1 (fixture) must be green on UNMODIFIED product code before Phase 3 (rename) starts — a synthetic same-name upgrade cannot validate a changed binary identity. Phase 2 (autostart self-heal) is a prerequisite for Phase 3, not a nice-to-have: without it every upgrading user silently loses autostart while the UI reports it as on.
```

---

## Open work (blocks the approval gate)

Three audits, three rejections. R3 records the corrections but two phases need redesign, not edits:

1. **Phase 1 (fixture) needs a real design.** `_e2e-updater-windows.yml` cannot be dispatched
   standalone — it takes `n-version` and consumes an N artifact built by its caller, and runs one
   mode per call. Options: give it a self-contained build path; or add a thin dispatchable wrapper
   workflow that builds N then calls it once per mode; or accept it only runs via the release caller
   and gate the rename on a manual Windows verification instead. **Unresolved.**
2. **Phase 2 (autostart self-heal) needs a safe design.** Disable→enable is unsafe
   (`crash_recovery.rs:75-101`, macOS `KeepAlive` stripping), and nothing coordinates with the
   updater's deliberate disarm or with a second instance's startup reconciliation. A targeted
   "rewrite the Run value / plist path in place, without going through the plugin's enable path"
   may be safer, but it has not been designed or reviewed. **Unresolved.**
3. Phase 4's gate must name jobs that (a) exist and (b) are triggered by accelerator path filters.
   `Release Smoke` and `Windows Build Smoke` qualify; `Production Build Smoke` does not.
4. `README.md:27` still tells macOS users to kill the old process name — add to the identity map.

Once 1 and 2 have designs, the plan needs a **fourth** audit round before the gate, because both are
new material no reviewer has seen.
