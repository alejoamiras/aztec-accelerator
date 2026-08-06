# c5-certs-trust — Claude (Opus) raw findings

## F-C5-1 — `install_ca_trust()` installs WHATEVER is at `certs/ca.pem` into the OS root store, with no anchor validation and no purpose scoping

**Impact.** Integrity + Confidentiality (a root CA the ATTACKER holds the key for); secondarily
Authorization (trust granted far beyond loopback TLS server auth). Blast radius: the whole user account —
on Linux `-t "C,,"` ⇒ TLS MITM of every HTTPS site for that user's browsers; on macOS/Windows the anchor
is installed with **no policy/EKU restriction** ⇒ additionally code signing, package signing, S/MIME,
timestamping. Vector local; complexity LOW (write three files, wait); privileges low (unprivileged
same-user, no elevation); **user interaction required — but it is the DEFAULT path**: the wizard's HTTPS
toggle is pre-checked for every install and upgrade (`commands.rs:520-531`, `https_default = true`).
**Confidence HIGH.** **CWE-295** primary; CWE-345, CWE-20, CWE-269. OWASP A08:2021 / A04:2021.

**Trace.** Attacker writes `~/.aztec-accelerator/certs/{ca.pem,localhost.pem,localhost.key}` (paths
`certs.rs:14-19`, `:36-45`). Same-user write is explicitly inside this project's threat model —
`migrate_legacy_ca_key` (`certs.rs:239-275`) treats a same-user-READABLE `ca.key` as severe enough to
refuse HTTPS bring-up. → `commands.rs:566-577` `complete_onboarding` → `enable_https_inner` (or
`commands.rs:343-351` from Settings) → `:392` `if !(certs::certs_exist() && certs::is_ca_trusted())` →
`certs.rs:145-152` `certs_exist()` returns TRUE for the planted set: it checks all three files exist,
`leaf_secs_remaining() > 0`, `load_rustls_config().is_ok()` (leaf↔key), `leaf_matches_ca()`
(`:162-185`, leaf↔CA signature) — **it never inspects the CA's own basicConstraints, keyUsage, subject CN,
or nameConstraints** → `certs.rs:455-457` `is_ca_trusted()` false (different SHA-1/serial/nickname) so the
branch is taken → `commands.rs:393` → `certs.rs:228-234` `generate_and_save()` **no-ops because
`certs_exist()` is true — the planted set is not replaced** → `commands.rs:394` → `certs.rs:440-452`
`install_ca_trust()` → `trust/mod.rs:112-114` → sinks:
- **macOS** `trust/macos.rs:24-39`: `security add-trusted-cert -r trustRoot -k <login keychain> <planted
  ca.pem>` — **no `-p` policy argument**, so the trust-settings dictionary carries only
  `kSecTrustSettingsResult` ⇒ unrestricted for ALL policies.
- **Windows** `trust/windows.rs:59-66`: `certutil -user -addstore Root <planted ca.pem>` — no EKU property
  ⇒ all purposes.
- **Linux** `trust/linux.rs:283-300`: `certutil -A -t "C,,"` into `~/.pki/nssdb` + every Firefox profile —
  correctly scoped to SSL-CA, but still a browser-trusted root for every HTTPS host.

**Missing control.** (a) **No anchor pinning before install** — nothing between `certs_exist()` and the
`security`/`certutil` invocation re-parses `ca.pem` to assert the invariants the design rests on:
`basicConstraints CA=true`, `CN = "Aztec Accelerator Local CA"`, `keyUsage = keyCertSign|cRLSign`, and a
CRITICAL `nameConstraints` whose permitted subtrees are exactly `127.0.0.1/32`, `::1/128`, `DNS:localhost`.
`leaf_matches_ca()` proves the set is SELF-CONSISTENT, not that it is OURS. (b) **No purpose scoping** —
macOS omits `-p ssl`; Windows sets no EKU. Linux already does this correctly, demonstrating the narrower
grant is both intended and achievable.

