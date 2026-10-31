//! Verification-work pricing for signature- and proof-heavy transactions (#2143).
//!
//! Filling blocks with signature-heavy transactions is the CPU analogue of
//! state bloat: cheap to send, expensive to verify, and the cost lands on
//! every node in the network rather than on the sender. A flat per-transaction
//! fee makes the worst case free — a `RewardClaim` carrying a 1024-member ring
//! (`MAX_RING_SIZE`, `privacy/src/ring_signatures.rs:34`) forces 1024 elliptic-
//! curve operations on every verifier for the same fee as a 1-member ring.
//!
//! This module separates two things that are easy to conflate:
//!
//! - **What a transaction makes a node verify** ([`VerifyWork`]) — a count of
//!   primitives, derived from the transaction bytes. Consensus-visible, and
//!   identical on every node that sees the same transaction.
//! - **What those primitives cost** ([`VerifyPricing`]) — constants measured by
//!   `benches/verify_cost.rs` rather than chosen by intuition. Three of the five
//!   are measured today; the two that are not say so on the line that sets them
//!   and are priced deliberately high, because over-charging a proof costs fees
//!   while under-charging one reopens the vector.
//!
//! Keeping them apart means recalibrating the constants never changes how work
//! is counted, and the counting rules can be reviewed without arguing about
//! numbers.
//!
//! # The fail-expensive rule
//!
//! A proof this module cannot parse is charged the **maximum**, never zero.
//! Pricing runs before verification, so an attacker who can make the cost
//! function fail to understand a payload must not thereby make it free; that
//! would turn the pricing into the very hole it closes. Every parse fallback
//! below charges a bound, and `an_unparseable_ring_is_charged_the_maximum`
//! pins it.

use serde::Deserialize;
use crate::blockchain::transaction::{Transaction, TransactionType};

/// Largest ring an MLSAG verifier will accept, mirrored from
/// `privacy/src/ring_signatures.rs:34`. Pricing must agree with the verifier's
/// own ceiling: a ring larger than this is rejected rather than verified, so
/// it can never cost more than this many members, and an unparseable ring can
/// never cost less. `max_ring_size_agrees_with_the_verifier` pins the two
/// together (the privacy crate is a dev-dependency for exactly that test).
pub const MAX_RING_SIZE: u32 = 1024;

/// Ceiling charged in the range-proof dimension when a payload cannot be
/// parsed.
///
/// The fail-expensive rule has to hold in EVERY dimension a payload could
/// have carried, not just the loudest one. An earlier revision of this module
/// charged an unparseable payload a maximal RING and zero range proofs, which
/// meant a malformed `ConfidentialTransfer` skipped range-proof pricing
/// entirely — the rule stated in this module's own header, violated by its own
/// fallback.
///
/// Eight — one amount proof plus up to seven selective disclosures — is above
/// any realistic payload while staying BELOW the maximal-ring charge
/// (320,000 vu here against `MAX_RING_SIZE * per_ring_sig_member` =
/// 358,400 vu). That ordering is deliberate. The first value tried was 64,
/// which put the range term at 2,560,000 vu: it swamped the ring term
/// entirely, made the measured attacker-controlled linear term — the whole
/// subject of #2143 — irrelevant to the price of a claim, and put a single
/// malformed transaction over half the block budget. The tests in
/// `tests/verify_cost_budget_2143.rs` pin both properties.
pub const MAX_PRICED_RANGE_PROOFS: u32 = 8;

/// Verification units — the chain's unit of CPU work, as a saturating scalar.
///
/// Deliberately `u64` and saturating: a block's total must be comparable to a
/// budget without any possibility of wrapping into a small number, which is
/// the arithmetic version of the hole this module closes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct VerifyUnits(pub u64);

impl VerifyUnits {
    pub const ZERO: Self = Self(0);

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl std::fmt::Display for VerifyUnits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} vu", self.0)
    }
}

/// A count of the verification primitives one transaction forces.
///
/// Counts, not costs. Two nodes computing this over the same transaction bytes
/// must agree exactly, so every field is derived from the transaction and
/// nothing here reads configuration, the clock, or node-local state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VerifyWork {
    /// Ed25519 signature verifications (transaction inputs plus any
    /// type-specific authorizing signatures).
    pub ed25519: u32,
    /// ECDSA verifications (bridge/EVM-compatible surfaces).
    pub ecdsa: u32,
    /// MLSAG ring members. Ring verification is O(members) — the loop at
    /// `privacy/src/ring_signatures.rs:445` does two point operations per
    /// member — so members, not signatures, is the honest unit.
    pub ring_members: u32,
    /// Groth16 (or equivalent SNARK) proof verifications.
    pub groth16: u32,
    /// Bulletproof-style range proof verifications.
    pub range_proofs: u32,
}

impl VerifyWork {
    pub const NONE: Self = Self {
        ed25519: 0,
        ecdsa: 0,
        ring_members: 0,
        groth16: 0,
        range_proofs: 0,
    };

