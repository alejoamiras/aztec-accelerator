# Quality audit — core-auth-config cluster (claude)

Scope: `packages/accelerator/core/src/authorization.rs` (614 LOC, ~277 production), `packages/accelerator/core/src/config.rs` (389 LOC, ~165 production). Cross-file instances traced into `core/src/server/auth.rs`, `core/src/server.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`, `server/src/main.rs` only where the smell originates in a target file's API.

Context for the consolidator: both files were actively de-smelled recently (PR-1/F-02 introduced `CanonicalOrigin` to kill comment-only canonicality contracts; `mutate_config` was extracted to kill copy-pasted save blocks). Every finding below is a seam where that paydown stopped short — no HIGH findings; the code is small and cohesive.

---

## F1. Pending-request dual map synced by discipline, not structure

1. **Title**: `by_origin` + `by_request` must be mutated in lockstep; nothing enforces the pairing.
2. **Smell**: **Temporal Coupling** (operations on two structures must always happen together, in order), with a **Mutable Data** (Fowler 2nd ed.) overtone — `by_origin` is derived data that can drift from `by_request`.
3. **Impact**: MEDIUM. Blast radius: `AuthorizationManager` only, plus every future pending-flow feature (cancel-on-window-close, expiry sweep, "pending origins" settings view). Change frequency: WARM — 3 of the last 4 commits to this file reworked exactly this flow (SEC-06 `b0dfcc0`, SEC-01b/c `d908193`, F-02 `5c57d39`).
4. **Instances** (all `core/src/authorization.rs`):
   - 171–177 — `PendingState` declares the two maps as plain fields with no invariant-preserving API.
   - 220–231 — `request()`: paired insert into both maps (224 + 225).
   - 238–246 — `resolve()`: paired remove from both maps (240 + 241).
   - 212–217 — the tell: the piggyback path nests `if let Some(req) = st.by_request.get_mut(&request_id)` *inside* a successful `by_origin` hit — defensive handling of a desync that is supposed to be impossible. Readers must re-derive "can these drift?" on every change.
   - (Adjacent doc note, same lines: the `/// Manages pending authorization requests…` block at 154–159 is attached to the `MAX_PENDING_ORIGINS` const at 162, leaving `AuthorizationManager` (179) undocumented in rustdoc — the dual-map design description lands on the wrong item.)
5. **Why future change gets harder**: any third mutation site (a `cancel(origin)` command, a timeout sweep that prunes stale entries, a UI listing) must remember to touch both maps under the same lock with matching keys. The compiler is silent if it doesn't; the failure mode is quiet (piggyback gate breaks → duplicate popups, or an entry leaks until resolve). Today's two sites are correct by inspection; the cost is paid by every future reader proving that again.
6. **Smallest safe refactoring**: **Substitute Algorithm + Encapsulate Collection** — delete `by_origin` entirely and find piggyback targets by linear scan: `st.by_request.iter_mut().find(|(_, r)| r.origin == origin)`. The map is hard-capped at `MAX_PENDING_ORIGINS = 10` (162), so the index buys nothing. Alternative if both maps stay: give `PendingState` `insert(origin) -> request_id` / `remove(request_id) -> Option<PendingRequest>` methods and make `AuthorizationManager` use only those.
7. **What disappears**: the second map, the defensive nested `if let` at 214, the cross-map pairing obligation, and the desync question itself — future pending-flow features become single-map edits.

---

## F2. Lock-mutate-save exists three times, with three different error policies

