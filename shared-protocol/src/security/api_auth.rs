use serde::{Deserialize, Serialize};

/// API roles for route-level authorization in the native-chain API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApiRole {
    /// Full administrative access: mine blocks, manage oracle config, manage bridge, etc.
    Operator,
    /// Can submit contribution commits and reveals.
    Contributor,
    /// Can submit oracle attestations.
    Oracle,
    /// Read-only access to all endpoints.
    Reader,
}

impl ApiRole {
    /// Returns `true` if `self` satisfies the `required` role.
    ///
    /// Operator satisfies all roles. Each other role satisfies itself and Reader.
    pub fn satisfies(self, required: ApiRole) -> bool {
        match required {
            ApiRole::Reader => true,
            ApiRole::Contributor => matches!(self, ApiRole::Operator | ApiRole::Contributor),
            ApiRole::Oracle => matches!(self, ApiRole::Operator | ApiRole::Oracle),
            ApiRole::Operator => self == ApiRole::Operator,
        }
    }
}

impl std::fmt::Display for ApiRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiRole::Operator => write!(f, "operator"),
            ApiRole::Contributor => write!(f, "contributor"),
            ApiRole::Oracle => write!(f, "oracle"),
            ApiRole::Reader => write!(f, "reader"),
        }
    }
}

/// Caller identity claims bound to an authorization token.
/// Enables ownership-scoped access control for private channels (wallet:<addr>, rewards:<id>).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Identity {
    /// Wallet address owned by the caller.
    pub wallet_address: Option<String>,
    /// Contributor or station identifier (e.g., "station-123" or participant ID).
    pub contributor_id: Option<String>,
    /// Oracle node identifier.
    pub oracle_node_id: Option<String>,
}

impl Identity {
    pub fn new(
        wallet_address: Option<String>,
        contributor_id: Option<String>,
        oracle_node_id: Option<String>,
    ) -> Self {
        Self {
            wallet_address,
            contributor_id,
            oracle_node_id,
        }
    }

    pub fn wallet(wallet_address: impl Into<String>) -> Self {
        Self {
            wallet_address: Some(wallet_address.into()),
            ..Self::default()
        }
    }

    pub fn contributor(contributor_id: impl Into<String>) -> Self {
        Self {
            contributor_id: Some(contributor_id.into()),
            ..Self::default()
        }
    }

    pub fn oracle_node(oracle_node_id: impl Into<String>) -> Self {
        Self {
            oracle_node_id: Some(oracle_node_id.into()),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.wallet_address.is_none()
            && self.contributor_id.is_none()
            && self.oracle_node_id.is_none()
    }
}

/// Complete authorization principal: role + optional identity claims.
/// Replaces the old `token -> ApiRole` model to support owner-scoped websocket subscriptions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthPrincipal {
    /// The caller's role (determines baseline access level).
    pub role: ApiRole,
    /// Optional identity claims (enables ownership checks for private channels).
    pub identity: Option<Identity>,
}

impl AuthPrincipal {
    /// Create a principal with only a role (backward-compatible for public access).
    pub fn with_role(role: ApiRole) -> Self {
        Self {
            role,
            identity: None,
        }
    }

    /// Create a principal with role and identity claims.
    pub fn with_identity(role: ApiRole, identity: Identity) -> Self {
        Self {
            role,
            identity: (!identity.is_empty()).then_some(identity),
        }
    }

    /// Check if caller role satisfies the required role.
    pub fn satisfies_role(&self, required: ApiRole) -> bool {
        self.role.satisfies(required)
    }
}


// ---------------------------------------------------------------------------
// Host and Origin validation (#2136)
// ---------------------------------------------------------------------------

/// Host names a request may address this node by when nothing else is
/// configured. A DNS-rebinding page resolves its own name to this node's
/// address; the `Host` header then carries the attacker's name, which is not
/// in this list.
pub const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]", "::1"];

/// The allowlists a request's `Host` and `Origin` are checked against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginPolicy {
    /// Lower-case host names without port. Never `*`.
    pub allowed_hosts: Vec<String>,
    /// Normalised origins (`scheme://host[:port]`, lower-case, no trailing
    /// slash). Never `*`, never `null`. CORS is set from this same list, so an
    /// origin that is not here is refused before any handler runs rather than
    /// merely denied a CORS header.
    pub allowed_origins: Vec<String>,
    /// Whether a request carrying no `Origin` at all may proceed. `true` for
    /// non-browser clients (CLI, curl, node-to-node), which never send one.
    /// Browser-originated requests to admin methods require an allowlisted
    /// `Origin` regardless.
    pub allow_no_origin: bool,
}

