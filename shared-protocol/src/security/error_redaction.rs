//! #2138 — error-message redaction.
//!
//! A handler error crosses the trust boundary in exactly one shape:
//! [`PublicError`] — a numeric code, a **`&'static str`** message and a
//! correlation id. The full internal detail (paths, store names, versions,
//! host names, whatever a dependency chose to put in its `Display`) stays in
//! the [`ServerRecord`] that the caller logs against the same correlation id.
//! Support correlates the two; the client learns nothing but the class.
//!
//! `message` being `&'static str` is the mechanism, not a convention: a
//! `format!`ed internal value cannot be assigned to it, so an interpolation
//! bug is a compile error rather than a leak.
//!
//! [`redaction_findings`] is the scanner the CI test (`tests/error_redaction.rs`)
//! runs over every public shape: it finds filesystem paths, version strings,
//! host names and addresses, store/SQL fragments, and the secret-name segments
//! `scripts/check_secret_leak.py` uses. It is `pub` so a handler can assert the
//! same property on anything it is about to send.
//!
//! This module deliberately depends on nothing but `serde` and `uuid`: logging is
//! the caller's (the caller has the request context; this crate has no logger).

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The class of an internal failure — the only thing a client learns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorKind {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Unprocessable,
    RateLimited,
    Unavailable,
    /// Local state could not be read or written.
    Storage,
    /// A consensus operation (block production, state transition) failed.
    Consensus,
    /// A cryptographic check failed inside the node.
    Crypto,
    /// A remote service (oracle feed, peer, bridge) misbehaved.
    Upstream,
    Internal,
}

impl ErrorKind {
    /// Every kind, so a test can prove each one has a mapping.
    pub const ALL: [ErrorKind; 13] = [
        ErrorKind::BadRequest,
        ErrorKind::Unauthorized,
        ErrorKind::Forbidden,
        ErrorKind::NotFound,
        ErrorKind::Conflict,
        ErrorKind::Unprocessable,
        ErrorKind::RateLimited,
        ErrorKind::Unavailable,
        ErrorKind::Storage,
        ErrorKind::Consensus,
        ErrorKind::Crypto,
        ErrorKind::Upstream,
        ErrorKind::Internal,
    ];

    /// Stable numeric code: HTTP status × 10 for client classes, 50xx for node faults.
    pub const fn code(self) -> i32 {
        match self {
            ErrorKind::BadRequest => 4000,
            ErrorKind::Unauthorized => 4010,
            ErrorKind::Forbidden => 4030,
            ErrorKind::NotFound => 4040,
            ErrorKind::Conflict => 4090,
            ErrorKind::Unprocessable => 4220,
            ErrorKind::RateLimited => 4290,
            ErrorKind::Unavailable => 5030,
            ErrorKind::Storage => 5001,
            ErrorKind::Consensus => 5002,
            ErrorKind::Crypto => 5003,
            ErrorKind::Upstream => 5020,
            ErrorKind::Internal => 5000,
        }
    }

    pub const fn http_status(self) -> u16 {
        match self {
            ErrorKind::BadRequest => 400,
            ErrorKind::Unauthorized => 401,
            ErrorKind::Forbidden => 403,
            ErrorKind::NotFound => 404,
            ErrorKind::Conflict => 409,
            ErrorKind::Unprocessable => 422,
            ErrorKind::RateLimited => 429,
            ErrorKind::Unavailable => 503,
            ErrorKind::Upstream => 502,
            ErrorKind::Storage | ErrorKind::Consensus | ErrorKind::Crypto | ErrorKind::Internal => 500,
        }
    }

    /// The envelope `error.code` string the launch_api already uses for the class.
    pub const fn envelope_code(self) -> &'static str {
        match self {
            ErrorKind::BadRequest => "BAD_REQUEST",
            ErrorKind::Unauthorized => "UNAUTHORIZED",
            ErrorKind::Forbidden => "FORBIDDEN",
            ErrorKind::NotFound => "NOT_FOUND",
            ErrorKind::Conflict => "CONFLICT",
            ErrorKind::Unprocessable => "UNPROCESSABLE_ENTITY",
            ErrorKind::RateLimited => "TOO_MANY_REQUESTS",
            ErrorKind::Unavailable => "SERVICE_UNAVAILABLE",
            ErrorKind::Storage => "STORAGE_ERROR",
            ErrorKind::Consensus => "CONSENSUS_ERROR",
            ErrorKind::Crypto => "CRYPTO_ERROR",
            ErrorKind::Upstream => "UPSTREAM_ERROR",
            ErrorKind::Internal => "INTERNAL_SERVER_ERROR",
        }
    }

    /// The only text a client ever sees for this class. Static by type.
    pub const fn public_message(self) -> &'static str {
        match self {
            ErrorKind::BadRequest => "The request was malformed or failed validation",
            ErrorKind::Unauthorized => "Authentication is required for this method",
            ErrorKind::Forbidden => "This method is not permitted for the caller",
            ErrorKind::NotFound => "Not found",
            ErrorKind::Conflict => "The request conflicts with the current state",
            ErrorKind::Unprocessable => "The request was understood but cannot be applied",
            ErrorKind::RateLimited => "Rate limit exceeded. Please slow down.",
            ErrorKind::Unavailable => "This capability is disabled or unavailable on this node",
            ErrorKind::Storage => "The node could not read or write its local state",
            ErrorKind::Consensus => "The node could not apply the requested consensus operation",
            ErrorKind::Crypto => "A cryptographic check failed",
            ErrorKind::Upstream => "An upstream service did not respond correctly",
            ErrorKind::Internal => "The node encountered an internal error",
        }
    }

    /// Node faults are logged at error level; client classes at warn.
    pub const fn is_server_fault(self) -> bool {
        self.code() >= 5000
    }
}

