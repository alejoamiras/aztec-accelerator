# accelerator-core Rust Crate: QUALITY Audit Map

**Path:** `/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src`  
**Architecture:** GUI-agnostic HTTP proving server library (zero Tauri coupling). Used by both desktop app (via src-tauri) and headless `accelerator-server`.

---

## 1. MODULE INVENTORY

| File | Purpose | LOC | Notes |
|------|---------|-----|-------|
| **lib.rs** | Crate root; module exports + `log_dir()` helper | ~28 | Entry point; intentionally `build.rs`-free; zero Tauri coupling |
| **server.rs** | Router setup, state types, `/health` handler, test scaffolding | 1424 | Largest module; server config, AppState + HeadlessState, status enums |
| **authorization.rs** | RFC 6454 origin canonicalization, approval gate, popup flow | 614 | CanonicalOrigin newtype + AuthorizationManager; SEC-04/06 contracts |
| **config.rs** | Config schema (Speed, AcceleratorConfig), persist/load | 390 | Speed enum + file I/O; lenient origin deserializer (F-02) |
| **bb.rs** | bb binary discovery + execution (prove, version caching) | 280 | find_bb() resolution chain (ENV → cache → sidecar → ~/.bb → PATH) |
| **versions/mod.rs** | Version validation, cache policy, retention logic | 1006 | AztecVersion value object (Q3), NetworkTier, eviction algorithm |
| **versions/downloader.rs** | HTTP download → digest verify → atomic install | 364 | F-04 extraction; download_bb orchestrator, extract_bb_from_tarball (decompression bomb cap SEC-07) |
| **server/prove.rs** | `/prove` handler + version/thread resolution | 237 | F-08 status sequence (Proving→Downloading→Proving→Idle); StatusGuard RAII |
| **server/auth.rs** | Origin auth gate (approval, popup, timeout, remember) | 142 | authorize_origin; SEC-04 localhost auto-approve flag |
| **server/host.rs** | Host header validation (DNS-rebinding mitigation SEC-01a) | 136 | host_is_trusted; loopback-only constraint |
| **server/bind.rs** | TCP bind retry (AddrInUse wait-out + hard deadline) | 125 | Restart overlap tolerance; Q2 extraction |
| **server/probe.rs** | Redundant-instance detection (health endpoint parse) | 75 | Classify /health response; avoid silent bow-out to foreign processes |

**Total LOC (module sources only):** ~4,797 (excluding test code)

---

## 2. PUBLIC SURFACE (External API)

### lib.rs
- `pub mod authorization` — CanonicalOrigin, AuthorizationManager, AuthDecision
- `pub mod bb` — find_bb(), prove() async
- `pub mod config` — AcceleratorConfig, Speed, load(), save()
- `pub mod server` — AppState, HeadlessState, ServerStatus, start()
- `pub mod versions` — AztecVersion, NetworkTier, download_bb(), cleanup_old_versions()
- `pub fn log_dir() -> PathBuf`

### server.rs (Primary Public Exports)
- `pub struct AppState` — Full state (core + 3 GUI callbacks)
- `pub struct HeadlessState` — Core state (no GUI)
- `pub enum ServerStatus` — {Idle, Downloading, Proving}
- `pub fn router(AppState) -> Router`
- `pub fn start(AppState) -> Result<(), Box<dyn Error + Send + Sync>>`
- `pub const PORT: u16 = 59833`
- `pub const HTTPS_PORT: u16 = 59834`
- `pub const AUTH_DECISION_TIMEOUT: Duration`
- `pub type StatusCallback = Arc<dyn Fn(ServerStatus) + Send + Sync>`
- `pub type VersionsChangedCallback = Arc<dyn Fn() + Send + Sync>`
- `pub type ShowAuthPopupCallback = Arc<dyn Fn(&str, &str) + Send + Sync>`

