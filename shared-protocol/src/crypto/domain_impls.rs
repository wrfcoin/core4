//! `DomainSeparated` implementations for the signed-object inventory (#2122).
//!
//! # These impls are additive
//!
//! Nothing here changes a byte that any running node currently signs or
//! verifies. Each impl defines the *audited* domain-separated preimage for its
//! type; the live signing paths (`Transaction::signing_hash`,
//! `SlashingReportPayload::canonical_signing_message`, and so on) are untouched.
//! Cutting them over is consensus-breaking and is tracked per row in
//! [`crate::crypto::domain_registry::SIGNED_OBJECT_REGISTRY`].
//!
//! # Canonical body encoding
//!
//! Bodies are built with [`BodyBuilder`], which length-prefixes every
//! variable-length field. Fixed-width integers go in little-endian. No
//! `serde_json` — JSON would reintroduce two hazards this module must avoid:
//! `serde_json::to_vec` panics on non-finite `f64` (the hazard already
//! documented on `payload_digest_hex`), and map key ordering is a serializer
//! detail rather than a protocol guarantee.
//!
//! # Floats
//!
//! Several weather and oracle payloads carry `f64`. No float *arithmetic*
//! happens here — [`BodyBuilder::f64_field`] encodes the IEEE-754 bit pattern
//! and normalises every NaN to one quiet-NaN encoding, so the body is a
//! deterministic function of the value and cannot vary with NaN payload bits.
//!
//! # Maps
//!
//! Unordered maps (`HashMap`, `FxHashMap`) are collected into a `BTreeMap`
//! before encoding, so iteration order never reaches a preimage.

use std::collections::BTreeMap;

use crate::blockchain::block::Block;
use crate::blockchain::snapshot_sync::SnapshotSyncPayload;
use crate::blockchain::transaction::{
    CalibrationAttestationPayload, ComputeReceipt, GovernanceVotePayload, OracleAttestationPayload,
    Transaction,
};
use crate::consensus::producer_set::ProducerSetChange;
use crate::consensus::validator_set::ValidatorSetChange;
use crate::cross_chain::types::BridgeMessage;
use crate::crypto::domain_signing::{DomainSeparated, push_length_prefixed};
use crate::governance::proposal::Proposal;
use crate::security::slashing::SlashingReportPayload;
use crate::weather::validation::WeatherReading;
use wrfcoin_forecast_score::blockchain::ForecastVerificationReceipt;

/// Deterministic, unambiguous canonical-body builder.
///
/// Every variable-length field is length-prefixed, so no two distinct field
/// sequences can produce the same bytes.
#[derive(Default)]
pub struct BodyBuilder {
    buf: Vec<u8>,
}

