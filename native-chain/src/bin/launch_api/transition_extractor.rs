//! Shared helper for extracting state transitions from block transactions.
//!
//! Used by both the mining route (local block creation) and P2P event handler
//! (replaying transitions from received blocks).

use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::state::{
    ContributionRewardRecord, State, StateTransition,
};
use wrfcoin_native_chain::consensus::proof_of_publishing::ATTESTER_SETTLEMENTS_METADATA_KEY;
use wrfcoin_native_chain::oracle::attestation::AttesterSettlementRecord;
use wrfcoin_native_chain::blockchain::transaction::{Transaction, TransactionType};
use wrfcoin_native_chain::common::types::{Address, Balance, Timestamp};
use wrfcoin_native_chain::weather::validation::DataQualityScore;

/// Metadata key used to serialize per-address reward entries into the block.
///
/// The miner's `process_contribution_rewards` writes this key with a JSON array
/// of five-field [`ContributionRewardRecord`] values. `extract_transitions`
/// reads it back to produce deterministic `ApplyContributionRewardDecision`
/// transitions on P2P nodes. Legacy three-field tuples are rejected.
pub const REWARD_ENTRIES_METADATA_KEY: &str = "reward_entries";

/// Metadata key used to serialize forecast-receipt oracle-local accuracy
/// updates into the block for deterministic P2P replay.
pub const ORACLE_LOCAL_ACCURACY_ENTRIES_METADATA_KEY: &str = "oracle_local_accuracy_entries";

/// Extract state transitions from a block at a given height.
///
/// This mirrors the transition logic in the mining route, ensuring that P2P block
/// replay applies the same state mutations as locally-mined blocks.
pub fn extract_transitions(
    state: &State,
    block: &Block,
    block_height: u64,
    block_timestamp: i64,
) -> Vec<StateTransition> {
    let mut transitions = Vec::new();
    if state.node_share_activation_height().is_some_and(|a| block_height >= a) {
        transitions.push(StateTransition::BeginNodeServiceBlock { block_height, parent_hash: *block.prev_block_hash.as_bytes() });
    }
    let mut launch_transitions = Vec::new();
    if let Some(config) = wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig::from_block(block) {
        let transition = StateTransition::RecordGenesisEmissionConfig { config };
        transitions.push(transition);
    }

    for tx in &block.transactions {
        match tx.tx_type {
            TransactionType::NodeFraudReport => {
                if let Some(payload) = wrfcoin_shared_protocol::consensus::node_fraud::parse(tx) {
                    let transition = StateTransition::ApplyNodeFraudReport { payload, block_height };
                    launch_transitions.push(transition.clone()); transitions.push(transition);
                }
            },
            TransactionType::NodeServiceBatch => {
                if let Some(payload) = wrfcoin_shared_protocol::consensus::node_receipt_registry::NodeReceiptSubmission::parse(tx) {
                    let transition = StateTransition::ApplyNodeServiceBatch { payload, block_height };
                    launch_transitions.push(transition.clone()); transitions.push(transition);
                }
            },
            TransactionType::NodeBondLifecycle => {
                if let Some(payload) = wrfcoin_shared_protocol::consensus::node_bond_registry::NodeBondLifecycle::parse(tx) {
                    let transition = StateTransition::ApplyNodeBondLifecycle { payload, block_height };
                    launch_transitions.push(transition.clone()); transitions.push(transition);
                }
            },
            // Session-key register/revoke: the same parse as the PoP producer
            // arm, so producer == applier == WAL replay. The apply arm is gated
            // on `ConsensusFeature::SessionKeys` and fails closed.
            TransactionType::SessionKeyAction => {
                if let Some(action) = wrfcoin_shared_protocol::accounts::session_keys::SessionKeyActionPayload::parse(tx) {
                    let transition = StateTransition::ApplySessionKeyAction { action, block_height };
                    launch_transitions.push(transition.clone()); transitions.push(transition);
                }
            },
            // --- Commit/Reveal protocol (PoP core) ---
            // Fail-closed sender auth (#1848): a commit without a valid
            // authorization emits no transition so a forged payload injected
            // via P2P cannot register a commit in state.
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => {
                if tx.verify_commit_authorization_at_height(block_height) {
                    if let Some(payload) = tx.parse_commit_payload() {
                        let transition = StateTransition::RegisterCommit {
                            payload,
                            block_height,
                        };
                        launch_transitions.push(transition.clone());
                        transitions.push(transition);
                    }
                }
            },
            TransactionType::ContributionReveal | TransactionType::ForecastReveal => {
                if let Some(payload) = tx.parse_reveal_payload() {
                    // #3270: the header timestamp rides in the transition so
                    // the applier sizes the skew allowance exactly as the
                    // producer (`launch_transition_for_transaction`) did.
                    let transition = StateTransition::ApplyReveal {
                        payload,
                        block_height,
                        block_timestamp,
                    };
                    launch_transitions.push(transition.clone());
                    transitions.push(transition);
                }
            },
            TransactionType::OracleAttestation => {
                if let Some(payload) = tx.parse_oracle_attestation_payload() {
                    let transition = StateTransition::RecordOracleAttestation {
                        payload,
                        current_block: block_height,
                    };
                    launch_transitions.push(transition.clone());
                    transitions.push(transition);
                }
            },

            // --- Coinbase and reward (issue #955) ---
            TransactionType::Coinbase => {
                if let Some(output) = tx.outputs.first() {
                    if let Ok(addr) = output.address.parse::<Address>() {
                        transitions.push(StateTransition::ApplyReward {
                            address: addr,
                            amount: Balance::new(output.amount as u128),
                            reason: "coinbase".to_string(),
                        });
                        transitions.push(StateTransition::ApplyFaucetReward);
                    }
                }
            },
            TransactionType::Reward => {
                if let Some(output) = tx.outputs.first() {
                    if let Ok(addr) = output.address.parse::<Address>() {
                        // Authorized gamification payout (mobile#215): a `Reward`
                        // tx carrying a `GamificationClaimPayload` maps to the
                        // idempotent, supply-guarded `ApplyGamificationReward`.
                        // Uses the SAME `parse_gamification_claim` as the
                        // producer path (`launch_transition_for_transaction`) so
                        // producer == applier == WAL replay.
                        //
                        // Fail-closed on authorization (#2081 review): a claim
                        // whose operator signature does not verify against an
                        // allowlisted key emits NO transition — never a mint and
                        // never a plain reward to the forger's address. The block
                        // carrying it is separately rejected by `verify_block`;
                        // this keeps the apply path consistent when replay skips
                        // that gate, mirroring the stake authorization handling.
                        if tx.parse_gamification_claim().is_some() {
                            if let Some(claim) = tx
                                .parse_gamification_claim()
                                .filter(|_| tx.verify_gamification_authorization())
                            {
                                transitions.push(StateTransition::ApplyGamificationReward {
                                    address: addr,
                                    amount: Balance::new(output.amount as u128),
                                    claim_id: claim.claim_id,
                                    current_block: block_height,
                                });
                            }
                        } else {
                            transitions.push(StateTransition::ApplyReward {
                                address: addr,
                                amount: Balance::new(output.amount as u128),
                                reason: "reward".to_string(),
                            });
                        }
                    }
                }
            },

            // --- Staking (issue #955; hardened in #1763) ---
            // The staking mutation is authorized by an `account`-bound ed25519
            // signature over `account|amount|action|nonce`
            // (`verify_stake_authorization`), NOT by the spoofable
            // `tx.sender_address`/output amount the pre-#1763 extractor read.
            // Fail-closed: emit no transition for a Stake/Unstake without a
            // valid authorization, so a forged tx cannot freeze a victim's
            // balance or manipulate stake-derived voting power even if it
            // somehow reaches the applier. The amount and account both come from
            // the *signed* payload, so producer and applier mutate the same
            // account by the same amount. The block-validation gate
            // (`verify_block`) rejects such blocks outright on the sync path;
            // this gate keeps the apply path consistent (producer == applier ==
            // WAL replay).
            TransactionType::Stake => {
                if let Some(payload) = tx.parse_stake_auth_payload() {
                    if tx.verify_stake_authorization_with_view(
                        state,
                        block_height,
                        Timestamp::new(block_timestamp as u64),
                    ) {
                        if let Ok(account) = payload.account.parse::<Address>() {
                            transitions.push(StateTransition::LockToStake {
                                address: account,
                                amount: Balance::new(payload.amount as u128),
                            });
                        }
                    }
                }
            },
            TransactionType::Unstake => {
                if let Some(payload) = tx.parse_stake_auth_payload() {
                    if tx.verify_stake_authorization_with_view(
                        state,
                        block_height,
                        Timestamp::new(block_timestamp as u64),
                    ) {
                        if let Ok(account) = payload.account.parse::<Address>() {
                            transitions.push(StateTransition::ReleaseToSpendable {
                                address: account,
                                amount: Balance::new(payload.amount as u128),
                            });
                        }
                    }
                }
            },

            // --- Fork-safe on-chain oracle registration (#2055 Option A) ---
            // A governance-authorized `OracleRegistration` tx maps to the
            // compound, deterministic `ApplyOracleRegistration` transition. Same
            // parse (`parse_oracle_registration`) the producer path uses, so
            // producer == applier == WAL replay. Fail-closed exactly like the
            // stake gate above: a registration whose governance signature does
            // not verify against an allowlisted key emits NO transition (and its
            // block is separately rejected by `verify_block`), so a peer cannot
            // forge or redirect an oracle registration.
            TransactionType::OracleRegistration => {
                if let Some(p) = tx
                    .parse_oracle_registration()
                    .filter(|_| tx.verify_oracle_registration_authorization())
                {
                    transitions.push(StateTransition::ApplyOracleRegistration {
                        oracle_set: p.oracle_set,
                        oracle_quorum: p.oracle_quorum,
                        identity: p.identity,
                        public_key: p.public_key,
                        epoch: p.epoch,
                        activated_at: p.activated_at,
                    });
                }
            },

            // --- Fork-safe on-chain station registration (#2537) ---
            // The applier half of the production registration path. Uses the
            // same `parse_station_registration` +
            // `verify_station_registration_authorization` pair the producer
            // uses, and builds the `WeatherStation` through the same shared
            // `station_from_registration` constructor, so producer == applier
            // == WAL replay down to the byte. Fail-closed exactly like the
            // oracle gate above: an unauthorized registration emits NO
            // transition, so a peer cannot forge a station — and above all
            // cannot forge one classified `Real`, which is the only class that
            // earns.
            TransactionType::StationRegistration => {
                if tx.parse_station_registration().is_some() {
                    // The SAME shared step as the PoP producer arm: the
                    // format-vs-activation rule, authorization and the variant
                    // by height, so producer == applier == WAL replay.
                    if let Some(transition) = tx.station_registration_step(block_height) {
                        transitions.push(transition);
                    }
                } else if tx.parse_station_registration().is_none() {
                    // #3345 / #3347: the owner-ledger kinds carried on the
                    // same tx type — the SAME shared constructor as the PoP
                    // producer arm (which also tries it only when the tx is
                    // not a registration), so producer == applier == WAL
                    // replay. Nothing below the unscheduled activations.
                    if let Some(transition) = tx.station_owner_ledger_transition(block_height) {
                        transitions.push(transition);
                    }
                }
            },

            // --- Governance-signed feature-schedule extension (#3277 S4) ---
            // The applier half. Same `parse_feature_schedule_extension` as the
            // producer's `launch_transition_for_transaction`, so producer ==
            // applier == WAL replay. No authorization filter: the transition is
            // always emitted and its apply arm re-runs V1–V9 against the parent
            // state, failing the block closed, so an invalid extension is never
            // silently skipped. (An unparsable payload never gets here: both
            // `verify_block` and the launch replay reject its block.)
            TransactionType::FeatureScheduleExtension => {
                if let Some(extension) = tx.parse_feature_schedule_extension() {
                    transitions.push(StateTransition::ApplyFeatureScheduleExtension {
                        extension,
                        block_height,
                    });
                }
            },

            // --- On-chain slashing apply (#1393) ---
            // Route the full `SlashingReport` transaction through the
            // block-apply pipeline by wrapping it in
            // `StateTransition::ProcessSlashingReport`. The matching arm
            // in `apply_transition_inner` re-enters
            // `apply_protocol_transaction`, which is where evidence-hash
            // dedup, validator stake debit, `slashing_evidence_seen`
            // insertion, metrics, and the v7 state-root leaf update all
            // live. Without this arm the extractor would drop the tx and
            // the slash would never activate on the consensus path, even
            // though unit tests would keep passing by calling the apply
            // function directly.
            TransactionType::SlashingReport => {
                transitions.push(StateTransition::ProcessSlashingReport {
                    tx: tx.clone(),
                    current_block: block_height,
                });
            },

            // Type-only Option C slice: block verification, mempool and shared
            // apply explicitly refuse it. It has no native transition until the
            // retained-context/apply feature is implemented (Refs #3398).
            TransactionType::SlashEvidenceReport => {},

            // --- Governance-routed transactions (#1731) ---
            // Transfer, ConfidentialTransfer, ShieldedDeposit,
            // ShieldedWithdrawal, and RewardClaim are NOT emitted as direct
            // transitions here. The producer (`create_next_block` ->
            // `apply_governance_transitions` -> `governance_transitions_for_block`)
            // applies these EXCLUSIVELY by wrapping the tx in
            // `StateTransition::ProcessGovernanceTransaction` (which re-enters
            // `apply_protocol_transaction` -> `apply_transparent_transfer` /
            // the privacy handlers). They are picked up below from the
            // `governance_transitions` list (block metadata or simulation).
            //
            // Emitting a SECOND direct transition for these tx types here
            // double-applied the same mutation on the P2P applier and WAL
            // replay paths — e.g. a transparent Transfer was debited twice,
            // surfacing as `InsufficientSpendableBalance` at block 8 in
            // multi-node runs and silently forking the chain (#1731). The
            // producer never double-debits because its launch-transition path
            // (`launch_transition_for_transaction`) returns `None` for these
            // types. Routing the applier through the single shared governance
            // sink (same pattern as #1682) keeps producer == applier == WAL.
            TransactionType::Transfer
            | TransactionType::ConfidentialTransfer
            | TransactionType::ShieldedDeposit
            | TransactionType::ShieldedWithdrawal
            | TransactionType::RewardClaim
            // Governance proposal/vote/etc. were already governance-routed.
            | TransactionType::GovernanceProposal
            | TransactionType::GovernanceVote
            | TransactionType::GovernanceExecute
            | TransactionType::GovernanceFinalize
            | TransactionType::GovernanceDelegate
            // Remaining types are UTXO-only or data-only (no state-machine effect).
            | TransactionType::TreasuryDeposit
            | TransactionType::TreasuryWithdraw
            | TransactionType::Regular
            | TransactionType::DataAnalysis
            | TransactionType::DataCollection
            | TransactionType::DataStorage
            | TransactionType::EnvironmentalOffset
            | TransactionType::BridgeLock
            | TransactionType::BridgeUnlock
            | TransactionType::NftCreation
            | TransactionType::NftTransfer
            | TransactionType::NftExecution
            | TransactionType::Layer2BatchSubmit => {
                // No direct transition emitted here; see comment above.
            },
        }
    }

    // Governance transitions — read from block metadata if available (miner
    // already computed them in apply_governance_transitions, #940), otherwise
    // fall back to simulation (needed for legacy blocks without the key).
    let governance_transitions = if let Some(gov_json) =
        block.metadata.get("governance_transitions")
    {
        if let Ok(gov_transitions) = serde_json::from_str::<Vec<StateTransition>>(gov_json) {
            gov_transitions
        } else {
            // #2056: the miner serialized governance transitions but they no
            // longer deserialize (schema drift / corruption). Previously this
            // fell back to re-simulation silently; make the anomaly visible.
            log::warn!(
                "governance_transitions metadata present but failed to deserialize at \
                 block {block_height} — falling back to re-simulation (#2056)"
            );
            state.governance_transitions_for_block(
                &block.transactions,
                block_height,
                block_timestamp,
            )
        }
    } else {
        state.governance_transitions_for_block(
            &block.transactions,
            block_height,
            block_timestamp,
        )
    };
    // #3591: the producer begins the node-service block before its launch
    // batch, so from `streak_replay_node_service` the replay does too.
    let replay_node_service_begin = wrfcoin_shared_protocol::crypto::chain_binding::feature_active(
        wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature::StreakReplayNodeService,
        block_height,
    )
    .then(|| {
        transitions
            .iter()
            .find(|t| matches!(t, StateTransition::BeginNodeServiceBlock { .. }))
            .cloned()
    })
    .flatten();
    let low_quality_streak_transitions = derive_low_quality_streak_transitions(
        state,
        block,
        &governance_transitions,
        replay_node_service_begin.as_ref(),
        &launch_transitions,
    );
    transitions.extend(governance_transitions);
    transitions.extend(low_quality_streak_transitions);

    if let Some(entries_json) = block
        .metadata
        .get(ORACLE_LOCAL_ACCURACY_ENTRIES_METADATA_KEY)
    {
        if let Ok(entries) = serde_json::from_str::<Vec<(String, String, u32, u32)>>(entries_json) {
            for (oracle_id, station_id, score_bps, window) in entries {
                transitions.push(StateTransition::RecordOracleLocalAccuracy {
                    oracle_id,
                    station_id,
                    score_bps,
                    window,
                });
            }
        }
    }

    // Phase B: the header key authorizes roster lookup; the metadata is checked
    // against that derivation before any Producer slice is charged.
    if let Some(encoded) = block.metadata.get(
        wrfcoin_shared_protocol::consensus::proof_of_publishing::PRODUCER_SETTLEMENT_METADATA_KEY,
    ) {
        if let Ok(record) = serde_json::from_str::<
            wrfcoin_shared_protocol::oracle::attestation::ProducerSettlementRecord,
        >(encoded) {
            transitions.push(StateTransition::ApplyProducerSettlement {
                producer_pubkey: block.producer_pubkey, record,
            });
        }
    }

    // ADR-003 Phase A attester settlements (#2846), serialized by the producer
    // in `stage_attester_settlements`.
    //
    // THE METADATA IS A CLAIM, NOT AN AUTHORIZATION. What a follower genuinely
    // cannot recover from post-state is WHICH decisions crossed their threshold
    // in this block: that needs the pre-transition quorum snapshot, and
    // `AttestationSet::add` REPLACES an oracle's record so a re-attestation is
    // indistinguishable from a first vote afterwards. That — and only that — is
    // what these records carry authority over.
    //
    // Who gets paid and how much is DERIVED from the follower's own state by
    // `State::derive_attester_settlements`, and a record that does not match is
    // rejected. Trusting the record here would let a producer write a canonical,
    // in-slice settlement for an invented decision paying arbitrary addresses:
    // every peer would replay it identically, so the state roots would agree and
    // the E(t) bound would hold, and the payment would still be theft.
    //
    // Staged BEFORE the reward entries to match the producer's batch order.
    if let Some(settlements_json) = block.metadata.get(ATTESTER_SETTLEMENTS_METADATA_KEY) {
        match serde_json::from_str::<Vec<AttesterSettlementRecord>>(settlements_json) {
            Ok(records) if !records.is_empty() => {
                // One transition for the whole block: ADR-003 rule 5 divides the
                // attester slice ACROSS decisions, so the apply path has to see
                // them together to derive — and therefore to CHECK — the split.
                transitions.push(StateTransition::ApplyAttesterSettlements { records });
            },
            Ok(_) => {},
            Err(error) => {
                // Deferral, not loss — and the earlier version of this comment
                // was WRONG about why (#2846 review 2).
                //
                // It claimed unparsable metadata "cannot be distinguished here
                // from a block that simply settles nothing". That is false at
                // this call site: `apply_p2p_block_at` calls
                // `extract_transitions(&chain.state, &block, ..)` BEFORE
                // mutating state, so a follower holds both the pre-state quorum
                // snapshot and the ordered block — consensus inputs, not local
                // timing. It could tell.
                //
                // Worse, the branch called itself "conservative" when it was
                // PERMANENT: the crossing block used to be the only block that
                // could ever pay a decision, so skipping here DESTROYED the
                // payment rather than postponing it.
                //
                // Both are fixed at the source. `State.attester_settlement_due`
                // records the debt on the apply path when quorum is crossed, so
                // dropping a malformed settlement here defers the payment to the
                // next honest block. That is what makes it safe to keep replay
                // going rather than halting a follower on a malformed optional
                // key — the concern that motivated this branch is still real,
                // and is now met without the strand.
                log::warn!(
                    "block {}: attester settlement metadata failed to parse ({error}) — \
                     no attester payment replayed for this block; the debt stays \
                     recorded and a later block settles it (#2846)",
                    block.height
                );
            },
        }
    }

    // Contribution/forecast reward transitions serialized by the miner into
    // block metadata (see process_contribution_rewards in proof_of_publishing.rs).
    if let Some(entries_json) = block.metadata.get(REWARD_ENTRIES_METADATA_KEY) {
        match serde_json::from_str::<Vec<ContributionRewardRecord>>(entries_json) {
            Ok(entries) => {
                transitions.extend(entries.into_iter().map(|record| {
                    StateTransition::ApplyContributionRewardDecision {
                        record,
                        current_block: block.height,
                    }
                }));
            },
            Err(error) => {
                transitions.push(StateTransition::RejectInvalidContributionRewardMetadata {
                    reason: error.to_string(),
                });
            },
        }
    }

    transitions
}