    /// One ordinary signature and nothing else.
    pub const fn ed25519(count: u32) -> Self {
        Self {
            ed25519: count,
            ..Self::NONE
        }
    }

    /// Componentwise saturating sum, for totalling a block.
    pub fn saturating_add(self, other: Self) -> Self {
        Self {
            ed25519: self.ed25519.saturating_add(other.ed25519),
            ecdsa: self.ecdsa.saturating_add(other.ecdsa),
            ring_members: self.ring_members.saturating_add(other.ring_members),
            groth16: self.groth16.saturating_add(other.groth16),
            range_proofs: self.range_proofs.saturating_add(other.range_proofs),
        }
    }

    /// Whether this transaction forces no verification at all (a coinbase).
    pub fn is_free(self) -> bool {
        self == Self::NONE
    }
}

/// What each primitive costs, in verification units, plus the per-block
/// ceiling.
///
/// Constants are ratios anchored on ed25519 = 100, calibrated from
/// `consensus-engines/benches/verify_cost.rs`. Anchoring on a ratio rather
/// than on nanoseconds means the table stays meaningful on faster hardware:
/// what matters for pricing is that a ring member costs about as much as an
/// ed25519 verify, and a SNARK costs about twenty times one, not the absolute
/// time on any particular machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyPricing {
    pub per_ed25519: u64,
    pub per_ecdsa: u64,
    pub per_ring_sig_member: u64,
    pub per_groth16_verify: u64,
    pub per_range_proof: u64,
    /// Maximum verification units one block may force on a validator.
    pub block_verify_budget: u64,
}

impl VerifyPricing {
    /// The committed table. See `benches/verify_cost.rs` for the measurements
    /// these ratios come from and `docs/` notes in that file for how to
    /// recalibrate.
    /// MEASURED on build container (16-core, `cargo bench --bench verify_cost`,
    /// 2026-09-12) — see the module note on what is measured and what is not.
    ///
    /// | primitive | measured | ratio to ed25519 | constant |
    /// |---|---|---|---|
    /// | ed25519 verify | 22.4 µs | 1x (anchor) | 100 |
    /// | MLSAG, per ring member | 77.9 µs | 3.47x | 350 |
    /// | range proof verify | 7.7 ms | ~340x | 40_000 |
    /// | ECDSA verify | not measured | ~2.5x (published) | 250 |
    /// | Groth16 verify | not measured | >=400x (assumed) | 50_000 |
    ///
    /// The first draft of this table priced a ring member at PARITY with an
    /// ed25519 verify, by intuition. The benchmark says 3.47x, so that draft
    /// under-charged the one term an attacker controls linearly by a factor of
    /// three and a half. That is exactly why #2143 requires calibration rather
    /// than judgement, and it is the reason the two un-measured rows below are
    /// priced deliberately HIGH: over-charging a proof type costs an honest
    /// user fees, while under-charging it reopens the vector.
    pub const CALIBRATED: Self = Self {
        // Anchor: 22.4 µs measured. Every other row is a multiple of this.
        per_ed25519: 100,
        // NOT MEASURED. Published secp256k1 verify figures put it at roughly
        // 2-3x an ed25519 verify; priced at the top of that range.
        per_ecdsa: 250,
        // MEASURED, and the load-bearing constant: per-member cost was flat at
        // 76.7-78.9 µs across ring sizes 8, 64, 256 and 1024, which both fixes
        // the ratio at 3.47x and confirms verification is LINEAR in members —
        // the property that makes per-member pricing the honest unit.
        per_ring_sig_member: 350,
        // NOT MEASURED: benchmarking a Groth16 verify needs a circuit setup
        // this bench does not build. A BN254 verify is three pairings and is
        // at least the order of a bulletproof verify, so it is priced above
        // the measured range proof rather than below it. Filed to calibrate.
        per_groth16_verify: 50_000,
        // MEASURED, but only while build container was under CI load (7.7 ms at load 4,
        // 11.9 ms at load 19, against an ed25519 anchor that moved 4%). The
        // LOWEST observed ratio is ~340x; priced at 400x so the contaminated
        // reading cannot have produced an under-charge. Re-calibrate on an
        // idle box.
        per_range_proof: 40_000,
        // Derived, not guessed: at the measured rates one unit is ~0.216 µs of
        // verification, so this budget is ~1.08 s of CPU per block — about 7%
        // of the 15 s `TARGET_BLOCK_TIME_SECONDS`. A validator that cannot
        // spend 7% of a block interval verifying it has bigger problems, and a
        // proposer cannot push past it: 5,000,000 units buys ~12
        // maximal-ring reward claims, or ~109 honest ones, or 50,000 ordinary
        // transfer signatures.
        block_verify_budget: 5_000_000,
    };

