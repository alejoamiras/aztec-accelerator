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
