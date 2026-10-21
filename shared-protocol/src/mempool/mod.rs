pub mod pool;

/// Local admission/relay policy only. Historical block verification and state
/// application deliberately do not consult this predicate (Refs #3407, #3408).
pub fn privacy_admission_disabled(kind: crate::blockchain::transaction::TransactionType) -> bool {
    matches!(
        kind,
        crate::blockchain::transaction::TransactionType::RewardClaim
            | crate::blockchain::transaction::TransactionType::ConfidentialTransfer
    )
}

/// Admission and transaction-gossip policy, never historical block validity.
/// Staking stays refused until active, state-aware admission can prove nonce
/// safety and producer/peer agreement (Refs #3430).
pub fn local_admission_disabled(kind: crate::blockchain::transaction::TransactionType) -> bool {
    privacy_admission_disabled(kind)
        || matches!(
            kind,
            crate::blockchain::transaction::TransactionType::Stake
                | crate::blockchain::transaction::TransactionType::Unstake
        )
}
