Cluster C3 verdict: 2 findings, one architectural and one structural; the maintainability cost is concentrated in origin approval being modeled as raw `String`s and coordinated by cross-module call order.

## Finding 1 — Origin Identity Lives in Raw Strings
1. **Title** — Origin identity lives in raw strings.
2. **Smell** — Primitive Obsession.
3. **Maintenance impact** — Structural; blast radius is 3 modules (`config`, `authorization`, auth ingress), and this sits on the hot path for every `/prove` authorization plus every config load that touches `approved_origins`.
4. **Concrete evidence** — `approved_origins` is persisted as `Vec<String>` in `packages/accelerator/core/src/config.rs:49` and migrated via `&mut Vec<String>` in `packages/accelerator/core/src/config.rs:107`; `canonicalize_origin` still returns `Option<String>` in `packages/accelerator/core/src/authorization.rs:21`; pending requests are keyed by `String` in `packages/accelerator/core/src/authorization.rs:78`; `request`, `resolve`, `is_auto_approved`, and `is_approved` all traffic in `&str`/`[String]` in `packages/accelerator/core/src/authorization.rs:101`, `:116`, `:126`, and `:143`; the “already canonical” invariant exists only in comments at `packages/accelerator/core/src/authorization.rs:140-142`.
5. **Why it harms future change** — If origin handling gains another normalization rule, metadata, or a distinction between raw-header origin and approved canonical origin, maintainers must audit every string slot and equality check because the type system cannot distinguish those roles.
6. **Smallest safe refactoring** — Replace Primitive with Object: introduce a `CanonicalOrigin`/`ApprovedOrigin` value object with constructor-based canonicalization and serde support.
7. **What disappears** — Comment-enforced origin invariants, ad hoc string equality semantics, and the need to remember which `String` values are raw versus canonical.
8. **Instances** — `packages/accelerator/core/src/config.rs:49`, `:107`; `packages/accelerator/core/src/authorization.rs:21`, `:78`, `:101`, `:116`, `:126`, `:143`.

## Finding 2 — Approved-Origin Lifecycle Is Split Across Modules
1. **Title** — Approved-origin lifecycle is split across modules.
2. **Smell** — Inappropriate Intimacy.
3. **Maintenance impact** — Architectural; blast radius is 3 modules, because canonicalization, approval, mutation, and persistence are owned in different places and must evolve together.
4. **Concrete evidence** — `config::load` explicitly knows it must canonicalize persisted origins with auth logic and rewrite the file (`packages/accelerator/core/src/config.rs:83-99`, `:107-129`); `authorization::is_approved` explicitly assumes both request input and persisted entries were canonicalized elsewhere (`packages/accelerator/core/src/authorization.rs:138-145`); the ingress path has to remember the sequence itself by canonicalizing first, then checking approval, then mutating `cfg.approved_origins`, then calling `config::save` (`packages/accelerator/core/src/server/auth.rs:35-52`, `:111-118`).
5. **Why it harms future change** — A change such as “store approval timestamp/source”, “change canonicalization policy”, or “move approvals out of JSON” is not localized; it requires coordinated edits to load-time migration, runtime approval checks, and remember-on-allow persistence, with no single owner of the invariant.
6. **Smallest safe refactoring** — Extract Class: introduce an `ApprovedOrigins` abstraction that owns migration, membership checks, insertion, and serialization, then move the current free functions behind that API.
7. **What disappears** — Cross-module reach-through, call-order knowledge baked into comments, and direct manipulation of config’s internal `Vec<String>` from auth code.
8. **Instances** — `packages/accelerator/core/src/config.rs:83-99`, `:107-129`; `packages/accelerator/core/src/authorization.rs:138-145`; `packages/accelerator/core/src/server/auth.rs:35-52`, `:111-118`.