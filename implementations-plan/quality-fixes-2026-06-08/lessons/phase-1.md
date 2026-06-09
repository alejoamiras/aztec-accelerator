# PR-1 — F-02 CanonicalOrigin + F-01 state constructors

Branch: `quality/pr1-typed-invariants` off `main`.

## Ground truth (read before implementing)
- `canonicalize_origin(&str) -> Option<String>` already exists (`authorization.rs:21-58`) — RFC-6454, idempotent, 20+ tests. F-02 wraps it; the engine + its tests stay untouched.
- The system ALREADY canonicalizes at ingress (`auth.rs:35`) + persists canonical strings (`config.rs:96` migrate). F-02 makes the invariant **type-enforced** + closes the one bypass (headless `ALLOWED_ORIGINS`).
- `is_auto_approved` accepts `http|https` only (`authorization.rs:131-135`) — intentional; do NOT widen.
- `auth.rs` ingress: `state.config.as_ref()` is `Option` (headless no-config mode); `show_auth_popup.is_none()` = headless deny.

## Implementation decisions
- **Scope the ripple to storage + ingress** (NOT the transient pending map). `approved_origins: Vec<CanonicalOrigin>` + ingress `CanonicalOrigin::parse` are where the invariant lives + persists. `AuthorizationManager.pending` stays `HashMap<String>` / `request`/`resolve(&str)` — callers pass `origin.as_str()` (always canonical). **Deviation from the ledger's "type the pending map"**: it's transient transport, keyed by an already-canonical string; typing it adds 2 sig changes + ~5 test edits for marginal safety on a non-persisted structure. The finding's value (un-bypassable canonical storage + closed headless gap) is fully delivered. Will reconsider if codex post-impl flags it.
- `CanonicalOrigin`: manual `Serialize` (as inner str) + strict `Deserialize` (via `TryFrom<String>` → canonicalize); `Deref<str>`/`AsRef`/`Borrow<str>`/`Display`/`PartialEq<str>`. Mirrors the `AztecVersion` newtype shape.
- Config field uses lenient `#[serde(deserialize_with = "de_approved_origins")]` (drop+warn invalid, dedupe order-preserving) — replaces `migrate_approved_origins` + its load-time resave. Lossless on existing canonical data (idempotent).
- Headless `parse_allowed_origins_env`: `trim → drop empty → canonicalize non-empty → dedupe → fail-fast ONLY on invalid non-empty`. **PRESENCE semantics preserved**: env present (even ⇒ empty list) still instantiates `Some(auth)+Some(config)` = deny-all-browser (NOT `(None,None)` auto-approve).

## Log
- **F-02 ✓** (commit e7aaeb3): CanonicalOrigin newtype (Serialize/strict-Deserialize/Deref/Borrow<str>/PartialEq<str>);
  approved_origins: Vec<CanonicalOrigin> + lenient de_approved_origins (replaces migrate + load resave);
  ingress/is_approved typed; headless parse_allowed_origins_env (fail-fast invalid, present-but-empty=deny-all);
  respond_auth log-and-deny. Compiler caught 1 test site (server.rs:882). core 119 + server 5 green.
- **F-01 ✓** (commit 6ac4c14): HeadlessState::headless + AppState::headless/desktop (flat callbacks, no wrapper —
  audit-reverted DesktopCallbacks); prove_semaphore/app_version non-Option + manual Default; both binaries use ctors.
  Compiler caught 3 test sites (drop Some / use Default). core 119 + server 5 + src-tauri 19 + clippy -D warnings green.
- **Pending PR-1:** /code-review max --fix → codex post-impl → push → PR → CI.
