# Lessons — piece 2 implementation

- **`pub(crate)` does not cross the lib→bin boundary.** `main.rs` is a separate bin crate that
  links `aztec_accelerator` as an external crate, so a `pub(crate)` item in the lib is invisible
  to it — and the call site was `#[cfg(target_os = "windows")]`, so every LOCAL check on Linux was
  green while all three Windows CI jobs failed with E0603. Fix was a proper seam (`quit_disarm()`
  in the lib, encapsulating the A5 semantics) rather than widening the lock's visibility.
- **The local gate must include `cargo clippy --all-targets`** — `bun run test`'s lint:rust is fmt
  only; clippy with `-D warnings` runs in CI. The unused-import failure (cfg'd-out Windows test
  imports) was catchable locally and wasn't.
- The wine harness (test:nsis) proved the POSTINSTALL hook before any Windows runner saw it — all
  six cases first try. Measured-under-wine continues to beat push-and-pray for NSIS work.

## Post-impl audit round — residual declared

Blocker 4's "T4 must drive the production heal across a deterministic barrier" is implemented at
the reconcile layer (T4) and the production-core layer (T6: the heal's authoritative under-lock
check and the full Remove flow run against real default-path files on a real Windows runner). The
remaining gap — deterministically publishing a marker BETWEEN the heal's fast path and its
under-lock re-check — requires a test-only injection point inside production code, which is the
exact test-lever-in-shipped-code pattern the piece-2 planning round rejected (fable's option (a)).
Documented residual, not silent: the interleaving's correctness rests on the lock-exclusion
argument (creation and re-check hold the same lock), which T6 exercises structurally.
