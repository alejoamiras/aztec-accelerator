# Opus independent plan — condensed key deltas (2 of 3)

## Ground-truth facts it verified (resolving my inferences)
- `HeadlessState` is `#[derive(Clone, Default)]`, 6 fields incl. `https_bound: Arc<AtomicBool>`. Prod construction = exactly 2 sites (`server/main.rs:62-75`, `src-tauri/main.rs:345-367`); all other sites are tests using `AppState::default()`.
- `canonicalize_origin` (`authorization.rs:21-58`) already `pub`, RFC-6454, idempotent, **20+ unit tests** pinning the algorithm. `migrate_approved_origins` at `config.rs:104-130` (+5 tests at 327-393); `load` resaves on change (`config.rs:96-101`).
- `versions::` consumers: `bb.rs:29/40/48`, `core/server.rs:157`, `prove.rs:45/64/70/76`, `tray.rs:57` — all via `versions::` path → re-export keeps stable. **(RESOLVES: F-04 is a clean pure-move.)**
- **SDK: `ky` rides on `globalThis.fetch`; the test mocks fetch → ONE mock covers BOTH stacks.** The existing 28-test suite validates the F-06 transport extraction **unchanged**. Strong existing coverage (protocol caching, dual-probe, "detected protocol used for subsequent /prove").
- **RESOLVES my Ask (c):** e2e `ALLOWED_ORIGINS=http://localhost:5173` (README:158) is **already canonical** → F-02 canonicalization is a no-op there → **no harness change needed.**

## Refinements over main's plan (adopt these)
- **F-02 serde = field-level `#[serde(deserialize_with="de_approved_origins")]`, NOT type-level `try_from`** — so one bad persisted origin drops+warns without failing the whole config load (matches today's tolerance). `#[serde(transparent)]` for Serialize. Keep `canonicalize_origin` as the `pub(crate)` engine (its 20+ tests stay green).
- **F-01 needs a hand-written `Default` for `HeadlessState`** (can't derive once `prove_semaphore`/`app_version` are non-`Option`): Default fills `prove_semaphore: Arc::new(Semaphore::new(1))` + `app_version: env!("CARGO_PKG_VERSION")`. Note: making `prove_semaphore` non-Option makes `prove.rs:143` acquire unconditionally — a behavior change **for tests only** (default previously skipped the permit); the manual Default fixes it.
- **PR-1 order: F-02 FIRST** (it changes `approved_origins`' element type that `headless()`'s signature references), then F-01.
- **F-08**: split `resolve_version` → `parse_and_check_cache(...) -> {version, needs_download}` (pure, no callbacks); `prove()` emits `Downloading`/`Proving` around the download. The 3 `resolve_version_*` tests (`server.rs:1046-1074`) must update. **The download-arm status sequence is UNCOVERED** → the ONE justified new characterization test (Ask #2).
- **F-03**: keep callback builders as LOCAL closures inside `build_app_state` (extracting to free fns = lifetime churn, no gain). The Windows `#[cfg(target_os="windows")]` AddrInUse bow-out moves with `spawn_http_server` — off the PR gate (Ask #4).
- **F-07**: keep `ca_key_path` standalone (legacy-migration target, NOT part of the served triple); `certs_exist` keeps its leaf-validity check (not just `.exists()`); preserve rename order ca→leaf→key in `swap_into`. Existing `generation_writes_no_ca_key` test calls `write_new_cert_set` positionally → must update to the `CertPaths` arg.
- **F-06**: keep the parse→`AcceleratorStatus` discriminated-union construction IN the prover (domain logic); move only probe/cache/URL/protocol to `AcceleratorTransport`. Route every `#acceleratorProtocol` mutation through `transport.setProtocol` (the "does not cache protocol on non-ok" + "detected protocol used for subsequent /prove" tests pin exactly-when).
- **F-05**: `AcceleratorProtocol` barrel export is purely additive. Doc-sync test options: minimal (assert barrel export-set) → fuller (read README/SKILL .md, assert phase-literal parity). (Ask #3.)

## Security matrix (per-vector, with EXISTING test names)
Case→lowercased, trailing-dot→stripped, default-port→elided, scheme→exact-match (file/data/js rejected), userinfo→rejected, path/query/fragment→rejected, whitespace/NUL→parse-fail, **IDN→punycode (no homograph collision, but NOT unit-tested in-repo → add a test)**. Closing the headless bypass is *strictly safer*: raw `ALLOWED_ORIGINS` could never match a canonicalized request `Origin` today (dead/fail-closed); canonicalizing makes it work as intended. The no-`Origin` auto-approve boundary (`auth.rs:32`) is unchanged.

## Opus's 4 Asks (fold into plan)
1. **F-02 resave removal**: deleting `migrate_approved_origins` also removes the one-time on-disk *rewrite* at load. Configs still deserialize losslessly, but the file isn't rewritten — confirm no external tooling depends on the resave.
2. **F-08 download-arm test**: the one justified new characterization test (risky reorder, zero coverage of the download status arm) — wanted, or accept compiler + no-download char-test?
3. **F-05 doc-sync depth**: barrel export-set assertion vs. fuller README/SKILL `.md` phase-parity read.
4. **PR-2 Windows arm**: F-03 moves the Windows-only redundant-instance bow-out, validated only by `_e2e-crash-recovery-windows.yml` (off the PR gate) — confirm it runs before merge, or accept compiler-only for that arm.