/// Why a configured policy was refused. A wildcard is not a configuration
/// mistake to warn about: it is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginPolicyError {
    Wildcard { field: &'static str, value: String },
    BadOrigin(String),
    EmptyHost,
}

impl std::fmt::Display for OriginPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wildcard { field, value } => write!(f, "{field} entry `{value}` is a wildcard; allowlists are explicit"),
            Self::BadOrigin(o) => write!(f, "origin `{o}` is not `http(s)://host[:port]`"),
            Self::EmptyHost => write!(f, "empty host entry"),
        }
    }
}

impl std::error::Error for OriginPolicyError {}

/// The host name of a `Host` header value: port stripped, lower-cased.
/// `[::1]:8556` → `[::1]`, `Localhost:8556` → `localhost`, `::1` → `::1`.
pub fn host_name(host_header: &str) -> String {
    let h = host_header.trim();
    let without_port = if let Some(end) = h.strip_prefix('[').and_then(|_| h.find(']')) {
        &h[..=end]
    } else if h.matches(':').count() == 1 {
        h.split(':').next().unwrap_or(h)
    } else {
        h
    };
    without_port.to_ascii_lowercase()
}

/// Normalise an `Origin` value for allowlist comparison.
pub fn normalise_origin(origin: &str) -> String {
    origin.trim().trim_end_matches('/').to_ascii_lowercase()
}

impl OriginPolicy {
    /// Loopback hosts, no browser origins, non-browser clients allowed.
    pub fn loopback() -> Self {
        Self {
            allowed_hosts: LOOPBACK_HOSTS.iter().map(|h| h.to_string()).collect(),
            allowed_origins: Vec::new(),
            allow_no_origin: true,
        }
    }

    /// Loopback defaults plus explicitly configured host names and origins.
    /// Refuses any wildcard and any origin that is not `http(s)://…`.
    pub fn build<H, O>(extra_hosts: H, origins: O, allow_no_origin: bool) -> Result<Self, OriginPolicyError>
    where
        H: IntoIterator,
        H::Item: AsRef<str>,
        O: IntoIterator,
        O::Item: AsRef<str>,
    {
        let mut policy = Self::loopback();
        policy.allow_no_origin = allow_no_origin;
        for host in extra_hosts {
            let raw = host.as_ref().trim();
            if raw.is_empty() {
                return Err(OriginPolicyError::EmptyHost);
            }
            if raw.contains('*') {
                return Err(OriginPolicyError::Wildcard { field: "allowed_hosts", value: raw.to_string() });
            }
            let name = host_name(raw);
            if !policy.allowed_hosts.contains(&name) {
                policy.allowed_hosts.push(name);
            }
        }
        for origin in origins {
            let raw = origin.as_ref().trim();
            if raw.contains('*') {
                return Err(OriginPolicyError::Wildcard { field: "allowed_origins", value: raw.to_string() });
            }
            let norm = normalise_origin(raw);
            let ok = (norm.starts_with("http://") || norm.starts_with("https://"))
                && norm.len() > "https://".len()
                && !norm[norm.find("://").unwrap_or(0) + 3..].contains('/')
                && norm != "null";
            if !ok {
                return Err(OriginPolicyError::BadOrigin(raw.to_string()));
            }
            if !policy.allowed_origins.contains(&norm) {
                policy.allowed_origins.push(norm);
            }
        }
        Ok(policy)
    }

    pub fn host_allowed(&self, host_header: &str) -> bool {
        let name = host_name(host_header);
        !name.is_empty() && self.allowed_hosts.contains(&name)
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        let norm = normalise_origin(origin);
        !norm.is_empty() && self.allowed_origins.contains(&norm)
    }
}

