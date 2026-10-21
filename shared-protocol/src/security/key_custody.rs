//! Key-custody manifest and check (#3096).
//!
//! `docs/security/KEY_CUSTODY_MANIFEST.json` records, for every ceremony
//! identity, WHERE its private key lives: token serial, PIV slot, object
//! label, `CKA_ID`, host — and the public key the token must present. All of
//! it is public data; none of it is a secret. `wrfcoin ceremony custody
//! --check` reads every token a PKCS#11 module presents **without a PIN**
//! ([`crate::crypto::signer::enumerate_public_keys`] under
//! `--features hsm`) and compares. This module is the pure half: the manifest
//! shape, its validation, and the comparison, so the verdict is testable
//! without hardware and identical whatever produced the observation.

use serde::{Deserialize, Serialize};

/// The committed manifest (`docs/security/KEY_CUSTODY_MANIFEST.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustodyManifest {
    pub version: u32,
    pub entries: Vec<CustodyEntry>,
}

/// Where one identity's key lives. Public data only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] // secret-leak-ok: token serial, slot, labels, CKA_ID, host and PUBLIC key only
pub struct CustodyEntry {
    /// Ceremony identity: `producer_id`, oracle id, or `snapshot-<host>`.
    pub identity: String,
    /// `producer` | `oracle` | `snapshot` | `governance`.
    pub role: String,
    /// `CK_TOKEN_INFO.serialNumber`; `softhsm:<label>` for a SoftHSM token.
    pub token_serial: String,
    /// PIV slot (`9A`, `9C`, `9D`, `9E`) or `-` when not a PIV token.
    pub piv_slot: String,
    /// The public-key object's `CKA_LABEL`.
    pub public_label: String,
    /// Hex `CKA_ID` (`01`, `02`, `03`, …) or empty when the token exposes none.
    pub cka_id: String,
    /// Where the token is plugged in.
    pub host: String,
    /// Lowercase hex ed25519 public key the token must present. Empty means
    /// "not yet recorded": the entry is reported as unfilled, never as a
    /// match.
    #[serde(default)]
    pub public_key: String,
}

/// One public-key object as observed on a token, module-independent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] // secret-leak-ok: public data only
pub struct ObservedKey {
    pub token_serial: String,
    pub token_label: String,
    pub object_label: String,
    pub cka_id: Option<String>,
    pub public_key: String,
}

/// Why a manifest is not usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    UnsupportedVersion(u32),
    DuplicateIdentity(String),
    EmptyField { identity: String, field: &'static str },
    BadHex { identity: String, field: &'static str, value: String },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(f, "manifest version {v} is not supported"),
            Self::DuplicateIdentity(id) => write!(f, "identity {id:?} appears twice"),
            Self::EmptyField { identity, field } => {
                write!(f, "entry {identity:?}: {field} is empty")
            },
            Self::BadHex {
                identity,
                field,
                value,
            } => write!(f, "entry {identity:?}: {field} {value:?} is not hex"),
        }
    }
}

impl std::error::Error for ManifestError {}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

impl CustodyManifest {
    pub const VERSION: u32 = 1;

    /// Structural validation: version, unique identities, required fields,
    /// hex where hex is expected.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != Self::VERSION {
            return Err(ManifestError::UnsupportedVersion(self.version));
        }
        let mut seen = std::collections::BTreeSet::new();
        for e in &self.entries {
            if !seen.insert(e.identity.clone()) {
                return Err(ManifestError::DuplicateIdentity(e.identity.clone()));
            }
            for (field, value) in [
                ("identity", &e.identity),
                ("role", &e.role),
                ("token_serial", &e.token_serial),
                ("public_label", &e.public_label),
                ("host", &e.host),
            ] {
                if value.trim().is_empty() {
                    return Err(ManifestError::EmptyField {
                        identity: e.identity.clone(),
                        field,
                    });
                }
            }
            if !e.cka_id.is_empty() && !is_hex(&e.cka_id) {
                return Err(ManifestError::BadHex {
                    identity: e.identity.clone(),
                    field: "cka_id",
                    value: e.cka_id.clone(),
                });
            }
            if !e.public_key.is_empty() && !(is_hex(&e.public_key) && e.public_key.len() == 64) {
                return Err(ManifestError::BadHex {
                    identity: e.identity.clone(),
                    field: "public_key",
                    value: e.public_key.clone(),
                });
            }
        }
        Ok(())
    }
}

