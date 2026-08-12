# UX-neutral remediation — audit 2026-07-31-9c4cb0c

Close the open findings from the security audit **that no user can perceive**: no new prompt, no new
click, no changed default, no changed timing a human would notice. Findings whose fix alters consent
behaviour are excluded and stay with the owner.

Base: `df38283`. Worktree/branch: `audit-ux-neutral-fixes` / `worktree-audit-ux-neutral-fixes`.

**Revision 4** — post dual audit (codex `019fed5a` + fable) **and** a final fresh-context codex pass
(`codex-UcXKrZpx`). Six of my own claims were wrong across the two rounds and are corrected here,
including one — item 2 — where revision 3 **reverses** revision 2. Full reasoning in
[`decision-ledger.md`](decision-ledger.md). Recon: [`recon.md`](recon.md). Competing outline:
[`outline-b.md`](outline-b.md) (rejected as a sequencing proposal by all three reviewers; survives as
commit framing).

---

## Governing constraint (owner, verbatim)

> **"Anything you can not write tests for, we won't implement unless is inocuous."**

Applied strictly, and applied *against* myself: it admitted six items, and in revision 2 it **also
admitted a seventh I had wrongly deferred** — both auditors showed my stated reasons for deferring
F-03 sink A were false. "Innocuous" means the worst failure is *no worse than today's behaviour*.

Second rule, inherited: **mutation-proof or it doesn't count.** Every regression test is validated by
reverting its fix and confirming that named test fails.

---

## Scope

