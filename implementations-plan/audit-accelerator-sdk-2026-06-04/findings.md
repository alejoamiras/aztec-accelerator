# Full-depth audit — accelerator + SDK (2026-06-04)

Scope: accelerator (Rust+TS+Tauri) + SDK, full depth (owner choice). Bugs + simplification.
Repo had thorough March audits → focus net-new. Fixes via branch+PR (main protected).

## SDK — packages/sdk/src/lib/accelerator-prover.ts (419 LOC, published API)
- **[BUG med] setAcceleratorConfig stale cache** (L166-171): resets `#acceleratorProtocol` but NOT
  `#statusCache`; for ≤10s after a consumer changes port/host, `checkAcceleratorStatus()` returns the
  OLD endpoint's cached result. Fix: `this.#statusCache = null` in setAcceleratorConfig.
- **[BUG low-med] native path skips "proved" phase** (L399-403): emits "proved" only if the server
  sent `x-prove-duration-ms`. force-local (L323) + fallback (L340) + denied (L392) always emit it. If
  the header is absent the UI hangs on "proving"→"receive". Fix: time ky.post client-side, emit "proved"
  with the server duration when present else the client-measured one.
- **[BUG low, latent] lazy-simulator Proxy is universally thenable** (L90-98): `get` returns an async
  fn for EVERY prop incl. `then`/`catch`/symbols → `await proxy` / promise-probing misbehaves; symbol
  access (Symbol.iterator/toPrimitive) returns a function. Fix: in `get`, return undefined for `then`
  and `typeof prop==='symbol'` (or Reflect.get on a real method allowlist).
- **[SIMPLIFY] 3× duplicated local-prove block** (L316-324, L332-343, L385-394): identical
  onPhase("proving")→timed super.createChonkProof→onPhase("proved",{durationMs})→log. Extract
  `#proveLocally(steps, label)` → one copy. (createChonkProof drops ~95→~55 LOC.)
- [nit] `#getAztecVersion` (L411-418): `replace(/^[^0-9]*/,"")` mangles ranges (">=1.2 <2" keeps the
  tail) + returns "" for "workspace:*". Fine for a published pinned dep; brittle otherwise.
- [nit] No shape validation on `/prove` response (L404-407): `response.proof` undefined →
  Buffer.from(undefined) throws. Localhost+trusted, low risk.

## Accelerator Rust — (codex pass pending: authorization.rs, certs.rs, updater.rs)

## Accelerator Rust — (inline pass pending: server.rs, config.rs, bb.rs, crash_recovery.rs, ...)

## SDK — STATUS: FIXED (commit c832298, on branch fix/sdk-prover-audit, push blocked on 1Password)
All 4 SDK items above applied + 2 regression tests (25 pass, tsc+biome clean).

