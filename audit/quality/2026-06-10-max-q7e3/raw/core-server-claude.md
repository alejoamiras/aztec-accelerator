# Quality audit — core-server cluster (claude)

Date: 2026-06-10 · Run: max-q7e3 · Scope: maintainability only (no correctness/security)

Files read (all, in full):
- `packages/accelerator/core/src/server.rs` (1424 LOC: ~329 production, ~1095 inline tests)
- `packages/accelerator/core/src/server/prove.rs` (236 LOC, **zero** inline tests)
- `packages/accelerator/core/src/server/auth.rs` (141 LOC, **zero** inline tests)
- `packages/accelerator/core/src/server/host.rs` (135 LOC, self-tested)
- `packages/accelerator/core/src/server/bind.rs` (124 LOC, self-tested)
- `packages/accelerator/core/src/server/probe.rs` (74 LOC, self-tested)

Change-frequency ground truth (`git log --follow --since=2026-06-01`): server.rs **23 commits** (7 at the
current path + 16 at its pre-extraction path) — HOT. auth.rs 4, prove.rs 3, host/bind/probe 1 each.

Lead corrections (verified against source):
- `json_error()` is at server.rs:326 and has **11** call sites (5 prove.rs + 6 auth.rs), not ~17.
- The inline test module is **~1095 LOC** (lines 330–1424), not ~660.
- The status-sequencing temporal-coupling lead is **partially confirmed** — F-08 already consolidated the
  sequence into one function; the residue is real but smaller than the lead implies (Finding 6).

Context that shapes the findings: this code already went through a deliberate quality campaign
(Q1–Q15, F-01–F-09, the Q2 1/n–5/n extractions — visible in git history). The smells below are
overwhelmingly the *residue* of that campaign: extractions that moved code but left tests, error
plumbing, and shared literals behind in the parent.

---

## Finding 1 — Stringly-typed error channel: `(StatusCode, String)` + 11 hand-rolled `json_error` tuples

1. **Title**: `/prove`'s error type is an anonymous tuple of status code + pre-serialized JSON string,
   constructed by hand at every error site.
2. **Named smell**: **Primitive Obsession** (the error is a `(StatusCode, String)` where the `String` is
   already-serialized JSON) + **Data Clump** (the triple `status / error-id / message` always travels
   together) + **Duplicate Code** (the `(StatusCode::X, json_error("id", msg))` construction repeated 11x).
