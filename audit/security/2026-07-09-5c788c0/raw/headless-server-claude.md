# Headless Server — Security Audit (cluster: headless-server)

Scope: `packages/accelerator/server/src/main.rs`, traced into
`packages/accelerator/core/src/server/auth.rs` and
`packages/accelerator/core/src/authorization.rs`.

Summary of what was checked and found clean: `resolve_gating` / `parse_allowed_origins_env`
(main.rs:104-149) correctly fail-closed on every malformed `ALLOWED_ORIGINS` shape tried
(empty string, all-whitespace, bare/trailing/doubled commas, invalid entries, `*`/wildcard,
unknown schemes, userinfo, path/query/fragment) — each either normalizes to an empty allowlist
(still gated, non-localhost still denied) or hard-errors the process (`std::process::exit(1)`),
never silently widening to allow-all. `--allow-all` / `ALLOWED_ORIGINS` mutual exclusivity is
enforced and unit-tested. `AZTEC_BB_VERSION` only feeds `/health` display + a download/cleanup
comparison (`prove.rs::resolve_version`, `cleanup_old_versions`); it never reaches process
execution un-parsed and is operator/CI-controlled, not attacker-reachable under this threat
model — no finding there.

One finding below concerns the interaction between `ALLOWED_ORIGINS` and the hardcoded
localhost auto-approval, which is a distinct mechanism from the already-documented
absent-Origin residual.

---

## Finding 1: `ALLOWED_ORIGINS` cannot scope out localhost — any co-resident caller claiming a `localhost`/`127.0.0.1`/`[::1]` Origin (any port) bypasses the configured allowlist

**1. Title**: Headless `auto_approve_localhost` is hardcoded `true` for every gated mode, so `ALLOWED_ORIGINS` never restricts localhost-claiming origins — silently wider access than the operator's allowlist implies.

**2. Impact factors**:
- Confidentiality: not directly violated (the unauthorized caller submits its own witness; no third-party witness is disclosed by this path alone).
- Integrity: not directly violated.
- **Authorization: violated** — the operator's `ALLOWED_ORIGINS` allowlist is bypassed for any origin claiming to be localhost, regardless of port or of whether that exact origin is in the list.
- **Availability: violated** — `/prove` is serialized behind a single-permit semaphore (`prove_semaphore`, capacity 1, `core/src/server.rs`), so an unauthorized co-resident caller can monopolize the only proving slot, starving the legitimately-approved origin's CI job.
- Blast radius: all co-resident local users/processes/containers sharing the loopback network namespace with the accelerator process — i.e., exactly the "shared/self-hosted runner" / "multi-tenant host" scenario the project's own README explicitly calls out as unsafe (packages/accelerator/README.md:121,129,131), but for a broader mechanism than the one that warning names.
- Data sensitivity: proving compute + CI throughput, not witness confidentiality.
- Attack vector: local (loopback-only; no network path exists past the Host-allowlist).
- Attack complexity: low (one crafted `Origin` header value, or simply being a second legitimate localhost-hosted page on a different port).
- Privileges required: none beyond loopback network reachability to the accelerator's port — inherent on any host with more than one local tenant/process.
- User interaction: none.

**3. Evidence confidence**: high — traced end-to-end with line numbers; behavior confirmed by reading the exact `is_approved`/`is_auto_approved` logic and its unconditional wiring from `main.rs`.

**4. OWASP / CWE**: OWASP A01:2021 – Broken Access Control. CWE-346 (Origin Validation Error), secondarily CWE-863 (Incorrect Authorization).

**5. Trace** (source → sink):
- `packages/accelerator/server/src/main.rs:69-85` (`resolve_gating` → `Gating::Gated(origins)` arm) builds `AcceleratorConfig` for **every** gated invocation — whether `origins` (from `ALLOWED_ORIGINS`) is empty or populated — with `auto_approve_localhost: true` hardcoded at **main.rs:78**. There is no env var / CLI flag in headless mode that can set this to `false`.
- The accompanying comment at **main.rs:76-77** — "operators scope localhost via `ALLOWED_ORIGINS`" — is not accurate: no code path lets `ALLOWED_ORIGINS` content narrow or exclude the localhost carve-out.
- That config flows into `HeadlessState`/`AppState` (`core/src/server.rs:101,144-158`) and is read per-request in `authorize_origin` at **`core/src/server/auth.rs:44-51`**: `AuthorizationManager::is_approved(&origin, &cfg.approved_origins, cfg.auto_approve_localhost)`.
- `is_approved` at **`core/src/authorization.rs:291-298`**, specifically the OR at **lines 296-297**:
  ```rust
  (auto_approve_localhost && Self::is_auto_approved(origin))
      || approved_origins.iter().any(|o| o == origin)
  ```
  short-circuits `true` whenever `is_auto_approved(origin)` is true, **independent of `approved_origins`**.
- `is_auto_approved` at **`core/src/authorization.rs:271-281`** matches on **host only** (`"localhost" | "127.0.0.1" | "[::1]"`, lines 279-280) — it does not check port at all, so `http://localhost:1` through `http://localhost:65535` all match equally.
- Sink: `authorize_origin` returns `Ok(())` (**`auth.rs:53-55`**), admitting the request into `prove()` (`core/src/server/prove.rs:101`), which consumes the singleton `prove_semaphore` and runs `bb` on the caller-supplied witness.

