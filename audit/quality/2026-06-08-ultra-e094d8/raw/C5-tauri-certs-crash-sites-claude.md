# Cluster C5 — tauri-certs-crash-sites (QUALITY)

**Verdict:** 4 findings. Severity spread: 1 structural (cert path-set as a 3-tuple data clump duplicated across mint/check/load/rotate), 2 local (the `security`-CLI wrapper duplication; the staged-vs-live path string duplication inside `rotate`), 1 local-cosmetic (duplicate doc/section markers). No architectural smells — the three modules are well-separated, the `CrashRecovery` trait dispatch is clean, and `verified_sites` is a legitimately dumb data table whose only lookup logic is centralized in one method. Primitive Obsession on origins/paths is a **consistently-applied codebase convention** (the whole `authorization` crate uses `&str`/`String` for origins; there is no `Origin` newtype anywhere) and is therefore explicitly a NON-FINDING per the DO-NOT-FLAG list — noted below for completeness, not certified.

---

## Finding 1 — The (CA-cert, leaf-cert, leaf-key) path triple is an undeclared Data Clump threaded through every cert operation

**Smell:** Data Clumps (Fowler, Bloaters). The same three filesystem paths — CA cert, leaf cert, leaf key — travel together as a group through *every* cert lifecycle operation, but are never modeled as one thing. They are re-derived from individual zero-arg path accessors (`ca_cert_path()`, `leaf_cert_path()`, `leaf_key_path()`) at each site, and separately re-derived as a *parallel* `.new` staged set inside `rotate()`. This is the canonical "three or four data items that always appear together" clump, here in path form.

**Maintenance impact:** structural. Blast radius: one file (`certs.rs`) but ~6 functions within it (`certs_exist`, `generate_certs`, `write_new_cert_set` callers, `load_rustls_config`, `leaf_cert_days_remaining`, `rotate`). Change frequency: moderate-to-hot — this is the TLS identity surface that already churned through the keyless-CA migration and the staged-rotation redesign (per the inline history comments). Each future change to the on-disk layout (e.g. adding an intermediate, renaming files, supporting a second served identity) is a multi-site edit.

**Concrete evidence — the triple appears as a co-traveling group at:**
- `certs.rs:90-95` `certs_exist()` — checks `ca_cert_path().exists() && leaf_cert_path().exists() && leaf_key_path().exists()` plus leaf validity.
- `certs.rs:128` `generate_certs()` — `write_new_cert_set(&ca_cert_path(), &leaf_cert_path(), &leaf_key_path())`.
- `certs.rs:204-205` `load_rustls_config()` — reads `leaf_cert_path()` + `leaf_key_path()` (the leaf half of the triple).
- `certs.rs:264-266` `rotate()` — a **second, parallel derivation** of the same triple as staged paths: `dir.join("ca.pem.new")`, `dir.join("localhost.pem.new")`, `dir.join("localhost.key.new")`. These hardcode the basenames `ca.pem` / `localhost.pem` / `localhost.key` that `ca_cert_path()` etc. *also* hardcode (`certs.rs:21,29,33`) — so the filenames live in two places.
- `certs.rs:284-286` `rotate()` swap — `rename(staged, ca_cert_path())`, `rename(staged, leaf_cert_path())`, `rename(staged, leaf_key_path())`: the triple again, now pairing staged↔live element-wise.
- `write_new_cert_set(ca_cert_dst, leaf_cert_dst, leaf_key_dst)` at `certs.rs:101-105` is a 3-path **Long Parameter List** that is itself the clump passed positionally — the smell's tell-tale signature (the function takes the clump apart into three params instead of receiving one object).

**Why it harms future change:** the live filenames (`ca.pem`, `localhost.pem`, `localhost.key`) are encoded in the path accessors *and* re-encoded as `*.pem.new` literals in `rotate`. To rename the leaf file you must edit `leaf_cert_path()` AND the `"localhost.pem.new"` literal AND keep the element-wise staged↔live pairing in the swap correct. Positional `write_new_cert_set(a, b, c)` invites a silent arg-swap (writing the leaf key to the cert path) that the type system can't catch — all three are `&Path`. Adding a fourth artifact (intermediate cert, chain bundle) means touching all six sites in lockstep.

