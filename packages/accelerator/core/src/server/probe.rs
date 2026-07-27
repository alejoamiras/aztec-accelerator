//! Redundant-instance health probe.
//!
//! Classifies a lost `:59833` bind — the autostart entry AND the crash-recovery launcher can both
//! start us at logon, so a redundant instance should bow out, but ONLY if the incumbent is really us
//! and not a foreign process squatting on the port. Extracted from server.rs (Q2).

use std::time::Duration;

use super::PORT;

/// True iff a `/health` body looks like a healthy Aztec accelerator
/// (`status=="ok"` and `api_version==1`). Pure (unit-tested) so the redundant-vs-foreign
/// classification can't silently accept an arbitrary process answering on :59833.
fn is_healthy_aztec_response(body: &serde_json::Value) -> bool {
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
    let Ok(body) = resp.json::<serde_json::Value>().await else {
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
    let body = resp.json::<serde_json::Value>().await.ok()?;
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
}