### authorization.rs (Public)
- `pub struct CanonicalOrigin` — RFC 6454 validated origin
- `impl CanonicalOrigin::parse(input: &str) -> Option<Self>`
- `impl CanonicalOrigin::as_str() -> &str`
- `pub enum AuthDecision` — {Allow {remember: bool}, Deny}
- `pub struct AuthorizationManager`
- `impl AuthorizationManager::new() -> Self`
- `impl AuthorizationManager::request(origin: &str) -> Result<(Receiver, request_id, is_first)>`
- `impl AuthorizationManager::resolve(request_id: &str, decision: AuthDecision)`
- `impl AuthorizationManager::is_approved(origin, approved_origins, auto_approve_localhost) -> bool`

### config.rs (Public)
- `pub enum Speed` — {Low, Light, Balanced, High, Full}
- `impl Speed::to_threads() -> usize`
- `impl Speed::is_full() -> bool`
- `pub struct AcceleratorConfig` — {config_version, safari_support, approved_origins, speed, auto_update, auto_approve_localhost}
- `pub fn config_path() -> PathBuf`
- `pub fn load() -> AcceleratorConfig`
- `pub fn save(config: &AcceleratorConfig) -> Result<()>`

### bb.rs (Public)
- `pub fn find_bb(version: Option<&str>) -> Result<PathBuf, String>`
- `pub async fn prove(ivc_inputs: &[u8], version: Option<&str>, threads: Option<usize>) -> Result<Vec<u8>>`

### versions/mod.rs (Public)
- `pub enum NetworkTier` — {Nightly (keep 2), Devnet (keep 3), Testnet (keep 5), Mainnet (keep ∞)}
- `impl NetworkTier::from_version(version: &str) -> Self`
- `impl NetworkTier::retention_limit() -> Option<usize>`
- `pub struct AztecVersion` — Validated version with precomputed tier + sort_key
- `impl AztecVersion::parse(version: &str) -> Option<Self>`
- `impl AztecVersion::as_str() -> &str`
- `impl AztecVersion::tier() -> NetworkTier`
- `impl AztecVersion::sort_key() -> &(String, u64)`
- `pub fn versions_base_dir() -> PathBuf`
- `pub fn bb_binary_name() -> &'static str`
- `pub fn version_bb_path(version: &str) -> PathBuf`
- `pub fn current_platform() -> &'static str`
- `pub fn download_url(version: &str) -> String`
- `pub fn is_valid_version(version: &str) -> bool`
- `pub fn list_cached_versions() -> Vec<String>`
- `pub async fn cleanup_old_versions(bundled_version: &str)`

### versions/downloader.rs (Public)
- `pub async fn download_bb(version: &AztecVersion) -> Result<PathBuf>`

### server/bind.rs (Public)
- `pub async fn bind_with_retry(addr: SocketAddr) -> std::io::Result<TcpListener>`

### server/prove.rs (Private — internal prove handler logic)
- Used only by server.rs router

### server/auth.rs (Private — origin auth logic)
- Used only by server.rs / prove.rs

### server/host.rs (Public via server.rs middleware)
- `pub(crate) fn host_is_trusted(authority: &str, expected_port: u16) -> bool`

### server/probe.rs (Public)
- `pub async fn healthy_aztec_on_port() -> bool`

---

## 3. INTERNAL DEPENDENCY GRAPH (One Level Deep)

