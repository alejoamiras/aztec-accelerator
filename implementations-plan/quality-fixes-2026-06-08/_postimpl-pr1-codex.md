Post-implementation ADVERSARIAL review of PR-1 (2 of 9 quality-refactor findings). cwd = repo root.

Diff: `git diff main...quality/pr1-typed-invariants` — two commits: F-02 (CanonicalOrigin newtype + close headless ALLOWED_ORIGINS gap) and F-01 (typed state constructors). Plan + folded audits: `implementations-plan/quality-fixes-2026-06-08/plan.md` (§F-01, §F-02, ## Audit revisions). These were plan-time dual-audited; this reviews the IMPLEMENTATION. Read the real source.

Find what's wrong (or confirm clean):
- **F-02 invariant:** does `CanonicalOrigin` (core/src/authorization.rs) fully enforce canonicality? Any path that builds/stores a non-canonical one? Is the strict newtype `Deserialize` + the lenient `de_approved_origins` (config.rs — drop-invalid, dedupe, order-preserving) correct + lossless on already-canonical configs? Does deleting `migrate_approved_origins` strand any data or drop a needed resave?
- **Presence semantics (security-critical):** `parse_allowed_origins_env` (server/src/main.rs) — confirm a present-but-empty `ALLOWED_ORIGINS` ("", whitespace, ",,") still yields `Some(auth)+Some(config)` = deny-all (the binary keys on `Ok(var)` presence, NOT on the parsed list being non-empty). A regression here would silently auto-approve all browser origins.
- **Scope decision to scrutinize:** I typed STORAGE (`approved_origins: Vec<CanonicalOrigin>`) + INGRESS (`CanonicalOrigin::parse`), but left `AuthorizationManager.pending` keyed by `String` (request/resolve take `&str`), callers pass `origin.as_str()`. Is that a real gap, or fine (keys are always canonical strings from a CanonicalOrigin)?
- **`respond_auth`** (src-tauri/commands.rs): log-and-deny on non-canonical origin — correct? Can a malformed origin still be honored as Allow?
- **F-01:** making `prove_semaphore`/`app_version` non-`Option` — any runtime path that depended on `None`? Is the manual `Default` correct (semaphore + core's version)? Do `HeadlessState::headless` + `AppState::desktop` wire every field? Is `config`/`auth_manager` correctly kept `Option`?
- **Any behavior change** beyond F-02's intended gap-closure?

Lead with a one-line verdict (`clean` / `issues: …`). Then findings by severity with file:line + concrete fix. ~400–700 words.
