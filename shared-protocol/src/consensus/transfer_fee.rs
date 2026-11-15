//! Burned transfer fee (`ConsensusFeature::TransferFeeBurn`, owner GO 2026-11-01).
//!
//! From the feature's activation every transparent `Transfer` pays a small
//! protocol fee, and that fee is **burned**: debited from the payer and added to
//! `State::supply_authorized_burning`, credited to nobody. Burns free headroom
//! under the 21M cap (the cap is measured against `issuance_committed_supply()`,
//! the sum of what is held now), which is what lets issuance continue as a
//! perpetual tail (sinks ADR, core4#50 §5–§6).
//!
//! # Who pays
//!
//! * The sender, by default.
//! * The sponsor, when the transfer carries a `SponsorAuthorization` that
//!   `paymaster` accepts at that height. The sponsor's signed `fee` must be at
//!   least the transfer fee, and the sponsor pays its signed `fee` (all burned).
//!   The sender then pays no fee. This is why the two features are scheduled
//!   in the same release: a newcomer's first send is sponsored, never walled.
//!
//! # What is debited
//!
//! Exactly the protocol fee in force at the block height ([`transfer_fee_at`]),
//! never the transaction's `fee` field. `tx.fee` is not covered by the sender's
//! authorization signature (`from|to|amount|nonce`), so charging it would let
//! any relayer that rewrites the envelope burn a victim's balance. `tx.fee` is
//! still required to DECLARE at least the fee (consensus rule, so the
//! transaction says what it costs), and it still orders the mempool.
//!
//! # The parameter
//!
//! The amount is a schedule parameter, not a constant read in apply: each row
//! of [`TRANSFER_FEE_STEPS`] binds a fee to a feature, and the fee in force at a
//! height is that of the LAST row whose feature is active there. The fee is
//! therefore set by the same owner-signed schedule that switches the feature on
//! (genesis or a #3277 extension). Changing it later is a new row under a new
//! feature name (for example `transfer_fee_burn_r2`), so every past block keeps
//! replaying with the fee it was mined under.
//!
//! # What does not pay
//!
//! Only `TransactionType::Transfer` with a transparent payload pays. Coinbase,
//! reward, reward-claim, contribution and forecast commit/reveal, oracle
//! attestation and registration, station registration, node bond, node
//! service receipts, node fraud, slashing, governance and schedule-extension
//! transactions are untouched: they either create value under an emission rule
//! or carry their own deposit/authorization model, and charging them would
//! break block rewards, node-pay settlement or attester flows. Confidential and
//! shielded transfers are disabled at admission today; they get a fee rule in
//! the release that enables them.

use crate::crypto::chain_binding::{ConsensusFeature, feature_active, feature_scheduled};

/// One schedule row: from `feature`'s activation, a transparent transfer pays
/// `fee_base_units` (burned).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferFeeStep {
    /// The feature whose activation puts this fee in force.
    pub feature: ConsensusFeature,
    /// The fee, in base units (`WRF_UNITS_PER_COIN` per WRF).
    pub fee_base_units: u64,
}

/// The owner's 2026-11-01 figure: 0.00005 WRF.
pub const TRANSFER_FEE_V1_BASE_UNITS: u64 = 5_000;

/// Fee schedule, oldest first. **Reviewable consensus data**: a row changes
/// only through a binary release, and takes effect only when its feature is
/// scheduled. Append-only; a row is never edited once its feature may have
/// been scheduled anywhere.
pub const TRANSFER_FEE_STEPS: &[TransferFeeStep] = &[TransferFeeStep {
    feature: ConsensusFeature::TransferFeeBurn,
    fee_base_units: TRANSFER_FEE_V1_BASE_UNITS,
}];

/// The fee a transparent transfer mined at `height` must pay, or `None` when
/// no fee is in force (every chain that has not scheduled the feature).
pub fn transfer_fee_at(height: u64) -> Option<u64> {
    TRANSFER_FEE_STEPS
        .iter()
        .rev()
        .find(|step| feature_active(step.feature, height))
        .map(|step| step.fee_base_units)
}

/// Whether any fee step is scheduled on this chain at all.
pub fn transfer_fee_scheduled() -> bool {
    TRANSFER_FEE_STEPS
        .iter()
        .any(|step| feature_scheduled(step.feature))
}

/// The fee a node should DECLARE (`tx.fee`) on a transfer it builds for the
/// next block, `next_height`. The fee in force there; otherwise, when a step is
/// scheduled later, the latest scheduled fee, so a transfer queued just before
/// activation and mined just after it is not refused for declaring too little.
/// A declared fee below activation is never charged. `0` on chains that
/// schedule nothing (byte-identical envelopes).
pub fn transfer_fee_to_declare(next_height: u64) -> u64 {
    transfer_fee_at(next_height)
        .or_else(|| {
            transfer_fee_scheduled()
                .then(|| transfer_fee_at(u64::MAX))
                .flatten()
        })
        .unwrap_or(0)
}

