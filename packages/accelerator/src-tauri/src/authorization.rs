use parking_lot::Mutex;
use std::collections::HashMap;
use tokio::sync::oneshot;
use url::Url;

/// Canonicalize an origin string per RFC 6454 to a single comparable form.
///
/// Tuple-origin schemes (`http`, `https`, `ws`, `wss`):
///   - lowercased scheme + `://` + lowercased host + (non-default port)
///   - empty hosts and trailing-dot hosts both normalize/reject correctly
///
/// Opaque-origin schemes (`chrome-extension`, `moz-extension`, `safari-web-extension`):
///   - exact scheme match (not prefix), no port allowed
///   - lowercased extension ID
///
/// Universal rejections:
///   - path other than empty or `/`
///   - non-empty query, fragment, username, or password
///
/// Returns `None` for unparseable or disallowed input.
pub fn canonicalize_origin(input: &str) -> Option<String> {
    let url = Url::parse(input).ok()?;

    if !url.path().is_empty() && url.path() != "/" {
        return None;
    }
    if url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }

    match url.scheme() {
        "http" | "https" | "ws" | "wss" => {
            let host = url.host_str()?.to_ascii_lowercase();
            let host = host.trim_end_matches('.');
            if host.is_empty() {
                return None;
            }
            Some(match url.port() {
                Some(p) => format!("{}://{}:{}", url.scheme(), host, p),
                None => format!("{}://{}", url.scheme(), host),
            })
        }
        scheme @ ("chrome-extension" | "moz-extension" | "safari-web-extension") => {
            if url.port().is_some() {
                return None;
            }
            let id = url.host_str()?.to_ascii_lowercase();
            if id.is_empty() {
                return None;
            }
            Some(format!("{scheme}://{id}"))
        }
        _ => None,
    }
}

/// Decision from the user about whether to authorize an origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthDecision {
    Allow { remember: bool },
    Deny,
}

/// Manages pending authorization requests.
///
/// When a `/prove` request arrives from an unknown origin, the handler calls
/// `request(origin)`. If this is the first request for that origin, the caller
/// shows an authorization popup. Subsequent requests from the same origin
/// piggyback on the same popup — they all share the decision.
/// Maximum number of distinct origins that can have pending authorization simultaneously.
/// Prevents popup/memory spam from a malicious site generating many subdomains.
const MAX_PENDING_ORIGINS: usize = 10;

pub struct AuthorizationManager {
    pending: Mutex<HashMap<String, Vec<oneshot::Sender<AuthDecision>>>>,
}