/// One line of the custody verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "finding", rename_all = "snake_case")]
pub enum CustodyFinding {
    /// The token presents exactly the recorded public key at the recorded place.
    Match { identity: String },
    /// The token presents a DIFFERENT key at the recorded place — the
    /// manifest names the wrong slot/object, or the key was rotated.
    Mismatch {
        identity: String,
        expected: String,
        found: String,
    },
    /// No token with the recorded serial is visible.
    TokenAbsent { identity: String, token_serial: String },
    /// The token is visible but has no object at the recorded `CKA_ID` / label.
    ObjectAbsent {
        identity: String,
        token_serial: String,
        cka_id: String,
        public_label: String,
    },
    /// The manifest entry has no recorded public key yet.
    Unfilled { identity: String },
    /// A visible key the manifest does not account for.
    Unexpected {
        token_serial: String,
        cka_id: Option<String>,
        object_label: String,
        public_key: String,
    },
}

/// The custody verdict: every manifest entry accounted for, plus visible
/// keys the manifest does not name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustodyReport {
    pub findings: Vec<CustodyFinding>,
}

impl CustodyReport {
    /// Compare `manifest` against what the tokens presented. Matching is by
    /// `(token_serial, cka_id)` when the manifest records a `CKA_ID`, else by
    /// `(token_serial, object label)`. Entries are resolved in manifest order
    /// and an observed object satisfies at most one entry, so two entries can
    /// never be "matched" by the same physical key.
    pub fn check(manifest: &CustodyManifest, observed: &[ObservedKey]) -> Self {
        let mut findings = Vec::with_capacity(manifest.entries.len());
        let mut claimed: Vec<bool> = vec![false; observed.len()];
        for entry in &manifest.entries {
            let serial = entry.token_serial.trim();
            let on_token: Vec<usize> = observed
                .iter()
                .enumerate()
                .filter(|(i, o)| !claimed[*i] && o.token_serial.trim() == serial)
                .map(|(i, _)| i)
                .collect();
            let token_present = observed.iter().any(|o| o.token_serial.trim() == serial);
            if !token_present {
                findings.push(CustodyFinding::TokenAbsent {
                    identity: entry.identity.clone(),
                    token_serial: entry.token_serial.clone(),
                });
                continue;
            }
            let wanted_id = entry.cka_id.trim().to_ascii_lowercase();
            let object = on_token.iter().copied().find(|&i| {
                let o = &observed[i];
                if !wanted_id.is_empty() {
                    o.cka_id
                        .as_deref()
                        .map(|id| id.trim().eq_ignore_ascii_case(&wanted_id))
                        .unwrap_or(false)
                } else {
                    o.object_label.trim() == entry.public_label.trim()
                }
            });
            let Some(i) = object else {
                findings.push(CustodyFinding::ObjectAbsent {
                    identity: entry.identity.clone(),
                    token_serial: entry.token_serial.clone(),
                    cka_id: entry.cka_id.clone(),
                    public_label: entry.public_label.clone(),
                });
                continue;
            };
            claimed[i] = true;
            if entry.public_key.is_empty() {
                findings.push(CustodyFinding::Unfilled {
                    identity: entry.identity.clone(),
                });
                continue;
            }
            let found = observed[i].public_key.trim().to_ascii_lowercase();
            if found == entry.public_key.trim().to_ascii_lowercase() {
                findings.push(CustodyFinding::Match {
                    identity: entry.identity.clone(),
                });
            } else {
                findings.push(CustodyFinding::Mismatch {
                    identity: entry.identity.clone(),
                    expected: entry.public_key.clone(),
                    found,
                });
            }
        }
        for (i, o) in observed.iter().enumerate() {
            if !claimed[i] {
                findings.push(CustodyFinding::Unexpected {
                    token_serial: o.token_serial.clone(),
                    cka_id: o.cka_id.clone(),
                    object_label: o.object_label.clone(),
                    public_key: o.public_key.clone(),
                });
            }
        }
        Self { findings }
    }

    /// `true` when nothing contradicts the manifest: no mismatch, no absent
    /// token or object. Unfilled and unexpected entries are reported but do
    /// not fail the check.
    pub fn is_consistent(&self) -> bool {
        !self.findings.iter().any(|f| {
            matches!(
                f,
                CustodyFinding::Mismatch { .. }
                    | CustodyFinding::TokenAbsent { .. }
                    | CustodyFinding::ObjectAbsent { .. }
            )
        })
    }

    pub fn matches(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| matches!(f, CustodyFinding::Match { .. }))
            .count()
    }

    pub fn unfilled(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| matches!(f, CustodyFinding::Unfilled { .. }))
            .count()
    }
}
