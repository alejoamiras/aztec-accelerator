# Consolidated findings — security audit 2026-07-31 (run `9c4cb0c`)

**Phase 3 (reduce).** Inputs: 8 clusters × 2 independent legs (Opus + codex xhigh) = 15 raw reports
(`c6-persistence-codex.md` absent — see Coverage gaps), 2 Phase-1 maps, and the prior report
`PRIOR-2026-07-09-report.md` (F-001..F-016).

**Density.** 13 consolidated findings / 8 clusters = **1.6 per cluster**, above the 1.2 production
target. 22 raw findings were reduced: 5 merged into other findings on shared root cause, 4 dropped
outright (see *Dropped / not pursued*). The residual overshoot is real, not a reduce failure: two
clusters (C5 certs, C6 persistence) cover code that landed in the last three weeks and account for 4
of the 13.

## Threat-model decision applied throughout

**Same-user local code execution is IN SCOPE**, and this report applies that consistently. The
codebase itself repeatedly asserts it: `certs.rs:239-275` refuses HTTPS bring-up when a same-user-
*readable* `ca.key` exists; `win_acl.rs` applies owner-only PROTECTED DACLs against same-user
processes; `update_marker.rs:17-18` states "File mtime is never consulted: it is forgeable by the
same user who can write the file." The prior audit rated three same-user-local findings High/Medium.

But scope is not severity. Two corollaries, applied uniformly:

1. **No Critical.** Every finding here needs either a local foothold, control of the update feed, or
   a user click. Nothing is unauthenticated-remote. The highest band used is High, matching the
   prior audit's own conclusion.