/// A handler failure with its full detail. Never serialised; `to_public` is the
/// only way across the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalError {
    pub kind: ErrorKind,
    pub detail: String,
}

impl InternalError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// Wrap any error value with its class; the `Display` text is the detail.
    pub fn wrap(kind: ErrorKind, error: &dyn fmt::Display) -> Self {
        Self::new(kind, error.to_string())
    }
}

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for InternalError {}

/// What the client receives. `message` is `&'static str`: nothing formatted at
/// runtime can be placed in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicError {
    pub code: i32,
    pub message: &'static str,
    #[serde(serialize_with = "serialize_uuid")]
    pub correlation_id: Uuid,
}

/// Hyphenated form; `uuid` is compiled without its `serde` feature here.
fn serialize_uuid<S: serde::Serializer>(id: &Uuid, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&id.to_string())
}

/// What the server logs, keyed by the same correlation id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRecord {
    pub correlation_id: Uuid,
    pub kind: ErrorKind,
    pub detail: String,
}

impl fmt::Display for ServerRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "handler error correlation_id={} kind={:?} code={} detail={}",
            self.correlation_id,
            self.kind,
            self.kind.code(),
            self.detail
        )
    }
}

/// Both halves of one redaction. The caller sends `public` and logs `record`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    pub public: PublicError,
    pub record: ServerRecord,
}

/// Convert an internal error to its public shape under a fresh correlation id.
pub fn to_public(error: &InternalError) -> Redacted {
    to_public_with_id(error, Uuid::new_v4())
}

