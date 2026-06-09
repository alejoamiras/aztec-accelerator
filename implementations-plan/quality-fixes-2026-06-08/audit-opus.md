# Opus audit (combined contradiction-check + adversarial) — condensed

**Verdict: `issues: 2 High, 4 Med, 3 Low` — plan fundamentally sound + well-grounded; F-02 ripple under-scoped, F-08 "identical" claim wrong on the download arm, DesktopCallbacks should revert.**

## High
- **H1 — F-02 ripple under-scoped.** `auth.rs:114-115` `approved_origins.contains(&origin)`/`.push(origin)` use `origin: String`; `is_approved(&str, &[String])` (authorization.rs:143) → must be `&[CanonicalOrigin]`; `remove_approved_origin` retain compares `CanonicalOrigin != String`. **Fix:** enumerate the ~5 extra auth.rs sites; give `CanonicalOrigin` `PartialEq<str>` + `Borrow<str>` so contains/retain/compare-vs-`&str` stay ergonomic; `authorize_origin` builds a `CanonicalOrigin` not a `String`.
- **H2 — F-08 "net sequence identical" is FALSE on the download arm.** Today the download arm emits `[Proving, Downloading, Proving, Idle]` — with a **redundant leading `Proving`** (prove.rs:161 then resolve emits Downloading@67→Proving@95). If the refactor emits `Proving` once *after* resolve (the tempting cleanup), the leading Proving vanishes → behavior change on an uncovered path. **Fix:** emit the initial `Proving` BEFORE the `needs_download` check (preserve the redundant one); the new download-arm test asserts the full **4-element** `[Proving, Downloading, Proving, Idle]`. (Or consciously drop it + document as tests-only change — but default = preserve.)

## Med
- **M1 — F-09 stale citations.** `start_https` is NOT inline — it's in `tls.rs:15` (re-exported `server.rs:9`). The dup is the **spawn+error-log wrapper** (`main.rs:85-89` ≡ `commands.rs:160-164`). Plan's `spawn_https` targets it correctly; just fix the line citations. The divergent TLS-load-failure policy stays upstream of the spawn (auto-satisfies "do not unify").
- **M2 — env path dedupe.** `parse_allowed_origins_env` should **dedupe order-preserving** (today's `server/main.rs` doesn't; the deleted migrate did) so env + persisted ingress produce identically-shaped Vecs.
- **M3 — migrate-deletion resave.** Confirmed safe (only consumer is `load→save`, nothing external reads config.json). But the match works because **both sides canonicalize in-memory**, not because disk is rewritten. **Fix:** add a test asserting a non-canonical on-disk entry round-trips to a canonical in-memory `CanonicalOrigin` — closes Ask 1 with evidence.
- **M4 — DesktopCallbacks → REVERT to flat.** The 3 callbacks are read in **core** (prove.rs:160, auth.rs:59/83). A "Desktop"-named struct field-accessed in GUI-agnostic core **violates the core-extraction boundary** and buys nothing (`AppState::desktop(core, on_status, on_versions_changed, show_auth_popup)` works with 3 args, no wrapper). **Lean flat** — safer + keeps core GUI-ignorant.

## Low
- **L1 — explicit-literal test edits.** `prove_success_path_and_status_sequence` (server.rs:641-650) sets `prove_semaphore: Some(...)` explicitly → after non-`Option` it needs a hand edit (compiler-caught), not just the `Default` sites.
- **L2 — confirms** `ALLOWED_ORIGINS` not set in CI (codex right). Independent re-grep agrees.
- **L3 — F-05/F-06 non-breaking VERIFIED** against `dist/index.d.ts` + `src/index.ts` (exact export set; `AcceleratorProtocol` not currently exported → additive). `package.json exports: "./src/index.ts"` ships TS source → `tsc --noEmit` is the right gate.