## Accelerator Rust — codex security-trio audit (authorization.rs, certs.rs, updater.rs)
- **[HIGH] certs.rs:~71 — local CA installed as a system trust root.** Safari-HTTPS generates a full
  root CA (IsCa::Unconstrained) + installs it (`add-trusted-cert -r trustRoot`); `ca.key` is plaintext
  0o600 on disk. A same-user process reading ca.key can mint browser-trusted certs for ANY domain
  (name_constraints aren't reliably enforced on trust anchors). NEEDS A DECISION: redesign to a
  self-signed LEAF (no CA-minting primitive) + Safari re-test, OR accept-with-mitigations. Not a quick
  fix — own effort/Safari testing.
- **[MED] certs.rs:~47 — certs_exist() checks presence not validity + non-atomic write.** A partial
  write leaves corrupt-but-present PEMs → generation skipped forever → HTTPS broken until manual delete.
  Fix: write atomically (temp+rename) + validate (parse) on the exist-check.
- **[MED] certs.rs:~131 — mode(0o600) only on NEW files.** Pre-existing key files with loose perms
  aren't tightened. Fix: set_permissions(0o600) on the keys unconditionally after write.
- **[MED] updater.rs:~125 — Windows re-arm is best-effort + masks failures.** `is_enabled().unwrap_or(false)`
  hides query errors; enable_crash_recovery has no Result. Updater can disarm→install→restart and
  silently leave recovery OFF. (Mitigated by #97's smoke, which asserts re-arm — but defense-in-depth:
  return/propagate failure.)
- **[LOW] authorization.rs:~128 — is_auto_approved hand-parses origins** (strip_prefix/split(':'))
  instead of reusing canonicalize_origin/Url. No exploitable bypass seen, but duplicate parser in the
  trust boundary → collapse onto the canonicalizer.
- **[LOW] certs.rs:~91 — leaf-cert policy duplicated** in generation + renewal → extract one helper.
- **[LOW/by-design] server.rs:312 — no-Origin auto-approve** — documented; localhost service isn't an
  ACL against non-browser local clients. No change.
- codex CONFIRMED clean: authorization.rs core comparison (exact canonical Url compare, rejects
  null/file:/unknown-scheme/path/query/userinfo, no TOCTOU/panic); updater.rs signature+version gating.

## Accelerator — inline (server.rs, copy-bb.ts)
- server.rs: high quality. unwraps are on json! (infallible) + numeric headers (ASCII). prove handler:
  authorize-before-buffering, 50MB body cap, single-prove semaphore, validated version header. [nit] an
  APPROVED origin can request arbitrary valid versions → bb downloads (disk/bw); gated by approval+cleanup.
- copy-bb.ts: supply-chain hardened (pinned SHA-256 fail-closed, length caps pre-hash, bsdtar abs-path
  via execFileSync no-shell, single-entry canary). [nit] macOS xattr uses execSync+interpolation (dest
  is controlled → not exploitable; execFileSync cleaner).

## Remaining (not yet audited): config.rs, bb.rs, crash_recovery.rs, versions.rs, main.rs, commands.rs,
## verified_sites.rs, tray.rs, windows.rs, + accelerator TS (settings UI).

## Accelerator Rust — subagent sweep (config/commands/verified-sites/bb + versions/main/tray/windows)
### NET-NEW (worth fixing)
- **[HIGH] versions.rs:262-342 download_bb OOM** — buffers the full response via `response.bytes()`
  BEFORE the digest check; the 500MB guard checks only advertised Content-Length (skipped when absent).
  A malicious/compromised CDN omits Content-Length → streams GBs → OOM. Triggerable by an approved
  origin's `x-aztec-version` → download. Fix: stream + running byte counter, abort past MAX.
- **[MED] is_valid_version permits `..`** (server.rs:282; versions.rs) — `version_bb_path("..")` →
  `versions/..`; download_bb's `remove_dir_all(version_dir)` could target `~/.aztec-accelerator/`
  itself (nuke certs/config/cache). Currently UNREACHABLE (fail-closed digest rejects `..` before the
  destructive block) but one refactor from data-loss. Fix: reject `.`/`..`/empty components at ingress.
- **[LOW] bb.rs:133 `&stderr[..500]`** can panic on a non-UTF-8 char boundary → use char-safe truncation.
### LOW/INFO + clean
- versions.rs supply chain: GitHub `digest` over TLS is the only anchor (honest TODO; fail-closed
  correct; macOS ad-hoc codesign adds no authenticity) — acceptable until upstream signs.
- version_sort_key/eviction edge cases (LOW); tmp_dir TOCTOU (harmless via prove_semaphore).
- CLEAN: verified_sites (no ✓ spoofing — canonicalize + reject non-ASCII pre-punycode), bb spawn (not
  injectable, no shell), config migration, main.rs (_guard + AddrInUse classification), tray.rs, windows.rs.
### SIMPLIFY
- versions.rs:201 + commands.rs:124 hand-rolled hex → `hex::encode`. versions_to_evict effective_limit
  over-complex + Vec::remove(0) O(n²). bb.rs dirs_next dead indirection. config_version dead state.
  main.rs open_in_browser → tauri-plugin-opener.

## CA redesign (certs.rs HIGH) → deep plan in progress: implementations-plan/safari-tls-ca-removal-2026-06-04
