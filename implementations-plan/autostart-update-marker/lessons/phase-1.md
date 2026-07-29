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