**6. Missing control**: no way, in headless mode, to disable/scope the localhost auto-approve independent of `ALLOWED_ORIGINS`'s content; `is_auto_approved` has no port check, so even a port-qualified `ALLOWED_ORIGINS` entry (e.g. the README's own `http://localhost:5173`) provides no actual narrowing for localhost — it's already redundant with the default.

**7. Exploit/violation scenario**:
1. Operator follows the project's own GitHub Actions example (README.md:161-164) and starts the headless server on a CI runner with `ALLOWED_ORIGINS: http://localhost:5173`, believing this scopes `/prove` access to exactly their frontend dev server on port 5173.
2. On a shared/self-hosted runner (or any host where another local process/tenant/container shares the loopback namespace — the exact scenario the README's own "not for shared or self-hosted CI runners" warning anticipates), a co-resident process issues `POST http://127.0.0.1:59833/prove` with header `Origin: http://localhost:9999` (any port other than 5173), or a raw client omits the port entirely (`http://localhost`).
3. `is_auto_approved` matches on host only, ignoring port; `is_approved`'s OR short-circuits true regardless of the operator's `approved_origins` list. The request is authorized.
4. The unauthorized caller now occupies the single proving-concurrency slot with its own witness/job, either denying the legitimate CI job proving throughput (availability) or simply consuming compute it was never granted access to (authorization violation) — while the operator's `ALLOWED_ORIGINS=http://localhost:5173` line gave them no actual protection against this, since it was already implied by the (undocumented-as-total) localhost default.

**8. Preconditions**: headless server running in any `Gating::Gated` mode (default-empty or `ALLOWED_ORIGINS` populated — `--allow-all` mode is out of scope, it has no gating by design); attacker/co-resident caller has loopback network reachability to the accelerator's port (true on any multi-tenant host, container-shared network namespace, or self-hosted/shared CI runner); attacker can set an arbitrary `Origin` header (any non-browser client) or is itself a legitimately browser-hosted page on a different localhost port than the operator intended.

**9. Why existing mitigations fail**: The loopback `Host`-header guard (`core/src/server/host.rs`) only constrains the **destination** the caller dials (must be exactly `127.0.0.1`/`localhost`/`[::1]` on the listener's port) — it cannot and does not distinguish which co-resident caller is connecting, so it provides no defense here. The `ALLOWED_ORIGINS` deny-by-default parsing (SEC-01c) itself is sound — every malformed/empty shape fails closed or errors, confirmed by the existing test suite in main.rs — so this is not a parsing bug. The gap is structural: `is_approved`'s allowlist check is additive-only (an OR), and headless's env-var surface offers no way to turn `auto_approve_localhost` off, so populating `ALLOWED_ORIGINS` can only ever **add** trusted non-localhost origins, never **narrow** the always-on localhost default. This is a different mechanism from the two residuals this project already documents and the audit brief excludes: (a) the absent-Origin bypass for non-browser callers (`auth.rs:24-34`, explicitly called out in README.md:129 and in the cluster's known-residuals list) concerns a caller sending **no** Origin header; this finding concerns a caller that **does** send an Origin header, one that merely claims (truthfully as an unrelated localhost dev server, or falsely as a spoofed raw-HTTP client) to be on `localhost`/`127.0.0.1`/`[::1]`, which the allowlist cannot exclude no matter its content. (b) The opt-in nature of `--allow-all`/`ALLOWED_ORIGINS` (also pre-excluded) is orthogonal — this finding is about what happens *after* an operator has already opted into the more restrictive `ALLOWED_ORIGINS` allowlist path and reasonably expects it to be authoritative for the origins it lists.

**10. Instances** (same root cause):
- `packages/accelerator/server/src/main.rs:78` — `auto_approve_localhost: true` hardcoded, unconditional across both empty and populated `Gating::Gated` allowlists.
- `packages/accelerator/server/src/main.rs:76-77` — comment claiming operators can "scope localhost via `ALLOWED_ORIGINS`", which no code path implements.
- `packages/accelerator/core/src/authorization.rs:296-297` — `is_approved`'s OR short-circuit bypasses `approved_origins` whenever `is_auto_approved(origin)` is true.
- `packages/accelerator/core/src/authorization.rs:271-281` — `is_auto_approved` checks host only, no port, so a port-qualified `ALLOWED_ORIGINS` entry for localhost provides no actual scoping.
- `packages/accelerator/core/src/server/auth.rs:44-55` — `authorize_origin` reads `cfg.auto_approve_localhost` and admits the request on `Ok(())`.
- `packages/accelerator/README.md:161-164,173` — operator-facing docs/example (`ALLOWED_ORIGINS: http://localhost:5173`) imply per-origin/per-port scoping for localhost that the implementation does not provide.

**Suggested direction (not scored/severity-assigned per instructions)**: expose an explicit headless knob (e.g. `ACCEL_NO_LOCALHOST_AUTO_APPROVE=1`, or simply stop forcing `auto_approve_localhost: true` when `ALLOWED_ORIGINS` is explicitly set to a non-empty value) so an operator who deliberately curates an allowlist can make it authoritative; and/or correct the `main.rs:77` comment and the README to state plainly that localhost/127.0.0.1/[::1] on **any port** are always implicitly approved in headless mode regardless of `ALLOWED_ORIGINS` content.
