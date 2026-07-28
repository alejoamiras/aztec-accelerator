## 1. Adversarial / security review

Verdict: The UX diagnosis is plausible, but the security conclusion is not—the plan removes a real rate-limit and disguises that as equivalence.

- **High — The counter-argument is materially wrong.** Re-prompting on every later proof is exactly the audit’s rate-limit: it requires renewed user presence and intent. Its severity as UX friction proves it functions. Prompt fatigue may make it counterproductive, but that is an unmeasured behavioral hypothesis, not evidence that the control “does not function.”

- **High — An attacker gains durable capability from one ordinary click.** Today, absent the checkbox, they get only requests already awaiting that decision. Afterwards they can return on later visits, exploit a future XSS/dependency compromise, or retain access after domain ownership changes without another prompt. The authorization does not itself reveal witnesses, but it permanently authorizes any witness that origin later obtains.

- **High — `[Allow]` does not disclose permanence.** Removing “Always allow” while making plain “Allow” permanent weakens informed consent. At minimum the popup needs explicit copy such as “This site will remain approved until removed in Settings,” with a test pinning it. For unknown/unverified origins, a second confirmation is defensible.

- **Medium — The compensating controls are overstated.**
  - Full origin display is necessary, but humans parse long domains poorly and the registrable domain is not emphasized.
  - `VERIFIED_SITES.md` explicitly says the badge is **not a security guarantee**; DNS/site/extension compromise retains the badge.
  - Revocation is reactive and undiscoverable unless the user already suspects a problem; it is not “one click” from the popup.
  - The 700 ms guard prevents immediate click-steal, not social engineering, habituation, or delayed clicks.
  - The Host guard does nothing against a malicious browser origin legitimately contacting loopback.

- **Medium — “Always persists” is false on save failure.** `auth.rs` logs persistence errors and still returns `Ok(())`; the popup closes and the proof proceeds, but the next proof can prompt again.

## 2. Assumption attack

Verdict: Several facts are sound, but the rename’s claimed finite blast radius is disproven.

**Facts**

- **High — F8/F9 are overclaimed.** Tauri’s `CFBundleExecutable` handling proves immediate macOS restart only. An existing `~/Library/LaunchAgents/Aztec Accelerator.plist` retains N-1’s old executable path: startup skips `autolaunch.enable()` when the plist exists, and `enable_crash_recovery()` returns early when `KeepAlive` exists. Windows similarly leaves the autostart Run value pointing at the old executable, although its recovery task is rewritten.

- **Medium — F7 is not a finite list.** Other consequential consumers include cleanup process names, shell execution of a path containing a space, autostart launchers, and `UPDATER_TESTING.md`’s bundle invariant.

- **Low — F1, F2, F4, F5 and F12 check out.** F3 is supported by the local audit/plan, although the cited git object is absent from this checkout. The publisher fallback is source-backed, but its exact Apps & Features rendering remains unmeasured.

**Inferences**

- **High — I4 is false.** Besides built-debug, move/removal breaks stale launchers; simply changing `APP_CMD` to a spaced filename also fails because the workflow executes `$APP_CMD` unquoted.

- **Medium — I1 must remain a release gate on the owner’s Mac, not merely a note.**

- **Medium — I3 is unsafe.** License, category, and icon inputs alter bundling/signature inputs and can fail platform builds; “cannot break signing/updater” is not established.

- **Low — I2 is sound:** ACLs grant command names, not argument schemas.

**Asks**

- **High — Security acceptance is missing as an Ask.** D1 treats the owner’s UX choice as already dispositive despite reversing an audited property.

- **High — ASK-1 is silently answered by sequencing:** Phase 3 ships the rename before Phase 4 proves it.

- **Medium — ASK-3/4 are not safely blocked.** The loop seed says an agent should decide and act when stuck; legal publisher/copyright identity must not be delegated that way.

- **Low — ASK-2 conflicts with “all phases ✓” in the goal seed, implicitly selecting the optional scope.

## 3. Implementation critique

Verdict: D2 is better than retaining a wire parameter, but a full migration is cleaner and the file map misses release-critical work.

- **Medium — D2 is a half-migration.** A renderer-provided `false` is less privilege, so the “IPC trust boundary” rationale is misleading. Since no production core producer will emit ephemeral Allow, change the enum to `AuthDecision::Allow` and make persistence unconditional. Five test-site edits are cheap and encode the new invariant. Retain the field only if core deliberately supports another real ephemeral adapter—and then document and test that contract.

- **High — Add launcher migration and upgrade assertions.** Rewrite existing macOS LaunchAgent/Windows Run entries to the current executable while preserving enabled/disabled state; test the stored path after an old-name → new-name update.

- **High — The proposed Rust persistence test is unsafe and insufficient.** `auth_state_with_popup` uses default config, while `lock_mutate_save` writes the real home config. An in-memory assertion can pass even when disk save fails. Inject a temp config path/save sink and reload from disk.

- **Medium — Missing mechanics/files:** quote `"$APP_CMD"`; update new-name cleanup; remove the now-unused `isClickGuardActive` import; update `UPDATER_TESTING.md`; test that the checkbox is absent and permanence is disclosed.

- **Low — Reuse is otherwise respected:** config mutation, revocation UI, bridge button guard, arbiter, shared CSS, headless behavior, and capability names remain reusable.

## 4. Ordering

Verdict: `competing-outline.md` has the correct dependency principle, but neither ordering is executable or sufficient as written.

- **High — Fix the instrument before the rename.** Phase 4 is too late; a green synthetic same-name upgrade cannot validate the changed identity.

- **High — Both proposed gates are currently impossible:** `_e2e-updater-windows.yml` exposes only `workflow_call`, not `workflow_dispatch`. The release caller is dispatchable only from `main`, so it cannot provide the claimed branch gate without workflow changes.

- **Medium — Third option:** keep the hermetic synthetic fixture, but force N-1 to the old binary name and assert pre-update “old exists/new absent,” post-update “new exists/old absent,” launch succeeds, and autostart/recovery references point to N. Prefer a pinned real release when available, but this targeted fixture is smaller and directly observes the rename.

- **Recommended order:** security risk-acceptance gate → executable old-name fixture plus launcher migration → rename → metadata → authorization implementation after explicit consent wording.

reject (with blocking findings: the security rationale denies a real rate-limit, permanent consent is undisclosed, the rename leaves stale autostart paths, and the updater gate is non-representative and non-executable as written)