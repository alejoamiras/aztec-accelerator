# Plan — quality-fixes-2026-06-08 (blueprint deep)

Implement all 9 findings from the `/harden quality ultra` audit (`audit/quality/2026-06-08-ultra-e094d8/`) as behavior-preserving refactors **except F-02** (closes the headless origin-canonicalization gap), with tests that validate each change and add coverage at the new seams.

Consolidated from 3 independent plans (main + codex + opus — see `_planner-{main,codex,opus}.md`). **Tier: deep** (blast radius HIGH — server-state core, origin-auth trust boundary, release-critical `versions.rs`/`certs`, published SDK; security-sensitivity MED-HIGH).

## Fixed decisions (from clarifying answers)
1. **F-02 closes the gap, with tests.** `CanonicalOrigin` newtype makes the canonical-origin invariant un-bypassable; the headless `ALLOWED_ORIGINS` ingress is routed through it. Other 8 findings stay behavior-preserving.
2. **No blanket characterization tests** (user steer). Regression net = existing ~90 Rust + ~96 TS unit tests + 9 WebDriver E2E + Rust compiler. New tests only for new seams/behavior. (One exception adopted below: F-08's uncovered download arm — a single targeted test, justified by "make very sure they are valid.")
3. **4 package-coherent themed PRs:** PR-1 F-01+F-02 (`accelerator.yml`), PR-2 F-03+F-04 (`accelerator.yml`), PR-3 F-07+F-08+F-09 (`accelerator.yml`), PR-4 F-05+F-06 (`sdk.yml`).
4. **`/harden security` scheduled post-implementation** (F-02 trust boundary).

## Hard constraints
- **SDK public API: no breaking changes.** F-06 internal-only; F-05 additive (export `AcceleratorProtocol`). `tsc --noEmit` is the gate.
- `main` branch-protected → branch + PR + auto-merge after green CI. Each PR keeps the FULL suite + lint + WebDriver E2E green.
- Don't change runtime behavior of release-critical paths (`versions.rs`, `certs.rs`, server startup) — F-03/F-04/F-07/F-09 are pure moves.

---

## Per-finding approach (consolidated)

### F-02 ✓ — `CanonicalOrigin` newtype (PR-1, do FIRST — it changes the type F-01's ctor references)
New type in `core/src/authorization.rs`, mirroring the proven `AztecVersion` shape:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CanonicalOrigin(String);
impl CanonicalOrigin { pub fn parse(input: &str) -> Option<Self> { canonicalize_origin(input).map(Self) }
                       pub fn as_str(&self) -> &str { &self.0 } }
impl TryFrom<String> for CanonicalOrigin { type Error = NonCanonicalOrigin; /* canonicalize or err */ }
impl Deref<Target=str> / AsRef<str> / Display for CanonicalOrigin
```
- Keep `canonicalize_origin` as the `pub(crate)` engine (its **20+ tests stay green** — they pin the algorithm). Collapse the duplicated parse block (`authorization.rs:34-44` vs `131-135`) into it (also fixes the `ws`/`wss` vs `http`/`https` divergence).
- **Config field:** `approved_origins: Vec<CanonicalOrigin>` with **field-level** `#[serde(default, deserialize_with = "de_approved_origins")]` (NOT type-level `try_from` on the Vec) — `de_approved_origins` deserializes `Vec<String>`, canonicalizes per-entry, **drops+`warn`s** invalid, dedupes order-preserving. This **replaces** `migrate_approved_origins` (delete it + its resave block at `config.rs:96-130`) — lossless for existing configs (persisted entries are already canonical → idempotent).
- **Type the pending map too** (codex): `AuthorizationManager.pending: HashMap<CanonicalOrigin, …>`. Auth signatures take `&CanonicalOrigin`. `authorize_origin()` parses the `Origin` header; `respond_auth()`/`remove_approved_origin()` re-parse the Tauri string arg (no command signature change → no frontend break).
- **Headless ingress (closes the gap):** extract `fn parse_allowed_origins_env(raw: &str) -> Result<Vec<CanonicalOrigin>, String>` (pure, testable). **Policy: fail-fast** — any invalid entry returns `Err` and the server exits non-zero (operator security input; silent drop of an intended origin is dangerous). `server/src/main.rs:43-57` uses it.

### F-01 ✓ — state constructors + selective non-`Option` (PR-1, after F-02)
```rust
// core/src/server.rs — prove_semaphore + app_version become required; config/auth_manager/bundled_version stay Option
impl HeadlessState {
  pub fn headless(app_version: impl Into<String>, bundled_version: Option<String>,
                  config: Option<Arc<RwLock<AcceleratorConfig>>>, auth_manager: Option<Arc<AuthorizationManager>>) -> Self
}
impl AppState { pub fn headless(core: HeadlessState) -> Self;
                pub fn desktop(core: HeadlessState, on_status, on_versions_changed, show_auth_popup) -> Self } // FLAT — no wrapper struct (audit-reverted)
```
- **`prove_semaphore: Arc<Semaphore>`** + **`app_version: String`** non-`Option`. **Hand-write `Default for HeadlessState`** (can't derive) filling `prove_semaphore: Arc::new(Semaphore::new(1))` + `app_version: env!("CARGO_PKG_VERSION")` so test sites using `AppState::default()` still compile. `prove.rs:143` semaphore acquire becomes unconditional (behavior change **tests-only**; Default supplies the permit). `health()` reads `&state.app_version` directly.
- `config`/`auth_manager`/`bundled_version` **stay `Option`** (headless `None` mode verified at `server/src/main.rs:58`).
- **Keep `AppState` callbacks FLAT (audit-reverted from `DesktopCallbacks`)** — the 3 callbacks are read in **core** (`prove.rs:160`, `auth.rs:59/83`); a "Desktop"-named struct field-accessed in GUI-agnostic core violates the core-extraction boundary and buys nothing (`AppState::desktop` takes 3 args fine). Both auditors → flat. The "callback data clump" sub-concern is dropped (not worth the boundary cost).

### F-03 ✓ — decompose `.setup` (PR-2; pure move; uses PR-1's `AppState::desktop`)
Extract from `main.rs:260-462`: `build_tray_and_status`, `build_desktop_state -> AppState` (wires the 3 callbacks inline via `AppState::desktop(core, on_status, on_versions_changed, show_auth_popup)` — **no `DesktopCallbacks` wrapper; flat only**), `run_startup_diagnostics`, `spawn_http_server` (the Windows `#[cfg]` `AddrInUse` block moves **verbatim** here), `spawn_update_poller`. `.setup` becomes linear orchestration. The callback *builders* stay **local closures** inside `build_desktop_state` (lifetime churn if hoisted). `#[cfg]` gates move with their code. **Required gate (audit-promoted):** `spawn_http_server` moves the Windows-only `AddrInUse` bow-out (covered only by `_e2e-crash-recovery-windows.yml`, off the normal PR gate) → that workflow MUST run + be green before PR-2 merges, since F-03's whole claim is "pure move".

### F-04 ✓ — `versions.rs` → façade + submodules (PR-2; pure move)
Keep `pub mod versions;` in `lib.rs` and the `src-tauri/lib.rs` re-export **unchanged**. Convert `versions.rs` → `versions/mod.rs` (re-exports preserving every `versions::X` path) + `{version_id,platform,artifact_layout,cache,downloader}.rs`. macOS `xattr`+`codesign` finalize tail extracts into `downloader::finalize_macos_binary` (stays in the downloader slice). Inline tests move to the submodule owning the unit. Verified consumers (`bb.rs`, `core/server.rs`, `prove.rs`, `tray.rs`) need **no edits**.

### F-05 ✓ — SDK barrel canonical + doc-sync test (PR-4; additive)
Export `AcceleratorProtocol` from `index.ts`; replace README's obsolete flat `interface AcceleratorStatus` with the union; add `setForceLocal` to the README method table; add the `denied` phase to the SKILL phase table (align all 5 surfaces). **Doc-sync test** `src/lib/public-contract.test.ts`: (a) type-imports `AcceleratorProtocol` from the barrel (compile-fail if dropped) + asserts the exact export-name set; (b) reads README/MIGRATION/SKILL and asserts required markers present + the obsolete `interface AcceleratorStatus {` snippet absent.

### F-06 — extract `AcceleratorTransport` (PR-4; internal; same public surface)
New non-exported `AcceleratorTransport` in `src/lib/accelerator-transport.ts` owning URL construction, the dual http/https probe + protocol negotiation, the status cache, and one error model. **Keep `ky` for BOTH** health and prove (health uses `throwHttpErrors: false`) — preserves the thrown-error surface (the main risk; the team is break-sensitive). The **parse → `AcceleratorStatus`** discriminated-union construction stays in the prover (domain). Route every `#acceleratorProtocol` mutation through `transport.setProtocol` (the "doesn't cache protocol on non-ok" + "detected protocol used for subsequent /prove" tests pin exactly-when).

### F-07 ✓ — `CertPaths` parameter object (PR-3; pure)
```rust
struct CertPaths { ca_cert, leaf_cert, leaf_key: PathBuf }
impl CertPaths { fn live()->Self; fn staged(dir:&Path)->Self; fn exists(&self)->bool; fn swap_into(&self, live:&CertPaths)->io::Result<()> }
```
`write_new_cert_set(&CertPaths)`, `load_rustls_config_from(&CertPaths)`; public no-arg wrappers delegate to `CertPaths::live()`. **Keep `ca_key_path` standalone** (legacy-migration target, not part of the served triple). `certs_exist` keeps its **leaf-validity** check (not just `.exists()`). `swap_into` preserves rename order **ca→leaf→key**.

### F-08 ✓ — `/prove` status ownership (PR-3; behavior-preserving)
`resolve_version` → `pub(crate) fn resolve_version(state, requested) -> Result<ResolvedVersion>` where `ResolvedVersion { version: Option<…>, needs_download: bool }` (no callbacks). `prove()` owns the sequence: emit `Proving` **before** the `needs_download` check → if `needs_download` { `Downloading`; `download_bb`; spawn cleanup; `Proving` } → `bb::prove` → `StatusGuard` drops to `Idle`. Update the 3 `resolve_version_*` tests (`server.rs:1046-1074`). **Preserve the redundant leading `Proving`** (today's download arm is `[Proving, Downloading, Proving, Idle]`) — do NOT "clean it up" (that'd be a tests-only behavior change on an uncovered path). `server.rs:626` pins only the no-download arm → the new download-arm test asserts the full **4-element** sequence.

### F-09 ✓ — shared `spawn_https()` (PR-3; pure)
`pub fn spawn_https(state: AppState, tls: Arc<rustls::ServerConfig>)` in `src-tauri/src/server.rs` (spawn + error-log only). `main.rs::try_start_https` + `commands.rs::enable_safari_support` keep their distinct preambles and both call it. Do NOT unify the divergent TLS-load-failure policies (main resets Safari support; commands propagates the error — intentional).

---

## PR structure, ordering, dependencies
| PR | Findings | Gate | Order + dep |
|----|----------|------|-------------|
| PR-1 | F-02 → F-01 | accelerator | F-02 first (changes `approved_origins` element type the ctor references). Foundation for PR-2/PR-3. |
| PR-2 | F-04 → F-03 | accelerator | After PR-1 (F-03 calls `HeadlessState::headless`). F-04 (mechanical) first, then F-03. |
| PR-3 | F-07 → F-09 → F-08 | accelerator | After PR-1 (types). F-08 last (most behavior-sensitive). |
| PR-4 | F-05 → F-06 | sdk | Independent of Rust PRs. F-05 (docs+guard) first as the safety net, then F-06 under it. |
Sequence: **PR-1 → PR-2**; PR-3 after PR-1; PR-4 anytime. Each finding = its own commit (clean per-finding rollback).

## Test plan (no blanket characterization; new tests at new seams only)
| F | Existing regression net | New tests |
|---|---|---|
| F-01 | server router/auth/prove tests + WebDriver E2E + compiler | ctor field-population unit tests |
| F-02 | 20+ `canonicalize_*` tests + auth tests + WebDriver auth flow + compiler | `CanonicalOrigin` serde round-trip (lossless on canonical); `de_approved_origins` drop/dedupe (ports the 5 deleted migrate tests onto the serde seam); `parse_allowed_origins_env` (valid/invalid/fail-fast); **IDN/punycode** + the security matrix vectors; headless-approval behavior pin |
| F-03 | WebDriver E2E (real `.setup`) + compiler | none |
| F-04 | existing `versions` suite (moves) + release-smoke + compiler | none |
| F-05 | — | the doc-sync `public-contract.test.ts` |
| F-06 | 28 SDK unit tests (mock `fetch`; `ky` rides on it → covers both) + `tsc --noEmit` | `AcceleratorTransport` baseUrl/protocol/error unit tests |
| F-07 | cert tests (update `generation_writes_no_ca_key` to `CertPaths` arg) | `CertPaths` live/staged/exists/swap |
| F-08 | `server.rs:626` (no-download arm) | **one targeted test for the download arm** (`Proving→Downloading→Proving→Idle`) — adopted per "make very sure they are valid"; the only uncovered behavior-sensitive reorder |
| F-09 | WebDriver E2E + Playwright settings + compiler | none |

**Validity argument (global):** the Rust type system makes every `CanonicalOrigin` / non-`Option` / `CertPaths` threading a hard compile error if a site is missed; the existing E2E exercises real startup/auth/HTTPS; `tsc --noEmit` gates the SDK public surface; new unit tests cover each new seam + the one behavior change (F-02) + the one uncovered reorder (F-08).

---

## Security & Adversarial Considerations
**Threat:** an attacker-controlled web origin smuggling a near-miss string past exact-match approval. `CanonicalOrigin::parse` delegates to the existing audited `canonicalize_origin` (`url::Url` + explicit rules). Per-vector (all with existing or new tests):

| Vector | Handling |
|---|---|
| Case `HTTPS://X.COM` | scheme+host lowercased |
| Trailing dot `x.com.` | stripped; empty-after-strip rejected |
| Default port `:443`/`:80` | elided; non-default preserved (distinct origin) |
| Scheme (`file:`/`data:`/`javascript:`/prefix-lookalike) | exact-match allow-list; rejected |
| Userinfo `user:pass@host` | **rejected** (not stripped) |
| Path/query/fragment | **rejected** (not ignored) |
| Whitespace/`\0`/control | env trims surrounding ws; embedded control → URL parse fail → rejected |
| **IDN/punycode** (Cyrillic homograph) | `url::Url` IDNA→punycode `xn--` → distinct from ASCII, no collision. **NOT unit-tested in-repo → new test locks it.** |

**Accurate framing (audit-corrected — this was overstated in an earlier draft):** today an *already-canonical* `ALLOWED_ORIGINS` value **does** match (server stores raw → `auth.rs:35` canonicalizes the request `Origin` → `is_approved` does exact-string-eq). The real gap is that a *non-canonical* env value **silently fails to match**. Closing it via fail-fast is therefore a **real operator-visible change** (startup error on an invalid non-empty entry), not a no-risk hardening — surfaced as an Ask. The no-`Origin` auto-approve boundary (`auth.rs:32`, documented single-tenant-CI assumption) is **unchanged**. **Do NOT widen** the localhost auto-approve scheme set — it stays `http|https` only (`authorization.rs:131`); the duplicate-parse collapse touches `url::Url` parse *mechanics* only, and `ws://localhost` (not auto-approved) is pinned by a test. **Supply chain:** no new deps (`url`, `serde`, `ky` already present). **Least privilege / SDK:** F-06 internal-only; no public surface or capability change. **Post-impl `/harden security`** validates the trust boundary end-to-end.

---

## Assumptions
**Facts (verified):** prod state construction = exactly 2 sites (`server/main.rs:62-75`, `src-tauri/main.rs:345-367`); `canonicalize_origin` exists + 20-test-pinned + idempotent; `versions::` consumers (`bb.rs`/`core/server.rs`/`prove.rs`/`tray.rs`) all via `versions::` path → re-export keeps stable; `ky` rides on `globalThis.fetch` → existing mock covers both stacks; **`ALLOWED_ORIGINS` is NOT set in any CI workflow/e2e harness** (grep-verified — only docs); `config` must stay `Option` (headless `None` mode); F-08 no-download arm pinned at `server.rs:626`, download arm uncovered; `start_https` GUI-only, 2 callers.
**Inferences (to lock with tests):** `url::Url` IDNA-normalizes to punycode (new test); field-level `deserialize_with` preserves today's load-tolerance; making `prove_semaphore`/`app_version` non-`Option` needs a manual `Default`; F-06 on `ky` w/ `throwHttpErrors:false` preserves the `/health` error surface.
**Asks (surface at gate — all have a recommendation, none silent):**
1. **F-02 resave removal** — deleting `migrate_approved_origins` also stops the one-time on-disk *resave* at load. Configs still deserialize losslessly. *Rec: safe (config is app-private; nothing external reads it) — confirm.*
2. **PR-2 Windows arm** — F-03 moves the Windows-only `AddrInUse` bow-out (validated only by `_e2e-crash-recovery-windows.yml`, off the PR gate). *Rec: run that E2E before merging PR-2.*
3. **F-01 callbacks → FLAT `AppState`** — RESOLVED post-audit (no `DesktopCallbacks` wrapper anywhere; both auditors → flat). No longer an open ask.

---

## Decision ledger (3-plan consolidation)
**Converged (all/most agreed → high confidence):** field-level `deserialize_with` for `approved_origins` (codex+opus; codex initially floated type-level `try_from` then self-corrected to the helper); manual `Default for HeadlessState`; F-02-first ordering in PR-1; `config` stays `Option`; F-04 façade + re-exports; F-09 `spawn_https` in `server.rs`; F-06 keep `ky` both + preserve error surface; F-08 split with `prove()` owning status; the F-08 download-arm test as the one justified new test.
**Resolved disputes:**
- *e2e `ALLOWED_ORIGINS` fact* (opus said set+canonical; codex said not set): **grep-verified codex right** — not set in CI → headless path untested → add `parse_allowed_origins_env` unit test; closing the gap is low-blast-radius.
- *Invalid env-origin policy* (main/opus warn-drop vs codex fail-fast): **synthesized** — env = fail-fast (operator security input), persisted config = lenient drop+warn (don't brick existing data). Two ingress points, two policies.
- *F-01 callbacks* (codex `DesktopCallbacks` vs opus flat): **chose FLAT `AppState`** — the double audit reverted `DesktopCallbacks` (a "Desktop"-named type read in GUI-agnostic core violates the core-extraction boundary; both auditors → flat). **No wrapper struct anywhere** (incl. §F-03's `build_desktop_state`).
**Adopted-from-codex extras:** type `AuthorizationManager.pending` keys as `CanonicalOrigin`; `parse_allowed_origins_env` pure seam; `throwHttpErrors:false` health probe; `denied` phase in the SKILL table.
**Rejected:** type-level `#[serde(try_from)]` on the `Vec` (one bad entry bricks the load — codex+opus); hoisting F-03 callback builders to free fns (lifetime churn, no gain — opus); unifying F-09's divergent TLS-failure policies (intentional divergence — codex).

---

## Audit revisions (double audit folded — codex + opus; both: fundamentally sound)

**Behavior-correctness (must-not-regress):**
- **[opus H2] F-08 download arm.** Preserve the redundant leading `Proving` → `[Proving, Downloading, Proving, Idle]`; the new download-arm test asserts all 4 (the `server.rs:626` pin only covers the no-download arm). *(Folded inline in §F-08.)*
- **[codex High] Security framing corrected.** Headless auth is NOT globally fail-closed — canonical env values already match; the gap is *non-canonical values silently fail*, and fail-fast is an operator-visible change. *(Folded inline in §Security + Assumptions.)*
- **[codex Med3] No scheme widening.** The duplicate-parse collapse is parse-mechanics only; localhost auto-approve stays `http|https` (`authorization.rs:131`). Pin `ws://localhost` (not auto-approved) with a test. *(Folded inline in §Security.)*

**F-02 completeness:**
- **[opus H1] Enumerate the ripple:** `is_approved(origin: &CanonicalOrigin, approved: &[CanonicalOrigin])`; `auth.rs:114-115` push/contains build a `CanonicalOrigin`; `remove_approved_origin` re-parses the Tauri `String`. Add `PartialEq<str>` + `Borrow<str>` to `CanonicalOrigin` so `contains`/`retain`/compare-vs-`&str` stay ergonomic.
- **[codex Med1] `CanonicalOrigin` derives `Deserialize` too** (strict, via `try_from`) so the round-trip test holds; lenient `de_approved_origins` stays only on the config Vec field.
- **[codex Med2] `parse_allowed_origins_env` exact pipeline:** `trim → drop empty segments → canonicalize non-empty → dedupe (order-preserving) → fail-fast ONLY on an invalid non-empty entry`. Preserves today's tolerance of `""`/trailing-comma/whitespace-only. Tests: empty var, trailing comma, duplicate, whitespace-only, invalid-non-empty.
- **[final-codex — SECURITY-CRITICAL] Preserve PRESENCE semantics.** The env var being *present* enables gating even when it trims to `[]`: `ALLOWED_ORIGINS=""`/whitespace-only/trailing-comma still instantiates `Some(auth_manager)` + `Some(config)` with an **empty approved list (= deny ALL browser origins)**, exactly as today (`server/main.rs:43` keys on `Ok(var)` presence, not on the parsed list being non-empty). Do **NOT** map empty parse output to `(None, None)` — that would silently flip to "auth disabled → auto-approve everyone". Headless regression test pins: present-but-empty ⇒ browser caller denied.
- **[opus M3] Resave-removal evidence test:** assert a non-canonical on-disk entry round-trips to a canonical in-memory `CanonicalOrigin` (proves the deleted resave is unnecessary → closes Ask 1).
- **`respond_auth` malformed-origin = log-and-deny (DECIDED + folded, not a floating Ask):** if the pending-key re-parse fails at `commands.rs:105-121`, log + deny (do not hang to the popup timeout). (final-codex: no exploitable bypass here — the popup round-trips the server's canonical string — but specify it so the implementer doesn't infer it at merge.)
- **Security matrix extended** (new vectors, locked by tests): `Origin: null`, `blob:`, percent-encoded host, mixed-case punycode, IPv6 zone-id, port `0`/overflow.

**Smaller:**
- **[opus M4 + codex] F-01 `DesktopCallbacks` → REVERTED to flat `AppState`** (3 callback params; no wrapper). *(Folded inline in §F-01; resolves Ask 3.)*
- **[opus M1] F-09 citations corrected:** `start_https` is in `tls.rs:15` (re-exported `server.rs:9`); the dup is the spawn+error-log wrapper (`main.rs:85-89` ≡ `commands.rs:160-164`). `spawn_https` extracts that; divergent TLS-load-failure policy stays upstream (auto-satisfied).
- **[opus L1] Explicit-literal test edits:** `prove_success_path_and_status_sequence` (`server.rs:641-650`) sets `prove_semaphore: Some(...)` → drop the `Some` after the non-`Option` change (compiler-caught).
- **[final-codex — REQUIRED gate] PR-4 SDK build check.** `sdk.yml` typechecks source but does NOT build `packages/sdk`; the publish workflow (`_publish-sdk.yml`) rewrites `package.json` to ship `dist/index.js` + `dist/index.d.ts`. Source-only `tsc --noEmit` can't prove the npm artifact is intact, and F-05/F-06 touch the barrel/public surface. So PR-4 **MUST** run `bun run --cwd packages/sdk build` (or assert the built `dist/index.d.ts` barrel surface) as a required gate — not a low-priority note.

**Updated open Asks (post final-codex) — only ONE remains, low-risk:** **(1) F-02 resave removal** — backed by the new round-trip test; confirm nothing external reads `config.json` (it's app-private). Everything else is now DECIDED + folded: env present-but-empty ⇒ deny-all (security-critical, test added); `respond_auth` = log-and-deny; **Windows-arm E2E + PR-4 SDK `build` = REQUIRED gates** (promoted from recommendations); `DesktopCallbacks` = flat (Ask 3 resolved). The two "required gates" are merge conditions, not approval blockers.

**Ledger updates:** `DesktopCallbacks` REVERTED post-audit (core-extraction boundary); security "strictly safer" framing CORRECTED (operator-visible change); `ws`/`wss` collapse scoped to parse-mechanics (no policy widening); `CanonicalOrigin` gains `Deserialize` + `PartialEq<str>`/`Borrow<str>`.

---

## Seeds
**`/goal` (recommended — completion is transcript-observable):**
```
/goal All 4 PRs for quality-fixes-2026-06-08 opened + merged-green (or surfaced if a merge gate stalls); every finding F-01..F-09 marked ✓ in plan.md with a LESSONS_FILE=implementations-plan/quality-fixes-2026-06-08/lessons/phase-N.md printed; `/code-review max --fix` applied+committed per PR; codex post-impl audit clean (or High/Critical addressed); `bun run test` + `bun run lint` + `cargo test`/`clippy` all exit 0 in the transcript; SDK `tsc --noEmit` green. Behavior-preserving everywhere except F-02. Never merge to main without green CI; never push to main directly.
```
**`/loop` (alternative):**
```
/loop Implement quality-fixes-2026-06-08 PR-by-PR (PR-1 F-02→F-01, PR-2 F-04→F-03, PR-3 F-07→F-09→F-08, PR-4 F-05→F-06). Each turn: read plan.md + lessons/ for status; next pending finding → implement (concrete shapes in plan.md) → cargo test/clippy or bun test + tsc → commit per finding → when a PR's findings done, push + gh pr create + watch CI. Failed check → triage, /codex xhigh if non-trivial, fix, commit (stop after 5 fails on one step). PR green → mark its findings ✓ in plan.md + file lessons/phase-N.md + print LESSONS_FILE=…; auto-merge; next PR. All 4 merged → /code-review max --fix → codex post-impl audit → address high/critical → then /harden security → stop + surface. Never push to main; never merge without green CI.
```