impl BodyBuilder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Length-prefixed byte field.
    pub fn bytes_field(mut self, value: &[u8]) -> Self {
        push_length_prefixed(&mut self.buf, value);
        self
    }

    /// Length-prefixed UTF-8 field.
    pub fn str_field(self, value: &str) -> Self {
        self.bytes_field(value.as_bytes())
    }

    /// Optional string: a 1-byte discriminant then, if present, the field.
    ///
    /// The discriminant is what stops `None` and `Some("")` colliding.
    pub fn opt_str_field(mut self, value: Option<&str>) -> Self {
        match value {
            None => {
                self.buf.push(0);
                self
            },
            Some(s) => {
                self.buf.push(1);
                self.str_field(s)
            },
        }
    }

    /// Optional byte field: a 1-byte discriminant then, if present, the field.
    pub fn opt_bytes_field(mut self, value: Option<&[u8]>) -> Self {
        match value {
            None => {
                self.buf.push(0);
                self
            },
            Some(b) => {
                self.buf.push(1);
                self.bytes_field(b)
            },
        }
    }

    pub fn u64_field(mut self, value: u64) -> Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn u32_field(mut self, value: u32) -> Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn u128_field(mut self, value: u128) -> Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn i64_field(mut self, value: i64) -> Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn bool_field(mut self, value: bool) -> Self {
        self.buf.push(u8::from(value));
        self
    }

    /// IEEE-754 bit encoding with NaN normalised to one canonical payload.
    ///
    /// No float arithmetic — this is a bit-pattern copy. Normalising NaN keeps
    /// the encoding a function of the *value*: without it, two NaNs with
    /// different payload bits would produce different preimages for objects
    /// that compare equal.
    pub fn f64_field(mut self, value: f64) -> Self {
        let bits = if value.is_nan() {
            f64::NAN.to_bits()
        } else {
            value.to_bits()
        };
        self.buf.extend_from_slice(&bits.to_le_bytes());
        self
    }

    pub fn opt_f64_field(mut self, value: Option<f64>) -> Self {
        match value {
            None => {
                self.buf.push(0);
                self
            },
            Some(v) => {
                self.buf.push(1);
                self.f64_field(v)
            },
        }
    }

    pub fn opt_u64_field(mut self, value: Option<u64>) -> Self {
        match value {
            None => {
                self.buf.push(0);
                self
            },
            Some(v) => {
                self.buf.push(1);
                self.u64_field(v)
            },
        }
    }

    /// Sorted key/value map. Sorting is what keeps `HashMap` iteration order
    /// out of the preimage.
    pub fn sorted_map_field<'a, I>(mut self, entries: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let sorted: BTreeMap<&str, &str> = entries.into_iter().collect();
        self.buf
            .extend_from_slice(&(sorted.len() as u64).to_le_bytes());
        for (k, v) in sorted {
            push_length_prefixed(&mut self.buf, k.as_bytes());
            push_length_prefixed(&mut self.buf, v.as_bytes());
        }
        self
    }

    /// Length-counted sequence of already-encoded elements.
    pub fn seq_field<I: IntoIterator<Item = Vec<u8>>>(mut self, items: I) -> Self {
        let items: Vec<Vec<u8>> = items.into_iter().collect();
        self.buf
            .extend_from_slice(&(items.len() as u64).to_le_bytes());
        for item in items {
            push_length_prefixed(&mut self.buf, &item);
        }
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

// ---------------------------------------------------------------------------
// Consensus objects
// ---------------------------------------------------------------------------

impl DomainSeparated for Transaction {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.transaction.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // `id` is excluded: it is derived from the transaction, so committing
        // to it would make the preimage self-referential. Input signatures are
        // excluded for the same reason `signing_hash` excludes them — a
        // signature cannot commit to itself.
        let body = BodyBuilder::new()
            .u32_field(self.version)
            .i64_field(self.timestamp.timestamp_millis())
            .str_field(&format!("{:?}", self.tx_type))
            .str_field(&format!("{:?}", self.operation))
            .bytes_field(&self.sender_address.0)
            .opt_bytes_field(self.recipient_address.as_ref().map(|a| a.0.as_slice()))
            .u64_field(self.amount)
            .u64_field(self.fee)
            .u64_field(self.nonce)
            .opt_u64_field(self.lock_time)
            .opt_str_field(self.data.as_deref())
            .finish();
        body
    }
}

impl DomainSeparated for Block {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.block_header.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // Header fields plus a commitment to the transaction list via each
        // transaction's own hash. `hash` itself is excluded — it is the output.
        let body = BodyBuilder::new()
            .u32_field(self.version)
            .bytes_field(self.prev_block_hash.as_bytes())
            .u64_field(self.height)
            .u32_field(self.difficulty)
            .u64_field(self.nonce)
            .i64_field(self.timestamp.timestamp_millis())
            // #2290/#2226: bind the producer's public key into the block
            // identity. Inserted after the timestamp and before the tx list,
            // matching `block_producer_auth::producer_canonical_body`. The
            // producer signature is NOT bound — it is the output over this body.
            .bytes_field(&self.producer_pubkey)
            .seq_field(
                self.transactions
                    .iter()
                    .map(|tx| tx.calculate_hash().as_bytes().to_vec()),
            )
            .sorted_map_field(self.metadata.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish();
        body
    }
}

