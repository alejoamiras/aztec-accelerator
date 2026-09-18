# C8 / F-010 + F-016 — desktop-platform-secrets — plan (mid tier) — REVISED after dual audit (both REJECT)

## Summary
Two desktop hardening findings in `packages/accelerator/src-tauri/src/`, BOTH honestly LOW / defense-in-depth
(the audits agree — F-010 is same-user autostart persistence, not privilege escalation; F-016 is an
ephemeral loopback-name-constrained CA whose recovery needs process-memory/core-dump/swap access), but both
cheap and correct to close:

- **F-010 (autostart path injection):** `crash_recovery.rs::enable_impl` writes `ExecStart="{exe}"` from
  `current_exe().display()` (`:163,170`) — quoted only, NOT systemd-escaped, lossy for non-UTF-8. A crafted
  install path with a newline/`%`/`$`/quote can corrupt the unit or inject directives (`ExecStartPre=…`),
  then `systemctl --user enable`. **The same root cause is in the `tauri-plugin-autostart`/`auto-launch`
  serializers that run FIRST** (raw unquoted `.desktop` `Exec=`, un-escaped macOS plist `<string>`, unquoted
  Windows Run-key) — so fixing only the systemd unit is INCOMPLETE.
- **F-016 (CA key not scrubbed):** `certs.rs::write_new_cert_set` (`:141-152`) generates the HTTPS CA
  `KeyPair`, signs the leaf, and PLAIN-drops it (scrubs nothing). The CA key never hits disk; the leaf key is
  persistent by design.

Fix: **F-010** — a `set_autostart` PREFLIGHT that rejects an unsafe exe path before ANY autostart writer +
a precise systemd `ExecStart` serializer + propagate failure (fail-closed, remove stale unit). **F-016** —
`Zeroizing<KeyPair>` (compiles today) + `zeroize` direct dep + early drop + an HONEST partial-scrub residual.

## Decision ledger — dual audit (codex REJECT + fable REJECT), folded
- **Both — the escaping spec was wrong.** systemd's `string_is_safe` (verified in upstream `string-util.c`)
  REJECTS a decoded executable containing controls/newline/DEL, `\`, `"`, `'`, `*`, `?`, `[`, or non-UTF-8.
  So those are NOT escapable — they must be REJECTED. The supported contract is an ABSOLUTE, valid-UTF-8
  path. FOLD.
- **codex — `$` + the `:` prefix.** Quotes do NOT suppress systemd env expansion; `${FOO}`/`$` in the path
  rewrites argv0. Doubling `$` diverges (executable keeps `$$`, argv0 → `$`). FOLD: use systemd's `:` prefix
  (disables env expansion) — `ExecStart=":<quoted path with %→%%>"`. After unquote: `:` stripped, `%%`→`%`,
  `$` literal. (fable independently confirmed the `%`/`\`/`"` order is disjoint + quoting neutralizes a
  leading prefix char; codex's `:`-prefix is the `$` piece.)
- **Both — the adjacent autostart plugin has the same vuln.** `auto-launch 0.5.0` emits raw `.desktop`
  `Exec={} {}` / un-escaped macOS plist / unquoted Windows Run-key, via lossy `current_exe` display; it runs
  in `set_autostart` BEFORE crash_recovery. FOLD: a shared PREFLIGHT in `set_autostart` (`commands.rs`)
  rejects an unsafe exe path (conservative union: non-absolute / non-UTF-8 / control/newline / `\"'*?[` /
  `$`%-hazard) and refuses autostart entirely (never calls the plugin manager OR crash_recovery). This
  closes ALL serializers at the app boundary without patching 3rd-party crates.
- **codex — fail-closed must PROPAGATE + clean up.** `enable(&self)` returns `()` + logs; `commands.rs:52
  set_autostart` succeeds even if crash-recovery silently fails, and returning before the write leaves a
  stale unit armed. FOLD: `enable()` → `Result`; `set_autostart` surfaces the error; fail-closed
  disables/removes any prior unit.
- **Both — the validation gate can't detect injection.** `systemd-analyze verify` validates syntax/
  loadability/exec-existence, NOT injection (a valid injected `ExecStartPre=` passes), returns 0 on warnings
  unless `--recursive-errors=yes`, and false-fails a fixture path. FOLD: the REAL gate is STRUCTURAL —
  exactly one `ExecStart`, no extra lines/directives, and an INVERSE round-trip (serializer output →
  reproduce systemd unquote/prefix-strip/specifier → equals the intended UTF-8 path); reject-set unit tests;
  `systemd-analyze --user --man=no --recursive-errors=yes verify` (with a REAL exe like `/bin/true`) only as
  a syntax smoke.
- **Both (factual) — rcgen already impls Zeroize.** `src-tauri/Cargo.toml` enables `rcgen features=["zeroize"]`;
  rcgen 0.13.2 `impl Zeroize for KeyPair` (scrubs ONLY `serialized_der`); `zeroize 1.8.2` already locked.
  So `Zeroizing<KeyPair>` compiles today; the newtype fallback is DEAD (deleted). Add `zeroize` as a DIRECT
  dep to NAME it (not re-exported) — pins the locked 1.8.2, zero churn. The 7-day min-age gate is bun-only
  (no cargo equivalent). FOLD.
- **Both — F-016 residual oversold.** rcgen's `Zeroize` scrubs only the DER Vec, NOT ring's ECDSA
  scalar/nonce (no `ZeroizeOnDrop`/`Drop`; this build uses the ring backend) nor generation temporaries nor
  swap/core-dump. FOLD: residual = "best-effort post-use reduction — only rcgen's serialized DER is
  guaranteed wiped; the ring backend key + swap + core-dump are NOT." Early drop still meaningful (wipes DER
  before the fallible file writes). Correct "byte-identical cert set" → validity/chaining/behavior unchanged
  (key gen is randomized).