```
lib.rs
 ├─ authorization
 ├─ bb
 ├─ config
 ├─ server (root)
 └─ versions (root)

server.rs (depends on:)
 ├─ authorization::{AuthorizationManager, CanonicalOrigin}
 ├─ bb::{find_bb, prove}
 ├─ config::{AcceleratorConfig, Speed}
 ├─ versions::{list_cached_versions, ...}
 ├─ server/auth (internal: authorize_origin)
 ├─ server/prove (internal: prove handler)
 ├─ server/host (internal: host guard middleware)
 ├─ server/bind (internal: bind_with_retry)
 └─ server/probe (internal: healthy_aztec_on_port)

authorization.rs (depends on:)
 └─ url, parking_lot, tokio, uuid, serde

config.rs (depends on:)
 ├─ authorization::{CanonicalOrigin, ...}
 ├─ serde, dirs

bb.rs (depends on:)
 ├─ versions::{version_bb_path, bb_binary_name, ...}
 └─ which, tempfile

versions/mod.rs (depends on:)
 └─ sha2, hex, serde_json

versions/downloader.rs (depends on:)
 ├─ versions/mod (same module)::{http_client, AztecVersion, ...}
 ├─ flate2, tar, tempfile
 └─ macOS-specific: std::process::Command (xattr, codesign)

server/prove.rs (depends on:)
 ├─ bb::{prove}
 ├─ versions::{download_bb, cleanup_old_versions, AztecVersion, ...}
 ├─ server/auth (authorize_origin)
 └─ server modules: json_error, AppState, StatusCallback, ServerStatus

server/auth.rs (depends on:)
 ├─ authorization::{AuthDecision, AuthorizationManager, CanonicalOrigin}
 ├─ config::{...}
 └─ server modules: json_error, AppState, ProveError

server/host.rs (depends on:)
 └─ axum

server/bind.rs (no intra-crate deps)
 └─ tokio::net::TcpListener

server/probe.rs (depends on:)
 ├─ server::PORT constant
 └─ reqwest, serde_json
```

---

## 4. SIMILARITY CANDIDATES (Duplication Risk)

### String-Based Errors & HTTP Status Mapping
- **server.rs:326-328** `fn json_error(error: &str, message: &str) -> String` — Builds JSON error bodies
- **server/prove.rs:multiple** — Error handling duplicates this pattern (e.g. line 65-70, 123-128, 185-188)
- **server/auth.rs:multiple** — Same pattern (line 41-46, 69-73, 78-85, 99-102, 131-136)
- **Risk:** The `(StatusCode, String)` error tuple + `json_error()` call is repeated in 10+ places across server submodules. Consolidation would reduce churn.

### Version Validation at Ingress
- **server/prove.rs:60-70** `resolve_version()` parses version via `AztecVersion::parse()`
- **bb.rs:18** `find_bb(version: Option<&str>)` does NOT parse (trusts caller)
- **versions/downloader.rs:15-21** `download_bb(&AztecVersion)` takes validated type (structural guard)
- **Risk:** Asymmetry — find_bb() trusts the caller to have parsed, but the traversal guard only applies if AztecVersion is used. A direct call to `version_bb_path()` could still bypass Q3 if the path ever leaks outside the module boundary.

### Host Header Parsing
- **server/host.rs:22-42** `host_is_trusted()` parses via `Authority`, normalizes
- **server.rs:400-408** Tests do NOT use the helper (rebuild logic inline)
- **Risk:** Low — contained in the module. But the test code reimplements the parse+normalize logic manually, which is a minor duplication.

### Origin Canonicalization
- **authorization.rs:21-58** `canonicalize_origin()` function
- **authorization.rs:249-258** `AuthorizationManager::is_auto_approved()` re-parses origins with `Url::parse()`
- **Risk:** Low — `is_auto_approved()` only needs scheme+host extraction (narrower scope), so the reimplementation is justified. But it adds a second Url::parse dependency.

### Config Loading / Deserialization
- **config.rs:89-103** `load()` with fallback to default + warning
- **versions/downloader.rs:158-175** Similar error-recovery pattern in `verify_digest()`
- **Risk:** Different contexts (config vs. HTTP digest), so the similarity is superficial. No real duplication.

### Status Sequencing & Guard Patterns
- **server/prove.rs:20-30** `StatusGuard` RAII drop guard
- **server.rs:780-788** Similar RAII pattern with `EnvGuard` in tests (cleanup-on-drop)
- **Risk:** Very low — both are correct guard patterns, each in appropriate scope. Not a code smell.