**Exploit.** Unprivileged same-user execution → generate an ECDSA CA **keeping the private key**, with no
nameConstraints and no EKU, plus a `localhost` leaf signed by it (~10 lines of rcgen/openssl) → write the
three files into `~/.aztec-accelerator/certs/`, creating the dir if the app has not run → user installs or
upgrades and completes the first-run wizard with the PRE-CHECKED "Encrypted Connection" toggle → the OS
consent prompt appears **attributed to Aztec Accelerator** → user approves, exactly the ceremony the app
told them to expect → attacker owns a root CA trusted by the user's browsers on all three OSes and, on
macOS/Windows, by Authenticode/installer/S-MIME as well.

**Why mitigations fail.** *Keyless CA* protects the anchor THIS APP MINTS — it is an invariant of
`write_new_cert_set` (`certs.rs:192-211`), not of the file at `CertPaths::live().ca_cert`; a planted CA
simply has a key. *Name constraints* exist only because `ca_params()` (`:95-115`) puts them there (rcgen
0.13.2 does write them critical — verified `certificate.rs:724-750`); a planted CA omits them and nothing
checks. *`leaf_matches_ca()`* is satisfied trivially by an attacker who mints both sides; its stated
purpose is detecting a torn `swap_into`, not provenance. *File permissions* (`:217-221` 0700, `:280-320`
0600/owner-only DACL) bound OTHER users, not the same user — who is already in the threat model (SEC-08).
*The OS consent dialog* names the application, not the certificate's constraints; a user cannot
distinguish "Aztec's loopback-constrained keyless CA" from "an unconstrained CA whose key an attacker
holds". *`trust/mod.rs:16-17`* asserts "the load-bearing control is the keyless CA (it can sign nothing),
so a trusted anchor in any store is harmless" — that reasoning holds only for anchors THE APP GENERATED,
and independently the "harmless" half is weaker than stated on macOS/Windows because the grant is not
purpose-scoped: RFC 5280 §4.2.1.10 restrictions apply only to certificates that CARRY a name of the
constrained type, so a leaf with no dNSName/iPAddress SAN (typical for code signing or S/MIME) escapes the
loopback constraint entirely.

**Instances.** `certs.rs:440-452` (`install_ca_trust`, no validation); `certs.rs:412-434` (`rotate()` →
`trust::trust_new_anchor(&staged.ca_cert)` at `:420` — staged files are also attacker-writable between
`write_new_cert_set` `:417` and the trust call); `trust/macos.rs:24-39`, `:161-168` (no `-p`);
`trust/windows.rs:59-66` (used by install `:132-147` and `trust_new_anchor` `:182-192`, no EKU);
`trust/linux.rs:283-300` (installs the unvalidated anchor; purpose scoping here is correct).

## F-C5-2 — Trust REMOVAL fails open on macOS and Windows: an execution failure is reported as "removed"

**Impact.** Integrity/Authorization — the user's explicit revocation silently does not take effect, and the
security state is misrepresented to the user AND to scripted uninstallers. Blast radius: one account,
persistent (a root anchor surviving uninstall indefinitely), multiplied by F-C5-1 (the surviving anchor may
be attacker-keyed and unrestricted-purpose). Vector local; complexity low; privileges low or NONE (this
fires on benign environmental failure too); the user actively clicks "Remove certificate trust" or runs
uninstall and is told it worked. **Confidence** high for the behaviour and fail-open direction; moderate
for real-world failure frequency. **CWE-390**, CWE-754, CWE-273. OWASP A04:2021 / A09:2021.