    /// Price a work count. Saturating throughout.
    pub fn units(&self, work: &VerifyWork) -> VerifyUnits {
        let term = |count: u32, price: u64| (count as u64).saturating_mul(price);
        VerifyUnits(
            term(work.ed25519, self.per_ed25519)
                .saturating_add(term(work.ecdsa, self.per_ecdsa))
                .saturating_add(term(work.ring_members, self.per_ring_sig_member))
                .saturating_add(term(work.groth16, self.per_groth16_verify))
                .saturating_add(term(work.range_proofs, self.per_range_proof)),
        )
    }
}

impl Default for VerifyPricing {
    fn default() -> Self {
        Self::CALIBRATED
    }
}

/// What a thing makes a verifier do.
pub trait VerifyCost {
    fn verify_work(&self) -> VerifyWork;
}

/// Minimal view of a serialized MLSAG used only to learn the ring size.
///
/// Deliberately NOT the `privacy` crate's `MLSAGSignature`: pricing must not
/// depend on the privacy crate's build features, and it needs one number.
#[derive(Deserialize)]
struct RingShape {
    #[serde(default)]
    ring: Vec<serde_json::Value>,
}

/// Ring members in a hex-encoded, JSON-serialized MLSAG.
///
/// Returns [`MAX_RING_SIZE`] when the payload cannot be decoded or parsed —
/// the fail-expensive rule. A ring larger than the verifier's ceiling is also
/// clamped to it, because such a signature is rejected rather than verified.
fn ring_members_in(hex_encoded: &str) -> u32 {
    let Ok(bytes) = hex::decode(hex_encoded.trim()) else {
        return MAX_RING_SIZE;
    };
    match serde_json::from_slice::<RingShape>(&bytes) {
        Ok(shape) => (shape.ring.len() as u64).min(MAX_RING_SIZE as u64) as u32,
        Err(_) => MAX_RING_SIZE,
    }
}

/// The privacy payload fields this module prices, parsed leniently: a field
/// that is absent costs nothing, a field that is present but malformed costs
/// the maximum.
#[derive(Deserialize, Default)]
struct PricedPrivacyPayload {
    #[serde(default)]
    ring_signature: Option<String>,
    #[serde(default)]
    range_proof: Option<String>,
    #[serde(default)]
    selective_disclosures: Vec<serde_json::Value>,
}