3. **Impact**: structural · blast radius 3 files (server.rs, prove.rs, auth.rs; host.rs diverges — see
   instance list) plus the SDK, which string-matches these error ids · **hot** (error sites were touched in
   SEC-01b/SEC-04/SEC-06 PRs #338–#340 this month).
4. **Instances**:
   - Definition: `packages/accelerator/core/src/server.rs:312` (`type ProveError = (StatusCode, String)`),
     `:319-323` (`ProveErrorBody`), `:326-328` (`json_error`) — defined in the parent, used **only** by the
     child modules (`prove.rs:16`, `auth.rs:13` import it back up via `super::`).
   - Tuple constructions: `server/auth.rs:41-48` (invalid_origin), `:67-74` (origin_denied, headless),
     `:78-86` (too_many_requests), `:96-103` (authorization_timeout), `:104-112` (authorization_cancelled),
     `:130-138` (origin_denied); `server/prove.rs:62-70` (invalid_version), `:120-129` (payload_too_large),
     `:133-138` (service_unavailable), `:180-189` (download_failed), `:221-226` (prove_failed).
   - Divergent third shape: `server/host.rs:68-72` builds its error a different way —
     `axum::Json(json!({"error": "invalid_host"}))` — `application/json`, no `message` field, while all
     prove/auth errors are `{error,message}` served as `text/plain`. Two error envelopes in one server;
     the test at `server.rs:1132-1139` has to string-sniff (`body.contains("invalid_host")`) because the
     shape is inconsistent.
5. **Why future change gets harder**: adding one field to the error envelope (e.g. `retry_after`, a docs
   URL) means editing 11 sites while keeping field order byte-identical (the Q8 wire contract — SDK `ky`
   keys on `text/plain`). The only thing preventing a new contributor from writing the natural
   `axum::Json(...)` for error #12 — silently flipping Content-Type and changing SDK runtime behavior — is
   a doc comment plus one characterization test. The error-id vocabulary the SDK matches on
   (`invalid_version`, `origin_denied`, …) exists only as 11 scattered string literals; nothing enumerates it.
6. **Smallest safe refactoring**: **Replace Error-Tuple-with-Type** (Fowler: Replace Primitive with
   Object): `struct ProveError { status: StatusCode, id: &'static str, message: String }` with an
   `impl IntoResponse` that reproduces the exact `(StatusCode, String)`/`text/plain` serialization, plus
   per-id constructors (`ProveError::origin_denied(origin)`). The existing characterization test
   (`prove_error_responses_stay_text_plain_json_string`, server.rs:669) makes this refactor safe to land.
   Put the type in a new `server/error.rs` (also fixes the children-import-parent plumbing). Folding
   host.rs's `invalid_host` into the same type is a *wire-visible* change (content-type flips) — do it as
   a separate, deliberately-flagged step or not at all.
7. **What disappears**: 11 hand-built tuples; the free-floating `json_error` fn; the unenforced "never use
   `axum::Json` for errors" convention (the type's `IntoResponse` becomes the single serialization point);
   the error vocabulary becomes a greppable, enumerable set of constructors.

---

## Finding 2 — server.rs is a six-concern module — Divergent Change

1. **Title**: The parent module mixes wire constants, tray presentation, state types, router wiring, the
   /health endpoint, and another module's error plumbing.
2. **Named smell**: **Divergent Change** (one file changes for five unrelated reasons), with **Large
   Class** as the module-level analog (Large Module).
3. **Impact**: structural · blast radius: server.rs + every consumer of `accelerator_core::server::*`
   (`packages/accelerator/server/src/main.rs`, `packages/accelerator/src-tauri/src/main.rs`,
   `src-tauri/src/server/tls.rs`, `src-tauri/src/server.rs` re-export shim) — though the refactor itself
   is re-export-preserving · **hot** (23 commits since 06-01).
4. **Instances** (the six concerns, all in `packages/accelerator/core/src/server.rs`):
   - `:30-43` wire constants (PORT, HTTPS_PORT, DEFAULT_BB_VERSION, AUTH_DECISION_TIMEOUT);
   - `:45-73` tray presentation (`ServerStatus::display_text`/`is_busy` — sole consumer is
     `src-tauri/src/main.rs:401,409`, the GUI tray);
   - `:85-184` state types + constructors (HeadlessState, AppState, Deref, Default, headless/desktop);
   - `:186-229` lifecycle + router wiring (start, router, router_for_port, CORS);
   - `:237-310` `/health` endpoint + its origin-tier policy (`health_is_detailed`, `health`);
   - `:312-328` `/prove` error plumbing (used only by prove.rs/auth.rs — see Finding 1).
   Git history is the proof of divergence: PR #299 touched it for a status enum, #308/#333 for state
   shape, #314/#339 for the health body, #296 for error shape, #338 for router middleware — five
   independent change axes landing in one file.
5. **Why future change gets harder**: every one of those axes is active (all five PRs above are recent).
   Two concurrent PRs — say a health-body change and a state-constructor change — conflict in the same
   file despite sharing nothing; reviewers must re-orient in a 1424-line file to review a 10-line change;
   `git log server.rs` no longer answers "what changed about the server wiring".
6. **Smallest safe refactoring**: **Extract Module** (continue the Q2 campaign to its end): `server/state.rs`
   (HeadlessState/AppState/callback aliases), `server/status.rs` (ServerStatus), `server/health.rs`
   (health + health_is_detailed), `server/error.rs` (Finding 1's type). `server.rs` keeps constants +
   router wiring + `pub use` re-exports, so `accelerator_core::server::*` paths stay stable (the
   src-tauri shim at `src-tauri/src/server.rs:6` already glob-re-exports). Pure moves; zero behavior change.
7. **What disappears**: cross-axis merge conflicts on the hot file; the `use super::{json_error, ProveError, …}`
   reach-ups from children; the navigation tax on every review touching this cluster.

---

## Finding 3 — ~715 LOC of prove/auth/host tests stranded in server.rs after the Q2 extraction — Shotgun Surgery

1. **Title**: The Q2 extraction moved `prove`/`auth` production code out of server.rs but left their tests
   behind; the two largest extracted modules have zero inline tests while the parent carries them.
2. **Named smell**: **Shotgun Surgery** (a single behavioral change to auth.rs or prove.rs forces edits in
   server.rs every time) — the operational residue of an incomplete Extract Module. Secondary:
   inconsistent convention (host.rs/bind.rs/probe.rs own their tests; prove.rs/auth.rs don't).
3. **Impact**: structural · blast radius 3 files (server.rs, prove.rs, auth.rs) · **hot** — auth.rs had 4
   behavioral commits since 06-01, every one of which had to edit server.rs's test module too.
4. **Instances** (all in `packages/accelerator/core/src/server.rs`; subject code in child modules):
   - `:332` — `use super::prove::{compute_threads, resolve_version};` (parent test module importing a
     child's `pub(crate)` internals);
   - prove-behavior tests: `:568-599` (bb-not-found), `:633-660` (invalid version header), `:662-756`
     (error content-type characterization), `:758-832` (success-path characterization), `:1372-1423`
     (empty/oversized body);
   - auth-behavior tests: `:955-1280` (auto-approve, popup, deny, no-origin, remembered, headless, 429,
     timeout — 8 tests, ~326 LOC) + the shared fixture `auth_state_with_popup` `:935-953`;
   - host-behavior test: `:1112-1144` (DNS-rebinding rejection — host.rs's keystone behavior, pinned in
     the parent while host.rs has its own test module);
   - pure unit tests of prove.rs helpers: `:1284-1370` (`compute_threads_*` x3, `resolve_version_*` x4).
   Net: ~715 of ~1095 test LOC exercise behavior implemented in child modules; `prove.rs` and `auth.rs`
   contain `#[cfg(test)]` **zero** times.
5. **Why future change gets harder**: every prove/auth change produces a two-file diff where one file is
   the 1424-line monolith; the file grows with every child-module feature forever; a contributor changing
   auth.rs has no signal that its safety net lives 800 lines into a different file; `git blame`/`log` on
   server.rs is dominated by churn that isn't about server.rs.
6. **Smallest safe refactoring**: **Move Function** (the tests) to their owning modules — the
   `resolve_version_*`/`compute_threads_*` unit tests are trivially movable today; the router-driven
   integration tests move with them (child modules can build the router via `super::super::router`).
   Extract `auth_state_with_popup` into a `#[cfg(test)] pub(crate) mod test_support` in server.rs. Keep
   only health/CORS/router-wiring tests in the parent. Zero production change.
7. **What disappears**: the hot-file churn coupling (auth change => server.rs edit); ~715 LOC off the
   monolith (1424 → ~700); the parent-imports-child-internals seam at `:332`.

---

## Finding 4 — Origin-approval check duplicated between /health tiering and /prove auth

1. **Title**: The "is this Origin approved?" mechanism (header extract → canonical parse → config lock →
   `is_approved`) is implemented twice, with intentionally different policy mapping but identical mechanism.
2. **Named smell**: **Duplicate Code** (the mechanism) + **Data Clump** (`&cfg.approved_origins` +
   `cfg.auto_approve_localhost` always ripped out of the same read-locked config and passed as a pair) +
   **Feature Envy** (both call sites interrogate `AcceleratorConfig`'s fields instead of asking it for a
   decision).
3. **Impact**: structural · blast radius 3 files (server.rs, server/auth.rs, authorization.rs) · **hot**
   (this exact logic was modified by SEC-04/SEC-05 in PR #339 this month — in both places at once).
4. **Instances**:
   - Header-extract chain: `packages/accelerator/core/src/server.rs:238-242` ≡ `server/auth.rs:25-35`
     (same `headers.get(ORIGIN).and_then(|v| v.to_str().ok())` dance, different absent-policy);
   - Canonical parse step: `server.rs:244-246` ≡ `server/auth.rs:37-49` (same parse, different
     malformed-policy);
   - Lock + clump call: `server.rs:247-259` ≡ `server/auth.rs:51-58` — byte-similar
     `cfg.read()` → `AuthorizationManager::is_approved(&origin, &cfg.approved_origins, cfg.auto_approve_localhost)`;
   - The clumped signature itself: `authorization.rs:269-277` (`is_approved(origin, approved_origins,
     auto_approve_localhost)` — its only two production callers both deconstruct the same config).
5. **Why future change gets harder**: any change to approval semantics (per-origin expiry, wildcard
   subdomains, a new auto-approve class) must be applied to two parallel blocks in two files; if one is
   updated and the other isn't, `/health`'s detail tier and `/prove`'s gate silently disagree about what
   "approved" means — a drift that no compiler error and no single test catches.
6. **Smallest safe refactoring**: **Move Method** — `AcceleratorConfig::is_origin_approved(&self,
   &CanonicalOrigin) -> bool` (or `is_approved(&origin, &cfg)`), so the lock-holder passes the whole
   config and the field pair stops travelling. Optionally a step further: **Extract Method**
   `origin_disposition(state, headers) -> {Absent | Malformed | Approved | Unapproved(origin)}` consumed
   by both `health_is_detailed` and `authorize_origin`, leaving each with only its policy match.
7. **What disappears**: the duplicated lock-dance and 3-arg clump; two definitions of "approved" collapse
   to one; the header-extraction chain stops being copy-paste between the two gates.

---

## Finding 5 — `prove()` is a 130-line, 7-phase handler with download orchestration inlined

1. **Title**: The `/prove` handler runs authorize → buffer → serialize → resolve → download/cleanup →
   compute-threads → run-bb → encode → respond as one straight-line function.
2. **Named smell**: **Long Method**, with the download arm as the concrete hotspot (a `match` nested in an
   `if let` with a `tokio::spawn` inside the `Ok` arm — three concerns deep).
3. **Impact**: local · blast radius 1 file (prove.rs) · warm-hot (3 commits since 06-01; it is the product's
   hot path so every proving feature lands here).
4. **Instances**: `packages/accelerator/core/src/server/prove.rs:107-236` (the whole handler); worst block
   `:156-196` (resolution + status emission + download + spawn-cleanup + versions-changed callback + error
   mapping + status re-emit); `:203-219` (success/failure logging duplicated across both match arms with
   the same `elapsed_ms` field).
5. **Why future change gets harder**: changing cache-cleanup policy or download error mapping means
   editing the middle of a request handler whose locals (`_permit`, `_guard`, `version_for_prove`,
   `threads`, `start`) stay live across phases — any reorder risks silently moving work outside the
   semaphore or the status guard. The function's RAII subtleties (drop order of `_permit` vs `_guard`)
   are invisible at the edit site.
6. **Smallest safe refactoring**: **Extract Method**: `ensure_version_available(state, to_download) ->
   Result<(), ProveError>` for `:160-196` (download + cleanup-spawn + re-emit), and optionally
   `run_bb(state, body, version) -> Result<(Vec<u8>, Duration), ProveError>` for `:197-227`. The
   characterization tests (success path + error envelopes) already pin the observable behavior.
7. **What disappears**: the 3-deep nesting; the handler reads as the phase list its module doc promises;
   download policy becomes editable without touching request-lifecycle code.

---

## Finding 6 — Status emission protocol enforced by source order (lead verdict: partially confirmed)

1. **Title**: The tray-status sequence `[Proving, (Downloading, Proving,) Idle]` exists only as the
   ordering of three separate optional-callback blocks plus a Drop guard — and the download arm is
   explicitly untestable.
2. **Named smell**: **Temporal Coupling** (the protocol is call-order, not structure), plus repeated
   optional-collaborator checks (Fowler analog: repeated null-checks → **Introduce Special Case / Null
   Object**). *Lead verdict*: the F-08 refactor already consolidated the sequence into one function with a
   characterization pin for the no-download arm — so the smell is half-fixed; what remains is the implicit
   emission protocol and the unpinnable download arm.
3. **Impact**: local · blast radius prove.rs (+ the pin in server.rs, + tray consumer in src-tauri) · warm.
4. **Instances**: `packages/accelerator/core/src/server/prove.rs:146-148` (first Proving), `:149-151`
   (StatusGuard construction), `:161-163` (Downloading), `:191-195` (re-emit Proving — the comment itself
   calls the leading Proving "redundant"), `:20-30` (Drop → Idle); the admission of untestability at
   `server.rs:1326-1331` ("the full 4-element download-arm sequence can't be unit-tested — download_bb
   needs the network").
5. **Why future change gets harder**: the no-download arm is pinned, the download arm is not — so the most
   likely future edit ("clean up the redundant re-emit at `:193-195`", invited by the comment's own word
   "redundant") leaves the tray label stuck on "Downloading bb..." for the entire proof with **no failing
   test**. Likewise inserting a new phase (e.g. Verifying) requires manually threading emissions in the
   right order across 50 lines, with correctness checkable only by reading.
6. **Smallest safe refactoring**: **Extract Class** — a `StatusReporter` owning the
   `Option<StatusCallback>` with `proving()` / `downloading()` methods and Drop → Idle (absorbing
   StatusGuard). The None case becomes a no-op inside the reporter (Special Case), deleting the three
   `if let Some(ref cb)` blocks; the emission protocol becomes one greppable type. A test seam for
   `download_bb` (closure/trait parameter) would make the 4-element sequence pinnable — demonstrated
   demand exists (the server.rs comment laments it), but treat it as an optional second step.
7. **What disappears**: the 3x duplicated optional-callback dance; order-only enforcement becomes
   type-shaped; the "redundant but load-bearing" re-emit gets a named home instead of a warning comment.

---

## Finding 7 — Wire-contract literals defined independently in two or three places

1. **Title**: The body-size cap, the two custom header names, and the /health shape literals each exist as
   multiple unlinked definitions that must stay in sync by hand.
2. **Named smell**: **Duplicate Code / Magic Literal** (Fowler: Replace Magic Literal with Symbolic
   Constant) — each instance is a silent-divergence trap, not a style nit.
3. **Impact**: local · blast radius 3 files (server.rs, prove.rs, probe.rs) + the SDK on the other side of
   each literal · hot file on one side of every pair.
4. **Instances**:
   - **50MB cap, twice**: `packages/accelerator/core/src/server.rs:217`
     (`DefaultBodyLimit::max(50 * 1024 * 1024)`) vs `server/prove.rs:105`
     (`const MAX_BODY_SIZE: usize = 50 * 1024 * 1024`). Two layers cap the same body; effective limit is
     the min of the two; nothing links them.
   - **`x-aztec-version`, twice**: `server.rs:210` (CORS allow-list) vs `prove.rs:142` (handler read).
   - **`x-prove-duration-ms`, twice**: `server.rs:212` (CORS expose) vs `prove.rs:231` (insert).
   - **Health shape (`"status": "ok"`, `"api_version": 1`), three production sites**: `server.rs:268`
     (minimal body), `server.rs:284-291` (detailed body), `probe.rs:14-17` (the consumer predicate that
     classifies our own /health) — producer and consumer of the same contract keyed to unlinked literals.
5. **Why future change gets harder**: bump the cap in the router but not the handler → requests between
   the two limits fail with the wrong status and nobody notices; add/rename a header in the handler but
   not the CORS list → browser-only breakage invisible to curl-based tests; bump `api_version` in
   `health()` but not `probe.rs` → the redundant-instance probe stops recognizing our own server and the
   bow-out logic misclassifies. Each pair is editable in isolation with all tests green on the edited side.
6. **Smallest safe refactoring**: **Replace Magic Literal with Symbolic Constant** — `MAX_PROVE_BODY_BYTES`,
   `HEADER_AZTEC_VERSION`, `HEADER_PROVE_DURATION_MS`, `API_VERSION` in `server.rs` (or `server/wire.rs`),
   referenced from all sites; probe.rs already imports `super::PORT`, so the precedent exists.
7. **What disappears**: three independent drift traps; the health producer/consumer contract gets a single
   definition point.

---

## Finding 8 — Bundled-version fallback expression repeated 3x

1. **Title**: `state.bundled_version.as_deref().unwrap_or(DEFAULT_BB_VERSION)` is computed inline at three
   sites in two files.
2. **Named smell**: **Duplicate Code** (a missing query method on the owning type).
3. **Impact**: cosmetic-to-local · blast radius 2 files · warm.
4. **Instances**: `packages/accelerator/core/src/server.rs:271-274` (health),
   `server/prove.rs:74-77` (resolve_version), `server/prove.rs:167-170` (download arm, with `.to_string()`).
5. **Why future change gets harder**: when the `"unknown"` fallback semantics change (e.g. all binaries now
   inject a real version and the fallback should log or differ per consumer), three sites in two files must
   change in lockstep; the third site is buried mid-handler (Finding 5's block).
6. **Smallest safe refactoring**: **Extract Method** — `HeadlessState::bundled_or_default(&self) -> &str`.
7. **What disappears**: the 3-site sync requirement; the fallback policy gets one owner.

---

## Finding 9 — `ResolvedVersion` carries two representations of one concept; the value object doesn't reach the main sink

1. **Title**: `ResolvedVersion` holds both the raw `&str` version and the parsed `AztecVersion`, and the
   parsed object is dropped on the cached/bundled path while the raw string flows to `bb::prove`.
2. **Named smell**: **Primitive Obsession** residue — a validated value object exists (Q3's
   `AztecVersion`) but the struct keeps a parallel primitive field, and the hottest downstream sink
   (`bb::prove` → `find_bb`) still takes `Option<&str>`. The doc claim at `prove.rs:57-59` ("every
   downstream sink takes `&AztecVersion`") holds only for the download sink.
3. **Impact**: local · blast radius prove.rs + the bb.rs boundary (+4 unit tests) · warm.
4. **Instances**: `packages/accelerator/core/src/server/prove.rs:38-41` (the dual fields
   `version: Option<&'a str>` / `to_download: Option<AztecVersion>`), `:79-89` (parsed value either moved
   into `to_download` or discarded; raw `v.as_str()` returned regardless), `:200` (raw str passed to
   `bb::prove`); `bb.rs:75-79` (`prove(…, version: Option<&str>, …)`), `bb.rs:18`
   (`find_bb(version: Option<&str>)`). Note `AztecVersion` already `Deref`s/`AsRef`s to `str`
   (`versions/mod.rs:117-129`) — the dual field buys nothing.
5. **Why future change gets harder**: anyone extending version handling must know which of the two fields
   is authoritative and that `version` is "raw but was validated earlier"; the `'a` lifetime on
   `ResolvedVersion` exists solely to borrow the raw header `String`; tightening `find_bb` to the typed
   version later requires re-threading because the type was dropped at this boundary.
6. **Smallest safe refactoring**: carry `Option<AztecVersion>` only (plus `to_download` as a bool flag or
   keep the moved value), exposing the `&str` at the bb boundary via the existing `Deref` — drops the
   lifetime parameter and the dual representation; contained to prove.rs + the four `resolve_version_*`
   tests. (Fowler: Replace Primitive with Object, completed.)
7. **What disappears**: two-fields-one-concept; the lifetime parameter; the gap between the Q3 doc claim
   and the code.

---

## Finding 10 — `AppState` Derefs to `HeadlessState` (Deref-polymorphism anti-pattern)

1. **Title**: `AppState` implements `Deref<Target = HeadlessState>` to simulate field inheritance so
   pre-split `state.<field>` reads compile unchanged.
2. **Named smell**: named analog — **"Deref polymorphism"**, a catalogued Rust anti-pattern
   (rust-unofficial/patterns); the Fowler-adjacent cost is hidden coupling via implicit name resolution.
   Deliberate and documented (Q1 migration aid), so this is the weakest finding here.
3. **Impact**: cosmetic · blast radius: every `state.<core-field>` read in prove.rs/auth.rs/server.rs · warm.
4. **Instances**: `packages/accelerator/core/src/server.rs:115-120` (the impl); representative implicit
   uses: `server/prove.rs:133` (`state.prove_semaphore`), `server/prove.rs:146` vs `server/auth.rs:51`
   (`state.on_status` is a real AppState field while `state.config` auto-derefs — visually identical,
   resolved on different types).
5. **Why future change gets harder**: adding an `AppState` field/method whose name collides with a
   `HeadlessState` one silently changes which type call sites resolve to; grepping for `\.core` undercounts
   actual core-state usage; newcomers can't tell GUI-state reads from core-state reads at the call site.
6. **Smallest safe refactoring**: none urgent — when Finding 2's `state.rs` extraction happens, prefer
   explicit `.core` access (or thin delegating accessors) in new/touched code and let the Deref retire by
   attrition. Flagging it now mainly so the extraction doesn't entrench it.
7. **What disappears**: implicit name-resolution coupling between the two state types.

---

## Out-of-scope observations (one line each, not findings)

- `server/auth.rs:115-127`: `Allow { remember: true }` with `config: None` silently skips persistence — correctness/UX behavior, out of scope.
- `server.rs:1374-1396`: `prove_rejects_oversized_body` admits it never exercises the oversized path — test-efficacy, test-only code.
- host-guard errors are `application/json` while prove/auth errors are `text/plain` — the *behavioral* unification is wire-visible (SDK-facing) and out of scope; only the duplication aspect is covered in Finding 1.
- Explicit non-findings: `bind.rs`, `probe.rs`, `host.rs` are clean, single-purpose, self-tested modules — they are the template the rest of the cluster should converge on.

## Priority order

1. Finding 1 (typed `ProveError` — 12 sites, SDK-facing envelope, hot)
2. Finding 3 (move the ~715 stranded test LOC — kills most of the monolith and the churn coupling)
3. Finding 2 (finish the module split; same campaign as 3)
4. Finding 4 (single definition of "approved")
5. Findings 5–7 (Long Method, status reporter, wire literals)
6. Findings 8–10 (small extracts / attrition)
