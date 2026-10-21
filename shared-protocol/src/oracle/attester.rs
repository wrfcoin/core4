//! Oracle attester service skeleton — discovery, signing, submission (#2464).
//!
//! The chain is live and 5,578 accepted observations have earned zero WRF
//! because `oracle_set` is empty and no service ever attests. This module is
//! the library half of the path that starts paying contributors: it discovers
//! observations awaiting attestation, signs an attestation with the production
//! oracle signer role added in #2472, and renders the exact submission body the
//! node accepts.
//!
//! # Two hash contracts, not one
//!
//! An attester touches **two** distinct hashes, and conflating them is the
//! failure mode this module is written to prevent — a mismatch in either
//! produces attestations the chain rejects, or worse, attestations that certify
//! a reading the commit never bound.
//!
//! 1. **The commit id** — [`Transaction::compute_commit_id`], SHA3-256 over the
//!    5-field preimage `data || nonce || station_id || timestamp ||
//!    contribution_type`, with `data` serialized as sorted-key JSON. This is the
//!    cross-language contract the testnet collector implements
//!    (`infra/scripts/weather-station-onboarding.js`) and it is what an attester
//!    **validates**: recomputing it is the only way to know the reading being
//!    attested is the reading the contributor committed to. See
//!    [`PendingObservation::verify_commit_id`].
//!
//! 2. **The attestation signing message** —
//!    [`OracleAttestationPayload::canonical_signing_message`], a
//!    domain-separated message (`wrfcoin-sign-v1|wrfcoin.oracle_attestation.v1|…`)
//!    over a 6-field digest. This is what an attester **signs**, and it is *not*
//!    the commit-id recipe. `submit_oracle_attestation` verifies exactly this
//!    message; signing the commit-id preimage instead yields a signature the
//!    node rejects with "oracle attestation signature verification failed".
//!
//! This module keeps them in their respective places by construction: the
//! commit id is only ever *recomputed and compared*, never signed, and the
//! signing message is only ever obtained from the protocol's own payload type
//! rather than rebuilt here.
//!
//! # Scope
//!
//! Deliberately no HTTP. [`AttestationSubmission`] renders the request body and
//! the caller performs the POST, matching the precedent set by the
//! `oracle_attest_sign` example. That keeps the whole discover → validate →
//! sign pipeline unit-testable without a node.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::blockchain::transaction::{OracleAttestationPayload, Transaction};
use crate::crypto::signature::PublicKey;
use crate::crypto::signer::{BlockSigner, ResolvedSigner, SignerEnv, load_oracle_signer};
use crate::oracle::attestation::AttestationSet;

/// The `contribution_type` core4 hardcodes for both commit and reveal when
/// deriving a commit id. Held as a constant so a caller cannot silently pass a
/// different string and produce a commit id that never matches the chain's.
pub const CONTRIBUTION_TYPE: &str = "contribution";

/// An observation discovered as awaiting attestation, carrying the revealed
/// reading needed to validate it.
///
/// `commit_id` is what the chain tracks; every other field is the preimage that
/// must reproduce it. Holding both is what makes
/// [`verify_commit_id`](Self::verify_commit_id) possible — an attester that
/// only had the commit id could not tell whether the reading it is about to
/// certify is the committed one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingObservation {
    /// The commit id as recorded on chain.
    pub commit_id: String,
    pub station_id: String,
    pub nonce: String,
    pub timestamp: i64,
    /// The revealed reading. Must be a JSON **object** — core4 rejects a
    /// non-object payload with 422 "payload must be a JSON object".
    pub data: serde_json::Value,
    /// Defaults to [`CONTRIBUTION_TYPE`]; carried explicitly so a future
    /// contribution kind cannot be introduced by accident.
    #[serde(default = "default_contribution_type")]
    pub contribution_type: String,
}

fn default_contribution_type() -> String {
    CONTRIBUTION_TYPE.to_string()
}

/// Why an observation could not be attested.
#[derive(Debug, Clone, PartialEq)]
pub enum AttestError {
    /// The reading does not reproduce the on-chain commit id. Refusing is the
    /// whole point: signing anyway would certify a reading the contributor
    /// never committed to.
    CommitIdMismatch { expected: String, recomputed: String },
    /// The revealed reading is not a JSON object.
    DataNotAnObject,
    /// Confidence outside `[0, 1]`; the node rejects these with 422.
    // determinism-ok: mirrors OracleAttestationPayload::confidence (the protocol wire type); carries a rejected value back to the caller, never arithmetic or consensus input
    ConfidenceOutOfRange(f64),
    /// The node rejects a missing or blank nonce with 422 "nonce is required".
    MissingNonce,
    /// The resolved oracle key is a key belonging to another signing role.
    /// See [`Attester::from_env`].
    CustodyViolation { oracle_id: String, role: String },
    /// The forbidden-role inventory could not account for every non-oracle
    /// role, so distinctness cannot be asserted. Fails closed: an unverifiable
    /// custody rule reads as a pass, which is worse than a refusal.
    IncompleteRoleInventory { missing: Vec<String> },
    /// Signing failed at the signer boundary (HSM refusal, rotation, …).
    Signing(String),
    /// The signature did not verify against the protocol's own verifier. This
    /// means the signing recipe drifted; it must never reach the node.
    SelfVerificationFailed,
}

impl std::fmt::Display for AttestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommitIdMismatch {
                expected,
                recomputed,
            } => write!(
                f,
                "commit id mismatch: chain has {expected}, reading recomputes to {recomputed} — \
                 refusing to attest a reading the commit does not bind"
            ),
            Self::DataNotAnObject => {
                write!(f, "revealed reading must be a JSON object")
            },
            Self::ConfidenceOutOfRange(c) => {
                write!(f, "confidence {c} is outside [0, 1]")
            },
            Self::MissingNonce => write!(f, "nonce is required and must be non-empty"),
            Self::CustodyViolation { oracle_id, role } => write!(
                f,
                "resolved oracle key {oracle_id} is the {role} role's key — refusing to attest \
                 with another role's key material (#2464 custody rule)"
            ),
            Self::IncompleteRoleInventory { missing } => write!(
                f,
                "cannot prove the oracle key is distinct: no key could be inventoried for \
                 role(s) [{}]. A label is not a key object, so distinctness is unverified — \
                 refusing rather than assuming (#2464 custody rule)",
                missing.join(", ")
            ),
            Self::Signing(e) => write!(f, "oracle signer refused to sign: {e}"),
            Self::SelfVerificationFailed => write!(
                f,
                "attestation self-verification failed — signing recipe drifted from the protocol"
            ),
        }
    }
}