/// Why a transfer's fee was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransferFeeError {
    /// `tx.fee` declares less than the fee in force.
    #[error(
        "SP_TRANSFER_FEE_BELOW_MINIMUM: this transfer declares a fee of {declared} base units; \
         the chain requires {required} base units (burned)"
    )]
    DeclaredBelowMinimum { declared: u64, required: u64 },
    /// A sponsorship whose signed fee does not cover the transfer fee.
    #[error(
        "SP_TRANSFER_FEE_SPONSOR_BELOW_MINIMUM: the sponsor signed a fee of {signed} base units; \
         the chain requires at least {required} base units (burned)"
    )]
    SponsorBelowMinimum { signed: u64, required: u64 },
}

/// The consensus check on a transfer's declared fee at `height`. `Ok(None)`
/// when no fee is in force; `Ok(Some(fee))` with the fee to burn otherwise.
pub fn check_declared_fee(declared: u64, height: u64) -> Result<Option<u64>, TransferFeeError> {
    match transfer_fee_at(height) {
        Some(required) if declared < required => {
            Err(TransferFeeError::DeclaredBelowMinimum { declared, required })
        },
        other => Ok(other),
    }
}

/// Render base units as a WRF decimal string without float rounding.
pub fn format_wrf(base_units: u64) -> String {
    let per = crate::common::constants::WRF_UNITS_PER_COIN;
    let whole = base_units / per;
    let frac = base_units % per;
    if frac == 0 {
        return whole.to_string();
    }
    let digits = format!("{frac:08}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_fee_is_the_owner_figure() {
        assert_eq!(TRANSFER_FEE_V1_BASE_UNITS, 5_000);
        assert_eq!(format_wrf(TRANSFER_FEE_V1_BASE_UNITS), "0.00005");
        assert_eq!(format_wrf(100_000_000), "1");
        assert_eq!(format_wrf(150_000_000), "1.5");
        assert_eq!(format_wrf(0), "0");
    }

    #[test]
    fn steps_are_append_only_features_registered_after_paymaster() {
        let all = ConsensusFeature::ALL;
        let paymaster = all
            .iter()
            .position(|f| *f == ConsensusFeature::Paymaster)
            .unwrap();
        for step in TRANSFER_FEE_STEPS {
            let at = all.iter().position(|f| *f == step.feature).unwrap();
            assert!(at > paymaster);
            assert!(
                step.fee_base_units > 0,
                "a zero fee row would be a no-op schedule"
            );
            assert!(
                step.fee_base_units >= crate::accounts::paymaster::MIN_SPONSORED_FEE
                    && step.fee_base_units <= crate::accounts::paymaster::MAX_SPONSORED_FEE,
                "a sponsor must be able to sign a fee that covers every step"
            );
            assert!(
                !step.feature.schedulable(None),
                "scheduling is the owner's step"
            );
            assert!(step.feature.has_behaviour());
        }
    }

    /// Appended right after station_coordinates_fixed_point; later features
    /// append after it, so the wire ordinal stays fixed. Unscheduled.
    #[test]
    fn feature_follows_station_coordinates_fixed_point_and_is_unscheduled() {
        let f = ConsensusFeature::TransferFeeBurn;
        assert_eq!(ConsensusFeature::parse("transfer_fee_burn"), Some(f));
        let all = ConsensusFeature::ALL;
        let at = all.iter().position(|x| *x == f).unwrap();
        assert_eq!(all[at - 1], ConsensusFeature::StationCoordinatesFixedPoint);
        let known = crate::crypto::protocol_support::KNOWN_FEATURES;
        let k = known
            .iter()
            .position(|x| *x == "transfer_fee_burn")
            .unwrap();
        assert_eq!(known[k - 1], "station_coordinates_fixed_point");
        assert_eq!(bincode::serialize(&f).unwrap(), 26u32.to_le_bytes());
        assert!(!f.schedulable(None));
        assert!(f.has_behaviour());
    }

    #[test]
    fn unscheduled_chain_charges_nothing_and_declares_zero() {
        // The unit-test process installs no schedule.
        if crate::crypto::chain_binding::try_feature_schedule().is_some() {
            return;
        }
        assert_eq!(transfer_fee_at(0), None);
        assert_eq!(transfer_fee_at(u64::MAX), None);
        assert_eq!(transfer_fee_to_declare(1), 0);
        assert_eq!(check_declared_fee(0, u64::MAX), Ok(None));
    }
}