**Smallest safe refactoring:** Extract Class / Introduce Parameter Object — a small `CertPaths { ca_cert, leaf_cert, leaf_key }` struct with a `live()` constructor (wrapping the current accessors) and a `staged(dir)` constructor (the `.new` set). Give it a `swap_into(&live)` method for the three renames and an `all_exist()` for the existence check. `write_new_cert_set` then takes one `&CertPaths` instead of three `&Path`s.

**What disappears:** the three free path accessors collapse into one struct; the duplicated `localhost.pem`/`ca.pem` basenames stop existing in two places; the positional 3-arg call (and its arg-swap hazard) becomes a single typed argument; the staged↔live element-wise pairing in `rotate` becomes one `swap_into` call instead of three hand-aligned renames.

**Instances:** `certs.rs:20-34` (accessors), `certs.rs:90-95`, `certs.rs:101-117`, `certs.rs:128`, `certs.rs:204-205`, `certs.rs:225`, `certs.rs:264-266`, `certs.rs:284-286`.

---

## Finding 2 — `security` CLI invocations are five hand-rolled `Command::new("security")` blocks with duplicated spawn/status/log scaffolding

**Smell:** Duplicate Code (Dispensables). Every macOS Keychain operation re-implements the same shape: `Command::new("security").args([...]).arg(login_keychain()).output()` then branch on `output.status.success()` and log. The keychain-binary name, the `login_keychain()` argument, and the success/stderr-logging skeleton are copy-pasted across five functions.

**Maintenance impact:** local. Blast radius: one file, the `#[cfg(target_os = "macos")]` trust-management block (~5 functions). Change frequency: low-moderate, but it *is* the trust surface that gets revisited whenever macOS changes `security` semantics or a new keychain op is needed.

**Concrete evidence — the duplicated `security` spawn pattern:**
- `certs.rs:316-328` `add_trusted_cert` — `Command::new("security").args(["add-trusted-cert", ...]).arg(login_keychain()).arg(cert_path).output()`, then `if output.status.success() { info } else { error+Err }`.
- `certs.rs:333-341` `verify_cert_trusted` — `Command::new("security").args(["verify-cert", "-c"]).arg(cert_path).output().map(|o| o.status.success())`.
- `certs.rs:347-357` `ca_keychain_sha1` — `Command::new("security").args(["find-certificate", ...]).arg(login_keychain()).output()`.
- `certs.rs:362-372` `remove_trusted_cert_by_sha1` — `Command::new("security").args(["delete-certificate", "-Z", sha1]).arg(login_keychain()).output()`, then the same `Ok(success)/Ok(fail-warn)/Err(warn)` triad.
The literal `"security"`, the `login_keychain()` trailing arg (4 of 5 sites), and the success-or-log-stderr block are duplicated verbatim. `crash_recovery.rs` has the **isomorphic** pattern for `systemctl` (`crash_recovery.rs:179-197`, `204-209`) and `schtasks` (`crash_recovery.rs:300-315`, `327-344`) — same "spawn external tool, branch on `status.success()`, log stderr" boilerplate — but those live in a different module/binary surface and share no obvious helper, so I scope the *certified* duplication to `certs.rs`'s five `security` sites (the tightest, same-file cluster) and note the cross-module echo as related.

**Why it harms future change:** the success/stderr-logging contract is re-decided per call site, so they already drift — `add_trusted_cert` returns `Result` and logs at `error`, `remove_trusted_cert_by_sha1` returns `()` and logs at `warn`, `verify_cert_trusted` swallows everything to `bool`. Adding a uniform behavior (e.g. capture+redact stderr, add a timeout, or route through `sudo`/an entitlement) means editing all five spots and re-deciding the log level each time. A typo'd subcommand or a missing `login_keychain()` arg is invisible because nothing centralizes "how we call `security`."

**Smallest safe refactoring:** Extract Method — a `fn run_security(args: &[&str]) -> std::io::Result<std::process::Output>` (or a tiny `security_cmd()` returning a pre-seeded `Command` with the binary + `login_keychain()` already attached) that the five callers funnel through. Keep each caller's bespoke success interpretation; only the spawn + (optional) keychain-arg + the stderr-logging triad get centralized.

