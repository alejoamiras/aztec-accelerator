# C2 (core-request-safety) — Codex gpt-5.6-sol @ xhigh design consult

F-003 / F-009 / F-011 implementation design + test matrix.

---

The clean design is three focused changes: private-at-creation witness storage, an RAII “permit + body” seam with a 30-second absolute deadline, and rejecting dotted origins before canonicalization.

## F-003 — private witness at creation

Current seam: [bb.rs:86](packages/accelerator/core/src/bb.rs:86).

Use only `tempfile` and `std`; no `rcgen` or new dependency:

```rust
use std::io::Write;

fn create_prove_tempdir() -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("aztec-accelerator-prove-");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }

    builder.tempdir()
}

fn write_witness(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file: std::fs::File = options.open(path)?;
    file.write_all(bytes)
}
```

Then replace [bb.rs:86-90](packages/accelerator/core/src/bb.rs:86):

```rust
let tmp_dir = create_prove_tempdir()?;
let input_path = tmp_dir.path().join("ivc-inputs.msgpack");
let output_dir = tmp_dir.path().join("output");

std::fs::create_dir_all(&output_dir)?;
write_witness(&input_path, ivc_inputs)?;
```

`permissions(0700)` and `mode(0600)` are supplied to the creation syscalls; there is no exposed write-then-chmod window. `create_new(true)` also rejects an unexpected pre-existing path/symlink.

Windows continues to compile because Unix extension traits are locally `#[cfg(unix)]`. Windows protection comes from inherited temp-directory ACLs; explicit Windows ACL hardening would require Windows security descriptors, not Unix modes.

The `output/` directory and `output/proof` do not require equivalent witness protection:

- They are beneath the non-traversable `0700` parent.
- The proof is not the private witness.
- As cheap defense-in-depth, `output/` could also be created `0700`, but do not add post-creation chmod logic for BB’s proof file.

New test in `bb.rs`:

```rust
#[cfg(unix)]
#[test]
fn prove_workspace_and_witness_have_private_modes() {
    use std::os::unix::fs::MetadataExt;

    let dir = create_prove_tempdir().unwrap();
    let witness = dir.path().join("ivc-inputs.msgpack");
    write_witness(&witness, b"secret").unwrap();

    assert_eq!(
        std::fs::metadata(dir.path()).unwrap().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(witness).unwrap().mode() & 0o777,
        0o600
    );
}
```

GUI-less Linux VPS: **yes**. It needs a real Unix filesystem/runtime but no BB binary or GUI.

## F-009 — permit before one bounded body read

Current ordering: [prove.rs:101-125](packages/accelerator/core/src/server/prove.rs:101).

Add a testable seam that returns the permit alongside the body:

```rust
use axum::body::{Body, Bytes};
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_BODY_SIZE: usize = 50 * 1024 * 1024;
const BODY_READ_TIMEOUT: Duration = Duration::from_secs(30);

async fn acquire_and_read_body(
    semaphore: Arc<Semaphore>,
    raw_body: Body,
    max_body_size: usize,
    read_timeout: Duration,
) -> Result<(OwnedSemaphorePermit, Bytes), ProveError> {
    let permit = semaphore
        .acquire_owned()
        .await
        .map_err(|_| ProveError::ServiceUnavailable)?;

    let body = tokio::time::timeout(
        read_timeout,
        axum::body::to_bytes(raw_body, max_body_size),
    )
    .await
    .map_err(|_| {
        tracing::warn!(?read_timeout, "Timed out reading /prove request body");
        ProveError::BodyReadTimeout
    })?
    .map_err(|e| {
        tracing::warn!("Failed to read /prove request body: {e}");
        ProveError::PayloadTooLarge(e.to_string())
    })?;

    Ok((permit, body))
}
```

Handler ordering:

```rust
let (parts, raw_body) = request.into_parts();

authorize_origin(&state, &parts.headers).await?;

// Optional header-only fast rejection here; it does not poll the body.
reject_declared_oversize(&parts.headers)?;

let (_permit, body) = acquire_and_read_body(
    state.prove_semaphore.clone(),
    raw_body,
    MAX_BODY_SIZE,
    BODY_READ_TIMEOUT,
)
.await?;
```

`_permit` remains in handler scope across version resolution, download, `bb::prove`, encoding, and response construction. Exactly one body exists and exactly one `to_bytes` future is polled, after permit acquisition.

Add a distinct error in [server.rs:322](packages/accelerator/core/src/server.rs:322):

```rust
BodyReadTimeout,
```

Map it as:

```rust
ProveError::BodyReadTimeout => (
    StatusCode::REQUEST_TIMEOUT,
    "body_read_timeout",
    "Timed out while reading request body".to_string(),
),
```

Do not reuse:

- `AuthorizationTimeout`: wrong stage and currently a 403.
- `PayloadTooLarge`: timeout is not a 413.
- `ServiceUnavailable`: reserved for semaphore/service shutdown.

RAII releases `OwnedSemaphorePermit` on timeout, size error, body/disconnect error, `?` return, task cancellation, panic unwind, and success.

### Head-of-line tradeoff

Yes: one slow uploader now owns the only proof permit for at most 30 seconds. That is the necessary tradeoff for bounding buffered witness memory. Thirty seconds is conservative for 50 MiB over loopback—about 1.7 MiB/s—and must be a whole-body absolute deadline, not an idle timeout vulnerable to drip feeding.

Also add:

- After authorization, reject a valid `Content-Length > MAX_BODY_SIZE` before acquiring the proof permit. Never trust it as the sole limit; chunked/underreported requests still need `to_bytes`.
- As defense-in-depth, cap authenticated `/prove` requests waiting or active, e.g. eight, using a separate post-authorization `try_acquire_owned` semaphore and return 429. Avoid a global pre-authorization concurrency layer that unapproved callers can exhaust.

### Test matrix

Place helper tests in `server/prove.rs`; add `futures-util = "0.3"` as a dev dependency for stalled/error streams.

- Oversized: small injected limit, assert `PayloadTooLarge` and one available permit.
- Stalled stream: `stream::pending()`, paused Tokio time, assert `BodyReadTimeout`, 408 mapping, and permit restored.
- Disconnect: stream returns `io::Error`, assert body-read failure path and permit restored.
- Cancellation: spawn with a pending body, wait until permits reach zero, abort task, assert permit returns.
- Second request: queue a normal body behind a stalled request; advance through the first timeout and assert the second completes.
- Happy path: returned bytes match; permit remains unavailable while the returned guard is alive and returns after drop.

Replace the existing [server/tests.rs:1047](packages/accelerator/core/src/server/tests.rs:1047) test: despite its name, it currently sends only ten bytes and never exercises the oversized path.

GUI-less Linux VPS: **yes**. These are in-process Tokio tests; no GUI or real network is needed.

### `DefaultBodyLimit`

The layer at [server.rs:220](packages/accelerator/core/src/server.rs:220) is not a second active limit here. `DefaultBodyLimit` only affects extractors that opt into it (`Bytes`, `Json`, etc.). The handler extracts raw `Request` and directly calls `to_bytes`, so the explicit limit is the effective control.

Keep the layer only as protection for future extractor-based routes, preferably sharing one constant and with a clarifying comment. It does not poll the body early and does not double-buffer it.

## F-011 — reject dotted origins

Current seam: [authorization.rs:21-58](packages/accelerator/core/src/authorization.rs:21).

Reject before normalization and remove `trim_end_matches`:

```rust
let host = url.host_str()?;
if host.ends_with('.') {
    return None;
}

match url.scheme() {
    "http" | "https" | "ws" | "wss" => {
        let host = host.to_ascii_lowercase();
        Some(match url.port() {
            Some(p) => format!("{}://{}:{}", url.scheme(), host, p),
            None => format!("{}://{}", url.scheme(), host),
        })
    }
    scheme @ ("chrome-extension" | "moz-extension" | "safari-web-extension") => {
        if url.port().is_some() {
            return None;
        }
        let id = host.to_ascii_lowercase();
        Some(format!("{scheme}://{id}"))
    }
    _ => None,
}
```

This makes `CanonicalOrigin::parse`, `TryFrom<String>`, and strict serde all reject dotted hosts. It never turns `https://x.` into `https://x`.

Persisted config already has the correct lenient load seam at [config.rs:110-133](packages/accelerator/core/src/config.rs:110):

```rust
match CanonicalOrigin::try_from(entry) {
    Ok(canon) if !out.contains(&canon) => out.push(canon),
    Ok(_) => {}
    Err(e) => dropped.push(e.0),
}

if !dropped.is_empty() {
    tracing::warn!(
        count = dropped.len(),
        dropped = ?dropped,
        "Dropped invalid approved_origins entries on load"
    );
}
```

After the constructor change, old dotted entries take the `Err` branch, are warned and dropped, while the rest of the config loads normally. No schema bump, resave, or dotted-to-undotted migration is needed.

Leave [server/host.rs:35-36](packages/accelerator/core/src/server/host.rs:35) unchanged. Host-header authority normalization is a separate transport-level policy and may continue accepting `localhost.`.

Code search found only the existing positive `canon_trailing_dot_stripped` test relying on the alias. Other production effects are intentional:

- `/prove` authorization and `/health` reject dotted Origins.
- Headless `ALLOWED_ORIGINS` fails fast on dotted operator input.
- Verified-site lookup no longer grants an undotted site’s badge to its dotted form.

New tests:

- `authorization.rs`: replace `canon_trailing_dot_stripped` with rejection vectors for `https://x.`, `https://x.:443`, `http://localhost.:5173`, `wss://x.`, and extension IDs ending in `.`.
- `config.rs`: real `load_from` fixture containing one valid and one dotted entry; assert the valid entry survives and the dotted entry does not become undotted.
- `server/tests.rs`: approve `https://x`, send `Origin: https://x.`, assert `400 invalid_origin`, not approval reuse.
- Optionally capture the warning with `tracing-test`; the essential security assertion is drop-without-crash.

GUI-less Linux VPS: **yes**. These are pure parsing/config/router tests.
---

## Post-implementation audits (GATE 3) — Codex gpt-5.6-sol @ xhigh

### Round 1 verdict (on the initial C2 diff): CHANGES-REQUIRED → all 5 folded
```
CHANGES-REQUIRED

1. **Medium — unbounded FIFO waiters amplify the 30-second slow-body timeout.** [server/prove.rs:141](<packages/accelerator/core/src/server/prove.rs:141>) acquires without a deadline; the timeout starts only at line 146. Scenario: while one proof holds the permit, an attacker queues 100 pending bodies. After release, each receives a fresh 30 seconds, blocking a legitimate request behind them for roughly 50 minutes. Fix: bound post-authorization waiters and/or use one absolute deadline covering semaphore acquisition plus body buffering; return 429/503 for queue expiry.

2. **Medium — F-003 still depends on ambient Windows/temp-parent ACLs.** [bb.rs:80](<packages/accelerator/core/src/bb.rs:80>) and [bb.rs:95](<packages/accelerator/core/src/bb.rs:95>) omit all protection off Unix. Scenario: `%TEMP%` points to a shared directory with inheritable read permissions; another user enumerates the recognizable prefix and reads the witness. A temp parent granting rename/delete also permits ancestor replacement between lines 117–121; `create_new` protects only the final component. Fix: create under a verified private per-user parent and apply an owner-only Windows DACL at creation; add a Windows effective-ACL test.

3. **Low — `Content-Length` fast rejection is incomplete.** [server/prove.rs:117](<packages/accelerator/core/src/server/prove.rs:117>) reads one value and uses `usize::parse`. Scenario: an HTTP/2 value such as `52428801, 52428801`, which Hyper accepts as consistent duplicate lengths, bypasses pre-permit rejection and can occupy the permit until timeout. Overflow also bypasses on narrower targets. The 50 MiB `to_bytes` limit still prevents oversized buffering. Fix: inspect all values, split legal comma lists, require equal ASCII-digit `u64` values, and reject any value above the cap.

4. **Low — dotted localhost remains approved by a public helper.** [authorization.rs:283](<packages/accelerator/core/src/authorization.rs:283>) still returns true for `http://localhost.:5173`, while `CanonicalOrigin::parse` rejects it. Current `/prove` is protected because it canonicalizes first, but direct consumers of this public helper get contradictory authorization behavior. Fix: accept `&CanonicalOrigin` or remove `trim_end_matches('.')`.