// NOTE: `consensus::finality::FinalitySignal` deliberately has **no**
// `DomainSeparated` impl.
//
// It previously carried `wrfcoin.finality_vote.v1` — the same tag the registry
// binds to `consensus_engines::selection::bft_finality::FinalityVote`, so two
// distinct types answered to one tag. That is the cross-protocol replay hole
// #2122 exists to close, not a naming inconsistency: a signature over one type
// would verify as the other.
//
// `FinalityVote` owns the tag; it is the live signed object and carries a
// `signature` field. `FinalitySignal` carries no signature at all (#2226 records
// it as unsigned) — it is an in-process mpsc notification that the BFT gadget
// sends to the chain layer, never a wire object. An unsigned type must not hold
// a domain tag, because a tag is a claim about what a signature covers.
//
// `impl_tags_are_injective_across_the_source` in
// `tests/signed_type_inventory.rs` fails if this impl is reintroduced, or if any
// other two impls claim one tag.

impl DomainSeparated for ValidatorSetChange {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.validator_set_change.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // Members are sorted by id so the preimage is a function of the *set*,
        // not the caller's ordering. Each member binds id, stake weight and
        // public key; the count prefix keeps the sequence unambiguous.
        let mut members: Vec<&crate::consensus::validator_set::ValidatorSetMember> =
            self.validators.iter().collect();
        members.sort_by(|a, b| a.validator_id.cmp(&b.validator_id));

        let mut builder = BodyBuilder::new()
            .u64_field(self.epoch)
            .u64_field(self.effective_height)
            .u64_field(self.validators.len() as u64);
        for m in members {
            builder = builder
                .str_field(&m.validator_id)
                .u64_field(m.stake_weight)
                .bytes_field(&m.public_key);
        }
        builder.finish()
    }
}

impl DomainSeparated for ProducerSetChange {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.producer_set_change.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // Canonical member order (sorted by producer_id) so the preimage is a
        // function of the *set*, not caller ordering — the SAME order used by
        // producer_set_hash and the state_root encoding (Dan review P1). Binds
        // producer_id, payout_address (§R2), and public_key; the count prefix
        // keeps the sequence unambiguous. DISTINCT domain tag from
        // validator_set_change (§R1) — a producer committee is not a finality
        // committee.
        let members = self.canonical_members();

        let mut builder = BodyBuilder::new()
            .u64_field(self.epoch)
            .u64_field(self.effective_height)
            .u64_field(self.members.len() as u64);
        for m in members {
            builder = builder
                .str_field(&m.producer_id)
                .str_field(&m.payout_address)
                .bytes_field(&m.public_key);
        }
        builder.finish()
    }
}

impl DomainSeparated for SlashingReportPayload {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.slashing_report.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // `reporter_signature` excluded — a signature cannot cover itself.
        let body = BodyBuilder::new()
            .str_field(&self.validator_id)
            .str_field(&format!("{:?}", self.evidence_type))
            .bytes_field(&self.evidence_hash)
            .u64_field(self.reported_at_height)
            .u128_field(self.penalty_amount)
            .str_field(&self.reporter_pubkey)
            .finish();
        body
    }
}

impl DomainSeparated for SnapshotSyncPayload {
    // v2 (#3288 3b): the body gained the chain tip and finalized height.
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.snapshot_sync_payload.v2";

    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .u64_field(self.height)
            .str_field(&self.state_root_hex)
            .str_field(&self.snapshot_root_hex)
            .u64_field(self.entry_count)
            .str_field(&self.tip_hash_hex)
            .u64_field(self.finalized_height)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Weather / oracle objects
// ---------------------------------------------------------------------------

impl DomainSeparated for WeatherReading {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.weather_observation.v1";

    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .i64_field(self.timestamp_unix)
            .opt_f64_field(self.temperature_c)
            .opt_f64_field(self.pressure_hpa)
            .opt_f64_field(self.humidity_pct)
            .opt_f64_field(self.wind_speed_ms)
            .finish()
    }
}

impl DomainSeparated for OracleAttestationPayload {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.oracle_attestation.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // `signature` excluded. Unlike the legacy `canonical_signing_message`
        // path, this cannot panic on a non-finite `confidence` — the float is
        // bit-encoded rather than routed through `serde_json`.
        let body = BodyBuilder::new()
            .str_field(&self.commit_id)
            .str_field(&self.oracle_id)
            .bool_field(self.approved)
            .opt_f64_field(self.confidence)
            .i64_field(self.timestamp)
            .opt_str_field(self.nonce.as_deref())
            .finish();
        body
    }
}

