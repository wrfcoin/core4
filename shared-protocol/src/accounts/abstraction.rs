use crate::common::types::{Address, BlockNumber, ChainId, Nonce, Timestamp};
use crate::crypto::chain_binding::{SigningProtocolVersion, protocol_version_at};
use crate::crypto::hash::hash_bytes;
use crate::crypto::signature::PublicKey;
use rustc_hash::{FxHashMap, FxHashSet};
use thiserror::Error;

const ACCOUNT_OP_DOMAIN_TAG: &[u8] = b"wrfcoin.account_abstraction.userop.v1";
const PAYMASTER_AUTH_DOMAIN_TAG: &[u8] = b"wrfcoin.account_abstraction.paymaster.v1";

#[derive(Debug, Clone)]
pub struct UserOperation {
    pub sender: Address,
    pub nonce: Nonce,
    pub call_data_hash: [u8; 32],
    pub max_fee_per_gas: u128,
    pub signer_public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub valid_after: Option<Timestamp>,
    pub valid_until: Option<Timestamp>,
    /// Ceiling on the work the account's validation program may do (#2168).
    ///
    /// Arbitrary validation logic with no ceiling is a free-CPU mempool DoS:
    /// an operation that costs a node a great deal to reject costs its sender
    /// nothing. `0` admits no validation work at all, which is the
    /// fail-closed default for an operation that forgot to set it.
    pub validation_gas_limit: u64,
    /// Ceiling on the execution that follows a successful validation.
    pub call_gas_limit: u64,
    /// Optional sponsor for this operation's fees (#2171 builds on this).
    pub paymaster: Option<PaymasterData>,
}

/// A sponsor's offer to pay for an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymasterData {
    pub paymaster: Address,
    pub max_fee_per_gas: u128,
    /// Sponsor's authorization over the operation hash.
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct ValidationContext {
    pub chain_id: ChainId,
    pub block_number: BlockNumber,
    pub timestamp: Timestamp,
}

#[derive(Debug, Clone)]
pub struct ValidationOutcome {
    pub accepted: bool,
    pub reason: Option<String>,
}

impl ValidationOutcome {
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            reason: None,
        }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Error)]
pub enum AccountValidationError {
    #[error("operation is outside validity window")]
    InvalidTimeWindow,
    #[error("invalid operation signature")]
    InvalidSignature,
    #[error("replay detected for operation hash {0:?}")]
    ReplayDetected([u8; 32]),
    #[error("nonce mismatch for account {address}: expected {expected}, got {actual}")]
    NonceMismatch {
        address: Address,
        expected: Nonce,
        actual: Nonce,
    },
    #[error("validation gas exhausted: limit {limit}, spent {spent}")]
    ValidationGasExhausted { limit: u64, spent: u64 },
    #[error("gas sponsorship is not allowed for account {account}")]
    SponsorshipNotAllowed { account: Address },
    #[error(
        "sponsored fee is below operation fee requirement: required {required}, provided {provided}"
    )]
    SponsoredFeeBelowOperationFee { required: u128, provided: u128 },
    #[error("sponsored fee exceeds policy limit: limit {limit}, provided {provided}")]
    SponsoredFeeAbovePolicyLimit { limit: u128, provided: u128 },
}

/// A bounded budget for validation work (#2168).
///
/// The rule the story exists for: **charge for validation work whether or not
/// the operation succeeds.** An operation that is expensive to reject is the
/// same DoS as one that is expensive to accept — worse, because a rejected
/// operation pays no fee at all unless the charge is recorded. So `charge`
/// deducts first and reports exhaustion second, and `spent()` is readable
/// after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GasMeter {
    limit: u64,
    spent: u64,
}

impl GasMeter {
    pub const fn new(limit: u64) -> Self {
        Self { limit, spent: 0 }
    }

    /// Charge `amount`. Returns `Err` once the budget is exhausted, having
    /// recorded the spend either way (saturating, so a huge charge cannot wrap
    /// the counter back under the limit).
    pub fn charge(&mut self, amount: u64) -> Result<(), AccountValidationError> {
        self.spent = self.spent.saturating_add(amount);
        if self.spent > self.limit {
            return Err(AccountValidationError::ValidationGasExhausted {
                limit: self.limit,
                spent: self.spent,
            });
        }
        Ok(())
    }

