# C8 / F-010 + F-016 — desktop-platform-secrets — plan (mid tier)

## Summary
Two contained desktop hardening findings, both in `packages/accelerator/src-tauri/src/`:

- **F-010 (systemd unit injection):** `crash_recovery.rs::enable_impl` writes a systemd user unit with
  `ExecStart="{exe}"` where `{exe}` = `current_exe().display()` (`:163,170`) — merely wrapped in double
  quotes, NOT systemd-escaped, and lossy for non-UTF-8. A binary path containing a newline, control char,
  `"`/`\`, or `%` corrupts the unit or INJECTS directives (a newline can append `ExecStartPre=…`; `%` is a
  systemd specifier). The path is `current_exe()` — normally the install dir, but a user-writable / crafted
  install path makes this an autostart-persistence injection primitive.
- **F-016 (CA key not scrubbed):** `certs.rs::write_new_cert_set` (`:141-152`) generates the HTTPS **CA**
  private `KeyPair` (`:143`), signs the leaf (`:146`), and plain-`drop`s it at function end (`:151`). A plain
  drop frees the heap WITHOUT scrubbing — the CA signing key lingers in freed memory / swap / a core dump.
  The CA key is never written to disk (good), but it should be zeroized + dropped as early as possible. The
  leaf key stays persistent by design (written to disk, `:150`).

Fix: **F-010** — serialize `ExecStart` with systemd escaping over the path BYTES (reject controls/newlines;
escape `"`/`\`; double `%`), fail-closed on an unsafe path, and validate the generated unit with
`systemd-analyze verify` (test-only, never install). **F-016** — wrap the CA key in `Zeroizing` + explicit
early `drop` right after leaf signing (before the disk writes); document the rcgen backend-allocation residual.

## Facts (verified)
- `enable_impl` interpolates `exe.display()` into `ExecStart="{exe}"` unquoted-beyond-the-literal-quotes,
  then `systemctl --user enable` (`crash_recovery.rs:134-197`). Linux-only (`#[cfg(target_os="linux")]`).
- `write_new_cert_set` generates `ca_key` (rcgen `KeyPair`, PKCS_ECDSA_P256_SHA256), `self_signed`s the CA,
  signs the leaf via `signed_by(&leaf_key, &ca_cert, &ca_key)`, writes ca_cert+leaf_cert+leaf_key, drops
  `ca_key` at end (`certs.rs:141-152`). CA key never written; leaf key persistent by design.
- `certs.rs` already has a `#[cfg(test)] mod tests` (`:458+`); src-tauri tests run in CI (C4 ran 25).

## Inferences (verify in impl)
- rcgen 0.13 `KeyPair` may not impl `Zeroize` directly ⇒ `Zeroizing<KeyPair>` may not compile; the achievable
  fix may be a `Zeroize`-newtype over the serialized key bytes OR explicit early-drop + documented residual
  (rcgen internally scrubs serialized DER but not every ring/aws-lc backend allocation — master-plan note).
- `systemd-analyze verify` is available on Linux CI runners; gate the verify test behind its presence.
- src-tauri may not `cargo test` on a GUI-less VPS (Tauri deps) ⇒ local GATE 4 may be core-only + escaping
  unit logic; the src-tauri tests run in CI (HARD RULE: Tauri-GUI ⇒ CI). Verify locally in impl.

## Asks (defaults chosen — flag to override)
- A1: F-010 fails CLOSED on an unsafe exe path (control/newline) — skip writing + enabling the unit, log a
  warning (better than writing a corrupt/injected unit) — chosen.
- A2: F-016 uses `Zeroizing` if `KeyPair: Zeroize`, else a minimal `Zeroize`-newtype over the key's
  serialized secret bytes; PLUS explicit early `drop`; residual documented — chosen.
- A3: `zeroize` crate is the battle-tested choice (already a transitive dep? verify) — chosen.