/// What the request presented, as the transport layer saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestOrigin<'a> {
    /// The raw `Host` header (or the server's configured host when a client
    /// sent none — no browser ever omits it).
    pub host: Option<&'a str>,
    /// The raw `Origin` header, if any.
    pub origin: Option<&'a str>,
    /// `true` when the request carries `Origin` or any `Sec-Fetch-*` header —
    /// headers only a browser sets and a page cannot forge or remove.
    pub browser_originated: bool,
    /// The method is administrative (`RPC_SURFACE.md` class `admin`).
    pub admin_method: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginError {
    /// No `Host` at all.
    MissingHost,
    /// `Host` names something this node is not (the DNS-rebinding signature).
    HostNotAllowed(String),
    /// A browser-supplied `Origin` outside the allowlist.
    OriginNotAllowed(String),
    /// A browser-originated request to an admin method without an `Origin`,
    /// or any request without one when `allow_no_origin` is off.
    OriginRequired,
}

impl OriginError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingHost | Self::HostNotAllowed(_) => "HOST_NOT_ALLOWED",
            Self::OriginNotAllowed(_) => "ORIGIN_NOT_ALLOWED",
            Self::OriginRequired => "ORIGIN_REQUIRED",
        }
    }
}

/// The rule. Host first (every request), then Origin (browser requests):
/// a `Host` outside the allowlist is refused even though the packet reached
/// the socket — that is the DNS-rebinding defence; an `Origin` outside the
/// allowlist is refused *before the handler executes*, since withholding a
/// CORS header only stops the browser reading the response, not the request
/// running. Admin methods reached from a browser must carry an allowlisted
/// `Origin`; non-browser clients (no `Origin`, no `Sec-Fetch-*`) proceed when
/// `allow_no_origin` permits.
pub fn check_origin(req: &RequestOrigin<'_>, policy: &OriginPolicy) -> Result<(), OriginError> {
    let host = req.host.map(str::trim).filter(|h| !h.is_empty()).ok_or(OriginError::MissingHost)?;
    if !policy.host_allowed(host) {
        return Err(OriginError::HostNotAllowed(host_name(host)));
    }
    match req.origin.map(str::trim).filter(|o| !o.is_empty()) {
        Some(origin) => {
            if policy.origin_allowed(origin) {
                Ok(())
            } else {
                Err(OriginError::OriginNotAllowed(normalise_origin(origin)))
            }
        }
        None => {
            if req.admin_method && req.browser_originated {
                Err(OriginError::OriginRequired)
            } else if policy.allow_no_origin {
                Ok(())
            } else {
                Err(OriginError::OriginRequired)
            }
        }
    }
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    fn policy() -> OriginPolicy {
        OriginPolicy::build(["node.example", "N5:8556"], ["https://app.example", "http://localhost:3000/"], true)
            .unwrap()
    }

    fn req<'a>(host: Option<&'a str>, origin: Option<&'a str>, browser: bool, admin: bool) -> RequestOrigin<'a> {
        RequestOrigin { host, origin, browser_originated: browser, admin_method: admin }
    }

    #[test]
    fn defaults_are_loopback_and_never_a_wildcard() {
        let p = OriginPolicy::loopback();
        for h in ["localhost", "localhost:8556", "127.0.0.1:8556", "[::1]:8556", "LOCALHOST"] {
            assert!(p.host_allowed(h), "{h}");
        }
        assert!(!p.host_allowed("*"));
        assert!(!p.host_allowed("evil.example"));
        assert!(p.allowed_origins.is_empty());
        assert!(matches!(
            OriginPolicy::build(["*"], Vec::<&str>::new(), true),
            Err(OriginPolicyError::Wildcard { field: "allowed_hosts", .. })
        ));
        assert!(matches!(
            OriginPolicy::build(Vec::<&str>::new(), ["*"], true),
            Err(OriginPolicyError::Wildcard { field: "allowed_origins", .. })
        ));
        assert!(matches!(
            OriginPolicy::build(Vec::<&str>::new(), ["https://*.example"], true),
            Err(OriginPolicyError::Wildcard { .. })
        ));
        assert!(matches!(OriginPolicy::build(Vec::<&str>::new(), ["null"], true), Err(OriginPolicyError::BadOrigin(_))));
        assert!(matches!(OriginPolicy::build(Vec::<&str>::new(), ["app.example"], true), Err(OriginPolicyError::BadOrigin(_))));
        assert!(matches!(OriginPolicy::build(Vec::<&str>::new(), ["https://a.example/path"], true), Err(OriginPolicyError::BadOrigin(_))));
    }

    #[test]
    fn configured_hosts_and_origins_are_normalised() {
        let p = policy();
        assert!(p.host_allowed("node.example:8556"));
        assert!(p.host_allowed("n5"));
        assert!(p.origin_allowed("https://APP.example"));
        assert!(p.origin_allowed("http://localhost:3000"));
        assert!(!p.origin_allowed("https://app.example.evil"));
        assert!(!p.origin_allowed("null"));
    }

    #[test]
    fn rebinding_host_is_rejected_even_though_it_reached_the_socket() {
        let p = policy();
        assert_eq!(check_origin(&req(Some("attacker.example:8556"), None, false, false), &p), Err(OriginError::HostNotAllowed("attacker.example".into())));
        assert_eq!(check_origin(&req(None, None, false, false), &p), Err(OriginError::MissingHost));
        assert_eq!(check_origin(&req(Some("127.0.0.1:8556"), None, false, false), &p), Ok(()));
    }

    #[test]
    fn disallowed_origin_is_rejected_and_allowlisted_origin_passes() {
        let p = policy();
        assert_eq!(
            check_origin(&req(Some("localhost:8556"), Some("https://evil.example"), true, false), &p),
            Err(OriginError::OriginNotAllowed("https://evil.example".into()))
        );
        assert_eq!(check_origin(&req(Some("localhost:8556"), Some("https://app.example"), true, false), &p), Ok(()));
        assert_eq!(check_origin(&req(Some("localhost:8556"), Some("https://app.example"), true, true), &p), Ok(()));
        assert_eq!(check_origin(&req(Some("localhost:8556"), Some("null"), true, false), &p), Err(OriginError::OriginNotAllowed("null".into())));
    }

    #[test]
    fn admin_method_from_a_browser_requires_an_origin_but_a_cli_does_not() {
        let p = policy();
        // Browser, same-origin navigation shape: Sec-Fetch-* present, no Origin.
        assert_eq!(check_origin(&req(Some("localhost:8556"), None, true, true), &p), Err(OriginError::OriginRequired));
        // Same shape on a public method proceeds.
        assert_eq!(check_origin(&req(Some("localhost:8556"), None, true, false), &p), Ok(()));
        // CLI / curl: no Origin, no Sec-Fetch-*.
        assert_eq!(check_origin(&req(Some("localhost:8556"), None, false, true), &p), Ok(()));
        let mut strict = policy();
        strict.allow_no_origin = false;
        assert_eq!(check_origin(&req(Some("localhost:8556"), None, false, false), &strict), Err(OriginError::OriginRequired));
    }

    #[test]
    fn host_name_strips_ports_and_keeps_bracketed_ipv6() {
        assert_eq!(host_name("Localhost:8556"), "localhost");
        assert_eq!(host_name("[::1]:8556"), "[::1]");
        assert_eq!(host_name("::1"), "::1");
        assert_eq!(host_name("127.0.0.1"), "127.0.0.1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_satisfies_all_roles() {
        assert!(ApiRole::Operator.satisfies(ApiRole::Operator));
        assert!(ApiRole::Operator.satisfies(ApiRole::Contributor));
        assert!(ApiRole::Operator.satisfies(ApiRole::Oracle));
        assert!(ApiRole::Operator.satisfies(ApiRole::Reader));
    }

    #[test]
    fn contributor_satisfies_contributor_and_reader() {
        assert!(ApiRole::Contributor.satisfies(ApiRole::Contributor));
        assert!(ApiRole::Contributor.satisfies(ApiRole::Reader));
        assert!(!ApiRole::Contributor.satisfies(ApiRole::Operator));
        assert!(!ApiRole::Contributor.satisfies(ApiRole::Oracle));
    }

    #[test]
    fn oracle_satisfies_oracle_and_reader() {
        assert!(ApiRole::Oracle.satisfies(ApiRole::Oracle));
        assert!(ApiRole::Oracle.satisfies(ApiRole::Reader));
        assert!(!ApiRole::Oracle.satisfies(ApiRole::Operator));
        assert!(!ApiRole::Oracle.satisfies(ApiRole::Contributor));
    }

    #[test]
    fn reader_only_satisfies_reader() {
        assert!(ApiRole::Reader.satisfies(ApiRole::Reader));
        assert!(!ApiRole::Reader.satisfies(ApiRole::Operator));
        assert!(!ApiRole::Reader.satisfies(ApiRole::Contributor));
        assert!(!ApiRole::Reader.satisfies(ApiRole::Oracle));
    }
}