impl std::error::Error for AttestError {}

impl PendingObservation {
    /// Recompute the commit id from the revealed reading using the consensus
    /// commit-id contract.
    ///
    /// Delegates to [`Transaction::compute_commit_id`] rather than restating
    /// the recipe. An attester with its own copy of the preimage format is an
    /// attester that drifts from consensus silently.
    pub fn recomputed_commit_id(&self) -> String {
        Transaction::compute_commit_id(
            &self.data,
            &self.nonce,
            &self.station_id,
            self.timestamp,
            &self.contribution_type,
        )
    }

    /// Refuse unless the revealed reading reproduces the on-chain commit id.
    pub fn verify_commit_id(&self) -> Result<(), AttestError> {
        if !self.data.is_object() {
            return Err(AttestError::DataNotAnObject);
        }
        let recomputed = self.recomputed_commit_id();
        if recomputed != self.commit_id {
            return Err(AttestError::CommitIdMismatch {
                expected: self.commit_id.clone(),
                recomputed,
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The #2464 custody rule: a complete forbidden-role inventory
// ---------------------------------------------------------------------------

/// Every **non-oracle** signing role's public key, plus an honest record of the
/// roles whose key could not be determined.
///
/// # Why an inventory instead of a caller-supplied list
///
/// Review of #2474 found the earlier `from_env(&[PublicKey])` insufficient in
/// two ways, and both are about the same mistake — treating configuration as
/// evidence:
///
/// 1. It compared only against **compiled producer pins**, so an explicit
///    `WRFCOIN_HSM_ORACLE_KEY_LABEL` pointing at the *snapshot* object was
///    accepted. Distinct labels do not imply distinct key objects; two labels
///    can name one key.
/// 2. `from_env(&[])` was accepted, so any caller could opt out of the rule
///    entirely by passing nothing.
///
/// So the inventory is built here rather than supplied, and it reports what it
/// could **not** see. A role that cannot be inventoried is recorded in
/// [`uninventoried`](Self::uninventoried) rather than silently omitted, because
/// an omission is indistinguishable from "no conflict" — which is exactly how
/// an unverifiable custody rule comes to read as a pass.
#[derive(Debug, Default, Clone)]
pub struct ForbiddenRoleKeys {
    entries: Vec<(String, PublicKey)>,
}

/// Every signing role an oracle key must differ from.
///
/// Completeness is measured against this list rather than against whatever the
/// caller happened to supply. That distinction is the fix for the #2474
/// finding: an inventory that simply *held nothing* used to count as complete,
/// so `from_env(&[])` waved the rule through. A role missing from an inventory
/// is now a missing role, not an absence of conflict.
///
/// **`governance` is listed even though no governance signer ships yet.** An
/// earlier revision excluded it on the reasoning that listing a role with no
/// implementation would demand a key that cannot exist. That was backwards: the
/// custody rule is about key *objects*, not about which binaries exist, and a
/// governance key can be provisioned on the token long before any code loads
/// it. Excluding it meant `is_complete()` returned true while the oracle key
/// had never been compared against governance material at all — the exact shape
/// of "unverified reported as verified" this type exists to prevent.
///
/// Nothing here needs a governance *signer*: the public half is enough, it is
/// recorded in the key ceremony, and `--forbid-key governance=<hex>` already
/// supplies it. So the cost of listing it is one ceremony argument, and the
/// cost of omitting it is an unproven custody claim.
pub const NON_ORACLE_SIGNING_ROLES: &[&str] = &["snapshot", "producer", "governance"];

impl ForbiddenRoleKeys {
    /// Inventory every non-oracle role resolvable **without touching hardware**.
    ///
    /// Software-configured roles (hex var or key file) yield a real public key.
    /// A role configured through PKCS#11 cannot be compared without opening the
    /// token — which needs a PIN and must never happen implicitly — so it is
    /// simply absent, and therefore reported by [`uninventoried`](Self::uninventoried).
    pub fn from_env() -> Self {
        let mut inventory = Self::default();
        for role in NON_ORACLE_SIGNING_ROLES {
            let env = match *role {
                // The snapshot signer has no named constructor; it is the plain
                // hex-var role that `load_block_signer` uses.
                "snapshot" => SignerEnv::with_hex_var(
                    crate::crypto::signer::env_vars::SNAPSHOT_SIGNER_PRIVATE_KEY,
                ),
                "producer" => SignerEnv::producer(),
                // `governance` has no SignerEnv: no governance signer ships
                // yet, so it is never locally resolvable and stays
                // uninventoried until supplied explicitly. That is the intended
                // outcome, not an oversight — see NON_ORACLE_SIGNING_ROLES.
                _ => continue,
            };
            if let Some(key) = software_public_key_for(&env) {
                inventory.entries.push(((*role).to_string(), key));
            }
        }
        inventory
    }

    /// Add a known key for a role — e.g. the compiled genesis producer pins,
    /// which need no hardware and no environment at all.
    pub fn with_key(mut self, role: impl Into<String>, key: PublicKey) -> Self {
        self.entries.push((role.into(), key));
        self
    }

    /// Roles from [`NON_ORACLE_SIGNING_ROLES`] with no key to compare against.
    /// Non-empty means distinctness is **unverified**, not verified-absent.
    pub fn uninventoried(&self) -> Vec<String> {
        NON_ORACLE_SIGNING_ROLES
            .iter()
            .filter(|role| !self.entries.iter().any(|(have, _)| have == *role))
            .map(|role| (*role).to_string())
            .collect()
    }

    /// True when every role in [`NON_ORACLE_SIGNING_ROLES`] has a key.
    pub fn is_complete(&self) -> bool {
        self.uninventoried().is_empty()
    }

    /// The role `candidate` collides with, if any.
    pub fn conflict(&self, candidate: &PublicKey) -> Option<&str> {
        let candidate = candidate.to_hex();
        self.entries
            .iter()
            .find(|(_, key)| key.to_hex() == candidate)
            .map(|(role, _)| role.as_str())
    }
}

/// Resolve a role's public key from its **software** configuration only.
///
/// Never opens a PKCS#11 token: doing so needs a PIN, and a constructor that
/// silently consumes a PIN retry would be a worse failure than the one this
/// guards against.
fn software_public_key_for(env: &SignerEnv<'_>) -> Option<PublicKey> {
    // determinism-ok: startup custody inventory, not a consensus path; it reads the role's own key configuration to prove distinctness and never influences block state
    if let Ok(hex) = std::env::var(env.hex_key_var) {
        if let Ok(sk) = crate::crypto::signature::PrivateKey::from_hex(hex.trim()) {
            return Some(PublicKey::from_private_key(&sk));
        }
    }
    // determinism-ok: same startup custody inventory; reads the role's key-file path only
    let path = env.key_file_var.and_then(|var| std::env::var(var).ok())?;
    let path = std::path::Path::new(path.trim());

    // Mode-0600 is enforced here exactly as on the loading path. Caught by
    // `no_file_backed_key_load_exists_without_the_permission_check`, which
    // refuses any file-backed key read that skips this — and it was right to:
    // an inventory that happily read a world-readable key file would be
    // enforcing custody while quietly relaxing the permission rule protecting
    // the very material it is comparing.
    crate::security::key_file::assert_key_file_permissions(path).ok()?;

    let contents = std::fs::read_to_string(path).ok()?;
    let sk = crate::crypto::signature::PrivateKey::from_hex(contents.trim()).ok()?;
    Some(PublicKey::from_private_key(&sk))
}

// ---------------------------------------------------------------------------
// 1. Discovery
// ---------------------------------------------------------------------------

/// Select the observations this oracle should attest next.
///
/// Skips two classes, both of which would be wasted or rejected work:
///
/// - commits this `oracle_id` has **already** attested — the chain keys
///   [`AttestationSet::by_oracle`] by oracle id, so a re-submission replaces
///   rather than adds and never advances quorum;
/// - commits that **already have quorum** — the reward decision is made, and
///   another attestation cannot change it.
///
/// `quorum` of `0` is treated as "every tracked commit already has quorum" by
/// [`AttestationSet::has_quorum`], which would empty the work list. A zero
/// quorum is a genesis misconfiguration (#2464 acceptance criteria forbid
/// `oracle_set.len() < oracle_quorum` at block 1), so it is clamped to 1 here
/// rather than silently reported as "nothing to do".
pub fn discover_pending<'a>(
    observations: &'a [PendingObservation],
    tracked: &FxHashMap<String, AttestationSet>,
    oracle_id: &str,
    quorum: usize,
) -> Vec<&'a PendingObservation> {
    let effective_quorum = quorum.max(1);
    observations
        .iter()
        .filter(|obs| match tracked.get(&obs.commit_id) {
            None => true,
            Some(set) => {
                !set.by_oracle.contains_key(oracle_id) && !set.has_quorum(effective_quorum)
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 2. Signing
// ---------------------------------------------------------------------------

/// An oracle attester bound to the production oracle signer role.
pub struct Attester {
    oracle_id: String,
    signer: Arc<dyn BlockSigner>,
}

impl std::fmt::Debug for Attester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attester")
            .field("oracle_id", &self.oracle_id)
            .field("provenance", self.signer.provenance())
            .finish()
    }
}

impl Attester {
    /// Build an attester from an already-resolved signer.
    ///
    /// The `oracle_id` is taken from the signer's own public key rather than
    /// accepted as a parameter. The node verifies the attestation signature
    /// against the key registered for `oracle_id`, so an attester allowed to
    /// name an identity its key does not control could only ever produce
    /// rejected attestations.
    pub fn new(signer: Arc<dyn BlockSigner>) -> Self {
        let oracle_id = signer.public_key().to_hex();
        Self { oracle_id, signer }
    }

    /// Resolve the production oracle signer from the environment (#2472) and
    /// enforce the #2464 custody rule against `forbidden_keys`.
    ///
    /// `Ok(None)` means nothing was configured. Callers must treat that as
    /// fatal — an oracle that cannot sign cannot attest, and a node short of
    /// quorum pays no contributor.
    ///
    /// # Why an inventory, not a caller-supplied list
    ///
    /// `SignerEnv::oracle()` does **not** guarantee a distinct identity. Review
    /// of #2472 proved with a RED control that when `WRFCOIN_HSM_ORACLE_KEY_LABEL`
    /// is unset, `Pkcs11Config::from_env_for_role_inner` falls through to the
    /// shared `WRFCOIN_HSM_KEY_LABEL` and the oracle role resolves the
    /// **snapshot keypair**.
    ///
    /// Review of #2474 then showed the first guard here was still too weak: it
    /// compared only against compiled *producer* pins, so an explicit oracle
    /// label pointing at the *snapshot* object passed, and `from_env(&[])` let a
    /// caller opt out of the rule entirely. Both were the same mistake —
    /// treating configuration as evidence of distinctness. **A label is not a
    /// key object; two labels can name one key.**
    ///
    /// So this takes a [`ForbiddenRoleKeys`] inventory of every non-oracle role
    /// and refuses on two conditions:
    ///
    /// - the resolved oracle key **is** another role's key, or
    /// - the inventory is **incomplete**, because distinctness is then
    ///   unverified rather than verified-absent. Failing closed here is the
    ///   whole point: an unverifiable custody rule reads as a pass.
    ///
    /// The consequence being guarded is concrete — an attestation signed with
    /// producer key material would carry block-production authority into the
    /// oracle quorum.
    pub fn from_env(forbidden: &ForbiddenRoleKeys) -> anyhow::Result<Option<Self>> {
        let Some(ResolvedSigner { signer, .. }) = load_oracle_signer()? else {
            return Ok(None);
        };
        let attester = Self::new(signer);
        let resolved = attester.signer.public_key();

        if let Some(role) = forbidden.conflict(&resolved) {
            return Err(AttestError::CustodyViolation {
                oracle_id: attester.oracle_id.clone(),
                role: role.to_string(),
            }
            .into());
        }
        if !forbidden.is_complete() {
            return Err(AttestError::IncompleteRoleInventory {
                missing: forbidden.uninventoried(),
            }
            .into());
        }
        Ok(Some(attester))
    }

    /// This attester's oracle id — the hex public key the node checks against
    /// its registered oracle set.
    pub fn oracle_id(&self) -> &str {
        &self.oracle_id
    }

    /// Validate, sign, and self-verify an attestation for one observation.
    ///
    /// Order matters: the commit id is verified **before** anything is signed,
    /// so a reading that does not match its commit can never produce a
    /// signature at all.
    pub fn attest(
        &self,
        observation: &PendingObservation,
        approved: bool,
        // determinism-ok: OracleAttestationPayload::confidence is Option<f64> in the protocol; passed through unmodified, never computed on
        confidence: Option<f64>,
        timestamp: i64,
        nonce: &str,
    ) -> Result<OracleAttestationPayload, AttestError> {
        observation.verify_commit_id()?;

        if let Some(c) = confidence {
            // determinism-ok: exact comparison against 0.0 and 1.0, both exactly representable in binary FP; no arithmetic or accumulation, so the verdict is machine-independent
            if !(0.0..=1.0).contains(&c) {
                return Err(AttestError::ConfidenceOutOfRange(c));
            }
        }
        let nonce = nonce.trim();
        if nonce.is_empty() {
            return Err(AttestError::MissingNonce);
        }

        let mut payload = OracleAttestationPayload {
            commit_id: observation.commit_id.clone(),
            oracle_id: self.oracle_id.clone(),
            approved,
            signature: None,
            confidence,
            timestamp,
            nonce: Some(nonce.to_string()),
        };

        // The signing message comes from the protocol's own payload type. This
        // is the attestation contract, NOT the commit-id recipe verified above.
        let message = payload.canonical_signing_message();
        let signature = self
            .signer
            .sign(message.as_bytes())
            .map_err(|e| AttestError::Signing(e.to_string()))?;
        payload.signature = Some(signature.to_hex());

        // Self-verify through the protocol verifier before the payload can
        // escape. If the recipe ever drifts, fail here rather than shipping
        // attestations the chain silently rejects.
        if !payload.has_valid_signature() {
            return Err(AttestError::SelfVerificationFailed);
        }
        Ok(payload)
    }
}

// ---------------------------------------------------------------------------
// 3. Submission
// ---------------------------------------------------------------------------

/// The exact JSON body `POST /transactions/oracle/attestation` accepts.
///
/// The wire field is `commit_hash`, **not** `commit_id` — the route reads
/// `commit_hash` and `OracleAttestationPayload` carries a serde alias for it.
/// Rendering through this type rather than serializing the payload directly is
/// what keeps that asymmetry from becoming a 422 at the node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttestationSubmission {
    pub oracle_id: String,
    pub commit_hash: String,
    pub approved: bool,
    pub signature: Option<String>,
    // determinism-ok: the wire shape the node accepts; submit_oracle_attestation types confidence as Option<f64>, so anything else would not deserialize
    pub confidence: Option<f64>,
    pub timestamp: Option<i64>,
    pub nonce: Option<String>,
}

impl From<&OracleAttestationPayload> for AttestationSubmission {
    fn from(p: &OracleAttestationPayload) -> Self {
        Self {
            oracle_id: p.oracle_id.clone(),
            commit_hash: p.commit_id.clone(),
            approved: p.approved,
            signature: p.signature.clone(),
            confidence: p.confidence,
            timestamp: Some(p.timestamp),
            nonce: p.nonce.clone(),
        }
    }
}

impl AttestationSubmission {
    /// Render the request body.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("attestation submission is always serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use crate::crypto::signer::EnvKeySigner;
    use crate::oracle::attestation::AttestationRecord;

    /// The `collector-style-converted` golden vector from
    /// `native-chain/tests/commit_reveal_hash_parity.rs`, which is itself
    /// pinned against the backend's TypeScript implementation and the testnet
    /// collector (`infra/scripts/weather-station-onboarding.js`).
    ///
    /// Reused verbatim rather than freshly computed: a golden vector invented
    /// here would pin this module to *itself*, which proves nothing about
    /// cross-language parity. Anchoring to the already-proven vector is what
    /// makes this a real parity assertion.
    const GOLDEN_DATA: &str = r#"{"humidity_pct":50,"pressure_hpa":1016.916,"rainfall_mm":0,"temperature_c":21.11111111111111,"wind_mps":2.2352}"#;
    const GOLDEN_NONCE: &str = "n1";
    const GOLDEN_STATION: &str = "WX-DAVIS-7";
    const GOLDEN_TIMESTAMP: i64 = 1_699_999_999;
    const GOLDEN_COMMIT_ID: &str =
        "42bcec3eb1fe024428c10468723fb76b37a78d4082a3fdd8f72f21e6f14b2729";  // pragma: allowlist secret -- public SHA3-256 commit-id golden vector, already pinned in native-chain/tests/commit_reveal_hash_parity.rs; a published hash, not key material

    fn golden_observation() -> PendingObservation {
        PendingObservation {
            commit_id: GOLDEN_COMMIT_ID.to_string(),
            station_id: GOLDEN_STATION.to_string(),
            nonce: GOLDEN_NONCE.to_string(),
            timestamp: GOLDEN_TIMESTAMP,
            data: serde_json::from_str(GOLDEN_DATA).expect("golden data is valid JSON"),
            contribution_type: CONTRIBUTION_TYPE.to_string(),
        }
    }

    fn test_attester() -> Attester {
        let sk = PrivateKey::from_hex(
            "4242424242424242424242424242424242424242424242424242424242424242",  // pragma: allowlist secret -- deterministic test-only ed25519 private key; exists solely to make these unit tests reproducible and controls nothing
        )
        .expect("fixed test key is valid");
        Attester::new(Arc::new(EnvKeySigner::from_private_key(
            sk,
            "WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY",
        )))
    }

    // -- commit-id contract --------------------------------------------------

    #[test]
    fn the_attester_reproduces_the_collector_commit_id_golden_vector_2464() {
        let obs = golden_observation();
        assert_eq!(
            obs.recomputed_commit_id(),
            GOLDEN_COMMIT_ID,
            "attester drifted from the cross-language commit-id contract"
        );
        assert!(obs.verify_commit_id().is_ok());
    }

    /// Parity across **every** pinned cross-language vector, not just the
    /// collector-shaped one.
    ///
    /// These are the same four vectors as
    /// `native-chain/tests/commit_reveal_hash_parity.rs`, which pins core4
    /// against the backend's TypeScript implementation
    /// (`backend/shared/commit-reveal-hash.ts`) and the testnet collector
    /// (`infra/scripts/weather-station-onboarding.js`). Keep the two lists in
    /// sync.
    ///
    /// One vector proves the attester calls the right function; four prove it
    /// agrees on the awkward parts — integer vs fractional values, a
    /// non-terminating decimal (`21.11111111111111`, the V8↔ryu round-trip
    /// case), and an all-zero reading whose keys must still serialize sorted.
    /// A drift that only shows up on one of those is exactly the kind that
    /// reaches production, because ordinary readings keep working.
    #[test]
    fn the_attester_matches_every_pinned_cross_language_vector_2464() {
        // (label, canonical data JSON as sent on the wire, nonce, station_id,
        //  timestamp, expected sha3 hex)
        const VECTORS: &[(&str, &str, &str, &str, i64, &str)] = &[
            (
                "integer+fractional",
                r#"{"humidity_pct":65,"pressure_hpa":1013.25,"rainfall_mm":0,"temperature_c":22.5,"wind_mps":0}"#,
                "reveal-nonce",
                "WX-REVEAL-001",
                1_700_000_000,
                "f662f1e6a3e3c2cfb07ded9a65a9da0106250efe8985144ece9bf15bd8677708",  // pragma: allowlist secret -- public SHA3-256 commit-id golden vector, already pinned in native-chain/tests/commit_reveal_hash_parity.rs; a published hash, not key material
            ),
            (
                "all-fractional",
                r#"{"humidity_pct":54.2,"pressure_hpa":1009.7,"rainfall_mm":0.4,"temperature_c":18.3,"wind_mps":3.6}"#,
                "abc-123",
                "station-001",
                1_733_000_000,
                "140ded74a31cb1fb52d42a4c88b82764f1f05bb7e19b4aedd9d67cf08f9e7ee4",  // pragma: allowlist secret -- public SHA3-256 commit-id golden vector, already pinned in native-chain/tests/commit_reveal_hash_parity.rs; a published hash, not key material
            ),
            (
                "collector-style-converted",
                GOLDEN_DATA,
                GOLDEN_NONCE,
                GOLDEN_STATION,
                GOLDEN_TIMESTAMP,
                GOLDEN_COMMIT_ID,
            ),
            (
                "all-zero",
                r#"{"humidity_pct":0,"pressure_hpa":0,"rainfall_mm":0,"temperature_c":0,"wind_mps":0}"#,
                "z",
                "S0",
                1,
                "98db71e92de2b1327bba34822c2595347c820ad522a8adf6a0fec30ba42f9cd8",  // pragma: allowlist secret -- public SHA3-256 commit-id golden vector, already pinned in native-chain/tests/commit_reveal_hash_parity.rs; a published hash, not key material
            ),
        ];

        for (label, data_json, nonce, station_id, timestamp, expected) in VECTORS {
            let obs = PendingObservation {
                commit_id: (*expected).to_string(),
                station_id: (*station_id).to_string(),
                nonce: (*nonce).to_string(),
                timestamp: *timestamp,
                data: serde_json::from_str(data_json).expect("vector data is valid JSON"),
                contribution_type: CONTRIBUTION_TYPE.to_string(),
            };

            assert_eq!(
                &obs.recomputed_commit_id(),
                expected,
                "commit-id parity mismatch on vector '{label}' — the attester has drifted from \
                 the collector/backend contract, so its attestations would certify a reading \
                 the chain binds to a different commit id"
            );
            assert!(
                obs.verify_commit_id().is_ok(),
                "vector '{label}' must validate"
            );
        }
    }

    /// RED control: mutating any single preimage field must diverge the hash
    /// and be refused. Runs over every field so a future change that drops one
    /// from the preimage is caught rather than silently ignored.
    #[test]
    fn mutating_any_commit_id_preimage_field_is_refused_2464() {
        let base = golden_observation();

        let mut mutations = vec![];

        let mut m = base.clone();
        m.station_id = "WX-DAVIS-8".to_string();
        mutations.push(("station_id", m));

        let mut m = base.clone();
        m.nonce = "n2".to_string();
        mutations.push(("nonce", m));

        let mut m = base.clone();
        m.timestamp = GOLDEN_TIMESTAMP + 1;
        mutations.push(("timestamp", m));

        let mut m = base.clone();
        m.contribution_type = "forecast".to_string();
        mutations.push(("contribution_type", m));

        // A one-ulp change to a single reading value.
        let mut m = base.clone();
        m.data["temperature_c"] = serde_json::json!(21.11111111111112);
        mutations.push(("data", m));

        for (field, mutated) in mutations {
            let recomputed = mutated.recomputed_commit_id();
            assert_ne!(
                recomputed, GOLDEN_COMMIT_ID,
                "mutating {field} did not change the commit id — it is not bound by the preimage"
            );
            assert_eq!(
                mutated.verify_commit_id(),
                Err(AttestError::CommitIdMismatch {
                    expected: GOLDEN_COMMIT_ID.to_string(),
                    recomputed,
                }),
                "mutating {field} was not refused"
            );
        }
    }

    /// The RED control that matters most: a mismatched reading must not merely
    /// be reported — it must be impossible to obtain a signature for.
    #[test]
    fn a_mismatched_reading_never_produces_a_signature_2464() {
        let attester = test_attester();
        let mut tampered = golden_observation();
        tampered.data["temperature_c"] = serde_json::json!(99.0);

        let result = attester.attest(&tampered, true, Some(1.0), GOLDEN_TIMESTAMP, "nonce-1");
        assert!(
            matches!(result, Err(AttestError::CommitIdMismatch { .. })),
            "a tampered reading produced {result:?} instead of a refusal"
        );
    }

    #[test]
    fn a_non_object_reading_is_refused_2464() {
        let mut obs = golden_observation();
        obs.data = serde_json::json!([1, 2, 3]);
        assert_eq!(obs.verify_commit_id(), Err(AttestError::DataNotAnObject));
    }

    // -- signing contract ----------------------------------------------------

    #[test]
    fn a_signed_attestation_self_verifies_against_the_protocol_verifier_2464() {
        let attester = test_attester();
        let payload = attester
            .attest(&golden_observation(), true, Some(0.9), GOLDEN_TIMESTAMP, "nonce-1")
            .expect("golden observation must attest cleanly");

        assert!(payload.has_valid_signature());
        assert_eq!(payload.commit_id, GOLDEN_COMMIT_ID);
        assert_eq!(payload.oracle_id, attester.oracle_id());
        assert_eq!(payload.nonce.as_deref(), Some("nonce-1"));
    }

    /// RED control for the *other* hash contract, and the reason this module
    /// exists in the shape it does.
    ///
    /// Signing the commit-id preimage — the literal reading of "match the
    /// collector's SHA3-256 5-field hash" — produces a signature the node's
    /// verifier rejects. This test pins the distinction so a future refactor
    /// cannot quietly collapse the two contracts into one.
    #[test]
    fn signing_the_commit_id_preimage_instead_of_the_canonical_message_is_rejected_2464() {
        let sk = PrivateKey::from_hex(
            "4242424242424242424242424242424242424242424242424242424242424242",  // pragma: allowlist secret -- deterministic test-only ed25519 private key; exists solely to make these unit tests reproducible and controls nothing
        )
        .expect("fixed test key is valid");
        let oracle_id = crate::crypto::signature::PublicKey::from_private_key(&sk).to_hex();
        let obs = golden_observation();

        let mut payload = OracleAttestationPayload {
            commit_id: obs.commit_id.clone(),
            oracle_id,
            approved: true,
            signature: None,
            confidence: Some(0.9),
            timestamp: GOLDEN_TIMESTAMP,
            nonce: Some("nonce-1".to_string()),
        };

        // The wrong contract: sign the commit id itself.
        payload.signature = Some(sk.sign(obs.commit_id.as_bytes()).to_hex());
        assert!(
            // secret-leak-ok: `sk` is a fixed in-test key; the message interpolates nothing
            !payload.has_valid_signature(),
            "signing the commit id verified — the two hash contracts have collapsed, and \
             attestations signed the wrong way would now be indistinguishable"
        );

        // The right contract, same key, same payload: verifies.
        payload.signature =
            Some(sk.sign(payload.canonical_signing_message().as_bytes()).to_hex());
        assert!(payload.has_valid_signature());
    }

    #[test]
    fn the_oracle_id_comes_from_the_signing_key_not_the_caller_2464() {
        let attester = test_attester();
        let sk = PrivateKey::from_hex(
            "4242424242424242424242424242424242424242424242424242424242424242",  // pragma: allowlist secret -- deterministic test-only ed25519 private key; exists solely to make these unit tests reproducible and controls nothing
        )
        .expect("fixed test key is valid");
        assert_eq!(
            // secret-leak-ok: compares PUBLIC keys derived from a fixed in-test key
            attester.oracle_id(),
            crate::crypto::signature::PublicKey::from_private_key(&sk).to_hex()
        );
    }

    #[test]
    fn an_out_of_range_confidence_is_refused_before_signing_2464() {
        let attester = test_attester();
        for bad in [-0.1_f64, 1.1] {
            assert_eq!(
                attester.attest(&golden_observation(), true, Some(bad), GOLDEN_TIMESTAMP, "n"),
                Err(AttestError::ConfidenceOutOfRange(bad))
            );
        }
    }

    #[test]
    fn a_blank_nonce_is_refused_because_the_node_rejects_it_2464() {
        let attester = test_attester();
        for blank in ["", "   "] {
            assert_eq!(
                attester.attest(&golden_observation(), true, None, GOLDEN_TIMESTAMP, blank),
                Err(AttestError::MissingNonce)
            );
        }
    }

    // -- custody rule (#2464) ------------------------------------------------

    /// These exercise the software (debug) oracle path, so they assert the
    /// guard itself rather than the PKCS#11 fallthrough. That is deliberate:
    /// the fallthrough is #2472's bug to fix, and this guard must hold
    /// regardless of whether it is fixed.
    #[cfg(debug_assertions)]
    mod custody {
        use super::*;
        use crate::crypto::signer::{env_vars, test_env_lock};

        const ORACLE_KEY: &str =
            "4242424242424242424242424242424242424242424242424242424242424242";  // pragma: allowlist secret -- deterministic test-only ed25519 private key; exists solely to make these unit tests reproducible and controls nothing
        const PRODUCER_KEY: &str =
            "1313131313131313131313131313131313131313131313131313131313131313";
        const SNAPSHOT_KEY: &str =
            "2727272727272727272727272727272727272727272727272727272727272727";  // pragma: allowlist secret -- deterministic test-only ed25519 private key
        const GOVERNANCE_KEY: &str =
            "5555555555555555555555555555555555555555555555555555555555555555";  // pragma: allowlist secret -- deterministic test-only ed25519 private key  // pragma: allowlist secret -- deterministic test-only ed25519 private key; exists solely to make these unit tests reproducible and controls nothing

        fn pubkey_of(hex: &str) -> PublicKey {
            crate::crypto::signature::PublicKey::from_private_key(
                &PrivateKey::from_hex(hex).expect("fixed test key is valid"),
            )
        }

        /// Point the oracle role at a known software key. Caller holds the lock.
        fn set_oracle_key(hex: &str) {
            // SAFETY: serialized by the module-tree `test_env_lock()` held by
            // the caller, so no other thread reads or writes the environment.
            unsafe {
                std::env::remove_var(env_vars::HSM_MODULE_PATH);
                std::env::remove_var(env_vars::ORACLE_SIGNER_KEY_FILE);
                std::env::set_var(env_vars::ORACLE_SIGNER_PRIVATE_KEY, hex);
            }
        }

        fn clear_oracle_key() {
            // SAFETY: as above.
            unsafe {
                std::env::remove_var(env_vars::ORACLE_SIGNER_PRIVATE_KEY);
            }
        }

        /// A complete inventory naming every role in NON_ORACLE_SIGNING_ROLES.
        ///
        /// `governance` is supplied explicitly because no governance signer
        /// ships yet, so it is never locally resolvable — which is exactly the
        /// case `--forbid-key governance=<hex>` exists to cover.
        fn complete_inventory(snapshot: &str, producer: &str) -> ForbiddenRoleKeys {
            ForbiddenRoleKeys::default()
                .with_key("snapshot", pubkey_of(snapshot))
                .with_key("producer", pubkey_of(producer))
                .with_key("governance", pubkey_of(GOVERNANCE_KEY))
        }

        #[test]
        fn an_oracle_key_that_collides_with_a_producer_key_is_refused_2464() {
            let _guard = test_env_lock();
            set_oracle_key(ORACLE_KEY);

            // Reachable today via the #2472 HSM label fallthrough.
            let err = Attester::from_env(&complete_inventory(SNAPSHOT_KEY, ORACLE_KEY))
                .expect_err("a colliding oracle key must be refused, not returned");

            assert_eq!(
                err.downcast_ref::<AttestError>()
                    .expect("custody refusal must surface as AttestError"),
                &AttestError::CustodyViolation {
                    oracle_id: pubkey_of(ORACLE_KEY).to_hex(),
                    role: "producer".to_string(),
                }
            );
            clear_oracle_key();
        }

        /// The gap review of #2474 found: the earlier guard compared only
        /// against producer pins, so an explicit oracle label pointing at the
        /// SNAPSHOT object was accepted. Distinct labels do not imply distinct
        /// key objects.
        #[test]
        fn an_oracle_key_that_collides_with_the_snapshot_key_is_refused_2464() {
            let _guard = test_env_lock();
            set_oracle_key(ORACLE_KEY);

            let err = Attester::from_env(&complete_inventory(ORACLE_KEY, PRODUCER_KEY))
                .expect_err("a snapshot-key collision must be refused too");

            assert_eq!(
                err.downcast_ref::<AttestError>().unwrap(),
                &AttestError::CustodyViolation {
                    oracle_id: pubkey_of(ORACLE_KEY).to_hex(),
                    role: "snapshot".to_string(),
                }
            );
            clear_oracle_key();
        }

        /// The other #2474 gap: `from_env(&[])` used to be accepted, letting a
        /// caller opt out of the rule entirely. An incomplete inventory is now a
        /// refusal, because distinctness is then unverified — not verified-absent.
        #[test]
        fn an_incomplete_inventory_is_refused_rather_than_assumed_distinct_2464() {
            let _guard = test_env_lock();
            set_oracle_key(ORACLE_KEY);

            let err = Attester::from_env(&ForbiddenRoleKeys::default().with_key(
                "producer",
                pubkey_of(PRODUCER_KEY),
            ))
            .expect_err("an inventory missing a role must not be treated as no-conflict");

            match err.downcast_ref::<AttestError>().unwrap() {
                AttestError::IncompleteRoleInventory { missing } => {
                    assert_eq!(
                        missing,
                        &vec!["snapshot".to_string(), "governance".to_string()]
                    );
                },
                other => panic!("expected IncompleteRoleInventory, got {other:?}"),
            }
            clear_oracle_key();
        }

        #[test]
        fn an_empty_inventory_can_no_longer_wave_the_rule_through_2464() {
            let _guard = test_env_lock();
            set_oracle_key(ORACLE_KEY);

            let err = Attester::from_env(&ForbiddenRoleKeys::default())
                .expect_err("an empty inventory must not be an escape hatch");
            assert!(matches!(
                err.downcast_ref::<AttestError>().unwrap(),
                AttestError::IncompleteRoleInventory { .. }
            ));
            clear_oracle_key();
        }

        /// RED-control companion: the same environment with a COMPLETE
        /// inventory naming distinct keys yields a usable attester, proving the
        /// refusals above come from the custody check rather than the loader.
        #[test]
        fn a_distinct_oracle_key_with_a_complete_inventory_is_accepted_2464() {
            let _guard = test_env_lock();
            set_oracle_key(ORACLE_KEY);

            let attester = Attester::from_env(&complete_inventory(SNAPSHOT_KEY, PRODUCER_KEY))
                .expect("a distinct oracle key must not be refused")
                .expect("an oracle signer is configured");
            assert_eq!(attester.oracle_id(), pubkey_of(ORACLE_KEY).to_hex());
            clear_oracle_key();
        }

        /// `from_env` inventories every role it can resolve locally, and
        /// reports the rest. If a new non-oracle signing role is added and not
        /// inventoried, this fails — which is the point, since a silently
        /// omitted role is indistinguishable from "no conflict".
        ///
        /// `governance` is deliberately NOT locally resolvable (no governance
        /// signer exists), so `from_env` alone must report the inventory as
        /// INCOMPLETE. Asserting that is the guard Elsa's RED control exposed:
        /// with governance missing from the role list, this test passed while
        /// the oracle key had never been compared against governance material.
        #[test]
        fn every_non_oracle_role_is_inventoried_2464() {
            let _guard = test_env_lock();
            // SAFETY: serialized by the lock held above.
            unsafe {
                std::env::set_var(env_vars::SNAPSHOT_SIGNER_PRIVATE_KEY, SNAPSHOT_KEY);
                std::env::set_var(env_vars::PRODUCER_SIGNER_PRIVATE_KEY, PRODUCER_KEY);
            }

            let from_env = ForbiddenRoleKeys::from_env();
            assert!(
                !from_env.is_complete(),
                "governance is not locally resolvable, so from_env alone must never \
                 report a complete inventory"
            );
            assert_eq!(from_env.uninventoried(), vec!["governance".to_string()]);
            assert_eq!(from_env.conflict(&pubkey_of(SNAPSHOT_KEY)), Some("snapshot"));
            assert_eq!(from_env.conflict(&pubkey_of(PRODUCER_KEY)), Some("producer"));
            assert_eq!(from_env.conflict(&pubkey_of(ORACLE_KEY)), None);

            // Supplying the governance public key — what --forbid-key does —
            // completes it.
            let completed = from_env.with_key("governance", pubkey_of(GOVERNANCE_KEY));
            assert!(completed.is_complete(), "still missing: {:?}", completed.uninventoried());
            assert_eq!(completed.conflict(&pubkey_of(GOVERNANCE_KEY)), Some("governance"));
        }

        /// A role with no resolvable key is reported, never silently dropped.
        #[test]
        fn an_unresolvable_role_is_reported_as_uninventoried_2464() {
            let _guard = test_env_lock();
            // SAFETY: serialized by the lock held above.
            unsafe {
                std::env::remove_var(env_vars::SNAPSHOT_SIGNER_PRIVATE_KEY);
                std::env::remove_var(env_vars::PRODUCER_SIGNER_PRIVATE_KEY);
            }

            let inventory = ForbiddenRoleKeys::from_env();
            assert!(!inventory.is_complete());
            assert!(inventory.uninventoried().contains(&"snapshot".to_string()));
            assert!(inventory.uninventoried().contains(&"producer".to_string()));
            assert!(inventory.uninventoried().contains(&"governance".to_string()));
        }

        #[test]
        fn an_unconfigured_oracle_role_reports_none_rather_than_refusing_2464() {
            let _guard = test_env_lock();
            clear_oracle_key();
            // SAFETY: serialized by the lock held above.
            unsafe {
                std::env::remove_var(env_vars::HSM_MODULE_PATH);
                std::env::remove_var(env_vars::ORACLE_SIGNER_KEY_FILE);
            }

            assert!(
                Attester::from_env(&complete_inventory(SNAPSHOT_KEY, PRODUCER_KEY))
                    .expect("absent config is not an error")
                    .is_none(),
                "an unconfigured oracle role must be None so the caller can decide it is fatal"
            );
        }
    }

    // -- discovery -----------------------------------------------------------

    fn observation_with_commit(id: &str) -> PendingObservation {
        let mut obs = golden_observation();
        obs.commit_id = id.to_string();
        obs
    }

    fn set_with(oracles: &[(&str, bool)]) -> AttestationSet {
        let mut set = AttestationSet::default();
        for (id, approved) in oracles {
            set.add(AttestationRecord {
                oracle_id: (*id).to_string(),
                approved: *approved,
                confidence: 1.0,
                timestamp: 0,
            });
        }
        set
    }

    #[test]
    fn discovery_skips_commits_this_oracle_already_attested_2464() {
        let observations = vec![observation_with_commit("a"), observation_with_commit("b")];
        let mut tracked = FxHashMap::default();
        tracked.insert("a".to_string(), set_with(&[("me", true)]));

        let found = discover_pending(&observations, &tracked, "me", 2);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].commit_id, "b");
    }

    #[test]
    fn discovery_skips_commits_that_already_have_quorum_2464() {
        let observations = vec![observation_with_commit("a")];
        let mut tracked = FxHashMap::default();
        tracked.insert(
            "a".to_string(),
            set_with(&[("other-1", true), ("other-2", true)]),
        );

        assert!(discover_pending(&observations, &tracked, "me", 2).is_empty());
    }

    #[test]
    fn discovery_includes_tracked_commits_still_short_of_quorum_2464() {
        let observations = vec![observation_with_commit("a")];
        let mut tracked = FxHashMap::default();
        tracked.insert("a".to_string(), set_with(&[("other-1", true)]));

        let found = discover_pending(&observations, &tracked, "me", 2);
        assert_eq!(found.len(), 1, "a commit one attestation short must still be work");
    }

    #[test]
    fn discovery_counts_only_approvals_toward_quorum_2464() {
        let observations = vec![observation_with_commit("a")];
        let mut tracked = FxHashMap::default();
        // Two attestations, but one rejects — approvals() == 1, short of 2.
        tracked.insert(
            "a".to_string(),
            set_with(&[("other-1", true), ("other-2", false)]),
        );

        assert_eq!(
            discover_pending(&observations, &tracked, "me", 2).len(),
            1,
            "a rejecting attestation must not be counted toward quorum"
        );
    }

    /// A zero quorum makes `has_quorum` trivially true, which would report a
    /// misconfigured chain as "nothing to attest" — the exact silent-idle
    /// failure #2464 is about.
    #[test]
    fn a_zero_quorum_does_not_silently_empty_the_work_list_2464() {
        let observations = vec![observation_with_commit("a")];
        let mut tracked = FxHashMap::default();
        tracked.insert("a".to_string(), AttestationSet::default());

        assert_eq!(discover_pending(&observations, &tracked, "me", 0).len(), 1);
    }

    // -- submission ----------------------------------------------------------

    #[test]
    fn the_submission_body_uses_commit_hash_not_commit_id_2464() {
        let attester = test_attester();
        let payload = attester
            .attest(&golden_observation(), true, Some(0.9), GOLDEN_TIMESTAMP, "nonce-1")
            .expect("golden observation must attest cleanly");
        let body = AttestationSubmission::from(&payload);
        let json: serde_json::Value =
            serde_json::from_str(&body.to_json()).expect("body is valid JSON");

        assert_eq!(
            json.get("commit_hash").and_then(|v| v.as_str()),
            Some(GOLDEN_COMMIT_ID),
            "the node reads commit_hash; emitting commit_id would 422"
        );
        assert!(
            json.get("commit_id").is_none(),
            "commit_id must not appear on the wire"
        );
        assert!(json.get("signature").and_then(|v| v.as_str()).is_some());
        assert_eq!(json.get("nonce").and_then(|v| v.as_str()), Some("nonce-1"));
    }

    /// The submission must survive a round trip through the node's own request
    /// type shape — `commit_hash` is what `OracleAttestationPayload`'s serde
    /// alias accepts, so a body this module renders must parse back.
    #[test]
    fn a_rendered_submission_parses_back_into_the_protocol_payload_2464() {
        let attester = test_attester();
        let payload = attester
            .attest(&golden_observation(), true, Some(0.9), GOLDEN_TIMESTAMP, "nonce-1")
            .expect("golden observation must attest cleanly");
        let body = AttestationSubmission::from(&payload).to_json();

        let round_tripped: OracleAttestationPayload =
            serde_json::from_str(&body).expect("submission body must parse as the payload type");
        assert_eq!(round_tripped.commit_id, GOLDEN_COMMIT_ID);
        assert!(
            round_tripped.has_valid_signature(),
            "the signature must still verify after the wire round trip"
        );
    }
}
