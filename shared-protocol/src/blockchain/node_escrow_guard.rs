//! Preflight account/configuration write guard: refuse before any side effect.
//! No State/account snapshots or rollback are needed. The exhaustive match
//! forces new transition variants to declare their protected-account footprint.
use super::{GovernanceParameterUpdate, State, StateError, StateTransition};
use crate::{common::types::Address, crypto::signature::PublicKey};

impl State {
    fn node_protected_account(&self, address: &Address) -> bool {
        (self.node_fraud.is_some()
            && *address == crate::consensus::node_fraud::forfeiture_address())
            || self
                .node_bond_registry
                .as_ref()
                .is_some_and(|r| r.is_escrow(address))
    }

    fn node_protected_identity(&self, identity: &str) -> bool {
        identity
            .parse::<Address>()
            .ok()
            .is_some_and(|a| self.node_protected_account(&a))
            || PublicKey::from_hex(identity)
                .ok()
                .is_some_and(|k| self.node_protected_account(&Address::from_public_key(&k)))
    }

    fn node_protected_update(&self, update: &GovernanceParameterUpdate) -> bool {
        match update {
            GovernanceParameterUpdate::OracleSet(ids) => {
                ids.iter().any(|id| self.node_protected_identity(id.trim()))
            },
            GovernanceParameterUpdate::StakeRequired { address, .. } => {
                self.node_protected_account(address)
            },
            _ => false,
        }
    }

    pub(super) fn guard_node_escrow_transition(
        &self,
        transition: &StateTransition,
    ) -> Result<(), StateError> {
        use StateTransition::*;
        let protected = |a: &Address| self.node_protected_account(a);
        let identity = |s: &str| self.node_protected_identity(s);
        let forbidden = match transition {
            BalanceTransfer { from, to, .. } => protected(from) || protected(to),
            LockToStake { address, amount } | ReleaseToSpendable { address, amount } => {
                amount.0 != 0 && protected(address)
            },
            ApplyReward { address, .. }
            | ApplyGamificationReward { address, .. }
            | Slash { address, .. } => protected(address),
            ApplyContributionRewardDecision { record, .. } => protected(&record.address),
            ApplyShieldedDeposit { from, .. } => protected(from),
            ApplyShieldedWithdrawal { payload, .. } => identity(&payload.transparent_recipient),
            ApplyRewardClaim { payload, .. } => identity(&payload.destination),
            ApplyAttesterSettlements { records } => records
                .iter()
                .flat_map(|r| &r.allocations)
                .any(|a| identity(&a.attester_id)),
            ApplyProducerSettlement { record, .. } => identity(&record.payout_address),
            ApplyFaucetReward => self
                .effective_faucet_address()
                .as_ref()
                .is_some_and(protected),
            RecordGenesisEmissionConfig { config } => {
                config.faucet_address.as_ref().is_some_and(protected)
            },
            ApplyProducerSetChange { change, .. } => {
                change.members.iter().any(|m| identity(&m.payout_address) || PublicKey::from_bytes(&m.public_key).ok().is_some_and(|k| protected(&Address::from_public_key(&k))))
            },
            SetOracleConfig { oracle_set, .. } | ApplyOracleRegistration { oracle_set, .. } => {
                oracle_set.iter().any(|s| identity(s.trim()))
            },
            CreateGovernanceProposal { proposer, .. } => protected(proposer),
            FinalizeGovernanceProposal { proposal_id, .. } => self
                .governance
                .proposals
                .get(proposal_id)
                .is_some_and(|p| self.node_protected_update(&p.update)),
            ProcessGovernanceTransaction { tx, .. } | ProcessSlashingReport { tx, .. } => {
                // Nested typed effects also pass through this guard. Fence the
                // outer nonce and every parsed account target before dispatch.
                protected(&tx.sender_address)
                    || tx
                        .parse_transparent_transfer_payload()
                        .is_some_and(|p| identity(&p.from) || identity(&p.to))
                    || tx
                        .parse_shielded_withdrawal_payload()
                        .is_some_and(|p| identity(&p.transparent_recipient))
                    || tx
                        .parse_reward_claim_payload()
                        .is_some_and(|p| identity(&p.destination))
                    || tx
                        .parse_slashing_report_payload()
                        .is_some_and(|p| identity(&p.validator_id))
            },
            ApplyAccountRecovery {
                account, action, ..
            } => {
                use crate::accounts::recovery::RecoveryAction;
                protected(account)
                    || (matches!(action, RecoveryAction::Execute)
                        && (self.node_bond_registry.as_ref().is_some_and(|r| {
                            r.registrations
                                .values()
                                .any(|r| &r.binding.owner_account == account)
                        }) || self
                            .account_recovery
                            .get(account)
                            .and_then(|r| r.active_request.as_ref())
                            .is_some_and(|r| protected(&Address::from_public_key(&r.new_owner)))))
            },
            DeclareAuthorization { declaration, .. } => protected(&declaration.account),
            ApplyPaymasterAction { action, .. } => {
                use crate::accounts::paymaster::PaymasterAction::*;
                let (Stake { paymaster, .. }
                | RequestUnstake { paymaster }
                | WithdrawStake { paymaster }
                | SponsorOperation { paymaster, .. }) = action;
                protected(paymaster)
            },
            ApplyStationRegistration { station }
            | ApplyStationRegistrationWithOwnerConsent { station, .. }
            | ApplyStationRegistrationV2 { station, .. } => identity(&station.owner_address),
            ApplyStationOwnerTransfer {
                to_owner_address, ..
            }
            | ApplyStationOwnerRecoveryFiling {
                to_owner_address, ..
            } => identity(to_owner_address),
            ApplyStationOwnerRecoveryExecute { station_id, .. } => self
                .station_owner_ledger
                .get(station_id)
                .and_then(|l| l.pending_recovery.as_ref())
                .is_some_and(|r| identity(&r.to_owner_address)),
            ApplyFeatureScheduleExtension { extension, .. } => {
                extension.extension.entries.iter().any(|entry| {
                    crate::crypto::chain_binding::faucet_binding_row_for(&entry.feature, None)
                        .ok()
                        .and_then(|row| row.address)
                        .is_some_and(identity)
                })
            },
            // These effects do not write transparent accounts or credit-target configuration.
            RejectInvalidContributionRewardMetadata { .. }
            | RegisterCommit { .. }
            | ApplyReveal { .. }
            | ApplyConfidentialTransfer { .. }
            | RecordOracleAttestation { .. }
            | RegisterSigningKeyEpoch { .. }
            | RevokeSigningKeyEpoch { .. }
            | VoteGovernanceProposal { .. }
            | RecordAuditEvent { .. }
            | RecordQualityPolicyEvent { .. }
            | RecordOracleLocalAccuracy { .. }
            | CreateNFT { .. }
            | TransferNFT { .. }
            | UpdateNFT { .. }
            | VerifyNFT { .. }
            | RecordCalibrationAttestation { .. }
            | CreateMiningPool { .. }
            | JoinMiningPool { .. }
            | LeaveMiningPool { .. }
            | SetLowQualityStreak { .. }
            | ApplySessionKeyAction { .. }
            | ApplyStationOwnerAction { .. }
            | ApplyStationOwnerRecoveryCancel { .. }
            | BeginNodeServiceBlock { .. }
            | ApplyNodeBondLifecycle { .. }
            | ApplyNodeServiceBatch { .. }
            | ApplyNodeFraudReport { .. } => false,
        };
        if forbidden {
            return Err(StateError::Governance("SP_NODE_ESCROW_PROTECTED".into()));
        }
        Ok(())
    }
}
