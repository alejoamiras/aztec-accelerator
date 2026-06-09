# Cluster C3 — core-config-auth — Quality findings (Claude)

**Verdict:** 3 findings. One structural (Primitive Obsession on canonical origin, spans both files), two local (cross-function duplication of the URL-parse/host-normalize block; temporal coupling between `config::load` migration and `is_approved`). No architectural smells; the cluster is small, cohesive, and heavily tested. Severity spread: 1 moderate-structural, 2 low-local.

---

## Finding 1 — Canonical origin is a bare `String`/`&str` everywhere despite a hard invariant

1. **Title** — Origin modeled as a primitive string, even though "canonical origin" is a domain type with an enforced invariant.

2. **Smell** — **Primitive Obsession** (Bloaters). The codebase has a genuine domain concept — a *canonicalized* origin per RFC 6454 — produced by exactly one function (`canonicalize_origin`) and required by every consumer. Yet it is represented as `String`/`&str` indistinguishable from any raw user input. The invariant ("this string is canonical") exists only in prose, not in the type.

3. **Maintenance impact** — bucket **structural**; blast radius **2 files + every caller of these APIs** (`config.rs`, `authorization.rs`, plus the src-tauri server-ingress handlers and verified-sites registry that pass origins around — per the cluster's dependency notes the popup/registry render off these strings). Change frequency: moderate — auth/origin handling is a security-relevant hot path that this audit run is itself touching.

4. **Concrete evidence** —
   - `authorization.rs:21` — `pub fn canonicalize_origin(input: &str) -> Option<String>` produces the canonical form but returns a plain `String`; the "canonical" guarantee is immediately lost at the type boundary.
   - `authorization.rs:101-104` — `request(&self, origin: &str)` takes `&str`; nothing distinguishes a canonical origin from a raw one.
   - `authorization.rs:116` — `resolve(&self, origin: &str, ...)` same.
   - `authorization.rs:126` — `is_auto_approved(origin: &str)` — comment at :128-130 must *assert in prose* "The input is already canonical" because the type cannot.
   - `authorization.rs:143` — `is_approved(origin: &str, approved_origins: &[String])` — doc at :140-142 again must spell out "The input `origin` is expected to ALREADY be canonical … Persisted entries … are likewise canonical." Two separate prose obligations guarding one missing type.
   - `config.rs:49` — `pub approved_origins: Vec<String>` — the persisted set is `Vec<String>`; canonicality is enforced only by the `migrate_approved_origins` pass (`config.rs:96-101, 107-130`), not by the type.

5. **Why it harms future change** — The canonicality invariant is enforced by convention spread across ≥4 doc comments and one migration function. A future caller (e.g. a new endpoint in src-tauri, or a new verified-sites lookup) can pass a *raw* origin to `is_approved`/`request` and it compiles cleanly, silently breaking the exact-match comparison at `authorization.rs:144` (`approved_origins.iter().any(|o| o == origin)`) — a string mismatch that fails open or closed with no compiler help. The reviewer must re-prove "is this string canonical here?" at every call site forever.

6. **Smallest safe refactoring** — **Introduce a Whole Value / newtype**: `struct CanonicalOrigin(String)` (or `Origin`) whose only constructor is `CanonicalOrigin::parse(&str) -> Option<Self>` (the current `canonicalize_origin` body). Change `request`, `resolve`, `is_auto_approved`, `is_approved`, and `approved_origins: Vec<CanonicalOrigin>` to take/hold the newtype. Serde `#[serde(try_from = "String", into = "String")]` keeps the on-disk JSON format identical and folds the migration's canonicalization into deserialization.

7. **What disappears** — All four "input is already canonical" doc-comment obligations (`authorization.rs:128-130, 140-142`), the class of bug where a raw origin reaches a comparison, and arguably the entire `migrate_approved_origins` function (canonicalization-on-load becomes deserialization via `try_from`). The invariant moves from prose to the type system.

8. **Instances** — `authorization.rs:21, 101-104, 116, 126, 143-144`; `config.rs:49, 96-101, 107-130`.

---

## Finding 2 — URL-parse + scheme-gate + host-lowercase + trailing-dot-strip duplicated across two functions

1. **Title** — The "parse URL, match tuple scheme, lowercase host, strip trailing dot" block is hand-rolled twice and the two copies have already diverged.

2. **Smell** — **Duplicate Code** (Dispensables). Two functions independently perform: `Url::parse` → filter to `http`/`https`(+`ws`/`wss`) → `host_str()` → `to_ascii_lowercase()` → `trim_end_matches('.')`. The author *noticed* this (the `Q14` comment at `authorization.rs:127-130` explicitly says it replaced a hand-rolled extractor with the "same `url::Url` parsing as `canonicalize_origin`") but stopped at copying the technique rather than extracting the shared step — so the two copies still drift.

3. **Maintenance impact** — bucket **local**; blast radius **1 file** (`authorization.rs`), but it is the security-critical origin-comparison path. Change frequency: low-moderate, but high-consequence — any host-normalization rule change (e.g. IDNA/punycode handling, Unicode case folding) must be applied in two places or the auto-approve and persisted-approve paths disagree.

4. **Concrete evidence** — duplicated normalization logic:
   - `canonicalize_origin` — `authorization.rs:35-44`: `match url.scheme() { "http"|"https"|"ws"|"wss" => { let host = url.host_str()?.to_ascii_lowercase(); let host = host.trim_end_matches('.'); if host.is_empty() { return None; } … }`
   - `is_auto_approved` — `authorization.rs:131-135`: `Url::parse(origin).ok().filter(|u| matches!(u.scheme(), "http" | "https")).and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())).is_some_and(|h| matches!(h.trim_end_matches('.'), …))`
   - **Divergence already present:** the canonical block accepts `ws`/`wss` (:35) but the auto-approve block only matches `http`/`https` (:133). They also disagree on empty-host handling (`canonicalize_origin` rejects empty at :38; `is_auto_approved` relies on the `matches!` literal list to miss it). This is exactly the drift that makes duplicate normalization dangerous.

5. **Why it harms future change** — A maintainer who updates host normalization in `canonicalize_origin` (the "obvious" home for it) will not realize `is_auto_approved` re-parses from scratch. Because `is_auto_approved` runs on already-canonical input (per Finding 1's invariant), the re-parse is also redundant work, but the real cost is two normalization codepaths that must be kept bit-identical for the security comparison at `is_approved` (:144) to be sound.

6. **Smallest safe refactoring** — **Extract Function**: a private `fn parsed_tuple_host(input: &str) -> Option<(/*scheme*/ &str, /*host*/ String)>` (or, once Finding 1 lands, make `is_auto_approved` consume the `CanonicalOrigin` newtype and split its already-normalized host instead of re-parsing). `canonicalize_origin` and `is_auto_approved` then call the one extracted normalizer.

7. **What disappears** — The second `Url::parse` + lowercase + trailing-dot-strip copy in `is_auto_approved` (`authorization.rs:131-135`), and the latent scheme-list / empty-host divergence between the two blocks.

8. **Instances** — `authorization.rs:34-44` and `authorization.rs:131-135`.

---

## Finding 3 — `is_approved` correctness depends on `config::load`'s migration having already run — encoded only in prose

1. **Title** — Temporal/ordering coupling: the approval comparison is only correct if a *separate* function in the *other* file ran first to canonicalize the persisted set.

2. **Smell** — **Temporal Coupling** (named analog; mapping below) with a dash of **Inappropriate Intimacy** across the module boundary. `is_approved` does an exact-string match (`o == origin`, `authorization.rs:144`) and is correct *only* because `config::load` (`config.rs:96-101`) invoked `migrate_approved_origins` to canonicalize `approved_origins` on read. Nothing enforces that ordering — it lives in the doc comment at `authorization.rs:140-142` ("Persisted entries … are likewise canonical (enforced by `crate::config::load`'s migration step)"). This is Temporal Coupling because two operations (load-migrate, then compare) must happen in a fixed order across a module boundary, and the contract is documentation-only.

3. **Maintenance impact** — bucket **local**, but the blast radius is **cross-module** (`config.rs` ↔ `authorization.rs`): the canonicalization rule is authored in `authorization.rs`, *invoked* from `config.rs::load`, and *depended upon* by `authorization.rs::is_approved`. Change frequency: low, but a violation fails silently in a security check.

4. **Concrete evidence** —
   - `config.rs:96-101` — `load()` calls `migrate_approved_origins(&mut config.approved_origins)` and only then returns the config; this is the *only* place the persisted set is canonicalized.
   - `config.rs:108` — `migrate_approved_origins` reaches back into `authorization`: `use crate::authorization::canonicalize_origin;` (the canonical rule lives in the other module).
   - `authorization.rs:140-142` + `:144` — `is_approved` assumes that work happened: doc says entries are "canonical (enforced by `crate::config::load`'s migration step)", then compares with raw `==`.
   - **The gap:** any code path that builds `approved_origins` *without* going through `config::load` (a test helper, a future "import settings" feature, a direct `AcceleratorConfig { approved_origins: … }` construction like the one at `config.rs:317-320`) feeds un-canonicalized strings straight into the `==` comparison, silently breaking matches.

5. **Why it harms future change** — The safe invariant ("everything in `approved_origins` is canonical") is maintained by a single chokepoint (`load`) that is easy to bypass. A future contributor adding a settings-import or sync feature, or mutating `approved_origins` after load, has no compiler or API guard reminding them to canonicalize. The failure mode is a silent authorization mismatch — the worst place to rely on a prose contract.

6. **Smallest safe refactoring** — Subsumed by Finding 1: making `approved_origins: Vec<CanonicalOrigin>` with serde `try_from` moves canonicalization into deserialization, so *any* path that loads or constructs the set is canonical by construction and the ordering contract evaporates. If the newtype is deferred, the minimal local fix is **Encapsulate Field**: route all mutation/construction of `approved_origins` through an `add_approved_origin(&str) -> bool` helper that canonicalizes, removing the public-field bypass.

7. **What disappears** — The `config::load`-must-run-first ordering obligation, the `authorization.rs:140-142` doc contract, and the bypass class where a non-`load` construction path inserts a raw origin into a set compared by exact match.

8. **Instances** — `config.rs:96-101, 107-130, 317-320`; `authorization.rs:140-145`.

---

### Notes (not findings)

- `Speed` / `AcceleratorConfig` are **not** Data Class smells: `Speed::to_threads` (`config.rs:18-29`) and `is_full` (:32) give `Speed` behavior, and `AcceleratorConfig`'s logic correctly lives in free functions (`load`/`save`/`migrate`) — appropriate for a serde DTO.
- `config_version` / `default_config_version` (`config.rs:39, 44-45, 69-71`) is a migration seam with no consumer yet, but it is a deliberate forward-compat hook with a documented contract, not **Speculative Generality** worth flagging per the DO-NOT-FLAG rule on speculative future flexibility.
- The `#[cfg(unix)]` permission-setting in `save` (`config.rs:138-143, 149-160`) is platform-conditional, not duplicated logic.
