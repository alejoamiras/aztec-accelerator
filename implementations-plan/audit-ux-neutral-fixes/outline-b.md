# Competing outline B — decompose by defect class, not by finding ID

Outline A ([`plan.md`](plan.md)) walks the audit's finding list and groups by *risk*. This outline
argues that grouping is an artifact of how the audit was written, not of how the code is broken.

## The claim

Six work items, three defect classes. Two of the classes contain findings the audit filed far apart.

**Class 1 — an attacker-settable ambient signal is treated as identity.** Three of the six:

| Finding | The ambient signal | Believed to mean |
|---|---|---|
| F-13 | `%SystemRoot%` / `%windir%` | "where Windows lives" |
| F-12 | `$APPDIR` | "the AppImage mount I came from" |
| F-03 sink B | an HTTP `/health` body on loopback | "my own server answered" |

Same bug, three times, in three languages' worth of idiom. The audit gave them three IDs and three
severities (3.8 / 5.4 / ~5.0), which hides that they are one lesson. Fixing them in one pass, back to
back, is how the *reviewer* catches the fourth instance — and there probably is one.

**Class 2 — a remote body is read without a bound.** F-11 (SDK, `/prove`, binary) and F-03 sink C
(Rust, `/health`, JSON). One cap policy, two languages. Outline A already pairs these; no
disagreement.

**Class 3 — lifetime tied to a destructor that a real exit path skips.** F-08a alone.

## Why this ordering beats A's

1. **It front-loads the cheap generalisation.** Doing F-13 → F-12 → F-03B consecutively means the
   pure-function-with-injected-signal shape is written three times in a row. A's risk ordering splits
   them across phases 1 and 4, so the third one is written a week later, from memory.
2. **It makes a fourth instance findable.** After Class 1 lands, one focused sweep — "what else do we
   read from env or from a loopback response and treat as proof of who we are?" — is a cheap,
   high-yield pass. A's ordering never creates the moment where that question is natural. The audit
   itself flagged this cluster: *"`trust/windows.rs` distrusts `%SystemRoot%`; `crash_recovery.rs`
   does not"* (report.md:346).
3. **It separates the one dangerous item cleanly.** F-11's cap is the only change that can break
   proving for real users. Under B it is its own PR with its own e2e gate, not phase 2 of five.

## Where B is worse

- **It invites a shared abstraction that should not exist.** The three Class-1 fixes live in
  `crash_recovery.rs`, `autostart.rs`, and `server/probe.rs` — two crates, no common domain. A
  literal "ambient signal" trait across them would be textbook over-engineering. B only pays off if
  the shared thing is a *review habit*, not a type.
- **It de-prioritises risk.** B ships F-03 sink B (touches update gating) in the first PR alongside
  a four-line path fix. A would not.
- **F-08a becomes an orphan phase** with no class siblings — fine, but it loses A's argument for why
  it comes after the reads are bounded.

## Minimum-viable variant (worth costing separately)

If the goal were "close the maximum number of audit rows for the minimum risk", the answer is
**F-13 + F-12 only**: two pure functions, two table tests, ~30 lines, zero behavioural surface, and
it closes one Low and one Medium. Everything else in scope carries real design content. This is the
floor, and it is worth naming so the owner can see what "do almost nothing" buys.

## Recommendation

Take **A's phase order** (risk-ordered, so the dangerous item is gated behind a measurement) but
adopt **B's framing in the commit messages and the report**: name the three Class-1 findings as one
defect, and run B's sweep for a fourth instance as an explicit step in the final phase. The two
outlines disagree about sequencing, not about content — and the sequencing argument is won by "the
item that can break proving must be gated behind an e2e measurement", which is A's.
