# Phase 1 — lessons + codex loop log

## Codex round 1 (design, before implementation)

Session `01a0143a`-successor, prompt: "can a Windows composed-proof E2E leg work on a hosted runner?"
Framed as facts/inferences with the four candidate resolutions, and asked explicitly for a third way.

**Verdict**: *"Don't add a bypass or change production yet — the unproven premise is that
`certutil -user -store Root` cannot see an inherited machine root; measure that first."*

**The correction that reframed the whole fork.** I had assumed `-user` proves LocalMachine invisibility.
It does not: Microsoft documents CurrentUser stores (except Personal) as INHERITING LocalMachine contents,
and CurrentUser `Root` is a *logical collection* containing a `.LocalMachine` physical store. So the shipped
app's existing predicate may already be satisfied by an anchor imported into LocalMachine\Root — which CI
*can* seed silently (the runner is admin), unlike CurrentUser\Root (consent dialog).
⇒ If true, the composed proof runs against the SHIPPED binary with **zero production change**.

**Folded (acted on immediately)**
- Wrote `spike-windows-trust.yml`, a throwaway probe against the REAL published 2.0.0 installer, measuring:
  baseline (predicate must not find the anchor pre-import) → import to LM Root → does
  `certutil -user -store Root <serial>` return 0 → does the app actually bind `:59834` → and a
  **Disallowed negative control** (LM Disallowed must make the gate REJECT; codex's own safety condition).
  HTTPS is probed WITHOUT `-SkipCertificateCheck`, so a 200 proves the OS itself chain-builds to the anchor —
  the same store set Chromium's verifier consumes. Deleted before merge.
- Codex's "a post-uninstall 'CN absent' assertion without a positive precondition is worthless" matches the
  discipline the uninstall leg already follows: every artifact asserted GONE is asserted PRESENT first, and
  the OS-trust-store assertion is deliberately ABSENT rather than vacuous (the leg asserts the certs
  *directory*, which `--generate-certs-only` arms for real).

**Rejected (with rationale — "reject over-engineering" is an explicit goal constraint)**
- **My own SPKI-pin idea**: codex is right to kill it. `--ignore-certificate-errors-spki-list` deliberately
  IGNORES verification errors — it proves possession of one TLS key, not genuine trust — and it does not help
  the app's own launch gate anyway.
- **Shipped signed-capability seam** (ship a verification key; CI supplies a signed short-lived token
  authorising a trust bypass): exact-byte honest, but leaves a PERMANENT bypass verifier in the shipped
  product plus a CI signing-key liability, and the run would no longer prove the production trust gate.
  Strictly worse than simply extending the predicate. Not built.
- **UI automation / SYSTEM writes / another session**: desktop- and localization-fragile; SYSTEM writes
  SYSTEM's own HKCU, i.e. the wrong store.
- **Raw `CertSerializeCertificateStoreElement` registry blob**: parked. Probably filtered from enumeration
  (CurrentUser Root drops entries absent from `ProtectedRoots`), and only worth measuring if the LM path
  fails AND positive-precondition uninstall coverage of the physical CurrentUser store becomes required.

**Carried forward if the spike says NO-GO** (would need the owner): extend the production predicate to
`(CU.Root || LM.Root) && !(CU.Disallowed || LM.Disallowed)`, preferring thumbprint over serial for identity,
and make the uninstaller NOT claim to remove administrator-managed LocalMachine trust — report it separately.

## Spike result — codex's hypothesis MEASURED and DISPROVEN (run 32144187291, 2026-08-18)

Probed the REAL published 2.0.0 Windows installer on `windows-latest`:

| Measurement | Value |
|---|---|
| `certutil -user -store Root <serial>` BEFORE any import | `-2146893807` (= `0x80092004` CRYPT_E_NOT_FOUND) — correct baseline |
| same query AFTER importing the CA into **LocalMachine\Root** | `-2146893807` — **still not found** |
| `USER_STORE_INHERITS_LOCALMACHINE` | **False** |
| `Get-ChildItem Cert:\CurrentUser\Root` sees the LM cert | **True** |
| `HTTPS_GATE_OPENED_WITH_LM_SEED_ONLY` (app binds `:59834`) | **False** |
| HTTP `/health` (sanity: the app itself runs fine) | `{"status":"ok","version":"2.0.0","bb_available":true,...}` |
| `HTTPS_GATE_OPEN_WHILE_LM_DISALLOWED` (negative control) | False |

**The precise finding** — sharper than either of us predicted: PowerShell's `Cert:\CurrentUser\Root`
provider DOES enumerate a LocalMachine-imported anchor (the logical-collection view codex cited), but
`certutil -user -store Root` — the exact call the shipped predicate makes (`trust/windows.rs:59-66,155-177`)
— does NOT. It opens the PHYSICAL CurrentUser store. So the inheritance is real at the CryptoAPI
logical-store layer and invisible at the tool layer the product happens to use. The shipped app therefore
keeps HTTPS closed with an LM-only seed, confirmed end-to-end (`:59834` never came up while `:59833` served
`/health` normally).

**Verdict: NO-GO for a zero-production-change composed proof.** This is exactly the goal's
"STOP and surface" trigger, so it goes to the owner rather than being descoped silently. Note the spike also
proved the negative control cheaply, and — usefully — that the shipped 2.0.0 app installs, runs, and serves
`/health` on a hosted Windows runner with no virtual display, which de-risks the rest of the Windows work.

## Codex round 2 (after the uninstall leg's first green run)

Session `01a0152c`. Verdict: *"Not yet trustworthy as a proof — likely product success, but several
assertions can still false-green."* Exactly the right finding for a leg whose entire value is that it can
FAIL when the product is broken. Folded all six:

| # | Finding | Fix |
|---|---|---|
| High | **Task absence was fail-open** — any non-zero `schtasks /Query` counted as "gone", so access-denied or a scheduler fault passed with the task intact | exit **1** is the only "does not exist" answer; every other non-zero is now an explicit error. (The product's own code draws the same distinction.) |
| High | **Task precondition proved only a NAME existed**, not that it was ours | query `/XML` and require it to reference the installed exe |
| High | **"Uninstalled while running" was unproven** — health was sampled earlier, so an app that crashed in between would let a broken running-app teardown pass | assert `-not $proc.HasExited` immediately before the uninstall, and assert THAT process is gone after |
| Medium | installer/uninstaller **exit codes ignored** — either can fail after enough side effects for the filesystem assertions to pass | check `.ExitCode` after both |
| Medium | **config "survival" permitted corruption** — `Test-Path` passes on a truncated or rewritten file | SHA-256 before/after |
| Medium | **Run-value absence was fail-open** — `-EA SilentlyContinue` maps a read failure to `$null` | enumerate value names via `Get-Item`, so a real error throws |

**Deleted on codex's advice** (it was evidence-shaped, not evidence): the `Start-Sleep 75` + "did anything
come back?" check, and its contract-test pin. A missed scheduled start can be delayed by minutes, so no
bounded wait proves the absence of a relaunch. The relaunch trigger is proven dead by asserting the TASK is
gone — which is now a fail-closed assertion. Runtime drops by 75s as a side effect.

**Rejected, with reasons**
- **Foreign-ownership case** (seed a Run value pointing at another exe, expect PRESERVED). Codex said no and
  I agree: it doubles this leg's runtime, and the branch is already covered by the ownership classifier's
  unit tests, the real-OS `uninstall_ownership` test, and the Wine execution of the native foreign belt.
  This leg's unique signal is the composed ConfirmedOurs teardown.
- **Deleting the 20s heal poll** (codex: reconciliation completes synchronously before the server binds).
  Kept — `accelerator.yml:770-773` polls for exactly this value "to be robust" despite making the same
  ordering argument, and a poll that normally exits on its first iteration costs nothing. Not worth
  contradicting working prior art to save zero seconds.

**Also folded**: Defender exclusion narrowed from all of `%LOCALAPPDATA%` to the exact install directory.

## Codex round 3 (resumed session — judge the result, not the prior reasoning)

Verdict: *"Findings 1-8 are substantially closed; no remaining HIGH findings, but one MEDIUM process-oracle
gap remains."* Both remaining findings folded:

- **MEDIUM — the exact-PID oracle was too narrow, in BOTH directions.** Microsoft documents that
  `schtasks /delete` does **not** interrupt a program the task already started, so a task-spawned instance
  can outlive both the task and our original process and sail past a pid-only postcondition (false green).
  Symmetrically, before the uninstall the armed recovery task may legitimately start a second instance that
  wins the port race, letting our original process bow out benignly (false red). Both sides are now
  by-NAME: "at least one AztecAccelerator process exists" before, "none" after. This is the assertion that
  task-absence genuinely does not subsume — confirmed-absent task kills FUTURE triggers, not a running one.
- **LOW — the task binding was weaker than its own message claimed.** Substring-matching the whole XML would
  also match the path appearing in an argument or description, so "bound to our exe" overstated it. Now
  parses the XML and compares `Task.Actions.Exec.Command` to the installed exe, failing closed if the XML
  will not parse (a silent fallback to substring matching would reintroduce exactly the fail-open pattern
  round 2 removed).

Codex also confirmed, against docs, the things I had reasoned about but not verified: `$LASTEXITCODE` is
intact after `| Out-Null`; `GetValueNames()` on the `RegistryKey` from `Get-Item` throws on read failure
(which is the point); `.ExitCode` is valid after a timed `WaitForExit` returns true; and `Refresh()` was
redundant because `HasExited` self-updates — deleted. It re-confirmed that deleting the 75s resurrection
wait was right.

## Spike 2 — the LAST inherited assumption, now measured (run 32148985152)

The P2 blocker rested on two claims. Spike 1 measured one (LocalMachine is invisible to the predicate). The
other — "CurrentUser\Root cannot be written headlessly" — was INHERITED from `tests/trust_windows.rs:1-9`
and a comment in `updater-smoke-windows.ps1:156-161`, never measured in this environment. Since the entire
"needs an owner decision" conclusion rested on it, it had to be tested, not cited. All three seeding paths,
each with a hard 45s timeout so a hang costs seconds:

| Path | Outcome | Predicate (`certutil -user -store Root <serial>`) sees it? |
|---|---|---|
| `certutil -user -addstore Root` — the EXACT call the product makes | **HUNG** (killed at 45s) | No |
| `Import-Certificate -CertStoreLocation Cert:\CurrentUser\Root` | **completed, reported "ok"** | **No** |
| `.NET X509Store("Root","CurrentUser").Add()` | **HUNG** (killed at 45s) | No |

**VERDICT: NO-GO — no headless path seeds the store the shipped predicate reads.**

The `Import-Certificate` row is the interesting one and sharpens the earlier finding: it does not hang and
reports success, yet the anchor never becomes visible to `certutil -user -store Root`. That is
protected-root filtering exactly as `v2-release-train/plan.md:241-270` predicted — a write that "succeeds"
without landing in the physical store the product queries. Anyone re-attempting this should note that a
green-looking `Import-Certificate` is NOT evidence the seed worked; only the predicate query is.

Both blocker claims are now first-hand measurements against the SHIPPED 2.0.0 binary on the exact runner
image CI uses. The composed proof is genuinely infeasible without a production change, which is the goal's
explicit STOP-and-surface condition rather than something to descope quietly.

## Codex round 5 — loop CLOSED

Verdict: *"terminate — the fixes are correct; **no remaining findings**."* The round-4 collection fix is
sound (`@(...)` normalises zero/one/many; count + non-blank + exact-path all fail closed).

Two answers worth keeping, both decided on mechanism rather than deference:
- **Reverting the installer poll was right.** `/S` suppresses UI; it does NOT make NSIS extraction
  asynchronous. Sections execute inside the installer process and this per-user installer has no elevation
  handoff, so once `WaitForExit` returns true with exit code 0 the files exist. The UNINSTALLER is the
  genuinely different case — it is documented to run from a temp copy — which is why its 60s settle loop
  stays. So the two loops are not inconsistent: one guards a real documented behaviour, the other guarded a
  mechanism that does not exist.
- **No script-level version validation.** Artifact identity belongs to the CALLERS: the smoke pins its
  default tag, and the draft gate uses the validated release tag plus manifest-verified bytes. A real check
  would need an expected-version parameter, plumbing through both callers, and normalisation across tag,
  filename and embedded version; a filename-only check would buy almost nothing. Rejected as ceremony.

Loop summary: **5 rounds, 4 of which found something real** — 3 HIGH + 3 MEDIUM false-green paths (r2), a
two-way pid-oracle gap with a Microsoft citation (r3), a PowerShell collection-comparison trap (r4),
terminate (r5). Two of codex's suggestions were rejected with reasons (foreign-ownership case; deleting the
heal poll), and one of its deletions was adopted enthusiastically (the 75s resurrection wait).

## Windows-CI gotchas hit while building the uninstall leg

1. **The release pipeline refuses a non-main ref** — `release-accelerator.yml`'s `Assert main ref` step
   (F-005 / codex GATE-3 H4: gate all side effects on main) fails the run immediately, and it is the ONLY
   caller of `_e2e-packaged.yml`. Consequence: a new leg in that workflow cannot be exercised at all until it
   is merged, at which point it is already release-blocking. Fix: extract the leg body into
   `.github/scripts/packaged-e2e-uninstall-windows.ps1` and give a branch-scoped probe workflow a second entry
   point into the SAME script (against the already-published installer). The gate is not weakened — the
   script is what ships, so the probe cannot drift from it.
2. **A Tauri GUI-subsystem binary returns before its writes land.** `--generate-certs-only` printed
   `generated CA + leaf at ...ca.pem`, yet a `Test-Path` 190ms later still failed: the certs land via a
   staged write + atomic rename after the call detaches. Poll with a deadline, never test once.
3. **PowerShell 7.4+ turns native non-zero exits into terminating errors.**
   `$PSNativeCommandUseErrorActionPreference` defaults to `$true`, so under `$ErrorActionPreference = "Stop"`
   a `schtasks /Delete` on a task that does not exist YET (the pre-arm cleanup) throws and kills the script
   with no message of its own — it looked like a silent `exit 1` right after the certs step. This script
   deliberately shells out to tools whose non-zero exit IS the answer (`schtasks /Query` absent = non-zero,
   which the postconditions read), so it sets `$PSNativeCommandUseErrorActionPreference = $false` and decides
   on `$LASTEXITCODE` explicitly — the same discipline the rest of the repo's pwsh uses. Related known
   gotcha, already documented in `accelerator.yml:786-790`: Actions appends `exit $LASTEXITCODE` to pwsh
   steps, so a trailing native non-zero fails an otherwise-passing step.

4. **The `Run` KEY itself does not exist on a fresh runner profile** — `Set-ItemProperty` fails with
   "Cannot find path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run' because it does not exist".
   `accelerator.yml`'s windows-build gets away with a bare `Set-ItemProperty` because earlier steps in that
   job have already caused the key to exist. Create it with `New-Item -Force` first (a no-op on a real
   user's machine, where it always exists).

5. **`inputs.*` is EMPTY on a push trigger, and `gh release download ""` does not fail** — it silently
   resolves to whatever GitHub calls "Latest". This repo deliberately never updates that badge (every
   release is published `--latest=false`; the signed S3 feed is the source of truth), so the empty tag
   resolved to a **1.0.7** installer, whose binary still carries the pre-rename `aztec-accelerator.exe`
   name — and the run failed hunting for `AztecAccelerator.exe`. Give any input a literal fallback
   (`${{ inputs.x || 'default' }}`) whenever a workflow has more than one trigger.

### A wrong diagnosis, corrected (worth more than the bug)

The first time that failure appeared I concluded it was a filesystem race — "the installer exited 0 but the
exe had not materialised yet" — and added a 60-second poll, reasoning by analogy to the genuine
`--generate-certs-only` race (gotcha 2). The analogy was plausible, self-consistent, and **wrong**. The next
run's log settled it in one line: it was installing `Aztec-Accelerator-1.0.7-...exe`. There was never a race;
the wrong installer was being fed in, so the file could never appear. The poll has been reverted (its comment
asserted an observation that never happened — a false claim in the codebase is worse than the missing
guard), keeping only the directory dump, which is what made the real cause visible.

Two takeaways. First: three consecutive green runs did NOT mean the harness was sound — the very next run
failed on a path those three never exercised, so repetition builds confidence in the thing you varied, not
in the thing you didn't. Second: a diagnosis that explains the symptom is not the same as a diagnosis
supported by evidence; the cheap move (read one more line of the log) beat the plausible one (ship a fix for
the mechanism I had recently been burned by).

### 3-strike reassessment (after gotchas 2, 3 and 4 all landed in the same arming preamble)

Three consecutive failures, three DIFFERENT root causes, all in the same ~15 lines — and all of the same
species: environment assumptions that hold on a developer's Windows box but not on a fresh hosted runner.
The process fault was fixing them one at a time (each cycle ~4 minutes of CI to learn one fact). Adaptation:
after the third, audit the WHOLE script for the same class of assumption before pushing again — every native
command whose non-zero exit is expected, every registry path assumed to pre-exist, every filesystem write
assumed to be synchronous. That audit found no further instances (the remaining calls already use
`-ErrorAction SilentlyContinue` or read `$LASTEXITCODE` explicitly). Worth remembering for the composed-proof
leg, which will add a browser and a second listener to the same runner.

## Harness notes

- `gh workflow run` cannot dispatch a `workflow_dispatch` workflow that exists only on a feature branch (404:
  "not found on the default branch"). A throwaway probe should use a branch-scoped `push:` trigger instead of
  polluting main.
- commitlint here rejects `subject-case` (no leading capital: "Windows …" fails, "add the windows …" passes)
  and long body lines. Write the message to a file, keep body lines under 100 chars.
- `intent_enabled_now()` (`autostart.rs:1486-1491`) reads the **OS artifact** (the Run-key value), not a
  config field — so seeding that value IS how CI expresses "the user enabled Start-on-Login", and the product
  then derives the healed value + the crash-recovery task itself. That is why the uninstall leg's arming is
  product-written rather than CI-faked.