2. **A local finding must beat what the attacker already has.** A same-user process can already
   read the witness workspace, kill the app, and write its own autostart entry. So a local finding
   only counts when it yields something *more*: **durability past the foothold** (state that keeps
   working after the malware is removed), **consent laundering** (the app performs a privileged act
   the attacker could not get the user to approve directly), or **crossing into another trust
   domain** (browser TLS, the update channel, another origin's data). One raw finding was dropped
   for failing this test (see *Dropped*), and it is why F-12 is Medium and not High.

---

## Findings

Ordered by severity band, then confidence.

---

### F-2026-07-31-01 — The SDK hands the private witness to an endpoint it never authenticates, in cleartext, by default

**Band:** HIGH · **CVSS v4.0 estimate: 8.1**
(`AV:L/AC:L/AT:N/PR:N/UI:P/VC:H/VI:N/VA:N/SC:H/SI:N/SA:N`)
**Confidence:** high · **Found by:** both (convergent — `c8-claude` F-C8-3, `c8-codex` Finding 1)
**CWE:** CWE-306 (primary), CWE-319, CWE-940, CWE-20 · **OWASP:** A07:2021 Identification and
Authentication Failures; A02:2021 Cryptographic Failures
**Prior audit:** **ESCALATION — incomplete remediation of F-001 (HIGH).** F-001's recommended fix had
two halves: (a) authenticate the server (pin the cert/public key, or exchange a per-install token
through the `0700` data dir) and (b) stop treating unrecognized `/health` bodies as available. Only
(b) shipped, as `isRecognizedHealthBody`. The audit adds two sinks the prior finding did not cover:
the HTTPS→HTTP demotion re-probe, and the unvalidated `host` option (Facet B).

**Instances**
- `packages/sdk/src/lib/accelerator-prover.ts:126-127` (`httpsOnly` defaults false), `:107`, `:124`
  (`host` accepted unvalidated), `:222-226` (pin), `:351-381` (witness serialize → post)
- `packages/sdk/src/lib/accelerator-transport.ts:52-56` (the entire identity test), `:302`, `:315-320`
  (plaintext probe fired), `:333-335`, `:383-392` (winner selection), `:455-466` (witness sink)
- `packages/sdk/src/lib/accelerator-transport.ts:415-434` → `accelerator-prover.ts:423-447` (the
  demotion re-probe re-uses the same forgeable marker — second instance of the same root cause)
- Facet B (unparsed `host`): `accelerator-transport.ts:188`, `:255`, `:257`, `:302`, `:318`,
  `:420-421`, `:439-441`; `packages/sdk/README.md:82-87`, `:216`
- `packages/sdk/src/lib/types.ts:29-37`

**Description.** The SDK decides where to send the complete msgpack-serialized private execution
witness on the basis of a two-constant JSON shape (`status === "ok" && api_version === 1`) served by
whatever process owns `127.0.0.1:59833`. The SDK's own comment concedes the point:
*"Field-presence is NOT identity; this shape check is collision resistance, not authentication"*
(`accelerator-transport.ts:44-50`). `httpsOnly` — the only mode in which the accelerator's
name-constrained CA actually authenticates the endpoint — is **off by default**
(`accelerator-prover.ts:126-127`), so the shipped posture is plaintext HTTP to an unauthenticated
peer. Facet B: `host` is caller-settable at construction *and* at runtime via the public
`setAcceleratorConfig`, is interpolated raw into six URL templates with no `new URL()` and no
loopback constraint, so `host = "127.0.0.1@attacker.tld"` produces an authority of `attacker.tld`
while the *logged* URL still reads `127.0.0.1` (`accelerator-prover.ts:359`).

**Trace (verified against source).** `accelerator-prover.ts:126` `httpsOnly` false →
`accelerator-transport.ts:317-319` plaintext `/health` probe constructed and fired →
`:333-335` `#isHealthy` = `response.ok && isRecognizedHealthBody(body)` → `:383-392` a healthy HTTP
wins when HTTPS is absent (the common case: nothing bound on `:59834` rejects in ~0 ms, so the
250 ms grace is never paid) → `accelerator-prover.ts:222-226` protocol pinned `http` →
`:351-357` URL snapshotted → `:362` `serializePrivateExecutionSteps(steps)` → `:381` →
**sink** `accelerator-transport.ts:455` `ky.post(body)`.

**Why existing mitigations fail.** `redirect:"error"` prevents steering the witness *after* the
endpoint is chosen, not choosing the wrong endpoint. The generation guard
(`accelerator-prover.ts:309-322,373-376,402-410,424`) is genuinely sound — it prevents the witness
reaching an *unprobed* endpoint, not a *probed but unauthenticated* one. The server-side Host guard,
Origin authorization and SEC-05 `/health` tiering are all controls the **genuine** server enforces;
an impostor simply does not implement them. The disclosed residual in `README.md:197-202` describes
the `httpsOnly` case explicitly — *"any same-machine process that **obtained a browser-trusted
certificate for localhost** and squats the HTTPS port is past this line"* — which sets the attacker
bar at forging browser trust. In the default mode the bar is *bind a TCP port and return ~31 bytes
of JSON*. That posture is not covered by any accepted residual.

**Smallest safe fix.** Two steps, in order:
1. **Flip the default.** Make `httpsOnly` default **true** when the HTTPS probe succeeds, and require
   an explicit opt-in (`allowInsecureTransport: true`) for the plaintext path — with a one-line
   `logger.warn` on every plaintext prove. This is a behaviour change for consumers whose accelerator
   has HTTPS disabled, so ship it with the SDK minor bump and the README caveat.
2. **Parse, don't interpolate.** Build every URL with `new URL()` and reject a `host` that does not
   resolve to a loopback literal unless `allowRemoteHost` is explicitly set. A prefix-match guard
   (`host.startsWith("127.")`) would be bypassable by the userinfo trick — the fix must parse.

Longer term (and the fix F-001 actually asked for, shared with F-03): a per-install secret written to
`~/.aztec-accelerator/` at `0600`, echoed in `/health` (or better, an HMAC over a client nonce), read
by the SDK. That is the only control that authenticates the peer without HTTPS.
**Effort:** steps 1+2 hours–1 day; per-install secret days.

---

### F-2026-07-31-02 — `install_ca_trust()` installs whatever is at `certs/ca.pem` into OS root stores, with no profile validation and no purpose scoping

**Band:** HIGH · **CVSS v4.0 estimate: 7.8**
(`AV:L/AC:L/AT:N/PR:L/UI:A/VC:H/VI:H/VA:N/SC:H/SI:H/SA:N`)
**Confidence:** high · **Found by:** both (convergent — `c5-claude` F-C5-1, `c5-codex` Finding 1)
**CWE:** CWE-295 (primary), CWE-345, CWE-20, CWE-269 · **OWASP:** A08:2021 Software and Data
Integrity Failures
**Prior audit:** **NEW.** The whole trust-store subsystem postdates 2026-07-09. Adjacent to F-016
(CA key zeroization) but a different property.

**Instances**
- `packages/accelerator/src-tauri/src/certs.rs:145-152` (`certs_exist()` — the accepting predicate),
  `:162-185` (`leaf_matches_ca`), `:228-234` (`generate_and_save` no-ops on a planted set),
  `:440-452` (`install_ca_trust` hands the unvalidated path straight through)
- `packages/accelerator/src-tauri/src/certs.rs:412-434` — the rotation path
  (`trust::trust_new_anchor(&staged.ca_cert)` at `:420`) has the same gap on staged files
- `packages/accelerator/src-tauri/src/trust/macos.rs:24-39` — `add-trusted-cert -r trustRoot` with
  **no `-p` policy argument** ⇒ trust for all policies; same at `:161-168`
- `packages/accelerator/src-tauri/src/trust/windows.rs:59-66` — `certutil -user -addstore Root` with
  no EKU property ⇒ all purposes; reached from `:132-147` and `:182-192`
- `packages/accelerator/src-tauri/src/trust/linux.rs:283-300` — installs the unvalidated anchor
  (purpose scoping `-t "C,,"` here **is** correct)
- `packages/accelerator/src-tauri/src/commands.rs:392-394` (the gate), `:520-531` +
  `frontend/onboarding.html:26` (HTTPS toggle pre-checked on every install and upgrade)

**Description.** `certs_exist()` accepts a certificate set on four criteria: the three files exist,
the leaf has time remaining, rustls accepts the leaf↔key pair, and the leaf verifies under the CA.
**All four are satisfiable by an attacker who mints both sides.** Nothing re-parses `ca.pem` to
assert the invariants the entire design rests on — `basicConstraints:CA=true`, the expected CN, and
the CRITICAL `nameConstraints` restricting the anchor to `127.0.0.1/32`, `::1/128`, `DNS:localhost`.
Those constraints exist only because `ca_params()` (`certs.rs:95-115`) puts them on certificates
*this app generates*; they are an invariant of `write_new_cert_set`, not of the file at
`CertPaths::live().ca_cert`. A planted set therefore skips regeneration (`:228-234`) and is handed
verbatim to `security add-trusted-cert -r trustRoot` / `certutil -addstore Root` — under the OS
consent dialog, which names *the application*, not the certificate's constraints.

Independently, and true even for the app's own honest anchor: **macOS omits `-p` and Windows sets no
EKU**, so the anchor is trusted for code signing, package signing, S/MIME and timestamping as well as
TLS. Linux already gets this right (`-t "C,,"` = SSL-CA only), which is the proof that the narrower
grant is both intended and achievable. RFC 5280 §4.2.1.10 name constraints only bind certificates
that *carry* a name of the constrained type — a code-signing or S/MIME leaf with no dNSName/iPAddress
SAN escapes the loopback constraint entirely. So `trust/mod.rs:16-17`'s claim (*"the load-bearing
control is the keyless CA … so a trusted anchor in any store is harmless"*) is weaker than stated on
two of three platforms even in the honest case.

**Trace (verified against source).** Attacker writes `~/.aztec-accelerator/certs/{ca.pem,
localhost.pem,localhost.key}` (paths `certs.rs:36-45`) → user completes the first-run wizard with the
pre-checked "Encrypted Connection" toggle → `commands.rs:566-577` `complete_onboarding` →
`enable_https_inner` → `commands.rs:392` `if !(certs::certs_exist() && certs::is_ca_trusted())` —
`certs_exist()` **true** for the planted set, `is_ca_trusted()` false (different serial) ⇒ branch
taken → `:393` `generate_and_save()` **returns Ok without writing** (`certs.rs:228-231`) → `:394`
`install_ca_trust()` → `certs.rs:440` → `trust/mod.rs:112-114` → `trust/macos.rs:24-39` /
`trust/windows.rs:59-66` / `trust/linux.rs:283-300`.

**Honest scoping.** On **Linux** the marginal gain to the attacker is near zero: `certutil -A` into
`~/.pki/nssdb` needs no prompt, so a same-user process can install its own anchor directly. The real
gain is on **macOS and Windows**, where adding a user-root anchor requires a consent dialog the
attacker cannot get past on their own — the app launders exactly that consent, under its own name,
in a ceremony the app has told the user to expect. That is a textbook consent-laundering escalation
and it is why this is High rather than Medium. The purpose-scoping half applies on all three.

**Why existing mitigations fail.** The keyless-CA design protects the anchor the app *mints*; a
planted CA simply has a key. `leaf_matches_ca()` proves the set is self-consistent, not that it is
ours — its documented purpose is detecting a torn `swap_into`, not provenance. `0700`/`0600` and the
Windows PROTECTED DACLs are cross-user controls; the actor here is the same user, whom
`certs.rs:239-275` already treats as hostile. The per-OS post-install verification confirms the
attacker's certificate became trusted.

**Smallest safe fix.** One function, called immediately before every trust-store write (both
`install_ca_trust` and `rotate`'s `trust_new_anchor`): re-parse the DER with the already-vendored
`x509-parser` and assert `basicConstraints CA=true` + `pathLen=0`, `keyUsage == keyCertSign|cRLSign`,
`subject CN == "Aztec Accelerator Local CA"`, and a **critical** `nameConstraints` whose permitted
subtrees are exactly the three loopback names. Fail closed by regenerating rather than installing.
Separately, add `-p ssl` to the macOS `add-trusted-cert` invocation and set the Windows EKU property,
matching what Linux already does. **Effort:** 1 day including tests (a planted-set rejection test
belongs next to the existing `generation_writes_no_ca_key` pin at `certs.rs:679-718`).

---

### F-2026-07-31-03 — The app treats a forgeable `/health` shape as process identity: Windows self-eviction, anti-rollback floor ratchet, and an unbounded probe read

**Band:** MEDIUM · **CVSS v4.0 estimate: 6.8**
(`AV:L/AC:L/AT:P/PR:N/UI:N/VC:N/VI:H/VA:H/SC:H/SI:N/SA:N`)
**Confidence:** high · **Found by:** both (convergent — `c1-claude` F-C1-1, `c1-codex` §1+§2,
`c7-codex` F-1)
**CWE:** CWE-290 (primary), CWE-306, CWE-346, CWE-940; secondary CWE-770 for the body read ·
**OWASP:** A07:2021
**Prior audit:** **ESCALATION of F-002 "Spoofable `/health` probe evicts the real accelerator
(Windows)" (MEDIUM).** F-002 covered exactly one sink (the `exit(0)`) and its recommended fix
("authenticate the incumbent — per-install secret / named-pipe identity / signed challenge") was
**not implemented**; the probe predicate is unchanged. Two sinks are new, and one of them did not
exist in July: `commit_launch_floor` is part of the **F-004 remediation**, so the anti-rollback
control shipped for one prior finding is gated on the forgeable signal of another. That composition
is what lifts this above a re-report.

**Instances**
- `packages/accelerator/core/src/server/probe.rs:14-17` — `is_healthy_aztec_response`, the entire
  identity test (single fix point)
- Sink A (Windows bow-out): `probe.rs:24-45` → `packages/accelerator/src-tauri/src/main.rs:294-303`
- Sink B (floor ratchet, all platforms): `probe.rs:54-71` →
  `packages/accelerator/src-tauri/src/main.rs:340-360` → `updater::commit_launch_floor()`
- Sink C (unbounded read, secondary root cause, same source): `probe.rs:41`, `:64`, `:68` —
  `resp.json::<serde_json::Value>()` with no size cap; only bound is the 3 s client timeout
- Bind side: `packages/accelerator/core/src/server/bind.rs:30` — no `SO_EXCLUSIVEADDRUSE` on Windows,
  no post-bind ownership assertion

**Description.** `is_healthy_aztec_response` checks two constants that are part of the **public**
`/health` contract, observable by probing the real app. Its doc claims it stops "an arbitrary process
answering on :59833"; it distinguishes nothing an attacker cannot satisfy. Two decisions consume it:
on Windows a lost bind plus a positive probe makes the legitimate app call `app_handle.exit(0)` —
status 0 chosen deliberately (`main.rs:290-292`) so the supervisor will *not* retry, i.e. the app
removes itself with no tray icon and no error while the squatter keeps the port the SDK trusts
(F-01). On every platform, three probes returning the responder's self-declared `version` matching
`CARGO_PKG_VERSION` commit the monotonic update floor. The source comment at `main.rs:335` states
*"Matching the version proves we are observing our own server (we own the bind)"* — that inference
rules out an honest different-version incumbent and is vacuous against a hostile responder echoing a
string that is printed on the release page.

**Why existing mitigations fail.** The Host guard and Origin gate protect *our listener from
browsers*; they do nothing for an *outbound* probe that trusts whatever answers. The 2xx requirement
and the three-consecutive-probe rule establish stability, not identity. SEC-05 `/health` tiering does
not apply — the probe sends no Origin, so it takes the detailed branch, and the impostor can mirror
any shape regardless. The 5 s `bind_with_retry` budget only delays a persistent squatter. Rust's
`TcpListener` does not set `SO_EXCLUSIVEADDRUSE` on Windows, so a second `SO_REUSEADDR` bind by the
same user can steal the socket outright (moderate confidence on this refinement; the rest is high).

**Smallest safe fix.** Replace the HTTP probe with an OS single-instance primitive, which is what the
bow-out decision actually needs: a named mutex on Windows (`CreateMutexW` with a per-install name),
an abstract socket or lockfile elsewhere. That removes sink A entirely. For sink B, do not probe at
all — the floor tracker wants to know *this process's own listener came up healthy*, which is an
in-process readiness signal (a `tokio::sync::watch` set by the server task after a successful bind
and first served request), not a network fact. For sink C, bound the probe read the way the SDK
already bounds `/health` (64 KB). All three are local edits.
**Effort:** sinks B+C hours; sink A 1–2 days (platform-specific).

---

### F-2026-07-31-04 — Unauthenticated, same-user-writable local state gates security mechanisms: a one-shot write permanently disables the patch channel

**Band:** MEDIUM · **CVSS v4.0 estimate: 6.5**
(`AV:L/AC:L/AT:N/PR:L/UI:N/VC:N/VI:H/VA:H/SC:N/SI:H/SA:H`)
**Confidence:** high (mechanism and permanence); moderate (severity class — precondition is a
same-user actor) · **Found by:** both, on different files (`c4-claude` F-C4-1, `c6-claude` F-C6-2,
`c3-codex` §1)
**CWE:** CWE-15 (external control of system setting), CWE-345, CWE-1329, CWE-755 ·
**OWASP:** A08:2021
**Prior audit:** **NEW.** `updater_state.rs` and `update_marker.rs` are both **remediation code** for
prior findings (F-004's Layer B and the Windows update-window transaction respectively) — so this is
a gap introduced *by* the previous round of fixes, not a regression of one.

**Merge decision (explicit, per brief).** These three were reported as separate findings by three
different agents against three different files. They are **one root cause**: a security decision is
taken from a local file that carries no authenticity binding, in a directory the threat model already
concedes to the attacker. Same source class, same boundary (local process → the app's trust in its
own on-disk state), same fix shape. Sinks are enumerated below.

**Instances**
- **Sink A — permanent update lockout via `updater-state.json`** (all platforms):
  `packages/accelerator/core/src/updater_state.rs:118-123` (`Corrupt => false` rejects every
  candidate), `:140-142` (`running_below_floor`), `:154-159` and `:183-187` (both mutators refuse to
  overwrite a `Corrupt` file — **nothing in the repo repairs or deletes it**), `:162-173`, `:189-198`
  (monotonic max ⇒ a poisoned high floor is irreversible);
  `packages/accelerator/src-tauri/src/updater.rs:34-36`, `:132-149`, `:184-187`
- **Sink A′ — silent lock denial**: `updater.rs:51-57` — `OpenOptions::…open(…).ok()?` collapses
  "another instance holds the lock" (benign, logged at `:60-65`) and "the lock file cannot be opened
  at all" (permanent, **unlogged**) into the same `None`. Collateral:
  `packages/accelerator/src-tauri/src/autostart.rs:1365-1367` — the autostart self-heal is disabled
  as a free side effect
- **Sink B — update-window marker forgery** (Windows):
  `packages/accelerator/src-tauri/src/update_marker.rs:141-168` (`load` validates shape only — no
  MAC, no owner/ACL check on read, no binding to an update this process started), `:33`+`:164`
  (24 h future clamp with **no `created_unix`** to bound it against), `:297-302`
  (`read_token_nonce` — the `txn` "nonce" is read from the very file it would authenticate).
  Consumers: `updater.rs:283-289`, `:470-487`; `autostart.rs:1372-1377`, `:1387-1392`, `:1586-1591`,
  `:1474-1489`, `:1710-1718`
- **Sink C — self-authenticating bb cache marker** (disputed, see below):
  `packages/accelerator/core/src/versions/cache_layout.rs:149-180`, `:185-218`;
  `packages/accelerator/core/src/versions/downloader.rs:27-31`; executed at
  `packages/accelerator/core/src/bb.rs:38`, `:206`. Plus the check→exec gap: the verifier returns a
  bare `PathBuf` (`cache_layout.rs:218`), so the object hashed is not the object spawned
  (`c2-codex` §2, `c3-codex` §2)
- **Survives the standard remediation:** `packages/accelerator/src-tauri/nsis/hooks.nsi:96-112` —
  verified: the uninstall hook removes the CA anchor and `RMDir /r "$PROFILE\.aztec-accelerator\certs"`
  and **nothing else**. `updater-state.json`, `updater.lock` and the marker survive uninstall +
  reinstall. macOS and Linux have no uninstall hook at all

**Description.** Sink A is the sharpest: `printf '{"schema":1,"floor":"999.0.0"}' >
~/.aztec-accelerator/updater-state.json` is a *schema-valid* document. It is not corruption — it
parses to `LoadedState::Valid`, trips `running_below_floor`, and every update from every version is
refused forever. The variant with one garbage byte is equally permanent by the opposite route:
`Corrupt => false` rejects all candidates, and **both** mutators refuse to overwrite a corrupt file,
so the state can never self-repair. Neither variant is user-visible — every path is
`tracing::error!` to a log file, and a user cannot distinguish "no update available" from "updates
permanently disabled". Variant A′ is reachable **accidentally, with no attacker**: a user who
deliberately downgrades to an older build from GitHub Releases (the normal response to a bad release)
trips `running_below_floor` and is permanently cut off from all future updates, including the fix.

Sink B is the same shape on Windows with a wider blast radius: one well-formed
`update-in-progress.json` with a 24-hour deadline suppresses the update poll, the crash-recovery
re-arm, the autostart heal, and the whole startup reconcile — and tells the user *"an update is
finishing; try turning Start on Login on again in a moment"*, disguising the attack as transient. The
`update_marker` module comment at `:17-18` gets the principle exactly right for mtime — "forgeable by
the same user who can write the file" — and then does not carry that reasoning through to the payload.

**Sink C — cross-model disagreement adjudicated.** `c3-codex` reported the bb cache marker as a
standalone High; `c3-claude` **rejected** it, reasoning that a process which can write the cache "can
already read the witness out of the 0700 workspace and own the session," so no independent security
property is violated. I keep it, at reduced weight, on one ground `c3-claude` did not address:
**durability**. A planted `versions/<v>/bb` + matching marker persists after the foothold is removed
and is executed on demand — and combined with F-07 (a remote origin chooses the version string) it
becomes *remotely triggerable*. That is re-entry, not a restatement of existing access. `c3-claude`'s
narrower point stands and is recorded: the marker is not, as `cache_layout.rs:208` claims, "the
SINGLE authority the runtime trusts" against a same-user writer, and that doc line should be fixed.

**Why existing mitigations fail.** `0600`/`0700` and owner-only DACLs are cross-user controls,
irrelevant to a same-user actor. Atomic write + fsync protects *our* writes from tearing; it does not
authenticate the file. `deny_unknown_fields` plus canonical-SemVer round-trips *increase* the attack
surface for sink A — they turn more inputs into `Corrupt`, i.e. into permanent lockouts — and the
poisoned-floor variant bypasses them with a perfectly canonical document. The 24 h clamp in sink B
caps one forgery, not a refreshing attacker; the compare-and-create deliberately never deletes a live
foreign marker (D22), and that property is exactly what makes the forgery durable. F-004 Layer A is
orthogonal (hostile *feed*, not hostile local *file*).

**Smallest safe fix.**
1. **Sink A, high value / low cost:** make the fail-closed policy *bounded*. On `Corrupt` — and on
   `running_below_floor` — fall back to `floor = current_running_version` instead of refusing
   everything. This preserves the entire anti-rollback property that matters, because
   `candidate_allowed` already enforces `candidate > current` **unconditionally and independently of
   the state file** (`updater_state.rs:118-121`, verified). The only thing the persisted floor adds
   over `current` is protection against an out-of-band downgrade of the binary itself — which
   requires an attacker who already owns the app files. Log loudly and surface a one-line banner in
   Settings so the state is not silent.
2. **Sink A′:** split the two `None`s at `updater.rs:51-57` — log the unopenable-lock case at `error`.
3. **Sink B:** add `created_unix` to the marker payload and reject `deadline > created +
   DEADLINE_SECS + slack`; verify owner/ACL on **read**, not only on write.
4. **Sink C:** persist the *authenticated release digest* in the marker (the one verified against
   GitHub during download) and re-check against it, rather than against a digest the marker itself
   supplies; and open the executable once and spawn from that handle (`/proc/self/fd` on Linux,
   `fexecve`, or a verified copy) so the object hashed is the object executed.
5. **All sinks:** MAC the state files with a key held outside the same-user filesystem (macOS
   Keychain, Windows DPAPI `CRYPTPROTECT_LOCAL_MACHINE` is not right here — use DPAPI user scope with
   an entropy blob in the registry). This is the complete fix and the expensive one.

**Effort:** items 1–3 hours; item 4 1–2 days; item 5 days–weeks. **Items 1 and 2 alone remove the
permanent, silent, accidentally-triggerable lockout — do those first.**

---

### F-2026-07-31-05 — Trust *removal* fails open on macOS and Windows: an execution failure is reported to the user as "removed"

**Band:** MEDIUM · **CVSS v4.0 estimate: 6.1**
(`AV:L/AC:L/AT:P/PR:N/UI:A/VC:N/VI:H/VA:N/SC:H/SI:H/SA:N`)
**Confidence:** high (behaviour and fail-open direction); moderate (real-world failure frequency)
**Found by:** claude only (`c5-claude` F-C5-2) — **cross-model disagreement**: `c5-codex` audited the
same files and did not raise it
**CWE:** CWE-390 (detection of error condition without action), CWE-754, CWE-273 ·
**OWASP:** A04:2021 Insecure Design; A09:2021
**Prior audit:** **NEW.**

**Instances**
- `packages/accelerator/src-tauri/src/trust/macos.rs:61-71` — `keychain_sha1()` uses
  `Command…output().ok()?` (swallows spawn failure) and **never inspects `output.status`**; `:140`,
  `:148-154` — the post-check reads that `None` as "absent"
- `packages/accelerator/src-tauri/src/trust/windows.rs:70-76` —
  `.map(|o| o.status.success()).unwrap_or(false)`: a spawn failure returns `false` = "not present" =
  "removed"; `:125-130` `delete_by_cn()` discards both spawn failure and non-zero exit; `:160-174`
- Sinks: `packages/accelerator/src-tauri/src/commands.rs:505-515` (Settings reports success);
  `packages/accelerator/src-tauri/src/main.rs:486-507` (the `--remove-ca-trust` CLI prints "removed /
  absent" and **exits 0**, telling scripted cleanup the anchor is gone)
- Downstream amplifier: `packages/accelerator/src-tauri/nsis/hooks.nsi:109` — the real Windows
  uninstall does not call the verified CLI at all; it runs a bare `ExecWait '"$SYSDIR\certutil.exe"
  -user -delstore Root "Aztec Accelerator Local CA"'` with **no exit-code check and no
  post-verification**, so on the one path where removal matters most the failure is invisible by
  construction
- **The correct implementation already exists in the same subsystem:**
  `packages/accelerator/src-tauri/src/trust/linux.rs:328-350` defines
  `enum Present { Yes, No, Unknown }` and maps any spawn error or non-zero exit to `Unknown`; `:497`
  does `let installed = !matches!(our_anchors_present(&bin, store), Present::No);` — unknown ⇒ still
  trusted ⇒ loud failure

**Description.** On macOS the removal loop's first action is `keychain_sha1()`. A locked keychain, a
wrong keychain path (e.g. `--remove-ca-trust` invoked under a different `HOME`, which
`login_keychain()` derives from `dirs::home_dir()`), or an EDR/MDM block yields empty stdout ⇒
`None` ⇒ `break` **before any `delete-certificate` is attempted**, `delete_failed` stays false, the
post-check `keychain_sha1().is_some()` is false, and `removal_incomplete()` reports success. On
Windows the same conflation happens at the helper level: `is_present_by_serial` returning `false` on
error is *fail-closed and correct* for the launch gate, and *fail-open and wrong* for `remove()` —
one helper, two callers with opposite safety requirements. `certutil.exe` is a commonly blocked
LOLBIN, so this is not a hypothetical failure mode.

**Why existing mitigations fail.** `removal_incomplete()`/`removal_failure_detail()`
(`trust/mod.rs:88-100`) **are** the loud-failure mechanism and they work correctly — they are simply
fed a `false` that means "unknown". `commands.rs:502` flips `https_enabled = false` regardless, which
stops the app presenting the certificate but does nothing about the anchor left in the OS store. The
macOS loop does stop on a failed delete — but only when a delete is *attempted*, and this failure
short-circuits before any.

**Smallest safe fix.** Port Linux's `Present` tri-state to `macos.rs` and `windows.rs`: any spawn
error or non-zero exit ⇒ `Unknown` ⇒ treated as still-present ⇒ the Settings action and the CLI both
fail loudly with a non-zero exit. Additionally, have `hooks.nsi` call the app's own
`--remove-ca-trust` CLI (already possible — recorded at
`implementations-plan/https-by-default-onboarding-2026-07-09/audit-codex.md:217`) and check its exit
code, instead of a bare `certutil` call. **Effort:** hours. This is the cheapest real fix in the
report and it directly bounds F-02's blast radius.

---

### F-2026-07-31-06 — `x-aztec-version` is an unbounded remote control over which native binary is installed and executed, and over how much disk the cache consumes

**Band:** MEDIUM · **CVSS v4.0 estimate: 6.3**
(`AV:N/AC:L/AT:P/PR:L/UI:N/VC:H/VI:H/VA:H/SC:N/SI:N/SA:N` — impact metrics reflect facet A's
contingent ceiling)
**Confidence:** high (trace, and the absence of any bound); **moderate** (realized impact of facet A —
contingent on some historical barretenberg release having an input-reachable memory-safety defect,
which was *not* audited and is not claimed) · **Found by:** claude only (`c3-claude` F-C3-1, F-C3-2,
F-C3-3) — **cross-model disagreement**: `c3-codex` audited the same files and reported neither
**CWE:** CWE-829 (inclusion of functionality from an untrusted control sphere), CWE-1104, CWE-770,
CWE-400 · **OWASP:** A06:2021, A08:2021
**Prior audit:** **NEW.** Distinct from F-007/F-008 (which were about an unverified download script
and a TOFU checksum pin — both since fixed); this holds *even with a perfect digest chain*, because
the attacker selects an **authentic** binary. Resolves **map uncertainty flag #4**
(`KNOWN_VULNERABLE_VERSIONS` empty) as an exploitable design property rather than an oversight.

**Merge decision (explicit).** Two root causes are merged here — "no bound on *which* version" and
"no bound on *how many*, with no reclamation" — because they share one source header, one boundary
(browser origin → local install), one sink family (download → install → execute), and one fix area
(a version-admission policy). Facet C (the double re-hash) is folded in as the third consequence of
the same missing rate control.

**Instances**
- Facet A (version choice): `packages/accelerator/core/src/versions/version_policy.rs:194`
  (`KNOWN_VULNERABLE_VERSIONS` — **verified empty**), `:240-260` (`check_version_selectable`: the only
  semantic gate is the empty denylist);
  `packages/accelerator/core/src/versions/release_metadata.rs:54-60` (URL built from the attacker's
  string, any tag in repo history); `packages/accelerator/core/src/bb.rs:191-229` (spawn with the
  attacker's body already written); macOS additionally **ad-hoc re-signs** the fetched binary at
  `downloader.rs:83-86`
- Facet B (unbounded retention): `version_policy.rs:52`, `:30-44` (`NetworkTier::from_version`
  classifies *any* unrecognized prerelease label as `Mainnet`), `:158` (Mainnet skips eviction
  entirely), `:299` (5-minute active-window exemption); `downloader.rs:215,219-230`;
  `packages/accelerator/core/src/server/prove.rs:287-299` — `cleanup_old_versions` has exactly **one**
  call site, a detached task that runs only after a *successful* download
- Facet C (CPU amplifier): `prove.rs:108` (`resolve_version` called synchronously from the async
  handler, outside the single prove permit, so up to 8 run in parallel on runtime workers);
  `cache_layout.rs:99-113`; `bb.rs:38` — the same 37 MB binary is hashed **twice** per request

**Description.** An approved origin's `x-aztec-version` header selects any tag in aztec-packages
history; the app fetches it, digest-verifies it against GitHub, installs it permanently, and executes
it as a child process with the user's full privileges, handing it the private witness via
`--ivc_inputs_path`. The **same origin controls both the binary choice and the input bytes**. The
absence of a floor is a *documented* decision (`version_policy.rs:226-231`, "many Aztec releases share
one bb") and the empty denylist is documented too — but the denylist is a compile-time `const`, so
shipping a revocation requires a full app release *and* the user accepting a declinable auto-update
(which F-04 can permanently block). There is no runtime revocation channel at all.

Facet B: the download completes **before the body is interpreted** (`bb::prove` is only called at
`prove.rs:329`), so a 2-byte junk body still pays for a full 37 MB install. Nothing reclaims it:
eviction is event-driven off a single success path, skips anything under 5 minutes old, and skips
`Mainnet` entirely — a tier that `from_version` assigns to every unrecognized prerelease label
(`-alpha-testnet.*`, `-staging.*`, `-beta.*`) and every plain `X.Y.Z`. Hundreds of qualifying
historical releases × 37 MB is 15–40 GB, permanent, with no "clear cache" affordance anywhere in the
tray or IPC surface.

**Why existing mitigations fail.** Digest verification proves **authenticity, not safety** — it is
precisely what makes facet A work. The traversal guard and the `AztecVersion` newtype are sound (no
bypass found, on record) but they gate *well-formedness*, not *ordering*. `MAX_INFLIGHT_PROVE = 8` is
a concurrency cap, not a rate cap or a completed-download bound. The 64 MB / 512 MB caps bound a
single archive. The in-code claim that stale entries are "self-healing — a later cleanup evicts them"
(`downloader.rs:210-214`) is false for the Mainnet tier.

**Smallest safe fix.** Three independent, small changes:
1. Add a **total-bytes ceiling** on `~/.aztec-accelerator/versions/` (e.g. 2 GB) enforced *before*
   `install_version_dir` publishes, and make `NetworkTier::from_version` fail **closed** — an
   unrecognized label gets the tightest tier, not the unbounded one.
2. Add a **rate limit on new-version downloads** (a token bucket, e.g. 3/hour per origin), separate
   from the concurrency semaphore.
3. Move `resolve_version` onto `spawn_blocking` and memoize `(path, mtime, size) → digest` for a few
   seconds so a request pays one hash, not two synchronous ones on runtime workers.

For facet A specifically, the smallest *honest* improvement is a **runtime-updatable revocation
list** — a signed denylist fetched alongside the update manifest (the minisign machinery already
exists) — rather than a version floor, which the existing design decision argues against.
**Effort:** items 1–3 hours each; the signed revocation channel 2–3 days.

---

### F-2026-07-31-07 — Authorization-prompt flooding: the pending cap is global and un-grouped, and denial is never remembered

**Band:** MEDIUM · **CVSS v4.0 estimate: 5.9**
(`AV:N/AC:L/AT:N/PR:N/UI:P/VC:N/VI:N/VA:H/SC:L/SI:L/SA:N`)
**Confidence:** high (mechanism); moderate (the consent-fatigue step is behavioural) ·
**Found by:** both (convergent — `c1-claude` F-C1-2, `c1-codex` §3)
**CWE:** CWE-770, CWE-799 (improper control of interaction frequency) · **OWASP:** A04:2021
**Prior audit:** **NEW.** The single-active-popup arbiter and the caps this attacks are themselves
post-F-014/C9 hardening.

**Instances**
- `packages/accelerator/core/src/authorization.rs:344-346` — the only global limiter,
  `st.by_request.len() >= MAX_PENDING_ORIGINS (10)`, first-come-first-served, no per-site grouping,
  no reservation; `:206-209` (the doc claiming it "prevents popup/memory spam"); `:274` (denial
  removes all history for the origin)
- `packages/accelerator/core/src/server/auth.rs:64-67` (the 429 path), `:69-75` (every new origin
  fires a real OS window), `:118-121` (`Deny` records **nothing** — no deny-list, no cooldown, no
  per-origin rate limit anywhere)
- Sink: `packages/accelerator/core/src/server.rs:449-453` — the victim's real dApp gets 429 and never
  reaches a prompt

**Description.** Ten iframes on ten attacker sub-origins fill the ten-slot pending table. The
victim's legitimate dApp then receives `429 TooManyRequests` and is **never prompted** — the only
mechanism by which any site can ever be approved is unavailable while the attacker's page is open.
When each attacker popup auto-denies after 60 s, denial is not recorded, so the iframe's `.catch()`
immediately re-fires and reclaims the slot. The table never drains. Secondary: the user faces an
endless queue of near-identical approval windows, and one mis-click grants an origin **unconditional
permanent** access to private witnesses (`authorization.rs:181-188`; the grant has no expiry).

**Why existing mitigations fail.** `MAX_PENDING_ORIGINS` bounds *concurrent slots*, not prompts over
time, and is precisely the mechanism that converts the attacker's subdomains into a denial of service
against everyone else. `MAX_PIGGYBACK_SENDERS` bounds fan-out *within* one origin. The single-active-
popup arbiter reduces click-hijacking but **lengthens** the drain to ~60 s per queued origin,
strengthening the availability impact. `MAX_INFLIGHT_PROVE`/`prove_waiters` never engage:
`authorize_origin` runs before `try_enter` (`prove.rs:233` vs `:238`), so unapproved requests never
reach that cap.

**Smallest safe fix.** Two small additions to `AuthorizationManager`: (a) group the pending cap by
registrable domain (eTLD+1) so ten sub-origins of one site occupy one slot, and reserve at least
2 slots for unseen registrable domains; (b) record a short negative cache on `Deny` (e.g. 5 minutes
per canonical origin) so a denied origin cannot immediately re-prompt. Both live entirely inside
`authorization.rs` and are covered by the existing test harness.
**Effort:** 1 day (eTLD+1 needs a public-suffix list — `psl`/`publicsuffix` crate, or accept
registrable-domain approximation by last-two-labels for the cap only, never for authorization).

---

### F-2026-07-31-08 — Witness-derived data escapes the controls designed to contain it: RAII-only workspace deletion, and inherited `bb` stderr

**Band:** MEDIUM · **CVSS v4.0 estimate: 6.0**
(`AV:L/AC:L/AT:N/PR:L/UI:N/VC:H/VI:N/VA:N/SC:N/SI:N/SA:N`)
**Confidence:** high (both mechanisms verified in source); moderate (realized disclosure for the
stderr facet — the destination varies by launch mode) · **Found by:** claude only (`c2-claude` F-C2-1,
F-C2-2) — **cross-model disagreement**: `c2-codex` audited `bb.rs` and reported neither
**CWE:** CWE-459 (incomplete cleanup), CWE-226 (sensitive information in resource not removed before
reuse), CWE-532, CWE-212 · **OWASP:** A02:2021, A09:2021
**Prior audit:** **NEW, and it partially contradicts a prior negative.** Prior F-003 fixed the
*permissions* on this exact file (world-readable → `0600` in a `0700` dir); this is the *temporal*
property on the same file, which the permission fix does not touch. The stderr facet contradicts the
prior audit's dropped item *"bb subprocess argv/env/stderr witness leak — witness travels only as file
content; the client receives a generic error"*: that assessment is correct about the **HTTP client**
and silent about the **local stderr sink**, which is where the data actually goes.

**Instances**
- Facet A (residue): `packages/accelerator/core/src/bb.rs:82-113` (`prove_tmp_parent` — a
  **persistent** dir under `dirs::data_local_dir()`), `:194-198` (workspace created, plaintext witness
  written), `:255` (deletion is exclusively `TempDir::drop`). Drop-skipping exits in shipped code:
  `src-tauri/src/main.rs:396` (tray Quit → `app.exit(0)`), `main.rs:301`, `src-tauri/src/updater.rs:542`
  (`app.restart()` after install), `src-tauri/src/commands.rs:666`, and the Windows
  `std::process::exit(0)` handoff documented at `src-tauri/src/update_marker.rs:3`. No graceful
  shutdown drain exists (grep for `with_graceful_shutdown` across the server modules: nothing), and
  **no startup reaper** enumerates stale `prove-*` dirs
- Facet B (dead containment): `packages/accelerator/core/src/bb.rs:206-229` — **verified**: the
  `tokio::process::Command` sets args, env and `kill_on_drop` and **never sets `.stdout()`/
  `.stderr()`**, so both default to `Stdio::inherit()`; `wait_with_output()` therefore returns an
  unconditionally empty `Output.stderr`, making the truncation guard at `:238-241` dead and
  `truncate_stderr` (`:272-280`) unreachable from production. The child writes verbatim to the
  inherited fd 2 — which on a Linux desktop autostart is the session journal /
  `~/.xsession-errors`, **outside** the `0700` log dir and outside its 7-day/7-file rotation

**Description.** The witness is the plaintext of everything the chain hides. Facet A: it is written
to a predictable path under a *persistent* parent and deleted only by `Drop` — which every one of the
app's own termination paths (`process::exit` family) skips. Mid-proof tray Quit, an auto-update
restart, logout or OOM leaves a full plaintext witness on disk forever; nothing ever reclaims it.
Over months this accumulates into a directory of plaintext witnesses harvestable by later same-user
code, a stolen laptop without FDE, or a user-scoped backup restore (Time Machine covers
`~/Library/Application Support`). Facet B: the code's own comment says the `bb` stderr stream may
carry "file paths, witness data" and then never captures it — the truncation control provably never
runs, and the three tests at `bb.rs:331-352` exercise `truncate_stderr` as a pure function, giving
false assurance that the path is covered.

**Why existing mitigations fail.** `0700`/`0600` modes and Windows PROTECTED DACLs are **spatial**
controls (other local users); they give zero **temporal** control. `kill_on_drop(true)` covers the
child, not the workspace, and is equally bypassed by `process::exit`. The prove-path `StatusGuard`
(`prove.rs:23-35`) shows the authors reasoned about Drop-based cleanup — but its enumeration stops at
*panic*, and both it and `TempDir` are defeated by the app's own normal quit mechanism. The version
cache **does** have eviction; the omission is specific to the witness workspace. For facet B, the
generic-HTTP-error mitigation (`bb.rs:243-248` → `server.rs:438`) genuinely holds — only the
server-side containment half is broken.

**Honest counterweight on facet B.** The macOS LaunchAgent sets no `StandardErrorPath`
(`autostart.rs:245-252`) so launchd routes to `/dev/null`, and a Windows GUI-subsystem build has no
console. The realized leak is Linux-desktop, terminal-launched, and CI. The *dead control* is
platform-independent and certain.

**Smallest safe fix.** Facet A: a startup sweep of `prove_tmp_parent()` removing any `prove-*` dir
(the app is single-instance per port, so an unconditional sweep at boot is safe), plus an explicit
`tmp_dir.close()` on the success path. Facet B: one line — `cmd.stderr(Stdio::piped())`, which
simultaneously revives the existing truncation and containment logic, or `Stdio::null()` if capture
is unwanted. Add a test asserting the warn line is emitted for a failing child.
**Effort:** hours for both. Facet B is a one-line fix that restores an already-written control.

---

### F-2026-07-31-09 — Updater accepts a byte-for-byte replay of a withdrawn-but-signed higher release

**Band:** MEDIUM · **CVSS v4.0 estimate: 5.9**
(`AV:N/AC:L/AT:P/PR:H/UI:N/VC:N/VI:H/VA:N/SC:H/SI:H/SA:N`)
**Confidence:** high (mechanism) · **Found by:** codex only (`c4-codex` F-01) — **cross-model
disagreement**: `c4-claude` modelled a fully hostile feed field-by-field against the actual
`tauri-plugin-updater-2.10.1` source and declared Layer A complete; that analysis is correct *for
tampering* and silent on *unmodified replay*, which is the gap
**CWE:** CWE-345 (missing freshness/currentness verification) · **OWASP:** A08:2021
**Prior audit:** **ESCALATION — residual of F-004 (HIGH).** F-004's remediation (Layer A envelope
binding + Layer B monotonic floor) fully closes the reported vector (a spliced high `version` pointing
at an old artifact). It does not close replay of an *unmodified historical envelope whose version is
genuinely higher than the victim's*, because freshness is never checked.

**Instances**
- `packages/accelerator/core/src/update_manifest.rs:38`, `:54`, `:185`, `:220` — `pub_date` is
  compared only for equality with the equally historical outer value, is never parsed, never compared
  against a clock or persisted metadata, and is **discarded from the verification result**
- `packages/accelerator/core/src/updater_state.rs:42`, `:118-123` — the persistent policy tracks only
  `floor` and `pending`; both cover releases already run or committed, never "a newer release the
  victim has not yet observed"
- `packages/accelerator/src-tauri/src/updater.rs:169`, `:184`, `:304` — the same non-freshness-aware
  policy at check time and at install time; sink `:533`

**Description.** An attacker with write access to `latest.json` (but **not** the signing key) can
serve an exact, previously-signed release that has since been withdrawn — e.g. `1.0.8`, pulled after
a security regression fixed in `1.0.9` — to a victim on `1.0.7`. Every signature verifies, every
outer↔envelope comparison passes (nothing was modified), `1.0.8 > 1.0.7 > floor`, and auto-update
installs the genuine but withdrawn build. Continuing to replay that feed also *withholds* `1.0.9`
indefinitely. The `pub_date` field that would carry freshness is signed and then thrown away.

**Why existing mitigations fail.** Minisign proves the envelope and artifact were *once* authorized;
it does not prove they are *current*. Exact outer-envelope binding defeats field splicing but is by
construction indifferent to byte-for-byte replay. The floor and pending values are rollback
protection, not freshness. The artifact signature and size checks correctly accept the authentic
historical artifact.

**Scope note.** The concrete path to feed write is prior **F-005** (wildcard OIDC + whole-bucket
write), which lives in `infra/tofu` and `.github/workflows` — **explicitly out of scope for this
run**. This finding is therefore reported on its merits with the honest note that its precondition
was last assessed three weeks ago and has not been re-verified here.

**Smallest safe fix.** Persist `highest_seen_version` and `latest_pub_date` in `updater-state.json`
alongside `floor`, and refuse an envelope whose `pub_date` is older than the highest already seen —
which converts long-term replay into a detectable, logged anomaly. A full fix is TUF-shaped (signed,
short-lived timestamp/snapshot metadata) and is a much larger change; the persisted-max approach gets
most of the benefit for a fraction of the cost, and composes with the F-04 fix in the same file.
**Effort:** 1 day for the persisted-max approach; weeks for signed timestamp metadata.

---

### F-2026-07-31-10 — Consent-ceremony integrity: clicks are accepted before the anti-click-steal guard applies, and approval is not bound to what was displayed

**Band:** MEDIUM · **CVSS v4.0 estimate: 5.6**
(`AV:N/AC:H/AT:P/PR:N/UI:A/VC:H/VI:H/VA:N/SC:L/SI:L/SA:N`)
**Confidence:** moderate (mechanisms are certain from code; exploitation rate depends on human
re-click timing and on a 12-hour window) · **Found by:** both (`c7-claude` F-C7-1 + F-C7-2,
`c7-codex` F-2)
**CWE:** CWE-1021 (improper restriction of rendered UI layers), CWE-451, CWE-863; adjacent CWE-367 ·
**OWASP:** A01:2021, A04:2021
**Prior audit:** **NEW.** The 700 ms click-steal guard and the single-active-popup arbiter are
themselves post-F-014/C9 hardening; all three facets are gaps in that new control.

**Merge decision (explicit).** Three raw findings merged. They are one security property — *the user's
click means what the UI showed them, at a moment when they intended to act* — and the fixes all live
in the same four files. Facets A and B share a root cause outright (the guard is opt-in and
event-misaligned); facet C is a different mechanism reaching the same property.

**Instances**
- **Facet A — guard armed on *focus*, not on *actionability*:**
  `packages/accelerator/src-tauri/frontend-src/bridge.js:30`, `:34-37`, `:40-42`, `:104-108` (700 ms
  guard, re-armed only from `focus`/`pageshow`); `frontend-src/authorize.js:68`, `:81-87` (buttons are
  enabled by a **1000 ms** self-scheduled poll — so a promoted popup's Allow can become clickable
  after the 700 ms guard has already expired, ≈30 % of promotions);
  `packages/accelerator/src-tauri/src/commands.rs:226-233` (`arm_active_popup`), `:238-253`
  (`spawn_active_deny_timer`, same misalignment). **Worst instance, deterministic:** if the user had
  already clicked the queued popup, it already holds focus, so `set_focus()` fires **no focus event
  at all** and the promoted popup goes live with **no guard whatsoever**
- **Facet B — three of four unsolicited auto-focused consent windows opt out of the guard:**
  `frontend-src/onboarding.js:86` (→ CA generation + `install_ca_trust` + autostart + `auto_update`,
  **all three toggles pre-checked**, and on Linux `certutil` installs the anchor with **no OS
  prompt at all**); `frontend-src/renewal.js:5-11` (→ rotation + trust dialog + immediate
  `restart()`); `frontend-src/update-prompt.js:8-15` (→ download + install + restart, auto-update
  checkbox pre-checked). **Contradicted invariant:** `bridge.js:38-39` asserts *"every remaining
  consequential control is a button wired through `wireButton({guard:true})`"* — only
  `authorize.js:107-108` actually passes it
- **Facet C — approval not bound to the displayed version:**
  `packages/accelerator/src-tauri/src/main.rs:243-245` (the `PendingUpdate` singleton is replaced
  unconditionally by a later poll); `src-tauri/src/windows.rs:65-69`, `:255` (the static
  `update-prompt` label means `open_or_focus_window` returns without refreshing the displayed URL);
  `frontend-src/update-prompt.js:13` (the response carries no version or token);
  `src-tauri/src/commands.rs:731-742` (installs whichever object is *currently* in the singleton)

**Description.** Facet A is the adversarially reachable one: an attacker page opens two origins'
`/prove` requests in the same tick, producing popup A (active, on top) and popup B (queued, built
unfocused at identical coordinates and size — `windows.rs:197-208` 400×300, `:75` `.center()`). The
user clicks Allow on A; B is promoted into the exact same rectangle with Allow in the same pixels; to
the user the click looks like it did not register, so they re-click — and if B's 1 s poll enabled the
buttons after the 700 ms guard elapsed, that click permanently approves an origin the user never
intended. An `Allow` here is an **unconditional permanent** grant of private-witness access.

Facets B and C are accident-shaped rather than attacker-timed (no adversary controls when the
onboarding, renewal, or update windows appear, and facet C needs a prompt left open across a 12-hour
poll). They are reported because the codebase asserts the opposite invariant and because the
consequences — an OS trust-store write, an autostart entry, a permanent auto-update opt-in, or
installing a different signed binary than the one shown — are exactly the class the guard exists for.

**Why existing mitigations fail.** The server-side arbiter (`authorization.rs:371`) enforces "only the
active popup may decide" — it does not care *why* the button was clicked. The button-disable while
queued is precisely the lag that consumes the guard. **Tests cannot catch facet A:** the Playwright
mock sets `window.__CLICK_GUARD_MS__ = 0` (`e2e/tauri-mock.js:14`) and the WebDriver spec sleeps past
the guard (`e2e-webdriver/auth-flow.spec.ts:142`), so no test exercises promotion-path guard timing.
For facet C, signature verification proves both artifacts came from the authorized channel — not that
the installed one is the one the user saw.

**Smallest safe fix.**
- Facet A: call `rearmInputGuard()` on the `false → true` edge of `info.active` in `authorize.js`, or
  better, gate `isClickGuardActive()` on `enabledAt` rather than `focusedAt`. One-line-ish.
- Facet B: make `guard` **default true** in `bridge.js`'s `wireButton`, opt-*out* rather than opt-in,
  and fix the stale invariant comment. Then it is structurally impossible for a new consent window to
  miss it.
- Facet C: pass an opaque prompt token in the URL and require `respond_update_prompt` to match it
  against the token stored with the pending `VerifiedUpdate`; on replacement, close and re-open the
  prompt rather than leaving a stale one.
**Effort:** facets A+B hours; facet C half a day.

---

### F-2026-07-31-11 — SDK reads the `/prove` success body with no size cap, no deadline, and no abort path

**Band:** MEDIUM · **CVSS v4.0 estimate: 5.3**
(`AV:L/AC:L/AT:N/PR:N/UI:P/VC:N/VI:N/VA:H/SC:N/SI:N/SA:L`)
**Confidence:** high (the absence of any bound was verified in ky's shipped source, not inferred) ·
**Found by:** both (convergent — `c8-claude` F-C8-1, `c8-codex` Finding 2)
**CWE:** CWE-400, CWE-770 · **OWASP:** API4:2023 Unrestricted Resource Consumption
**Prior audit:** **NEW.**

**Instances**
- `packages/sdk/src/lib/accelerator-prover.ts:508` (`(await res.json())` — the sink, **verified
  unbounded**), `:510` (`Buffer.from(response.proof,"base64")` — a second unbounded allocation),
  `:502-506` (the `"proved"` phase fires with the attacker's own `x-prove-duration-ms` **before** the
  body is read), `:484` (primary call), `:448` (HTTP-demotion retry, identical exposure)
- `packages/sdk/src/lib/accelerator-transport.ts:450-467` — `postProve`, the enclosing request that
  should carry the cap/deadline/signal; it accepts and forwards no `AbortSignal`, so a dApp **cannot
  cancel a hung proof at all**

**Description.** The `/prove` response is trusted far more weakly than `/health`, in the same file
family. `/health` has a byte cap (`HEALTH_BODY_MAX_BYTES = 64 KB`, enforced chunk-by-chunk), a 2 s
body deadline enforced both by timer and by in-loop wall-clock, and a zero-length-chunk starvation
guard — all added deliberately, with the comment *"a responder that returns 200 and then stalls (or
streams forever) would otherwise hang the probe indefinitely and buffer unbounded bytes (post-impl
codex High)"*. **None of it was carried over to the far larger `/prove` body.** ky's `timeout` is
cleared the moment `fetch()` resolves — i.e. at *headers* — and `totalTimeout` is never set, so past
headers there is no bound in bytes or in time. ky's 10 MB error-body cap is only reached on the
non-2xx path; a 200 never touches it.

Outcome: a hostile responder replies `200`, `x-prove-duration-ms: 1`, chunked, then streams forever or
never sends a byte. The UI renders "Proved natively in 1 ms" and the promise never settles — the
transaction flow is wedged behind a success-looking UI — or the tab OOMs and takes the wallet session
with it. Neither reaches the `catch (decodeErr)`, so the documented fail-safe-to-WASM contract
(`README.md:157`, `SKILL.md:147`) does not hold on the one path an attacker controls.

**Marginal-impact note.** The precondition is F-01's (the attacker owns the endpoint), and an attacker
who owns the endpoint already gets the witness — a strictly worse outcome. This is Medium rather than
High for exactly that reason: its independent value is the availability property and the false
"proved" signal.

**Why existing mitigations fail.** See above — verified in `node_modules/ky/distribution/`. The WASM
fallback `catch` catches *throws*; a stalled stream throws nothing, and a huge-but-valid stream throws
nothing until allocation fails, by which point the tab is gone.

**Smallest safe fix.** Reuse `readJsonBounded` — it already exists in the same file — with a
`/prove`-appropriate cap (e.g. 32 MB) and a deadline, and thread a caller-supplied `AbortSignal`
through `postProve` so a dApp can cancel. Move the `"proved"` phase emission to *after* a successful
decode so the UI signal is not attacker-timed. **Effort:** hours.

---

### F-2026-07-31-12 — `appimage_self` accepts any ancestor as `$APPDIR`, so the app writes attacker-chosen autostart and systemd persistence (Linux)

**Band:** MEDIUM · **CVSS v4.0 estimate: 5.4**
(`AV:L/AC:L/AT:P/PR:L/UI:N/VC:N/VI:H/VA:L/SC:N/SI:H/SA:N`)
**Confidence:** high (the bypass — `APPDIR=/` is demonstrably accepted); **moderate** (end-to-end
severity — the marginal gain over what the attacker already has is attribution laundering and
self-repair, not new execution) · **Found by:** claude only (`c6-claude` F-C6-1) — **no codex leg
exists for this cluster** (see Coverage gaps), so there is no cross-model signal either way
**CWE:** CWE-454 (external initialization of trusted variable), CWE-426, CWE-15 ·
**OWASP:** A08:2021
**Prior audit:** **NEW** to the security track — but see the flag below.

> **⚠ FLAG — this weakens a control shipped earlier today.** `appimage_self` was introduced by
> `implementations-plan/arc-bug-hunt` round 6 finding #1 (PR #429, `0033c0e`) *specifically* to stop an
> inherited `$APPIMAGE` being trusted. **However — and c6-claude did not surface this — the bug hunt's
> round-7 close-out explicitly considered this exact case and declined to widen the check:** *"A false
> ACCEPT needs a spoofed `APPDIR=/` (which already implies control of our launch environment) or
> genuine containment. No widening warranted."* So this is a **known and consciously accepted**
> residual, not an unnoticed bypass. What this audit adds is a different lens: that risk call was made
> under a *correctness* threat model, where "control of our launch environment" reads as exotic. Under
> the *security* threat model this project applies elsewhere (`certs.rs` treats same-user file access
> as hostile), setting `APPDIR=/` needs only a write to `~/.config/environment.d/10-x.conf` or
> `~/.profile` — the same permission the attacker needs for anything else here. **The owner should
> re-take the decision, not treat it as new.**

**Instances**
- `packages/accelerator/src-tauri/src/autostart.rs:1197` — **verified**:
  `exe_c.starts_with(&dir_c).then(|| PathBuf::from(appimage))` is the entire provenance test.
  `APPDIR=/` satisfies it for every absolute exe; so do `/usr`, `$HOME`, `/opt`. Nothing checks that
  `$APPDIR` is an AppImage mount, that `$APPIMAGE` relates to `$APPDIR`, or that `$APPIMAGE` is even a
  regular file. **Single fix point.**
- Sink A (`.desktop` autostart): `autostart.rs:1227-1229` (`desired_path`) → `:1684-1760` → `:933-945`
- Sink B (systemd user unit, reads the env **independently**):
  `packages/accelerator/src-tauri/src/crash_recovery.rs:264` → `:276` → `:287-313` writes
  `ExecStart=` with `Restart=on-failure`, `WantedBy=default.target`, then `systemctl --user enable`
- Sink C (no user interaction at all): `autostart.rs:1346-1408` (`heal_if_broken_at`), called
  unconditionally at startup from `src-tauri/src/main.rs:611`
- **Ownership-gate inversion:** `autostart.rs:1627-1632` passes the *poisoned* `desired_path(app)` as
  the ownership reference to `implicit_arm_gate` (`:1547-1555`), so `classify_program` compares the
  stored `Exec` against the **attacker's** path; a match makes `points_elsewhere` false and arms crash
  recovery at the attacker's binary. The gate whose entire purpose is "decline when another copy owns
  the entry" reads the attacker's path as "us"

**Description.** `starts_with` is a **containment** test masquerading as a **provenance** test. Two
environment variables cause the app to write, enable and *self-repair* OS persistence pointing at an
attacker-chosen binary — attributed to a legitimate signed application, which defeats "unknown
autostart entry" triage and EDR heuristics, and rewritten on every subsequent launch so the user
deleting it does not stick.

**Why existing mitigations fail.** `autostart_path_is_safe` (`crash_recovery.rs:211-216`) rejects only
non-absolute paths and control bytes. `desktop_quote` and `systemd_exec_start` are *correct
serializers* — they prevent directive injection, not a wrong path. The existence check at `:1690` is
satisfied (the attacker's file exists). The existing test
`appimage_trusted_only_when_our_exe_lives_under_appdir` (`autostart.rs:1789-1818`) covers the honest
`/tmp/.mount_AbC` shape and an obviously-disjoint foreign mount; **no case widens `APPDIR` to an
ancestor**, so the suite structurally cannot catch this.

**Smallest safe fix.** Replace the containment test with a device-identity test: a genuine AppImage's
`current_exe()` lives on the squashfs loop device, so `stat(current_exe()).st_dev !=
stat("/").st_dev` is a one-line, allocation-free check that `APPDIR=/`, `/usr`, `$HOME` and `/opt` all
fail. Stronger variant: require `$APPDIR` to be a proper mountpoint per `/proc/self/mountinfo`.
Add the ancestor-widening case to the existing provenance table test.
**Effort:** hours, including the test.

---

### F-2026-07-31-13 — `schtasks_exe()` resolves through `SystemRoot`/`windir` without the hardcoded-System32 preference its sibling applies

**Band:** LOW · **CVSS v4.0 estimate: 3.8**
(`AV:L/AC:L/AT:P/PR:L/UI:N/VC:N/VI:L/VA:H/SC:N/SI:L/SA:N`)
**Confidence:** high (the asymmetry and its consequences); moderate (the specific shadowing mechanics)
**Found by:** claude only (`c6-claude` F-C6-3; also raised as a cross-cluster note by `c7-claude`) —
no codex leg for this cluster
**CWE:** CWE-426 (untrusted search path), CWE-454 · **OWASP:** A08:2021
**Prior audit:** **NEW.** Resolves **map uncertainty flag #5** ("asymmetry observed, not adjudicated")
as exploitable.

**Instances**
- `packages/accelerator/src-tauri/src/crash_recovery.rs:388-395` — the resolver, **single fix point**:
  `std::env::var("SystemRoot").or_else(|_| std::env::var("windir")).unwrap_or_else(|_| "C:\\Windows")`
- Call sites: `:427` (`/Create`), `:456` (`/Delete`), `:459` (`/Query`)
- **Reference implementation to mirror, in this same repo:**
  `packages/accelerator/src-tauri/src/trust/windows.rs:36-47`, whose doc at `:31-33` states the exact
  rationale — *"Prefers the hardcoded `C:\Windows\System32\certutil.exe` when it exists, so a tainted
  `SystemRoot`/`windir` environment can't redirect this privileged trust operation (post-impl codex
  High)"* — and whose `:4` **cites `crash_recovery::schtasks_exe` as the pattern it follows.** The
  hardening was applied to one of the two and not the other

**Description.** Two directions, both bad. **Denial:** point `SystemRoot` at any directory lacking
`System32\schtasks.exe` and `/Query` errors; `.unwrap_or(true)` at `:465` reports "task still
present"; after three attempts `disable_impl` returns false; `updater.rs:443-450` reads that as
"cannot confirm the relauncher is disarmed ⇒ do NOT install" and **every update aborts, permanently
and silently** — a second, independent patch-channel-denial primitive alongside F-04. **False
confirmation:** a planted `%SystemRoot%\System32\schtasks.exe` that exits non-zero on `/Query` makes
`disable_impl` return *true* while the real task stays armed, so the updater hands off to NSIS
believing the every-minute relauncher is gone — precisely the "tick during NSIS file mutation could
spawn a half-written binary" hazard that `updater.rs:440-442` exists to prevent. The same stub also
intercepts `/Create`, yielding the task XML and control over what gets registered.

**Why existing mitigations fail.** `crash_recovery.rs:385-386` claims the defence ("avoids a bare-name
PATH lookup … a planted `schtasks` earlier on PATH can't win") — true for PATH, false for
`SystemRoot`/`windir`, because the *root* of that absolute path is attacker-supplied. The
three-attempt retry plus `/Query` verification exists to make the disarm result trustworthy, but all
attempts route through the same poisoned resolver: more retries against an attacker-controlled binary
is not verification. No privilege boundary is crossed (the planted binary runs as the same user) —
this is contract subversion, which is why it is Low.

**Smallest safe fix.** Copy the four lines from `trust/windows.rs:36-47`: prefer the hardcoded
`C:\Windows\System32\schtasks.exe` when it exists, fall back to the env only if it does not. Better,
call `GetSystemDirectoryW`. Secondarily, distinguish a *persistently* unreachable `schtasks` from a
transient error at `:465` so it produces a distinct diagnostic instead of a silent permanent block.
**Effort:** under an hour.

---

## Dropped / not pursued

Each line records what was dropped and why. Items an agent self-rejected are respected unless noted.

**Dropped by the coordinator (raised as findings by an agent):**

- **`/prove` inflight monopolization — one approved origin can hold all 8 slots and ~400 MiB**
  (`c2-codex` §1). **RE-REPORT of prior F-009**, whose remediation consciously landed at
  8 × 50 MB with the body read deliberately decoupled from the prove permit (the A1 slowloris fix).
  `c2-claude` independently audited the same code and **cleared** it (`reject_declared_oversize`
  handles comma-lists and RFC 7230 §3.3.2 agreement; `to_bytes` bounds *during* accumulation).
  Cross-model disagreement noted; prior acceptance controls.
- **Unbounded TLS handshakes / no connection cap on `:59834`** (`c5-codex` §2). Concrete trace, but
  **impact is strictly dominated by capabilities the threat actor already has**: the modelled attacker
  is a same-user local process, which can `SIGKILL` the app outright. Availability-only, no
  durability, no cross-domain effect. Fails the corollary-2 test stated at the top of this report.
  (The same absence exists on the HTTP listener, so the TLS framing is an artefact of the file split.)
- **bb hash-then-exec TOCTOU as a standalone finding** (`c2-codex` §2, `c3-codex` §2). Not dropped —
  **merged** into F-04 sink C, where the missing control (verification not bound to the executed
  object) belongs with the rest of the cache-authenticity discussion.

**Respected agent self-rejections (recorded so they are not re-derived):**

- **Loopback `Host`/`:authority` guard bypass** — `c1-claude` attempted 12 distinct bypasses
  (absolute-form vs `Host` disagreement, HTTP/2 `:authority` conflict, userinfo, IPv4-mapped/decimal/
  hex forms, trailing dots, `*.localhost`, port confusion, duplicate headers, layer ordering). Fails
  closed on all. The guard is the outermost layer and axum 0.8 wraps fallback routes.
- **Origin canonicalization collision/smuggle** — no cross-scheme aliasing (uses `Url::port()`, not
  `port_or_known_default()`); trailing-dot hosts rejected outright (F-011 fix holds); IDN punycoded;
  extension-ID grammar checked before lowercasing; `null`/`blob:`/`file:`/`data:`/opaque origins all
  fail closed and can never enter `approved_origins`.
- **Browser-forced `Origin` omission on `/prove`** — not reachable. Per Fetch, a non-GET/HEAD request
  always carries `Origin`; downgrades produce `null`, which is rejected. The accepted "absent Origin
  auto-approves" residual remains local-process-only.
- **`/health` detail-gating leak** — a no-Origin cross-origin GET does take the detailed branch, but
  the response is opaque and unreadable, and no cache-reuse path is constructible. The comment at
  `server.rs:296-300` is factually wrong and should be fixed; the leak is not realizable.
- **Path traversal / version-string abuse; tar-gzip bomb; digest fail-open** — charset gate plus the
  `AztecVersion` newtype make every path/URL sink structurally unreachable; `CappedReader` counts
  decompressed bytes across skipped entries; `verify_digest` treats `Ok(None)` and `Err` as fatal. No
  regression of F-007's fix.
- **F-004 Layer A against a fully hostile feed** — `c4-claude` checked every consumed field against
  `tauri-plugin-updater-2.10.1` source: all bound; the only unbound field (`notes`) is never read.
  Pinned pubkey cannot diverge (release pipeline patches the file on disk, so `include_str!` matches
  what `tauri-build` embeds). `VerifiedUpdate` is genuinely unforgeable. Lock ordering sound.
- **`swap_into` partial-rename interleavings** — enumerated; every one drives `certs_exist() == false`
  ⇒ regenerate, or resets `https_enabled`. No interleaving serves a mismatched pair or downgrades to
  an untrusted anchor.
- **Linux `certutil` path resolution and Firefox profile discovery** — canonicalizes, checks the
  binary and every ancestor for group/world-write and foreign ownership; absolute `Path=` under
  `IsRelative=1` neutralized by canonicalize + `starts_with($HOME)`; canonicalization failure fails
  closed.
- **Tauri IPC gate** — all 19 commands appear in exactly one capability **and** carry a handler-side
  `require_label`; no capability grants `core:*` or plugin surfaces; `withGlobalTauri: false`;
  `app.windows: []`. Auth-popup label binding is a 128-bit UUID-derived label re-derived on every call.
  Navigation guard rejects `data:`/`file:`/`javascript:`/look-alike suffixes.
- **Config fail-open on malformed JSON** — produces only *safer* defaults; approved origins are
  re-validated and re-canonicalized on load, never trusted as written.
- **NSIS `POSTUNINSTALL` guards** — the `$UpdateMode <> 1` **and** short-path-normalized
  `$EXEDIR != $INSTDIR` pair is sound; both operands normalized identically; no attacker-influenced
  input reaches either. (Map uncertainty flag #6 — the tauri-bundler-version dependency — remains a
  standing watch item, not a finding.)
- **`xml_escape`, `systemd_exec_start`, `desktop_quote`, `run_value_quote`** — all four verified
  correct or fail-closed; no injection or round-trip asymmetry.
- **SDK: `src/**` shipped in the npm tarball incl. `test-setup.ts`** — `exports` is a plain string, so
  every exports-aware resolver refuses deep paths; the `globalThis.expect` mutation is unreachable in a
  consumer. Packaging hygiene for the quality run.
- **SDK: `@aztec/simulator/client` dynamically imported but only a devDependency** — real supply-chain
  hygiene defect (no version constraint of any kind), but no attacker-controlled step; failure path is
  a clean actionable error.
- **SDK prototype pollution via the health body; secret leakage to logs; env-var range parsing** — all
  traced and cleared.

**Routed elsewhere (not security):**

- `auth.rs:84` discards the promoted request id returned by `resolve(...)` on backstop expiry, unlike
  every other call site — unreachable in practice, outcome is denial ⇒ `/harden bugs`.
- `cleanup_old_versions` can evict a version a concurrent prove is using ⇒ `/harden bugs`.
- `createLazySimulator`'s `Proxy` returns a function for every string property, so feature-detection
  gets a false positive ⇒ `/harden bugs`.
- macOS `crash_recovery::enable_impl` writes the LaunchAgent plist with `std::fs::write`, bypassing the
  symlink-refusal + atomic-rename discipline `autostart::write_artifact_atomic` applies to the same
  file — real asymmetry, no security property violated ⇒ consistency cleanup.
- `cache_layout.rs:208` describes the marker as "the SINGLE authority the runtime trusts", which
  overstates it against a same-user writer ⇒ doc fix (referenced in F-04).
- `server.rs:296-300`'s "no Origin → local, non-browser caller" comment is factually wrong ⇒ doc fix.

---

## Cross-cutting observations

1. **Hardening is applied on the write path and not the read path.** This recurs verbatim across three
   clusters. `update_marker::try_create_exclusive` uses `win_acl::secure_create_file` so *our* markers
   are owner-private, but `load` never checks the ACL or owner of a marker it did not create.
   `certs.rs` enforces name constraints, keylessness and `0600` when it *generates* a certificate set,
   and enforces none of them when it *adopts* one. `updater_state` writes atomically with fsync and
   authenticates nothing on read. The general fix shape is the same everywhere: **whatever invariant
   the writer establishes, the reader must re-assert** — because the threat model concedes that
   between write and read, someone else may have written.

2. **A control exists on one platform (or one sibling) and not the others, with the correct
   implementation sitting in the same directory.** `trust/linux.rs` has the `Present::{Yes,No,Unknown}`
   tri-state; macOS and Windows do not (F-05). `trust/windows.rs` prefers hardcoded System32 and even
   *cites* `crash_recovery::schtasks_exe` as its model; `schtasks_exe` never got the fix (F-13).
   Linux's trust install scopes purpose with `-t "C,,"`; macOS omits `-p` and Windows sets no EKU
   (F-02). The SDK bounds the `/health` read and not the `/prove` read (F-11). In every case the fix is
   *copy the sibling*, which makes these unusually cheap — and makes their persistence a review-process
   signal rather than a knowledge gap.

3. **Documentation asserts invariants the code does not enforce — and several of those assertions were
   used as the basis for accepting risk.** `bridge.js:38-39` claims every consequential control is
   guarded (one of five is). `probe.rs:11-13` claims the classifier stops arbitrary processes (it
   checks two public constants). `main.rs:335` claims a version match "proves we are observing our own
   server". `trust/mod.rs:16-17` claims a trusted anchor "in any store is harmless". `cache_layout.rs:208`
   claims the marker is the single authority. `downloader.rs:210-214` claims cleanup is self-healing
   (false for the Mainnet tier). These are not cosmetic: a wrong invariant comment is how the next
   reviewer stops looking.

4. **Fail-closed without a bound becomes a denial primitive.** `updater_state`'s `Corrupt => false`,
   the update marker's live-window suppression, and `schtasks`' `.unwrap_or(true)` are each individually
   defensible fail-closed choices that compose into *three independent ways for a one-shot local write
   to permanently and silently disable the patch channel*. The consistent fix is not to fail open, but
   to fail closed **to a provably-safe floor** and to make the state user-visible. A security control
   that can be permanently disabled without the user noticing is worth less than its author thinks.

5. **The July fixes moved the problem rather than closing it, in two traceable cases.** F-001's fix
   shipped the shape check but not the authentication (F-01). F-004's fix introduced
   `commit_launch_floor`, which is now gated on the forgeable probe from F-002 — a finding whose own
   recommended fix was never applied (F-03). Anti-rollback state introduced by F-004's fix is itself
   the wedge in F-04. This is worth a process note: **when a remediation only implements the cheaper
   half of a two-part recommendation, record which half was skipped.**

6. **Every consent surface in the app is pre-checked by default.** The onboarding HTTPS toggle
   (`commands.rs:520-531`, `onboarding.html:26,39,52`), the update prompt's auto-update checkbox
   (`update-prompt.html:15`), and — historically — the authorize popup's "Remember" box (prior F-014,
   since fixed). Pre-checking is what turns each stolen or accidental click in F-10 into a durable
   grant rather than a one-off. This is a product decision, not a bug, but it is load-bearing for three
   findings and deserves a deliberate re-take.

---

## Coverage gaps

Stated honestly. This audit did **not** examine:

- **`packages/accelerator/server`** — the headless server crate, owner-excluded. It shares
  `accelerator_core` with the desktop app, so F-03 (probe), F-06 (`x-aztec-version`), F-07
  (authorization caps) and F-08 (witness residue, stderr) plausibly apply to it too, with a different
  threat model (`auto_approve_localhost: true`, prior F-013). **None of that was verified.**
- **`.github/workflows` and `infra/tofu`** — owner-excluded, deferred to a separate pass. This matters
  concretely for **F-09**, whose precondition (write access to `releases/latest.json`) is prior
  **F-005**, last assessed 2026-07-09 and **not re-verified here**. F-005's status therefore gates
  F-09's real-world severity and is unknown as of this run.
- **`packages/playground`, `packages/landing`** — owner-excluded.
- **barretenberg / `bb` itself** — F-06 facet A's realized impact is contingent on some historical
  release having an input-reachable memory-safety defect. **No such claim is made**; the binary was not
  audited. What is certain is that the design grants the choice.
- **The `c6-persistence` codex leg is missing.** It had not completed when Phase 3 began. C6 is the
  cluster covering OS persistence, the update-window transaction, and the NSIS hooks — the "recent arc"
  the owner flagged as concern #3 — so **the two findings in this report drawn from C6 (F-12, F-13) and
  its non-findings list have no cross-model corroboration.** Both are marked accordingly. C6 is the
  single highest-value place to re-run a second leg.

**Map uncertainty flags — resolution status:**

| # | Flag | Status |
|---|---|---|
| 1 | `tauri-plugin-webdriver` / port 4445 behind a non-default feature | **Partially resolved.** `c7-claude` confirmed `webdriver` is not in `default = []`. The *release build command* was still not traced end-to-end. Residual risk: a release built with the feature on would expose an unauthenticated WebDriver server. Worth a one-line CI assertion. |
| 2 | `trust::stub` compiles only for non-mac/Linux/Windows targets | **Resolved** — no such target ships. Not finding-eligible. |
| 3 | `snapshot_restore_roundtrip_for_tests` is a `pub fn` in the production library that mutates real autostart state | **UNRESOLVED.** No agent adjudicated it. It is not reachable from IPC (not a `#[tauri::command]`) so there is no attacker path, but a `pub` state-mutating test helper in a shipped library is a latent footgun. Should be `#[cfg(test)]` or `#[doc(hidden)]`. |
| 4 | `KNOWN_VULNERABLE_VERSIONS` is empty; no version floor | **Resolved as F-06.** Verified empty; confirmed as a documented design decision with a real gap (compile-time-only revocation). |
| 5 | `schtasks_exe` resolves via `SystemRoot`/`windir` without the System32-first preference | **Resolved as F-13.** Exploitable in both directions. |
| 6 | NSIS uninstall guard depends on `$EXEDIR != $INSTDIR` semantics measured against tauri-bundler 2.8.1 | **Partially resolved.** `c6-claude` verified the guard logic is sound *as written*. The bundler-version dependency is unchanged and remains a standing watch item — a bundler bump can silently invalidate it, and no test would catch that. Recommend pinning the bundler version and asserting it in CI. |

**Method note.** Every claim promoted to High was re-read against source by the coordinator before
promotion: `certs.rs:145-152,228-234,440-452`, `trust/macos.rs:24-39`, `trust/windows.rs:59-76`,
`commands.rs:392-394`, `accelerator-transport.ts:44-56,296-340,380-395,448-470`,
`accelerator-prover.ts:120-132,500-515`, `probe.rs:1-75`, `main.rs:285-310,335-360`,
`updater_state.rs:112-200`, `nsis/hooks.nsi:85-115`, `version_policy.rs:188-200`, `bb.rs:200-245`,
`autostart.rs:1185-1235`. No promoted claim failed verification; one framing was corrected (F-12's
"the tests cannot catch this" — the round-7 close-out shows the case *was* considered and declined).
