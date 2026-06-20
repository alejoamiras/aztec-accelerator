# Re-audit: supply-chain + TLS + headless trust boundary

**Scope:** SEC-07 (decompression cap), SEC-03 (updater pre-flight size cap), SEC-08 (cert fail-closed), SEC-01b/c (headless deny-by-default). Known deferrals SEC-02, M2.
**Method:** read production source + the relevant `tauri-plugin-updater` 2.10.1 vendored source. Confirmed all 5 PRs landed in committed history (`5c57d39`..`d90cbda`); cluster files have no uncommitted changes (`git status --porcelain` clean for the audited set).

## Verdict: CLOSED-CLEAN

No regression, no new concrete vuln in the audited cluster. The 4 "verify CLOSED" items are genuinely closed with the controls described; both known deferrals (SEC-02, M2) are still open and accurately documented in-code. Zero findings at Critical/High/Medium/Low.

---

## Findings

None.

I attacked each closed item for a bypass and found none reachable; details under "Confirmed closed". Two non-findings worth recording so they are not re-raised later:

- **SEC-07 writes up to `cap` (512 MB) to disk before the counter trips — by design, not a finding.** `entry.unpack` streams through the `CappedReader`, which permits exactly `cap` bytes total before erroring, and `unpack` writes incrementally — so a bomb can land ≤512 MB on disk in the temp dir before abort. That is the *intended* disk ceiling (the cap is a disk-fill bound, not a memory bound), the temp dir is removed on the next `install_version_dir` call, and the compressed input is already ≤64 MB (`MAX_DOWNLOAD_BYTES`, downloader.rs:129) so the achievable inflation is bounded at the source. Not a memory-DoS, not unbounded. No action.
- **M2 (updater `None`-size arm) is a real, currently-open availability gap — but it is the declared deferral, so it is NOT counted as a new finding.** See "Deferrals still open" for why it is genuinely exploitable today and the in-code rationale is honest about it.

---

## Confirmed closed (attacked, no bypass)

### SEC-07 — bb decompression cap (CLOSED)
- Running-counter backstop: `CappedReader::read` at `downloader.rs:232-245` — `self.read = self.read.saturating_add(n); if self.read > self.cap { return Err(InvalidData) }`. The counter sits **under** `GzDecoder` (`downloader.rs:266-270`), so every *decompressed* byte is counted, including bytes consumed while `tar::Archive` skips a junk entry positioned before `bb`.
- Lying-header attack defeated: the per-entry `header().size()` pre-check (`downloader.rs:288-293`, `if declared > cap`) is attacker-controlled and explicitly documented as "necessary but not sufficient" (`downloader.rs:224`, `:287`). A header that under-declares (e.g. 16 bytes) then over-streams 600 MB trips the running counter at byte `cap+1` during `entry.unpack` (`downloader.rs:294`). Pinned by `capped_reader_trips_on_cumulative_decompressed_bytes` (`downloader.rs:351-362`), which places a 2 MB junk entry *before* `bb` against a 1 MB cap and asserts the abort comes from the counter, not the declared size.
- Boundary: `> cap` (not `>=`) — a legit binary of exactly `cap` bytes passes; `cap+1` fails. The per-entry check matches (`declared > cap`). No off-by-one that admits a bomb.
- No "extract before the cap applies" path: the only writer is `entry.unpack`, which reads exclusively through the wrapped `CappedReader`; there is no pre-cap read of entry data.
- `saturating_add` (`downloader.rs:234`) prevents a `u64` wrap from resetting the counter below the cap.

### SEC-03 — updater pre-flight size cap (CLOSED, for the `Some(size)` arm)
- The cap is enforced **before** `update.download()`: the `match advertised_update_size(&update)` block (`updater.rs:106-119`) returns early on `Some(size) if size > MAX_UPDATE_BYTES` (`updater.rs:107`, `MAX_UPDATE_BYTES = 500 MB` at `:62`); `update.download(...)` is only reached afterward at `updater.rs:124`. Truly pre-download.
- The premise is verified against the vendored plugin: `tauri-plugin-updater-2.10.1/src/updater.rs:702-712` — `let mut buffer = Vec::new();` then an **unbounded** `while let Some(chunk) = stream.next() { on_chunk(...); buffer.extend(chunk); }` with **no size check**, and `verify_signature(&buffer, ...)` runs only at line 712, **after** the whole artifact is buffered. The `on_chunk` callback (line 707) returns `()` and cannot abort the loop. So the signature is NOT an availability control (buffering precedes verification) — the pre-flight `size` cap is the sole memory-DoS defense, exactly as the comment claims. `update.raw_json` / `update.download_url` are confirmed public fields (`updater.rs:620`, `:624`).
- Boundary: `size > MAX_UPDATE_BYTES` rejects; `size == MAX` proceeds. 500 MB exactly is allowed — intentional headroom, real artifacts are tens of MB. No off-by-one risk.
- No bypass call site: the only `update.download`/`update.install` calls are inside `perform_update` (`updater.rs:124`, `:160`), after the cap match. Both `perform_update` callers (`commands.rs:250` via `respond_update_prompt`, and `updater.rs:49` via `check_for_update` auto-update) route through that single body. No path reaches `download()` without the pre-flight.
- `size_from_feed` matches by URL within `platforms.*` (`updater.rs:67-74`), so the size compared is this platform's artifact, not a sibling's. Pinned by `size_from_feed_matches_url`.