impl VerifyCost for Transaction {
    fn verify_work(&self) -> VerifyWork {
        // A coinbase is minted by the producer and carries no authorization to
        // check, so it is the one genuinely free transaction.
        if matches!(self.tx_type, TransactionType::Coinbase) {
            return VerifyWork::NONE;
        }

        // Base: one ed25519 verify per signed input, and never fewer than one
        // — every non-coinbase transaction is authorized by at least one
        // signature, and a transaction that presents none is still checked
        // (and rejected) before it can be free.
        let signed_inputs = self
            .inputs
            .iter()
            .filter(|input| input.signature.is_some())
            .count() as u32;
        let mut work = VerifyWork::ed25519(signed_inputs.max(1));

        // Type-specific authorizing signatures, beyond the input signatures.
        // Each entry is one signature the verifier checks in addition.
        work.ed25519 = work.ed25519.saturating_add(match self.tx_type {
            // Allowlisted governance-operator signature (#2055).
            TransactionType::OracleRegistration => 1,
            // Governance signature plus the owner possession proof (#2537).
            TransactionType::StationRegistration => 2,
            // One per listed governance signature (#3277 S4). Allowlist
            // membership is checked before any curve operation, so a hostile
            // list is refused before it costs this much; an unparsable
            // payload is rejected outright and charged one.
            TransactionType::FeatureScheduleExtension => self
                .parse_feature_schedule_extension()
                .map(|signed| u32::try_from(signed.signatures.len()).unwrap_or(u32::MAX))
                .unwrap_or(1),
            // The oracle's attestation signature over the commit id.
            TransactionType::OracleAttestation => 1,
            // Equivocation evidence is two conflicting signed headers.
            TransactionType::SlashingReport => 2,
            // Conservative maximum: reporter, original commit, 16 optional attestations.
            // This type-only slice refuses admission and apply regardless of price.
            TransactionType::SlashEvidenceReport => 2 + 16,
            TransactionType::NodeFraudReport => crate::consensus::node_fraud::parse(self).map(|p| 1 + 4 * p.batch.claims.len() as u32).unwrap_or(9),
            TransactionType::NodeServiceBatch => crate::consensus::node_receipt_registry::NodeReceiptSubmission::parse(self)
                .map(|p| 1 + 4 * p.batch.claims.len() as u32).unwrap_or(1 + 4 * crate::consensus::node_share_contracts::MAX_NODE_CLAIMS as u32),
            // Register: owner signature + session-key possession proof
            // (charged for the larger case; revoke verifies one).
            TransactionType::SessionKeyAction => 2,
            TransactionType::NodeBondLifecycle => crate::consensus::node_bond_registry::NodeBondLifecycle::parse(self)
                .map(|p| if matches!(p.signed.authorization.action,
                    crate::consensus::node_share_contracts::NodeBondAction::Deposit | crate::consensus::node_share_contracts::NodeBondAction::Update) { 5 } else { 1 })
                .unwrap_or(5),
            _ => 0,
        });

        // Proof-bearing payloads.
        match self.tx_type {
            TransactionType::RewardClaim => {
                // A reward claim is always an MLSAG plus one range proof over
                // the amount commitment. An absent or malformed payload is
                // charged the maximum in BOTH dimensions: it must never be
                // cheaper to send something this module cannot read than to
                // send the real thing.
                match self.priced_privacy_payload() {
                    PricedPayloadView::Unparseable => {
                        work = work.saturating_add(PricedPayloadView::unparseable_work());
                    },
                    PricedPayloadView::Parsed(payload) => {
                        work.ring_members = work.ring_members.saturating_add(
                            payload
                                .ring_signature
                                .as_deref()
                                .map_or(MAX_RING_SIZE, ring_members_in),
                        );
                        work.range_proofs = work.range_proofs.saturating_add(1);
                    },
                }
            },
            TransactionType::ConfidentialTransfer => {
                match self.priced_privacy_payload() {
                    // Same rule as the reward claim, and the reason this arm
                    // exists as a `match`: charging only the ring here is what
                    // let a malformed payload skip range-proof pricing.
                    PricedPayloadView::Unparseable => {
                        work = work.saturating_add(PricedPayloadView::unparseable_work());
                    },
                    PricedPayloadView::Parsed(payload) => {
                        if let Some(ring) = payload.ring_signature.as_deref() {
                            work.ring_members =
                                work.ring_members.saturating_add(ring_members_in(ring));
                        }
                        if payload.range_proof.is_some() {
                            work.range_proofs = work.range_proofs.saturating_add(1);
                        }
                        // Each selective disclosure is its own verification.
                        work.range_proofs = work.range_proofs.saturating_add(
                            payload.selective_disclosures.len().min(u32::MAX as usize) as u32,
                        );
                    },
                }
            },
            // Shielded value movement and rollup batches each carry one SNARK.
            TransactionType::ShieldedDeposit
            | TransactionType::ShieldedWithdrawal
            | TransactionType::Layer2BatchSubmit => {
                work.groth16 = work.groth16.saturating_add(1);
            },
            // #2169: a transparent transfer may be authorized by a WebAuthn
            // P-256 passkey assertion (`passkey_auth`) instead of the ed25519
            // signature. That is one P-256 ECDSA verification on top of the
            // base ed25519 term, so a block of passkey transfers imposes real
            // verification work the #2143 budget must count. Detected with the
            // same lenient parse consensus uses: a malformed `passkey_auth`
            // member reads as absent and is not charged, exactly as it is
            // refused on the verification path. A payload that also carries a
            // valid ed25519 signature is still charged the ECDSA even though
            // verification short-circuits on ed25519 — over-charging is the
            // fail-expensive side and is intended.
            TransactionType::Transfer
                if self
                    .parse_transparent_transfer_payload()
                    .is_some_and(|payload| payload.passkey_auth.is_some()) =>
            {
                work.ecdsa = work.ecdsa.saturating_add(1);
            },
            _ => {},
        }

        work
    }
}

/// The result of trying to read a privacy payload for pricing.
///
/// Kept as an explicit two-state view rather than a defaulted struct so the
/// unparseable case cannot silently inherit a zero in some dimension — which
/// is exactly the bug the [`MAX_PRICED_RANGE_PROOFS`] note describes.
enum PricedPayloadView {
    Parsed(PricedPrivacyPayload),
    /// Absent, or present and unreadable. Charged the maximum in every
    /// dimension the type could have carried.
    Unparseable,
}

impl PricedPayloadView {
    /// The work an unparseable payload is charged, for a type that carries a
    /// ring and range proofs.
    fn unparseable_work() -> VerifyWork {
        VerifyWork {
            ring_members: MAX_RING_SIZE,
            range_proofs: MAX_PRICED_RANGE_PROOFS,
            ..VerifyWork::NONE
        }
    }
}

/// Private helper: read the privacy fields this module prices.
trait PricedPayload {
    fn priced_privacy_payload(&self) -> PricedPayloadView;
}

impl PricedPayload for Transaction {
    fn priced_privacy_payload(&self) -> PricedPayloadView {
        match self.data.as_deref() {
            None => PricedPayloadView::Unparseable,
            Some(raw) => match serde_json::from_str(raw) {
                Ok(parsed) => PricedPayloadView::Parsed(parsed),
                Err(_) => PricedPayloadView::Unparseable,
            },
        }
    }
}