1. **Title**: the "single source of truth" `mutate_config` helper has two unextracted siblings in other crates — and the copies have already diverged on what to do when `save` fails.
2. **Smell**: **Duplicate Code** (Fowler), aggravated by **Feature Envy** in the helper's placement — `mutate_config` lives in the GUI crate but touches only core types (`Arc<RwLock<AcceleratorConfig>>` + `config::save`), which is exactly why the two core-side sites cannot reuse it.
3. **Impact**: MEDIUM. Blast radius: every config-persisting flow across three crates (Tauri commands, the in-core approval flow, Tauri startup recovery). Change frequency: WARM — config.rs keeps gaining persisted fields (`auto_update` #100, `auto_approve_localhost` `d908193`), and each new mutation site picks one of three patterns by example.
4. **Instances**:
   - `src-tauri/src/commands.rs:12–19` — `mutate_config`: lock → mutate → save, **propagates** the error. Doc comment (10–11) claims it "replaces copy-pasted `config.write()` + `config::save` blocks" — true only within its own crate. Users: 60, 68–71, 172, 190, 212, 236.
   - `core/src/server/auth.rs:117–126` — inline lock → `contains`/push → `config::save`, **warns and continues** on error (122–124). Cannot call the helper: it lives downstream.
   - `src-tauri/src/main.rs:98–104` — `reset_safari_support`: inline lock → mutate → `let _ = config::save(&cfg)`, **silently swallows** the error (102) — in the same crate as the helper, post-dating its "propagates the save error" contract.
   - Supporting: `ConfigState = Arc<RwLock<AcceleratorConfig>>` is aliased in `src-tauri/src/commands.rs:8` but spelled structurally in core (`core/src/server.rs:98`, `server/src/main.rs:83`) — the shared-config type has no canonical home.
5. **Why future change gets harder**: a change to save semantics (debounce, fsync, migration-on-save, surfacing persist failures to the tray) must be hunted across three sites in two crates, and the misleading "single source of truth" comment actively points maintainers away from two of them. The error-policy drift (propagate / warn / swallow) is the documented rot mechanism of duplicate code — it has already happened here.
6. **Smallest safe refactoring**: **Move Function** — relocate `mutate_config` (and the `ConfigState` alias) into `core::config` as `pub fn mutate(lock: &ConfigState, f: impl FnOnce(&mut AcceleratorConfig)) -> Result<(), …>`; the three sites call it and apply their *deliberate* policy to the returned `Result` (`?`, `warn!`, or an explicit commented `let _ =`). Behavior-preserving; policies stay where they are, the mechanics stop being copied.
7. **What disappears**: two inline copies, the now-false "single source of truth" claim, the structural re-spelling of the config-lock type in three places, and the pick-a-pattern-by-example trap for the next persisted setting.

---

## F3. `CanonicalOrigin` stops at the approval check — the pending pipeline runs on raw strings

1. **Title**: the newtype built to kill comment-only canonicality contracts guards `is_approved` only; `request`/`resolve`/popup/removal still traffic in `&str`, including one surviving comment-only contract in the same file.
2. **Smell**: **Primitive Obsession** (Fowler) — domain value (canonical origin) degraded to `String` across an API seam; minor **Speculative Generality** rider: the newtype's `Borrow<str>`/`AsRef<str>`/`PartialEq<str>` kit (84–93, 99–103) has zero production users (PartialEq<str> is test-only, authorization.rs:545) while the one map that would exercise `Borrow<str>` still keys by `String`.
3. **Impact**: MEDIUM. Blast radius: every current and future caller of `request` / `is_auto_approved` / the popup callback / origin removal — i.e. the whole prompt-and-remember flow. Change frequency: WARM (same SEC-04/05/06 churn as F1).
4. **Instances**:
   - `core/src/authorization.rs:166–168` — `PendingRequest.origin: String`.
   - `core/src/authorization.rs:173–174` — `by_origin: HashMap<String, String>` (origin-keyed piggybacking on raw strings).
   - `core/src/authorization.rs:206–209` — `pub fn request(&self, origin: &str)`.
   - `core/src/authorization.rs:249–259` — `pub fn is_auto_approved(origin: &str)` whose comment (250–253) states "The input is already canonical" — exactly the comment-only precondition the newtype's own doc (60–64) says it exists to eliminate, one screen below that doc.
   - Downgrade points: `core/src/server/auth.rs:77` (`request(origin.as_str())`), `:90` (`show_popup(origin.as_str(), &request_id)`); `core/src/server.rs:80` (`ShowAuthPopupCallback = Arc<dyn Fn(&str, &str)>` — two positional `&str`s, origin and request_id, swappable without a compile error).
   - `src-tauri/src/commands.rs:64–72` — `remove_approved_origin(origin: String)` compares a raw frontend string against canonical entries with `o.as_str() != origin.as_str()`; correctness rests on the Settings UI round-tripping values it got from `get_config` (discipline, not type).
5. **Why future change gets harder**: canonicality must be re-established by reading call chains instead of signatures. A non-canonical string reaching `request` splits a piggyback group (`HTTPS://X.com` and `https://x.com` key separately → duplicate popups); reaching `remove_approved_origin` it makes removal a silent no-op. Both degrade quietly, so every reviewer of every new call site re-proves the invariant forever — the precise cost F-02 was paid to remove.
6. **Smallest safe refactoring**: **Replace Primitive with Object** at the remaining seams — `request(&CanonicalOrigin)`, `PendingRequest.origin: CanonicalOrigin`, `by_origin: HashMap<CanonicalOrigin, String>` (the `Borrow<str>` impl finally earns its keep), `is_auto_approved(&CanonicalOrigin)` (or demote it to a private helper of `is_approved`), and `remove_approved_origin` parsing via `CanonicalOrigin::parse` with explicit handling of invalid input. (`request_id` legitimately stays `String` — it round-trips through a URL query param.)
7. **What disappears**: the comment-only precondition at 250–253, the `.as_str()` downgrades, the silent-no-op removal bug class, the unused-conversion-kit oddity, and per-hop canonicality review.

---

## F4. `is_approved`'s parameter pair is a Data Clump unpacked identically at both call sites — on a type it doesn't belong to

1. **Title**: `(approved_origins, auto_approve_localhost)` always travel together into a static fn on `AuthorizationManager` that uses no `AuthorizationManager` state.
2. **Smell**: **Data Clump** (the two config fields) + **Feature Envy** (the function reads only `AcceleratorConfig` data but lives on `AuthorizationManager` as a self-less associated fn — a namespace, not an owner).
3. **Impact**: LOW. Blast radius: two call sites today; grows with any new approval-policy input. Change frequency: WARM (this signature was created/extended in `5c57d39` and `d908193`).
4. **Instances**:
   - `core/src/authorization.rs:269–276` — `is_approved(origin: &CanonicalOrigin, approved_origins: &[CanonicalOrigin], auto_approve_localhost: bool)`; `is_auto_approved` (249) likewise has no `self`.
   - `core/src/server/auth.rs:51–58` and `core/src/server.rs:247–259` — the same six-line lock → read → unpack-two-fields → call block, twice. Note for any dedupe: the two blocks already encode *different* `config: None` policies (server.rs:258 → detailed/true; auth.rs:51 `is_some_and` → false) — hidden variation riding on duplicated structure.
5. **Why future change gets harder**: adding a policy input (deny-list, per-origin expiry, verified-site fast-path) means widening a three-arg signature and editing every unpack site in two files, while keeping the SEC-04 flag semantics aligned across copies by hand.
6. **Smallest safe refactoring**: **Move Function** — `impl AcceleratorConfig { pub fn is_origin_approved(&self, origin: &CanonicalOrigin) -> bool }` in config.rs (delegating to `is_auto_approved` for the localhost arm); call sites collapse to `cfg.read().is_origin_approved(&origin)`. The differing `None` policies stay at the call sites, where they are visible decisions.
7. **What disappears**: both unpack blocks, future signature churn, and the misdirection of approval policy living on the pending-popup manager.

---

## F5. `load()`/`save()` hard-wired to the real home directory — the file's own "roundtrip via save/load" test cannot call either

1. **Title**: `config_path()` is baked into `load`/`save`, so the atomic-rename + permissions logic ships untested and the roundtrip test hand-duplicates `save`'s body.
2. **Smell**: **Global Data** (Fowler 2nd ed.) — a hidden hard-coded resource dependency; the named fix is **Parameterize Function**.
3. **Impact**: MEDIUM. Blast radius: all of config persistence — every future on-disk change (new field shape, fsync, migration) is verified only manually. Change frequency: WARM, and specifically the *save* path has churned (atomic write + 0o600 #87, versioning #100, `skip_serializing_if` for `auto_update`).
4. **Instances** (all `core/src/config.rs`):
   - 82–87 — `config_path()` resolves `dirs::home_dir()` directly.
   - 94–103 — `load()` calls it with no injection point.
   - 132–165 — `save()` likewise; the tmp-file + rename + `0o600`/`0o700` block (134–163) is the most platform-sensitive code in the file.
   - 179–206 — the in-file evidence: `config_roundtrip_via_save_load` admits "Override config_path by writing/reading directly through save()/load()" then does neither — it re-implements serialization at 196–200 (`serde_json::to_string_pretty` + `fs::write`, annotated "same as save()"). The test's name overstates it; the atomic/permission logic has zero coverage.
5. **Why future change gets harder**: any edit to the write path (the likeliest place for a regression — partial writes, perms, Windows rename semantics) cannot get a unit test without either touching the real `~/.aztec-accelerator` or env-var gymnastics; meanwhile the "same as save()" test comment must be manually kept in sync with `save`'s actual behavior — duplicate-by-description.
6. **Smallest safe refactoring**: **Parameterize Function** — `pub fn load_from(path: &Path) -> AcceleratorConfig` / `pub fn save_to(path: &Path, cfg: &AcceleratorConfig) -> Result<…>`, with `load()`/`save()` as one-line wrappers applying `config_path()`. Tests run the real functions against `tempfile::tempdir()`.
7. **What disappears**: the lying test (replaced by a true roundtrip through the production code), the untested 30-line unix write block, and the temptation for the next persistence test to copy the serialize-by-hand pattern.

---

## F6. Canonicalize-and-dedupe origin-list loop duplicated across crates (LOW / borderline)

1. **Title**: the parse-each → `contains`-dedupe (order-preserving) kernel is hand-rolled twice, with a third variant.
2. **Smell**: **Duplicate Code** — same sub-algorithm in sibling functions; honest caveat: the invalid-entry *policy* differs deliberately per site (drop+warn / fail-fast / fail-on-dup), and the shared kernel is ~5 lines, so this is the weakest finding here.
3. **Impact**: LOW. Blast radius: three list-ingestion sites in three crates. Change frequency: WARM (all three touched in the F-02/SEC-01c wave).
4. **Instances**: `core/src/config.rs:110–128` (`de_approved_origins`: drop invalid + warn, dedupe via `out.contains`); `server/src/main.rs:112–126` (`parse_allowed_origins_env`: error on invalid, same `contains` dedupe); variant `src-tauri/src/verified_sites.rs:103–114` (error on invalid *and* on duplicate, map-keyed).
5. **Why future change gets harder**: a rule change to list ingestion (cap entry count, normalize ordering, report duplicates) must be replicated at three sites, and the two `Vec`+`contains` dedupes can drift (e.g. one becoming case-tolerant) without any test failing across crates.
6. **Smallest safe refactoring**: **Extract Function** in `authorization.rs` — e.g. `CanonicalOrigin::parse_dedup<'a>(items: impl Iterator<Item = String>) -> (Vec<CanonicalOrigin>, Vec<String>)` returning survivors + rejects; each caller keeps its own policy (warn vs `Err`) over the rejects. Verified-sites can stay as-is (its dup-is-an-error semantics are map-shaped).
7. **What disappears**: the two hand-rolled dedupe loops and the cross-crate drift risk; policy code stays exactly where it is.

---

## Non-findings (leads attacked and partially rejected)

- **"Data Clump in the config fields"** (lead 2b): rejected as stated — `AcceleratorConfig`'s six fields already cohere in one struct with no subgroup traveling separately. The real clump is the `(approved_origins, auto_approve_localhost)` *pair at the `is_approved` seam* → F4.
- **Duplicate Code inside config.rs itself** (lead 2a): rejected — `load`/`save` are single functions; the duplication is at the *mutate* call sites in other crates (F2) and in the test that re-implements `save` (F5).
- The `cfg(unix)`/`cfg(not(unix))` split in `save()` is idiomatic conditional compilation, not duplication.
- The test-only `co(s)` helper appears in three test modules — test code, not production-wired; not flagged.

## Out-of-scope observations

- `config: None` means "serve detailed /health" in `server.rs:258` but "not approved" in `auth.rs:51` — an intentional headless allow-all asymmetry that is trust-boundary semantics, not assessed here.