### Download + Verify Loop
- **versions/downloader.rs:116-175** `download_tarball()` + `verify_digest()` orchestration
- **server/prove.rs:160-196** Parallel orchestration of "download if needed" in the prove handler
- **Risk:** The orchestration is slightly different (prove handler also emits status, downloads conditionally). Not a candidate for consolidation without breaking responsibilities.

### Decompression & Tarball Extraction
- **versions/downloader.rs:257-300** `extract_bb_from_tarball_capped()` with CappedReader
- **versions/mod.rs tests:691-804** Test helpers build tarballs the same way
- **Risk:** Very low — tests reuse the extraction logic they're testing, which is correct. No unnecessary duplication.

**Top Similarity Candidate:** `(StatusCode, String)` error tuple + `json_error()` calls across server submodules. A helper module or centralized error type would reduce boilerplate.

---

## 5. HOUSE CONVENTIONS

### Error Handling Style
- **Result<T, ProveError>** where `ProveError = (StatusCode, String)` — lightweight, tuple-based error carrying HTTP status + JSON message body
- **String-based errors** in some modules (e.g., `find_bb()` returns `Result<PathBuf, String>`)
- **Box<dyn Error + Send + Sync>** in async boundary-crossing functions (versions/downloader.rs, bb.rs::prove)
- **Fallible operations use Option/Result**, with no custom error types (except ProveError)
- **No error module** — errors are inline
- **Conventions differ by module scope:** HTTP-facing code uses (StatusCode, String); library-internal code uses String; async code uses Box trait objects

### HTTP Response Building
- **axum::Json wrapper** for success (e.g., server.rs:309 `/health` response)
- **axum::Json(json!(...))** macro for building response bodies
- **Manual (StatusCode, String) tuple** for errors to preserve `text/plain` Content-Type (Q8 wire contract — server.rs:314-328)
- **Custom headers via `response.headers_mut().insert()`** (e.g., x-prove-duration-ms in server/prove.rs:230-232)
- **SetResponseHeaderLayer** for cross-origin middleware headers (server.rs:219-222)

### Test Layout
- **Inline `#[cfg(test)] mod tests { ... }`** — all modules follow this pattern
- **#[serial]** for tests reading/writing process-global state (BB_BINARY_PATH in server.rs and bb.rs)
- **Characterization tests** (behavior-preserving docs) pinned with comments like "opus M3" or "Q8 wire contract"
- **Heavy use of temp dirs** (tempfile crate) for filesystem isolation
- **Mock utilities** (e.g., fake-bb script in server.rs:767-776 test) rather than external mocking frameworks
- **No test-specific features** — all tests use standard Rust #[test] / #[tokio::test]
- **Test count:** server.rs has ~47 tests; authorization.rs has ~33; versions/mod.rs has ~30; others proportionally fewer

### Naming Conventions
- **Module-internal functions:** `snake_case`, rarely pub
- **Public types:** `PascalCase` (AppState, ServerStatus, CanonicalOrigin, AztecVersion)
- **Constants:** `SCREAMING_SNAKE_CASE` (PORT, HTTPS_PORT, AUTH_DECISION_TIMEOUT, MAX_DOWNLOAD_BYTES)
- **Prefixes:** no special prefix; module organization is the namespacing strategy
- **Callbacks:** `Callback` suffix with type alias (StatusCallback, VersionsChangedCallback, ShowAuthPopupCallback)

---

## 6. TEST SURFACES

### Test Coverage by Module