### SEC-08 — cert fail-closed (CLOSED, both HTTPS-start paths)
- `migrate_legacy_ca_key_at` (`certs.rs:189-217`) returns `Result`: removes the legacy `ca.key`, retries once on transient failure (`certs.rs:195-203`), then **re-checks `ca_key.exists()` and returns `Err`** if it persists (`certs.rs:205-211`). Fail-closed. Pinned by `migrate_fails_closed_when_key_cannot_be_removed` (`certs.rs:611-631`, parent-dir `0o500` makes `remove_file` fail → asserts `Err`).
- Startup path: `main.rs:424-428` — `match certs::migrate_legacy_ca_key() { Ok(()) => try_start_https(&state), Err(e) => tracing::error!(... "Safari HTTPS NOT started ...") }`. On `Err`, `try_start_https` is never called → no HTTPS bring-up. HTTP (`spawn_http_server`, `main.rs:451`) is independent and unaffected.
- Settings toggle path (the second HTTPS-start site, post-impl fix `d90cbda`): `enable_safari_support` (`commands.rs:151-184`) calls `migrate_legacy_ca_key().map_err(...)?` at `commands.rs:162-164` **before** `generate_and_save`, `install_ca_trust`, config save, and `spawn_https` (`commands.rs:180`). The `?` short-circuits on `Err`, so a toggle on an upgraded install with an unremovable legacy key refuses to enable HTTPS and surfaces the error to the Settings UI. No HTTPS-start path bypasses the gate.
- (Note for the other agent: `enable_safari_support` at `commands.rs:151` is the toggle site referenced as their scope — the migration gate there is correct.)

### SEC-01b/c — headless deny-by-default (CLOSED)
- `resolve_gating(allow_all, allowed_origins)` (`server/main.rs:140-149`) — exhaustive over the 2×2 input space, no fail-open arm:
  - `(true, Some)` → `Err` (mutually exclusive, fails loud; `main.rs:57-61` exits 1).
  - `(true, None)` → `AllowAll` — the **only** no-gating mode, gated behind an explicit operator opt-in (`--allow-all` / `ACCEL_ALLOW_ALL`), loudly `warn!`-logged (`main.rs:63-67`).
  - `(false, Some(raw))` → `Gated(parse_allowed_origins_env(raw)?)`.
  - `(false, None)` → `Gated(Vec::new())` — **empty allowlist that DENIES, not allows.** This is the SEC-01 fix.
- Empty allowlist does not silently allow all: an empty `Gated([])` builds a real `AuthorizationManager` + config with `approved_origins: []` (`main.rs:74-85`). At ingress, `authorize_origin` (`server/auth.rs:51-74`) calls `is_approved(origin, [], auto_approve_localhost=true)`; for a non-localhost origin `is_approved` returns `false` (`authorization.rs:274-276`), there is no popup callback in headless, so `auth.rs:65-74` returns `403 origin_denied`. Pinned by `gating_default_is_deny_by_default_empty_allowlist` and `gating_present_but_empty_allowlist_still_gates`.
- `auto_approve_localhost: true` in headless is intentional and scoped: set only inside the `Gated` arm (`main.rs:78`) with the rationale that headless has no approval popup; it gates **only** loopback literals. `is_auto_approved` (`authorization.rs:249-259`) accepts solely `localhost` / `127.0.0.1` / `[::1]` over http/https via `url::Url` host parsing — `https://evil.localhost.com` is rejected (`non_localhost_not_auto_approved`, `authorization.rs:298-309`). Operators scope non-localhost via `ALLOWED_ORIGINS`. Defense-in-depth: every request is *also* constrained to a loopback `Host`/`:authority` by `host::guard` (SEC-01a, `server/host.rs:50-74`) before the Origin gate runs, and that gate rejects DNS-rebinding hosts, userinfo smuggling, and IPv4-mapped/alternate-numeric forms (`host.rs` tests). A no-Origin request auto-approves (`auth.rs:34`) but is an inherent localhost-service property, not a regression, and still must pass the loopback-Host gate.

---

## Deferrals still open (confirmed, NOT re-flagged as new)

- **SEC-02 (#343)** — `fetch_github_asset_digest` (`versions/mod.rs:297-334`) fetches the digest from `api.github.com` (`mod.rs:301-303`), the same control plane that serves the binary — circular trust. The in-code `// SECURITY (SEC-02, deferred — circular trust)` comment (`mod.rs:287-296`) is present and accurate: a pure network MITM is blocked (both hops HTTPS), but an upstream/release compromise (or dual-endpoint MITM) forges both binary and digest; real fix is an upstream publisher signature (minisign/cosign/TUF) pinned in the app, which Aztec does not yet provide for `bb`. Tracking reference present. Still open, as expected.
- **M2 (#345)** — `perform_update`'s `None`-size arm proceeds (`updater.rs:116-118`: `warn!` then falls through to `download()`). Confirmed still-open and genuinely exploitable today: because the plugin buffers before verifying (proven above), an attacker who can tamper only the *manifest* (strip `size`, repoint the URL at a multi-GB blob) re-opens the memory-DoS **without** the signing key. The clean fix (make `size` mandatory, fail closed on absence) is deferred only because the live prod `latest.json` is still size-less; the in-code rationale (`updater.rs:96-105`) is honest about this being weaker than it looks. Still open, as expected.
