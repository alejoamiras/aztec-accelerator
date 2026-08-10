# Decision ledger — dual audit (codex `019fed5a`, fable subagent)

Both auditors read `plan.md`, `outline-b.md`, `recon.md`, the audit findings, and the source. Every
disputed claim below was re-verified against the code by me before ruling; where the two auditors
disagreed, the verification is cited.

## Errors in my own plan (highest priority)

### E-1 — `/prove` returns JSON, not a binary proof. **Both auditors, independently. I was wrong.**

I wrote in `recon.md` and in `plan.md` Fact 4 that the audit's advice ("reuse `readJsonBounded`") was
wrong because the `/prove` body is binary. **Verified false**: `core/src/server/prove.rs:372` returns
`axum::Json(json!({ "proof": encoded }))` and the SDK reads it with
`(await res.json()) as { proof: string }` (`accelerator-prover.ts:565`). The body is JSON carrying a
base64 string.

The audit was right and I was not. This inverts item 3: the deliverable is **not** a new
`readBytesBounded`; it is parameterizing `readJsonBounded`'s two hardcoded constants
(`HEALTH_BODY_MAX_BYTES`, `HEALTH_BODY_TIMEOUT_MS`, `accelerator-transport.ts:26-27`) so a caller can
pass a `/prove`-appropriate cap. Strictly less code than I planned.

Codex adds a step neither the audit nor I had: **bound the base64 string before `Buffer.from`**
(`accelerator-prover.ts:567`) — a capped JSON body still decodes to ~0.75× its size in a fresh buffer.

**Ruling: adopt in full.** Item 3 rewritten.

### E-2 — Sink A's deferral rested on a false dependency claim. **Both auditors.**

I argued sink A needs an FFI dependency that D3 deliberately avoided. **Verified false**:
`windows-sys 0.61` is already a *production* dependency of `core` under
`[target.'cfg(windows)'.dependencies]` (`core/Cargo.toml:44-52`), with `Win32_System_Threading`
already enabled. `GetExtendedTcpTable` needs an added **feature flag on an existing crate**, not a new
crate — and D3's constraint was "no NEW crate" (`core/Cargo.toml:41-43`, verbatim).

Fable adds the sharper point: my "not innocuous — the app refuses to start" claim describes a
single-instance *mutex*, not a socket-owner *gate*. Structured as an extra conjunct on the existing
`main.rs:294-303` bow-out, a false negative means we do **not** `exit(0)` and fall through to
"surface the error, stay resident" (`main.rs:304-311`) — today's foreign-process behaviour. **Worst
case is never worse than today, which is this plan's own definition of innocuous.**

**Ruling: my deferral collapses. Sink A moves into scope** as its own phase, flagged at the approval
gate as the one item that touches app startup.
Caveat recorded: `windows-sys` is production in `core` but only a **dev**-dependency in `src-tauri`
(`src-tauri/Cargo.toml:105`), so the check must live in `core`.

### E-3 — Item 5's age gate is the mechanism F-06 already abandoned. **Fable. The best finding of the two.**