**What disappears:** four duplicate `Command::new("security")` literals, four duplicate `.arg(login_keychain())` lines, and the repeated `match … status.success() … log stderr` skeleton collapse to one helper; the log-level/return-shape drift becomes a deliberate per-caller choice over a shared base instead of accidental copy-paste variance.

**Instances:** `certs.rs:316-328`, `certs.rs:333-341`, `certs.rs:347-357`, `certs.rs:362-372`. Related (not certified, cross-module): `crash_recovery.rs:179-197`, `204-209`, `300-315`, `327-344`.

---

## Finding 3 — `rotate()` carries a Temporal-Coupling staged→swap protocol with no enforcing structure, hand-aligning three staged paths to three live paths

**Smell:** Temporal Coupling (named analog of Fowler's couplers / "call sequence that must not be reordered"). `rotate()` encodes a strict ordering — *write staged → capture old SHA-1 → trust+verify new → atomically swap all three → remove old anchor* — but the contract is enforced only by the linear arrangement of statements and three independent `rename` calls that must stay element-wise aligned (`ca.pem.new`→`ca_cert_path()`, `localhost.pem.new`→`leaf_cert_path()`, `localhost.key.new`→`leaf_key_path()`). Nothing prevents a future editor from reordering the swap before the verify, or mis-pairing a staged path with the wrong live path. The mint→persist→serve / stage→swap ordering called out in the cluster brief is exactly this.

**Maintenance impact:** structural-leaning-local. Blast radius: the single `rotate()` function (`certs.rs:261-296`) plus the `*.new` literals it shares with the accessors (overlaps Finding 1). Change frequency: this function was *already* redesigned once (the fail-closed staged-rotation note at `certs.rs:254-260` documents a prior, different sequencing) — i.e. it is a proven change magnet, which is precisely where unguarded temporal coupling bites.

**Concrete evidence:**
- `certs.rs:268` writes the staged triple; `certs.rs:271-272` captures `old_sha1` *before* keychain mutation (order-sensitive: capturing after `add_trusted_cert` could grab the new anchor's hash); `certs.rs:275-281` trust+verify-or-abort; `certs.rs:284-286` three separate `rename`s that must stay element-aligned; `certs.rs:289-292` remove-old-by-SHA-1 *after* the swap. Five ordered phases, zero structural guard rails — purely statement order.
- The three staged literals (`certs.rs:264-266`) and the three live accessors (`certs.rs:284-286`) are paired by *position only*; the basenames duplicate the accessor basenames (the Finding-1 overlap), so the pairing correctness is implicit, not typed.
- A partial-failure window exists between the three renames (CA renamed, leaf not yet) — not a *correctness* certification here, but it is the structural symptom: three operations that morally form one atomic step are three statements.

**Why it harms future change:** anyone adding a step (e.g. "also re-trust on Linux," "stage a chain file") has to slot it into the exact right position and remember the SHA-1-before-mutation invariant, the verify-before-swap invariant, and the remove-after-swap invariant — all undocumented except as comment prose. Reordering or adding a fourth artifact silently breaks the atomicity intent. The element-wise rename triple makes a wrong-pairing edit a `&Path`-typed no-op the compiler accepts.

**Smallest safe refactoring:** Extract Method to name each phase (`stage_new_set`, `promote_staged` / `swap_into`, `reinstall_trust`) and—building on Finding 1's `CertPaths`—give the swap a single `staged.swap_into(&live)?` that performs the three renames internally with the pairing baked in. This converts the implicit ordering into named, individually-meaningful steps and removes the hand-aligned triple.

**What disappears:** the three position-paired `rename` calls collapse into one `swap_into`; the staged↔live basename duplication (shared with Finding 1) is eliminated; the five-phase protocol becomes five named calls whose ordering intent is legible instead of inferred from raw statement sequence.

**Instances:** `certs.rs:261-296` (whole function), specifically the ordered phases at `:268`, `:271-272`, `:275-281`, `:284-286`, `:289-292`.

---

## Finding 4 — Duplicated section/doc markers as comment-deodorant (`// ── macOS trust management ──` twice; doubled doc-comment on `leaf_cert_days_remaining`)

**Smell:** Comments-as-deodorant / Duplicate Code (Dispensables), minor. A literally-duplicated banner comment and a doc-comment that contradicts itself by stitching two stale descriptions together.

**Maintenance impact:** cosmetic. Blast radius: two spots in `certs.rs`. Change frequency: trivial, but it's an active *misleading-doc* hazard (worse than no comment).

**Concrete evidence:**
- `certs.rs:298` and `certs.rs:300` — the identical banner `// ── macOS trust management ──` appears twice in a row (an editing artifact). Pure noise; rustfmt won't touch it.
- `certs.rs:219-223` — `leaf_cert_days_remaining`'s doc-comment is two pasted-together descriptions: it opens *"Uses file modification time as a proxy for creation date."* and then immediately contradicts itself *"Uses the actual X.509 certificate, not file mtime …"*. The first sentence is stale (the function parses the X.509 `notAfter`, `certs.rs:226-228`, and never touches mtime). A reader trusting line 220 would believe the expiry check is mtime-based — actively wrong.

**Why it harms future change:** the self-contradicting doc forces a future maintainer to read the body to learn which half is true, defeating the doc's purpose; worse, someone might "fix a bug" against the phantom mtime behavior. The duplicate banner is harmless but signals copy-paste editing that erodes trust in the comments.

**Smallest safe refactoring:** delete the duplicate banner (`certs.rs:298` or `:300`); delete the stale first sentence of the doc-comment (`certs.rs:220`) so only the accurate X.509 description remains. (Pure deletion — no behavior touched.)

**What disappears:** one redundant banner line and one false sentence; the doc-comment stops contradicting the code.

**Instances:** `certs.rs:219-223`, `certs.rs:298-300`.

---

## Non-findings (examined, deliberately NOT certified)

- **Primitive Obsession — origins-as-`String`, paths-as-`PathBuf`.** `verified_sites.rs` keys its map on `String` canonical origins (`:55,111,123`) and `canonicalize_origin` returns `Option<String>` (core `authorization.rs:21`). There is **no `Origin` newtype anywhere in the crate** — the entire authorization layer (`is_auto_approved(origin: &str)`, `is_approved(origin: &str, …)`, `request`, `resolve`) uses bare `&str`/`String`. This is a *consistently applied codebase-wide convention*, which the DO-NOT-FLAG list excludes unless it causes measurable duplication/coupling. The only lookup logic (`canonicalize` then `HashMap::get`) is centralized in the single `lookup` method (`:121-124`) and reused by `try_load` — no duplication. Introducing an `Origin` newtype is a cross-cutting change to a *different* cluster (the `authorization` crate) and would be Speculative Generality to bolt on from here. NON-FINDING.
- **`verified_sites` as a Data Class.** `VerifiedSite` (`:23-34`) is a dumb DTO, but the brief explicitly says that's fine *unless lookup logic is duplicated* — it isn't (single `lookup`). The `VerifiedSitesFile`/`VerifiedSitesEntry`/`VerifiedSite` three-struct shape mirrors the on-disk-vs-runtime split and the field copy in `try_load` (`:94-99`) is a deliberate projection (drops `origins`, keeps curator fields), not accidental duplication. NON-FINDING.
- **`CrashRecovery` trait + `PlatformRecovery` ZST dispatch (`crash_recovery.rs:16-44`).** Could read as a Lazy Class / Middle Man (a one-impl trait wrapping `enable_impl`/`disable_impl`). But the module doc justifies it as the mock seam for tests, and the `disable() -> bool` contract carries real Windows-specific semantics. It is a single, coherent abstraction with a stated test purpose — not certified.
- **Per-platform `enable_impl`/`disable_impl` divergence.** These are genuinely different OS mechanisms (launchd plist / systemd unit / Task Scheduler XML), correctly isolated behind `#[cfg]`. This is appropriate platform variance, not Divergent Change. NON-FINDING.