**Trace (macOS).** `commands.rs:492-515` `remove_https_trust` → `:500` `trust::remove_ca_trust(...)` →
`trust/mod.rs:117-119` → `trust/macos.rs:121` → the delete loop's first action is `keychain_sha1()`
(`:126-136`) → `:61-71` `keychain_sha1()` uses `Command…output().ok()?`, **swallowing spawn failure into
`None`**, and **never inspects `output.status`**, so a non-zero exit (locked keychain, wrong keychain path,
EDR/MDM block) yields empty stdout → `None` → `break` at `:134` with **no `delete-certificate` ever
attempted**, `delete_failed` stays false → `:140` `let remaining = keychain_sha1().is_some()` → false →
`:148-154` `StoreStatus { installed: false, detail: None }` → `trust/mod.rs:88-100` `removal_incomplete()`
false → `commands.rs:505-515` logs "Removed CA trust via Settings" (UI success) and `main.rs:486-507`
prints "macOS Keychain: removed / absent" and **returns exit code 0**, telling scripted cleanup the anchor
is gone.

**Trace (Windows).** Same entry points → `trust/windows.rs:160-174` `remove()` → `:125-130`
`delete_by_cn()` does `let _ = Command…output();` — **spawn failure and non-zero exit both discarded** →
`:165` `is_present_by_cn()` → `:70-76` `.map(|o| o.status.success()).unwrap_or(false)` — **a spawn failure
returns `false` = "not present" = removed**. Same sinks as macOS.

**The fix already exists on Linux.** `trust/linux.rs:328-350` defines `enum Present { Yes, No, Unknown }`
and maps ANY certutil spawn error or non-zero exit to `Unknown`; `:497` does
`let installed = !matches!(our_anchors_present(&bin, store), Present::No);` — unknown ⇒ still-trusted ⇒
loud failure. `:471-485` likewise reports `installed: true` when `certutil` is absent, with the comment
"so the caller FAILS the Settings/CLI action rather than reporting a clean removal it never performed".
macOS and Windows have no equivalent. **Root cause of the conflation:** the same helper serves two purposes
with opposite safety requirements — `windows.rs:83-89 is_present_by_serial` returning false on error is
FAIL-CLOSED (correct) for the launch gate, and FAIL-OPEN (wrong) for `remove()`.

**Missing control.** No tri-state ("present / absent / could not determine") on the macOS and Windows
removal post-checks, and no propagation of the delete command's own exit status. A removal must treat
"could not verify" as "still trusted", exactly as `Present::Unknown` does on Linux.

