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
