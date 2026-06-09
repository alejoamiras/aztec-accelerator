# Main-agent independent plan — quality-fixes (1 of 3)

## Per-finding approach (concrete shapes)

### F-01 — `AppState`/`HeadlessState` factory + selective non-`Option`
**Key subtlety:** `app_version` and `bundled_version` are **injected by each binary** (the headless `server` crate's `CARGO_PKG_VERSION` is release-patched; src-tauri injects its own). So the core constructor must take them as **params**, not hardcode `env!`. `config` and `auth_manager` **stay `Option`** (headless runs with `config: None` when `ALLOWED_ORIGINS` is unset — `server/src/main.rs:59`). Only genuinely-always-present deps become required.

```rust
// core/src/server.rs
impl HeadlessState {
    pub fn new(
        config: Option<Arc<RwLock<AcceleratorConfig>>>,
        auth_manager: Option<Arc<AuthorizationManager>>,
        app_version: String,            // was Option — always injected → required
        bundled_version: Option<String>,// stays Option (env-derived, may be unset)
    ) -> Self {
        Self { config, auth_manager, app_version,
               prove_semaphore: Arc::new(Semaphore::new(1)), // was Option — always Some → required
               bundled_version }
    }
}
impl AppState {
    pub fn headless(core: HeadlessState) -> Self { Self { core: Arc::new(core), on_status: None, show_auth_popup: None, on_versions_changed: None } }
    pub fn desktop(core: HeadlessState, on_status: OnStatus, show_auth_popup: ShowAuthPopup, on_versions_changed: OnVersionsChanged) -> Self { ... }
}
```
Make `prove_semaphore: Arc<Semaphore>` and `app_version: String` non-`Option`; update the 2 prod construction sites + the ~5 test helpers to use the ctor. `..Default::default()` trap gone (ctor lists every field).

### F-02 — `CanonicalOrigin` newtype (closes the headless gap)
```rust
// core/src/authorization.rs
pub struct CanonicalOrigin(String);
impl CanonicalOrigin {
    pub fn parse(input: &str) -> Result<Self, OriginError>; // wraps the EXISTING canonicalize_origin (url::Url) logic
}
impl TryFrom<String> for CanonicalOrigin { ... }
impl AsRef<str> / Display / PartialEq / Eq / Hash
// serde: derive Serialize (as string) + Deserialize via try_from (idempotent on already-canonical strings)
```
- `config.approved_origins: Vec<CanonicalOrigin>`. **Lenient Vec deserialize** (custom `Deserialize` collecting valid + `tracing::warn`-dropping invalid) so one bad persisted entry can't brick config load. This **replaces** `migrate_approved_origins` — the serde `try_from` canonicalizes on load (idempotent on existing canonical data → lossless).
- Collapse the **duplicated** URL-parse block (`authorization.rs:34-44` vs `131-135`) into `CanonicalOrigin::parse` — also fixes the divergence (one path accepted `ws`/`wss`, the other only `http`/`https`).
- `server/auth.rs` ingress: parse the incoming `Origin` header into `CanonicalOrigin`, compare against `Vec<CanonicalOrigin>`.
- **Close the gap:** `server/src/main.rs:43-57` routes each `ALLOWED_ORIGINS` entry through `CanonicalOrigin::parse` (invalid → warn+skip). Behavior change: env origins now canonical (intended).

### F-03 — extract `.setup` phases (depends on F-01's `AppState::desktop`)
Extract from `main.rs:260-462`: `build_tray(app)`, `wire_core_callbacks(...) -> (on_status, show_auth_popup, on_versions_changed)`, `classify_bind_error(e) -> BindOutcome` (the `:396-441` AddrInUse block), `spawn_http_server(...)`, `spawn_update_poller(...)`. `.setup` becomes a linear orchestration: build tray → wire callbacks → `AppState::desktop(...)` → spawn servers → spawn poller. **Pure move** (same calls/order). Validity = WebDriver E2E (exercises real startup) + compiler.

### F-04 — split `versions.rs` into a module dir (pure move)
`core/src/versions/{platform,layout,cache,download}.rs` + `versions/mod.rs` (keeps `AztecVersion`, `is_valid_version`, and **`pub use` re-exports** so `bb.rs`/server/src-tauri callers are unchanged). `download.rs` gets the macOS `xattr`+`codesign` tail extracted into `finalize_macos_binary()` (folds the C2 sub-findings). Validity = compiler + the (extensive) existing `versions` tests move with the code.

### F-05 — SDK doc-sync (SDK PR)
- README `:88-101` → replace the obsolete flat `interface` with the shipped discriminated union.
- `index.ts` → add `AcceleratorProtocol` to the barrel (additive, non-breaking).
- README method table → add `setForceLocal`.
- **Doc-sync test** (the deliverable): a `bun:test` asserting the barrel exports the expected name set (incl. `AcceleratorProtocol`) + a guard that fails if the README still contains the obsolete `interface AcceleratorStatus {` block. Cheap, catches future drift.

### F-06 — extract `AcceleratorTransport` (internal; NO public API change)
New **non-exported** class owning URL construction, the dual http/https probe + protocol negotiation, the status cache, and one normalized error model. `AcceleratorProver` delegates `/health` + `/prove` to it. **Unify on `ky` for both** (already a dep; gives consistent timeout/retry) — replicate the `/health` `Promise.any` dual-probe inside the transport. Public methods/types unchanged. Validity = existing 28 SDK unit tests + new `AcceleratorTransport` unit tests (URL building, protocol negotiation, error mapping).

### F-07 — `CertPaths` parameter object (pure refactor)
```rust
struct CertPaths { ca_cert: PathBuf, leaf_cert: PathBuf, leaf_key: PathBuf }
impl CertPaths { fn live(base:&Path)->Self; fn staged(base:&Path)->Self; fn exists(&self)->bool; fn swap_into(&self, live:&CertPaths)->io::Result<()> }
```
Replaces the 3×`&Path` args + the parallel `.new` triplet + the element-wise rename dance. Unit tests for the path/swap logic.

### F-08 — move `/prove` status ownership into `prove()` (behavior-preserving)
`resolve_version()` returns data only (no status emission); `prove()` emits `Downloading` before the download and `Proving` after. Net emitted sequence identical → the **characterization test at `server.rs:617-685` stays green** (that's the validity proof; no new test).

### F-09 — extract shared `spawn_https()` (pure refactor)
`fn spawn_https(state, tls_config)` in `src-tauri/src/server.rs`; both `main.rs:try_start_https` and `commands.rs` call it after their own preamble + `load_rustls_config()`.

## PR structure + ordering
- **PR-1 (Rust core invariants):** F-01 then F-02. Both in `core` + the two binaries. `accelerator.yml` gate.
- **PR-2 (Rust structural):** F-04 (independent) + F-03 (uses PR-1's `AppState::desktop`). **After PR-1.**
- **PR-3 (Rust local):** F-07 + F-08 + F-09 — independent, any order. Can land anytime.
- **PR-4 (SDK):** F-05 + F-06. `sdk.yml` gate. Independent of the Rust PRs.
Recommended sequence: PR-1 → PR-2; PR-3 and PR-4 interleave freely. Each PR: full `bun run test` + `bun run lint` + `cargo test`/`clippy` + the WebDriver E2E gate green before auto-merge.

## Test plan (no blanket characterization — per user steer)
| Finding | Existing coverage (regression net) | NEW tests |
|---|---|---|
| F-01 | server tests + WebDriver E2E | unit: ctor sets fields; compiler enforces non-Option |
| F-02 | existing auth tests | unit: `CanonicalOrigin::parse` attack matrix (case/trailing-dot/port/scheme/IDN/userinfo/path/whitespace); serde idempotence on existing data; ALLOWED_ORIGINS canonicalization; lenient-drop of invalid persisted entry |
| F-03 | WebDriver E2E (real startup) + compiler | none |
| F-04 | existing `versions` tests + compiler | none |
| F-05 | — | the doc-sync test (deliverable) |
| F-06 | 28 SDK unit tests | unit: `AcceleratorTransport` URL/protocol/error |
| F-07 | existing cert tests | unit: `CertPaths` live/staged/swap |
| F-08 | characterization `server.rs:617-685` | none |
| F-09 | E2E + compiler | none |

## Security & Adversarial Considerations
**F-02 origin canonicalization attack matrix** (all handled by `url::Url` inside `CanonicalOrigin::parse`): uppercase scheme/host → lowercased; default port (`:443`/`:80`) → dropped, explicit non-default → kept (distinct origin); userinfo `user:pass@` → stripped by `.origin()`; path/query/fragment → stripped; IDN `exämple` → punycode `xn--` (kills homograph bypass); embedded whitespace/NUL/control → parse error → rejected; trailing dot → normalized consistently (pin with a test). The newtype makes a non-canonical origin **unconstructable**, so no ingress can smuggle one past exact-match approval. **Closing the headless gap is strictly safer:** request origins are already canonicalized at ingress, so a raw `ALLOWED_ORIGINS` entry could *never* match today (dead/fail-closed); canonicalizing it makes approval work as intended for already-canonical inputs (the e2e case = no-op) — no new trust granted, a dead path made live-correct. **Supply chain:** no new deps (`url`, `serde`, `ky` all already present). **Least privilege / SDK:** F-06 is internal-only; no public surface or capability change.

## Assumptions
**Facts:** headless sets `prove_semaphore`+`app_version` to `Some` and `config: None` when no origins (`server/src/main.rs:59-75`); `versions.rs` is `pub`-consumed by `bb.rs`+server+src-tauri (re-exports keep paths stable); status characterization test at `server.rs:617-685`; `url`/`serde`/`ky` already deps; F-02's `canonicalize_origin` is the audited Safari-era logic.
**Inferences (attack):** that `url::Url` handles every vector above identically to the current `canonicalize_origin` (must pin with the new attack-matrix test); that unifying F-06 on `ky` preserves the `/health` `Promise.any` dual-probe semantics (replicate carefully); that removing `migrate_approved_origins` is safe given lenient serde (existing persisted origins are already canonical → idempotent).
**Asks (surface):** (a) lenient-drop vs fail-load on an invalid persisted origin — I propose **lenient drop + warn** (robustness); (b) F-06 unify on `ky` vs `fetch` — I propose **ky**; (c) confirm the e2e/WebDriver `ALLOWED_ORIGINS` values are already canonical (else the harness needs a one-line tweak). All three are low-stakes and I have a default; none block planning.