/// Total verification work a block's transactions force on a validator.
pub fn block_verify_work<'a, I>(transactions: I) -> VerifyWork
where
    I: IntoIterator<Item = &'a Transaction>,
{
    transactions
        .into_iter()
        .fold(VerifyWork::NONE, |total, tx| {
            total.saturating_add(tx.verify_work())
        })
}

/// Why a block was refused on verification cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockVerifyBudgetExceeded {
    pub units: VerifyUnits,
    pub budget: u64,
}

impl std::fmt::Display for BlockVerifyBudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "block verification cost {} exceeds the per-block budget of {} vu (#2143): a proposer \
             may not force validators to spend unbounded CPU on a legal-looking block",
            self.units, self.budget
        )
    }
}

impl std::error::Error for BlockVerifyBudgetExceeded {}

/// Whether a block's transactions fit the per-block verification budget.
///
/// The producer asks this before sealing and every applier asks it before
/// accepting, so an over-budget block is never produced and never accepted.
pub fn check_block_verify_budget<'a, I>(
    transactions: I,
    pricing: &VerifyPricing,
) -> Result<VerifyUnits, BlockVerifyBudgetExceeded>
where
    I: IntoIterator<Item = &'a Transaction>,
{
    let units = pricing.units(&block_verify_work(transactions));
    if units.0 > pricing.block_verify_budget {
        return Err(BlockVerifyBudgetExceeded {
            units,
            budget: pricing.block_verify_budget,
        });
    }
    Ok(units)
}

