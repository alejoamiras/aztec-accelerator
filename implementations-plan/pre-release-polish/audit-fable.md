# Fable audit — revision 1 of plan.md

Run in parallel with codex (`audit-codex.md`) on plan.md R1 + competing-outline.md, with the standard
packet (adversarial/security · assumption-attack · implementation-critique · recon reuse-check).

**Verdict**: `reject`

**Blocking findings as returned:**

1. The rename silently breaks autostart + Windows crash-recovery for upgrading users —
   `updater.rs` re-arms via `crash_recovery.rs:383` `std::env::current_exe()` in the pre-restart old
   process, writing a path N's installer deletes; unaddressed in plan.md and contradicted by recon B4.
2. plan.md Fact 12 / ASK-1 and competing-outline both rest on "no Windows stable N-1 exists" —
   `accelerator-v1.0.7` is stable and ships `Aztec-Accelerator-1.0.7-Windows-x86_64-setup.exe`, so the
   fixture fix is cheap and must precede the rename, plus a Run-key/ExecStart post-update assertion
   the current smoke lacks.
3. The compensating controls the security section leans on are weaker than claimed —
   `settings.js:91` + `style.css:220-232` render approved origins with none of the F-014
   bidi/selection hardening at `style.css:523-530`, and persistence after the change is silent with no
   notification path.
4. Phase 2's validation gate names "Release Smoke", which does not run on PRs
   (`release-accelerator.yml:381,472`), and Phase 2 mutates `bundle.icon` i.e. signed bundle contents,
   invalidating I3.

## Notable findings by ask

**Security** — HIGH: the revocation surface was never F-014-hardened, so the plan promotes exactly
the surface that work skipped. HIGH: silent persistence has no discovery path — *"Today the
affirmative checkbox **is** the notification… This, not 'rate-limiting', is the property F-014
actually bought."* MED: the verified badge is a 2-origin positive-only allowlist; no badge is
indistinguishable between "malicious" and "not curated". MED: today's mis-click is a bounded damage
window; after, an unbounded standing grant — "habituation" is asserted with zero measurement and is a
legitimate tie-breaker, not a refutation. LOW: `auth.rs:31-34` auto-approves a missing `Origin`
header, so the popup is not the whole boundary.

**Assumptions** — HIGH: F12 misstated; `_e2e-updater-windows.yml:8-14` states its own precondition
("Once a Windows STABLE exists, switch N-1 to the linux download-real-N-1 pattern") and it is now met.
MED: F7 undercounts — `_e2e-webdriver.yml:163,165` omitted while the plan's own security section
demands every such pattern be re-anchored. HIGH: I3 unsafe and Phase 2's gate broken. MED: I4 true in
letter, useless in scope — `:93` is a byte-identical decoy. MED: a missing inference entirely —
`mainBinaryName` contains a **space**, propagating into `.app/Contents/MacOS/`, NSIS
`${MAINBINARYNAME}`, the `.deb`'s `/usr/bin/`, and `main.desktop`'s `Exec=`.

**Implementation** — MED: D2 picks the right boundary for the wrong reason and lands the
half-migration it claims to avoid. MED: test map incomplete (5 touchpoints not 4;
`auth-flow.spec.ts:193-196` `isSelected()`; `README.md:375` hardcodes the WebDriver test count;
`authorization.rs:410` doc comment). MED: phase ordering re-invalidates Phase 1 — Phase 3 breaks the
launcher Phase 1's gate used, and no phase re-runs it. Reuse: recon A4/A7 honoured, nothing duplicated.

**Ordering** — competing-outline right on principle, both wrong on facts. Third option: fixture fix is
necessary but not sufficient — `updater-smoke-windows.ps1:212` sets the Run key itself and only
asserts `/health == N`, so it would not catch the autostart break.

## Disposition

All adopted except finding 1's crash-recovery half. See plan.md R2 "Audit adopted / rejected log" —
`main.rs:614-618` re-arms crash recovery from `current_exe()` on every launch when autostart reads
enabled, and `auto-launch-0.5.0/src/windows.rs:73-83` returns `true` regardless of path staleness, so
the re-arm fires. Verified directly; codex independently agrees. The **autostart** half is real,
confirmed, and became Phase 2. The macOS early-return variant is real and is adopted.