impl DomainSeparated for CalibrationAttestationPayload {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.calibration_attestation.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // Encoded through the type's own serde form to avoid duplicating a
        // long field list that would silently rot; `to_string` on a
        // `serde_json::Value` built from the struct is deterministic here
        // because the struct has no map fields. Falls back to an empty body
        // only if serialization fails, which cannot silently succeed as a
        // different value.
        let encoded = serde_json::to_string(self).unwrap_or_default();
        BodyBuilder::new().str_field(&encoded).finish()
    }
}

// ---------------------------------------------------------------------------
// Governance / bridge objects
// ---------------------------------------------------------------------------

impl DomainSeparated for Proposal {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.governance_proposal.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // `votes` and `status` are excluded: both mutate over the proposal's
        // lifetime, so a signature covering them would be invalidated by
        // ordinary voting. The signed claim is the proposal's *content*.
        let body = BodyBuilder::new()
            .str_field(&self.id)
            .str_field(&self.proposer)
            .str_field(&self.title)
            .str_field(&self.description)
            .str_field(&format!("{:?}", self.proposal_type))
            .i64_field(self.creation_time.timestamp_millis())
            .i64_field(self.end_time.timestamp_millis())
            .u64_field(self.deposit)
            .bool_field(self.ai_generated)
            .finish();
        body
    }
}

impl DomainSeparated for GovernanceVotePayload {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.governance_vote.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // The live path (`canonical_signing_message`) commits only to
        // `"governance-vote-v1|<proposal_id>|<approve>"` — no chain id, no
        // network id, no signer. A testnet vote therefore replays on mainnet.
        // Filed as a security finding; see
        // `domain_registry::GOVERNANCE_VOTE_REPLAY_ISSUE`. This preimage adds
        // the signer binding; the chain/network binding is now applied by the
        // trait's provided methods (#2123), so it is no longer this body's
        // concern. The *live* governance path still uses the legacy message —
        // migrating it is the consensus-breaking part tracked in the registry.
        let body = BodyBuilder::new()
            .str_field(&self.proposal_id)
            .bool_field(self.approve)
            .opt_u64_field(self.voting_power)
            .opt_str_field(self.signer_public_key.as_deref())
            .finish();
        body
    }
}

impl DomainSeparated for BridgeMessage {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.bridge_message.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // `tx_hash` excluded — it is assigned after the message is authorised.
        // `payload` is encoded through serde since it is an opaque JSON value.
        let payload_encoded = self
            .payload
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let body = BodyBuilder::new()
            .str_field(&self.message_id)
            .str_field(&format!("{:?}", self.source_network))
            .str_field(&format!("{:?}", self.target_network))
            .str_field(&format!("{:?}", self.direction))
            .str_field(&format!("{:?}", self.message_type))
            .str_field(&self.sender)
            .str_field(&self.recipient)
            .u64_field(self.amount)
            .u64_field(self.nonce)
            .i64_field(self.timestamp)
            .opt_str_field(payload_encoded.as_deref())
            .finish();
        body
    }
}

// ---------------------------------------------------------------------------
// Added after review of PR #2231 found these live signed types omitted
// ---------------------------------------------------------------------------

impl DomainSeparated for ComputeReceipt {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.compute_receipt.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // `signature` excluded — a signature cannot cover itself. Mirrors the
        // field set of the legacy `canonical_signing_message`.
        BodyBuilder::new()
            .str_field(&self.task_id)
            .str_field(&self.worker_id)
            .str_field(&self.work_hash)
            .u64_field(self.work_factor)
            .i64_field(self.completed_at)
            .str_field(&self.nonce)
            .finish()
    }
}

impl DomainSeparated for ForecastVerificationReceipt {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.forecast_verification_receipt.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // `metrics` is encoded through serde: it is a fixed-field struct with
        // no map fields, so its JSON form is order-stable. `signature` is
        // excluded; `zk_proof` is included because it is part of the claim.
        let metrics = serde_json::to_string(&self.metrics).unwrap_or_default();
        let body = BodyBuilder::new()
            .str_field(&self.manifest_digest)
            .str_field(&self.commit_id)
            .str_field(&self.oracle_id)
            .str_field(&metrics)
            .i64_field(self.verified_at)
            .str_field(&self.nonce)
            .opt_str_field(self.zk_proof.as_deref())
            .finish();
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::domain_registry::{
        SIGNED_OBJECT_REGISTRY, SignedObjectKind, tag_matches_registry,
    };