| # | Finding | What ships | Test runs on |
|---|---|---|---|
| 1 | **F-13** | `schtasks.exe` stops trusting `%SystemRoot%`, matching its sibling | Linux + **real Windows** |
| 2 | **F-12** | `$APPDIR` must be a mountpoint our exe lives on; `/` rejected | every platform (pure) |
| 3 | **F-11** | `readJsonBounded` gains caller-supplied cap/deadline; `/prove` uses it; base64 bounded before decode | every platform |
| 4 | **F-03 sink C** | `/health` probe read bounded at 64 KiB (its own constant, not `/prove`'s) | every platform |
| 5 | **F-08a** | Witness-workspace reaper, gated on the `:59833` bind win | every platform |
| 6 | **F-03 sink B** | Version-floor tracker requires in-process bind ownership | every platform |
| 7 | **F-03 sink A** | Windows bow-out requires the listener's owning process to be our image | Linux (logic) + **real Windows** |

**Deferred, with argument:** F-07, F-10 (change consent UX — owner call), F-09 (owner-deferred: its
fix reintroduces F-04's lockout shape), F-11's `"proved"` phase-ordering (user-visible),
`SO_EXCLUSIVEADDRUSE` (§A-2 — **not** a new-crate problem; deferred for restart-overlap availability).

Items 6 and 7 together close F-03 entirely.

---

## Success criteria

1. Seven items implemented; each with a named regression test that **fails when its fix is reverted**.
2. `bun run test`, `bun run lint`, `bun run lint:actions` clean; 594 existing local tests still green.
3. Full CI matrix green, **including the `windows-build` leg** — items 1 and 7 depend on it.
4. No user-visible change: no dialog, no default change, no added latency on the proving path, no
   change to the tray or onboarding.
5. `report.md` remediation table reconciled to the truth.
6. Deferred items carry a written argument, not a bare status of "open".

---

## What the `NTE_NOT_FOUND` lesson actually is

Revision 1 of this plan generalised it into "every decision must be pure, every `cfg` wrapper ≤4
lines". **Both auditors rejected that**, and they were right. The accurate version:

> Purity buys testability of **decision logic**. It buys nothing for **platform constants, env-var
> names, error codes, and path formats** — which is exactly where `cfg`-gated bugs live.

`CRYPT_E_NOT_FOUND` was a wrong *constant value*. A pure function handed that same wrong constant by a
trivial wrapper catches nothing anywhere. Only running it on real Windows found it.

So the rule is now two-sided, and both sides are used below:

- **Logic → pure functions**, table-tested on every platform (items 2, 6, 7's comparison).
- **Values → run it on the real OS.** Confirmed available: the `windows-build` job
  (`accelerator.yml:590`) runs `cargo test` for **both** crates on `windows-latest` (`:608`, `:612`),
  and the `cert-trust` macOS leg runs it for `src-tauri` (`:188`). A `#[cfg(windows)]` unit test is
  **not** decoration.

Consequence: item 1 drops the `hardcoded_exists: bool` model-test both auditors called out, and tests
the **production resolver** under a poisoned `SystemRoot` on the real Windows leg.

---

## Assumptions

### Facts (verified in this worktree at `df38283`)

1. `certutil_exe()` (`trust/windows.rs:36`) prefers hardcoded `C:\Windows\System32\certutil.exe`;
   `schtasks_exe()` (`crash_recovery.rs:370`) does not. **Neither has a test.**
2. `appimage_self` (`autostart.rs:1186`) is pure and table-tested (`:1789`); its containment test
   `exe_c.starts_with(&dir_c)` (`:1197`) is satisfied by `APPDIR=/` for every absolute path.
3. **`/prove` returns JSON**, not binary: `axum::Json(json!({"proof": encoded}))`
   (`server/prove.rs:372`), read as `(await res.json()) as { proof: string }`
   (`accelerator-prover.ts:565`), then `Buffer.from(response.proof, "base64")` (`:567`).
   *(Revision 1 asserted the opposite. It was wrong — see ledger E-1.)*
4. `readJsonBounded` (`accelerator-transport.ts:147`) has hardcoded `HEALTH_BODY_MAX_BYTES` /
   `HEALTH_BODY_TIMEOUT_MS` (`:26-27`). ky's `timeout` stops counting at headers.
5. `probe.rs:41,64` are `resp.json::<serde_json::Value>()` with no size cap.
6. `create_prove_tempdir()` (`bb.rs:120`) deletes only via `TempDir::Drop`. No reaper exists.
7. **`server::start` binds a hardcoded `PORT = 59833`** (`server.rs:30,257`) and **both** binaries go
   through it — so exactly one process serves at a time.
8. **F-06 already solved item 5's ordering problem.** `sweep_cache_on_start` is spawned *only after*
   the bind succeeds, commented "This ordering is load-bearing, not stylistic" (`server.rs:261-267`).
9. `leases.rs` documents an mtime window as both too weak and racy — the mechanism revision 1 proposed.
10. ~~`PROVE_TIMEOUT = 300s` (`bb.rs:6`) is a real lifecycle bound.~~ **CORRECTED (rev 3):** it is
    **not**. The workspace is created at `bb.rs:209`, the 300 s timeout starts only at `:245`, and the
    proof is read at `:265` — so workspace lifetime strictly exceeds `PROVE_TIMEOUT`. Any floor set
    *at* 300 s can delete a live predecessor workspace.
11. `commit_launch_floor()` commits `env!("CARGO_PKG_VERSION")`, never the probed value — sink B is
    **correctness**, not security.
12. **`windows-sys 0.61` is already a production dependency of `core`**
    (`core/Cargo.toml:44-52`, `cfg(windows)`), with `Win32_System_Threading` enabled. D3's constraint
    was "no NEW crate" (`:41-43`). In `src-tauri` it is **dev-only** (`:105`) — so item 7 lives in
    `core`.
13. `bind.rs` uses `tokio::net::TcpListener` throughout. **CORRECTED (rev 3):** `socket2 0.6.4` is
    already resolved in `core/Cargo.lock:1368` (transitive). By this repo's own precedent — promoting
    the already-transitive `windows-sys` to a direct edge, "no NEW crate" (`core/Cargo.toml:41-43`) —
    `SO_EXCLUSIVEADDRUSE` is **not** blocked on a new crate. It is deferred for a different, honest
    reason: see §A-2.
14. `filetime` is a dev-dep of `core` (`core/Cargo.toml:62`); `bb.rs` is in `core`.

### Inferences (still unverified — attack these)

- **I-1.** A `/prove` cap of *(to be measured)* is above any real proof by a wide margin. Blocked on a
  measurement; **one observed proof is a lower bound, not a protocol maximum** (codex). The cap must be
  argued from headroom, not from a single sample.
- **I-2.** `GetExtendedTcpTable` + `QueryFullProcessImageNameW` can resolve the `:59833` owner for a
  same-user process without elevation. High confidence, unverified — Phase 4 proves it on CI before
  the logic is wired in.
- **I-3.** ~~Does the `--lib` suite run on Windows?~~ **Resolved: yes** (see above).
- **I-4.** ~~mountinfo shape inside an AppImage~~ — **no longer needed**; the device-id design (item 2)
  removes the `/proc` parse entirely.

---

## Architecture & Implementation

### Proposed architecture

No new modules except one small `core` file for item 7. Six of seven items **narrow an existing
decision**; the shape is: *extract the decision into a pure function, test the logic everywhere, test
the platform values on the platform.*

| Item | Where | Reused from |
|---|---|---|
| 1 | `crash_recovery.rs` | `certutil_exe`'s existing preference (copied, not shared — see trade-offs) |
| 2 | `autostart.rs` | its own table test at `:1789` |
| 3 | `accelerator-transport.ts`, `accelerator-prover.ts` | `readJsonBounded`, parameterized |
| 4 | `core/src/server/probe.rs` | its own 64 KiB constant |
| 5 | `core/src/bb.rs` + `core/src/server.rs` | `sweep_cache_on_start`'s post-bind slot (`server.rs:267`) |
| 6 | `core/src/server.rs`, `src-tauri/src/main.rs` | — |
| 7 | `core/src/server/owner.rs` (new) | `windows-sys`, already a prod dep |

### Key interfaces

```rust
// item 2 — autostart.rs. Purity preserved: device ids arrive as parameters.
pub(crate) fn appimage_self(
    appimage: Option<OsString>,
    appdir:   Option<OsString>,
    exe:      &Path,
    dev:      DevIds,       // { appdir, appdir_parent, exe } — production wrapper stats them
) -> Option<PathBuf>;

// item 5 — core/src/bb.rs. No age parameter in the guard position: the BIND is the guard.
pub fn reap_orphaned_prove_dirs(parent: &Path, floor: Duration) -> usize;

// item 6 — core/src/server.rs
pub fn bind_ownership() -> tokio::sync::watch::Receiver<bool>;   // reset when the listener ends
fn should_commit_floor(we_own_bind: bool, probed: Option<&str>, want: &str) -> bool;   // pure

// item 7 — core/src/server/owner.rs
pub enum PortOwner { Ours, Foreign, Unknown }
pub fn classify(owner_image: Option<&Path>, our_image: &Path) -> PortOwner;   // pure
#[cfg(windows)] pub fn owner_image_of_port(port: u16) -> Option<PathBuf>;     // the FFI leg
```

```ts
// item 3 — accelerator-transport.ts. Same function, caller-supplied policy.
async function readJsonBounded(
  response: Response,
  maxBytes = HEALTH_BODY_MAX_BYTES,
  timeoutMs = HEALTH_BODY_TIMEOUT_MS,
): Promise<unknown>;
```

### Data & control flow

**Item 6 + 7 — the two forgeable decisions, both gaining a non-forgeable conjunct:**

```
                    ┌─ HTTP /health ─────────┐  (forgeable: proves SERVING, not identity)
floor tracker ──────┤                         ├── AND ──> commit floor        [item 6]
                    └─ in-process watch(bind) ┘  (not forgeable: we own the socket)

                    ┌─ HTTP /health ─────────┐
Windows bow-out ────┤                         ├── AND ──> exit(0)             [item 7]
                    └─ socket owner's image ──┘  (not forgeable: OS-mediated)
```

In both, the HTTP probe is **kept**, because it is the only evidence the server *serves* — deleting it
(the audit's advice) would let a bound-but-wedged build ratchet the version floor. Both auditors
agreed this refutation holds.

Item 7's failure direction is **security-monotone**: `Unknown` → do **not** `exit(0)` → fall through to
"surface the error, stay resident" (`main.rs:304-311`). It creates no new fail-to-start path.

**But it is not unconditionally UX-neutral, and revision 2 overclaimed that.** If the lookup returns
`Unknown` against a *genuine* incumbent, today's clean silent exit becomes a **resident duplicate
showing a tray error**. That is user-visible.

### D-ITEM7 (binding design constraint, rev 4) — exit unless positively `Foreign`

Revision 3 still under-sold the frequency. **The bow-out is a per-minute hot path on Windows, not a
once-per-logon edge**: `crash_recovery.rs:494` sets `<Interval>PT1M</Interval>`, and `main.rs`'s own
comment states "the Run-key-vs-tick race is absorbed by the exit-0-if-healthy guard". Task Scheduler
attempts a launch every 60 seconds. A verdict of `Unknown` meaning "stay resident" would therefore give
a transient lookup failure **up to 1440 chances a day** to strand a permanent duplicate tray process.
That is not a residual risk; it is a defect generator.

**The rule is therefore inverted from revision 3, and this is not optional:**

```
exit(0)  iff  healthy_aztec_on_port() && owner != PortOwner::Foreign
```

| Verdict | Action | vs. today |
|---|---|---|
| `Ours` | exit(0) | identical |
| `Unknown` | **exit(0)** | identical |
| `Foreign` | stay resident, surface the error | **the fix** |

The check may only ever **add** a reason to stay resident, never remove one. This makes item 7 exactly
behaviour-preserving on every path except the attack it closes.

**Security cost of this polarity, stated plainly**: an attacker who can force the lookup to fail keeps
the attack. That cost is small — the attacker is *holding the listening socket*, so their row is in the
table; forcing `Unknown` means releasing the port, which forfeits the squat. Trading a theoretical
attacker-induced-lookup-failure path for exact behavioural parity on a path that runs 1440×/day is the
right trade.

**The two-instance Windows test remains a hard gate anyway.** This is the item I have now been wrong
about twice — deferred for a false reason, then declared innocuous when it was not. That argues for
testing the design rather than trusting the argument.

**Item 5 — the guard is the bind, not a clock:**

```
start() ──> bind :59833 ──ok──> [ sweep_cache_on_start (F-06) ]   ← existing
                                [ reap_orphaned_prove_dirs   ]   ← item 5, same slot
```

Winning the bind means no other instance is serving (Fact 7), which is precisely why F-06 put its
sweep here (Fact 8).

### Non-obvious mechanics

**Item 5 — the guard is the bind; the floor is 24 h, and that number is deliberate.**
`bind_with_retry` waits up to 5 s for a prior instance to release the port (`bind.rs:16-18`), so we can
win the bind while a predecessor is still completing an in-flight proof.

Revision 2 sized the residual floor from `PROVE_TIMEOUT = 300s`. **That was wrong** (Fact 10, corrected):
the workspace outlives the timed region on both ends, so a 300 s floor can delete a live directory.

The correct-by-construction answer is a per-workspace advisory lock (`flock` / `LockFileEx`) held for
the workspace's lifetime, reaped only on a successful non-blocking acquire. **Deliberately not built**,
because it is disproportionate to the finding: F-08a is Med/Low and the harm is *disk residue
accumulating over months*. There is no value in reaping aggressively.

So: **bind-win as the guard, plus a 24-hour floor** — roughly 240× the longest possible proof. The race
is not closed by argument, it is made unreachable by ratio. Recorded as such, with the lock design named
as the upgrade path if same-session reaping is ever needed.

Sweep scope is `prove_tmp_parent()` only. `create_prove_tempdir` falls back to OS temp on non-Windows
when no data-local dir resolves (`bb.rs:144-146`); prefix-matching `prove-*` in a shared `/tmp` could
delete a stranger's directory. That residue stays unreaped, deliberately.

**Item 2 — bind the mount source to `$APPIMAGE`. (Revision 3 reverses revision 2 here.)**

Revision 2 proposed a two-`stat` mountpoint test and rejected the `/proc/self/mountinfo` design on the
grounds that a poisoned `$APPIMAGE` defeats both. **That was self-contradictory and the fresh pass
caught it**: mountinfo carries the *mount source*, so binding "the mount our exe lives on" to canonical
`$APPIMAGE` rejects exactly that attack. I rejected the stronger design using an argument that only
applied to the weaker one. That is motivated reasoning, and it is reversed.

The rule is: *find the mount containing `current_exe()` in `/proc/self/mountinfo`; its source must
canonicalize to `$APPIMAGE`.* Mountinfo content arrives as a `&str` parameter, so `autostart.rs:1789`'s
table test still works and no ambient read enters the pure function.

**Gated on a real fixture (Phase 0).** The fixture must be captured from a genuinely built AppImage,
never hand-written — inventing what a real system prints is precisely the `NTE_NOT_FOUND` mistake. If
the fixture cannot be obtained, item 2 falls back to the two-`stat` mountpoint test **explicitly
labelled partial hardening** in the code, the plan, and the report — not silently shipped as closure.

Residual either way: `/usr`-as-its-own-filesystem passes the weaker test. Stated, not hidden.

**Item 3 — the cap cannot be guessed, and one sample is not a maximum.** Measure a real proof via
`test:e2e:remote`, then set the cap from *headroom over the largest plausible circuit*, not from the
sample. This is the only change in the plan that can break proving for real users.

**Item 3 closes F-11's body-bound facet only (rev 3).** The finding also names two things this plan
does not fix, and they are listed rather than quietly dropped:
- **No caller `AbortSignal`** — optional cancellation is additive and UX-neutral; **in scope**, tested.
- **`"proved"` is emitted before the body is read** (`accelerator-prover.ts:560-565`), so the UI claims
  success while bytes are still arriving. Correcting the order **changes what the user sees** and is
  therefore **explicitly deferred to the owner**, alongside F-07/F-10 — not counted as closed.

### Trade-offs and alternatives not taken

- **Item 1: copy four lines rather than share a helper.** Revision 1 proposed a shared pure
  `system32_tool(hardcoded_exists: bool, …)`; both auditors called that "testing a model, not the
  production wiring". Since a real Windows test is available, the honest fix is to make `schtasks_exe`
  match `certutil_exe` and test **both production resolvers** under a poisoned `SystemRoot`.
  **Scope honesty (rev 3):** this does not *eliminate* the env fallback — `certutil_exe:41-46` keeps it
  for non-standard Windows roots, and the CI test passes because `C:\Windows` exists on the runner. The
  fix closes F-13 exactly as the finding states it (make the resolver match its hardened sibling); the
  residual is the sibling's own documented residual. `GetSystemDirectoryW` for both is the complete fix
  and is named as a follow-up, not smuggled in as closure.
- **Item 2: device ids over `/proc/self/mountinfo`.** Codex preferred mountinfo (it could additionally
  bind the mount source to `$APPIMAGE`); rejected as disproportionate given the corollary-2 argument,
  and because a fail-closed mountinfo design risks breaking autostart for legitimate users.
- **Item 3: parameterize rather than write a sibling reader.** Direct consequence of Fact 3.
  `readJsonBounded`'s hardened empty-chunk / partial-body defences were found by five rounds of
  adversarial review; a second reader would have to re-earn them.
- **Item 4: a separate constant.** `/health` stays 64 KiB. Sharing `/prove`'s cap would leave sink C
  effectively unbounded. Share mechanics, not policy.
- **Item 6: conjunction over deletion.** Trades no security for availability; both auditors upheld it.

---

## Phases

Ordered by risk, with the two items that can hurt users gated behind real-OS evidence. Every phase
runs every validation layer, per the owner's answer: `bun run test` + `bun run lint` +
`bun run lint:actions`, the targeted suite, and mutation-proof of that phase's tests.

### Phase 0 — two experiments, no production code · added in rev 3

Both were buried inside later phases in revision 2, where a negative result would have arrived after
the design depended on it. The fresh pass was right to pull them forward.

1. **Capture a real `/proc/self/mountinfo` from a built AppImage** (item 2's fixture). Never
   hand-written. If unobtainable, item 2 downgrades to the two-`stat` test labelled partial hardening.
2. **Prove `GetExtendedTcpTable` resolves a same-user socket owner without elevation on CI** (item 7).
   A throwaway Windows-only test opens a listener and asserts the resolved owner image matches
   `current_exe()`. If it fails, item 7 reverts to deferred — cheaply, and before anything is wired in.

**Gate**: both answers in `lessons/phase-0.md`. Neither blocks phases 1–3.

### Phase 1 — F-13 + F-12 (items 1, 2) · risk: minimal

Two contained fixes, both with real tests. **Item 1's test must be seen to run on the Windows leg** —
that is the whole point of it.

- Item 1 tests: poisoned `SystemRoot`/`windir` cannot redirect either resolver (real Windows).
- Item 2 tests: extend `autostart.rs:1789` with `APPDIR=/`, a non-mountpoint ancestor, and the genuine
  AppImage case.

### Phase 2 — bounded reads (items 3, 4) · risk: **highest**

1. Measure a real proof via `test:e2e:remote`; record the number **and** the headroom argument.
2. Parameterize `readJsonBounded`; route `/prove` through it; bound the base64 string before
   `Buffer.from` (`accelerator-prover.ts:567`).
3. Bound `probe.rs`'s reads at its own 64 KiB.

Tests: over-cap rejected; stalled body hits the deadline; empty-chunk spin terminates (existing
hardened cases must not regress); a normal proof passes unchanged. Gate additionally requires
`test:e2e` green and the measured size stated in the commit message.

### Phase 3 — witness reaper (item 5) · risk: moderate (deletes files)

`reap_orphaned_prove_dirs`, called from the post-bind block at `server.rs:267` beside
`sweep_cache_on_start`. Never follows symlinks (`symlink_metadata`), never recurses above the parent,
sweeps only `prove_tmp_parent()`.

Tests: an orphan is removed; the sweep **does not run before the bind succeeds** (the test that
encodes Fact 8 — and the one that fails if someone later moves the call); a symlink is not followed;
a permission error does not panic.

### Phase 4 — F-03, both remaining sinks (items 6, 7) · risk: low logic, high care

Sink B first (pure, self-contained), then sink A.

Sink A proceeds only if Phase 0's experiment 2 passed. Add the `Win32_NetworkManagement_IpHelper`
feature to `core`'s existing `windows-sys` entry, add `core/src/server/owner.rs`, and make the bow-out
at `main.rs:294-303` require `PortOwner::Ours`. Multiple owner rows for the port ⇒ `Unknown`, never a
guess. Also fix the false comment at `main.rs:336` ("Matching the version proves we are observing our
own server") — the sentence that made sink B look safe.

Tests: pure `classify` table (all three verdicts) on every platform; sink B's
forged-probe-without-bind-ownership case; and — per the fresh pass — a **real two-instance Windows
test**, not only the self-listener probe. The self-listener proves the API is available; it does not
prove packaged dual-launch behaviour or path normalisation. Additionally an injected-lookup-failure
test asserting `Unknown` ⇒ stays resident.

### Phase 5 — reconcile, review, ship as a stack

1. **Reconcile the record.** Update `report.md` + `report.html` remediation tables. Record, as
   residuals rather than closures: F-12's `/usr`-as-own-filesystem case (and the partial-hardening
   downgrade if Phase 0 failed), item 1's retained env fallback, item 7's `Unknown`-exits polarity and
   its security cost, F-11's deferred phase-ordering, and the excluded `SO_EXCLUSIVEADDRUSE`.
2. **`/code-review max --fix`** over the whole branch.
3. **Codex iteration loop** — fresh session per round, `xhigh`, scoped to *these* changes: find bugs
   and improvements. Iterate until a round returns nothing load-bearing. **Explicitly ask each round to
   flag over-engineering**, and treat "this is more machinery than the finding warrants" as a valid
   finding to act on. The last two PRs each took five rounds and every round found a defect in the
   previous round's fix — budget for that, do not assume round 1 is clean.
4. **Ship as a stack of PRs**, not one blob. `gh stack` (github/gh-stack v0.1.0) is installed.

   One PR per phase, in dependency order — each reviewable alone, each independently revertable:

   | PR | Branch | Contents |
   |---|---|---|
   | 1 | `…-p1-resolvers` | F-13 + F-12 |
   | 2 | `…-p2-bounded-reads` | F-11 (+ `AbortSignal`) + F-03 sink C |
   | 3 | `…-p3-witness-reaper` | F-08a |
   | 4 | `…-p4-f03-identity` | F-03 sinks B + A |
   | 5 | `…-p5-report` | report reconciliation |

   `gh stack init` → `gh stack add <branch>` per phase → `gh stack submit`. Phase 4 is the one a human
   should read hardest; the stack exists so it is not buried under four unrelated diffs.
5. **Watch CI on the whole stack**, especially the `windows-build` leg — items 1 and 7 are inert
   without it.

---

## Security & Adversarial Considerations

- **Threat model**: unchanged — a same-user local process, plus a hostile endpoint answering the SDK.
- **Least privilege**: no new capabilities. One added *feature flag* on an existing production
  dependency (item 7); **zero new crates**.
- **Input validation at trust boundaries**: items 3 and 4 are exactly this — the `/prove` and `/health`
  bodies are the boundary and both read unbounded today.
- **New attack surface this plan introduces**:
  - Item 5 **deletes files** — a symlink or traversal on `prove-tmp` entries would make the reaper an
    arbitrary-delete primitive. Mitigated by `symlink_metadata`, single known parent, no recursion up.
  - Item 3's cap is a **self-inflicted DoS knob** if set too low.
  - Item 7 parses OS structures via FFI — a malformed table must yield `Unknown` (fail safe), never a
    panic in the startup path.
- **Regression risk**: item 3 must not weaken `readJsonBounded`'s existing defences.
- **Corollary-2 discipline**: where a fix only defends against an actor who already holds the
  capability (item 2's residuals), that is stated rather than counted as closure.

---

## Testing plan

| Item | Test | Runs on |
|---|---|---|
| 1 | poisoned `SystemRoot` cannot redirect either production resolver | **real Windows** |
| 2 | `APPDIR=/` and non-mountpoint ancestors no longer prove provenance | all |
| 3 | over-cap rejected; stall hits deadline; spin terminates; base64 bounded | all |
| 3 | a real proof still succeeds | CI e2e |
| 4 | over-cap `/health` body rejected at 64 KiB | all |
| 5 | orphan removed; **not run before bind**; symlink not followed | all |
| 6 | forged probe without bind ownership does not commit the floor | all |
| 7 | `classify` table; a child-process listener classifies `Foreign` | all + **real Windows** |

Every one mutation-proved. Nothing ships with a test that has not been seen to fail.

---

## Risks and rollback

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Item 3's cap too low → proving breaks | Low | **High** | Measured + headroom argument; e2e gate |
| Item 7 misclassifies and the app won't start | Low | **High** | Fail-safe direction: `Unknown` ⇒ stay resident (today's behaviour) |
| `GetExtendedTcpTable` needs elevation | Medium | Low | Proven on CI in Phase 4 *before* wiring; item 7 reverts to deferred |
| Item 5 deletes a live workspace | Low | High | Bind-win gate (Fact 8) + residual age floor |
| Item 2 residuals mistaken for closure | Medium | Low | Stated in the plan, the code comment, and the report |

Each phase is one or two commits; rollback is a revert of that phase. Nothing is irreversible; no
state migration.

---

## Out of scope

F-07, F-10 (consent UX — owner), F-09 (owner-deferred), F-11's `"proved"` phase-ordering fix (visible
to users — owner call), outline B's fourth-instance sweep (falsified — the nearest cousins
`BB_BINARY_PATH` and `LOCALAPPDATA` are documented gated overrides), and the F-06 cross-process residual
(`scripts/download-bb.ts`, already accepted).

### A-2 — `SO_EXCLUSIVEADDRUSE`: deferred, with the reason corrected

Revision 2 deferred it as "needs `socket2`, a genuinely new crate". **That reason was wrong** —
`socket2 0.6.4` is already in `core/Cargo.lock:1368`, and this repo's own precedent promotes a
transitive crate to a direct edge without calling it new (`core/Cargo.toml:41-43`).

The honest reason to defer is **availability, not dependency cost**. `SO_EXCLUSIVEADDRUSE` changes
rebind semantics, and `bind_with_retry` exists precisely to wait out a prior instance's listener during
an in-place updater restart (`bind.rs:11-18`). Making the socket exclusive risks turning a restart
overlap into a failed start — the exact failure class this plan refuses to introduce. It also defends a
*different* attack from item 7 (incumbent-socket theft, not first-to-bind).

Re-raise it alongside item 7's implementation, with a restart-overlap test, or not at all.
