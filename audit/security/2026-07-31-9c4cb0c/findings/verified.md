# Phase 4 — independent verification (run `9c4cb0c`)

**Method.** For each finding below I read the cited source *before* re-reading the finding's claim,
formed my own conclusion about mechanism and impact, then compared. Every `file:line` in this
document was opened and checked by me at commit `9c4cb0c`. Where the consolidated report's citation
is wrong or stale it is corrected inline. Where the claim over- or under-states, that is stated
plainly.

**Scope.** All 2 High (F-01, F-02) plus 3 Medium (F-03, F-04, F-08). Selection rationale for the
Mediums is in *§ Medium selection*. The 8 findings not verified are listed at the end.

---

## Medium selection — why these three

I did not take the suggested trio wholesale. Criterion applied: **largest gap between claimed impact
and verifiable evidence**, weighted by whether a verifier can actually close the gap.

- **F-03** — chosen. It asserts three sinks off one root cause and stakes a High-adjacent CVSS
  (6.8, `VI:H/VA:H/SC:H`) on them. Sink B (`commit_launch_floor`) is a pure-logic claim that either
  survives or collapses under ten minutes of reading, and its collapse would move the band. Highest
  expected information.
- **F-04** — chosen. It makes the single strongest empirical claim in the corpus: *permanent, silent,
  and reachable **with no attacker at all***. If true it is the most actionable item in the report; if
  false it is the most misleading. Binary and checkable.
- **F-08** — chosen over F-12 and F-06. F-12 is already flagged in the report itself as a known
  accepted risk (arc-bug-hunt round 7), so verification adds little the owner does not have; F-06
  self-declares its impact as contingent on an unaudited premise, so there is no gap for me to close.
  F-08 by contrast asserts two things as "**verified**" (Drop-only witness deletion; a *provably
  dead* stderr control), is claude-only with an explicit codex non-finding against it, and its
  claimed consequence ("a directory of plaintext witnesses" accumulating "over months") is a
  magnitude claim I can test.

**F-12 check (per brief).** Confirmed as a documented, consciously-accepted decision, not a missed
bug: `implementations-plan/arc-bug-hunt/log.md:301-313` (round 7 close-out) states verbatim *"A false
ACCEPT needs a spoofed `APPDIR=/` (which already implies control of our launch environment) or
genuine containment. No widening warranted."* The consolidated report already flags this correctly.

**A second finding is in that category and the report does NOT say so — see F-01 below.** That is the
most important correction in this document.

---

## F-2026-07-31-01 — SDK hands the witness to an unauthenticated endpoint, plaintext by default

**Verdict: PARTIALLY CONFIRMED.** Every mechanical claim is true. The *framing* is wrong in two
places — one that would have the owner re-litigate a settled decision, one that hides the strongest
version of the argument. Facet B does not belong in a High.

### My independent reading

Verified end to end, in the order the witness travels:

- `packages/sdk/src/lib/accelerator-transport.ts:174` — `constructor(host, port, httpsPort, httpsOnly = false)`.
  Default is plaintext-capable. Confirmed.
- `packages/sdk/src/lib/accelerator-prover.ts:126-127` — `httpsOnly ?? (envHttpsOnly === "1" || envHttpsOnly?.toLowerCase() === "true")`.
  With no option and no env var this is `false`. Confirmed.
- `accelerator-transport.ts:52-56` — `isRecognizedHealthBody` is `b.status === "ok" && b.api_version === 1`.
  That is the **entire** identity test. Confirmed, and the code says so itself at `:49-50`
  (*"Field-presence is NOT identity"*).
- `accelerator-transport.ts:315-319` — in non-strict mode a plaintext `http://${host}:${port}/health`
  is constructed and fired. Confirmed.
- `accelerator-transport.ts:333-335` `#isHealthy` = `response.ok && isRecognizedHealthBody(body)`;
  `:372-392` winner selection. I traced the race myself: a healthy HTTP that settles first yields at
  most `HTTPS_GRACE_MS = 250` (`:19`) to HTTPS. **The report's parenthetical is right and matters** —
  when nothing is bound on `:59834` the HTTPS probe rejects in ~0 ms, so the grace is never paid and
  plaintext wins with no delay.
- Sink: `accelerator-prover.ts:362` `serializePrivateExecutionSteps` → `:381` `postProve` →
  `accelerator-transport.ts:455` `ky.post(body)`. Confirmed.

**Where I disagree with the finding, point by point:**

1. **"That posture is not covered by any accepted residual" — FALSE, and this is the material
   correction.** The plaintext-by-default posture is a *recorded, gate-approved* risk decision:
   - `implementations-plan/https-by-default-onboarding-2026-07-09/plan.md:18` — *"default mode is
     UX/positioning parity … HTTPS-preferred only gains real impostor-resistance for dApps that set
     `httpsOnly` … a same-user attacker is still game-over (recorded SEC-04 threat model)."*
   - same file `:131` (S7) — *"`httpsOnly` gives strict dApps no-fallback; **default mode's loopback
     MITM is already local-game-over**."*
   - `implementations-plan/https-by-default-onboarding-2026-07-09/audit-review.md:17` — the squatter
     case was raised at audit time and closed as *"past the recorded SEC-04 line."*
   The finding therefore is **not** an unnoticed gap in F-001's remediation. It is a challenge to a
   decision the owner already took with their eyes open. That is still a legitimate thing for an
   audit to do — but it must be labelled as a *re-take*, not as a discovery, or the owner will
   re-derive the same reasoning and dismiss it.