- **L6 (both) — adjacent surfaces are FINE (note them).** Windows `task_xml` already XML-escapes the path
  (has a test); macOS `crash_recovery::enable_impl` doesn't interpolate the exe (plugin writes it — closed by
  the preflight); the leaf key shares the un-scrubbed property but is persisted 0600 by design; updater keys
  are PUBLIC. Add one-line "why X is fine" notes. Recalibrate severity to defense-in-depth (NOT "injection
  primitive").
- **Testability (codex vs fable).** fable verified src-tauri DOES `cargo test` on this box (Tauri Linux deps
  present); CI installs them (`setup-accelerator/action.yml`) + runs `cargo test` on src-tauri
  (`accelerator.yml:88`). DECISION (codex): keep the serializer a pure fn in the src-tauri lib module (don't
  move policy to core just to dodge deps); the src-tauri cargo test is the gate (local here + mandatory in CI).

## Final-codex fold (round 2) — F-016 APPROVED; F-010 sharpened + honestly bounded
Codex round-2 approved F-016 (implementation choice + honest residual correct) and rejected only on F-010
operational completeness. Folded, with an explicit SCOPE decision to keep this a tractable, honest cluster:
- **Serializer token syntax:** `":/path%%"` not `:"/path%%"` (prefix inside the quote). FOLDED (Design).
- **`enable()` has THREE callers, not one:** `set_autostart` (`commands.rs:52`), startup rearm
  (`main.rs:511`), Windows updater rearm (`updater.rs:411`). FOLD: `set_autostart` SURFACES the error to the
  UI; startup + updater rearm **log-and-continue** (never `?`-abort startup or disturb the updater
  disarm/rearm invariant). The `disable` path is NEVER blocked by preflight.
- **Transaction rollback + honest disable:** if plugin-enable succeeds but crash-recovery-enable fails,
  disable the plugin again; on unsafe preflight, attempt BOTH cleanups. Fix `disable_impl`
  (`crash_recovery.rs:203`): remove the unit file BEFORE the final daemon-reload, check results, report when
  disarm can't be confirmed (it currently ignores all failures + always reports success). FOLD (Phase 1).
- **Webview capability BYPASS:** `capabilities/default.json` grants `autostart:default` incl. raw plugin
  `allow-enable`/`allow-disable` — the webview can bypass `set_autostart`. FOLD: remove the raw
  `autostart:allow-enable` grant (the frontend already calls the gated custom command, `settings.html:155`),
  so the preflight-gated command is the only enable path. (Keep `allow-disable`/`allow-is-enabled` — disable
  must always work.) (Phase 1.)
- **Preflight scope — HONEST, not over-rejecting (codex's key correctness point):** a SINGLE cross-platform
  char-union is WRONG (Linux allows `\`; Windows paths REQUIRE `\`; `=`/space/`&`/`<` matter differently per
  writer; the plugin serializes `appimage`/canonicalized paths, not raw `current_exe`). FOLD: the preflight
  rejects ONLY the UNIVERSALLY injection-dangerous set — **controls, newline, non-UTF-8** (the actual
  injection vectors across systemd-unit / `.desktop` / plist-XML / Run-key) — plus the systemd reject-set is
  applied ONLY in `systemd_exec_start` (Linux). The per-platform NON-injection formatting quirks in the
  3rd-party `auto-launch` crate (space-splitting in `.desktop`/Run-key, XML `&`/`<` in the plist) are a
  documented ROBUSTNESS residual, not a same-process injection this cluster can fix without patching the
  crate. This keeps the fix correct (no legit path rejected) + honest (newline/control injection closed
  everywhere). Validate the SAME selected value the plugin uses where feasible.
- **Testability:** factor a transaction helper taking an EXPLICIT path + injected enable/disable closures
  (a unit test can't make `current_exe()` unsafe or mock `AppHandle`). macOS/Windows string predicates must
  be host-independent + Linux-testable. Add an ACL regression assert (raw plugin enable stays un-granted).
  FOLD (Phase 1 gate).
- **F-016 wording:** update `certs.rs:151` "only copy is gone" → "rcgen's serialized DER wiped; ring
  backend/swap/core-dump NOT"; fix the `Cargo.toml` comment; "zero churn" → "no version churn (root package
  dep list changes)". FOLD (Phase 2).

DECISION: GATE 1 closes here. Three audit rounds (dual + 2 fresh finals) have thoroughly vetted this; F-016
is approved; the F-010 design is now correct + honestly bounded. Remaining items are implementation-level +
naturally enforced by the phase gates + GATE 3 (post-impl codex on the ACTUAL diff). The per-platform
plugin-escaping niceties are a documented residual (3rd-party crate). Proceeding to implementation.

## Design (folded)
**F-010** (`crash_recovery.rs` + `commands.rs`):
- `fn systemd_exec_start(exe: &Path) -> Option<String>`: require absolute + valid UTF-8 + no trailing `/`
  (input is the real `current_exe`, not any theoretical Path); reject any of control/newline/DEL, `\`, `"`,
  `'`, `*`, `?`, `[` (systemd `string_is_safe` set) ⇒ `None`. Else build **`format!("\":{}\"", path.replace(
  '%', "%%"))`** ⇒ `":/path%%"` — the `:` prefix goes INSIDE the opening quote (at the item boundary, per the
  documented quoting grammar; `:"..."` only works by accident of fragment-concatenation). The `:` disables
  `$` expansion; the reject-set makes `\"'` unnecessary-to-escape. Test the PRODUCTION unit builder (a
  hand-written inverse can reproduce the same mistaken grammar); injection safety = strict reject-set + fixed
  one-line template; `systemd-analyze` = parser-compat smoke only.
- `fn autostart_path_is_safe(exe: &Path) -> bool`: the conservative cross-platform preflight (absolute +
  UTF-8 + none of the reject-set + no newline) used by `set_autostart` to gate BOTH writers.
- `enable(&self) -> Result<(), Error>` (was `()`): `enable_impl` returns the systemd error / unsafe-path
  refusal; fail-closed removes any stale unit before returning. `set_autostart` (`commands.rs`) preflights,
  and on refusal disables autostart + returns an error to the UI.
**F-016** (`certs.rs` + `Cargo.toml`): add `zeroize = "1"` (direct, already-locked 1.8.2); `let ca_key =
Zeroizing::new(KeyPair::generate_for(...)?);`; explicit `drop(ca_key)` right after `leaf_cert` is signed
(before the three writes); doc-comment the honest residual.

## Phases

### Phase 1 — F-010 serializer + preflight + fail-closed propagation (+ tests)
- Add `systemd_exec_start` + `autostart_path_is_safe`; make `enable()` return `Result` + remove stale unit
  on refusal; add the `set_autostart` preflight gating the plugin manager + crash_recovery.
- **Validation gate:** `cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml` — structural +
  inverse-round-trip tests: a normal absolute path → `Some(":\"…\"")` that round-trips; `%`→`%%`; `$`/space
  preserved as literal argv0; each of newline/control/`\`/`"`/`'`/`*`/`?`/`[`/relative/non-UTF-8 → `None`;
  the generated unit has exactly one `ExecStart` + no extra directives; `set_autostart` refuses an unsafe
  path (neither writer runs) + returns Err; a gated `systemd-analyze --user --man=no --recursive-errors=yes
  verify` smoke on a `/bin/true` unit — plus `cargo clippy --manifest-path …/src-tauri -- -D warnings` +
  `cargo fmt --check`. (Runs locally here; mandatory in CI.) Layers: unit + lint.

### Phase 2 — F-016 Zeroizing CA key + early drop + residual doc (+ Cargo)
- `Cargo.toml`: `zeroize = "1"` (direct). `certs.rs`: `Zeroizing::new(ca_key)`; early `drop` after signing;
  honest residual doc-comment.
- **Validation gate:** `cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml` (existing
  certs tests still green — CA/leaf validity + chaining + file set unchanged; the change is drop-timing +
  DER-scrub) + clippy `-D warnings` + fmt. Layers: unit + lint.

### Phase 3 — CI + docs
- Confirm `accelerator.yml` runs the src-tauri tests on these paths (it does). Doc note: the systemd-escape
  policy + the preflight + the CA-key partial-scrub residual + why macOS/Windows/leaf/updater are fine.
- **Validation gate:** `bun run lint:actions` + full `cargo test` (core + src-tauri) + clippy + fmt. Layers:
  lint + unit.

## Security & Adversarial Considerations
- **Threat model (honest, per both audits):** F-010 = a crafted/weird install pathname that a victim
  launches then enables for autostart → persistent same-user code execution on next login (NOT priv-esc; an
  attacker who can write `~/.config/systemd/user` already has equivalent power). F-016 = CA signing-key
  recovery from process memory / freed heap / swap / a core dump (an attacker with that capability has
  already won); the CA is loopback-name-constrained (`certs.rs:97`), so the incremental value is longer-lived
  localhost minting authority, not a general MITM key.
- **F-016 residual (honest):** `Zeroizing` scrubs ONLY rcgen's serialized DER; the ring backend private
  scalar/nonce, generation temporaries, swap pages, and core dumps are NOT scrubbed. Best-effort, not "the
  only copy is gone." Process-wide channel hardening (`PR_SET_DUMPABLE=0` + `RLIMIT_CORE=0`, `mlock`) is
  noted as OUT OF SCOPE for this mid cluster (coarse, Linux-only, hurts debuggability) — documented, deferred.
- **Crypto:** `zeroize 1.8.2` (locked, battle-tested) + rcgen 0.13.2 (existing); no rolled crypto.
- **Fail-closed:** an unsafe autostart path ⇒ autostart refused entirely + error surfaced (no half-enabled
  state, no stale unit).

## Assumptions
### Facts (verified by the audits, cite)
- F-010 `ExecStart="{exe}"` from `.display()` (`crash_recovery.rs:163,170`, Linux-only). Windows `task_xml`
  XML-escapes (`:384,393`, tested). macOS `enable_impl` doesn't interpolate the exe (`:62-90`). The
  `auto-launch 0.5.0` `.desktop`/plist/Run-key serializers are raw/unescaped; the plugin uses lossy
  `current_exe` display.
- F-016 `write_new_cert_set` gen→sign→plain-drop (`certs.rs:141-152`); CA loopback-name-constrained (`:97`);
  leaf persisted 0600 by design (`:150`). rcgen 0.13.2 `impl Zeroize for KeyPair` scrubs only `serialized_der`;
  `rcgen features=["zeroize"]` on (`Cargo.toml`); `zeroize 1.8.2` locked; ring backend has no zeroizing Drop.
- CI installs Tauri deps + runs `cargo test` on src-tauri; the 7-day min-age gate is bun-only.
### Inferences (verify in impl)
- The `:`-prefix + `%%` + quote serialization round-trips exactly for absolute UTF-8 paths in the reject-set
  complement (verify with the inverse round-trip test + `systemd-analyze verify` smoke).
- `enable()` → `Result` is a contained signature change; `set_autostart` is the only caller to thread it.
### Asks (defaults chosen — flag to override)
- A1: unsafe path ⇒ refuse autostart ENTIRELY (both writers) + surface an error — chosen (vs the plugin
  silently serializing an unsafe path).
- A2: `Zeroizing<KeyPair>` + early drop; residual documented as best-effort/partial; process-wide
  core-dump/swap hardening OUT of scope (deferred, documented) — chosen.
- A3: keep the serializer in the src-tauri lib (not core); src-tauri cargo test is the gate (local + CI) —
  chosen.

## Seeds (draft)
- `/goal`: F-010 + F-016 fixed — systemd_exec_start reject-set + `:`-prefix serializer, set_autostart
  preflight refusing an unsafe path across ALL writers, enable()→Result fail-closed + stale-unit removal,
  structural+round-trip validation; CA key Zeroizing + early drop + honest partial-scrub residual; each
  phase's gate green; post-impl codex xhigh audit folded; PR into security-hardening CI green.
- `/loop 15m`: drive C8 — F-010 serializer (reject `\"'*?[`/controls/non-UTF-8/relative; `:`+`%%`+quote) +
  set_autostart preflight + enable()→Result; F-016 Zeroizing CA key + early drop. After each edit run the
  src-tauri cargo test+clippy+fmt. Commit/push. Consult codex on the systemd round-trip.