impl Default for AuthorizationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthorizationManager {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Register a pending authorization request for `origin`.
    ///
    /// Returns `Ok((receiver, is_first))`. If `is_first` is true, the caller should
    /// show the authorization popup. Otherwise, a popup is already showing and
    /// this request will piggyback on that decision.
    ///
    /// Returns `Err` if the maximum number of pending origins is exceeded (DoS protection).
    pub fn request(
        &self,
        origin: &str,
    ) -> Result<(oneshot::Receiver<AuthDecision>, bool), &'static str> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock();
        let is_first = !pending.contains_key(origin);
        if is_first && pending.len() >= MAX_PENDING_ORIGINS {
            return Err("too many pending authorization requests");
        }
        pending.entry(origin.to_string()).or_default().push(tx);
        Ok((rx, is_first))
    }

    /// Resolve all pending requests for `origin` with the given decision.
    pub fn resolve(&self, origin: &str, decision: AuthDecision) {
        let mut pending = self.pending.lock();
        if let Some(senders) = pending.remove(origin) {
            for tx in senders {
                let _ = tx.send(decision);
            }
        }
    }

    /// Returns true for localhost origins that should be auto-approved.
    pub fn is_auto_approved(origin: &str) -> bool {
        // Q14: reuse the same `url::Url` parsing as `canonicalize_origin` (Substitute Algorithm)
        // instead of the hand-rolled prefix-strip + ':'-split host extraction. The input is already
        // canonical, so this is behavior-identical — pinned by `auto_approved_localhost_variants`
        // (incl. the `[::1]` IPv6 case) + `non_localhost_not_auto_approved`.
        Url::parse(origin)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
            .is_some_and(|h| matches!(h.trim_end_matches('.'), "localhost" | "127.0.0.1" | "[::1]"))
    }

    /// Returns true if the origin is approved (auto-approved or in the approved list).
    ///
    /// The input `origin` is expected to ALREADY be canonical (use [`canonicalize_origin`]
    /// at request ingress). Persisted entries in `approved_origins` are likewise canonical
    /// (enforced by [`crate::config::load`]'s migration step).
    pub fn is_approved(origin: &str, approved_origins: &[String]) -> bool {
        Self::is_auto_approved(origin) || approved_origins.iter().any(|o| o == origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_approved_localhost_variants() {
        assert!(AuthorizationManager::is_auto_approved(
            "http://localhost:5173"
        ));
        assert!(AuthorizationManager::is_auto_approved(
            "http://127.0.0.1:5173"
        ));
        assert!(AuthorizationManager::is_auto_approved(
            "https://localhost:59834"
        ));
        assert!(AuthorizationManager::is_auto_approved("http://localhost"));
        assert!(AuthorizationManager::is_auto_approved("http://[::1]:5173"));
    }

    #[test]
    fn non_localhost_not_auto_approved() {
        assert!(!AuthorizationManager::is_auto_approved(
            "https://example.com"
        ));
        assert!(!AuthorizationManager::is_auto_approved(
            "https://evil.localhost.com"
        ));
        assert!(!AuthorizationManager::is_auto_approved(
            "http://192.168.1.1:8080"
        ));
    }

    #[test]
    fn is_approved_checks_both() {
        let approved = vec!["https://playground.aztec-accelerator.dev".to_string()];
        assert!(AuthorizationManager::is_approved(
            "http://localhost:5173",
            &approved
        ));
        assert!(AuthorizationManager::is_approved(
            "https://playground.aztec-accelerator.dev",
            &approved
        ));
        assert!(!AuthorizationManager::is_approved(
            "https://evil.com",
            &approved
        ));
    }

    #[tokio::test]
    async fn request_and_resolve() {
        let mgr = AuthorizationManager::new();
        let (rx1, is_first1) = mgr.request("https://example.com").unwrap();
        assert!(is_first1);

        let (rx2, is_first2) = mgr.request("https://example.com").unwrap();
        assert!(!is_first2);

        mgr.resolve(
            "https://example.com",
            AuthDecision::Allow { remember: true },
        );

        assert_eq!(rx1.await.unwrap(), AuthDecision::Allow { remember: true });
        assert_eq!(rx2.await.unwrap(), AuthDecision::Allow { remember: true });
    }

    #[tokio::test]
    async fn resolve_deny() {
        let mgr = AuthorizationManager::new();
        let (rx, _) = mgr.request("https://evil.com").unwrap();
        mgr.resolve("https://evil.com", AuthDecision::Deny);
        assert_eq!(rx.await.unwrap(), AuthDecision::Deny);
    }

    #[test]
    fn rejects_when_too_many_pending_origins() {
        let mgr = AuthorizationManager::new();
        for i in 0..MAX_PENDING_ORIGINS {
            assert!(mgr.request(&format!("https://site{i}.com")).is_ok());
        }
        // One more should fail
        assert!(mgr.request("https://one-too-many.com").is_err());
        // Piggybacking on an existing origin should still work
        assert!(mgr.request("https://site0.com").is_ok());
    }

    // ─── canonicalize_origin ────────────────────────────────────────────

    #[test]
    fn canon_default_https_port_elided() {
        assert_eq!(
            canonicalize_origin("https://nulo.sh:443"),
            Some("https://nulo.sh".to_string()),
        );
        assert_eq!(
            canonicalize_origin("https://nulo.sh"),
            Some("https://nulo.sh".to_string()),
        );
    }

    #[test]
    fn canon_default_http_port_elided() {
        assert_eq!(
            canonicalize_origin("http://example.com:80"),
            Some("http://example.com".to_string()),
        );
    }

    #[test]
    fn canon_non_default_port_kept() {
        assert_eq!(
            canonicalize_origin("https://nulo.sh:8443"),
            Some("https://nulo.sh:8443".to_string()),
        );
    }

    #[test]
    fn canon_lowercase_host_and_scheme() {
        assert_eq!(
            canonicalize_origin("HTTPS://NULO.SH"),
            Some("https://nulo.sh".to_string()),
        );
    }

    #[test]
    fn canon_trailing_dot_stripped() {
        assert_eq!(
            canonicalize_origin("https://nulo.sh."),
            Some("https://nulo.sh".to_string()),
        );
    }

    #[test]
    fn canon_root_path_accepted() {
        assert_eq!(
            canonicalize_origin("https://nulo.sh/"),
            Some("https://nulo.sh".to_string()),
        );
    }

    #[test]
    fn canon_rejects_path_content() {
        assert!(canonicalize_origin("https://nulo.sh/admin").is_none());
        assert!(canonicalize_origin("https://nulo.sh//").is_none());
    }

    #[test]
    fn canon_rejects_query() {
        assert!(canonicalize_origin("https://nulo.sh?x=1").is_none());
    }

    #[test]
    fn canon_rejects_fragment() {
        assert!(canonicalize_origin("https://nulo.sh#frag").is_none());
    }

    #[test]
    fn canon_rejects_userinfo() {
        assert!(canonicalize_origin("https://user@nulo.sh").is_none());
        assert!(canonicalize_origin("https://user:pass@nulo.sh").is_none());
    }

    #[test]
    fn canon_rejects_empty_host() {
        // Bare "https://" doesn't even parse, but explicit empty/trim-to-empty must reject.
        assert!(canonicalize_origin("https://.").is_none());
    }

    #[test]
    fn canon_chrome_extension_lowercased() {
        assert_eq!(
            canonicalize_origin("chrome-extension://BAFBIOGFMIBDOJBHPHGPBMBFOKMHBPEH"),
            Some("chrome-extension://bafbiogfmibdojbhphgpbmbfokmhbpeh".to_string()),
        );
    }

    #[test]
    fn canon_chrome_extension_trailing_slash_stripped() {
        assert_eq!(
            canonicalize_origin("chrome-extension://bafbiogfmibdojbhphgpbmbfokmhbpeh/"),
            Some("chrome-extension://bafbiogfmibdojbhphgpbmbfokmhbpeh".to_string()),
        );
    }

    #[test]
    fn canon_extension_rejects_port() {
        assert!(canonicalize_origin("chrome-extension://abc:1234").is_none());
    }

    #[test]
    fn canon_rejects_prefix_lookalike_scheme() {
        // exact scheme match required — `chrome-extension-malicious` must NOT collapse
        // into a canonical form that aliases a real chrome-extension origin.
        assert!(canonicalize_origin("chrome-extension-malicious://abc").is_none());
    }

    #[test]
    fn canon_rejects_unknown_scheme() {
        assert!(canonicalize_origin("file:///etc/passwd").is_none());
        assert!(canonicalize_origin("data:text/html,hi").is_none());
        assert!(canonicalize_origin("javascript:alert(1)").is_none());
    }

    #[test]
    fn canon_is_idempotent() {
        let cases = [
            "https://nulo.sh",
            "https://nulo.sh:8443",
            "chrome-extension://abc",
        ];
        for c in cases {
            let once = canonicalize_origin(c).unwrap();
            let twice = canonicalize_origin(&once).unwrap();
            assert_eq!(once, twice, "non-idempotent for {c}");
        }
    }

    #[test]
    fn canon_rejects_garbage() {
        assert!(canonicalize_origin("").is_none());
        assert!(canonicalize_origin("not a url").is_none());
        assert!(canonicalize_origin("//nulo.sh").is_none());
    }
}
