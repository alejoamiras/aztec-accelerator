//! Redundant-instance health probe.
//!
//! Classifies a lost `:59833` bind — the autostart entry AND the crash-recovery launcher can both
//! start us at logon, so a redundant instance should bow out, but ONLY if the incumbent is really us
//! and not a foreign process squatting on the port. Extracted from server.rs (Q2).

use std::time::Duration;

use super::PORT;

/// Byte cap for a `/health` body (F-03 sink C, audit 2026-07-31-9c4cb0c).
///
/// Both probes below used `resp.json::<Value>()`, which buffers whatever arrives. The 3 s client
/// timeout bounds the *time* — reqwest applies it through body completion — but not the *size*, so a
/// local responder could stream multiple GB into this process inside the window. The real body is a
/// few hundred bytes.
///
/// Mirrors the SDK's `HEALTH_BODY_MAX_BYTES` **value** deliberately, not its constant: the two are
/// different endpoints in different languages that happen to share a policy, and `/prove`'s much
/// larger cap must never be able to leak into this one by a shared symbol.
const HEALTH_BODY_MAX_BYTES: usize = 64 * 1024;

/// Read a response body under [`HEALTH_BODY_MAX_BYTES`] and parse it as JSON.
///
/// `None` on a transport error, an over-cap body, or invalid JSON — every one of which means "do not
/// treat this responder as a healthy Aztec instance", which is the only question the callers ask.
/// Chunk-wise so an over-cap body is abandoned mid-stream rather than after it has been buffered.
async fn read_json_capped(mut resp: reqwest::Response) -> Option<serde_json::Value> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len() + chunk.len() > HEALTH_BODY_MAX_BYTES {
                    tracing::warn!(
                        "/health body exceeded {HEALTH_BODY_MAX_BYTES} bytes — ignoring"
                    );
                    return None;
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    serde_json::from_slice(&buf).ok()
}

/// True iff a `/health` body looks like a healthy Aztec accelerator
/// (`status=="ok"` and `api_version==1`). Pure (unit-tested) so the redundant-vs-foreign
/// classification can't silently accept an arbitrary process answering on :59833.
pub(crate) fn is_healthy_aztec_response(body: &serde_json::Value) -> bool {
    body.get("status").and_then(|s| s.as_str()) == Some("ok")
        && body.get("api_version").and_then(|v| v.as_u64()) == Some(1)
}

/// Probe `http://127.0.0.1:59833/health` and return true iff a HEALTHY Aztec instance
/// answers. Used to classify a lost `:59833` bind: the autostart entry AND the
/// crash-recovery launcher (Task Scheduler / launchd / systemd) can both start us at
/// logon, so a redundant instance should bow out — but only if the incumbent is really
/// us, not some foreign process squatting on the port.
pub async fn healthy_aztec_on_port() -> bool {
    let url = format!("http://127.0.0.1:{PORT}/health");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Ok(resp) = client.get(&url).send().await else {
        return false;
    };
    // Require a 2xx so a non-success responder that happens to echo the right JSON
    // shape isn't mistaken for a healthy Aztec instance.
    if !resp.status().is_success() {
        return false;
    }
    let Some(body) = read_json_capped(resp).await else {
        return false;
    };
    is_healthy_aztec_response(&body)
}

/// Probe `/health` and return the reported `version` iff the responder is a healthy Aztec
/// (`status=="ok"`, `api_version==1`) AND carries a string `version`; `None` otherwise. Lets the
/// F-004 launch tracker confirm it is observing THIS build's OWN server (by matching the reported
/// version to `CARGO_PKG_VERSION`) rather than a foreign healthy Aztec that happens to own `:59833`.
/// This matters because the redundant-instance bow-out is Windows-only — on macOS/Linux a second
/// instance keeps running after losing the bind, so without the version-match a broken new build could
/// see the incumbent's healthy `/health` and ratchet the floor to a version whose server never ran.
pub async fn healthy_aztec_version_on_port() -> Option<String> {
    let url = format!("http://127.0.0.1:{PORT}/health");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = read_json_capped(resp).await?;
    if !is_healthy_aztec_response(&body) {
        return None;
    }
    body.get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_health_responses() {
        // Healthy Aztec: bow out (redundant instance).
        assert!(is_healthy_aztec_response(
            &json!({"status": "ok", "api_version": 1})
        ));
        assert!(is_healthy_aztec_response(
            &json!({"status": "ok", "api_version": 1, "version": "1.2.3"})
        ));
        // Foreign / wrong / malformed: do NOT treat as Aztec (must surface the error,
        // never silently exit and leave the user with no accelerator).
        assert!(!is_healthy_aztec_response(
            &json!({"status": "ok", "api_version": 2})
        ));
        assert!(!is_healthy_aztec_response(
            &json!({"status": "error", "api_version": 1})
        ));
        assert!(!is_healthy_aztec_response(&json!({"api_version": 1})));
        assert!(!is_healthy_aztec_response(&json!({"hello": "world"})));
        assert!(!is_healthy_aztec_response(&json!({})));
        assert!(!is_healthy_aztec_response(&json!("not even an object")));
    }

    /// Serve one raw HTTP response of `body` from an ephemeral loopback port and return its URL.
    ///
    /// Port 0, never the real `PORT`: this suite must be safe to run concurrently with a live
    /// accelerator and with other agents' runs on the same machine.
    async fn serve_once(body: String) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut scratch = [0u8; 1024];
            let _ = sock.read(&mut scratch).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(body.as_bytes()).await;
        });
        format!("http://{addr}/health")
    }

    /// F-03 sink C (audit 2026-07-31-9c4cb0c): `resp.json::<Value>()` buffered whatever a local
    /// responder chose to send. The 3 s client timeout bounds time, not bytes.
    #[tokio::test]
    async fn oversized_health_body_is_rejected_rather_than_buffered() {
        let padding = "A".repeat(128 * 1024); // 2x the 64 KiB cap
        let url = serve_once(format!(
            r#"{{"status":"ok","api_version":1,"pad":"{padding}"}}"#
        ))
        .await;
        let resp = reqwest::get(&url).await.unwrap();
        assert!(
            read_json_capped(resp).await.is_none(),
            "an over-cap /health body must be ignored, not buffered and parsed"
        );
    }

    /// The cap must not reject the bodies the real server actually sends.
    #[tokio::test]
    async fn a_normal_health_body_still_parses() {
        let url =
            serve_once(r#"{"status":"ok","api_version":1,"version":"1.2.3"}"#.to_string()).await;
        let resp = reqwest::get(&url).await.unwrap();
        let body = read_json_capped(resp)
            .await
            .expect("normal body must parse");
        assert!(is_healthy_aztec_response(&body));
        assert_eq!(body.get("version").and_then(|v| v.as_str()), Some("1.2.3"));
    }
}