2. **The finding argues the weak case and misses the strong one.** It frames the attacker
   exclusively as a same-user local process — precisely the actor the accepted residual dismisses.
   The argument that actually defeats SEC-04 is one neither the report nor the plan makes:
   **any local user can bind `127.0.0.1:59833`.** Loopback is not owner-scoped on Linux, macOS or
   Windows. On a shared/multi-user host, user B squats the port and receives user A's witness in
   cleartext — a **cross-user** compromise, not a same-user one. The project's own threat model
   already admits multi-tenant hosts: prior F-003 was rated on exactly that basis (*"readable by any
   other local user during proving on a default-umask multi-tenant host"*). And the plan's own
   statement of what HTTPS buys — *"an encrypted, authenticated loopback channel that a **different
   non-admin local user** can no longer trivially impersonate"* (`plan.md:18`) — is the description
   of a control that is **off in the shipped default path**. That is the sentence that should carry
   this finding.

3. **Facet B (`host` unvalidated) does not belong in a High — it is a hardening nit.** The claim is
   mechanically true: `host` is interpolated raw into six templates (`accelerator-transport.ts:255`,
   `:257`, `:302`, `:318`, `:420-421`, `:439-441`) with no `new URL()`, so
   `host = "127.0.0.1@attacker.tld"` yields an authority of `attacker.tld`. But I checked the
   ingress and **there is no ambient path to set it**: `host` comes only from
   `AcceleratorProverOptions.accelerator.host` (`accelerator-prover.ts:107`) or
   `setAcceleratorConfig` (`:137-140`), i.e. the integrating dApp's own code. Unlike `port` /
   `httpsPort` / `httpsOnly`, there is **no `AZTEC_ACCELERATOR_HOST` env var** (`:111-116` — I
   checked; only three are read). A dApp that pipes untrusted input into `host` has already lost.
   Real defence-in-depth defect, **Low**, and it inflates the High if left bundled.

4. **The "demotion re-probe re-uses the same forgeable marker" instance is true but is not a second
   root cause.** `accelerator-prover.ts:417-433` calls `isProtocolHealthy("http")`
   (`accelerator-transport.ts:415-434`) before retrying, and that helper applies exactly the same
   `isRecognizedHealthBody` check. So it inherits the weakness — but it was *added* to close a
   codex Critical (*"a foreign responder there receives the serialized witness the instant HTTPS
   fails"*), and it strictly improves on no check at all. Listing it as a distinct instance
   overstates the count of independent defects.

### Severity

**HIGH survives — but on the multi-user argument, not the one given.** Restricted to the same-user
actor the report models, this is Medium at best under the report's own corollary 2, and it is
already an accepted residual. Elevated to cross-user, it is an unauthenticated cleartext transfer of
the single most sensitive object the system handles, across a real privilege boundary, with no
precondition beyond "another account exists on this machine." I keep HIGH. The CVSS `AV:L/PR:N` is
consistent with that reading; `SC:H` is justified.

One scope narrowing the report should have made: with the accelerator **installed, running, and
HTTPS-enabled** (the new default posture after onboarding), a plaintext squatter on `:59833` *loses*
— a healthy HTTPS beats a healthy HTTP at `accelerator-transport.ts:383-390`. The exposure window is
(a) accelerator absent, (b) accelerator present with HTTPS off/unhealthy, or (c) the real app evicted
— which is **exactly F-03 sink A**. The F-01 ↔ F-03 composition is the sharp end and both findings
under-sell it.

### Refined smallest-safe fix

- **Do not flip `httpsOnly` to true by default as step 1.** It fails closed into a WASM fallback for
  every user whose accelerator has HTTPS off, and it does not fix the case where the attacker holds
  `:59834` with a browser-trusted cert. Ship instead:
  1. **A per-install token.** Written to `~/.aztec-accelerator/` at `0600` by the app, echoed in the
     detailed `/health` branch (which is already Origin-tiered, so it is not handed to unapproved
     origins). This is the fix F-001 actually asked for, it closes the cross-user case completely
     (mode `0600` is what makes it cross-user-safe), and it composes with F-03's sink A — the same
     token authenticates the incumbent. **This is the one fix that closes both.**
  2. **Parse, don't interpolate.** `new URL()` per endpoint; reject a `host` that is not a loopback
     literal unless `allowRemoteHost` is set. Prefix matching (`startsWith("127.")`) is bypassable by
     the userinfo form — the fix must parse, as the report says.
  3. Only then consider the default flip, as a separate, versioned decision.
- Re-open the SEC-04/S7 residual explicitly in `implementations-plan/`, with the multi-user framing
  recorded, so the next reviewer inherits the corrected reasoning rather than the 2026-07-09 one.

### Confidence

**High.** Trace verified end to end in source; the accepted-residual status verified in the plan
record; the `host` ingress surface enumerated exhaustively.

---

## F-2026-07-31-02 — `install_ca_trust()` installs an unvalidated, unscoped anchor into OS root stores

**Verdict: CONFIRMED**, with one citation corrected and the platform scoping tightened.

### My independent reading

- `packages/accelerator/src-tauri/src/certs.rs:145-152` — `certs_exist()` is exactly four
  predicates: `CertPaths::live().exists()`, `leaf_secs_remaining() > 0`, `load_rustls_config().is_ok()`,
  `leaf_matches_ca()`. I agree with the finding's central observation and reached it independently:
  **all four are properties of an internally consistent set, none is a property of *our* set.**
  An attacker with `openssl` satisfies all four.
- `certs.rs:162-185` — `leaf_matches_ca()` is `leaf.verify_signature(Some(ca.public_key()))`. It
  proves self-consistency. Its own doc (`:156-161`) says its purpose is detecting a torn `swap_into`.
  Not a provenance check. Confirmed.
- `certs.rs:95-115` — `ca_params()` is where `IsCa::Ca`, `KeyCertSign|CrlSign` and the loopback
  `NameConstraints` live. These are invariants of **`write_new_cert_set`**, not of the file at
  `CertPaths::live().ca_cert`. Nothing re-asserts them on the adoption path. Confirmed — this is the
  crux and it is correctly identified.
- `certs.rs:228-233` — `generate_and_save()` returns `Ok(())` without writing when `certs_exist()`.
  (Report says `:228-234`; the function ends at `:234`. Immaterial.)
- `certs.rs:440-452` — `install_ca_trust()` passes `CertPaths::live().ca_cert` straight to the
  backend. No parse, no validation. Confirmed.
- **Gate: `packages/accelerator/src-tauri/src/commands.rs:392-394`** — verbatim
  `if !(certs::certs_exist() && certs::is_ca_trusted()) { generate_and_save()?; install_ca_trust()?; }`.
  Citation exact. A planted set makes the first conjunct true and the second false, so the branch is
  taken, generation no-ops, and the planted CA is installed.
- `trust/macos.rs:24-39` — `security add-trusted-cert -r trustRoot -k <login.keychain-db> <cert>`.
  **No `-p`.** Confirmed. A trust-settings dictionary with no policy constraint applies to all
  policies. Same at `:161-168` (`trust_new_anchor`).
- `trust/windows.rs:59-66` — `certutil -user -addstore Root <cert>`. **No EKU property set.**
  Confirmed; a root with no EKU constraint in the CurrentUser store is trusted for all purposes.
- `trust/linux.rs:282-300` — `certutil -A -t "C,," …`. Confirmed: SSL-CA only. Linux is the correct
  implementation and its existence proves the narrower grant is achievable, exactly as argued.
- `trust/mod.rs:16-18` — verbatim *"Name constraints on the anchor are defense-in-depth; the
  load-bearing control is the keyless CA (it can sign nothing), so a trusted anchor in any store is
  harmless."* Confirmed present, and confirmed **false for an adopted anchor**, which has a key by
  construction. This comment is the reason the gap survived review.
- `commands.rs:520-531` + `get_onboarding_state` — `https_default: true` unconditionally, documented
  as A9/§2.1 ("pre-checked for everyone incl. upgraders"). Confirmed.

**Reachability, checked independently.** I verified the launch path does **not** install trust:
`main.rs:64-67` states *"Trust is only ever VERIFIED at launch … never installed — launch must never
raise the macOS Keychain prompt"*, and `classify_launch_https` (`main.rs:68-82`) only
verifies. So `UI:A` is correct — the user must run the wizard or the Settings toggle. **But** I found
an inducement the report missed: with planted certs present and untrusted, launch takes
`LaunchHttpsGate::UntrustedSkip` (`main.rs:114-117`) and HTTPS silently stops working, which is
precisely the condition that sends a user to Settings to re-enable — i.e. the attacker can *prompt*
the ceremony. That strengthens reachability.

**Citation correction — the rotation instance is overstated.** The report lists
`certs.rs:412-434` as having "the same gap on staged files." It does not. `rotate()` calls
`write_new_cert_set(&staged)` at `:417` and hands `&staged.ca_cert` to `trust_new_anchor` at `:420` —
the staged files are freshly minted three lines earlier. Substituting them requires winning a TOCTOU
race between `:417` and `:420`, which is a materially different (and much narrower) defect than the
`certs_exist()` adoption gap. Keep it as a hardening note, not as a co-equal instance.

### Severity

**HIGH survives, with the platform scoping tightened.** The impact is not "a trusted local CA" — it
is **a browser-trusted root CA whose private key the attacker holds and which need carry no name
constraints at all**, i.e. universal TLS interception for every site in that user's browsers (and,
via Firefox's enterprise-roots import on macOS/Windows, in Firefox too). That is unambiguously
crossing into another trust domain, so it passes corollary 2.

Two honest narrowings the report should carry:

- **macOS: the laundering argument is strong (high confidence).** `security add-trusted-cert` for the
  user trust domain raises an authentication prompt the attacker cannot satisfy alone. What the app
  launders is the *user's expectation* of that prompt — the wizard has told them to expect it. Genuine
  escalation.
- **Windows: weaker than claimed (moderate confidence).** The `certutil -addstore Root` warning is a
  UI-layer control; a same-user process can write the CurrentUser root store directly via
  `HKCU\Software\Microsoft\SystemCertificates\Root\Certificates` without any dialog — a
  long-documented malware technique. The marginal gain there is closer to Linux's ("near zero") than
  to macOS's. The report's "macOS **and** Windows" should be "macOS, and to a lesser degree Windows."
- The **purpose-scoping half is platform-independent, true for the honest anchor, and cheap.** It is
  the part I would ship first.

### Refined smallest-safe fix

Unchanged in shape from the report, with two adjustments:

1. One `validate_our_anchor(der) -> Result<()>` using the already-vendored `x509-parser` (same crate
   `leaf_matches_ca` uses at `certs.rs:170-181`, so no new dep): assert `basicConstraints CA=true`
   + `pathLen`, `keyUsage == keyCertSign|cRLSign`, `CN == "Aztec Accelerator Local CA"`, and a
   **critical** `nameConstraints` whose permitted subtrees are exactly the three from
   `ca_params()`. Call it inside `install_ca_trust()` at `certs.rs:441` — **not** at the
   `certs_exist()` layer, so the one call site guards every backend. Fail closed by deleting the set
   and regenerating.
2. Apply the **create-then-verify ordering** already used by `win_acl.rs` and by
   `windows.rs:135` (`add_store(ca_cert) && live_present(ca_cert)`): validate the exact bytes you are
   about to hand to the OS, ideally from a single read, so the rotation TOCTOU at `:417-420` closes
   with the same code.
3. `-p ssl` on macOS; set the EKU property on Windows. Mirror `linux.rs:284`'s `-t "C,,"`.
4. Add the planted-set rejection test next to `generation_writes_no_ca_key` (`certs.rs:679-718`).

### Confidence

**High.** Every instance opened and confirmed; the gate line-exact; reachability independently
traced through the launch classifier. Moderate confidence only on the Windows prompt-bypass
refinement, which I flag as such.

---

## F-2026-07-31-03 — forgeable `/health` as process identity (3 sinks)

**Verdict: PARTIALLY CONFIRMED — one sink is real, one is very close to impact-free, one is
marginal.** The composite framing ("the anti-rollback control shipped for one prior finding is gated
on the forgeable signal of another") is architecturally accurate and rhetorically effective, and it
carries a CVSS the individual sinks do not support.

### My independent reading

**Root cause — CONFIRMED.** `packages/accelerator/core/src/server/probe.rs:14-17`:
`is_healthy_aztec_response` is `status=="ok" && api_version==1`. Both constants are part of the
public `/health` contract. Its doc at `:11-13` claims it stops "an arbitrary process answering on
:59833"; it does not. Single fix point, correctly identified.

**Sink A (Windows bow-out) — CONFIRMED, and stronger than the report argues.**
`main.rs:294-303`: `addr_in_use && cfg!(target_os = "windows") && healthy_aztec_on_port()` →
`app_handle.exit(0)`. Exit code 0 is deliberate (`:290-292`) so the supervisor does not retry.
The report describes the gain as stealth. I think the real gain is larger and it should be stated:
**the eviction is what defeats the HTTPS preference.** If the genuine app stays resident it still
holds `:59834`, and the SDK's prefer-healthy-HTTPS selection (`accelerator-transport.ts:383-390`)
routes the witness to the *real* app — the squatter gets nothing. `exit(0)` kills the HTTPS listener
too, and only then does F-01 pay out. Sink A is the enabler for the F-01 exposure window on Windows,
which is the single most important sentence in either finding and neither says it.

`bind.rs:30` — plain `TcpListener::bind`, no socket options, no post-bind ownership assertion.
Confirmed. The `SO_EXCLUSIVEADDRUSE` refinement is correct and I would raise it from the report's
"moderate" to **high** confidence on mechanism: Windows permits a second `SO_REUSEADDR` bind to steal
a socket whose owner did not set `SO_EXCLUSIVEADDRUSE`, restricted since XP SP2 to the same user
account — which is exactly the modelled actor.

**Sink B (floor ratchet) — SUBSTANTIALLY REFUTED on impact.** I traced this before reading the claim
and reached the opposite conclusion. `main.rs:340-360` probes
`healthy_aztec_version_on_port()` and compares to `want = env!("CARGO_PKG_VERSION")` (`:342`). On
three consecutive matches it calls `commit_launch_floor()`. Critically:
`updater.rs:79-101` commits **`env!("CARGO_PKG_VERSION")`** — *not* the probed value. The probe is a
gate, never a data source. So the most a hostile responder can achieve is to commit
`floor = current_running_version`. And `updater_state::candidate_allowed` already enforces
`gt(candidate, current)` **unconditionally, before it ever looks at the state file**
(`updater_state.rs:118-121`). A floor equal to `current` is therefore a **no-op for the update
gate**, and `running_below_floor(current, floor==current)` is `false` (`:140-141`), so it triggers no
lockout either. **The attacker gains nothing directly.**

The one residual composition, which the report does not make and which I would keep at Low: forcing
the commit for a build whose server is actually wedged means the floor is set for a broken version,
so a user who later downgrades to escape it trips F-04's `running_below_floor` lockout. That is a
chain into F-04, not an independent impact.

**Sink C (unbounded probe read) — CONFIRMED but marginal.** `probe.rs:41` and `:64` are
`resp.json::<serde_json::Value>()` with no size cap. (Report also cites `:68`; that line is
`body.get("version")` and is not a read — minor citation error.) The bound is the 3 s client timeout
(`:27-32`, `:56-59`), which on loopback still admits a multi-GB allocation. But the actor is a local
process that can allocate that memory itself, so under corollary 2 the marginal gain is ~zero — the
same test that dropped `c5-codex` §2 (unbounded TLS handshakes) from this report. Applying the
report's own rule consistently, sink C should have been dropped or filed as a bug, not carried as
part of a Medium.

### Severity

**MEDIUM survives, but only on sink A, and only on Windows.** CVSS 6.8 with `VI:H/VA:H/SC:H`
overstates: the integrity impact was carried by sink B, which I find impact-free, and the
availability impact by sink C, which is dominated by capabilities the actor has. Realistic band:
**Medium, ~5.0**, `AV:L/AC:L/PR:N/UI:N/VC:N/VI:L/VA:L/SC:H` — the `SC:H` earned entirely by enabling
F-01 on Windows.

The finding should be restructured as: *"Windows bow-out self-evicts on a forgeable signal, opening
the F-01 exposure window"*, with B and C demoted to notes.

### Refined smallest-safe fix

- **Sink A:** the report proposes an OS single-instance primitive (named mutex / lockfile). Correct,
  but there is a cheaper path that also fixes F-01: **reuse the per-install token from the F-01 fix.**
  The incumbent echoes it in `/health`; the redundant instance, which reads the same `0600` file,
  compares. One shared secret closes both findings — that is the composition F-001's original
  recommendation asked for and it is still the right answer.
  If a single-instance primitive is preferred anyway, pair it with `SO_EXCLUSIVEADDRUSE` on the
  Windows bind at `bind.rs:30` so the port cannot be stolen from under a live listener.
- **Sink B:** delete the probe. The floor tracker needs to know *this process's own listener came up
  healthy*, which is an in-process fact — a `tokio::sync::watch` set by the server task after a
  successful bind. The report's recommendation here is right, and it is now a **correctness/clarity**
  fix rather than a security one. Also fix the false invariant at `main.rs:336` (*"Matching the
  version proves we are observing our own server"*), which is the comment that made this look safe.
- **Sink C:** bound the read; mirror `readJsonBounded`'s 64 KB cap
  (`accelerator-transport.ts:26`). File as a bug.

### Confidence

**High** on all three traces. I am confident in the sink B refutation specifically — it turns on
`commit_launch_floor` using `env!()` rather than the probed value, which I verified at
`updater.rs:84` and `main.rs:342`.

---

## F-2026-07-31-04 — same-user-writable state gates security mechanisms; one write permanently kills the patch channel

**Verdict: CONFIRMED. Sink A is real, permanent, silent, and — as claimed — reachable with no
attacker at all.** I set out to refute the "permanently cut off from all future updates" claim,
because `candidate_allowed` looked like it would still admit a *higher* version. It does not, and the
reason is one line above it. This is the best finding in the report.

### My independent reading

**Sink A — CONFIRMED, mechanism exactly as described.**

- `updater_state.rs:118-123` — `candidate_allowed` returns `false` for `LoadedState::Corrupt`.
  Confirmed.
- `:140-142` — `running_below_floor(current, state)` is `gt(floor, current)`. Confirmed.
- `:153-159` (`record_pending`) and `:183-187` (`commit_successful_launch`) — **both** return
  `Err(InvalidData, "refusing to overwrite a corrupt version-floor state")`. Confirmed.
- I grepped the whole tree for any writer, remover or repairer of `updater-state.json`: the only
  references are `updater.rs:35` (path), the two refusing mutators, and `write_state` (`:205`), which
  is only ever reached *through* them. **Nothing repairs or deletes a corrupt file.** The
  permanence claim is verified, not inferred.
- Monotonic max at `:162-173` and `:189-198` — a poisoned high floor never comes down. Confirmed.

**My attempted refutation, and why it failed.** I expected a poisoned `floor: "999.0.0"` to block
only candidates `<= floor`, leaving a later legitimate `1.1.0` installable — which would have made
"cut off from *all* future updates" an overstatement. It is not, because of
**`packages/accelerator/src-tauri/src/updater.rs:132-149`**: `layer_b_gate` checks
`running_below_floor` **first** and returns `Err("running build is BELOW the version floor …
refusing all updates")` — a blanket refusal that never reaches `candidate_allowed`. Both the
check-time gate (`:184`) and the install-time re-check share it by construction (`:129-131`). So a
floor above the running version refuses **everything, unconditionally, forever**. The claim is
correct as written.

**The no-attacker path is real and I confirm it.** User on 1.0.9 (floor committed to 1.0.9 by the
launch tracker) downgrades to 1.0.7 from GitHub Releases — the normal response to a bad release.
`running_below_floor(1.0.7, floor 1.0.9)` is true. Every subsequent update is refused, including the
fix for whatever made them downgrade. Running 1.0.7 cannot lower the floor
(`commit_successful_launch` takes `max`). Nothing is surfaced in the UI — every path is
`tracing::error!`/`warn!` to a log file (`updater.rs:185`, `:98-99`). The user sees an app that
simply never updates. **This is a live, non-adversarial, silent-failure bug affecting real users, and
it is the single most actionable item in this report.**

**Sink A′ — CONFIRMED, and the exploit is cheaper than the report says.** `updater.rs:51-57`:
`OpenOptions::new()…open(parent.join("updater.lock")).ok()?` — the `.ok()?` discards the error and
returns `None` with **no log**, while genuine lock contention **is** logged at `:60-65`. Confirmed.
The report implies a permissions attack; the trivial version is `mkdir ~/.aztec-accelerator/updater.lock`
— `open()` then fails `EISDIR` forever, silently. Collateral confirmed: `commit_launch_floor` bails
at `:91-96`, and the autostart self-heal takes the same non-blocking lock (`:44-46`).

**Sink B — CONFIRMED as to mechanism, one sub-claim imprecise.** `update_marker.rs:141-168`
(`load`) validates schema, canonical SemVer and a future clamp — **no MAC, no owner/ACL check on
read**. Confirmed. The clamp is `parsed.deadline_unix > now_unix + MAX_FUTURE_SECS` (`:33`, `:164`)
with **no `created_unix` in the payload**, so a forger can set `now + 24h` and refresh indefinitely —
the report is right that the clamp caps one forgery, not a refreshing attacker. Confirmed.
*Imprecise:* the report says `read_token_nonce` reads the nonce "from the very file it would
authenticate" (`:297-302`). It reads `paths.token`, a **separate** NSIS-written file, not the marker.
The substance survives — both files are same-user-writable, so the nonce authenticates nothing — but
the sentence as written is wrong and should be corrected. Note also that a `Corrupt` marker *is*
self-healing (deleted + one-launch suppression, per the module doc at `:10-18`), unlike
`updater-state.json`; the asymmetry between these two state files is itself the tell.

**Sink C** — not independently verified beyond confirming the merge is coherent; see *Verification
not performed*.

**Survives uninstall — CONFIRMED.** `nsis/hooks.nsi:96-112`: inside the `$UpdateMode <> 1` and
`$0 != $1` guard, the hook runs exactly two actions — `certutil -delstore Root` and
`RMDir /r "$PROFILE\.aztec-accelerator\certs"`. `updater-state.json`, `updater.lock` and the marker
are all in `$PROFILE\.aztec-accelerator\` (not `\certs`) and are **not** touched. Verified by reading
the macro in full.

### Severity

**MEDIUM confirmed — and I would place it at the top of the band, above F-03.** It is the only
finding here that passes corollary 2 on *durability* with no argument needed: the poisoned state
outlives the malware, outlives uninstall+reinstall, and disables the mechanism by which any future
fix arrives. Add the non-adversarial trigger and the likelihood is not hypothetical. I would not
promote it to High — the impact is denial of future patches, not compromise — but it should be
sequenced **first** for remediation.

### Refined smallest-safe fix

The report's items 1 and 2 are correct and I endorse them without change; my verification
strengthens the argument for item 1:

1. **Fail closed to a provably-safe floor, not to nothing.** On `Corrupt` *and* on
   `running_below_floor`, fall back to `floor = current_running_version` rather than refusing all
   candidates. I verified the load-bearing premise myself: `candidate_allowed` enforces
   `gt(candidate, current)` **before consulting the state at all** (`updater_state.rs:118-121`), so
   the entire anti-rollback property that matters survives. The persisted floor's only marginal value
   over `current` is protection against an out-of-band binary downgrade — which requires an attacker
   who already owns the app files. **Ship this one first; it removes both the attack and the
   accidental lockout.**
2. Split the two `None`s at `updater.rs:51-57`; log the unopenable-lock case at `error`. Ten
   minutes.
3. Surface the state. A single line in Settings ("updates are currently blocked: …") converts a
   silent permanent failure into a support ticket. This is the cheapest half of the fix and the
   report under-weights it.
4. Sink B: add `created_unix` and bound `deadline <= created + DEADLINE_SECS + slack`; verify
   owner/ACL on **read** using the existing `win_acl.rs` machinery that already secures the write —
   this is the report's own cross-cutting observation #1 applied to its sharpest instance.
5. The MAC/DPAPI item is correct as the complete fix and correctly priced as the expensive one.
   Do not let it block items 1–3.

### Confidence

**High.** Mechanism, permanence and the no-attacker path each verified in source; the blanket-refusal
behaviour verified at `updater.rs:137-142`, which is the line the whole finding turns on.

---

## F-2026-07-31-08 — witness residue (RAII-only deletion) and dead `bb` stderr containment

**Verdict: PARTIALLY CONFIRMED.** Facet B's *dead-control* claim is not just confirmed, it is
provable from the dependency's own source — and I verified it there. Facet A's mechanism is
confirmed but its stated magnitude is not supported. Facet B's *security* impact is asserted, not
shown.

### My independent reading

**Facet B — CONFIRMED, definitively.** `packages/accelerator/core/src/bb.rs:206-229`: the
`tokio::process::Command` sets `args`, `env`, `kill_on_drop(true)` and **never calls `.stdout()` or
`.stderr()`**. I then verified the consequence directly in the vendored dependency rather than
inferring it: `tokio-1.52.3/src/process/mod.rs:1442-1445` states *"By default, stdin, stdout and
stderr are inherited from the parent. In order to capture the output into this `Output` it is
necessary to create new pipes"*, and the implementation at `:1457-1466` reads from
`self.stdout`/`self.stderr` only `if let Some(io)`, which are `None` under inherit. So
`output.stderr` is **unconditionally empty**.

Therefore `if !stderr.is_empty()` at `bb.rs:239` is never true, `truncate_stderr` (`:272-280`) is
**unreachable from production**, and the three tests at `:331-352` exercise it as a pure function —
giving exactly the false assurance the report describes. All confirmed.

**Where the report overstates facet B.** It files this under CWE-532 (information exposure through
logs) and asserts the child "writes verbatim to the inherited fd 2 — which on a Linux desktop
autostart is the session journal." Two problems:
1. Whether `bb` ever writes witness material to stderr is **not established anywhere**. The only
   support is the app's own defensive comment at `bb.rs:244-245` ("file paths, witness data"), which
   is a precaution, not an observation. The report's own honest counterweight already concedes the
   destination is `/dev/null` on macOS and non-existent on a Windows GUI build.
2. The *certain* consequence is the one the report buries: **the operator loses every byte of `bb`
   diagnostics**, on every platform, including the failure path at `:246-247` which logs only the exit
   code. That is a real defect with certain impact — it just is not a security defect.

**Facet A — CONFIRMED as to mechanism.** `bb.rs:82-113` `prove_tmp_parent()` is a **persistent**
directory under `dirs::data_local_dir()`. `:194-198` creates the workspace and writes the plaintext
witness (`write_witness`, `0600`/`secure_create_file`). `let tmp_dir = create_prove_tempdir()?` at
`:194` is never explicitly closed — deletion is `TempDir::drop` alone. Confirmed by reading the whole
function; there is no `close()` on any path.

Drop-skipping exits verified individually: `main.rs:301` (`app_handle.exit(0)`, the F-03 bow-out),
`main.rs:396` (tray Quit), `main.rs:504` (`std::process::exit(1)`), `updater.rs:542`
(`app.restart()`), `commands.rs:666` (`restart()`), plus the Windows `std::process::exit(0)` handoff
documented at `update_marker.rs:3`. All confirmed present. I also grepped for
`with_graceful_shutdown` across the server modules and for any startup sweep of `prove-tmp`: **there
is neither.** Both negative claims verified.

**Where the report overstates facet A.** "Over months this accumulates into a directory of plaintext
witnesses" is not supported. Residue is left **only** when the process dies *during* a proof — a
narrow window (seconds to a few minutes, a handful of times a day at most). Tray Quit, auto-update
restart, logout or OOM must land inside it. The realistic steady state is a small number of stale
directories, not a harvest. The security property — *a plaintext witness can persist indefinitely on
disk after the app exits* — is entirely real and worth fixing; the volume framing is not, and it is
the kind of rhetoric that gets a true finding dismissed.

**Marginal-gain test.** Facet A passes, but by a narrower margin than stated. The gain over the
modelled attacker is **temporal**: an attacker (or a backup, or a disk image, or the next owner of the
laptop) need not be resident during proving. That is the mirror image of durability and it counts.

### Severity

**Split the finding.**
- **Facet A: MEDIUM confirmed** (bottom of band). Real, cheap to fix, genuinely widens the exposure
  window in time. CVSS `VC:H` is defensible for the object; likelihood is the limiter.
- **Facet B: LOW as a security finding**, because the leak is unproven and platform-contingent —
  but **high-priority as a bug**, because a control the codebase believes it has does not run and all
  child diagnostics are silently discarded. Filing it at Medium under CWE-532 asserts a disclosure
  that was not demonstrated.

### Refined smallest-safe fix

- **Facet B — one line:** `cmd.stderr(Stdio::piped())` at `bb.rs:228`. This simultaneously revives
  `truncate_stderr` and the containment logic and restores diagnostics. `Stdio::null()` is the wrong
  choice — it keeps the control dead *and* keeps the operator blind. Add a test asserting the
  `bb.rs:240` warn line is emitted for a failing child (the current tests cannot catch this because
  they call the pure function directly).
- **Facet A:** an unconditional sweep of `prove_tmp_parent()` removing `prove-*` at startup — safe
  because the app is single-instance per port and the bind is the interlock — plus an explicit
  `tmp_dir.close()` on the success path so the common case does not depend on `Drop` at all. The
  existing eviction machinery in `versions/downloader.rs:252` is the pattern to mirror; the
  version cache already has reclamation and the witness workspace does not, which is the asymmetry
  worth naming in the commit message.

### Confidence

**High** for facet B (verified in the dependency's own source, not inferred). **High** for facet A's
mechanism and the two negative claims (no graceful shutdown, no reaper), **moderate** for realized
impact, which I judge smaller than the finding states.

---

## Severity changes

| Finding | Coordinator band | Verified band | Reason |
|---|---|---|---|
| F-01 | HIGH | **HIGH** (rationale replaced) | Band survives, but not for the stated reason. The same-user framing is an already-accepted residual (`https-by-default-onboarding-2026-07-09/plan.md:18,131`; `audit-review.md:17`) — the report's claim that it is "not covered by any accepted residual" is false. What justifies HIGH is the **cross-user** case the report never makes: any local account can bind `127.0.0.1:59833`. Facet B (`host`) → **LOW**, no ambient ingress exists. |
| F-02 | HIGH | **HIGH** (scope tightened) | Confirmed. Consent-laundering argument is strong on **macOS**, weaker on Windows (the CurrentUser root prompt is UI-layer and bypassable via direct HKCU registry write), near-zero on Linux as the report concedes. Rotation instance (`certs.rs:412-434`) is a narrow TOCTOU, not the same gap — citation corrected. |
| F-03 | MEDIUM (6.8) | **MEDIUM (~5.0)** | Sink A confirmed and *under*-argued (it is what defeats the SDK's HTTPS preference, i.e. it opens F-01's window on Windows). **Sink B substantially refuted**: `commit_launch_floor` commits `env!("CARGO_PKG_VERSION")`, not the probed value, and `candidate_allowed` enforces `candidate > current` independently of the state file — a forged probe yields `floor == current`, which is a no-op. Sink C is dominated by the actor's existing capabilities (the same test that dropped `c5-codex` §2). |
| F-04 | MEDIUM | **MEDIUM** (top of band; remediate first) | Fully confirmed, including the claim I tried hardest to refute. `updater.rs:137-142` refuses **all** updates when `running_below_floor`, before `candidate_allowed` is consulted — so a poisoned floor or a deliberate user downgrade is a total, permanent, silent patch-channel lockout. Sink A′ is cheaper to trigger than described (`mkdir updater.lock`). |
| F-08 | MEDIUM (both facets) | **A: MEDIUM** (bottom) · **B: LOW as security, high as a bug** | Facet A mechanism confirmed; the "accumulates over months into a directory of plaintext witnesses" magnitude is unsupported (residue requires death *during* a proof). Facet B's dead control is proven from `tokio-1.52.3/src/process/mod.rs:1442-1466`, but the CWE-532 disclosure is asserted, not shown; the certain harm is total loss of `bb` diagnostics. |

**Net:** no finding refuted outright; no band moved across a tier boundary. One sink (F-03 B)
refuted on impact, one facet (F-01 B) demoted, one facet (F-08 B) re-characterised. Two framings
corrected in ways that change what the owner should do: F-01 is a **re-take of an accepted decision**,
not a discovery; F-04 is a **live non-adversarial bug**, not only an attack.

---

## Verification not performed

Eight findings were **not** verified in this pass. They are carried at the coordinator's stated band
and confidence, unreviewed by me. This list exists so the report can be honest about depth.

| Finding | Band | Note on why it was not selected |
|---|---|---|
| F-2026-07-31-05 — trust removal fails open (macOS/Windows) | MEDIUM | Claude-only with an explicit codex non-finding — a genuine candidate. Displaced by F-04, whose claim was larger and more falsifiable. The `Present::{Yes,No,Unknown}` reference implementation at `trust/linux.rs:328-350` **was** read in the course of F-02 and does exist as described. |
| F-2026-07-31-06 — `x-aztec-version` unbounded version/disk control | MEDIUM | Self-declares facet A's impact as contingent on an unaudited premise (a memory-safety defect in some historical `bb`), so there is no evidentiary gap a verifier can close. Facet B (unbounded retention) is checkable and remains unverified. |
| F-2026-07-31-07 — authorization-prompt flooding | MEDIUM | Convergent (both legs), mechanism-level, and the behavioural step (consent fatigue) is not source-verifiable. |
| F-2026-07-31-09 — signed-but-withdrawn release replay | MEDIUM | Its precondition (feed write) is prior F-005, in `infra/tofu` + `.github/workflows`, **explicitly out of scope for this run** — so verification could not reach the question that governs its real severity. |
| F-2026-07-31-10 — consent-ceremony integrity (3 facets) | MEDIUM | Frontend timing behaviour; the decisive facet-A claim (~30 % of promotions) is empirical and not settleable by reading source. |
| F-2026-07-31-11 — SDK `/prove` body unbounded | MEDIUM | The report itself scopes this correctly ("precondition is F-01's … Medium rather than High for exactly that reason"), so there is little framing left to correct. |
| F-2026-07-31-12 — `appimage_self` / `APPDIR` containment | MEDIUM | **Confirmed as a known, consciously-accepted risk decision** (`implementations-plan/arc-bug-hunt/log.md:301-313`, round-7 close-out, quoted above) — which is all the brief asked me to establish. The *bypass mechanism* at `autostart.rs:1197` was not independently re-verified. |
| F-2026-07-31-13 — `schtasks_exe()` resolves via `SystemRoot`/`windir` | LOW | Lowest band. One incidental note from F-02 reading: the report says `trust/windows.rs:4` cites `crash_recovery::schtasks_exe` as its model — the rationale doc I read is at `trust/windows.rs:30-35` and does **not** name `schtasks_exe`. That citation should be re-checked before the fix is written. |

Also unverified: the report's *Dropped / not pursued* adjudications, the *Respected agent
self-rejections* list (12 Host-guard bypasses, origin canonicalization, the Tauri IPC gate, the
`tauri-plugin-updater` field-by-field analysis), and the *Coverage gaps* section — all carried as
stated.