/// The minimum fee a transaction must pay for the verification work it forces,
/// in base units, at `fee_per_unit`.
///
/// Admission policy, not block validity: a node may refuse to relay an
/// underpaying transaction without any two nodes having to agree on the
/// number. Block validity is the budget above, which every node computes
/// identically.
pub fn minimum_fee_for(
    transaction: &Transaction,
    pricing: &VerifyPricing,
    fee_per_unit: u64,
) -> u64 {
    pricing
        .units(&transaction.verify_work())
        .0
        .saturating_mul(fee_per_unit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::TransactionInput;

    fn tx(tx_type: TransactionType, data: Option<&str>, signed_inputs: usize) -> Transaction {
        let inputs: Vec<TransactionInput> = (0..signed_inputs)
            .map(|i| TransactionInput {
                txid: crate::crypto::hash::hash_bytes(&[i as u8]),
                vout: 0,
                signature: Some("00".into()),
                public_key: Some("00".into()),
                coinbase_data: None,
            })
            .collect();
        let mut transaction = Transaction::new(inputs, Vec::new());
        transaction.tx_type = tx_type;
        transaction.data = data.map(str::to_string);
        transaction
    }

    /// #3277 S4: an extension is charged one ed25519 unit per listed
    /// governance signature, and an unparsable one is charged one.
    #[test]
    fn feature_schedule_extension_is_charged_per_signature_3277() {
        use crate::governance::schedule_extension::{
            FeatureScheduleExtension, GovernanceSignature, ScheduleEntry,
            SignedFeatureScheduleExtension,
        };
        let signature = |i: u8| GovernanceSignature {
            signer_pubkey: format!("{i:02x}").repeat(32),
            signature: "00".repeat(64),
        };
        let signed = SignedFeatureScheduleExtension {
            extension: FeatureScheduleExtension {
                chain_id: "777".into(),
                network_id: "testnet".into(),
                genesis_hash: "ab".repeat(32),
                sequence: 0,
                entries: vec![ScheduleEntry::new("verify_unit_pricing", 5_000)],
                version_steps: Vec::new(),
                min_protocol_version: 3,
                release_commit: "c4d0a11".into(),
            },
            signatures: vec![signature(1), signature(2), signature(3)],
        };
        let three = Transaction::new_feature_schedule_extension(&signed);
        // One unit for the (unsigned) input plus one per signature.
        assert_eq!(three.verify_work().ed25519, 1 + 3);
        let unparsable = tx(TransactionType::FeatureScheduleExtension, Some("nope"), 0);
        assert_eq!(unparsable.verify_work().ed25519, 1 + 1);
    }

    /// A hex-encoded JSON MLSAG with `members` ring entries — the wire shape
    /// `RewardClaimPayload::ring_signature` carries.
    fn ring_of(members: usize) -> String {
        let ring: Vec<serde_json::Value> = (0..members)
            .map(|i| serde_json::json!({ "public_key": format!("{i:064x}") }))
            .collect();
        let json = serde_json::json!({ "key_image": "00", "c": "00", "s": [], "ring": ring });
        hex::encode(serde_json::to_vec(&json).expect("serialize"))
    }

    fn reward_claim(members: usize) -> Transaction {
        let payload = serde_json::json!({
            "epoch": 1,
            "ring_signature": ring_of(members),
            "key_image": "00",
            "amount_commitment": "00",
            "range_proof": "00",
            "destination": "0x00",
        });
        tx(TransactionType::RewardClaim, Some(&payload.to_string()), 1)
    }

    #[test]
    fn a_coinbase_is_the_only_free_transaction() {
        assert!(tx(TransactionType::Coinbase, None, 0).verify_work().is_free());
        assert!(!tx(TransactionType::Transfer, None, 1).verify_work().is_free());
        // Even a transaction presenting no signature is priced for one.
        assert_eq!(tx(TransactionType::Transfer, None, 0).verify_work().ed25519, 1);
    }

    #[test]
    fn ordinary_transfers_are_priced_per_signed_input() {
        assert_eq!(tx(TransactionType::Transfer, None, 1).verify_work().ed25519, 1);
        assert_eq!(tx(TransactionType::Transfer, None, 5).verify_work().ed25519, 5);
        let pricing = VerifyPricing::CALIBRATED;
        assert_eq!(
            pricing.units(&tx(TransactionType::Transfer, None, 5).verify_work()),
            VerifyUnits(500)
        );
    }

    /// #2169 × #2143: a transparent transfer authorized by a WebAuthn P-256
    /// passkey is charged one ECDSA verification on top of the base ed25519
    /// term, so the whole-block verify budget counts the real work. A plain
    /// transfer and a transfer whose `passkey_auth` member is malformed (and so
    /// reads as absent under the lenient parse) are unchanged.
    #[test]
    fn a_passkey_transfer_is_priced_for_one_extra_p256_verification() {
        use crate::accounts::passkey::PasskeyAuthorization;
        use crate::blockchain::transaction::TransparentTransferPayload;

        // The hex bytes need not verify — pricing only detects the member's
        // presence via the same parse consensus uses; all five fields present
        // makes it a well-formed `PasskeyAuthorization`.
        let passkey = PasskeyAuthorization {
            public_key_p256: "00".into(),
            rp_id_hash: "00".into(),
            authenticator_data: "00".into(),
            client_data_json: "00".into(),
            signature: "00".into(),
        };
        let transfer_with = |auth: Option<PasskeyAuthorization>| {
            let payload = TransparentTransferPayload {
                from: "0x0000000000000000000000000000000000000001".into(),
                to: "0x0000000000000000000000000000000000000002".into(),
                amount: 10,
                transparent_opt_out: true,
                opt_out_audit_tag: None,
                auth_pubkey: None,
                auth_signature: None,
                authority: None,
                authority_signature: None,
                passkey_auth: auth,
                sponsorship: None,
                session_key_auth: None,
            };
            tx(
                TransactionType::Transfer,
                Some(&serde_json::to_string(&payload).expect("serialize")),
                1,
            )
        };
        let pricing = VerifyPricing::CALIBRATED;

        // Well-formed passkey_auth: the base ed25519 (one signed input) PLUS
        // one P-256 ECDSA. This `with` case also carries an input signature, so
        // it is the both-present over-charge case, priced expensive on purpose.
        let with = transfer_with(Some(passkey));
        assert_eq!(with.verify_work().ed25519, 1);
        assert_eq!(with.verify_work().ecdsa, 1);
        assert_eq!(pricing.units(&with.verify_work()), VerifyUnits(100 + 250));

        // Plain transfer: no passkey_auth, no ECDSA term.
        let plain = transfer_with(None);
        assert_eq!(plain.verify_work().ecdsa, 0);
        assert_eq!(pricing.units(&plain.verify_work()), VerifyUnits(100));

        // Malformed passkey_auth member (missing fields) reads as absent under
        // the lenient deserializer, so it is not charged the ECDSA.
        let malformed_json = r#"{"from":"0x0000000000000000000000000000000000000001","to":"0x0000000000000000000000000000000000000002","amount":10,"transparent_opt_out":true,"passkey_auth":{"public_key_p256":"00"}}"#;
        let malformed = tx(TransactionType::Transfer, Some(malformed_json), 1);
        assert_eq!(malformed.verify_work().ecdsa, 0);
        assert_eq!(pricing.units(&malformed.verify_work()), VerifyUnits(100));
    }

    /// The acceptance criterion: cost scales with ring size, not per transaction.
    #[test]
    fn reward_claim_cost_scales_with_ring_size() {
        let pricing = VerifyPricing::CALIBRATED;
        let small = pricing.units(&reward_claim(2).verify_work());
        let medium = pricing.units(&reward_claim(64).verify_work());
        let large = pricing.units(&reward_claim(512).verify_work());
        assert!(small < medium && medium < large, "{small} {medium} {large}");

        // Linear in members, exactly: 64 members priced at parity with ed25519.
        assert_eq!(reward_claim(64).verify_work().ring_members, 64);
        let delta = large.0 - medium.0;
        assert_eq!(delta, (512 - 64) * pricing.per_ring_sig_member);

        // A flat fee would have made these identical; that is the defect.
        assert_ne!(small, large);
    }

    /// The pricing ceiling and the verifier's ceiling are one number. If the
    /// privacy crate ever raises `MAX_RING_SIZE`, pricing that kept the old
    /// value would under-charge every ring between the two — silently
    /// reopening the vector this module closes.
    #[test]
    fn max_ring_size_agrees_with_the_verifier() {
        assert_eq!(
            MAX_RING_SIZE as usize,
            wrfcoin_privacy::ring_signatures::MAX_RING_SIZE,
            "pricing ceiling drifted from the MLSAG verifier's own ceiling"
        );
    }

    #[test]
    fn a_ring_is_never_charged_above_the_verifier_ceiling() {
        // A ring larger than MAX_RING_SIZE is REJECTED by the verifier rather
        // than verified, so it cannot cost more than the ceiling.
        let work = reward_claim(MAX_RING_SIZE as usize + 500).verify_work();
        assert_eq!(work.ring_members, MAX_RING_SIZE);
    }

    /// The fail-expensive rule: an unreadable proof is never a cheap proof.
    #[test]
    fn an_unparseable_ring_is_charged_the_maximum() {
        assert_eq!(ring_members_in("not hex"), MAX_RING_SIZE);
        assert_eq!(ring_members_in(""), MAX_RING_SIZE);
        assert_eq!(ring_members_in(&hex::encode(b"{not json")), MAX_RING_SIZE);

        // ... and through the whole transaction, for every way the payload can
        // be missing or malformed.
        for broken in [None, Some("not json"), Some("{}"), Some(r#"{"ring_signature":"zz"}"#)] {
            let work = tx(TransactionType::RewardClaim, broken, 1).verify_work();
            assert_eq!(
                work.ring_members, MAX_RING_SIZE,
                "payload {broken:?} must be charged the maximum, not zero"
            );
        }

        // A maximal ring is genuinely expensive under the committed pricing.
        let pricing = VerifyPricing::CALIBRATED;
        let units = pricing.units(&tx(TransactionType::RewardClaim, None, 1).verify_work());
        assert!(units.0 >= MAX_RING_SIZE as u64 * pricing.per_ring_sig_member);
    }

    /// The fail-expensive rule, stated over EVERY dimension rather than just
    /// the ring.
    ///
    /// An earlier revision charged an unparseable payload a maximal ring and
    /// ZERO range proofs, so a malformed `ConfidentialTransfer` skipped
    /// range-proof pricing while still forcing work.
    ///
    /// Note what is and is not claimed. The charge for an unreadable payload
    /// is the committed CEILING in every dimension, and that ceiling sits
    /// above a realistic payload — so making a payload unreadable never turns
    /// it cheap. It is NOT claimed to exceed every conceivable parseable
    /// payload: a transaction genuinely carrying more than
    /// `MAX_PRICED_RANGE_PROOFS` proofs pays for what it carries, which is
    /// correct pricing. Clamping the parsed count to the ceiling instead
    /// would under-charge exactly the large payloads that cost the most to
    /// verify.
    #[test]
    fn an_unreadable_payload_is_charged_the_ceiling_in_every_dimension() {
        let pricing = VerifyPricing::CALIBRATED;

        for tx_type in [
            TransactionType::RewardClaim,
            TransactionType::ConfidentialTransfer,
        ] {
            // A realistic payload: a plausible ring and one amount proof.
            let realistic_payload = serde_json::json!({
                "epoch": 1,
                "ring_signature": ring_of(16),
                "key_image": "00",
                "amount_commitment": "00",
                "range_proof": "00",
                "destination": "0x00",
                "output_commitments": [],
                "key_images": [],
                "privacy_level": "Confidential",
            });
            let realistic =
                tx(tx_type, Some(&realistic_payload.to_string()), 1).verify_work();

            for (label, unreadable) in [
                ("malformed", tx(tx_type, Some("{not json"), 1).verify_work()),
                ("absent", tx(tx_type, None, 1).verify_work()),
            ] {
                // Charged the ceiling in BOTH dimensions — the range dimension
                // is the one the earlier revision left at zero.
                assert_eq!(
                    unreadable.ring_members, MAX_RING_SIZE,
                    "{tx_type} {label}: ring ceiling"
                );
                assert_eq!(
                    unreadable.range_proofs, MAX_PRICED_RANGE_PROOFS,
                    "{tx_type} {label}: RANGE-PROOF ceiling, not zero"
                );
                // And unreadable is decisively more expensive than realistic,
                // so garbage is never the cheap option.
                assert!(
                    pricing.units(&unreadable) > pricing.units(&realistic),
                    "{tx_type} {label}: {} must exceed a realistic payload's {}",
                    pricing.units(&unreadable),
                    pricing.units(&realistic)
                );
            }

        }

        // `{}` is a DIFFERENT case from unreadable: it PARSES, it is simply
        // empty, so it takes the parsed path — and the two types differ there,
        // correctly.
        //
        // A `RewardClaim` always carries a ring (it is the whole mechanism),
        // so an absent one means the claim cannot be priced from its contents
        // and is charged the maximum.
        let empty_claim = tx(TransactionType::RewardClaim, Some("{}"), 1).verify_work();
        assert_eq!(
            empty_claim.ring_members, MAX_RING_SIZE,
            "a reward claim with no ring is charged the maximum: the field is mandatory"
        );

        // A `ConfidentialTransfer`'s ring is OPTIONAL — the payload type
        // declares it `Option<String>`, and a lower privacy level legitimately
        // carries none. Charging the maximum for its absence would over-charge
        // honest traffic, so an absent ring costs nothing and the transfer is
        // priced on what it actually carries.
        let empty_transfer =
            tx(TransactionType::ConfidentialTransfer, Some("{}"), 1).verify_work();
        assert_eq!(
            empty_transfer.ring_members, 0,
            "an absent OPTIONAL ring is not charged; only a mandatory one is"
        );
        assert_eq!(empty_transfer.range_proofs, 0);
    }

    #[test]
    fn snark_and_range_proof_bearing_types_are_priced_for_them() {
        for snark_type in [
            TransactionType::ShieldedDeposit,
            TransactionType::ShieldedWithdrawal,
            TransactionType::Layer2BatchSubmit,
        ] {
            assert_eq!(tx(snark_type, None, 1).verify_work().groth16, 1, "{snark_type}");
        }
        let payload = serde_json::json!({
            "output_commitments": [],
            "range_proof": "00",
            "ring_signature": ring_of(8),
            "key_images": [],
            "privacy_level": "Confidential",
            "selective_disclosures": [{"a":1}, {"b":2}],
        });
        let work = tx(
            TransactionType::ConfidentialTransfer,
            Some(&payload.to_string()),
            1,
        )
        .verify_work();
        assert_eq!(work.ring_members, 8);
        assert_eq!(work.range_proofs, 3, "one range proof plus two disclosures");
    }

    #[test]
    fn type_specific_authorizing_signatures_are_counted() {
        assert_eq!(tx(TransactionType::OracleRegistration, None, 1).verify_work().ed25519, 2);
        assert_eq!(tx(TransactionType::StationRegistration, None, 1).verify_work().ed25519, 3);
        assert_eq!(tx(TransactionType::OracleAttestation, None, 1).verify_work().ed25519, 2);
        assert_eq!(tx(TransactionType::SlashingReport, None, 1).verify_work().ed25519, 3);
    }

    /// The acceptance criterion: a block over the budget is rejected.
    #[test]
    fn the_block_budget_rejects_an_over_budget_block_and_admits_an_honest_one() {
        let pricing = VerifyPricing::CALIBRATED;

        // An honest block: a hundred ordinary transfers is nowhere near it.
        let honest: Vec<Transaction> =
            (0..100).map(|_| tx(TransactionType::Transfer, None, 2)).collect();
        let units = check_block_verify_budget(&honest, &pricing).expect("honest block fits");
        assert_eq!(units, VerifyUnits(100 * 2 * pricing.per_ed25519));

        // A proposer stuffing maximal-ring reward claims is refused.
        let hostile: Vec<Transaction> = (0..20).map(|_| reward_claim(MAX_RING_SIZE as usize)).collect();
        let err = check_block_verify_budget(&hostile, &pricing).expect_err("over budget");
        assert!(err.units.0 > pricing.block_verify_budget);
        assert!(err.to_string().contains("#2143"), "{err}");

        // An empty block costs nothing.
        assert_eq!(check_block_verify_budget(&[], &pricing), Ok(VerifyUnits::ZERO));
    }

    #[test]
    fn pricing_and_totals_saturate_rather_than_wrap() {
        let absurd = VerifyPricing {
            per_ring_sig_member: u64::MAX,
            ..VerifyPricing::CALIBRATED
        };
        let units = absurd.units(&VerifyWork {
            ring_members: MAX_RING_SIZE,
            ..VerifyWork::NONE
        });
        assert_eq!(units, VerifyUnits(u64::MAX), "saturates, never wraps to a small number");

        let work = VerifyWork {
            ed25519: u32::MAX,
            ..VerifyWork::NONE
        }
        .saturating_add(VerifyWork::ed25519(10));
        assert_eq!(work.ed25519, u32::MAX);
    }

    #[test]
    fn the_fee_floor_is_a_function_of_verify_units() {
        let pricing = VerifyPricing::CALIBRATED;
        let cheap = tx(TransactionType::Transfer, None, 1);
        let dear = reward_claim(512);
        let cheap_fee = minimum_fee_for(&cheap, &pricing, 1);
        let dear_fee = minimum_fee_for(&dear, &pricing, 1);
        assert!(dear_fee > cheap_fee * 100, "{dear_fee} vs {cheap_fee}");
        // Zero rate prices nothing; the function is linear in the rate.
        assert_eq!(minimum_fee_for(&dear, &pricing, 0), 0);
        assert_eq!(minimum_fee_for(&dear, &pricing, 2), dear_fee * 2);
    }
}