    /// Work charged so far — valid to read after a failure, which is the
    /// point: the caller bills it.
    pub const fn spent(&self) -> u64 {
        self.spent
    }

    pub const fn limit(&self) -> u64 {
        self.limit
    }

    pub const fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.spent)
    }

    pub const fn is_exhausted(&self) -> bool {
        self.spent > self.limit
    }
}

/// What a successful validation reports back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationResult {
    pub valid_until: Option<Timestamp>,
    pub valid_after: Option<Timestamp>,
    /// Optional signature aggregator this account's operations may be batched
    /// under.
    pub aggregator: Option<Address>,
    /// Validation work actually charged, success or failure.
    pub gas_spent: u64,
}

/// An account that validates its own operations (#2168).
///
/// This is the shape `authorization::AuthorizationRule::SmartAccount`
/// dispatches to. Validation is handed a [`GasMeter`] rather than being
/// trusted to bound itself: an account program is attacker-authored, so the
/// ceiling has to be enforced by the caller's meter, not by the program's
/// good manners.
pub trait SmartAccount: Send + Sync {
    fn validate_user_op(
        &self,
        op: &UserOperation,
        hash: [u8; 32],
        meter: &mut GasMeter,
    ) -> Result<ValidationResult, AccountValidationError>;
}

/// Per-step validation charges. Committed constants so two nodes meter an
/// identical operation identically.
pub mod gas_costs {
    /// Charged once, for admitting the operation at all.
    pub const BASE: u64 = 100;
    /// One signature verification.
    pub const SIGNATURE_VERIFY: u64 = 1_000;
    /// Per byte of signature material, so an oversized signature is not free
    /// to reject.
    pub const PER_SIGNATURE_BYTE: u64 = 2;
    /// Nonce lookup and replay-set probe.
    pub const NONCE_CHECK: u64 = 50;
    /// Sponsorship policy evaluation.
    pub const SPONSORSHIP_CHECK: u64 = 200;
}

pub trait AccountValidator: Send + Sync {
    fn validate_user_operation(
        &mut self,
        op: &UserOperation,
        context: ValidationContext,
    ) -> Result<ValidationOutcome, AccountValidationError>;
}

#[derive(Debug, Default, Clone)]
pub struct NonceManager {
    account_nonces: FxHashMap<Address, Nonce>,
}

impl NonceManager {
    pub fn expected_nonce(&self, account: &Address) -> Nonce {
        self.account_nonces
            .get(account)
            .copied()
            .unwrap_or_default()
    }

