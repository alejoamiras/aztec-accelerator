use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

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
        let mut pending = self.pending.lock().unwrap();
        let is_first = !pending.contains_key(origin);
        if is_first && pending.len() >= MAX_PENDING_ORIGINS {
            return Err("too many pending authorization requests");
        }
        pending.entry(origin.to_string()).or_default().push(tx);
        Ok((rx, is_first))
    }

    /// Resolve all pending requests for `origin` with the given decision.
    pub fn resolve(&self, origin: &str, decision: AuthDecision) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(senders) = pending.remove(origin) {
            for tx in senders {
                let _ = tx.send(decision);
            }
        }
    }

    /// Returns true for localhost origins that should be auto-approved.
    pub fn is_auto_approved(origin: &str) -> bool {
        // Parse the origin to extract the host
        if let Some(host_part) = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
        {
            // Handle IPv6 bracket notation: [::1]:5173 → [::1]
            let host = if host_part.starts_with('[') {
                // IPv6: take everything up to and including the closing bracket
                host_part
                    .find(']')
                    .map(|i| &host_part[..=i])
                    .unwrap_or(host_part)
            } else {
                // IPv4 or hostname: take everything before the port separator
                host_part.split(':').next().unwrap_or(host_part)
            };
            matches!(host, "localhost" | "127.0.0.1" | "[::1]")
        } else {
            false
        }
    }

    /// Returns true if the origin is approved (auto-approved or in the approved list).
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
}