## Design (draft)
**F-010** (`crash_recovery.rs`): add `fn systemd_exec_start(exe: &Path) -> Option<String>`:
- take the path bytes (`exe.as_os_str().as_bytes()` on Unix); return `None` if any byte is a control char
  (`< 0x20`) or `0x7f` (fail closed).
- build the value: wrap in `"`; inside, `\` → `\\`, `"` → `\"`; then `%` → `%%` over the whole value
  (systemd specifier escaping). Return `Some(escaped)`.
- `enable_impl`: if `systemd_exec_start(&exe)` is `None`, log + return (don't write/enable). Use the escaped
  value in the unit template.

**F-016** (`certs.rs`): wrap `ca_key` in `Zeroizing` (or a `Zeroize`-newtype); after `leaf_cert` is signed,
explicit `drop(ca_key)` BEFORE the three `write_pem_file`s; document the residual in the fn doc-comment.

## Phases

### Phase 1 — F-010 systemd ExecStart escaping + fail-closed + unit verification
- Add `systemd_exec_start` + wire into `enable_impl`; escape/reject over path bytes.
- **Validation gate:** `cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml` (unit tests:
  plain path escapes to a valid quoted value; `%` → `%%`; embedded `"`/`\` escaped; a newline/control path
  → `None` (fail closed); + a `describe.skipIf(!systemd-analyze)`-style gated test that writes the generated
  unit to a temp file and asserts `systemd-analyze verify` passes — never installs) + `cargo clippy
  --manifest-path …/src-tauri -- -D warnings` + `cargo fmt --check`. If src-tauri can't build on the VPS,
  the escaping logic is extracted to a testable helper compiled in `core` OR validated in CI. Layers: unit + lint.

### Phase 2 — F-016 Zeroize the CA key + early drop + residual doc
- Wrap `ca_key` in `Zeroizing`/newtype; explicit early `drop` after leaf signing; doc the residual. Add the
  `zeroize` dep if not present.
- **Validation gate:** `cargo test --manifest-path packages/accelerator/core/… + …/src-tauri` (existing
  certs tests still green — the cert set is byte-identical; the CA key change is drop-timing/scrubbing) +
  clippy `-D warnings` + fmt. Layers: unit + lint.

### Phase 3 — CI + docs
- Confirm `accelerator.yml` runs the src-tauri tests on these paths; note the `systemd-analyze verify`
  gating. Brief doc note (README/comments) on the systemd-escape + CA-key-scrub + residual.
- **Validation gate:** `bun run lint:actions` + full `cargo test` (core + src-tauri, in CI if GUI-less
  locally). Layers: lint + unit.

## Security & Adversarial Considerations
- **Threat model:** (F-010) a crafted/writable install path escalating to autostart persistence via unit
  injection; (F-016) CA signing-key recovery from freed memory / swap / a core dump. Both are local-ish
  (attacker who can influence the binary path or read process memory), but cheap to close.
- **F-016 residual:** rcgen 0.13 scrubs serialized DER but not every backend (ring/aws-lc) allocation, and
  the OS may have already paged the key to swap before drop — `Zeroizing` + early drop is best-effort, not a
  guarantee; documented. The leaf key is persistent by design (localhost TLS).
- **Crypto:** `zeroize` (battle-tested) for scrubbing; rcgen 0.13 (existing) for key/cert; no rolled crypto.
- **Least privilege / fail-closed:** an unsafe exe path ⇒ no unit written (F-010).

## Seeds (draft)
- `/goal`: F-010 + F-016 fixed — systemd ExecStart byte-escaped + fail-closed + `systemd-analyze verify`
  test; CA key `Zeroizing` + early-drop + residual documented; each phase's gate green; post-impl codex
  xhigh audit folded; PR into security-hardening CI green.
- `/loop 15m`: drive C8 — F-010 systemd escaping/reject over path bytes + verify test; F-016 Zeroize CA key
  + early drop. After each edit run the src-tauri cargo test+clippy+fmt (or CI if GUI-less). Commit/push.
  Consult codex on the escaping spec + the Zeroize-newtype.
