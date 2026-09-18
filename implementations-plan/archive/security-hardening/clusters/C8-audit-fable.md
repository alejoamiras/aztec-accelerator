# C8 / F-010 + F-016 — fable dual-audit leg — VERDICT: REJECT (2 blockers; both findings LOW/defense-in-depth)

## B1 (BLOCKING) — `systemd-analyze verify` does NOT detect injection
It validates syntax/loadability, not injection — a cleanly-injected valid `ExecStartPre=/bin/malware` loads
and exits 0. It is also env-noisy (pulls the host unit graph) and checks ExecStart-executable existence, so
a fixture path false-FAILS a correct unit. FIX: the real gate is a STRUCTURAL/string assertion on the
generated unit (ExecStart line == expected escaped value; exactly one ExecStart; no injected directives; no
extra lines) + a PROPERTY/round-trip test (random path bytes → systemd_exec_start → parse-back-with-a-
systemd-unquote-repro → round-trips to the original bytes). Keep systemd-analyze verify only as an OPTIONAL
syntax smoke (point ExecStart at /bin/true, assert stdout not exit code).

## B2 (BLOCKING, factual) — rcgen 0.13.2 KeyPair ALREADY impls Zeroize
VERIFIED: `src-tauri/Cargo.toml:58` already enables `rcgen features=["zeroize"]`; rcgen 0.13.2
`impl Zeroize for KeyPair` exists; `zeroize 1.8.2` already in `src-tauri/Cargo.lock`. So `Zeroizing<KeyPair>`
compiles TODAY — the newtype fallback is DEAD. Add `zeroize` as a DIRECT dep (to name Zeroizing; rcgen
doesn't re-export it) → pins the already-locked 1.8.2, zero churn. The 7-day min-age gate is bun/npm-only
(no cargo equivalent) → non-issue. FIX: delete the Inference + A2/A3 hedging; spec = "add zeroize direct
dep, wrap ca_key in Zeroizing".

## M3 (MED) — F-016 mitigation oversold
rcgen's `impl Zeroize` scrubs ONLY `serialized_der` (the PKCS#8 DER Vec), NOT the ring backend
`EcdsaKeyPair` scalar (no ZeroizeOnDrop, no Drop; this build uses the ring backend). So Zeroizing<KeyPair>
scrubs one copy (real gain vs the current scrub-NOTHING) but the ring private key persists un-scrubbed in
freed heap; and Zeroize does nothing for swap/core-dump. Early-drop timing is nearly irrelevant; the
zeroize-on-drop is the value, and it's PARTIAL. FIX: residual doc must say exactly this.

## M4 (MED) — resolve the testability OR by EXTRACTION
`systemd_exec_start(&Path) -> Option<String>` is pure byte-in/string-out, no Tauri surface → put it in
`accelerator-core` (tauri-free, buildable anywhere) + call from the thin `#[cfg(linux)]` enable_impl. CI
already runs `cargo test --manifest-path ../core/Cargo.toml` (accelerator.yml:111) separately from src-tauri
(:109) → the security-critical logic gets a gate on ANY GUI-less runner. (Fable verified Tauri Linux deps
ARE present on this box, so src-tauri DOES test locally — but extraction is strictly better.) Drop the OR.

## M5 (MED) — escaping spec is CORRECT + complete (verified) — state WHY + non-UTF-8 residual
systemd expands specifiers first, then unquotes+C-unescapes: `%`→`%%` survives to literal `%`; `\\`/`\"`
survive (disjoint from `%`) then C-unescape to `\`/`"`; the two passes are order-independent. Quoting the
whole path makes it the single argv[0] AND neutralizes a leading systemd prefix char (`@ - + ! :`).
Escaping only `\` + `"` inside `"..."` is sufficient. GAPS: (a) non-UTF-8 high bytes 0x80-0xFF pass through
un-rejected — can't inject (no newline) but may make systemd REJECT the unit (fail-closed downstream,
acceptable) → soften "non-UTF-8 handling correct" to "injection-safe; unit may be rejected". (b)
fail-closed-on-control is a POLICY choice (controls are representable via \n/\t C-escapes) — frame as policy
+ note the cost (a weird path silently disables crash recovery).

## L6 (LOW) — adjacent surfaces are FINE (add notes) + severity recalibration
Verified: Windows `task_xml` already XML-escapes the path (has a test); macOS enable_impl does NOT
interpolate the exe path (tauri-plugin-autostart writes it; the plist splice is safe). Leaf key has the same
un-scrubbed property but is persisted to disk 0600 by design (memory-scrub near-zero value). Updater keys
are PUBLIC (no secret). Add one-line "why X doesn't need the fix" notes. SEVERITY: both findings are
LOW/defense-in-depth — F-010 = self-injection-as-same-user (marginal gain = boot/login persistence); F-016 =
ephemeral name-constrained (127.0.0.1/::1/localhost) CA whose recovery needs process-memory read (attacker
already won). Honest framing = robustness + defense-in-depth, NOT "autostart-persistence injection primitive".

## Competing approaches
- F-010: SYMLINK indirection (ExecStart=/fixed/app-owned/symlink → current_exe; ExecStart = only
  app-controlled bytes, no escaping, robust to non-UTF-8; still delivers recovery where the plan fail-closes).
  Cleaner; but the master plan directs the escaping approach → keep escaping + property-round-trip test; note
  symlink as considered.
- F-016: close exfil channels process-wide (PR_SET_DUMPABLE=0 + RLIMIT_CORE=0 for core-dump; mlock/madvise
  for swap) — attacks the offline vectors Zeroize can't. Best = both, or explicitly scope core-dump/swap OUT
  with the residual naming these channels. (Likely scope-out for a mid cluster; document.)