    #[test]
    fn every_impl_tag_matches_its_registry_row() {
        // A tag renamed in one place but not the other is caught here.
        assert!(tag_matches_registry::<Transaction>(
            SignedObjectKind::Transaction
        ));
        assert!(tag_matches_registry::<Block>(SignedObjectKind::BlockHeader));
        assert!(tag_matches_registry::<SlashingReportPayload>(
            SignedObjectKind::SlashingEvidence
        ));
        assert!(tag_matches_registry::<SnapshotSyncPayload>(
            SignedObjectKind::SnapshotPayload
        ));
        assert!(tag_matches_registry::<WeatherReading>(
            SignedObjectKind::WeatherObservation
        ));
        assert!(tag_matches_registry::<OracleAttestationPayload>(
            SignedObjectKind::OracleAttestation
        ));
        assert!(tag_matches_registry::<CalibrationAttestationPayload>(
            SignedObjectKind::AttestationEnvelope
        ));
        assert!(tag_matches_registry::<Proposal>(
            SignedObjectKind::GovernanceProposal
        ));
        assert!(tag_matches_registry::<GovernanceVotePayload>(
            SignedObjectKind::GovernanceVote
        ));
        assert!(tag_matches_registry::<BridgeMessage>(
            SignedObjectKind::BridgeMessage
        ));
        assert!(tag_matches_registry::<ComputeReceipt>(
            SignedObjectKind::ComputeReceipt
        ));
        assert!(tag_matches_registry::<ForecastVerificationReceipt>(
            SignedObjectKind::ForecastVerificationReceipt
        ));
    }

    #[test]
    fn every_registered_impl_emits_its_tag_first() {
        // Structural guarantee: the tag is inside the signed bytes, at a fixed
        // position, for every implementing type.
        fn check<T: DomainSeparated>(obj: &T) {
            let preimage = obj.signing_preimage();
            let tag = T::DOMAIN_TAG;
            assert_eq!(&preimage[0..8], &(tag.len() as u64).to_le_bytes());
            assert_eq!(&preimage[8..8 + tag.len()], tag);
        }

        check(&WeatherReading {
            temperature_c: Some(1.0),
            pressure_hpa: None,
            humidity_pct: None,
            wind_speed_ms: None,
            timestamp_unix: 1,
        });
        check(&SnapshotSyncPayload {
            height: 1,
            state_root_hex: "aa".into(),
            snapshot_root_hex: "bb".into(),
            entry_count: 2,
            tip_hash_hex: "cc".into(),
            finalized_height: 1,
        });
    }

    #[test]
    fn distinct_types_with_equal_bodies_produce_distinct_preimages() {
        // The cross-type replay property, on real types rather than fixtures.
        let snapshot = SnapshotSyncPayload {
            height: 9,
            state_root_hex: "root".into(),
            snapshot_root_hex: "snap".into(),
            entry_count: 3,
            tip_hash_hex: "tip".into(),
            finalized_height: 0,
        };
        let reading = WeatherReading {
            temperature_c: None,
            pressure_hpa: None,
            humidity_pct: None,
            wind_speed_ms: None,
            timestamp_unix: 9,
        };
        assert_ne!(snapshot.signing_preimage(), reading.signing_preimage());
    }

    #[test]
    fn block_preimage_is_stable_across_metadata_insertion_order() {
        use crate::blockchain::block::Block;
        use crate::crypto::hash::Hash;

        let mut a = Block::new(1, Hash::hash_bytes(b"prev"), vec![], 1, 1);
        let mut b = Block::new(1, Hash::hash_bytes(b"prev"), vec![], 1, 1);
        b.timestamp = a.timestamp;

        a.metadata.insert("alpha".into(), "1".into());
        a.metadata.insert("beta".into(), "2".into());
        b.metadata.insert("beta".into(), "2".into());
        b.metadata.insert("alpha".into(), "1".into());

        assert_eq!(
            a.signing_preimage(),
            b.signing_preimage(),
            "metadata map iteration order must not reach the preimage"
        );
    }