    pub fn consume_nonce(
        &mut self,
        account: Address,
        provided_nonce: Nonce,
    ) -> Result<(), AccountValidationError> {
        let expected = self.expected_nonce(&account);
        if expected != provided_nonce {
            return Err(AccountValidationError::NonceMismatch {
                address: account,
                expected,
                actual: provided_nonce,
            });
        }

        self.account_nonces
            .insert(account, expected.saturating_add(Nonce::new(1)));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SmartAccountPolicy {
    pub allow_gas_sponsorship: bool,
    pub max_sponsored_fee_per_op: u128,
}

impl Default for SmartAccountPolicy {
    fn default() -> Self {
        Self {
            allow_gas_sponsorship: false,
            max_sponsored_fee_per_op: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SponsorshipRequest {
    pub sponsor: Address,
    pub max_fee_per_gas: u128,
}

pub trait SignatureVerifier: Send + Sync {
    fn verify(&self, message: &[u8], signature: &[u8], signer_public_key: &[u8]) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicSignatureVerifier;

impl SignatureVerifier for BasicSignatureVerifier {
    fn verify(&self, message: &[u8], signature: &[u8], signer_public_key: &[u8]) -> bool {
        let Ok(public_key) = PublicKey::from_bytes(signer_public_key) else {
            return false;
        };
        public_key.verify_signature(message, signature)
    }
}

#[derive(Debug, Clone)]
pub struct ValidationPipeline<V: SignatureVerifier> {
    nonce_manager: NonceManager,
    policies: FxHashMap<Address, SmartAccountPolicy>,
    seen_operation_hashes: FxHashSet<[u8; 32]>,
    signature_verifier: V,
}

impl<V: SignatureVerifier> ValidationPipeline<V> {
    pub fn new(signature_verifier: V) -> Self {
        Self {
            nonce_manager: NonceManager::default(),
            policies: FxHashMap::default(),
            seen_operation_hashes: FxHashSet::default(),
            signature_verifier,
        }
    }

    pub fn set_policy(&mut self, account: Address, policy: SmartAccountPolicy) {
        self.policies.insert(account, policy);
    }

    pub fn policy_for(&self, account: &Address) -> SmartAccountPolicy {
        self.policies.get(account).copied().unwrap_or_default()
    }

    pub fn expected_nonce(&self, account: &Address) -> Nonce {
        self.nonce_manager.expected_nonce(account)
    }

    pub fn validate_and_apply(
        &mut self,
        op: &UserOperation,
        context: ValidationContext,
        sponsorship: Option<SponsorshipRequest>,
    ) -> Result<ValidationOutcome, AccountValidationError> {
        let mut meter = GasMeter::new(op.validation_gas_limit);
        self.validate_metered(op, context, sponsorship, &mut meter)
            .map(|_| ValidationOutcome::accepted())
    }

    /// [`Self::validate_and_apply`] with the meter exposed, so a caller that
    /// bills for rejected operations can read what the attempt cost (#2168).
    ///
    /// Every step charges BEFORE it runs. That ordering is the whole point: a
    /// check that charged afterwards would let an operation exhaust a node's
    /// CPU and then fail for free, which is the DoS the limit exists to stop.
    pub fn validate_metered(
        &mut self,
        op: &UserOperation,
        context: ValidationContext,
        sponsorship: Option<SponsorshipRequest>,
        meter: &mut GasMeter,
    ) -> Result<ValidationResult, AccountValidationError> {
        meter.charge(gas_costs::BASE)?;
        validate_time_window(op, context.timestamp)?;

        let op_hash = user_operation_hash(op, context);

        meter.charge(gas_costs::NONCE_CHECK)?;
        if self.seen_operation_hashes.contains(&op_hash) {
            return Err(AccountValidationError::ReplayDetected(op_hash));
        }

        // Charged by SIZE as well as per verification: an oversized signature
        // costs real work to reject, and a flat charge would make megabyte
        // signatures free to submit.
        meter.charge(gas_costs::SIGNATURE_VERIFY)?;
        meter.charge((op.signature.len() as u64).saturating_mul(gas_costs::PER_SIGNATURE_BYTE))?;
        if !self
            .signature_verifier
            .verify(&op_hash, &op.signature, &op.signer_public_key)
        {
            return Err(AccountValidationError::InvalidSignature);
        }

        if sponsorship.is_some() || op.paymaster.is_some() {
            meter.charge(gas_costs::SPONSORSHIP_CHECK)?;
        }
        self.validate_sponsorship(op, sponsorship)?;

        self.nonce_manager.consume_nonce(op.sender, op.nonce)?;
        self.seen_operation_hashes.insert(op_hash);
        Ok(ValidationResult {
            valid_until: op.valid_until,
            valid_after: op.valid_after,
            aggregator: None,
            gas_spent: meter.spent(),
        })
    }

    fn validate_sponsorship(
        &self,
        op: &UserOperation,
        sponsorship: Option<SponsorshipRequest>,
    ) -> Result<(), AccountValidationError> {
        if let Some(sponsorship) = sponsorship {
            let policy = self.policy_for(&op.sender);
            if !policy.allow_gas_sponsorship {
                return Err(AccountValidationError::SponsorshipNotAllowed { account: op.sender });
            }
            if sponsorship.max_fee_per_gas < op.max_fee_per_gas {
                return Err(AccountValidationError::SponsoredFeeBelowOperationFee {
                    required: op.max_fee_per_gas,
                    provided: sponsorship.max_fee_per_gas,
                });
            }
            if sponsorship.max_fee_per_gas > policy.max_sponsored_fee_per_op {
                return Err(AccountValidationError::SponsoredFeeAbovePolicyLimit {
                    limit: policy.max_sponsored_fee_per_op,
                    provided: sponsorship.max_fee_per_gas,
                });
            }
        }
        Ok(())
    }
}

impl Default for ValidationPipeline<BasicSignatureVerifier> {
    fn default() -> Self {
        Self::new(BasicSignatureVerifier)
    }
}

impl<V: SignatureVerifier> AccountValidator for ValidationPipeline<V> {
    fn validate_user_operation(
        &mut self,
        op: &UserOperation,
        context: ValidationContext,
    ) -> Result<ValidationOutcome, AccountValidationError> {
        self.validate_and_apply(op, context, None)
    }
}

pub fn validate_time_window(
    op: &UserOperation,
    now: Timestamp,
) -> Result<(), AccountValidationError> {
    if let Some(valid_after) = op.valid_after {
        if now < valid_after {
            return Err(AccountValidationError::InvalidTimeWindow);
        }
    }

    if let Some(valid_until) = op.valid_until {
        if now > valid_until {
            return Err(AccountValidationError::InvalidTimeWindow);
        }
    }

    Ok(())
}

/// The domain-separated operation hash.
///
/// #2168 binds the PROTOCOL VERSION as well as the chain id. Without it an
/// operation signed under one set of validation rules stays valid after a
/// rule change, which is the cross-version replay the crypto epic exists to
/// prevent; with it, an operation is bound to the rules it was signed under.
/// The gas limits are covered too — a sponsor who signed for a bounded amount
/// of validation work must not be bound to an unbounded one.
pub fn user_operation_hash(op: &UserOperation, context: ValidationContext) -> [u8; 32] {
    user_operation_hash_with_protocol_version(
        op,
        context,
        protocol_version_at(context.block_number.into_inner()),
    )
}

fn user_operation_hash_with_protocol_version(
    op: &UserOperation,
    context: ValidationContext,
    protocol_version: SigningProtocolVersion,
) -> [u8; 32] {
    let mut preimage = user_operation_preimage_before_signer_key(op, context, protocol_version);
    push_u32_be_prefixed(&mut preimage, &op.signer_public_key);
    append_paymaster_preimage(&mut preimage, &op.paymaster);
    hash_bytes(&preimage).into()
}

/// Write one variable-width field using the fixed-width user-operation framing.
///
/// The signer key sits before extension fields in the operation preimage. Its
/// length is therefore part of the signed bytes: without it, bytes could move
/// across the key/next-field boundary while leaving the concatenated preimage
/// unchanged.
fn push_u32_be_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    let length = u32::try_from(bytes.len())
        .expect("user-operation field exceeds u32 length");
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(bytes);
}

fn user_operation_preimage_before_signer_key(
    op: &UserOperation,
    context: ValidationContext,
    protocol_version: SigningProtocolVersion,
) -> Vec<u8> {
    let mut preimage = Vec::with_capacity(256);
    preimage.extend_from_slice(ACCOUNT_OP_DOMAIN_TAG);
    preimage.extend_from_slice(&protocol_version.as_u32().to_be_bytes());
    preimage.extend_from_slice(&context.chain_id.to_be_bytes());
    preimage.extend_from_slice(op.sender.as_bytes());
    preimage.extend_from_slice(&op.nonce.to_be_bytes());
    preimage.extend_from_slice(&op.call_data_hash);
    preimage.extend_from_slice(&op.max_fee_per_gas.to_be_bytes());
    preimage.extend_from_slice(&op.valid_after.unwrap_or_default().to_be_bytes());
    preimage.extend_from_slice(&op.valid_until.unwrap_or_default().to_be_bytes());
    preimage.extend_from_slice(&op.validation_gas_limit.to_be_bytes());
    preimage.extend_from_slice(&op.call_gas_limit.to_be_bytes());
    preimage
}

fn append_paymaster_preimage(preimage: &mut Vec<u8>, paymaster: &Option<PaymasterData>) {
    match paymaster {
        Some(paymaster) => {
            preimage.push(1);
            preimage.extend_from_slice(paymaster.paymaster.as_bytes());
            preimage.extend_from_slice(&paymaster.max_fee_per_gas.to_be_bytes());
        },
        None => preimage.push(0),
    }
}

/// Domain-separated preimage a paymaster authorizes for a sponsored operation.
///
/// The operation hash already commits to the paymaster identity and fee cap;
/// repeating those fields here makes the sponsor's authorization explicit and
/// prevents a sender signature from being replayed as a sponsor signature.
pub fn paymaster_authorization_hash(
    op: &UserOperation,
    context: ValidationContext,
) -> Option<[u8; 32]> {
    let paymaster = op.paymaster.as_ref()?;
    let op_hash = user_operation_hash(op, context);
    let mut preimage = Vec::with_capacity(PAYMASTER_AUTH_DOMAIN_TAG.len() + 68);
    preimage.extend_from_slice(PAYMASTER_AUTH_DOMAIN_TAG);
    preimage.extend_from_slice(&op_hash);
    preimage.extend_from_slice(paymaster.paymaster.as_bytes());
    preimage.extend_from_slice(&paymaster.max_fee_per_gas.to_be_bytes());
    Some(hash_bytes(&preimage).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::{SeedableRng, rngs::StdRng};

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn nonce_manager_consumes_in_order() {
        let account = addr(7);
        let mut manager = NonceManager::default();
        assert_eq!(manager.expected_nonce(&account), 0);
        manager.consume_nonce(account, Nonce::new(0)).unwrap();
        assert_eq!(manager.expected_nonce(&account), 1);
    }

    #[test]
    fn validate_time_window_rejects_out_of_range_ops() {
        let op = UserOperation {
            sender: addr(1),
            nonce: Nonce::new(0),
            call_data_hash: [0; 32],
            max_fee_per_gas: 1,
            signer_public_key: vec![],
            signature: vec![],
            valid_after: Some(Timestamp::new(10)),
            valid_until: Some(Timestamp::new(20)),
            validation_gas_limit: 1_000_000,
            call_gas_limit: 0,
            paymaster: None,
        };
        assert!(validate_time_window(&op, Timestamp::new(9)).is_err());
        assert!(validate_time_window(&op, Timestamp::new(21)).is_err());
        assert!(validate_time_window(&op, Timestamp::new(15)).is_ok());
    }

    fn sample_context() -> ValidationContext {
        ValidationContext {
            chain_id: ChainId::new(77),
            block_number: BlockNumber::new(100),
            timestamp: Timestamp::new(15),
        }
    }

    fn sample_operation(sender: Address, nonce: Nonce, max_fee_per_gas: u128) -> UserOperation {
        let mut rng = StdRng::seed_from_u64(11);
        let private_key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&private_key);
        let signer_public_key = public_key.as_bytes().to_vec();

        let mut op = UserOperation {
            sender,
            nonce,
            call_data_hash: [42; 32],
            max_fee_per_gas,
            signer_public_key,
            signature: vec![],
            valid_after: Some(Timestamp::new(10)),
            valid_until: Some(Timestamp::new(20)),
            validation_gas_limit: 1_000_000,
            call_gas_limit: 0,
            paymaster: None,
        };

        let message = user_operation_hash(&op, sample_context());
        op.signature = private_key.sign(&message).as_bytes().to_vec();
        op
    }

    #[test]
    fn operation_hash_changes_with_chain_id() {
        let op = sample_operation(addr(20), Nonce::new(0), 3);
        let mut ctx_b = sample_context();
        ctx_b.chain_id = ctx_b.chain_id.saturating_add(ChainId::new(1));
        assert_ne!(
            user_operation_hash(&op, sample_context()),
            user_operation_hash(&op, ctx_b)
        );
    }

    #[test]
    fn signer_key_length_prefix_separates_real_paymaster_tail_collision() {
        let mut a = sample_operation(addr(24), Nonce::new(0), 5);
        a.signer_public_key = vec![0xa5; 33];
        let p = addr(0x77);
        let fee: u128 = 0x0102_0304_0506_0708_090a_0b0c_0d0e_0f00;
        a.paymaster = Some(PaymasterData {
            paymaster: p,
            max_fee_per_gas: fee,
            signature: vec![],
        });

        let mut b = a.clone();
        let mut k = a.signer_public_key.clone();
        k.push(1);
        k.extend_from_slice(p.as_bytes());
        k.extend_from_slice(&fee.to_be_bytes()[..15]);
        b.signer_public_key = k;
        b.paymaster = None;

        assert_ne!(
            user_operation_hash(&a, sample_context()),
            user_operation_hash(&b, sample_context()),
            "the signer-key length prefix must separate the real paymaster tail collision"
        );
    }

    #[test]
    fn pipeline_rejects_replay_by_operation_hash() {
        let sender = addr(21);
        let mut pipeline = ValidationPipeline::default();
        let op = sample_operation(sender, Nonce::new(0), 5);
        pipeline
            .validate_and_apply(&op, sample_context(), None)
            .unwrap();
        let err = pipeline
            .validate_and_apply(&op, sample_context(), None)
            .unwrap_err();
        assert!(matches!(err, AccountValidationError::ReplayDetected(_)));
    }

    #[test]
    fn pipeline_rejects_signature_replayed_on_different_chain() {
        let sender = addr(22);
        let mut pipeline = ValidationPipeline::default();
        let op = sample_operation(sender, Nonce::new(0), 5);
        let mut wrong_chain = sample_context();
        wrong_chain.chain_id = wrong_chain.chain_id.saturating_add(ChainId::new(9));
        let err = pipeline
            .validate_and_apply(&op, wrong_chain, None)
            .unwrap_err();
        assert!(matches!(err, AccountValidationError::InvalidSignature));
    }

    #[test]
    fn pipeline_rejects_malformed_signer_key() {
        let sender = addr(23);
        let mut pipeline = ValidationPipeline::default();
        let op = UserOperation {
            sender,
            nonce: Nonce::new(0),
            call_data_hash: [42; 32],
            max_fee_per_gas: 5,
            signer_public_key: vec![1, 2, 3],
            signature: vec![1, 2, 3],
            valid_after: Some(Timestamp::new(10)),
            valid_until: Some(Timestamp::new(20)),
            validation_gas_limit: 1_000_000,
            call_gas_limit: 0,
            paymaster: None,
        };
        let err = pipeline
            .validate_and_apply(&op, sample_context(), None)
            .unwrap_err();
        assert!(matches!(err, AccountValidationError::InvalidSignature));
    }

    #[test]
    fn pipeline_accepts_valid_operation_and_consumes_nonce() {
        let sender = addr(9);
        let mut pipeline = ValidationPipeline::default();
        let op = sample_operation(sender, Nonce::new(0), 5);
        pipeline
            .validate_and_apply(&op, sample_context(), None)
            .unwrap();
        assert_eq!(pipeline.expected_nonce(&sender), 1);
    }

    #[test]
    fn pipeline_rejects_empty_signature() {
        let sender = addr(10);
        let mut pipeline = ValidationPipeline::default();
        let mut op = sample_operation(sender, Nonce::new(0), 5);
        op.signature.clear();
        let err = pipeline
            .validate_and_apply(&op, sample_context(), None)
            .unwrap_err();
        assert!(matches!(err, AccountValidationError::InvalidSignature));
    }

    #[test]
    fn pipeline_rejects_sponsorship_when_policy_disallows() {
        let sender = addr(11);
        let mut pipeline = ValidationPipeline::default();
        let op = sample_operation(sender, Nonce::new(0), 5);
        let err = pipeline
            .validate_and_apply(
                &op,
                sample_context(),
                Some(SponsorshipRequest {
                    sponsor: addr(99),
                    max_fee_per_gas: 5,
                }),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            AccountValidationError::SponsorshipNotAllowed { .. }
        ));
    }

    #[test]
    fn pipeline_accepts_sponsorship_within_policy_limits() {
        let sender = addr(12);
        let mut pipeline = ValidationPipeline::default();
        pipeline.set_policy(
            sender,
            SmartAccountPolicy {
                allow_gas_sponsorship: true,
                max_sponsored_fee_per_op: 10,
            },
        );
        let op = sample_operation(sender, Nonce::new(0), 5);
        pipeline
            .validate_and_apply(
                &op,
                sample_context(),
                Some(SponsorshipRequest {
                    sponsor: addr(88),
                    max_fee_per_gas: 8,
                }),
            )
            .unwrap();
    }

    #[test]
    fn pipeline_rejects_sponsored_fee_above_policy_limit() {
        let sender = addr(13);
        let mut pipeline = ValidationPipeline::default();
        pipeline.set_policy(
            sender,
            SmartAccountPolicy {
                allow_gas_sponsorship: true,
                max_sponsored_fee_per_op: 6,
            },
        );
        let op = sample_operation(sender, Nonce::new(0), 5);
        let err = pipeline
            .validate_and_apply(
                &op,
                sample_context(),
                Some(SponsorshipRequest {
                    sponsor: addr(77),
                    max_fee_per_gas: 7,
                }),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            AccountValidationError::SponsoredFeeAbovePolicyLimit { .. }
        ));
    }

    // ---------------------------------------------------------------
    // #2168: metered, bounded validation charged on failure.
    // ---------------------------------------------------------------

    #[test]
    fn the_meter_charges_saturates_and_stays_readable_after_exhaustion() {
        let mut meter = GasMeter::new(1_000);
        assert_eq!(meter.remaining(), 1_000);
        meter.charge(400).expect("within budget");
        assert_eq!(meter.spent(), 400);
        assert_eq!(meter.remaining(), 600);
        assert!(!meter.is_exhausted());

        let err = meter.charge(700).expect_err("over budget");
        assert!(matches!(
            err,
            AccountValidationError::ValidationGasExhausted {
                limit: 1_000,
                spent: 1_100
            }
        ));
        // The spend is RECORDED even though the charge failed — that is what a
        // caller bills for a rejected operation.
        assert_eq!(meter.spent(), 1_100);
        assert!(meter.is_exhausted());
        assert_eq!(meter.remaining(), 0);

        // A charge large enough to wrap a u64 saturates instead.
        let mut meter = GasMeter::new(10);
        let _ = meter.charge(u64::MAX);
        let _ = meter.charge(u64::MAX);
        assert_eq!(
            meter.spent(),
            u64::MAX,
            "saturates, never wraps under the limit"
        );
        assert!(meter.is_exhausted());
    }

    /// The acceptance criterion: validation is charged whether or not the
    /// operation succeeds.
    #[test]
    fn a_failing_validation_is_still_charged() {
        let mut pipeline = ValidationPipeline::default();
        let mut op = sample_operation(Address([9u8; 20]), Nonce::new(0), 5);
        // Break the signature so validation fails at the signature step.
        op.signature = vec![0u8; 64];

        let mut meter = GasMeter::new(1_000_000);
        let err = pipeline
            .validate_metered(&op, sample_context(), None, &mut meter)
            .expect_err("a bad signature must not validate");
        assert!(matches!(err, AccountValidationError::InvalidSignature));
        assert!(
            meter.spent() >= gas_costs::BASE + gas_costs::SIGNATURE_VERIFY,
            "a rejected operation must still be charged for the work it forced: {}",
            meter.spent()
        );
    }

    /// An operation whose validation budget is too small to reach the
    /// signature check is rejected for exhaustion, not admitted for free.
    #[test]
    fn a_budget_too_small_to_validate_is_rejected_as_exhausted() {
        let mut pipeline = ValidationPipeline::default();
        let op = sample_operation(Address([8u8; 20]), Nonce::new(0), 5);

        let mut meter = GasMeter::new(gas_costs::BASE - 1);
        let err = pipeline
            .validate_metered(&op, sample_context(), None, &mut meter)
            .expect_err("cannot even be admitted");
        assert!(matches!(
            err,
            AccountValidationError::ValidationGasExhausted { .. }
        ));

        // And through the ordinary entry point, which meters by the
        // operation's own declared limit: zero admits nothing.
        let mut zero_budget = op.clone();
        zero_budget.validation_gas_limit = 0;
        let err = pipeline
            .validate_and_apply(&zero_budget, sample_context(), None)
            .expect_err("a zero validation budget is fail-closed");
        assert!(matches!(
            err,
            AccountValidationError::ValidationGasExhausted { .. }
        ));
    }

    /// A megabyte signature must not be free to reject.
    #[test]
    fn an_oversized_signature_is_charged_by_size() {
        let mut pipeline = ValidationPipeline::default();
        let mut op = sample_operation(Address([7u8; 20]), Nonce::new(0), 5);
        op.signature = vec![0u8; 4096];

        let mut meter = GasMeter::new(u64::MAX);
        let _ = pipeline.validate_metered(&op, sample_context(), None, &mut meter);
        assert!(
            meter.spent() >= 4096 * gas_costs::PER_SIGNATURE_BYTE,
            "signature bytes must be charged: {}",
            meter.spent()
        );
    }

    /// The operation hash binds the protocol version and the gas limits, so an
    /// operation signed under one rule set or one budget is not valid under
    /// another.
    #[test]
    fn the_operation_hash_binds_the_protocol_version_and_the_gas_limits() {
        let op = sample_operation(Address([6u8; 20]), Nonce::new(0), 5);
        let base = user_operation_hash(&op, sample_context());

        let mut other_budget = op.clone();
        other_budget.validation_gas_limit += 1;
        assert_ne!(base, user_operation_hash(&other_budget, sample_context()));

        let mut other_call_budget = op.clone();
        other_call_budget.call_gas_limit += 1;
        assert_ne!(
            base,
            user_operation_hash(&other_call_budget, sample_context())
        );

        // Chain binding still holds.
        let mut other_chain = sample_context();
        other_chain.chain_id = ChainId::new(other_chain.chain_id.into_inner() + 1);
        assert_ne!(base, user_operation_hash(&op, other_chain));

        assert_ne!(
            // secret-leak-ok: test; `SigningProtocolVersion` is an enum type name, no key material
            user_operation_hash_with_protocol_version(
                &op,
                sample_context(),
                SigningProtocolVersion::V1,
            ),
            user_operation_hash_with_protocol_version(
                &op,
                sample_context(),
                SigningProtocolVersion::V2,
            ),
            "the version active at the operation's block must be committed"
        );
    }

    #[test]
    fn operation_and_sponsor_hashes_bind_paymaster_identity_and_fee_cap() {
        let mut op = sample_operation(Address([5u8; 20]), Nonce::new(0), 5);
        let unsigned = user_operation_hash(&op, sample_context());
        assert_eq!(paymaster_authorization_hash(&op, sample_context()), None);

        op.paymaster = Some(PaymasterData {
            paymaster: Address([7u8; 20]),
            max_fee_per_gas: 8,
            signature: vec![1u8; 64],
        });
        let sponsored = user_operation_hash(&op, sample_context());
        let sponsor_auth = paymaster_authorization_hash(&op, sample_context()).unwrap();
        assert_ne!(
            unsigned, sponsored,
            "attaching a sponsor must change the op hash"
        );

        let mut swapped = op.clone();
        swapped.paymaster.as_mut().unwrap().paymaster = Address([8u8; 20]);
        assert_ne!(sponsored, user_operation_hash(&swapped, sample_context()));
        assert_ne!(
            sponsor_auth,
            paymaster_authorization_hash(&swapped, sample_context()).unwrap()
        );

        let mut raised = op.clone();
        raised.paymaster.as_mut().unwrap().max_fee_per_gas += 1;
        assert_ne!(sponsored, user_operation_hash(&raised, sample_context()));
        assert_ne!(
            sponsor_auth,
            paymaster_authorization_hash(&raised, sample_context()).unwrap()
        );

        let mut signature_only = op.clone();
        signature_only.paymaster.as_mut().unwrap().signature[0] ^= 1;
        assert_eq!(
            sponsored,
            user_operation_hash(&signature_only, sample_context()),
            "the signature container must not recursively sign itself"
        );
    }
}