`versions/leases.rs` documents, verbatim, why an mtime window is both too weak ("one queued behind the
prove permit for longer than the window loses its binary") and racy ("cleanup reads the mtime and then
unlinks"). I proposed exactly that mechanism one directory over.

The correct primitive is already in the tree. **Verified**: `server::start` binds a hardcoded
`PORT = 59833` (`server.rs:30,257`) and *both* binaries go through it, so only one process serves at a
time; and `sweep_cache_on_start` is spawned **only after the bind succeeds**, with the comment
"This ordering is load-bearing, not stylistic (F-06 follow-up, found by a codex review)"
(`server.rs:261-267`).

**Ruling: adopt.** The reaper is gated on the bind win and goes in the same post-bind block. A
conservative age floor is retained **as defense-in-depth only** — not as the mechanism — covering the
narrow window where `bind_with_retry` (5 s, `bind.rs:16-18`) wins the port from a prior instance still
completing an in-flight proof. Documented as a belt, so no future reader mistakes it for the guard.

Codex's alternative (derive the age from `PROVE_TIMEOUT = 300s`, `bb.rs:6` — verified) is a better
*number* than my unverified 30 minutes, and is adopted for that residual floor.

## Disputes between the auditors

### D-1 — Does the Windows CI leg run the `--lib` suite? **Codex right, fable wrong.**

Fable (C4) claims the Windows leg runs only `--test trust_windows` and `--test autostart_heal`, so a
`#[cfg(windows)]` unit test "executes nowhere". It read the `cert-trust` matrix and missed a separate
job. **Verified** by enumerating every `cargo test` in the workflow: the `windows-build` job
(`runs-on: windows-latest`, `accelerator.yml:590`) runs bare `cargo test` (`:608`) **and**
`cargo test --manifest-path ../core/Cargo.toml` (`:612`).

So `#[cfg(windows)]` unit tests in both crates do execute on real Windows. Fable's derived
recommendation ("add `cargo test` to the Windows leg") is already done.

### D-2 — F-12's mechanism. **Neither is sufficient; taking a third position.**

- Codex: parse `/proc/self/mountinfo`, bind the mount containing `current_exe()` to canonical
  `$APPIMAGE`, fail closed.
- Fable: `st_dev(current_exe) != st_dev("/")` — keys off the unspoofable `current_exe`, one line, no
  parse, no fallback.

Fable's is cheaper but **has a hole**: on a system where `/usr` is its own filesystem, an inherited
`APPDIR=/usr` with `exe=/usr/bin/AztecAccelerator` passes both the device check and `starts_with`.

Codex's mountinfo binding is the only design that closes it — but codex's own attack survives *both*:
an attacker who launches the genuine binary from a genuinely-mounted AppImage while setting
`APPIMAGE=/path/to/payload` defeats any test that reasons about `APPDIR`, because `$APPIMAGE` is a
separate variable. And an actor who can set our environment **can already write
`~/.config/autostart/*.desktop` directly** — the audit's own corollary-2 test, which it used to drop
other findings.

**Ruling:** fix what the finding names, at proportionate cost. Require **`$APPDIR` to be a mountpoint**
(two `st_dev` comparisons: `APPDIR` vs `APPDIR/..`) **and** our exe to be on that same device, plus an
explicit rejection of `/`. Kills `APPDIR=/`, `$HOME`, `/opt`, and `/usr` on ordinary single-root
systems. Pure — takes the device ids as parameters, so the existing table test at `autostart.rs:1789`
still works, and no `/proc` parser is introduced.

**Residual, recorded not hidden**: split-`/usr` systems, and full `$APPIMAGE` authentication, are not
closed. Both require an actor who controls our process environment, who already has autostart write
access, so the marginal gain is nil.

### D-3 — `SO_EXCLUSIVEADDRUSE` (open question A-2). **Fable right; excluded.**

Codex says add it as defense-in-depth. Fable notes `bind.rs:30` is `tokio::net::TcpListener::bind`, so
setting the sockopt requires `socket2` (a genuinely new crate) or manual socket construction, is
Windows-only, and needs a two-process Windows test. **Verified** — `bind.rs` uses
`tokio::net::TcpListener` throughout. My plan called it "cheap and innocuous"; it is neither on the
dependency axis. **Excluded**, and re-raised only alongside sink A's implementation.

### D-4 — The `NTE_NOT_FOUND` rule. **Fable's formulation is sharper; adopted.**

Codex calls the rule superstition. Fable makes the useful distinction: purity buys testability of
**decision logic**, and buys nothing for **platform constants, env-var names, error codes, and path
formats** — which is exactly where `cfg`-gated bugs live, and exactly what `CRYPT_E_NOT_FOUND` was.

This matches the correction I had already written in `lessons/phase-0.md` before either audit
returned. Both auditors independently reject `hardcoded_exists: bool` as "testing a model, not the
production wiring".

**Ruling:** keep purity for decision logic; **drop the ≤4-line dogma**; and for F-13 add the test both
auditors actually want — poison `SystemRoot`/`windir` on the real Windows leg and assert the
*production* resolver still returns System32. That test is now known to run (D-1).

## Rejected

- **Codex: "make F-12 fail closed, and if that breaks AppImage autostart, defer it."** Rejected as
  disproportionate given D-2's marginal-gain argument; fail-closed here risks breaking autostart for
  legitimate users to defend against an actor who already has the capability.
- **Outline B's sequencing.** Both auditors reject it as an implementation order; fable additionally
  **falsified B's central claim** that a fourth instance of the ambient-signal bug exists — the
  nearest cousins (`BB_BINARY_PATH` at `bb.rs:28`, `LOCALAPPDATA` at `updater.rs:651`) are documented,
  gated overrides. B survives only as commit-message framing. **No scope is budgeted for the sweep.**
- **My "audits should rule on A-2" framing.** They did, and the answer was to exclude it.

## Still disputed / owner call

- **Sink A entering scope** (E-2). Both auditors say my deferral was unjustified and I agree with the
  reasoning. It is UX-neutral and testable, so it satisfies the governing constraint — but it is the
  only item that touches whether the app starts, and it adds a `windows-sys` feature flag. Surfaced at
  the approval gate rather than decided silently.

---

# Round 2 — final fresh-context codex pass (`codex-UcXKrZpx`)

A new session, no prior context, given revision 2 + this ledger and told to hunt for load-bearing
errors. It found four, three of which I verified as real. Revision 3 folds them in.

## E-4 — `PROVE_TIMEOUT` is not a workspace-lifecycle bound. **Verified. Revision 2 Fact 10 was false.**

`create_prove_tempdir()` is called at `bb.rs:209`; the 300 s timeout starts only at `:245`
(`tokio::time::timeout(PROVE_TIMEOUT, child.wait_with_output())`); the proof is read at `:265`. The
workspace therefore outlives the timed region on **both** ends. A residual floor set at 300 s can
delete a live predecessor workspace — the very thing the floor existed to prevent.

**Ruling: adopt the correction, reject the prescribed fix.** Codex asks for a per-workspace
cross-process advisory lock held for the workspace lifetime. That is correct by construction, and
disproportionate: F-08a is Med/Low and the harm is disk residue over months. Revision 3 uses the
bind-win guard plus a **24-hour** floor — ~240× the longest possible proof — so the race is unreachable
by ratio rather than closed by argument. The lock is named in the plan as the upgrade path.

## E-5 — my F-12 rejection was self-contradictory. **Verified. Revision 3 reverses revision 2.**

I rejected codex's mountinfo design on the grounds that a poisoned `$APPIMAGE` defeats it too. But that
design binds the **mount source** to canonical `$APPIMAGE` — which rejects exactly that attack. I
applied an objection that only holds against my own weaker two-`stat` design. The fresh pass named it
as motivated reasoning and it was.

**Ruling: adopt the mountinfo design**, gated on capturing a **real** fixture from a built AppImage in
Phase 0 (hand-writing one would be the `NTE_NOT_FOUND` mistake precisely). If the fixture cannot be
obtained, ship the two-`stat` test **labelled partial hardening** in code, plan, and report — never as
closure. My corollary-2 argument survives only as a severity note, not as a reason to prefer the weaker
design.

## E-6 — `socket2` is already resolved; A-2's stated reason was wrong. **Verified.**

`core/Cargo.lock:1368` has `socket2 0.6.4` (transitive). This repo's own precedent promotes a
transitive crate to a direct edge and explicitly calls that "no NEW crate" (`core/Cargo.toml:41-43`),
so my "genuinely new crate" deferral reason does not survive.

**Ruling: still deferred, honest reason substituted.** `SO_EXCLUSIVEADDRUSE` changes rebind semantics,
and `bind_with_retry` exists to wait out a prior instance during an in-place updater restart
(`bind.rs:11-18`). Exclusivity risks turning a restart overlap into a failed start — the failure class
this plan refuses to introduce.

## E-7 — item 7 is not unconditionally UX-neutral. **Accepted; revision 2 overclaimed.**

Codex concedes the change is security-monotone (no new fail-to-start path) but shows the residual I
missed: `Unknown` against a **genuine** incumbent turns today's clean silent `exit(0)` into a resident
duplicate showing a tray error. That is user-visible.

**Ruling: adopt.** Item 7 is UX-neutral only while the lookup succeeds. Its Phase 4 test set grows to a
real two-instance Windows test plus an injected-lookup-failure case; the self-listener probe moves to
Phase 0 and proves only API availability. This strengthens, rather than weakens, the case for item 7
being the owner's call.

## Partially accepted

- **Item 1 does not eliminate the env fallback** (`certutil_exe:41-46`), and the CI test passes because
  `C:\Windows` exists. True. But F-13 as written *is* "this resolver lacks its sibling's hardened
  preference", so matching the sibling closes the finding as stated. Revision 3 records the residual and
  names `GetSystemDirectoryW` for both resolvers as a follow-up rather than smuggling it in as closure.
- **F-11 is only partially remediated.** True. Revision 3 adds the caller `AbortSignal` (additive,
  UX-neutral) to scope and **explicitly defers** the premature-`"proved"` phase-ordering fix to the
  owner, because correcting it changes what users see.

## Process note

Across three review rounds, **six** of my own factual or reasoning claims were wrong, and every round
found a defect in the previous round's work — including one (E-5) where the error was not a missed fact
but a bad argument I used to reject good advice. That is the same pattern the last two PRs recorded, and
it is the substantive argument for keeping the fresh-context pass rather than only resuming reviewers.