| Module | Test Count | Coverage Shape | Notes |
|--------|-----------|---|---|
| server.rs | ~47 | HTTP contracts, auth gate, status sequencing, CORS, wire format | Heavy; includes characterization tests (Q8, Q10). Uses #[serial] for global env. |
| authorization.rs | ~33 | Origin canonicalization, approval logic, manager state | Comprehensive; RFC 6454 compliance pinned; SEC-04/06 contracts verified |
| versions/mod.rs | ~30 | Version validation, eviction policy, retention tiers, sort order | Heavy; pins Q3 value-object invariant; eviction algo via 5 tiered scenarios |
| config.rs | ~25 | Serde roundtrips, Speed enum, lenient origin deserializer | Moderate; covers F-02 deserialization behavior (drop invalid + dedupe) |
| bb.rs | ~6 | truncate_stderr, prepend_field_count_header, find_bb env handling | Light; mostly header/footer formatting |
| versions/downloader.rs | ~12 | Tarball extraction, decompression bomb cap, atomic install, digest verify | Moderate; SEC-07 bombing tests; synthetic tarballs + edge cases (symlinks, corruption) |
| server/prove.rs | 0 (tested via server.rs) | Tested indirectly via `/prove` integration tests | Coupled into server.rs tests; no standalone unit tests |
| server/auth.rs | 0 (tested via server.rs) | Tested indirectly via `/prove` + auth gate integration tests | Coupled into server.rs tests; no standalone unit tests |
| server/host.rs | ~7 | Host validation, DNS-rebinding rejection, userinfo smuggling | Isolated unit tests (pure `host_is_trusted()` function) |
| server/bind.rs | ~3 | Retry wait-out, hard deadline, non-AddrInUse propagation | Isolated unit tests with injectable timings |
| server/probe.rs | ~1 | Health response classification | Single test (pure `is_healthy_aztec_response()`) |

### Notable Test Patterns

**Characterization tests** (behavior-preserving quality refactors):
- server.rs:662-756 `prove_error_responses_stay_text_plain_json_string` (Q8 wire contract — 50+ assertions on exact error shapes)
- server.rs:758-832 `prove_success_path_and_status_sequence` (Q10 status enum pin + string literals)
- versions/mod.rs:432-484 `aztec_version_parse_matches_is_valid_version` (Q3 value-object invariant)
- authorization.rs:316-344 `is_approved_checks_both` (SEC-04 localhost flag pin)

**Heavy/Brittle Tests:**
- server.rs:758-832 — Uses fake bb binary, sets BB_BINARY_PATH env, expects exact status sequence. `#[serial]` due to global env. ~70 LOC.
- versions/downloader.rs:724-771 — Atomic-rename test with stale cache replacement scenario. Synthetic tar.gz construction.

**Test Utilities:**
- server.rs:935-953 `auth_state_with_popup()` — Helper for auth tests with mock callback
- versions/mod.rs:384-386 `av()` — Shorthand AztecVersion constructor (panics on invalid)
- versions/downloader.rs:308-328 `make_targz()` — Synthetic tarball builder

**Skip Gates:**
- versions/mod.rs:650-653, 950-955 — Tests gated behind `ACCELERATOR_DOWNLOAD_TEST=1` to avoid real network calls in CI
- server.rs:574-580 — Skips when `bb` is found on the dev machine (otherwise bb runs for 60+ seconds with garbage input)

---

## 7. LONG-FUNCTION / LARGE-MODULE HOTSPOTS

### Functions Over ~60 Lines