**Exploit / failure story.** User revokes (or uninstalls) → `security`/`certutil.exe` cannot execute or
cannot reach the store. Realistic triggers: EDR/AppLocker blocking `certutil.exe` (a commonly blocked
LOLBIN); `certutil_exe()` (`windows.rs:36-47`) resolving to a non-existent path on a non-standard Windows
root; a locked or non-default macOS login keychain; `--remove-ca-trust` invoked with a different `HOME`
than the one that installed it (`login_keychain()` `macos.rs:16-20` derives from `dirs::home_dir()`), e.g.
under `sudo -i`; or a same-user attacker shadowing/denying execute on the resolved binary → Settings prints
"trust removed", CLI exits 0 → the root anchor (possibly the attacker's from F-C5-1) remains trusted
indefinitely, including after uninstall.

**Why mitigations fail.** `removal_incomplete()`/`removal_failure_detail()` (`trust/mod.rs:88-100`) ARE the
loud-failure mechanism and work correctly — they are simply fed a `false` that means "unknown".
`commands.rs:502` flips `https_enabled = false` regardless, which stops the APP presenting the cert but
does nothing about the anchor left in the OS store. The macOS remove loop stops on a failed delete — but
only when a delete is ATTEMPTED; this failure short-circuits before any delete. The `-t` flag on
`delete-certificate` (`macos.rs:78-97`) correctly also clears user trust settings — irrelevant when the
command never runs.

**Instances.** `trust/macos.rs:61-71`, `:140`, `:148-154`; `trust/windows.rs:70-76`, `:125-130`, `:160-174`.
**Downstream amplifier (outside the strict file list, same consequence):** `nsis/hooks.nsi:109` — the real
Windows uninstall does NOT call the verified `--remove-ca-trust` CLI at all; it runs a bare
`ExecWait '"$SYSDIR\certutil.exe" -user -delstore Root "Aztec Accelerator Local CA"'` with no exit-code
check and no post-verification, so on the one path where removal matters most the failure is invisible by
construction. (`implementations-plan/https-by-default-onboarding-2026-07-09/audit-codex.md:217` already
recorded that the uninstaller CAN call the Rust CLI.)

## NON-FINDINGS (the focus questions, answered)

- **CA keyless-on-disk claim HOLDS.** `write_new_cert_set` (`certs.rs:192-211`) never writes the CA key;
  rcgen 0.13.2 `Certificate` does not retain the issuer `KeyPair`; `generation_writes_no_ca_key`
  (`:679-718`) pins it. `Zeroizing<KeyPair>` is real (rcgen `lib.rs:657-661`, `zeroize` feature enabled
  `Cargo.toml:70`) — it scrubs only `serialized_der`, leaving the ring/aws-lc private scalar unscrubbed,
  already documented as residual F-016 (`certs.rs:201-204`), not re-reported.
- **Name constraints are emitted correctly** — rcgen 0.13.2 `certificate.rs:724-750` writes
  `nameConstraints` CRITICAL, permitted subtrees only, both IP subnets and `DNS:localhost`;
  `tests/tls_handshake.rs` and `tests/trust_linux.rs:111-115` chain-validate through it. The objection in
  F-C5-1 is that nothing verifies it is present on the file being INSTALLED, and that it does not restrict
  non-TLS uses.
- **Command construction** — all shell-outs use `Command` + fixed argv, no shell string, no format-string
  interpolation of untrusted data; serials (`windows.rs:51-57`) and nicknames (`linux.rs:99-106`) are hex
  derived from our own parsed DER. No injection path.
- **Linux `certutil` path resolution** (`linux.rs:20-90`) canonicalizes, checks the resolved binary AND
  every ancestor for group/world-write and for ownership by a user other than root-or-us, and executes the
  canonical path it validated. No bypass for a DIFFERENT-user attacker; same-user planting is accepted by
  design (`owner == euid` passes) and crosses no boundary.
- **Firefox profile discovery** (`linux.rs:198-253`) — `PathBuf::join` with an absolute `Path=` under
  `IsRelative=1` is correctly neutralized by canonicalize + `starts_with(canonical $HOME)`; canonicalization
  failure fails closed (all profiles skipped).
- **`swap_into` interleavings — no finding.** Enumerated: (a) `ca=B, leaf/key=A` → caught by
  `leaf_matches_ca()`; (b) `ca=B, leaf=B, key=A` → caught by `with_single_cert` in `load_rustls_config()`.
  Both drive `certs_exist() == false` → regenerate, or `prepare_launch_https` (`main.rs:131-141`) resets
  `https_enabled`. **No interleaving serves a mismatched pair or downgrades to an untrusted anchor** — the
  old anchor is deliberately retained (`certs.rs:405-411`), so the fallback leaf always still has a trusted
  root. `.new.<pid>` staging names close the cross-process staging collision. Residue is leftover
  `*.new.<pid>` files with no cross-boundary impact.
- **`server/tls.rs`** — `with_no_client_auth()` correct for a browser-facing listener; rustls defaults
  (TLS 1.2/1.3, AEAD-only) sound; cert/key load failure fails CLOSED (HTTP-only, `main.rs:131-141` also
  resets `https_enabled`); the loopback Host guard receives the correct `HTTPS_PORT` (`tls.rs:31`), so a
  `:59833` authority replayed onto the TLS listener is rejected.
- **Web-reachability** — the router exposes only `/health` and `/prove` (`core/src/server.rs:271-273`).
  Nothing in this cluster is reachable from a web page; every entry point is a Tauri command behind a
  window-label guard (`commands.rs:349`, `:492`, `:575`, `:630`).
