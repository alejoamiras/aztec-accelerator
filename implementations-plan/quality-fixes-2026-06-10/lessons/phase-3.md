# Phase 3 — PR-3 (day-scale: F-07 versions split, F-08 newtype threading)

Branch `quality/pr3-dayscale-q7e3` off main (after PR-2 #350 merged green; codex post-impl on PR-2: SHIP, 0 findings).

## F-07 ✓ (709c97a)
- Split `versions/mod.rs` (1006 LOC) → `version_policy` / `cache_layout` / `release_metadata` + an 18-line re-export hub: every external `versions::X` path unchanged (the F-12 lesson). `downloader.rs` imports per-submodule instead of the 9-item `use super`, and absorbs its 9 stranded tests (the `cfg(test)` re-export deleted). Dropped the stranded `download_bb` doc comment left by the earlier extraction.
- **Lesson:** module-level circular imports (version_policy ⇄ cache_layout) are fine within a crate. Test-count parity (exactly 133) is the no-test-lost guard for big moves.
- **Gotcha:** child test mods see the parent's private `use` imports — the moved downloader tests resolve `bb_binary_name` etc. through downloader's own imports, no test-side `use` needed.

## F-08 ✓ (ce83ca3 + 3439b78)
- **AztecVersion** through the 5 sinks; `ResolvedVersion` double-representation collapsed to `Option<AztecVersion> + needs_download`, download arm borrows `as_ref()` (the move-then-need hazard). `cleanup_old_versions(&AztecVersion)` — the defensive parse moved to the caller with identical skip semantics; **"unknown" still parses → eviction + /health byte-identical (#352 deferred)**. Sub-fn split: typed where a fn owns a sink (`download_tarball` → `download_url`), `&str` where pure logging (`finalize_downloaded_binary`, `verify_digest`).
- **CanonicalOrigin** keys the pending maps (rides the F-09 `insert`/`remove` seam — D-2 paid off: one re-key, not two), `request(&CanonicalOrigin)`, popup callback `Fn(&CanonicalOrigin, &str)` with Deref-coercion at the window boundary. `remove_approved_origin` untouched (exact-match veto).
- **Lesson:** clippy `--all-targets` catches doc-list-continuation errors `cargo test` doesn't — run it before committing, not after (one amend on the unpushed F-08a).
