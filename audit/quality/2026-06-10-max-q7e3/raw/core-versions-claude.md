# Quality audit — core-versions cluster (claude)

Cluster: `packages/accelerator/core/src/versions/mod.rs`, `packages/accelerator/core/src/versions/downloader.rs`, `packages/accelerator/core/src/bb.rs`.
Scope: maintainability smells only (Fowler catalog + named analogs). Code is post-refactor (Q3/Q11/F-04 passes visible in comments); findings below are the residue those passes left, not greenfield rot.

Change-frequency measurement: `git log --follow` gives **20 commits** on the `versions/mod.rs` lineage (includes the pre-split `versions.rs`), 2 on `downloader.rs`, 1 on `bb.rs` (recent move). The cluster is WARM as claimed; mod.rs is its hottest file.

LOC honesty check (attacks the "1006 LOC Large Class" lead): `versions/mod.rs` is ~378 lines of production code + ~628 lines of `#[cfg(test)] mod tests` (lines 379–1006). **Large Class on raw LOC is NOT sustained.** What IS sustained is Divergent Change in the 378 production lines (Finding 2) — and the test bloat itself has a production-wired cause (the `#[cfg(test)]` re-export at mod.rs:7–8 exists solely so downloader's tests can live in mod.rs), so it is in scope.

---

## Finding 1 — Half-threaded value object: `AztecVersion` exists but raw `&str` still rules every seam

**Named smell:** Primitive Obsession (Fowler) — incomplete "Replace Primitive with Value Object". A typed `AztecVersion` (mod.rs:79–134) was introduced (Q3) as the validation-by-construction gate, but it survives for exactly one edge (`resolve_version` → `download_bb`) and is erased everywhere else.

**Impact:** Moderate (highest in cluster). Blast radius: all 3 cluster files plus `core/src/server/prove.rs`, `core/src/server.rs`, `src-tauri/src/tray.rs` — 6 files trade in version strings. Change frequency: WARM (20-commit lineage); every version-touching feature (pinning UI, listing endpoints, multi-version proving) re-confronts the split.

**Instances (all):**
- `versions/downloader.rs:21` — `let version: &str = version;` first line of the only typed sink deliberately erases the type; the whole downstream pipeline is stringly: `download_tarball(version: &str)` (downloader.rs:116), `verify_digest(version: &str, ...)` (downloader.rs:156).
- `versions/mod.rs:154` `version_bb_path(version: &str)`, `mod.rs:191` `download_url(version: &str)`, `mod.rs:297–300` `fetch_github_asset_digest(version: &str, ...)` — path/URL builders accept any string; nothing in the signature says "must be pre-validated".
- `versions/mod.rs:356–367` — `cleanup_old_versions(bundled_version: &str)` re-parses the bundled version into `AztecVersion` on every call (it was a typed value upstream and got flattened).
- `versions/mod.rs:260–274` — `list_cached_versions() -> Vec<String>` returns raw strings; `cleanup_old_versions` re-parses them (mod.rs:364–367), and the two other consumers each re-implement bundled-exclusion filtering on the raw strings (`core/src/server.rs:276–281`, `src-tauri/src/tray.rs:56–64` — a small Duplicate Code satellite of this finding).
- `versions/mod.rs:239,242–244` — `versions_to_evict` compares `v.as_str() != bundled_version.as_str()` although `AztecVersion` derives `PartialEq`; even inside the typed function the code drops to strings.
- `bb.rs:18` `find_bb(version: Option<&str>)` and `bb.rs:75–78` `prove(..., version: Option<&str>, ...)` — the proving entry points take untyped versions and build a cache path from the raw string (bb.rs:28–33).
- `core/src/server/prove.rs:38–41` — the smoking gun: `ResolvedVersion { version: Option<&'a str>, to_download: Option<AztecVersion> }` carries the SAME logical value in two representations because `bb::prove` wants `&str` while `download_bb` wants `&AztecVersion`; prove.rs:87 degrades back to the raw input borrow `v.as_str()`.
- `core/src/server.rs:37` — `DEFAULT_BB_VERSION: &str = "unknown"`: a magic sentinel string standing in for `Option::None`, with the collapse pattern `.as_deref().unwrap_or(DEFAULT_BB_VERSION)` repeated at server.rs:274, prove.rs:77, prove.rs:170–171. The sentinel then flows into version channels: `cleanup_old_versions("unknown")` parses it as a legitimate Mainnet version.

**Why future change gets harder:** the "validation by construction" guarantee documented at `AztecVersion::parse` actually holds on one edge only; for every other `&str` parameter a reviewer must re-derive provenance from comments ("came from resolve_version, so it's fine"). New call sites can silently bypass the gate (`version_bb_path` happily builds a path from anything). The dual-representation `ResolvedVersion` must be kept coherent by hand whenever the prove flow changes, and the lifetime `'a` tying `version` to the request string makes restructuring the handler awkward. The `"unknown"` sentinel means even the typed world can hold a non-version.

**Smallest safe refactoring:** complete *Replace Primitive with Value Object*, inward and mechanical: (1) change `version_bb_path` / `download_url` / `fetch_github_asset_digest` / `download_tarball` / `verify_digest` to take `&AztecVersion` — bodies unchanged thanks to `Deref<Target=str>`; (2) `find_bb(Option<&AztecVersion>)` and `bb::prove(..., Option<&AztecVersion>, ...)`; (3) collapse `ResolvedVersion` to a single `Option<AztecVersion>` + `needs_download: bool`; (4) parse `bundled_version` once into `Option<AztecVersion>` at `AppState` construction (*Replace Magic Literal* for the `"unknown"` sentinel).

**What disappears:** the dual-field `ResolvedVersion` and its lifetime, the re-parse inside `cleanup_old_versions`, the `let version: &str = version;` erasure plus its 6-line justifying comment, the three sentinel-unwrap sites, and the per-seam "is this string validated?" review question.

---

## Finding 2 — Divergent Change in `versions/mod.rs` root: identity + cache policy + GitHub-release plumbing in one module (with the downloader's helpers and tests stranded behind)

**Named smell:** Divergent Change (Fowler), with a Feature Envy / Misplaced Responsibility sub-pattern: a cluster of network helpers lives in mod.rs but exists exclusively to serve `downloader.rs`.

**Impact:** Moderate. Blast radius: `versions/mod.rs` (the WARM file) + `versions/downloader.rs`. Three independent maintenance axes share one file: version grammar/tier policy, cache layout/eviction, and GitHub release access.

**Instances (all):**
- Helpers in mod.rs root whose ONLY consumers are the download/digest pipeline (verified by grep — zero callers outside the versions module; the `download_url` hits in `src-tauri/src/updater.rs` are an unrelated local variable): `http_client` (mod.rs:11–18), `current_platform` (mod.rs:165–186), `download_url` (mod.rs:188–197), `sha256_hex` (mod.rs:276–280), `fetch_github_asset_digest` (mod.rs:297–334).
- `versions/downloader.rs:8–11` — the receipt: a 9-item `use super::{...}` back-import pulling those helpers in.
- `versions/mod.rs:7–8` — `#[cfg(test)] pub(crate) use downloader::{extract_bb_from_tarball, install_version_dir};` production-file plumbing whose sole purpose is letting downloader's tests live in mod.rs's test module.
- Downloader behavior tested from mod.rs's test module: `extract_bb_from_synthetic_tarball` (mod.rs:691–718), `install_version_dir_replaces_stale...` (mod.rs:723–771), `extract_bb_from_nested_tarball` (mod.rs:773–804), `extract_bb_fails_when_no_bb...` (mod.rs:806–832), `extract_bb_rejects_symlink_entry` (mod.rs:834–861), corrupted/empty-input tests (mod.rs:863–876), cleanup test (mod.rs:878–903), `download_and_verify_bb` E2E (mod.rs:949–1005) — while downloader.rs has its OWN test module too (downloader.rs:302–363). The F-04 extraction moved the code but not its tests, splitting one unit's tests across two files.
- The F-04 module doc admits the split was a deliberate stopping point (downloader.rs:6–7: "The smaller identity/platform/layout/cache concerns stay in the versions module root") — but the network/digest helpers are not "identity/layout" concerns.

**Why future change gets harder:** a GitHub API change (digest field shape, release URL), a tier-policy change, and a cache-layout change all edit the same file, inflating diff-collision odds on the hottest file in the cluster. Anyone touching tarball extraction must discover that its tests live in the *other* file's test module (and that mod.rs re-exports internals to make that work). New download-related helpers have no obviously correct home, so the back-import keeps growing.

**Smallest safe refactoring:** *Move Function* — relocate `http_client`, `current_platform`, `download_url`, `sha256_hex`, `fetch_github_asset_digest` into `downloader.rs` (or a `versions/release.rs` sibling), then move the nine downloader-behavior tests from mod.rs's test module into downloader.rs's and delete the `#[cfg(test)]` re-export. Pure relocation; no logic edits.

**What disappears:** the 9-item back-import, the cfg(test) re-export at mod.rs:7–8, ~150 production LOC + ~300 test LOC out of mod.rs (leaving a cohesive ~230-LOC "version identity + cache policy" module), and the "which file owns download behavior?" ambiguity.

---

## Finding 3 — Orphaned `download_bb` doc comment now documents `is_valid_version`

**Named smell:** Dead Code (orphaned artifact of Extract Module), manifesting as Fowler's "Comments" smell in its worst form — a lying doc comment.

**Impact:** Low bucket, but attached to the most safety-critical function in the cluster. Blast radius: anyone reading rustdoc/hover for `is_valid_version` (the function both the HTTP ingress and the download sink cite as the single source of truth).

**Instances (all):**
- `versions/mod.rs:336–339` — `/// Download the bb binary for the given Aztec version and cache it. ... /// Flow: check cache → GET tarball → verify digest → extract to temp dir → atomic rename → chmod. /// Returns the path to the cached bb binary.` sits immediately above the real doc (mod.rs:340–343) for `pub fn is_valid_version` (mod.rs:344). Rust attaches all consecutive `///` lines to the item, so the validation predicate's rendered documentation OPENS with the download pipeline's contract.
- `versions/downloader.rs:15` — the actual `download_bb` lost its rustdoc in the F-04 move; the stranded block above is it.

**Why future change gets harder:** the next person editing the validation grammar reads documentation claiming the function returns a cached binary path; IDE hover and docs.rs both render the wrong contract. Trust in the surrounding (dense) comment apparatus erodes once one block is provably stale.

**Smallest safe refactoring:** *Remove Dead Code* — delete mod.rs:336–339; optionally reattach the Flow line as `download_bb`'s rustdoc in downloader.rs.

**What disappears:** a wrong rustdoc page on the cluster's single most-cited function.

---

## Finding 4 — Release-asset naming and download cap duplicated (in-crate and cross-language)

**Named smell:** Duplicate Code (Fowler). Two flavors: a template duplicated inside the crate, and constants/URL patterns mirrored into the TypeScript build script with a comment as the only synchronization mechanism.

**Impact:** Low–Moderate. Blast radius: runtime download path (Rust) vs build-time Windows sidecar fetch (`scripts/copy-bb.ts`) — failures diverge between user machines and CI when one copy drifts.

**Instances (all):**
- In-crate: the asset-name template `barretenberg-{platform}.tar.gz` is encoded twice — inlined in the URL format string at `versions/mod.rs:191–197` (`download_url`) and rebuilt independently at `versions/downloader.rs:157` (`verify_digest`'s `format!("barretenberg-{}.tar.gz", current_platform())`). If Aztec renames the asset, the download URL and the digest lookup can drift separately (digest lookup then fails closed, but the cause is split across two files).
- Cross-language: `MAX_DOWNLOAD_BYTES = 64 * 1024 * 1024` (downloader.rs:129) mirrors `MAX_BB_TARBALL_BYTES = 64 * 1024 * 1024` (`scripts/copy-bb.ts:87`) — the Rust comment (downloader.rs:112–115) says "Mirrors copy-bb.ts" and that comment IS the sync mechanism.
- Cross-language: the GitHub release URL pattern `https://github.com/AztecProtocol/aztec-packages/releases/download/{tag}/{asset}` appears at mod.rs:191–197 and copy-bb.ts:92, with the windows asset name `barretenberg-amd64-windows.tar.gz` hardcoded at copy-bb.ts:54 duplicating mod.rs's `amd64-windows` platform arm (mod.rs:182–185).

**Why future change gets harder:** an upstream rename/move of release assets requires finding 4 sites across 2 languages with no compiler or test linking them; bumping one cap and forgetting the other yields environment-dependent size failures that look like flakes.

**Smallest safe refactoring:** *Extract Function* in-crate — `pub(crate) fn release_asset_name() -> String` used by both `download_url` and `verify_digest` (eliminates the in-crate copy outright). For the cross-language mirrors, full DRY is impractical; the smallest honest move is a pinning test (copy-bb.test.ts asserting the Rust source contains the same literal) so drift fails CI instead of production.

**What disappears:** the in-crate template duplicate; silent cross-language drift becomes a loud test failure.

---

## Finding 5 — Two home-directory resolution policies in the same crate

**Named smell:** Duplicate Code (Fowler) — the same decision ("where is home?") implemented twice with answers that have already diverged. Named analog: Inconsistent Fallback Policy.

**Impact:** Low. Blast radius: cache writes vs binary discovery; only bites in HOME-less environments (containers, stripped CI), but the dual policy is permanent reviewer overhead.

**Instances (all):**
- `versions/mod.rs:137–142` — `versions_base_dir()` uses `dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))` (fallback: current directory).
- `bb.rs:47` + `bb.rs:66–68` — `find_bb` step 3 uses `dirs::home_dir().or_else(home_dir_fallback)` (fallback: `$HOME` env var; note `dirs::home_dir()` already consults `$HOME` on Unix, so the fallback is live only in exotic Windows shells).
- The divergence is internally inconsistent within `find_bb` itself: step 1 (version cache via `version_bb_path` → `versions_base_dir`) silently uses the "." policy while step 3 (`~/.bb`) uses the `$HOME` policy.

**Why future change gets harder:** a fix to home resolution (the realistic trigger: headless/container deployments where `dirs::home_dir()` is `None`) must be discovered and applied at both sites or the cache writer and readers disagree about where home is.

**Smallest safe refactoring:** *Extract Function* — one `fn home_dir() -> Option<PathBuf>` (or `-> PathBuf` with a single chosen fallback) in the versions module, used by both call sites.

**What disappears:** `home_dir_fallback` (bb.rs:66–68) and the question of which fallback policy is the intended one.

---

## Finding 6 — Diff-relative archaeology comments assert equivalence to code that no longer exists

**Named smell:** Comments (Fowler) — specifically *temporal* comments that describe a diff against a deleted state ("byte-identical to the pre-Q11 inline block") rather than present-tense intent, plus plan-ID breadcrumbs (Q3/Q11/F-04/F-08/#99) that require external documents to decode.

**Impact:** Low. Blast radius: the whole cluster — these comments are dense in exactly the files that change.

**Instances (representative; the pattern is pervasive):**
- `versions/downloader.rs:115` and `downloader.rs:155` — "Byte-identical to the pre-Q11 inline block." (false the moment either function is edited; nothing enforces it).
- `versions/downloader.rs:16–20` — the Q3 erasure justification paragraph ("stay byte-identical to the pre-Q3 callee").
- `versions/mod.rs:250–251` — "(replaces an O(n²) `Vec::remove(0)` loop; `effective_limit` semantics unchanged)" — describes a diff, not the code.
- `versions/mod.rs:362–363` — "same net outcome as before (the old code classified it Mainnet ...)".
- Plan-ID-keyed intent throughout: mod.rs:71–78 (Q3), mod.rs:224–226 (Q3-followup), downloader.rs:2–6 (F-04), downloader.rs:29–30/39–40 (Q11), `core/src/server/prove.rs:35–36/57–59/153–155/191–192` (F-08, Q3, opus H2).

**Why future change gets harder:** each edit either silently falsifies an equivalence claim or pays a tax rewriting history paragraphs; new maintainers must resolve Q/F/# identifiers against `implementations-plan/` docs to extract the actual invariant. The genuinely load-bearing invariants (e.g., "verify digest BEFORE install", "temp-dir + atomic rename") are buried inside provenance prose.

**Smallest safe refactoring:** *Rewrite Comment as Intent* (Comments-smell remedy): keep the invariant, drop the diff ("Verify the digest before any filesystem write" stays; "byte-identical to the pre-Q11 inline block" moves to the commit message / plan doc where it already lives). No code changes.

**What disappears:** the maintenance tax on history paragraphs and the external-document dependency for reading the hot path.

---

## Non-findings (leads attacked and rejected)

- **"versions/mod.rs at 1006 LOC = Large Class"** — NOT sustained as Large Class: 62% of the file is `#[cfg(test)]` tests; production is ~378 LOC. The real, smaller problem is Divergent Change + stranded tests (Finding 2).
- **Network-tier logic as Switch Statements** — NON-FINDING. `NetworkTier` is the clean table the lead hoped for: one classification method (mod.rs:43–57), one `match` on tier in the entire production codebase (`retention_limit`, mod.rs:61–68), and eviction consumes it through `tier()`/`retention_limit()` (mod.rs:229–234). No tier prefix-sniffing or tier-match exists anywhere else (verified by grep). Adding a tier touches one enum and its two methods — exactly the Fowler-prescribed consolidation.
- **bb-resolution priority ladder duplicated** — NON-FINDING for the ladder itself. `find_bb` (bb.rs:18–64) expresses the 5-step search once, linearly, with early returns; no second copy exists in the crate. The only residue is the home-dir policy split (Finding 5).

## Out-of-scope observations

- `version_sort_key` (mod.rs:203–210) orders cross-base nightlies lexicographically ("10.0.0-nightly..." sorts before "9.0.0-nightly..."), which could mis-order eviction across a future major bump — correctness, not quality.
- `http_client()` (mod.rs:11–18) constructs a fresh `reqwest::Client` per call and silently drops all timeouts if the builder fails — robustness/perf.
- `DEFAULT_BB_VERSION = "unknown"` parses as a valid Mainnet version inside `cleanup_old_versions`, making the sentinel eviction-protected — behavior edge, noted under Finding 1 only for its typing cost.
