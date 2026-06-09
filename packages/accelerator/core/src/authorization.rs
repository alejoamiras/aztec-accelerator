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

/// An origin string guaranteed canonical (RFC 6454) **by construction**.
///
/// The only ways to build one run [`canonicalize_origin`], so a `CanonicalOrigin` can never
/// hold a non-canonical value — the invariant the old comment-only "input is already canonical"
/// contract tried (and could not enforce) to express. Compares/serializes as its inner string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalOrigin(String);

impl CanonicalOrigin {
    /// Canonicalize `input`; `None` if it is not a valid/allowed RFC-6454 origin.
    pub fn parse(input: &str) -> Option<Self> {
        canonicalize_origin(input).map(Self)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for CanonicalOrigin {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for CanonicalOrigin {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl std::borrow::Borrow<str> for CanonicalOrigin {
    fn borrow(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for CanonicalOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl PartialEq<str> for CanonicalOrigin {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

/// Error: a string is not a canonical RFC-6454 origin.
#[derive(Debug, Clone)]
pub struct NonCanonicalOrigin(pub String);
impl std::fmt::Display for NonCanonicalOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a canonical RFC 6454 origin: {}", self.0)
    }
}
impl std::error::Error for NonCanonicalOrigin {}

impl TryFrom<String> for CanonicalOrigin {
    type Error = NonCanonicalOrigin;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        canonicalize_origin(&s)
            .map(Self)
            .ok_or(NonCanonicalOrigin(s))
    }
}

impl serde::Serialize for CanonicalOrigin {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
impl<'de> serde::Deserialize<'de> for CanonicalOrigin {
    /// Strict: a directly-deserialized `CanonicalOrigin` must ALREADY be canonical — no silent
    /// normalization. The lenient canonicalize-and-drop path lives only on the config Vec via
    /// `de_approved_origins`. (Use [`CanonicalOrigin::parse`]/`TryFrom` when you WANT canonicalization.)
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        match canonicalize_origin(&s) {
            Some(canon) if canon == s => Ok(Self(s)),
            _ => Err(<D::Error as serde::de::Error>::custom(format!(
                "not an already-canonical RFC 6454 origin: {s:?}"
            ))),
        }
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
    /// Both `origin` and `approved_origins` are [`CanonicalOrigin`], so canonicality is
    /// guaranteed by the type — no comment-only precondition, no bypassable ingress.
    pub fn is_approved(origin: &CanonicalOrigin, approved_origins: &[CanonicalOrigin]) -> bool {
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

    /// Build a `CanonicalOrigin` for tests (panics if the literal isn't canonical).
    fn co(s: &str) -> CanonicalOrigin {
        CanonicalOrigin::parse(s).expect("canonical test origin")
    }

    #[test]
    fn is_approved_checks_both() {
        let approved = vec![co("https://playground.aztec-accelerator.dev")];
        assert!(AuthorizationManager::is_approved(
            &co("http://localhost:5173"),
            &approved
        ));
        assert!(AuthorizationManager::is_approved(
            &co("https://playground.aztec-accelerator.dev"),
            &approved
        ));
        assert!(!AuthorizationManager::is_approved(
            &co("https://evil.com"),
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

    // ─── CanonicalOrigin newtype (F-02) ─────────────────────────────────

    #[test]
    fn canonical_origin_parse_and_str() {
        let o = CanonicalOrigin::parse("HTTPS://NULO.SH:443").unwrap();
        assert_eq!(o.as_str(), "https://nulo.sh");
        assert_eq!(o.to_string(), "https://nulo.sh");
        assert!(o == *"https://nulo.sh"); // PartialEq<str>
        assert!(CanonicalOrigin::parse("not a url").is_none());
    }

    #[test]
    fn canonical_origin_serde_roundtrip_and_strict() {
        let o = CanonicalOrigin::parse("https://nulo.sh").unwrap();
        // serializes as the inner canonical string
        assert_eq!(serde_json::to_string(&o).unwrap(), "\"https://nulo.sh\"");
        // an ALREADY-canonical string deserializes 1:1
        let de: CanonicalOrigin = serde_json::from_str("\"https://nulo.sh\"").unwrap();
        assert_eq!(de.as_str(), "https://nulo.sh");
        // STRICT: a non-canonical-but-fixable string is REJECTED — no silent normalization. The
        // lenient canonicalize+drop path is `de_approved_origins`, used only by the config Vec.
        assert!(serde_json::from_str::<CanonicalOrigin>("\"HTTPS://NULO.SH:443\"").is_err());
        // truly-invalid input is rejected too
        assert!(serde_json::from_str::<CanonicalOrigin>("\"not a url\"").is_err());
        assert!(serde_json::from_str::<CanonicalOrigin>("\"https://x.com/admin\"").is_err());
    }

    #[test]
    fn canonical_origin_rejects_special_origins() {
        // Origin: null, blob:, javascript:, file:, data: — none are tuple/extension origins
        for bad in [
            "null",
            "blob:https://x.com",
            "javascript:alert(1)",
            "file:///x",
            "data:text/html,x",
        ] {
            assert!(CanonicalOrigin::parse(bad).is_none(), "should reject {bad}");
        }
    }

    #[test]
    fn canonical_origin_idn_punycode_no_homograph_collision() {
        // A Unicode/IDN host normalizes to punycode (xn--…), distinct from the ASCII lookalike,
        // so a homograph cannot alias an approved ASCII origin.
        let ascii = CanonicalOrigin::parse("https://example.com").unwrap();
        if let Some(u) = CanonicalOrigin::parse("https://ex\u{00e4}mple.com") {
            assert!(
                u.as_str().starts_with("https://xn--"),
                "expected punycode, got {}",
                u.as_str()
            );
            assert_ne!(u, ascii, "homograph must NOT collide with ASCII origin");
        }
    }

    #[test]
    fn canonical_origin_stable_on_odd_but_parseable_hosts() {
        // port 0 is non-default → preserved; canonicalization stays idempotent on odd inputs
        // (percent-encoded host, IPv6 zone-id) whatever url::Url decides for them.
        if let Some(p0) = CanonicalOrigin::parse("https://x.com:0") {
            assert_eq!(p0.as_str(), "https://x.com:0");
        }
        for odd in [
            "https://x.com:0",
            "https://[::1]:5173",
            "https://ex%41mple.com",
            "https://[fe80::1%25eth0]",
        ] {
            if let Some(once) = CanonicalOrigin::parse(odd) {
                let twice = CanonicalOrigin::parse(once.as_str()).expect("re-parse of canonical");
                assert_eq!(once, twice, "non-idempotent for {odd}");
            }
        }
    }
}