| File | Line | Name | Length | Purpose | Notes |
|------|------|------|--------|---------|-------|
| server.rs | 262-310 | `health()` async handler | 48 lines | `/health` endpoint; conditional detailed/minimal response (SEC-05) | Complex branching on auth state |
| server/prove.rs | 107-236 | `prove()` async handler | 130 lines | `/prove` request orchestration; auth, body read, semaphore, version resolve, download (conditional), bb invocation, response encode | **Hotspot:** Core proving path; orchestrates multiple stages |
| server/auth.rs | 16-141 | `authorize_origin()` async function | 125 lines | Origin validation + popup gating + timeout + decision handling + config persistence | Complex state machine (popup logic, timeout, decision routing) |
| versions/downloader.rs | 15-58 | `download_bb()` async orchestrator | 43 lines (actual flow) | Download → verify → extract → finalize (macOS codesign) | **Hotspot:** Network + filesystem boundary |
| versions/downloader.rs | 116-150 | `download_tarball()` async | 34 lines | Bounded streaming download (cap per chunk + header length check) | SEC-07-adjacent (decompression bomb prevention starts here) |
| versions/downloader.rs | 156-175 | `verify_digest()` async | 19 lines | Fetch digest from GitHub API + compare | Network call + error recovery |
| versions/downloader.rs | 257-300 | `extract_bb_from_tarball_capped()` | 43 lines | Gzip decode → tar parse → find bb → unpack with cap enforcement | **Hotspot:** Decompression bomb mitigation (CappedReader) |
| versions/mod.rs | 218-257 | `versions_to_evict()` | 39 lines | Grouping, sorting, retention limit enforcement per tier | Eviction algorithm; uses precomputed sort_key fields |
| bb.rs | 75-145 | `prove()` async function | 70 lines | Temp dirs, command construction, spawn with kill-on-drop, timeout, output handling | Long function with multiple error paths |
| authorization.rs | 21-58 | `canonicalize_origin()` | 37 lines | RFC 6454 validation; scheme, host, port, path, query, fragment, userinfo checks | Extensive pattern matching |

### Modules Over ~400 LOC

| File | LOC | Complexity Drivers | Maintainability Concerns |
|------|-----|---|---|
| **server.rs** | 1424 | State types (HeadlessState, AppState) 85 LOC; Tests ~660 LOC; Router setup + middleware 50 LOC; `/health` handler 50 LOC | Large monolith; test suite dominates; mixed concerns (state + routing + handler logic). Q2 extraction (prove.rs, auth.rs, host.rs, bind.rs) reduced coupling but file still carries all type definitions. |
| **versions/mod.rs** | 1006 | Tests ~520 LOC; AztecVersion value object + tier/sort_key ~150 LOC; Eviction algo ~40 LOC; List/load/validation ~100 LOC | Heavy test density; Q3 refactor introduced precomputed fields (tier, sort_key); correct but complex invariant (parse must match is_valid_version). |
| **authorization.rs** | 614 | Tests ~330 LOC; CanonicalOrigin type + trait impls ~120 LOC; AuthorizationManager ~100 LOC | Dual concerns: canonicalization (type safety for origins) + manager (state + decision flow). Tests verify RFC 6454 compliance + SEC-04/06 contracts in detail. |

### Architecturally Complex Regions

1. **server/prove.rs:156-196** — Version resolution + conditional download + status sequencing
   - Resolves version (pure), checks cache, conditionally downloads (emits Downloading status), spawns cleanup task, re-emits Proving status
   - **Issue:** Status sequencing is implicit in the control flow; the comment (F-08) explains it, but the nested conditionals make it hard to follow

2. **versions/downloader.rs:15-58** — Download orchestrator with multiple error boundaries
   - Cache check, download, verify, extract, finalize (macOS); each step can fail
   - **Issue:** Error handling is per-step (inline map_err); no consolidated error handler

3. **server.rs:237-260** — `health_is_detailed()` decision tree for SEC-05 fingerprint leakage
   - No Origin → detailed; unapproved cross-origin → minimal; approved/localhost (when flag on) → detailed
   - **Issue:** Nested if/match; the logic is correct but the branching is hard to scan

4. **authorization.rs:21-58** — `canonicalize_origin()` validator
   - Scheme + port handling varies by tuple-origin vs. opaque-origin schemes
   - **Issue:** Double-looping over url.scheme() with multiple pattern matches; correct but dense

---

## 8. QUALITY FINDINGS SUMMARY