/// Same, with a caller-chosen id (a request id already assigned upstream, or a
/// fixed id in tests).
pub fn to_public_with_id(error: &InternalError, correlation_id: Uuid) -> Redacted {
    Redacted {
        public: PublicError {
            code: error.kind.code(),
            message: error.kind.public_message(),
            correlation_id,
        },
        record: ServerRecord {
            correlation_id,
            kind: error.kind,
            detail: error.detail.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// Redaction scanner
// ---------------------------------------------------------------------------

/// Something a public response must never carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leak {
    FilesystemPath(String),
    Version(String),
    HostOrAddress(String),
    StoreFragment(String),
    SecretName(String),
}

const PATH_MARKERS: &[&str] = &[
    "/home/", "/build/", "/usr/", "/var/", "/tmp/", "/etc/", "/opt/", "/root/", "/mnt/", "/srv/", "C:\\", "src/",
    "target/", ".cargo/",
];
const HOST_SUFFIXES: &[&str] = &[".local", ".internal", ".lan", ".home", ".arpa", ".ts.net"];
const KNOWN_HOSTS: &[&str] = &["gh-runner", "n5pro", "n5-pro", "mango-mini", "ct101", "ct102"];
const STORE_FRAGMENTS: &[&str] = &[
    "select ", "insert into", "delete from", " where ", "redb", "sqlite", "postgres", "rocksdb", "bincode", "table ",
    "wal entry", "checkpoint envelope",
];
/// Identifier segments `scripts/check_secret_leak.py` treats as secret names.
const SECRET_SEGMENTS: &[&str] = &[
    "secret", "secrets", "priv", "private", "privkey", "seed", "mnemonic", "passphrase", "token", "jwt", "signing", "sk",
    "keypair", "credential", "pin",
];

fn is_numeric_dotted(token: &str, parts: usize) -> bool {
    let t = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let t = t.strip_prefix('v').unwrap_or(t);
    let segments: Vec<&str> = t.split('.').collect();
    segments.len() == parts && segments.iter().all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
}

/// Split an identifier into lower-case segments on `_`, `-`, `.` and camelCase
/// boundaries, the way the secret-leak scanner does.
fn identifier_segments(word: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = word.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
            continue;
        }
        let boundary = i > 0
            && c.is_ascii_uppercase()
            && chars[i - 1].is_ascii_alphanumeric()
            && !chars[i - 1].is_ascii_uppercase();
        if boundary && !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
        current.push(c.to_ascii_lowercase());
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

/// Every leak class present in `text`. Empty means the text may be sent.
pub fn redaction_findings(text: &str) -> Vec<Leak> {
    let mut findings = Vec::new();
    let lower = text.to_ascii_lowercase();
    for fragment in STORE_FRAGMENTS {
        if lower.contains(fragment) {
            findings.push(Leak::StoreFragment((*fragment).trim().to_string()));
        }
    }
    for token in text.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '`' || c == ',' || c == ';') {
        if token.is_empty() {
            continue;
        }
        let trimmed = token.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | ':' | '.'));
        if PATH_MARKERS.iter().any(|m| token.contains(m)) || (token.contains(".rs:") || token.ends_with(".rs")) {
            findings.push(Leak::FilesystemPath(token.to_string()));
            continue;
        }
        if is_numeric_dotted(trimmed, 4) {
            findings.push(Leak::HostOrAddress(trimmed.to_string()));
            continue;
        }
        if is_numeric_dotted(trimmed, 3) {
            findings.push(Leak::Version(trimmed.to_string()));
            continue;
        }
        let lower_token = trimmed.to_ascii_lowercase();
        if HOST_SUFFIXES.iter().any(|s| lower_token.ends_with(s))
            || KNOWN_HOSTS.iter().any(|h| lower_token == *h || lower_token.starts_with(&format!("{h}:")))
            || lower_token.starts_with("hostname=")
        {
            findings.push(Leak::HostOrAddress(trimmed.to_string()));
            continue;
        }
        for segment in identifier_segments(trimmed) {
            if SECRET_SEGMENTS.contains(&segment.as_str()) {
                findings.push(Leak::SecretName(trimmed.to_string()));
                break;
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_distinct_code_status_and_clean_static_message() {
        let mut codes = std::collections::HashSet::new();
        for kind in ErrorKind::ALL {
            assert!(codes.insert(kind.code()), "duplicate code for {kind:?}");
            assert!((400..600).contains(&kind.http_status()));
            assert!(redaction_findings(kind.public_message()).is_empty(), "{kind:?}");
            assert!(redaction_findings(kind.envelope_code()).is_empty(), "{kind:?}");
            assert_eq!(kind.is_server_fault(), kind.code() >= 5000);
        }
    }

    #[test]
    fn to_public_keeps_detail_server_side_only() {
        let id = Uuid::nil();
        let e = InternalError::new(ErrorKind::Storage, "redb table blocks at /home/x/wrfcoin.redb: io error");
        let r = to_public_with_id(&e, id);
        assert_eq!(r.public.code, 5001);
        assert_eq!(r.public.message, ErrorKind::Storage.public_message());
        assert_eq!(r.public.correlation_id, id);
        assert!(r.record.to_string().contains("/home/x/wrfcoin.redb"));
        assert!(r.record.to_string().contains(&id.to_string()));
        let json = serde_json::to_string(&r.public).unwrap();
        assert!(redaction_findings(&json).is_empty(), "{json}");
        assert!(!json.contains("redb"));
    }

    #[test]
    fn scanner_finds_each_leak_class_and_nothing_in_clean_prose() {
        let leaky = "failed at /home/example/wrfcoin/core4/native-chain/src/storage.rs:1234 (wrfcoin-native-chain 0.1.0) \
                     on gh-runner 192.0.2.10 private_key seedPhrase SELECT * FROM blocks WHERE h=1";
        let found = redaction_findings(leaky);
        assert!(found.iter().any(|l| matches!(l, Leak::FilesystemPath(_))), "{found:?}");
        assert!(found.iter().any(|l| matches!(l, Leak::Version(_))), "{found:?}");
        assert!(found.iter().any(|l| matches!(l, Leak::HostOrAddress(_))), "{found:?}");
        assert!(found.iter().any(|l| matches!(l, Leak::StoreFragment(_))), "{found:?}");
        assert!(found.iter().any(|l| matches!(l, Leak::SecretName(_))), "{found:?}"); // secret-leak-ok: asserts the scanner's own SecretName variant on fixture text
        assert!(redaction_findings("The request conflicts with the current state").is_empty());
        assert!(redaction_findings("{\"ok\":false,\"error\":{\"code\":\"NOT_FOUND\",\"message\":\"Not found\"}}").is_empty());
        // The secret scanner matches segments, not substrings: `task_id` is not `sk`.
        assert!(redaction_findings("task_id privacy skip").is_empty());
        assert!(!redaction_findings("api_token").is_empty());
        assert!(!redaction_findings("SigningKey").is_empty());
    }
}
