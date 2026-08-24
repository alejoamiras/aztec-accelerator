//! Loopback `Host`/`:authority` allowlist (SEC-01a) — the DNS-rebinding keystone.
//!
//! Every request must carry a `Host` (HTTP/1.1) or `:authority` (HTTP/2) whose host-component is an
//! exact loopback literal (`127.0.0.1` / `localhost` / `[::1]`) on the listener's expected port. A
//! DNS-rebinding page's `Host` is the attacker's domain (`evil.com:59833`), not a loopback literal,
//! so it is rejected here — *before* the Origin gate runs, and regardless of whether an Origin is
//! present. This is behaviour-preserving: every real client (the SDK over HTTP `127.0.0.1:59833` and
//! Safari HTTPS `127.0.0.1:59834`, curl/Node/CI) already sends a loopback Host on the right port.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// True iff `authority` is exactly a loopback host on `expected_port`, with no userinfo.
///
/// Parsed via [`axum::http::uri::Authority`] (canonical authority grammar) rather than ad-hoc
/// `split(':')`, so malformed/duplicate/IPv6 forms are handled correctly. Exact-port match is
/// required (real clients always send the explicit port). Rejects userinfo (`user@host` could
/// smuggle a host past a naive parser), non-loopback hosts, and all alternate numeric forms
/// (`0.0.0.0`, decimal/hex IPs, `[::ffff:127.0.0.1]`).
pub(crate) fn host_is_trusted(authority: &str, expected_port: u16) -> bool {
    // Userinfo is never legitimate for a localhost service and is a classic host-smuggle vector.
    if authority.contains('@') {
        return false;
    }
    let Ok(parsed) = authority.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    // The http crate's Authority parser silently DISCARDS junk between a closing `]` and the port
    // — `"[::1]..:59833"` parses Ok with host `[::1]` — so the bracketed form must be checked on
    // the RAW authority string or the dotted-quad and IPv6 surfaces diverge (IH-BUG-1).
    if let Some(i) = authority.find(']') {
        if !authority[i + 1..].starts_with(':') {
            return false;
        }
    }
    // Exact port required — no "port absent" loophole (drops the weaker invariant; real clients
    // send `127.0.0.1:59833`/`:59834`), and no wrong-port (`:59834` on the HTTP listener).
    if parsed.port_u16() != Some(expected_port) {
        return false;
    }
    // Normalise: lowercase, strip at most ONE trailing dot (`localhost.` — the DNS root form a
    // real resolver can emit), strip IPv6 brackets (`[::1]`→`::1`). `trim_end_matches` would strip
    // ALL trailing dots, silently widening the allowlist to forms no real client sends (IH-BUG-1).
    let host = parsed.host();
    let host = host.strip_suffix('.').unwrap_or(host);
    let host = host.to_ascii_lowercase();
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host.as_str());
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// Axum middleware gating every request on a trusted loopback `Host`/`:authority` for `expected_port`.
///
/// Reads the HTTP/1.1 `Host` header AND the HTTP/2 `:authority` (`req.uri().authority()`); if both
/// are present and disagree it fails closed (can't tell which the peer meant); if neither is present
/// it fails closed (HTTP/1.1 mandates `Host` and HTTP/2 mandates `:authority` — no real client omits
/// both). Replies `403 invalid_host` with a minimal body that does not echo the offending host.
pub(crate) async fn guard(expected_port: u16, req: Request, next: Next) -> Response {
    let host_header = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let authority = req.uri().authority().map(|a| a.as_str());

    let value = match (host_header, authority) {
        // Both present and disagreeing → fail closed.
        (Some(h), Some(a)) if h != a => None,
        (Some(h), _) => Some(h),
        (None, Some(a)) => Some(a),
        // Neither present → fail closed.
        (None, None) => None,
    };

    match value {
        Some(v) if host_is_trusted(v, expected_port) => next.run(req).await,
        _ => (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({ "error": "invalid_host" })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::host_is_trusted;

    const HTTP: u16 = 59833;
    const HTTPS: u16 = 59834;

    #[test]
    fn accepts_real_client_authorities() {
        // The exact forms the SDK + Safari emit (transport.ts: `127.0.0.1:{59833,59834}`).
        assert!(host_is_trusted("127.0.0.1:59833", HTTP));
        assert!(host_is_trusted("127.0.0.1:59834", HTTPS));
        // Configurable host + IPv6 loopback the cert also covers.
        assert!(host_is_trusted("localhost:59833", HTTP));
        assert!(host_is_trusted("[::1]:59834", HTTPS));
        // Case-insensitive + ONE trailing dot (DNS root form).
        assert!(host_is_trusted("LocalHost:59833", HTTP));
        assert!(host_is_trusted("localhost.:59833", HTTP));
    }

    #[test]
    fn rejects_multi_dot_trailing_forms() {
        // IH-BUG-1: strip at most one trailing dot — `trim_end_matches` used to accept
        // `Host: 127.0.0.1..:59833` (accepted live, 2026-08-21 attack matrix). The bracketed
        // sibling is rejected by the raw `]`+port check, since Authority::parse would otherwise
        // discard the dots before normalization ever sees them.
        assert!(!host_is_trusted("127.0.0.1..:59833", HTTP));
        assert!(!host_is_trusted("localhost..:59833", HTTP));
        assert!(!host_is_trusted("[::1]..:59833", HTTP));
    }

    #[test]
    fn rejects_dns_rebinding_and_external_hosts() {
        assert!(!host_is_trusted("evil.com:59833", HTTP));
        assert!(!host_is_trusted("attacker.localhost.com:59833", HTTP));
    }

    #[test]
    fn rejects_wrong_port() {
        // HTTPS authority on the HTTP listener and vice-versa.
        assert!(!host_is_trusted("127.0.0.1:59834", HTTP));
        assert!(!host_is_trusted("127.0.0.1:59833", HTTPS));
        // No port at all → reject (no "port absent" loophole).
        assert!(!host_is_trusted("127.0.0.1", HTTP));
        assert!(!host_is_trusted("localhost", HTTP));
    }

    #[test]
    fn rejects_alternate_numeric_and_mapped_forms() {
        assert!(!host_is_trusted("0.0.0.0:59833", HTTP));
        assert!(!host_is_trusted("2130706433:59833", HTTP)); // decimal 127.0.0.1
        assert!(!host_is_trusted("[::ffff:127.0.0.1]:59833", HTTP)); // IPv4-mapped IPv6
        assert!(!host_is_trusted("[::ffff:7f00:1]:59833", HTTP));
    }

    #[test]
    fn rejects_userinfo_smuggling() {
        // A naive `split(':')`/suffix parser could read the host as 127.0.0.1 here.
        assert!(!host_is_trusted("evil.com@127.0.0.1:59833", HTTP));
        assert!(!host_is_trusted("127.0.0.1@evil.com:59833", HTTP));
        assert!(!host_is_trusted("user@localhost:59833", HTTP));
    }

    #[test]
    fn rejects_malformed_authorities() {
        assert!(!host_is_trusted("", HTTP));
        assert!(!host_is_trusted(":59833", HTTP));
        assert!(!host_is_trusted("127.0.0.1:notaport", HTTP));
        assert!(!host_is_trusted("127.0.0.1:59833:extra", HTTP));
    }
}