### Strengths
1. **Type-driven safety:** CanonicalOrigin, AztecVersion newtype wrappers enforce invariants by construction (F-02, Q3)
2. **RFC 6454 compliance:** Origin canonicalization thoroughly tested; handles edge cases (punycode, IPv6, trailing dots, etc.)
3. **Security contracts pinned:** SEC-01a (DNS rebinding), SEC-04 (localhost auto-approve flag), SEC-05 (fingerprint leakage), SEC-06 (request_id opaque), SEC-07 (decompression bomb) all have characterization tests
4. **Modular responsibilities:** Q2 extractions (prove.rs, auth.rs, host.rs, bind.rs, probe.rs) separated concerns from server.rs monolith
5. **RAII patterns:** StatusGuard, EnvGuard drop guards prevent resource/state leaks
6. **Atomic operations:** Temp dir + rename for bb cache install prevents corruption on crash

### Maintainability Hotspots
1. **server.rs monolith (1424 LOC):** Carries all state definitions + tests. Further extraction (e.g., state types module) would reduce cognitive load.
2. **Asymmetric error handling:** (StatusCode, String) tuples in HTTP paths vs. String/Box errors elsewhere. No unified error type. 10+ `json_error()` calls duplicated across submodules.
3. **Implicit status sequencing:** F-08 comment documents Proving→Downloading→Proving→Idle order, but the control flow in server/prove.rs:156-196 makes it easy to disrupt.
4. **Version validation at multiple boundaries:** AztecVersion::parse enforces the guard, but find_bb() and other callees don't validate. Asymmetry.
5. **Test density:** server.rs (660 LOC tests), versions/mod.rs (520 LOC tests), authorization.rs (330 LOC tests) — nearly 50% of core logic is tests. Valuable for characterization, but makes the files hard to navigate.

### Code Smells (Low-Risk)
- `canonicalize_origin()` and `is_auto_approved()` both parse origins, but with different scope (full validation vs. host extraction)
- Host header parsing in tests reimplements normalize logic inline (server.rs:400-408)
- Multiple async orchestrators (download_bb, prove, authorize_origin) have similar error-recovery patterns but no shared abstraction
- Decompression bomb cap (512 MB) is separate from download cap (64 MB); the relationship is documented but not formalized

### Architecture Decisions (Correct, But Worth Noting)
1. **Headless vs. Desktop via callbacks:** AppState deref-delegates to HeadlessState so GUI callbacks are optional. Clean separation, though the three callback types (StatusCallback, VersionsChangedCallback, ShowAuthPopupCallback) are scattered as type aliases.
2. **Newtype value objects (Q3, F-02):** AztecVersion and CanonicalOrigin move validation from callsites into construction. Correct guard, but it means every path that *needs* a validated version must go through the parser — asymmetry if callers bypass it.
3. **Shared HTTP client pool:** http_client() in versions/mod.rs is created per-call (not pooled). Each download_bb invocation builds a new client. Low impact but slightly inefficient.
4. **Retention policy coupling:** NetworkTier classification + eviction limit are tightly coupled to version string format (e.g., "nightly", "rc"). A server release changing the format would silently break eviction.

---

## 9. REFACTOR CANDIDATES (FOR FUTURE QUALITY PASSES)

### Q9 (Hypothetical): Unified Error Handling
- Introduce an `AppError` enum that replaces `(StatusCode, String)` and ad-hoc String errors
- Centralize `json_error()` logic
- Reduces duplication in server submodules

### Q12 (Hypothetical): Status Sequencing Formalism
- Extract the Proving→Downloading→Proving→Idle machine into a state-machine type
- Prove handler owns the transitions; status guard becomes a type-safe wrapper
- Prevents accidental status-order mutations

### Q13 (Hypothetical): Callback Type Consolidation
- Define a trait (e.g., `ServerCallback`) and unify the three callback types
- Reduce API surface in AppState construction

### Q15 (Hypothetical): Async Orchestrator Abstractions
- Extract download → verify → extract pipeline into a reusable combinator
- Reduces boilerplate in download_bb, verify_digest, authorize_origin

---

## END MAP