fn derive_low_quality_streak_transitions(
    state: &State,
    block: &Block,
    governance_transitions: &[StateTransition],
    // #3591: `Some` only under `streak_replay_node_service`; `None` replays
    // exactly the pre-#3591 sequence.
    node_service_begin: Option<&StateTransition>,
    launch_transitions: &[StateTransition],
) -> Vec<StateTransition> {
    let mut replay_state = state.clone();

    for transition in governance_transitions
        .iter()
        .chain(node_service_begin)
        .chain(launch_transitions.iter())
    {
        // #2032: this replay clone exists only to derive streak/attestation
        // views (below); its `state_root` is never read, so skip the
        // O(state_size) per-transition root recompute that `apply_transition`
        // would perform. The block's canonical state_root is computed by the
        // real apply path, unaffected by this throwaway replay.
        if let Err(err) =
            replay_state.apply_transition_without_root_recompute(transition.clone())
        {
            // #2056: a governance/launch transition failed to replay during
            // low-quality-streak derivation; previously this bailed silently.
            log::warn!(
                "low-quality-streak replay aborted at block {}: transition failed to apply \
                 during re-simulation (error={err:?}) — deriving no streak transitions (#2056)",
                block.height
            );
            return Vec::new();
        }
    }

    let attested_commit_ids = replay_state
        .pending_reveals
        .keys()
        .filter(|commit_id| {
            replay_state
                .attestations
                .get(*commit_id)
                .map(|set| set.has_quorum(replay_state.oracle_quorum.max(1)))
                .unwrap_or(false)
        })
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();

    let quality_policy = replay_state.governance.params.data_quality_policy.clone();
    let mut transitions = Vec::new();
    for tx in &block.transactions {
        if tx.tx_type != TransactionType::ContributionReveal {
            continue;
        }
        let Some(reveal_payload) = tx.parse_reveal_payload() else {
            continue;
        };
        let commit_id = reveal_payload.commit_id.clone();
        if commit_id.is_empty() || !attested_commit_ids.contains(&commit_id) {
            continue;
        }

        let recipient = tx
            .outputs
            .first()
            .map(|o| o.address.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let recipient_address = recipient
            .parse::<Address>()
            .unwrap_or(tx.sender_address);
        let quality_score = derive_quality_score(
            tx,
            replay_state
                .pending_reveals
                .get(&commit_id)
                .and_then(|record| record.quality_score.as_ref()),
        );
        let quality_eval = quality_policy.evaluate(quality_score.as_ref());

        if quality_eval.counts_toward_low_quality_streak {
            let streak = replay_state.increment_low_quality_streak(recipient_address);
            transitions.push(StateTransition::SetLowQualityStreak {
                address: recipient_address,
                streak: Some(streak),
            });
            if streak >= quality_policy.low_quality_slash_streak {
                let slash = StateTransition::Slash {
                    address: recipient_address,
                    amount: Balance::new(quality_policy.low_quality_slash_amount),
                    reason: format!(
                        "repeated_low_quality:commit={commit_id}:streak={streak}:threshold={}",
                        quality_policy.low_quality_slash_streak
                    ),
                };
                if replay_state
                    .apply_transition_without_root_recompute(slash.clone())
                    .is_ok()
                {
                    transitions.push(slash);
                    replay_state.reset_low_quality_streak(&recipient_address);
                    transitions.push(StateTransition::SetLowQualityStreak {
                        address: recipient_address,
                        streak: None,
                    });
                }
            }
        } else {
            replay_state.reset_low_quality_streak(&recipient_address);
            transitions.push(StateTransition::SetLowQualityStreak {
                address: recipient_address,
                streak: None,
            });
        }
    }

    transitions
}

fn derive_quality_score(
    tx: &Transaction,
    explicit: Option<&DataQualityScore>,
) -> Option<DataQualityScore> {
    if let Some(score) = explicit {
        return Some(score.clone());
    }

    let metrics = extract_metrics(tx)?;
    let overall = metrics
        .get("data_quality_overall")
        .copied()
        .or_else(|| metrics.get("quality").copied())?
        .clamp(0.0, 1.0);

    Some(DataQualityScore {
        overall,
        range_score: overall,
        consistency_score: overall,
    })
}

fn extract_metrics(tx: &Transaction) -> Option<std::collections::BTreeMap<String, f64>> {
    if let Some(reveal_payload) = tx.parse_reveal_payload() {
        let reveal_data = serde_json::to_string(&reveal_payload.data).ok()?;
        return serde_json::from_str(&reveal_data).ok();
    }
    tx.data
        .as_ref()
        .and_then(|data| serde_json::from_str(data).ok())
}

#[cfg(test)]
mod tests {
    use super::{
        extract_transitions, ATTESTER_SETTLEMENTS_METADATA_KEY,
        ORACLE_LOCAL_ACCURACY_ENTRIES_METADATA_KEY, REWARD_ENTRIES_METADATA_KEY,
    };
    use wrfcoin_native_chain::blockchain::block::Block;
    use wrfcoin_native_chain::blockchain::state::{
        ContributionRewardRecord, GovernanceProposalStatus, State, StateTransition,
    };
    use wrfcoin_native_chain::blockchain::transaction::{
        CommitPayload, GovernanceProposalPayload, GovernanceVotePayload, StakeAuthPayload,
        Transaction, TransactionType,
    };
    use wrfcoin_native_chain::common::constants::GOVERNANCE_DEFAULT_DEPOSIT;
    use wrfcoin_native_chain::common::types::{Address, Balance};
    use wrfcoin_native_chain::crypto::hash::Hash;
    use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
    use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
    use rand::{SeedableRng, rngs::StdRng};

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    fn make_block(transactions: Vec<Transaction>, height: u64) -> Block {
        Block::new(3, Hash::default(), transactions, 1, height)
    }

    #[test]
    fn staged_slash_evidence_has_no_native_transition() {
        let state = State::new();
        let before = state.state_root_material();
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::SlashEvidenceReport;
        tx.data = Some("{}".into());
        tx.set_id();
        let block = make_block(vec![tx], 100);
        let transitions = extract_transitions(&state, &block, block.height, 0);
        assert!(
            transitions.is_empty(),
            "the staged type must have no native effect"
        );
        assert_eq!(state.state_root_material(), before);
    }

    #[test]
    fn faucet_block_one_producer_vector_3039() {
        const CHILD: &str = "WRFCOIN_FAUCET_BLOCK_ONE_CHILD";
        let child_chain = std::env::var(CHILD).ok();
        if crate::helpers::test_env::isolated_chain_domain(
            "transition_extractor::tests::faucet_block_one_producer_vector_3039",
            child_chain.as_deref().unwrap_or("wrfcoin-main"),
            "mainnet",
            "0x2727272727272727272727272727272727272727",
            "none",
        ) {
            return;
        }
        let _env = crate::helpers::test_env::lock();
        let Some(chain_id) = child_chain else {
            for chain_id in ["777", "wrfcoin-main"] {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "transition_extractor::tests::faucet_block_one_producer_vector_3039", "--nocapture"])
                    .env(CHILD, chain_id).env("WRFCOIN_CHAIN_ID", chain_id)
                    .env("WRFCOIN_NETWORK_ID", "mainnet")
                    .env("WRFCOIN_GENESIS_FAUCET_ADDRESS", "0x2727272727272727272727272727272727272727")
                    .env("WRFCOIN_GENESIS_ORACLE_SET", "none")
                    .status().unwrap();
                assert!(status.success(), "block-1 vector failed for {chain_id}");
            }
            return;
        };
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            use std::sync::Arc;
            use tokio::sync::RwLock;
            use wrfcoin_native_chain::blockchain::chain::Blockchain;
            use wrfcoin_native_chain::config::settings::Config;
            use wrfcoin_native_chain::consensus::proof_of_publishing::{
                coinbase_allowance, EmissionPartition, EmissionRole, ProofOfPublishing,
            };
            use wrfcoin_native_chain::mempool::pool::Mempool;
            use wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig;

            let sink = addr(39);
            let before = State::new();
            let chain = Arc::new(RwLock::new(Blockchain::default()));
            assert_eq!(chain.read().await.height, 0);
            let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
            pop.declare_no_genesis_oracles();
            pop.set_genesis_faucet_address(sink);
            let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
            let block = pop.create_next_block(chain.clone(), mempool, &addr(41).to_string())
                .await.expect("real producer must build block 1");
            assert_eq!(block.height, 1);
            let config = GenesisEmissionConfig::from_block(&block).unwrap();
            assert_eq!(config.chain_id, chain_id);
            assert_eq!(config.faucet_address, Some(sink));
            let producer = chain.read().await.state.clone();
            assert_eq!(producer.genesis_emission_config, Some(config));
            let expected = EmissionPartition::at_height(1, Some(&chain_id)).slice(EmissionRole::Faucet);
            assert_eq!(expected > 0, chain_id == "777");
            assert_eq!(producer.emission_ledger.height(), 1);
            assert_eq!(producer.emission_ledger.spent(EmissionRole::Faucet), expected);
            assert_eq!(producer.get_account(&sink).map(|a| a.balance.into_inner()).unwrap_or(0), expected);
            assert_eq!(producer.emission_ledger.spent(EmissionRole::Contribution), coinbase_allowance(1, Some(&chain_id)));
            let transitions = extract_transitions(&before, &block, 1, 0);
            assert!(matches!(transitions.first(), Some(StateTransition::RecordGenesisEmissionConfig { .. })));
            let mut applier = before;
            applier.apply_transitions_batch_at_height(1, transitions).unwrap();
            assert_eq!(applier.state_root, producer.state_root);
            assert_eq!(applier.emission_ledger, producer.emission_ledger);
            assert_eq!(applier.genesis_emission_config, producer.genesis_emission_config);
        });
    }

    #[tokio::test]
    async fn producer_settlement_real_producer_extractor_and_durable_wal_phase_b() {
        if crate::helpers::test_env::isolated_chain_domain(
            "transition_extractor::tests::producer_settlement_real_producer_extractor_and_durable_wal_phase_b",
            "wrfcoin-main",
            "mainnet",
            "0x2727272727272727272727272727272727272727",
            "none",
        ) {
            return;
        }
        let _env = crate::helpers::test_env::lock();
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use wrfcoin_native_chain::blockchain::chain::Blockchain;
        use wrfcoin_native_chain::config::settings::Config;
        use wrfcoin_native_chain::consensus::proof_of_publishing::{EmissionRole, ProofOfPublishing};
        use wrfcoin_native_chain::mempool::pool::Mempool;
        use wrfcoin_native_chain::storage::{Storage, DEFAULT_CHECKPOINT_INTERVAL};
        use wrfcoin_shared_protocol::crypto::signer::test_support::FixtureSigner as EnvKeySigner;
        use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::PRODUCER_SETTLEMENT_METADATA_KEY;

        let mut rng = StdRng::seed_from_u64(2846_777);
        let secret = PrivateKey::generate(&mut rng);
        let key = PublicKey::from_private_key(&secret).as_bytes().try_into().unwrap();
        let payout = addr(71);
        let member = ProducerMember { producer_id: "phase-b".into(),
            payout_address: payout.to_string(), public_key: key };
        let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
        pop.declare_no_genesis_oracles();
        pop.set_genesis_faucet_address(addr(39));
        pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(secret, "phase-b-fixture")));
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        chain.write().await.state.seed_genesis_producer_set(vec![member]).unwrap();
        let before = chain.read().await.state.clone();
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let block = pop.create_next_block(chain.clone(), mempool, &payout.to_string())
            .await.expect("authorized producer must create block");
        assert!(block.metadata.contains_key(PRODUCER_SETTLEMENT_METADATA_KEY),
            "disconnect guard: actual production must stage settlement metadata");
        let producer = chain.read().await.state.clone();
        let receipt = producer.last_producer_settlement.as_ref().unwrap();
        assert_eq!(receipt.payout_address, payout.to_string());
        assert!(receipt.amount > 0);
        assert_eq!(producer.emission_ledger.spent(EmissionRole::Producer), receipt.amount);
        let transitions = extract_transitions(&before, &block, block.height, 0);
        assert!(transitions.iter().any(|t| matches!(t, StateTransition::ApplyProducerSettlement { .. })));
        let mut applier = before.clone();
        applier.apply_transitions_batch_at_height(block.height, transitions.clone()).unwrap();
        let db = std::env::temp_dir().join(format!("producer-phase-b-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
        storage.persist_block(block.clone(), transitions.clone(), None).await.unwrap();
        let rows = storage.load_wal_transitions_range(block.height, block.height).unwrap();
        assert_eq!(rows.len(), transitions.len());
        let mut wal = before;
        wal.apply_transitions_batch_at_height(block.height, rows.into_iter().map(|(_, t)| t)).unwrap();
        for surface in [&applier, &wal] {
            assert_eq!(surface.state_root, producer.state_root);
            assert_eq!(surface.last_producer_settlement, producer.last_producer_settlement);
            assert_eq!(surface.emission_ledger, producer.emission_ledger);
            assert_eq!(surface.get_account(&payout).unwrap().balance,
                producer.get_account(&payout).unwrap().balance);
        }
        drop(storage);
        std::fs::remove_file(db).unwrap();
    }

    #[tokio::test]
    async fn producer_settlement_max_supply_remainder_producer_applier_wal_phase_b() {
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use wrfcoin_native_chain::blockchain::chain::Blockchain;
        use wrfcoin_native_chain::config::settings::Config;
        use wrfcoin_native_chain::consensus::proof_of_publishing::{EmissionRole, ProofOfPublishing};
        use wrfcoin_native_chain::mempool::pool::Mempool;
        use wrfcoin_native_chain::storage::{Storage, DEFAULT_CHECKPOINT_INTERVAL};
        use wrfcoin_shared_protocol::crypto::signer::test_support::FixtureSigner as EnvKeySigner;
        use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::PRODUCER_SETTLEMENT_METADATA_KEY;

        use wrfcoin_shared_protocol::consensus::proof_of_publishing::{coinbase_allowance, MAX_SUPPLY};
        use wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig;
        let mut rng = StdRng::seed_from_u64(2846_777);
        let secret = PrivateKey::generate(&mut rng);
        let key = PublicKey::from_private_key(&secret).as_bytes().try_into().unwrap();
        let payout = addr(71);
        let member = ProducerMember { producer_id: "phase-b".into(),
            payout_address: payout.to_string(), public_key: key };
        let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
        pop.declare_no_genesis_oracles();
        pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(secret, "phase-b-fixture")));
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let coinbase = coinbase_allowance(2, Some("777"));
        {
            let mut chain = chain.write().await;
            chain.height = 1;
            chain.state.seed_genesis_producer_set(vec![member]).unwrap();
            chain.state.open_emission_block(1);
            chain.state.apply_transition(StateTransition::RecordGenesisEmissionConfig {
                config: GenesisEmissionConfig { chain_id: "777".into(), faucet_address: None , ..Default::default() },
            }).unwrap();
            chain.state.create_account(addr(98), Balance::new(MAX_SUPPLY - coinbase - 7));
            chain.state.open_emission_block(1);
        }
        let before = chain.read().await.state.clone();
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let block = pop.create_next_block(chain.clone(), mempool, &payout.to_string())
            .await.expect("authorized producer must create block");
        assert!(block.metadata.contains_key(PRODUCER_SETTLEMENT_METADATA_KEY),
            "disconnect guard: actual production must stage settlement metadata");
        let producer = chain.read().await.state.clone();
        let receipt = producer.last_producer_settlement.as_ref().unwrap();
        assert_eq!(receipt.payout_address, payout.to_string());
        assert_eq!(receipt.amount, 7, "producer credits precisely the remaining supply");
        let slice = producer.emission_ledger.partition().slice(EmissionRole::Producer);
        assert!(slice > 7);
        assert_eq!(producer.emission_ledger.remaining(EmissionRole::Producer), slice - 7);
        assert_eq!(producer.emission_ledger.spent(EmissionRole::Contribution), coinbase);
        assert_eq!(producer.total_minted_supply(), MAX_SUPPLY);
        assert_eq!(producer.get_account(&payout).unwrap().balance.into_inner(), coinbase + 7);
        let metadata_receipt: serde_json::Value = serde_json::from_str(
            block.metadata.get(PRODUCER_SETTLEMENT_METADATA_KEY).unwrap()).unwrap();
        assert_eq!(metadata_receipt["amount"], serde_json::json!(7));
        assert_eq!(producer.emission_ledger.spent(EmissionRole::Producer), receipt.amount);
        let transitions = extract_transitions(&before, &block, block.height, 0);
        assert!(transitions.iter().any(|t| matches!(t, StateTransition::ApplyProducerSettlement { .. })));
        let mut applier = before.clone();
        applier.apply_transitions_batch_at_height(block.height, transitions.clone()).unwrap();
        let db = std::env::temp_dir().join(format!("producer-phase-b-remainder-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
        storage.persist_block(block.clone(), transitions.clone(), None).await.unwrap();
        let rows = storage.load_wal_transitions_range(block.height, block.height).unwrap();
        assert_eq!(rows.len(), transitions.len());
        let mut wal = before;
        wal.apply_transitions_batch_at_height(block.height, rows.into_iter().map(|(_, t)| t)).unwrap();
        for surface in [&applier, &wal] {
            assert_eq!(surface.state_root, producer.state_root);
            assert_eq!(surface.last_producer_settlement, producer.last_producer_settlement);
            assert_eq!(surface.emission_ledger, producer.emission_ledger);
            assert_eq!(surface.get_account(&payout).unwrap().balance,
                producer.get_account(&payout).unwrap().balance);
        }
        drop(storage);
        std::fs::remove_file(db).unwrap();
    }

    #[tokio::test]
    async fn faucet_real_producer_extractor_and_durable_wal_agree_3039() {
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use wrfcoin_native_chain::blockchain::chain::Blockchain;
        use wrfcoin_native_chain::config::settings::Config;
        use wrfcoin_native_chain::consensus::proof_of_publishing::{
            EmissionPartition, EmissionRole, ProofOfPublishing,
        };
        use wrfcoin_native_chain::mempool::pool::Mempool;
        use wrfcoin_native_chain::storage::{Storage, DEFAULT_CHECKPOINT_INTERVAL};
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            GENESIS_CHAIN_ID_KEY, GENESIS_FAUCET_ADDRESS_KEY,
        };

        let sink = addr(39);
        let mut genesis = make_block(Vec::new(), 1);
        genesis.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
        genesis.metadata.insert(GENESIS_FAUCET_ADDRESS_KEY.into(), sink.to_string());
        let genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
        assert_eq!(genesis_transitions.len(), 1, "genesis config must reach WAL");
        let mut before = State::new();
        before.apply_transitions_batch_at_height(1, genesis_transitions.clone()).unwrap();
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        {
            let mut c = chain.write().await;
            c.height = 1;
            c.state = before.clone();
        }
        let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
        // A different local cut sink must not redirect an already-cut chain.
        pop.set_genesis_faucet_address(addr(40));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let block = pop.create_next_block(chain.clone(), mempool, &addr(41).to_string())
            .await.expect("real producer must build the post-genesis block");
        let producer = chain.read().await.state.clone();
        let expected = EmissionPartition::at_height(2, Some("777"))
            .slice(EmissionRole::Faucet);
        assert!(expected > 0);
        assert_eq!(producer.emission_ledger.spent(EmissionRole::Faucet), expected,
            "disconnect guard: production must actually credit the faucet");
        assert_eq!(producer.get_account(&sink).unwrap().balance.into_inner(), expected);
        assert!(producer.get_account(&addr(40)).is_none());
        let transitions = extract_transitions(&before, &block, block.height, 0);
        assert!(transitions.iter().any(|t| matches!(t, StateTransition::ApplyFaucetReward)));
        let mut applier = before.clone();
        applier.apply_transitions_batch_at_height(block.height, transitions.clone()).unwrap();

        let db = std::env::temp_dir().join(format!("faucet-parity-3039-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
        storage.persist_block(genesis, genesis_transitions, None).await.unwrap();
        storage.persist_block(block, transitions, None).await.unwrap();
        let rows = storage.load_wal_transitions_range(1, 2).unwrap();
        assert!(rows.len() >= 3, "config, coinbase, and faucet must survive storage");
        let mut wal = State::new();
        for height in [1, 2] {
            wal.apply_transitions_batch_at_height(height,
                rows.iter().filter(|(h, _)| *h == height).map(|(_, t)| t.clone())).unwrap();
        }
        for surface in [&applier, &wal] {
            assert_eq!(surface.state_root, producer.state_root);
            assert_eq!(surface.genesis_emission_config, producer.genesis_emission_config);
            assert_eq!(surface.emission_ledger, producer.emission_ledger);
            assert_eq!(surface.get_account(&sink).unwrap().balance.into_inner(), expected);
        }
        drop(storage);
        std::fs::remove_file(db).unwrap();
    }

    fn fund_governance_voter(state: &mut State, voter: Address, voting_power: u64) {
        let stake_amount = Balance::new(voting_power as u128);
        state.create_account(voter, stake_amount);
        state
            .apply_transition(StateTransition::LockToStake {
                address: voter,
                amount: stake_amount,
            })
            .unwrap();
    }

    fn governance_voter(seed: u64) -> (PrivateKey, PublicKey, Address) {
        let mut rng = StdRng::seed_from_u64(seed);
        let private_key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&private_key);
        let address = Address::from_public_key(&public_key);
        (private_key, public_key, address)
    }

    fn signed_vote_json(
        proposal_id: &str,
        approve: bool,
        voting_power: Option<u64>,
        private_key: &PrivateKey,
        public_key: &PublicKey,
    ) -> String {
        let mut payload = GovernanceVotePayload {
            proposal_id: proposal_id.to_string(),
            approve,
            voting_power,
            signature: None,
            // #2229: the domain body binds `signer_public_key`, so it must be
            // set before the preimage is computed.
            signer_public_key: Some(public_key.to_hex()),
        };
        // #2229: sign the version-appropriate preimage (legacy string at V1,
        // domain-separated at V2), matching `verify_vote_signature`.
        use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
        let signature = match SigningProtocolVersion::active() {
            SigningProtocolVersion::V1 => {
                private_key.sign(payload.canonical_signing_message().as_bytes())
            },
            SigningProtocolVersion::V2 => {
                wrfcoin_native_chain::crypto::domain_signing::sign_domain(private_key, &payload)
            },
        };
        payload.signature = Some(signature.to_hex());
        serde_json::to_string(&payload).unwrap()
    }

    #[test]
    fn extract_transitions_includes_due_governance_finalization() {
        let mut pre_state = State::new();
        let proposer = addr(1);
        let (sk, pk, voter) = governance_voter(9100);
        pre_state.create_account(proposer, Balance::new(20_000));
        fund_governance_voter(&mut pre_state, voter, 10);

        let mut proposal_tx = Transaction::new(vec![], vec![]);
        proposal_tx.sender_address = proposer;
        proposal_tx.tx_type = TransactionType::GovernanceProposal;
        proposal_tx.data = Some(
            serde_json::to_string(&GovernanceProposalPayload {
                proposal_id: "extract-p1".to_string(),
                parameter: "emission_rate".to_string(),
                value: "77".to_string(),
                voting_deadline: Some(3),
                deposit: Some(GOVERNANCE_DEFAULT_DEPOSIT),
            })
            .unwrap(),
        );
        proposal_tx.set_id();
        pre_state
            .apply_transition(StateTransition::ProcessGovernanceTransaction {
                tx: proposal_tx,
                current_block: 1,
            })
            .unwrap();

        let mut vote_tx = Transaction::new(vec![], vec![]);
        vote_tx.sender_address = voter;
        vote_tx.tx_type = TransactionType::GovernanceVote;
        vote_tx.data = Some(signed_vote_json("extract-p1", true, Some(10), &sk, &pk));
        vote_tx.set_id();
        pre_state
            .apply_transition(StateTransition::ProcessGovernanceTransaction {
                tx: vote_tx,
                current_block: 2,
            })
            .unwrap();

        let block = make_block(vec![], 3);
        let transitions = extract_transitions(&pre_state, &block, 3, 3);
        assert!(
            transitions.iter().any(|transition| matches!(
                transition,
                StateTransition::FinalizeGovernanceProposal {
                    proposal_id,
                    current_block,
                    current_timestamp,
                } if proposal_id == "extract-p1" && *current_block == 3 && *current_timestamp == 3
            )),
            "due governance proposal should finalize in the extracted transition batch"
        );

        let mut replay_state = pre_state.clone();
        for transition in transitions {
            replay_state.apply_transition(transition).unwrap();
        }

        let proposal = replay_state.governance.proposals.get("extract-p1").unwrap();
        assert_eq!(proposal.status, GovernanceProposalStatus::Executed);
        assert_eq!(proposal.finalized_at_block, Some(3));
        assert_eq!(replay_state.governance.params.emission_rate, 77);
    }

    #[test]
    fn extract_transitions_handles_coinbase() {
        let state = State::new();
        let miner = addr(0xAA);
        let coinbase = Transaction::new_coinbase(&miner.to_string(), 5000, 1);
        let block = make_block(vec![coinbase], 1);
        let transitions = extract_transitions(&state, &block, 1, 1);
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyReward { address, amount, reason }
                    if *address == miner && *amount == Balance::new(5000) && reason == "coinbase"
            )),
            "coinbase transaction must produce an ApplyReward transition"
        );
    }

    /// mobile#215 — the P2P APPLIER reconstruction path maps a gamification
    /// `Reward` tx to `ApplyGamificationReward` with the same (address, amount,
    /// claim_id, block height) the producer derives
    /// (`proof_of_publishing::launch_transition_for_transaction`). Identical
    /// transition on both paths ⇒ identical applied state_root — the
    /// determinism invariant this feature depends on.
    #[test]
    fn extract_transitions_maps_gamification_reward() {
        let operator = gamification_test_operator();
        let state = State::new();
        let recipient = addr(0x09);
        let tx = Transaction::new_gamification_reward(
            &recipient.to_string(),
            500,
            "claim-x",
            &operator,
        );
        let block = make_block(vec![tx], 42);

        let transitions = extract_transitions(&state, &block, 42, 1);
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyGamificationReward { address, amount, claim_id, current_block }
                    if *address == recipient
                        && *amount == Balance::new(500)
                        && claim_id == "claim-x"
                        && *current_block == 42
            )),
            "gamification reward tx must map to ApplyGamificationReward, got {transitions:?}"
        );
        assert!(
            !transitions
                .iter()
                .any(|t| matches!(t, StateTransition::ApplyReward { .. })),
            "gamification reward must NOT also emit a plain ApplyReward"
        );
    }

    /// A fixed gamification-operator key whose public key is installed into the
    /// consensus allowlist env (#2081 review). Deterministic and set-only, so
    /// every gamification test in this binary installs the same value. That
    /// determinism is for reproducibility, NOT safety: the write itself is
    /// serialized on the process-env lock.
    fn gamification_test_operator() -> wrfcoin_native_chain::crypto::signature::PrivateKey {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::domain_signing::GAMIFICATION_OPERATOR_PUBKEYS_ENV;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        let mut rng = rand::rngs::StdRng::seed_from_u64(215_215);
        let key = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&key).to_hex();
        // THE process-env lock. Writing a byte-identical value from a fixed
        // seed does NOT make this safe: `setenv` mutates the `environ` array
        // and may reallocate and free it, so concurrent writes race at the
        // libc level whatever the value — which is why Rust 2024 marks
        // `set_var` unsafe.
        let _env_guard = crate::helpers::test_env::lock();
        std::env::set_var(GAMIFICATION_OPERATOR_PUBKEYS_ENV, pk);
        key
    }

    /// #2081 review, blocker 1: a gamification claim signed by a key that is NOT
    /// on the operator allowlist must emit NO minting transition — the P2P-applier
    /// forgery the fix closes.
    #[test]
    fn extract_transitions_rejects_unauthorized_gamification_reward() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::PrivateKey;
        let _authorized = gamification_test_operator(); // installs the allowlist
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xBAD_1);
        let attacker = PrivateKey::generate(&mut rng);

        let state = State::new();
        let recipient = addr(0x09);
        let tx = Transaction::new_gamification_reward(
            &recipient.to_string(),
            500,
            "claim-forged",
            &attacker,
        );
        let block = make_block(vec![tx], 42);
        let transitions = extract_transitions(&state, &block, 42, 1);
        assert!(
            !transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyGamificationReward { .. } | StateTransition::ApplyReward { .. }
            )),
            "unauthorized gamification claim must emit no minting transition, got {transitions:?}"
        );
    }

    /// A fixed oracle-governance key installed into the consensus allowlist env
    /// (#2055). Deterministic + set-only, so parallel oracle tests install the
    /// same value. Determinism is for reproducibility, not safety — the write
    /// is serialized on the process-env lock.
    ///
    /// Returns the process-env lock guard with the key: the allowlist is READ later, at
    /// verification time, so the caller must hold the guard to the end of its test (#3443).
    fn oracle_governance_test_key() -> (
        std::sync::MutexGuard<'static, ()>,
        wrfcoin_native_chain::crypto::signature::PrivateKey,
    ) {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};
        let mut rng = rand::rngs::StdRng::seed_from_u64(2055_215);
        let key = PrivateKey::generate(&mut rng);
        let env_guard = crate::helpers::test_env::lock();
        std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, PublicKey::from_private_key(&key).to_hex());
        (env_guard, key)
    }

    /// #3443: the governance allowlist is READ at verification time, so the process-env lock
    /// must stay held for the whole test; a guard dropped on helper return lets another test
    /// install a different key before `extract_transitions` reads it.
    #[test]
    fn oracle_governance_helper_holds_the_env_lock_for_the_whole_test_3443() {
        let (_env_guard, _gov) = oracle_governance_test_key();
        assert!(
            matches!(
                crate::helpers::test_env::ENV_LOCK.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ),
            "the process-env lock was released before the test finished reading the allowlist"
        );
    }

    /// #2055 Option A: the P2P APPLIER maps an authorized oracle-registration tx
    /// to the compound `ApplyOracleRegistration` — byte-identical to the producer
    /// path (`launch_transition_for_transaction`), so applied state_root agrees.
    #[test]
    fn extract_transitions_maps_oracle_registration() {
        let (_env_guard, gov) = oracle_governance_test_key();
        let state = State::new();
        let identity = "0xoracle-1";
        let tx = Transaction::new_oracle_registration(
            vec![identity.to_string()],
            1,
            identity,
            &"aa".repeat(32),
            2,
            1_700_000_000,
            &gov,
        );
        let block = make_block(vec![tx], 9);
        let transitions = extract_transitions(&state, &block, 9, 1);
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyOracleRegistration { oracle_set, oracle_quorum, identity: id, epoch, .. }
                    if oracle_set == &vec![identity.to_string()]
                        && *oracle_quorum == 1
                        && id == identity
                        && *epoch == 2
            )),
            "authorized oracle registration must map to ApplyOracleRegistration, got {transitions:?}"
        );
    }

    /// #2055: an oracle-registration tx signed by a non-allowlisted key emits NO
    /// transition (fail-closed) — the forgery the fix closes.
    #[test]
    fn extract_transitions_rejects_unauthorized_oracle_registration() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::PrivateKey;
        let (_env_guard, _authorized) = oracle_governance_test_key(); // installs the allowlist
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xBAD_2);
        let attacker = PrivateKey::generate(&mut rng);

        let state = State::new();
        let identity = "0xoracle-1";
        let tx = Transaction::new_oracle_registration(
            vec![identity.to_string()],
            1,
            identity,
            &"aa".repeat(32),
            2,
            1_700_000_000,
            &attacker,
        );
        let block = make_block(vec![tx], 9);
        let transitions = extract_transitions(&state, &block, 9, 1);
        assert!(
            !transitions
                .iter()
                .any(|t| matches!(t, StateTransition::ApplyOracleRegistration { .. })),
            "unauthorized oracle registration must emit no transition, got {transitions:?}"
        );
    }

    /// Build a fully-authorized station registration payload for the tests
    /// below. Owner possession is signed first because the owner key is folded
    /// into the governance preimage.
    fn signed_station_payload(
        station_id: &str,
        provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
        owner: &wrfcoin_native_chain::crypto::signature::PrivateKey,
        governance: &wrfcoin_native_chain::crypto::signature::PrivateKey,
    ) -> wrfcoin_native_chain::blockchain::transaction::StationRegistrationPayload {
        use wrfcoin_native_chain::blockchain::transaction::StationRegistrationPayload;
        use wrfcoin_shared_protocol::station_registry::DataQualityTier;
        let mut payload = StationRegistrationPayload::new(
            station_id,
            "0xstation-owner",
            46.7867,
            -92.1005,
            "applier-parity-station",
            DataQualityTier::Gold,
            provenance_class,
        );
        payload.sign_ownership(owner);
        payload.sign(governance);
        payload
    }

    fn station_registration_tx(
        payload: &wrfcoin_native_chain::blockchain::transaction::StationRegistrationPayload,
    ) -> Transaction {
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::StationRegistration;
        tx.data = Some(serde_json::to_string(payload).expect("serialize station payload"));
        tx.set_id();
        tx
    }

    /// #3277 S4: the P2P APPLIER (and so WAL replay, which persists these
    /// rows) maps a feature-schedule extension tx to exactly the transition
    /// the producer's `launch_transition_for_transaction` emits: the parsed
    /// signed extension at the carrying height. No rule is checked here — the
    /// apply arm re-runs V1–V9 and fails the block closed — so an unsigned
    /// extension is extracted too, never silently dropped.
    #[test]
    fn extract_transitions_maps_feature_schedule_extension_3277() {
        use wrfcoin_shared_protocol::governance::schedule_extension::{
            FeatureScheduleExtension, ScheduleEntry, SignedFeatureScheduleExtension,
        };
        let signed = SignedFeatureScheduleExtension {
            extension: FeatureScheduleExtension {
                chain_id: "777".into(),
                network_id: "testnet".into(),
                genesis_hash: "ab".repeat(32),
                sequence: 0,
                entries: vec![ScheduleEntry::new("verify_unit_pricing", 2_011)],
                version_steps: Vec::new(),
                min_protocol_version: 3,
                release_commit: "c4d0a11".into(),
            },
            signatures: Vec::new(),
        };
        let tx = Transaction::new_feature_schedule_extension(&signed);
        let transitions = extract_transitions(&State::new(), &make_block(vec![tx.clone()], 11), 11, 1);
        let carried: Vec<_> = transitions
            .iter()
            .filter(|t| matches!(t, StateTransition::ApplyFeatureScheduleExtension { .. }))
            .collect();
        assert_eq!(carried.len(), 1, "exactly one extension transition: {transitions:?}");
        match carried[0] {
            StateTransition::ApplyFeatureScheduleExtension { extension, block_height } => {
                assert_eq!(extension, &signed);
                assert_eq!(*block_height, 11, "the CARRYING height, which V7 measures from");
            },
            _ => unreachable!(),
        }

        let mut garbage = tx;
        garbage.data = Some("not json".into());
        garbage.set_id();
        let transitions = extract_transitions(&State::new(), &make_block(vec![garbage], 11), 11, 1);
        assert!(
            !transitions
                .iter()
                .any(|t| matches!(t, StateTransition::ApplyFeatureScheduleExtension { .. })),
            "an unparsable payload yields no transition (its block is rejected upstream)"
        );
    }

    /// #2537: the P2P APPLIER maps an authorized station-registration tx to
    /// `ApplyStationRegistration`, carrying the SAME `WeatherStation` the
    /// producer builds (`launch_transition_for_transaction` → the shared
    /// `Transaction::station_from_registration` constructor). Producer ==
    /// applier == WAL replay, so `station_registry` — and therefore
    /// `state_root` — agrees on every peer.
    #[test]
    fn extract_transitions_maps_station_registration() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::PrivateKey;
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let (_env_guard, gov) = oracle_governance_test_key();
        let mut rng = rand::rngs::StdRng::seed_from_u64(2537_001);
        let owner = PrivateKey::generate(&mut rng);

        let payload = signed_station_payload(
            "applier-station-1",
            ContributionProvenanceClass::Real,
            &owner,
            &gov,
        );
        let tx = station_registration_tx(&payload);

        // The producer's own derivation, for byte-comparison.
        let expected = Transaction::station_from_registration(&payload);

        let state = State::new();
        let block = make_block(vec![tx], 11);
        let transitions = extract_transitions(&state, &block, 11, 1);

        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyStationRegistration { station }
                    if station == &expected
                        && station.provenance_class == ContributionProvenanceClass::Real
            )),
            "authorized station registration must map to ApplyStationRegistration carrying the \
             producer's exact record, got {transitions:?}"
        );
    }

    /// #2537: a station registration signed by a non-allowlisted governance key
    /// emits NO transition. This is the forgery that matters most — a station
    /// admitted as `Real` is the only class that earns, so an applier that
    /// accepted it would mint to an attacker on every syncing peer.
    #[test]
    fn extract_transitions_rejects_unauthorized_station_registration() {
        use rand::SeedableRng;
        use wrfcoin_native_chain::crypto::signature::PrivateKey;
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let (_env_guard, _authorized) = oracle_governance_test_key(); // installs the allowlist
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xBAD_2537);
        let attacker = PrivateKey::generate(&mut rng);
        let owner = PrivateKey::generate(&mut rng);

        let payload = signed_station_payload(
            "applier-station-forged",
            ContributionProvenanceClass::Real,
            &owner,
            &attacker,
        );
        let tx = station_registration_tx(&payload);

        let state = State::new();
        let block = make_block(vec![tx], 11);
        let transitions = extract_transitions(&state, &block, 11, 1);

        assert!(
            !transitions
                .iter()
                .any(|t| matches!(t, StateTransition::ApplyStationRegistration { .. })),
            "unauthorized station registration must emit no transition, got {transitions:?}"
        );
    }

    #[test]
    fn extract_transitions_handles_transfer() {
        // #1731: a transparent Transfer must be routed EXCLUSIVELY through the
        // governance sink (`ProcessGovernanceTransaction`), exactly as the
        // producer does (`governance_transitions_for_block`). The extractor
        // must NOT additionally emit a standalone `BalanceTransfer` — that
        // parallel path double-debited the sender on the P2P applier / WAL
        // replay, surfacing as `InsufficientSpendableBalance` at block 8.
        use rand::SeedableRng;
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;
        use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};

        let mut state = State::new();
        // #1727/#1755: transparent transfers now require a verifiable
        // from-account authorization, so derive the sender from a real keypair
        // and sign the canonical auth message.
        let mut rng = rand::rngs::StdRng::seed_from_u64(0x1731);
        let sender_sk = PrivateKey::generate(&mut rng);
        let sender_pk = PublicKey::from_private_key(&sender_sk);
        let sender = Address::from_public_key(&sender_pk);
        let recipient = addr(0x02);
        // Fund the sender so the governance simulation actually applies the
        // transfer (an unfunded sender is dropped by the producer too).
        state.create_account(sender, Balance::new(10_000));

        let nonce = 0u64;
        let amount = 1000u64;
        let auth_message = transparent_transfer_authorization_message(
            &sender.to_string(),
            &recipient.to_string(),
            amount,
            nonce,
        );
        let auth_signature = sender_sk.sign(&auth_message).to_string();
        let auth_pubkey = sender_pk.to_string();

        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::Transfer;
        tx.sender_address = sender;
        tx.nonce = nonce;
        tx.data = Some(
            serde_json::to_string(&serde_json::json!({
                "from": sender.to_string(),
                "to": recipient.to_string(),
                "amount": amount,
                "transparent_opt_out": true,
                "auth_pubkey": auth_pubkey,
                "auth_signature": auth_signature,
            }))
            .unwrap(),
        );
        tx.set_id();

        let block = make_block(vec![tx.clone()], 5);
        let transitions = extract_transitions(&state, &block, 5, 5);

        // Exactly ONE governance-routed transition for the transfer.
        let gov_transfers = transitions
            .iter()
            .filter(|t| matches!(
                t,
                StateTransition::ProcessGovernanceTransaction { tx: gtx, .. }
                    if gtx.id == tx.id
            ))
            .count();
        assert_eq!(
            gov_transfers, 1,
            "Transfer must produce exactly one ProcessGovernanceTransaction \
             (governance sink), got transitions: {transitions:?}"
        );

        // And NO standalone BalanceTransfer (the removed parallel path that
        // caused the #1731 double-debit).
        assert!(
            !transitions
                .iter()
                .any(|t| matches!(t, StateTransition::BalanceTransfer { .. })),
            "Transfer must NOT emit a standalone BalanceTransfer (#1731 \
             double-apply regression), got transitions: {transitions:?}"
        );

        // End-to-end: applying the extracted transitions debits the sender
        // exactly once (1000), not twice.
        let mut applied = state.clone();
        applied
            .apply_transitions_batch(transitions.into_iter())
            .expect("extracted transitions must apply cleanly (single debit)");
        assert_eq!(
            applied.get_account(&sender).unwrap().spendable_balance,
            Balance::new(9_000),
            "sender must be debited exactly once (10_000 - 1_000)"
        );
        assert_eq!(
            applied.get_account(&recipient).unwrap().spendable_balance,
            Balance::new(1_000),
            "recipient must be credited exactly once"
        );
    }

    /// Build a Stake/Unstake transaction authorized by `account_key` (#1763).
    /// The staking account is the key's derived address; the payload carries a
    /// valid `account`-bound authorization over `account|amount|action|nonce`,
    /// so the extractor emits the staking transition.
    fn build_authorized_stake_tx(
        account_key: &PrivateKey,
        amount: u64,
        is_unstake: bool,
        nonce: u64,
    ) -> Transaction {
        let account =
            Address::from_public_key(&PublicKey::from_private_key(account_key));
        let mut payload = StakeAuthPayload {
            account: account.to_hex(),
            amount,
            auth_pubkey: None,
            auth_signature: None,
        };
        payload.sign_authorization(account_key, is_unstake, nonce);
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = if is_unstake {
            TransactionType::Unstake
        } else {
            TransactionType::Stake
        };
        tx.sender_address = account;
        tx.nonce = nonce;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();
        tx
    }

    #[test]
    fn extract_transitions_handles_stake_unstake() {
        let mut state = State::new();
        let (staker_key, staker_pk, staker) = governance_voter(0x0303);
        let _ = staker_pk;
        // Fund the staker so the emitted transitions also apply cleanly.
        state.create_account(staker, Balance::new(10_000));

        let stake_tx = build_authorized_stake_tx(&staker_key, 2000, false, 0);
        let unstake_tx = build_authorized_stake_tx(&staker_key, 500, true, 1);

        let block = make_block(vec![stake_tx, unstake_tx], 10);
        let transitions = extract_transitions(&state, &block, 10, 10);

        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::LockToStake { address, amount }
                    if *address == staker && *amount == Balance::new(2000)
            )),
            "authorized Stake transaction must produce a LockToStake transition"
        );
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ReleaseToSpendable { address, amount }
                    if *address == staker && *amount == Balance::new(500)
            )),
            "authorized Unstake transaction must produce a ReleaseToSpendable transition"
        );

        // End-to-end: applying the extracted transitions mutates the staker's
        // stake/spendable split exactly as authorized (lock 2000, release 500).
        let mut applied = state.clone();
        applied
            .apply_transitions_batch(transitions.into_iter())
            .expect("authorized stake transitions must apply cleanly");
        let acct = applied.get_account(&staker).unwrap();
        assert_eq!(acct.stake_locked, Balance::new(1_500), "2000 locked - 500 released");
        assert_eq!(
            acct.spendable_balance,
            Balance::new(8_500),
            "10_000 - 2000 + 500"
        );
    }

    #[test]
    fn stake_and_unstake_extractor_use_the_inclusion_height_for_the_state_view_2168() {
        const CHILD: &str = "WRFCOIN_AA_HEIGHT_2168_CHILD";
        const H: u64 = 21_680;
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "transition_extractor::tests::stake_and_unstake_extractor_use_the_inclusion_height_for_the_state_view_2168", "--nocapture"])
                .env(CHILD, "1")
                .status()
                .expect("re-exec child");
            assert!(status.success(), "isolated schedule child failed");
            return;
        }
        use wrfcoin_shared_protocol::accounts::authorization::{
            AccountAuthorization, AccountAuthorizationDeclaration, AuthorizationRule,
        };
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            install_feature_schedule, ConsensusFeature, FeatureSchedule,
        };
        install_feature_schedule(FeatureSchedule::from_entries(vec![(
            ConsensusFeature::AccountAuthorizationDispatch.name().to_string(), H,
        )]).expect("schedule"));
        assert!(wrfcoin_shared_protocol::accounts::authorization::dispatch_active(H));
        assert!(!wrfcoin_shared_protocol::accounts::authorization::dispatch_active(H - 1));

        let (key, _, owner) = governance_voter(21_680);
        for (height, accepted) in [(H - 1, true), (H, false)] {
            let mut state = State::new();
            state.create_account(owner, Balance::new(10_000));
            state.authorization_declarations.insert(owner, AccountAuthorizationDeclaration {
                next_nonce: 0,
                current: Some(AccountAuthorization::declaring(AuthorizationRule::SmartAccount { program: [7; 32] })),
                generation: 0,
            });
            let stake = build_authorized_stake_tx(&key, 100, false, 0);
            let unstake = build_authorized_stake_tx(&key, 100, true, 1);
            let transitions = extract_transitions(&state, &make_block(vec![stake, unstake], height), height, height as i64);
            assert_eq!(
                transitions.iter().any(|t| matches!(t, StateTransition::LockToStake { .. })),
                accepted,
                "stake extractor at height {height}"
            );
            assert_eq!(
                transitions.iter().any(|t| matches!(t, StateTransition::ReleaseToSpendable { .. })),
                accepted,
                "unstake extractor at height {height}"
            );
        }
    }

    #[test]
    fn extract_transitions_rejects_forged_stake() {
        // #1763: a Stake whose `account` the submitter does NOT control (no
        // valid authorization) must emit NO transition — the spoof the extractor
        // gate must drop. This covers both no-authorization and a well-formed
        // signature from a foreign key (which does not derive to `account`).
        let state = State::new();
        let victim = addr(0x55);

        // (1) No authorization at all.
        let mut unauth = Transaction::new(vec![], vec![]);
        unauth.tx_type = TransactionType::Stake;
        unauth.sender_address = victim;
        unauth.data = Some(
            serde_json::to_string(&StakeAuthPayload {
                account: victim.to_hex(),
                amount: 1_000,
                auth_pubkey: None,
                auth_signature: None,
            })
            .unwrap(),
        );
        unauth.set_id();

        // (2) Signed by an attacker key that does not derive to `victim`.
        let (attacker_key, _, _) = governance_voter(0xBEEF);
        let mut forged = StakeAuthPayload {
            account: victim.to_hex(),
            amount: 1_000,
            auth_pubkey: None,
            auth_signature: None,
        };
        forged.sign_authorization(&attacker_key, false, 0);
        let mut forged_tx = Transaction::new(vec![], vec![]);
        forged_tx.tx_type = TransactionType::Stake;
        forged_tx.sender_address = victim;
        forged_tx.data = Some(serde_json::to_string(&forged).unwrap());
        forged_tx.set_id();

        let block = make_block(vec![unauth, forged_tx], 10);
        let transitions = extract_transitions(&state, &block, 10, 10);
        assert!(
            !transitions
                .iter()
                .any(|t| matches!(t, StateTransition::LockToStake { .. })),
            "forged/unauthorized Stake must NOT produce a LockToStake transition (#1763)"
        );
    }

    #[test]
    fn extract_transitions_reads_reward_entries_from_metadata() {
        let state = State::new();
        let recipient = addr(0xBB);
        let reward_entries = vec![ContributionRewardRecord {
            address: recipient,
            amount: Balance::new(42_000),
            reason: "contribution:c1".to_string(),
            station_id: "station-real-1".to_string(),
            provenance_class: ContributionProvenanceClass::Real,
        }];

        let mut block = make_block(vec![], 7);
        block.metadata.insert(
            REWARD_ENTRIES_METADATA_KEY.to_string(),
            serde_json::to_string(&reward_entries).unwrap(),
        );

        let transitions = extract_transitions(&state, &block, 7, 7);
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyContributionRewardDecision { record, .. }
                    if record == &reward_entries[0]
            )),
            "reward_entries metadata must produce the typed contribution reward transition"
        );
    }

    #[test]
    fn legacy_three_field_reward_entries_fail_closed_when_executed() {
        let state = State::new();
        let recipient = addr(0xBC);
        let legacy_entries = serde_json::json!([{
            "address": recipient,
            "amount": Balance::new(42_000),
            "reason": "contribution:legacy-c1"
        }]);

        let mut block = make_block(vec![], 7);
        block.metadata.insert(
            REWARD_ENTRIES_METADATA_KEY.to_string(),
            serde_json::to_string(&legacy_entries).unwrap(),
        );

        let transitions = extract_transitions(&state, &block, 7, 7);
        assert_eq!(transitions.len(), 1);
        assert!(matches!(
            transitions[0],
            StateTransition::RejectInvalidContributionRewardMetadata { .. }
        ));

        let mut replay_state = state;
        let error = replay_state
            .apply_transition(transitions.into_iter().next().unwrap())
            .expect_err("legacy three-field reward metadata must fail during execution");
        assert!(
            error
                .to_string()
                .contains("invalid reward_entries metadata"),
            "unexpected execution error: {error}"
        );
    }

    #[test]
    fn extract_transitions_reads_oracle_local_accuracy_entries_from_metadata() {
        let state = State::new();
        let entries = vec![(
            "oracle-local-1".to_string(),
            "station-local-1".to_string(),
            9_250u32,
            12u32,
        )];

        let mut block = make_block(vec![], 7);
        block.metadata.insert(
            ORACLE_LOCAL_ACCURACY_ENTRIES_METADATA_KEY.to_string(),
            serde_json::to_string(&entries).unwrap(),
        );

        let transitions = extract_transitions(&state, &block, 7, 7);
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::RecordOracleLocalAccuracy {
                    oracle_id,
                    station_id,
                    score_bps,
                    window,
                } if oracle_id == "oracle-local-1"
                    && station_id == "station-local-1"
                    && *score_bps == 9_250
                    && *window == 12
            )),
            "oracle_local_accuracy_entries metadata must produce replay transitions"
        );
    }

    #[test]
    fn extract_transitions_ignores_untrusted_low_quality_streak_metadata() {
        let state = State::new();
        let streaked = addr(0xCC);
        let reset = addr(0xDD);
        let entries = vec![(streaked.to_string(), Some(2u32)), (reset.to_string(), None)];

        let mut block = make_block(vec![], 8);
        block.metadata.insert(
            "low_quality_streak_entries".to_string(),
            serde_json::to_string(&entries).unwrap(),
        );

        let transitions = extract_transitions(&state, &block, 8, 8);
        assert!(
            !transitions.iter().any(|t| matches!(
                t,
                StateTransition::SetLowQualityStreak { .. }
            )),
            "unvalidated low-quality metadata must not produce consensus transitions"
        );
    }

    #[test]
    fn extract_transitions_no_op_for_data_types() {
        let state = State::new();
        for tt in [
            TransactionType::Regular,
            TransactionType::DataAnalysis,
            TransactionType::DataCollection,
            TransactionType::DataStorage,
            TransactionType::EnvironmentalOffset,
            TransactionType::BridgeLock,
            TransactionType::BridgeUnlock,
            TransactionType::NftCreation,
            TransactionType::NftTransfer,
            TransactionType::NftExecution,
            TransactionType::Layer2BatchSubmit,
            TransactionType::GovernanceExecute,
            TransactionType::GovernanceFinalize,
            TransactionType::GovernanceDelegate,
            TransactionType::TreasuryDeposit,
            TransactionType::TreasuryWithdraw,
        ] {
            let mut tx = Transaction::new(vec![], vec![]);
            tx.tx_type = tt;
            tx.set_id();
            let block = make_block(vec![tx], 1);
            let transitions = extract_transitions(&state, &block, 1, 1);
            assert!(
                transitions.is_empty(),
                "{tt} should produce no state transitions"
            );
        }
    }

    /// TDD verification for PR #994: every `TransactionType` variant goes
    /// through `extract_transitions` and produces the expected transition
    /// category (or none for data-only types). Running twice asserts
    /// determinism.
    #[test]
    fn extract_transitions_all_variants_determinism() {
        use wrfcoin_native_chain::blockchain::transaction::TransactionOutput;

        let mut state = State::new();
        let sender = addr(0x10);
        let recipient = addr(0x20);
        state.create_account(sender, Balance::new(1_000_000));
        fund_governance_voter(&mut state, addr(0x30), 100);

        // Helper: build a tx with a given type and optional JSON data payload
        let build_tx = |tt: TransactionType, data: Option<String>| -> Transaction {
            let mut tx = Transaction::new(
                vec![],
                vec![TransactionOutput {
                    amount: 500,
                    address: sender.to_string(),
                    script: String::new(),
                    nft_data: None,
                }],
            );
            tx.tx_type = tt;
            tx.sender_address = sender;
            tx.nonce = 1;
            tx.data = data;
            tx.set_id();
            tx
        };

        // -- Commit payloads (ContributionCommit, ForecastCommit) -- signed (#1848)
        let commit_json = {
            let mut rng = StdRng::seed_from_u64(0xC0117);
            let private_key = PrivateKey::generate(&mut rng);
            let mut p = CommitPayload {
                commit_id: "c001".to_string(),
                station_id: "WX-TEST-01".to_string(),
                timestamp: 1700000000i64,
                contribution_type: "observation".to_string(),
                auth_pubkey: None,
                auth_signature: None,
            };
            p.sign_authorization(&private_key);
            serde_json::to_string(&p).unwrap()
        };

        // -- Reveal payloads (ContributionReveal, ForecastReveal) --
        let reveal_json = serde_json::to_string(&serde_json::json!({
            "commit_id": "c001",
            "station_id": "WX-TEST-01",
            "timestamp": 1700000000i64,
            "contribution_type": "observation",
            "data": {"temperature": 22.5},
            "nonce": "abc123"
        }))
        .unwrap();

        // -- OracleAttestation --
        let attestation_json = serde_json::to_string(&serde_json::json!({
            "commit_id": "c001",
            "oracle_id": "oracle-01",
            "approved": true,
            "signature": null,
            "confidence": 0.95,
            "timestamp": 1700000000i64
        }))
        .unwrap();

        // -- Transfer --
        let transfer_json = serde_json::to_string(&serde_json::json!({
            "from": sender.to_string(),
            "to": recipient.to_string(),
            "amount": 100u64,
            "transparent_opt_out": true
        }))
        .unwrap();

        // -- Governance payloads --
        let proposal_json = serde_json::to_string(&GovernanceProposalPayload {
            proposal_id: "all-var-p1".to_string(),
            parameter: "emission_rate".to_string(),
            value: "50".to_string(),
            voting_deadline: Some(999),
            deposit: Some(GOVERNANCE_DEFAULT_DEPOSIT),
        })
        .unwrap();

        let vote_json = serde_json::to_string(&GovernanceVotePayload {
            proposal_id: "all-var-p1".to_string(),
            approve: true,
            voting_power: Some(10),
            signature: None,
            signer_public_key: None,
        })
        .unwrap();

        // -- ConfidentialTransfer --
        let confidential_json = serde_json::to_string(&serde_json::json!({
            "output_commitments": ["aabb"],
            "range_proof": null,
            "stealth_data": null,
            "ring_signature": null,
            "key_images": ["ccdd"],
            "encrypted_payload": null,
            "privacy_level": "Standard"
        }))
        .unwrap();

        // -- ShieldedDeposit --
        let shielded_deposit_json = serde_json::to_string(&serde_json::json!({
            "shielded_commitment": "abcdef",
            "range_proof": "00112233",
            "transparent_amount": 500u64
        }))
        .unwrap();

        // -- ShieldedWithdrawal --
        let shielded_withdrawal_json = serde_json::to_string(&serde_json::json!({
            "nullifier": "null001",
            "shielded_commitment": "abcdef",
            "range_proof": "00112233",
            "transparent_recipient": recipient.to_string(),
            "transparent_amount": 250u64,
            "authorization_proof": "auth001"
        }))
        .unwrap();

        // -- RewardClaim --
        let reward_claim_json = serde_json::to_string(&serde_json::json!({
            "epoch": 1u64,
            "ring_signature": "sig_hex",
            "key_image": "ki_hex",
            "amount_commitment": "ac_hex",
            "range_proof": "rp_hex",
            "destination": recipient.to_string()
        }))
        .unwrap();

        // Build one tx per variant (27 total)
        let all_txs = vec![
            build_tx(TransactionType::Regular, None),
            Transaction::new_coinbase(&sender.to_string(), 5000, 1),
            build_tx(TransactionType::Transfer, Some(transfer_json)),
            // #1763: Stake/Unstake now require a valid account-bound
            // authorization to emit a transition, so build signed ones here.
            build_authorized_stake_tx(&governance_voter(0x5151).0, 500, false, 0),
            build_authorized_stake_tx(&governance_voter(0x5151).0, 250, true, 1),
            build_tx(TransactionType::NftCreation, None),
            build_tx(TransactionType::NftTransfer, None),
            build_tx(TransactionType::NftExecution, None),
            build_tx(TransactionType::DataAnalysis, None),
            build_tx(TransactionType::DataCollection, None),
            build_tx(TransactionType::DataStorage, None),
            build_tx(TransactionType::EnvironmentalOffset, None),
            build_tx(TransactionType::BridgeLock, None),
            build_tx(TransactionType::BridgeUnlock, None),
            build_tx(TransactionType::ContributionCommit, Some(commit_json.clone())),
            build_tx(TransactionType::ContributionReveal, Some(reveal_json.clone())),
            build_tx(TransactionType::ForecastCommit, Some(commit_json)),
            build_tx(TransactionType::ForecastReveal, Some(reveal_json)),
            build_tx(TransactionType::OracleAttestation, Some(attestation_json)),
            build_tx(TransactionType::GovernanceProposal, Some(proposal_json)),
            build_tx(TransactionType::GovernanceVote, Some(vote_json)),
            build_tx(TransactionType::Reward, None),
            build_tx(TransactionType::ConfidentialTransfer, Some(confidential_json)),
            build_tx(TransactionType::ShieldedDeposit, Some(shielded_deposit_json)),
            build_tx(TransactionType::ShieldedWithdrawal, Some(shielded_withdrawal_json)),
            build_tx(TransactionType::Layer2BatchSubmit, None),
            build_tx(TransactionType::RewardClaim, Some(reward_claim_json)),
        ];

        assert_eq!(
            all_txs.len(),
            27,
            "Must cover all 27 TransactionType variants"
        );

        let mut block = make_block(all_txs, 5);
        let mut producer_rng = StdRng::seed_from_u64(3069);
        let producer_secret = PrivateKey::generate(&mut producer_rng);
        let producer_key = PublicKey::from_private_key(&producer_secret).as_bytes().try_into().unwrap();
        state.seed_genesis_producer_set(vec![
            wrfcoin_shared_protocol::consensus::producer_set::ProducerMember {
                producer_id: "all-variants".into(), public_key: producer_key,
                payout_address: recipient.to_string(),
            },
        ]).unwrap();
        state.open_emission_block(5);
        let receipt = state.derive_producer_settlement(5, &producer_key).unwrap();
        block.producer_pubkey = producer_key;
        block.metadata.insert(
            wrfcoin_shared_protocol::consensus::proof_of_publishing::PRODUCER_SETTLEMENT_METADATA_KEY.into(),
            serde_json::to_string(&receipt).unwrap(),
        );

        // --- First run ---
        let transitions_a = extract_transitions(&state, &block, 5, 5);

        assert!(transitions_a.iter().any(|t| matches!(t,
            StateTransition::ApplyProducerSettlement { .. })),
            "the allow-list must exercise an actual Phase B settlement");
        assert!(transitions_a.iter().any(|t| matches!(t, StateTransition::ApplyFaucetReward)));

        // Commit types -> RegisterCommit
        let register_commit_count = transitions_a
            .iter()
            .filter(|t| matches!(t, StateTransition::RegisterCommit { .. }))
            .count();
        assert_eq!(
            register_commit_count, 2,
            "ContributionCommit + ForecastCommit -> 2 RegisterCommit transitions"
        );

        // Reveal types -> ApplyReveal
        let apply_reveal_count = transitions_a
            .iter()
            .filter(|t| matches!(t, StateTransition::ApplyReveal { .. }))
            .count();
        assert_eq!(
            apply_reveal_count, 2,
            "ContributionReveal + ForecastReveal -> 2 ApplyReveal transitions"
        );

        // OracleAttestation -> RecordOracleAttestation
        let oracle_count = transitions_a
            .iter()
            .filter(|t| matches!(t, StateTransition::RecordOracleAttestation { .. }))
            .count();
        assert_eq!(
            oracle_count, 1,
            "OracleAttestation -> 1 RecordOracleAttestation transition"
        );

        // Coinbase -> ApplyReward(coinbase)
        assert!(
            transitions_a.iter().any(|t| matches!(
                t,
                StateTransition::ApplyReward { reason, .. } if reason == "coinbase"
            )),
            "Coinbase must produce ApplyReward with reason 'coinbase'"
        );

        assert_eq!(transitions_a.iter().filter(|t| matches!(t,
            StateTransition::ApplyFaucetReward)).count(), 1,
            "the single coinbase must contribute exactly one faucet transition");

        // Reward -> ApplyReward(reward)
        assert!(
            transitions_a.iter().any(|t| matches!(
                t,
                StateTransition::ApplyReward { reason, .. } if reason == "reward"
            )),
            "Reward must produce ApplyReward with reason 'reward'"
        );

        // Transfer is governance-routed (#1731): it must NOT emit a standalone
        // BalanceTransfer. The producer applies transfers exclusively through
        // the governance sink (`ProcessGovernanceTransaction`); emitting a
        // direct BalanceTransfer here was the parallel path that double-debited
        // the sender on the applier / WAL replay. (Whether THIS block's transfer
        // also yields a ProcessGovernanceTransaction depends on the governance
        // simulation succeeding — the all-variants fixture reuses nonce 1 across
        // same-sender txs so the sim may drop it. The positive single-path
        // behavior is covered by `extract_transitions_handles_transfer` and the
        // p2p `producer_applier_block_apply_parity_governance_transfer` test;
        // here we assert only the absence of the removed parallel path.)
        assert!(
            !transitions_a
                .iter()
                .any(|t| matches!(t, StateTransition::BalanceTransfer { .. })),
            "Transfer must NOT emit a standalone BalanceTransfer (#1731 double-apply)"
        );

        // Stake -> LockToStake
        assert!(
            transitions_a
                .iter()
                .any(|t| matches!(t, StateTransition::LockToStake { .. })),
            "Stake must produce LockToStake"
        );

        // Unstake -> ReleaseToSpendable
        assert!(
            transitions_a
                .iter()
                .any(|t| matches!(t, StateTransition::ReleaseToSpendable { .. })),
            "Unstake must produce ReleaseToSpendable"
        );

        // ConfidentialTransfer / ShieldedDeposit / ShieldedWithdrawal /
        // RewardClaim are now governance-routed (#1731): the extractor no
        // longer emits a direct `Apply*` transition for them. The producer
        // applies them only via `ProcessGovernanceTransaction` (and only when
        // the payload passes validation in the governance simulation). The
        // minimal/placeholder payloads built above are intentionally not all
        // valid, so they are (correctly) dropped by the governance sink on
        // BOTH producer and applier — there is no longer a parallel direct
        // path that would apply an unvalidated privacy mutation. Their direct
        // `Apply*` variants must therefore NOT appear here.
        assert!(
            !transitions_a.iter().any(|t| matches!(
                t,
                StateTransition::ApplyConfidentialTransfer { .. }
                    | StateTransition::ApplyShieldedDeposit { .. }
                    | StateTransition::ApplyShieldedWithdrawal { .. }
                    | StateTransition::ApplyRewardClaim { .. }
            )),
            "privacy/reward-claim tx types must be governance-routed, not \
             emitted as direct Apply* transitions (#1731)"
        );

        // Data-only types must NOT contribute any transitions of their own.
        // (They may appear alongside governance finalization transitions which
        // come from state.governance_transitions_for_block, not from the tx
        // match arm itself.)  We verify by checking the known transition
        // counts add up to the total.
        let typed_count = register_commit_count
            + apply_reveal_count
            + oracle_count
            + transitions_a
                .iter()
                .filter(|t| {
                    matches!(
                        t,
                        StateTransition::ApplyReward { .. }
                            | StateTransition::RecordGenesisEmissionConfig { .. }
                            | StateTransition::ApplyFaucetReward
                            | StateTransition::ApplyProducerSettlement { .. }
                            | StateTransition::BalanceTransfer { .. }
                            | StateTransition::LockToStake { .. }
                            | StateTransition::ReleaseToSpendable { .. }
                            | StateTransition::ApplyConfidentialTransfer { .. }
                            | StateTransition::ApplyShieldedDeposit { .. }
                            | StateTransition::ApplyShieldedWithdrawal { .. }
                            | StateTransition::ApplyRewardClaim { .. }
                            | StateTransition::ProcessGovernanceTransaction { .. }
                            | StateTransition::FinalizeGovernanceProposal { .. }
                    )
                })
                .count();
        assert_eq!(
            typed_count,
            transitions_a.len(),
            "No unexpected transition types; data-only variants contribute nothing"
        );

        // --- Second run (determinism) ---
        let transitions_b = extract_transitions(&state, &block, 5, 5);
        assert_eq!(
            transitions_a.len(),
            transitions_b.len(),
            "extract_transitions must be deterministic (same length on re-run)"
        );
    }

    // ------------------------------------------------------------------------
    // #1848 regression: unauthenticated commit must not produce a RegisterCommit
    // transition; only a properly signed commit may do so.
    // ------------------------------------------------------------------------
    #[test]
    fn unsigned_commit_produces_no_transition_signed_commit_does() {
        let state = State::new();

        let build_commit_tx = |data: Option<String>| {
            let mut tx = Transaction::new(vec![], vec![]);
            tx.tx_type = TransactionType::ContributionCommit;
            tx.data = data;
            tx.set_id();
            tx
        };

        // 1. No auth fields → no transition.
        let unsigned_payload = serde_json::to_string(&CommitPayload {
            commit_id: "c-unsigned".to_string(),
            station_id: "WX-01".to_string(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".to_string(),
            auth_pubkey: None,
            auth_signature: None,
        }).unwrap();
        let unsigned_tx = build_commit_tx(Some(unsigned_payload));
        let block = make_block(vec![unsigned_tx], 1);
        let transitions = extract_transitions(&state, &block, 1, 1);
        let commit_count = transitions.iter()
            .filter(|t| matches!(t, StateTransition::RegisterCommit { .. }))
            .count();
        assert_eq!(commit_count, 0, "unsigned commit must not produce RegisterCommit");

        // 2. Properly signed → transition produced.
        let mut rng = StdRng::seed_from_u64(0x1848);
        let private_key = PrivateKey::generate(&mut rng);
        let mut signed_payload = CommitPayload {
            commit_id: "c-signed".to_string(),
            station_id: "WX-01".to_string(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".to_string(),
            auth_pubkey: None,
            auth_signature: None,
        };
        signed_payload.sign_authorization(&private_key);
        let signed_tx = build_commit_tx(Some(serde_json::to_string(&signed_payload).unwrap()));
        let block = make_block(vec![signed_tx], 2);
        let transitions = extract_transitions(&state, &block, 2, 2);
        let commit_count = transitions.iter()
            .filter(|t| matches!(t, StateTransition::RegisterCommit { .. }))
            .count();
        assert_eq!(commit_count, 1, "signed commit must produce exactly 1 RegisterCommit");
    }

    // ------------------------------------------------------------------------
    // #1393 Blocker 1 — block-apply integration test.
    //
    // Verifies that a block containing a `SlashingReport` transaction, fed
    // through the real block-apply path (`extract_transitions` →
    // `apply_transition`), actually activates the on-chain slashing: debit
    // lands, `slashing_evidence_seen` is populated, and a replay tx emitted
    // by the same block is rejected.
    //
    // The earlier `shared-protocol` unit tests (in `state.rs`) call
    // `apply_protocol_transaction` directly and therefore cannot detect a
    // broken extractor arm. This test intentionally does not reach into the
    // apply function — it can only pass if the extractor emits a transition
    // that feeds back into `apply_protocol_transaction`.
    // ------------------------------------------------------------------------
    fn signed_slashing_report_tx(
        sender: Address,
        nonce: u64,
        reporter_sk: &PrivateKey,
        reporter_pk: &PublicKey,
        validator_hex: &str,
        evidence_hash: [u8; 32],
    ) -> Transaction {
        use wrfcoin_native_chain::security::slashing::{
            SlashingEvidenceType, SlashingReportPayload, expected_penalty_for_evidence_type,
        };
        let evidence_type = SlashingEvidenceType::DoubleSign;
        let mut payload = SlashingReportPayload {
            validator_id: validator_hex.to_string(),
            evidence_type,
            evidence_hash,
            reported_at_height: 42,
            penalty_amount: expected_penalty_for_evidence_type(evidence_type),
            reporter_pubkey: reporter_pk.to_hex(),
            reporter_signature: None,
        };
        let message = payload.canonical_signing_message();
        payload.reporter_signature = Some(reporter_sk.sign(message.as_bytes()).to_hex());
        let mut tx = Transaction::new(vec![], vec![]);
        tx.tx_type = TransactionType::SlashingReport;
        tx.sender_address = sender;
        tx.nonce = nonce;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();
        tx
    }

    #[test]
    fn extract_transitions_wires_slashing_report_through_block_apply() {
        // Stage a validator with locked stake and a *staked* reporter.
        // The round-4 P0 fix requires the reporter to have `stake_locked
        // > 0` so the authentication gate passes.
        let mut state = State::new();
        let validator = addr(0x91);
        let initial_stake = 1_000u128;
        state.create_account(validator, Balance::new(initial_stake));
        state
            .apply_transition(StateTransition::LockToStake {
                address: validator,
                amount: Balance::new(initial_stake),
            })
            .expect("lock stake");

        let (reporter_sk, reporter_pk, reporter_addr) = governance_voter(9393);
        state.create_account(reporter_addr, Balance::new(100));
        state
            .apply_transition(StateTransition::LockToStake {
                address: reporter_addr,
                amount: Balance::new(100),
            })
            .expect("stake reporter");

        // Build a block with a single SlashingReport tx and walk it
        // through the same path the mining route / p2p replay uses.
        let evidence_hash = [0xD5; 32];
        let tx = signed_slashing_report_tx(
            reporter_addr,
            0,
            &reporter_sk,
            &reporter_pk,
            &validator.to_hex(),
            evidence_hash,
        );
        let block = make_block(vec![tx], 1);
        let transitions = extract_transitions(&state, &block, 1, 1);

        // The extractor must emit a ProcessSlashingReport transition — the
        // whole point of Blocker 1's fix. Without this, we'd fall through
        // to the catch-all no-op arm and the apply below would be a nop.
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ProcessSlashingReport { current_block, .. }
                    if *current_block == 1
            )),
            "extractor must emit ProcessSlashingReport for TransactionType::SlashingReport"
        );

        for t in transitions {
            state
                .apply_transition(t)
                .expect("block-apply walk must succeed");
        }

        // Debit landed through StateTransition::Slash dispatched from
        // apply_protocol_transaction. The canonical DoubleSign penalty
        // at oracle_count=1 is 25 base units (mirrored in state.rs
        // TEST_CANONICAL_DOUBLE_SIGN_PENALTY).
        let canonical_penalty = 25u128;
        let acc = state.get_account(&validator).expect("validator exists");
        assert_eq!(
            acc.stake_locked,
            Balance::new(initial_stake - canonical_penalty),
            "stake_locked must drop by canonical penalty after block-apply slash"
        );
        assert_eq!(
            acc.balance,
            Balance::new(initial_stake - canonical_penalty),
            "total balance must drop by the canonical penalty"
        );

        // slashing_evidence_seen leaf populated — this is what the v7
        // state-root bump reads. Without the extractor fix this set stays
        // empty on the consensus path, so the v7 root is invariant even
        // under valid slashes.
        assert!(
            state.slashing_evidence_seen.contains(&evidence_hash),
            "evidence hash must be recorded in slashing_evidence_seen"
        );

        // Replay inside a subsequent block with a fresh nonce must be
        // rejected by the dedup guard, not by nonce bookkeeping.
        let tx2 = signed_slashing_report_tx(
            reporter_addr,
            1,
            &reporter_sk,
            &reporter_pk,
            &validator.to_hex(),
            evidence_hash,
        );
        let replay_block = make_block(vec![tx2], 2);
        let replay_transitions = extract_transitions(&state, &replay_block, 2, 2);

        let stake_before_replay = state.get_account(&validator).unwrap().stake_locked;
        let mut replay_err: Option<String> = None;
        for t in replay_transitions {
            if let Err(err) = state.apply_transition(t) {
                replay_err = Some(err.to_string());
            }
        }
        assert!(
            replay_err
                .as_deref()
                .is_some_and(|msg| msg.contains("SP_SLASHING_REPORT_REPLAY")),
            "replayed SlashingReport must be rejected with SP_SLASHING_REPORT_REPLAY, got {replay_err:?}"
        );
        assert_eq!(
            state.get_account(&validator).unwrap().stake_locked,
            stake_before_replay,
            "rejected replay must not debit additional stake"
        );
    }

    /// #2742 at the applier surface: below the ownership-binding activation
    /// height an unbound registration still applies (the block-674 records
    /// must replay byte-identically), at the height it emits NO transition.
    /// The predicate itself is covered in
    /// `station_registration_production_path_2537.rs`; this pins that the
    /// extractor actually passes the block height through.
    #[test]
    fn station_registration_binding_gates_the_extractor_by_height() {
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        let h = wrfcoin_shared_protocol::consensus::proof_of_publishing::STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT;

        // The mod's fixed-seed governance helper: every test writes the SAME
        // allowlist value and none removes it, which is what makes the shared
        // env var race-free across parallel tests. Do not set/remove a
        // different value here.
        let (_env_guard, gov) = oracle_governance_test_key();
        let mut rng = rand::rngs::StdRng::seed_from_u64(27_423);
        let owner = wrfcoin_native_chain::crypto::signature::PrivateKey::generate(&mut rng);

        // signed_station_payload hardcodes owner_address "0xstation-owner" —
        // NOT derived from `owner`, i.e. unbound by construction: exactly the
        // legacy free-form shape #2742 ends.
        let payload = signed_station_payload(
            "PWS-BINDING-GATE",
            ContributionProvenanceClass::Real,
            &owner,
            &gov,
        );
        let tx = station_registration_tx(&payload);

        let state = State::new();
        let below = extract_transitions(&state, &make_block(vec![tx.clone()], h - 1), h - 1, 1);
        assert!(
            below
                .iter()
                .any(|t| matches!(t, StateTransition::ApplyStationRegistration { .. })),
            "below activation the legacy rule must still apply the registration"
        );

        let at = extract_transitions(&state, &make_block(vec![tx], h), h, 1);
        assert!(
            !at.iter()
                .any(|t| matches!(t, StateTransition::ApplyStationRegistration { .. })),
            "at activation an unbound owner_address must emit NO transition"
        );
    }

    /// Malformed settlement metadata DEFERS the payment; it cannot strand it
    /// (#2846 review 2).
    ///
    /// The real applier surface half of the strand fix. A producer that included
    /// the threshold-crossing votes but corrupted the settlement key used to
    /// destroy that payment permanently, because the crossing block was the only
    /// block that could ever pay it. Now the votes still commit, the corrupt key
    /// still yields no settlement transition — and the debt the apply path
    /// recorded when quorum was crossed survives for a later block to pay.
    #[test]
    fn malformed_settlement_metadata_defers_the_debt_on_the_applier_path_2846() {
        use wrfcoin_native_chain::blockchain::transaction::OracleAttestationPayload;
        use wrfcoin_native_chain::consensus::proof_of_publishing::EmissionRole;

        const HEIGHT: u64 = 11;
        let commit = "decision-malformed-metadata";

        let lanes: Vec<(PrivateKey, String)> = (0..2u64)
            .map(|i| {
                let mut rng = StdRng::seed_from_u64(2846_1100 + i);
                let secret = PrivateKey::generate(&mut rng);
                let id = PublicKey::from_private_key(&secret).to_string();
                (secret, id)
            })
            .collect();

        let mut state = State::new();
        state
            .apply_transition(StateTransition::SetOracleConfig {
                oracle_set: lanes.iter().map(|(_, id)| id.clone()).collect(),
                oracle_quorum: 2,
            })
            .expect("oracle config must apply");

        // The block carries the crossing votes as ordinary transactions...
        let mut transactions = vec![Transaction::new_coinbase("wrf-malformed", 0, HEIGHT)];
        for (index, (secret, id)) in lanes.iter().enumerate() {
            let mut payload = OracleAttestationPayload {
                commit_id: commit.to_string(),
                oracle_id: id.clone(),
                approved: true,
                signature: None,
                confidence: Some(1.0),
                timestamp: 1_700_000_000,
                nonce: Some(format!("nonce-malformed-{index}")),
            };
            payload.signature =
                Some(secret.sign(payload.signing_message().as_bytes()).to_string());
            let mut tx = Transaction::new(Vec::new(), Vec::new());
            tx.tx_type = TransactionType::OracleAttestation;
            tx.data =
                Some(serde_json::to_string(&payload).expect("payload serializes"));
            transactions.push(tx);
        }
        let mut block = make_block(transactions, HEIGHT);
        // ...and a settlement key that is not valid JSON for the record type.
        block.metadata.insert(
            ATTESTER_SETTLEMENTS_METADATA_KEY.to_string(),
            "{not-a-settlement-array".to_string(),
        );

        let transitions = extract_transitions(&state, &block, HEIGHT, 0);
        assert!(
            !transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyAttesterSettlements { .. }
            )),
            "malformed metadata must yield no settlement transition"
        );

        state
            .apply_transitions_batch_at_height(HEIGHT, transitions)
            .expect("a malformed optional key must not halt the follower");

        // The votes committed, so the payment is OWED.
        assert!(
            state.attester_settlement_due.contains_key(commit),
            "the crossing votes committed, so the payment must be owed — this is \
             the assertion that fails if the permanent strand ever returns"
        );
        assert!(!state.attester_settlements.contains_key(commit));
        assert_eq!(state.emission_ledger.spent(EmissionRole::Attester), 0);

        // A later honest block settles it: deferred, not lost.
        state.open_emission_block(HEIGHT + 1);
        let slice = state
            .emission_ledger
            .partition()
            .slice(EmissionRole::Attester);
        let owed: Vec<String> = state.attester_settlement_due.keys().cloned().collect();
        let records = state
            .derive_attester_settlements(&owed, HEIGHT + 1, slice)
            .expect("the outstanding debt must settle");
        state
            .apply_transition(StateTransition::ApplyAttesterSettlements { records })
            .expect("the deferred settlement must apply");
        assert!(state.attester_settlements.contains_key(commit));
        assert!(state.attester_settlement_due.is_empty());
        assert_eq!(state.emission_ledger.spent(EmissionRole::Attester), slice);
    }

    /// ADR-003 obligation (f), on the REAL applier surface (#2846).
    ///
    /// The shared-protocol supply-bound test proves producer/applier/WAL
    /// agreement over typed transition lists. This one closes the remaining
    /// gap: that `extract_transitions` — the function the P2P applier actually
    /// calls — reconstructs those transitions from a block's transactions and
    /// metadata, and that applying them through
    /// `apply_transitions_batch_at_height` respects every slice.
    ///
    /// #2891 — the answer to "does the applier reproduce
    /// `attester_settlement_due` / `attester_settlements` identically to the
    /// producer?", established over an HONEST settlement on the real
    /// native-chain surfaces: `extract_transitions` (the function the P2P
    /// applier actually calls) and `Storage::load_wal_transitions_range` (the
    /// rows WAL replay actually reads), both compared byte-for-byte against a
    /// producer leg built with `State::derive_attester_settlements` — the one
    /// shared implementation `ProofOfPublishing::stage_attester_settlements`
    /// calls (that fn itself is private to shared-protocol; its real-path
    /// behaviour over settlements is pinned there by the
    /// `attester_settlement_*_2846` scenarios ending in
    /// `assert_producer_applier_wal_parity`).
    ///
    /// This is the settlement-lifecycle parity test the RED #1682 pins demand
    /// before they may be updated: quorum crossed in-block (due written),
    /// settled in the same block (due drained, terminal record written), and
    /// three surfaces required to agree on the state root AND on both fields
    /// explicitly.
    /// #2897 review (Bob, blocker 1): the SEAM test. The parity test below
    /// verifies the producer half and the applier/WAL halves against the
    /// shared derivation — but its producer leg calls
    /// `derive_attester_settlements` directly, so severing the REAL
    /// production path (`create_next_block` → `process_contribution_rewards`
    /// → `stage_attester_settlements`) left it green. This test drives the
    /// actual `create_next_block` sequence end to end: real mempool
    /// admission, real block assembly, real settlement staging into block
    /// metadata and the producer's installed state — and feeds THAT block
    /// into `extract_transitions` and `Storage::load_wal_transitions_range`.
    ///
    /// The acceptance criterion is the disconnect mutation: with the
    /// `stage_attester_settlements` call severed inside
    /// `process_contribution_rewards`, the settlement-metadata guard below
    /// goes RED — the guard is what makes the seam observable, because a
    /// severed producer and a faithful applier still AGREE (both unsettled),
    /// so root equality alone cannot catch it.
    #[tokio::test]
    async fn real_create_next_block_settlement_flows_through_extractor_and_wal_2897() {
        if crate::helpers::test_env::isolated_chain_domain(
            "transition_extractor::tests::real_create_next_block_settlement_flows_through_extractor_and_wal_2897",
            "wrfcoin-main",
            "mainnet",
            "0x2727272727272727272727272727272727272727",
            "none",
        ) {
            return;
        }
        let _env = crate::helpers::test_env::lock();
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use wrfcoin_native_chain::blockchain::chain::Blockchain;
        use wrfcoin_native_chain::blockchain::transaction::OracleAttestationPayload;
        use wrfcoin_native_chain::config::settings::Config;
        use wrfcoin_native_chain::consensus::proof_of_publishing::{Clock, ProofOfPublishing};
        use wrfcoin_native_chain::mempool::pool::Mempool;
        use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};

        struct SeamClock;
        impl Clock for SeamClock {
            fn now(&self) -> chrono::DateTime<chrono::Utc> {
                chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
            }
        }

        let commit = "decision-seam-2897";
        let lanes: Vec<(PrivateKey, String)> = (0..2u64)
            .map(|i| {
                let mut rng = StdRng::seed_from_u64(2897_0100 + i);
                let secret = PrivateKey::generate(&mut rng);
                let id = PublicKey::from_private_key(&secret).to_string();
                (secret, id)
            })
            .collect();

        let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
        pop.declare_no_genesis_oracles();
        pop.set_genesis_faucet_address(addr(39));
        pop.set_clock(Arc::new(SeamClock));
        let chain = Arc::new(RwLock::new(Blockchain::default()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

        {
            let mut chain_guard = chain.write().await;
            chain_guard
                .apply_state_transition(StateTransition::SetOracleConfig {
                    oracle_set: lanes.iter().map(|(_, id)| id.clone()).collect(),
                    oracle_quorum: 2,
                })
                .expect("oracle config must apply");
        }

        // Real mempool admission (#1723: attestation signatures are checked
        // against the signing-key registry SetOracleConfig populated).
        let signing_keys = chain.read().await.state.signing_keys.clone(); // secret-copy-ok: SigningKeyRegistry holds public keys only
        for (index, (secret, id)) in lanes.iter().enumerate() {
            let mut payload = OracleAttestationPayload {
                commit_id: commit.to_string(),
                oracle_id: id.clone(),
                approved: true,
                signature: None,
                confidence: Some(1.0),
                timestamp: 1_700_000_001,
                nonce: Some(format!("nonce-seam-2897-{index}")),
            };
            payload.signature =
                Some(secret.sign(payload.signing_message().as_bytes()).to_string());
            // Structurally complete tx (input + output + id), as real mempool
            // admission requires — the same shape pop's own fixtures build.
            use wrfcoin_native_chain::blockchain::transaction::{
                TransactionInput, TransactionOutput,
            };
            use wrfcoin_native_chain::crypto::hash::hash_string;
            let mut tx = Transaction::new(
                vec![TransactionInput {
                    txid: hash_string(&format!("input-seam-2897-{index}")),
                    vout: 0,
                    signature: None,
                    public_key: None,
                    coinbase_data: None,
                }],
                vec![TransactionOutput {
                    amount: 1,
                    address: format!("wrf-seam-2897-{index}"),
                    script: "OP_TRUE".to_string(),
                    nft_data: None,
                }],
            );
            tx.tx_type = TransactionType::OracleAttestation;
            tx.timestamp =
                chrono::DateTime::from_timestamp(1_700_000_001, 0).expect("valid timestamp");
            tx.data = Some(serde_json::to_string(&payload).expect("payload serializes"));
            tx.set_id();
            mempool
                .write()
                .await
                .add_transaction_with_signing_keys(tx, Some(&signing_keys))
                .expect("attestation must pass real mempool admission");
        }

        let pre_state = chain.read().await.state.clone();

        // THE REAL PRODUCTION SEQUENCE — no shortcut to the derivation.
        let block = pop
            .create_next_block(chain.clone(), mempool.clone(), "wrf-seam-miner")
            .await
            .expect("create_next_block must produce the crossing block");

        // Seam guards — the assertions Bob's disconnect mutation must trip.
        // A severed producer and a faithful applier still AGREE on an
        // unsettled state, so these guards, not root equality, carry the
        // detection.
        assert!(
            block.metadata.contains_key(ATTESTER_SETTLEMENTS_METADATA_KEY),
            "the REAL production sequence must stage settlement metadata into \
             the block (create_next_block -> process_contribution_rewards -> \
             stage_attester_settlements); its absence means the production \
             path is severed (#2897 seam guard)"
        );
        let producer_state = chain.read().await.state.clone();
        assert!(
            producer_state.attester_settlements.contains_key(commit),
            "the produced block must have SETTLED the crossing on the \
             producer's installed state (#2897 seam guard)"
        );
        assert!(
            producer_state.attester_settlement_due.is_empty(),
            "the produced block must drain the settlement debt it created \
             (#2897 seam guard)"
        );

        // APPLIER LEG — the real surface, fed the REAL block.
        let transitions = extract_transitions(&pre_state, &block, block.height, 0);
        assert!(
            transitions
                .iter()
                .any(|t| matches!(t, StateTransition::ApplyAttesterSettlements { .. })),
            "the applier must reconstruct the settlement from the REAL block's \
             metadata"
        );
        let mut applier = pre_state.clone();
        applier
            .apply_transitions_batch_at_height(block.height, transitions.clone())
            .expect("the applier batch from the real block must apply");

        // WAL LEG — the real surface: persist the real block + transitions,
        // replay what load_wal_transitions_range returns.
        let db_path = std::env::temp_dir().join(format!(
            "wrfcoin-seam-2897-{}.redb",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);
        let mut storage =
            Storage::open(&db_path, DEFAULT_CHECKPOINT_INTERVAL).expect("storage opens");
        storage
            .persist_block(block.clone(), transitions.clone(), None)
            .await
            .expect("real block and WAL rows must persist");
        let replayed = storage
            .load_wal_transitions_range(block.height, block.height)
            .expect("WAL rows must load");
        assert_eq!(
            replayed.len(),
            transitions.len(),
            "vacuity guard: the WAL must return every persisted transition"
        );
        let mut wal = pre_state.clone();
        wal.apply_transitions_batch_at_height(
            block.height,
            replayed.into_iter().map(|(_, transition)| transition),
        )
        .expect("the WAL batch from the real block must apply");
        let _ = std::fs::remove_file(&db_path);

        // Byte-compare the three surfaces over the REAL block.
        assert_eq!(
            applier.state_root, producer_state.state_root,
            "#2897: extract_transitions applier diverged from the real producer"
        );
        assert_eq!(
            wal.state_root, producer_state.state_root,
            "#2897: WAL replay diverged from the real producer"
        );
        for (name, surface) in [("applier", &applier), ("wal", &wal)] {
            assert_eq!(
                surface.attester_settlement_due, producer_state.attester_settlement_due,
                "#2897: {name} attester_settlement_due diverged"
            );
            assert_eq!(
                surface.attester_settlements, producer_state.attester_settlements,
                "#2897: {name} attester_settlements diverged"
            );
        }
    }

    #[tokio::test]
    async fn producer_extractor_and_wal_agree_on_attester_settlement_fields_2891() {
        use wrfcoin_native_chain::blockchain::transaction::OracleAttestationPayload;
        use wrfcoin_native_chain::consensus::proof_of_publishing::{
            EmissionPartition, EmissionRole,
        };
        use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};

        const HEIGHT: u64 = 11;
        let commit = "decision-parity-2891";

        let lanes: Vec<(PrivateKey, String)> = (0..2u64)
            .map(|i| {
                let mut rng = StdRng::seed_from_u64(2891_0100 + i);
                let secret = PrivateKey::generate(&mut rng);
                let id = PublicKey::from_private_key(&secret).to_string();
                (secret, id)
            })
            .collect();

        let mut seed = State::new();
        seed.apply_transition(StateTransition::SetOracleConfig {
            oracle_set: lanes.iter().map(|(_, id)| id.clone()).collect(),
            oracle_quorum: 2,
        })
        .expect("oracle config must apply");

        // The block carries the crossing votes as ordinary transactions.
        let mut transactions = vec![Transaction::new_coinbase("wrf-parity-2891", 0, HEIGHT)];
        for (index, (secret, id)) in lanes.iter().enumerate() {
            let mut payload = OracleAttestationPayload {
                commit_id: commit.to_string(),
                oracle_id: id.clone(),
                approved: true,
                signature: None,
                confidence: Some(1.0),
                timestamp: 1_700_000_000,
                nonce: Some(format!("nonce-parity-2891-{index}")),
            };
            payload.signature =
                Some(secret.sign(payload.signing_message().as_bytes()).to_string());
            let mut tx = Transaction::new(Vec::new(), Vec::new());
            tx.tx_type = TransactionType::OracleAttestation;
            tx.data = Some(serde_json::to_string(&payload).expect("payload serializes"));
            transactions.push(tx);
        }
        let mut block = make_block(transactions, HEIGHT);

        // PRODUCER LEG. Mirror the production sequence: apply the block's
        // tx-derived transitions, observe the quorum crossing write the debt,
        // then derive-and-apply the settlement with the shared implementation
        // and serialize the records into block metadata exactly as
        // `stage_attester_settlements` does.
        let tx_transitions = extract_transitions(&seed, &block, HEIGHT, 0);
        let mut producer = seed.clone();
        producer
            .apply_transitions_batch_at_height(HEIGHT, tx_transitions.clone())
            .expect("attestation transitions must apply on the producer");
        assert!(
            producer.attester_settlement_due.contains_key(commit),
            "vacuity guard: the quorum crossing must write the settlement debt \
             — without this the test would compare three empty legs"
        );
        let owed: Vec<String> = producer.attester_settlement_due.keys().cloned().collect();
        let slice = EmissionPartition::at_height(HEIGHT, None).slice(EmissionRole::Attester);
        assert!(slice > 0, "vacuity guard: the fixture height must emit");
        let records = producer
            .derive_attester_settlements(&owed, HEIGHT, slice)
            .expect("the outstanding debt must derive");
        assert!(!records.is_empty(), "vacuity guard: a settlement must exist");
        block.metadata.insert(
            ATTESTER_SETTLEMENTS_METADATA_KEY.to_string(),
            serde_json::to_string(&records).expect("records serialize"),
        );
        producer
            .apply_transition(StateTransition::ApplyAttesterSettlements {
                records: records.clone(),
            })
            .expect("the settlement must apply on the producer");

        // APPLIER LEG — the real surface: reconstruct EVERYTHING (attestation
        // transitions from txs, settlement from metadata) with
        // `extract_transitions` and apply the batch as `apply_p2p_block_at`
        // would.
        let applier_transitions = extract_transitions(&seed, &block, HEIGHT, 0);
        assert!(
            applier_transitions
                .iter()
                .any(|t| matches!(t, StateTransition::ApplyAttesterSettlements { .. })),
            "the applier must reconstruct the settlement from block metadata"
        );
        let mut applier = seed.clone();
        applier
            .apply_transitions_batch_at_height(HEIGHT, applier_transitions.clone())
            .expect("the applier batch must apply");

        // WAL LEG — the real surface: persist the applier's transitions
        // through Storage (WAL v2 JSON rows) and replay what
        // `load_wal_transitions_range` returns.
        let db_path = std::env::temp_dir().join(format!(
            "wrfcoin-parity-2891-{}.redb",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);
        let mut storage =
            Storage::open(&db_path, DEFAULT_CHECKPOINT_INTERVAL).expect("storage opens");
        storage
            .persist_block(block.clone(), applier_transitions.clone(), None)
            .await
            .expect("block and WAL rows must persist");
        let replayed = storage
            .load_wal_transitions_range(HEIGHT, HEIGHT)
            .expect("WAL rows must load");
        assert_eq!(
            replayed.len(),
            applier_transitions.len(),
            "vacuity guard: the WAL must return every persisted transition"
        );
        let mut wal = seed.clone();
        wal.apply_transitions_batch_at_height(
            HEIGHT,
            replayed.into_iter().map(|(_, transition)| transition),
        )
        .expect("the WAL batch must apply");
        let _ = std::fs::remove_file(&db_path);

        // THE ANSWER. Roots first (they fold both fields via
        // state_root_material), then the two fields explicitly so a future
        // change that removes them from the material cannot silently weaken
        // this test into root-only agreement.
        assert_eq!(
            applier.state_root, producer.state_root,
            "#2891: extract_transitions applier diverged from the producer"
        );
        assert_eq!(
            wal.state_root, producer.state_root,
            "#2891: WAL replay diverged from the producer"
        );
        for (name, surface) in [("applier", &applier), ("wal", &wal)] {
            assert_eq!(
                surface.attester_settlement_due, producer.attester_settlement_due,
                "#2891: {name} attester_settlement_due diverged"
            );
            assert_eq!(
                surface.attester_settlements, producer.attester_settlements,
                "#2891: {name} attester_settlements diverged"
            );
        }
        // And the lifecycle genuinely completed: debt drained, terminal
        // record written, on every surface.
        assert!(producer.attester_settlement_due.is_empty());
        assert!(producer.attester_settlements.contains_key(commit));
    }

    /// The block is deliberately DISHONEST: the coinbase and gamification payout
    /// each claim a full slice, and the settlement metadata claims the whole
    /// block emission for an attester on a decision that never reached quorum.
    /// A producer cannot make a follower pay it, on two independent grounds —
    /// the settlement is not derivable from the follower's own state (#2852), and
    /// the partition ledger, not the metadata, is what authorizes emission.
    #[test]
    fn extract_transitions_respects_the_adr003_partition_on_the_applier_path_2846() {
        use wrfcoin_native_chain::consensus::proof_of_publishing::{
            EmissionPartition, EmissionRole, adjusted_block_reward,
        };
        use wrfcoin_native_chain::oracle::attestation::{
            AttesterAllocation, AttesterSettlementRecord,
        };

        const HEIGHT: u64 = 9;
        let partition = EmissionPartition::at_height(HEIGHT, None);
        let emission = adjusted_block_reward(HEIGHT) as u128;
        assert!(emission > 0, "the fixture needs a live emission height");

        let miner = Address([0xC1; 20]);
        let claimant = Address([0xC2; 20]);
        let attester = Address([0xC3; 20]);

        let applied = State::new();
        // Coinbase at the allowance the producer would build.
        let mut coinbase = Transaction::new_coinbase(
            &format!("0x{}", hex::encode(miner.0)),
            applied.emission_for_tests().coinbase_allowance(HEIGHT) as u64,
            HEIGHT,
        );
        coinbase.outputs[0].address = format!("0x{}", hex::encode(miner.0));

        let mut block = make_block(vec![coinbase], HEIGHT);

        // A contribution reward that, together with the coinbase, asks for far
        // more than the contribution slice.
        block.metadata.insert(
            REWARD_ENTRIES_METADATA_KEY.to_string(),
            serde_json::to_string(&vec![ContributionRewardRecord {
                address: claimant,
                amount: Balance::new(partition.slice(EmissionRole::Contribution)),
                reason: "contribution:commit-partition-applier".to_string(),
                station_id: "station-partition-applier".to_string(),
                provenance_class: ContributionProvenanceClass::Real,
            }])
            .unwrap(),
        );

        // Settlement metadata claiming the WHOLE block emission for one
        // attester — an inflated record a dishonest producer could write.
        block.metadata.insert(
            ATTESTER_SETTLEMENTS_METADATA_KEY.to_string(),
            serde_json::to_string(&vec![AttesterSettlementRecord::new(
                "decision-partition-applier".to_string(),
                HEIGHT,
                vec![AttesterAllocation {
                    attester_id: format!("0x{}", hex::encode(attester.0)),
                    amount: emission,
                }],
            )])
            .unwrap(),
        );

        let mut state = State::new();
        let mut rng = StdRng::seed_from_u64(2846_3039);
        let secret = PrivateKey::generate(&mut rng);
        block.producer_pubkey = PublicKey::from_private_key(&secret).as_bytes().try_into().unwrap();
        state.seed_genesis_producer_set(vec![wrfcoin_shared_protocol::consensus::producer_set::ProducerMember {
            producer_id: "partition-producer".into(), payout_address: miner.to_string(),
            public_key: block.producer_pubkey,
        }]).unwrap();
        let mut budget = state.clone();
        budget.open_emission_block(HEIGHT);
        let record = budget.derive_producer_settlement(HEIGHT, &block.producer_pubkey).unwrap();
        block.metadata.insert(
            wrfcoin_shared_protocol::consensus::proof_of_publishing::PRODUCER_SETTLEMENT_METADATA_KEY.into(),
            serde_json::to_string(&record).unwrap(),
        );
        let transitions = extract_transitions(&state, &block, HEIGHT, 0);
        assert!(
            transitions.iter().any(|t| matches!(
                t,
                StateTransition::ApplyAttesterSettlements { .. }
            )),
            "the applier must reconstruct the settlement from block metadata"
        );

        // The inflated settlement is now REJECTED by the derive-and-compare in
        // `ApplyAttesterSettlements`, which fails the whole batch. That is the
        // #2852 fix working: a follower does not quietly drop a forged payment
        // and carry on, it refuses the block that carried it. Applying the batch
        // WITHOUT the settlement is what a block from an honest producer looks
        // like, and it is what the slice assertions below are about.
        let mut rejected = state.clone();
        assert!(
            rejected
                .apply_transitions_batch_at_height(HEIGHT, transitions.clone())
                .is_err(),
            "a settlement for an invented decision must be rejected on the \
             applier path — metadata is a claim, not an authorization"
        );
        assert!(
            rejected.attester_settlements.is_empty(),
            "a rejected settlement must not consume its decision id"
        );

        let honest: Vec<StateTransition> = transitions
            .into_iter()
            .filter(|t| !matches!(t, StateTransition::ApplyAttesterSettlements { .. }))
            .collect();
        let mut applied = state.clone();
        applied
            .apply_transitions_batch_at_height(HEIGHT, honest)
            .expect("the applier batch must not fail the block");

        let minted = applied.total_minted_supply();
        assert!(
            minted <= emission,
            "the applier minted {minted} > E(t)={emission} — block metadata \
             cannot be allowed to authorize emission"
        );
        for role in [
            EmissionRole::Contribution,
            EmissionRole::Attester,
            EmissionRole::Producer,
            EmissionRole::Node,
        ] {
            assert!(
                applied.emission_ledger.spent(role) <= partition.slice(role),
                "{role:?}: applier spent {} > slice {}",
                applied.emission_ledger.spent(role),
                partition.slice(role)
            );
        }
        // No attester emission at all: the forged settlement never applied.
        assert_eq!(
            applied.emission_ledger.spent(EmissionRole::Attester),
            0,
            "an unearned settlement record must mint nothing"
        );
        assert!(
            !applied
                .attester_settlements
                .contains_key("decision-partition-applier"),
            "a refused settlement must not be marked settled"
        );
        // Phase B pays the authorized producer; Node remains unclaimed.
        assert_eq!(applied.emission_ledger.spent(EmissionRole::Producer), partition.slice(EmissionRole::Producer));
        assert_eq!(applied.emission_ledger.spent(EmissionRole::Node), 0);
        // The coinbase itself did land, on the contribution slice.
        assert_eq!(
            applied
                .get_account(&miner)
                .map(|account| account.balance.into_inner()),
            Some(applied.emission_for_tests().coinbase_allowance(HEIGHT) + partition.slice(EmissionRole::Producer)),
            "the coinbase the producer built must apply on the applier too"
        );
    }
}