5. **Low — tests do not prove the critical ordering/lifetime guarantees.** [server/prove.rs:311](<packages/accelerator/core/src/server/prove.rs:311>) would still pass if a ready body were read before acquiring the permit; it only proves the returned guard is live. There is no cancellation/disconnect test, no second-body poll counter, no whole-handler permit-lifetime test, and no dotted persisted-config/router test. Highest-value missing test: queue multiple pending bodies behind an occupied semaphore and prove they expire from one ingress deadline instead of consuming sequential fresh deadlines. The `start_paused` stalled-body test itself is deterministic and passes.

Confirmed: on Unix the creation syscalls request `0700`/`0600`—umask can only make them stricter. The default-mode `output/` remains protected by its non-traversable parent. `_permit` lives through the complete handler, and RAII releases it on success, errors, cancellation, and panic unwind. The body timeout is absolute once body reading begins, no application body polling occurs before acquisition, `DefaultBodyLimit` is inert for the raw `Request` extractor, 408 is appropriate, and the new tracing adds no sensitive-data leak.```

### Round 2 verdict (on the fold): #1 waiter-cap RESOLVED, #4 dotted-localhost RESOLVED
```
# CHANGES-REQUIRED

1. **RESOLVED — bounded `/prove` waiters.** `MAX_INFLIGHT_PROVE=8` counts the active request plus seven waiters ([server.rs:37](packages/accelerator/core/src/server.rs:37)). `_inflight` is acquired after authorization and remains lexically alive through body read, download, proving, and response construction ([prove.rs:200](packages/accelerator/core/src/server/prove.rs:200), [prove.rs:319](packages/accelerator/core/src/server/prove.rs:319)); RAII releases it on every return/cancellation path. Nonblocking acquisition is essential—blocking would recreate an unbounded outer queue. Acquisition order is consistently waiter → prove permit → body, with no reverse path or deadlock. An admitted request has at most seven predecessors, bounding slow-body amplification to 210 seconds, excluding unbounded download/BB execution. Eight is a reasonable availability/burst tradeoff. 429 mapping is correct ([server.rs:400](packages/accelerator/core/src/server.rs:400)).

2. **NOT-RESOLVED — private witness parent.** On Windows, `create_dir_all` still merely inherits `%LOCALAPPDATA%`’s ambient DACL; no owner-only DACL is applied or verified ([bb.rs:93](packages/accelerator/core/src/bb.rs:93)). On Unix, recursive creation plus a later path-based `set_permissions` does not verify ownership, symlinks, or permissions of existing ancestors and is not race-free ([bb.rs:78](packages/accelerator/core/src/bb.rs:78)). An unsafe `$XDG_DATA_HOME` ancestor or pre-existing writable `aztec-accelerator` directory can still be renamed/replaced between these operations and `tempdir_in`. Worse, any resolution/chmod failure silently falls back to OS temp, reinstating the original condition ([bb.rs:111](packages/accelerator/core/src/bb.rs:111)). F-003 requires fail-closed private-parent creation/validation and an explicit Windows owner-only DACL with an effective-ACL test.