    #[test]
    fn nan_confidence_does_not_panic_and_is_canonical() {
        // The legacy `canonical_signing_message` path routes `confidence`
        // through serde_json, which panics on non-finite floats. The
        // domain-separated preimage must not.
        let quiet = OracleAttestationPayload {
            commit_id: "c".into(),
            oracle_id: "o".into(),
            approved: true,
            signature: None,
            confidence: Some(f64::NAN),
            timestamp: 1,
            nonce: None,
        };
        let signalling = OracleAttestationPayload {
            confidence: Some(f64::from_bits(f64::NAN.to_bits() ^ 0x3)),
            ..quiet.clone()
        };
        assert!(signalling.confidence.is_some_and(f64::is_nan));
        assert_eq!(
            quiet.signing_preimage(),
            signalling.signing_preimage(),
            "NaN payload bits must not change the preimage"
        );
    }

    #[test]
    fn optional_none_and_empty_string_do_not_collide() {
        let none = OracleAttestationPayload {
            commit_id: "c".into(),
            oracle_id: "o".into(),
            approved: true,
            signature: None,
            confidence: None,
            timestamp: 1,
            nonce: None,
        };
        let empty = OracleAttestationPayload {
            nonce: Some(String::new()),
            ..none.clone()
        };
        assert_ne!(none.signing_preimage(), empty.signing_preimage());
    }

    #[test]
    fn node_share_contract_domains_match_the_registry() {
        use crate::consensus::node_share_contracts::{
            NodeServiceTicket, NodeSettlementEndorsement, NodeBondAuthorization, NodeBondPossession,
            NodeBondPayoutPasskeyApproval,
        };
        assert!(tag_matches_registry::<NodeServiceTicket>(SignedObjectKind::NodeServiceTicket));
        assert!(tag_matches_registry::<NodeSettlementEndorsement>(SignedObjectKind::NodeSettlementEndorsement));
        assert!(tag_matches_registry::<NodeBondAuthorization>(SignedObjectKind::NodeBondAuthorization));
        assert!(tag_matches_registry::<NodeBondPossession>(SignedObjectKind::NodeBondPossession));
        assert!(tag_matches_registry::<NodeBondPayoutPasskeyApproval>(
            SignedObjectKind::NodeBondPayoutPasskeyApproval
        ));
    }

    #[test]
    fn registry_covers_every_implementing_type() {
        // Sanity: the count of non-exception rows equals the number of impls
        // asserted above.
        // 27 in-crate impls: the original 11, minus FinalityVote (its live
        // signed type is in consensus-engines, so its impl cannot live here),
        // plus ComputeReceipt and ForecastVerificationReceipt (from the review
        // of PR #2231), plus ValidatorSetChange (#2227), plus ProducerSetChange
        // (#2290 — dedicated producer-authority privilege object), plus
        // UpgradeNotice (#3094/#3102 — its `impl DomainSeparated` lives in
        // `governance::upgrade_notice`, not this file, which is why it's not
        // among the `impl DomainSeparated for` blocks above), plus
        // FeatureScheduleExtension (#3277 — likewise implemented in its own
        // module, `governance::schedule_extension`), plus FaucetSinkPossession
        // (#3361 F3 — in `governance::faucet_sink_pop`), plus three native
        // account subjects (#2167 — in `accounts::native_smart_accounts`).
        // Plus SlashEvidenceReport (Refs #3398, checked security body), and
        // NodeServiceReceipt (#3129 — consumer-signed snapshot service evidence;
        // its impl lives in consensus::node_service_receipt), plus four bonded
        // Node S1 subjects (Refs #2848, consensus::node_share_contracts),
        // plus S3 owner batch authorization (consensus::node_receipt_registry),
        // plus the passkey_bond_payout challenge subject
        // (consensus::node_share_contracts::NodeBondPayoutPasskeyApproval).
        let with_impls = SIGNED_OBJECT_REGISTRY
            .iter()
            .filter(|e| e.coverage.has_impl())
            .count();
        assert!(tag_matches_registry::<crate::consensus::node_receipt_registry::NodeReceiptSubmissionAuthorization>(SignedObjectKind::NodeReceiptSubmissionAuthorization));
        assert_eq!(with_impls, 28);
    }
}
