# PR-5 — bb-extraction cap + fail-closed legacy ca.key + SEC-02 note — #342

Branch `sec/pr5-bb-cert` off main. Commits 983fced (SEC-07 + SEC-02 note), ec3bb3a (SEC-08).

## What shipped
- **SEC-07**: `CappedReader` wraps the GzDecoder, aborts >512 MB cumulative decompressed (~8x the
  64 MB compressed cap) + a per-entry declared-size pre-check. Cap-parameterized inner fn for testing
  (a real bomb is impractical); 3 tests incl. the cumulative-counter backstop (junk entry before bb).
- **SEC-08**: `migrate_legacy_ca_key` → `Result` (retry + re-check); startup gate skips Safari HTTPS
  on `Err`. Fail-closed test via a read-only parent dir. Cross-platform (file I/O) → Linux-testable.
- **SEC-02**: strengthened the digest-fetch deferral note (circular trust; nightlies block in-app
  pinning; real fix = upstream signature).

## SEC-09 — DEFERRED (needs macOS manual smoke)
The rotation chain-check (`verify-cert -c leaf -r ca`) + atomic-swap rewrite are macOS-only, and the
audit (R5) requires a NEGATIVE macOS manual smoke to confirm the `-r` binding actually holds (its
semantics are underdocumented). This Linux-CI autonomous loop can't run that; implementing blind risks
silently breaking Safari HTTPS. Deferred to a Mac-validated follow-up (like safari-tls-ca-removal). The
pre-existing mid-swap window is narrow + macOS-only. **Action for the owner: implement + smoke on a Mac.**
LESSONS_FILE=implementations-plan/security-hardening-2026-06-09/lessons/phase-5.md