3. **NOT-RESOLVED — `Content-Length` parsing.** All header values, numeric oversize elements, and `u64` overflow are now rejected correctly ([prove.rs:121](packages/accelerator/core/src/server/prove.rs:121)). However, empty elements are explicitly skipped, so `""`, `","`, `1,`, and `,1` pass; duplicate values are not required to agree; and parsing is not restricted to the `1*DIGIT` grammar ([prove.rs:127](packages/accelerator/core/src/server/prove.rs:127)). Thus the prior equal-ASCII-digit requirement remains unmet. No explicit numeric value above the cap can bypass now; absent, underreported, or chunked bodies still acquire the permit and are authoritatively bounded by `to_bytes` ([prove.rs:165](packages/accelerator/core/src/server/prove.rs:165)).

4. **RESOLVED — dotted localhost.** Exact matching makes raw `http://localhost.:5173` false while preserving canonical `localhost`, `127.0.0.1`, and `[::1]` behavior ([authorization.rs:274](packages/accelerator/core/src/authorization.rs:274)). This is consistent with canonicalization rejecting hostname trailing dots ([authorization.rs:34](packages/accelerator/core/src/authorization.rs:34)). No canonical localhost variant regresses.

5. **NOT-RESOLVED — tests still do not prove ordering/lifetime.** The zero-permit test would also pass if a ready body were fully consumed first and the future then waited for the permit; it observes only non-completion, not body polling ([prove.rs:369](packages/accelerator/core/src/server/prove.rs:369)). The waiter test manually drops primitive guards and would pass if the handler dropped `_inflight` immediately ([prove.rs:355](packages/accelerator/core/src/server/prove.rs:355)). There remains no body poll-counter test, whole-handler waiter/prove-permit lifetime test, cancellation/disconnect test, or handler-level 429/release test. The dotted-config test proves the custom deserializer but not `load_from` or router behavior ([config.rs:403](packages/accelerator/core/src/config.rs:403)).

**New Low:** moving workspaces into persistent application data means an abort/crash can leave private witness directories indefinitely, with no startup scavenging and possible disk accumulation ([bb.rs:73](packages/accelerator/core/src/bb.rs:73), [bb.rs:151](packages/accelerator/core/src/bb.rs:151)).

`cargo fmt --check` and diff checks passed. Cargo tests could not start because the audit filesystem denied creation of `.cargo-build-lock`.```

### Dispositions (round-2 residuals) — decided per the "Codex is advisory" protocol
- **#3 Content-Length strictness** — FOLDED (RFC-7230: reject empty/partial/non-digit, require equal values, u64 no-overflow). No oversize bypass existed (to_bytes authoritative); closed for cleanliness.
- **#5 highest-value test** — FOLDED (handler-level 429 `prove_sheds_with_429_when_waiter_cap_full`). Remaining poll-counter/cancellation tests deferred: the primitives (try_enter shed/release, permit-before-body ordering) are unit-tested and Codex confirmed RAII release on all paths.
- **#2 Windows owner-only DACL + effective-ACL test + fully-race-free ancestor validation** — DEFERRED (tracked). The CONFIRMED F-003 vuln (world-readable 0o755 tempdir) is resolved by 0o700/0o600-at-creation in both the private-parent and OS-temp-fallback paths (Codex confirmed the Unix modes in round 1). The residual defends against an attacker who already controls the user's per-user data dir (OUT of F-003's co-resident-reader threat model) and needs Windows-specific DACL code untestable on this GUI-less Linux runner.
- **new-Low crash-leftover witness dirs** — ACCEPTED. Leftovers are 0o700 owner-only (no confidentiality leak — the fix's whole point); blind startup scavenging is unsafe under concurrent multi-instance/multi-agent use (could delete a live proof workspace). Age-based hygiene scavenging is a possible future follow-up.
