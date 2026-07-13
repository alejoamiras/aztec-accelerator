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