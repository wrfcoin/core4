//! Extended supply-bound tests for ADR-003 Phase A and ADR-004 Phase B (#2846).
//!
//! Run from the core4 workspace root:
//!   cargo test -p wrfcoin-shared-protocol --test emission_partition_supply_bound_2846
//!
//! # What this proves, and why the helper identity was not enough
//!
//! The design review of 2026-08-30 was explicit that the supply-bound evidence
//! must prove more than "the partition helper sums to `E(t)`". That identity is
//! a property of a pure function; it says nothing about whether the CHAIN
//! respects it, because before Phase A the chain had two per-block ceilings that
//! could not see each other and a third mint path with no per-block ceiling at
//! all. A helper can be perfect while the chain over-mints.
//!
//! So each test below binds a REALIZED mint — a `StateTransition` applied
//! through the same sink the producer, the P2P applier and WAL replay all use —
//! rather than an arithmetic identity. The six obligations:
//!
//! | | obligation |
//! |---|---|
//! | (a) | the slices sum to `E(t)` exactly, per block |
//! | (b) | each realized role mint is at most its slice |
//! | (c) | total realized mints — coinbase, gamification, contribution and attester together — are at most `E(t)` |
//! | (d) | unclaimed slices mint zero |
//! | (e) | all of the above hold across a reduction boundary and at `E(t) = 0` |
//! | (f) | producer, P2P applier and WAL replay agree |
//!
//! The ADR-002 cumulative supply bound remains the outer invariant and is
//! unchanged by any of this; the partition bounds ONE block.
//!
//! # Which slice each Phase A path draws
//!
//! ADR-003 "One partition ledger authorizes every mint": the coinbase,
//! gamification mints and contribution rewards all draw the **contribution**
//! slice; attester settlements draw the **attester** slice. The **producer**
//! and **node** slices remain unclaimed in fixtures without a producer roster
//! or node receipts. Those zero-mint controls test isolation, not Phase B
//! payment coverage. The four-role Phase B composition vector seeds a roster
//! and pays the producer at every real bound height, including genesis. A
//! separate explicit E=75 partition vector checks genesis-height settlement.

use wrfcoin_shared_protocol::blockchain::state::{
    ContributionRewardRecord, State, StateTransition,
};
use wrfcoin_shared_protocol::blockchain::transaction::OracleAttestationPayload;
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    EMISSION_ROLE_ORDER, EmissionPartition, EmissionRole, MAX_SUPPLY, adjusted_block_reward,
    coinbase_allowance, emission_periods_at_height,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::oracle::attestation::{
    AttesterAllocation, AttesterSettlementRecord,
};
use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

use rand::SeedableRng;
use rand::rngs::StdRng;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

/// The first height at which the emission schedule has performed `periods`
/// reductions — i.e. a reduction BOUNDARY.
///
/// Binary searched rather than hard-coded, because a hard-coded boundary is a
/// fixture that silently stops being a boundary the moment the block-time table
/// or the reduction period changes, and the test then proves nothing about
/// reduction boundaries while still passing. `emission_periods_at_height` is
/// non-decreasing in height, so the search is sound.
fn first_height_with_periods(periods: u64) -> u64 {
    let (mut low, mut high) = (0u64, u64::MAX);
    while low < high {
        let mid = low + (high - low) / 2;
        if emission_periods_at_height(mid) >= periods {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

/// The first height at which the schedule has terminated — `E(t) == 0`.
///
/// Also searched, for the same reason: `base_block_reward` is non-increasing in
/// height and stays at zero once it reaches it, so "is zero" is a monotone
/// predicate and the first such height is well defined.
fn first_height_with_zero_emission() -> u64 {
    let (mut low, mut high) = (0u64, u64::MAX);
    while low < high {
        let mid = low + (high - low) / 2;
        if adjusted_block_reward(mid) == 0 {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

/// A state whose emission ledger is open at `height`, with enough supply
/// headroom that `MAX_SUPPLY` never becomes the binding constraint. This test
/// is about the PARTITION bound; the cumulative cap has its own tests.
fn state_at(height: u64) -> State {
    let mut state = State::new();
    state.open_emission_block(height);
    state
}

/// A state at `height` carrying a REAL, quorum-attested, unsettled decision.
///
/// Every attester-positive vector below must establish genuine authorization
/// before it asserts anything about bounds. Since the #2852 review, a settlement
/// is only payable when the decision has live oracle quorum in state and the
/// payees are the identities that actually attested it — so a fixture that
/// hand-builds a record for an invented decision no longer measures the bound,
/// it measures the rejection. Returns the state and the two attester identities.
fn state_with_attested_decision(height: u64, decision_id: &str, seed: u64) -> (State, Vec<String>) {
    attest_decision(State::new(), height, decision_id, seed)
}

fn attest_decision(mut state: State, height: u64, decision_id: &str, seed: u64) -> (State, Vec<String>) {
    let lanes: Vec<(PrivateKey, String)> = (0..2)
        .map(|index| {
            let mut rng = StdRng::seed_from_u64(seed + index);
            let secret = PrivateKey::generate(&mut rng);
            let id = PublicKey::from_private_key(&secret).to_string();
            (secret, id)
        })
        .collect();
    state
        .apply_transition(StateTransition::SetOracleConfig {
            oracle_set: lanes.iter().map(|(_, id)| id.clone()).collect(),
            oracle_quorum: 2,
        })
        .expect("oracle configuration must apply");
    for (index, (secret, id)) in lanes.iter().enumerate() {
        state
            .apply_transition(StateTransition::RecordOracleAttestation {
                payload: signed_attestation(decision_id, secret, id, &format!("{seed}-{index}")),
                current_block: height,
            })
            .expect("attestation must apply");
    }
    state.open_emission_block(height);
    (state, lanes.into_iter().map(|(_, id)| id).collect())
}

/// The settlement the CHAIN derives for `decision_id` — the only settlement it
/// will accept. Tests build their transitions from this rather than from
/// hand-written records, so they exercise the authorized path by construction.
fn authorized_settlement(
    state: &State,
    decision_id: &str,
    height: u64,
    slice: u128,
) -> Vec<AttesterSettlementRecord> {
    state
        .derive_attester_settlements(&[decision_id.to_string()], height, slice)
        .expect("the fixture's decision must be settleable")
}

/// Every height this file exercises: genesis, the live tier boundaries, the
/// first two reduction boundaries and their neighbours, and the terminal region.
fn bound_heights() -> Vec<u64> {
    let first_reduction = first_height_with_periods(1);
    let second_reduction = first_height_with_periods(2);
    let zero = first_height_with_zero_emission();
    let mut heights = vec![
        0,
        1,
        1_000,
        100_000,
        first_reduction.saturating_sub(1),
        first_reduction,
        first_reduction + 1,
        second_reduction.saturating_sub(1),
        second_reduction,
        second_reduction + 1,
        zero.saturating_sub(1),
        zero,
        zero.saturating_add(1),
        u64::MAX,
    ];
    heights.sort_unstable();
    heights.dedup();
    heights
}

fn oracle_identity(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&private_key).to_string();
    (private_key, oracle_id)
}

fn signed_attestation(
    commit_id: &str,
    signing_key: &PrivateKey,
    oracle_id: &str,
    tag: &str,
) -> OracleAttestationPayload {
    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.to_string(),
        oracle_id: oracle_id.to_string(),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp: 1_700_000_000,
        nonce: Some(format!("nonce-{tag}")),
    };
    let signature = signing_key
        .sign(payload.signing_message().as_bytes())
        .to_string();
    payload.signature = Some(signature);
    payload
}

/// Total base units this state has minted, measured from balances rather than
/// from the ledger, so the assertion is about REALIZED emission and cannot be
/// satisfied by a ledger that merely recorded a charge.
fn realized_supply(state: &State) -> u128 {
    state.total_minted_supply()
}

// ---------------------------------------------------------------------------
// (a) The slices sum to E(t) exactly, per block
// ---------------------------------------------------------------------------

#[test]
fn a_slices_sum_to_emission_exactly_at_every_bound_height_2846() {
    for height in bound_heights() {
        let emission = adjusted_block_reward(height) as u128;
        let partition = EmissionPartition::at_height(height, None);
        assert_eq!(
            partition.emission(),
            emission,
            "height {height}: partition is not derived from E(t)"
        );
        assert_eq!(
            partition.total(),
            emission,
            "height {height}: slices sum to {}, not E(t)={emission}",
            partition.total()
        );
        // And the ledger opened at that height carries exactly those slices —
        // the chain-side half of the identity.
        let ledger = state_at(height).emission_ledger.clone();
        assert_eq!(ledger.height(), height);
        for role in EMISSION_ROLE_ORDER {
            assert_eq!(
                ledger.remaining(role),
                partition.slice(role),
                "height {height} {role:?}: a freshly opened ledger must offer the \
                 whole slice"
            );
        }
        assert_eq!(ledger.total_spent(), 0);
    }
}

// ---------------------------------------------------------------------------
// (b) Each realized role mint is at most its slice
// ---------------------------------------------------------------------------

#[test]
fn b_each_realized_role_mint_is_capped_by_its_own_slice_2846() {
    for height in bound_heights() {
        let partition = EmissionPartition::at_height(height, None);

        // --- coinbase: draws the CONTRIBUTION slice (ADR-003), not its own ---
        {
            let mut state = state_at(height);
            let contribution_slice = partition.slice(EmissionRole::Contribution);
            // Ask for the whole block emission, which is what the coinbase used
            // to take. Under ADR-003 only the contribution slice may land.
            state
                .apply_transition(StateTransition::ApplyReward {
                    address: addr(0x11),
                    amount: Balance::new(partition.emission().max(1)),
                    reason: "coinbase".to_string(),
                })
                .expect("an over-slice coinbase must defer, never fail the block");
            let minted = realized_supply(&state);
            assert!(
                minted <= contribution_slice,
                "height {height}: coinbase minted {minted} > contribution slice \
                 {contribution_slice}"
            );
            // Fail-closed means an over-ask mints NOTHING, not a clamped
            // amount: the producer is expected to build the coinbase at
            // `coinbase_allowance`, and silently trimming an inflated one would
            // hide that bug.
            //
            // The ask is only genuinely over-slice while `E(t)` is large enough
            // for 70% of it to round below `E(t)`. Deep in the terminal region
            // `E(t) = 1` partitions to `[1, 0, 0, 0]` — largest-remainder gives
            // the single base unit to contribution — so the whole emission IS
            // the contribution slice and the ask fits. Asserting zero there
            // would be asserting a bug.
            if partition.emission().max(1) > contribution_slice {
                assert_eq!(
                    minted, 0,
                    "height {height}: an over-slice coinbase must mint zero, not \
                     a clamped amount"
                );
            } else {
                assert_eq!(
                    minted,
                    partition.emission().max(1),
                    "height {height}: an ask that fits the slice must mint in full"
                );
            }
            // It draws the CONTRIBUTION slice, so the producer slice is
            // untouched even by an attempted coinbase.
            assert_eq!(
                state.emission_ledger.spent(EmissionRole::Producer),
                0,
                "height {height}: the coinbase must never touch the producer \
                 slice — only a producer settlement can charge it"
            );

            // The exact slice does land.
            let mut exact = state_at(height);
            exact
                .apply_transition(StateTransition::ApplyReward {
                    address: addr(0x11),
                    amount: Balance::new(contribution_slice),
                    reason: "coinbase".to_string(),
                })
                .expect("a coinbase at exactly the slice must apply");
            assert_eq!(realized_supply(&exact), contribution_slice);
            assert_eq!(
                exact.emission_ledger.remaining(EmissionRole::Contribution),
                0
            );
            assert_eq!(exact.emission_ledger.spent(EmissionRole::Producer), 0);

            // And what the producer actually builds fits, with room left over
            // for the contribution rewards that share the slice.
            let allowance = exact.emission_for_tests().coinbase_allowance(height);
            assert!(
                allowance <= contribution_slice,
                "height {height}: the coinbase allowance must fit its slice"
            );
            if contribution_slice > 0 {
                assert!(
                    allowance < contribution_slice,
                    "height {height}: the coinbase must leave room in the slice \
                     for contribution rewards"
                );
            }
        }

        // --- gamification / contribution slice ---
        {
            let contribution_slice = partition.slice(EmissionRole::Contribution);
            let mut state = state_at(height);
            state
                .apply_transition(StateTransition::ApplyGamificationReward {
                    address: addr(0x22),
                    amount: Balance::new(partition.emission().max(1)),
                    claim_id: format!("claim-over-{height}"),
                    current_block: height,
                })
                .expect("an over-slice payout must defer, never fail the block");
            // Same terminal-region caveat as the coinbase above: where the
            // contribution slice is the whole of E(t), the greedy ask fits.
            if partition.emission().max(1) > contribution_slice {
                assert_eq!(
                    realized_supply(&state),
                    0,
                    "height {height}: an over-slice gamification payout must mint \
                     zero"
                );
                // Deferred, NOT consumed: the claim id is not recorded, so the
                // backend may retry it in a later block.
                assert!(
                    !state
                        .gamification_claims_seen
                        .contains(&format!("claim-over-{height}")),
                    "height {height}: a refused payout must stay retryable"
                );
            } else {
                assert_eq!(
                    realized_supply(&state),
                    partition.emission().max(1),
                    "height {height}: a payout that fits the slice must mint in \
                     full"
                );
            }

            let mut exact = state_at(height);
            exact
                .apply_transition(StateTransition::ApplyGamificationReward {
                    address: addr(0x22),
                    amount: Balance::new(contribution_slice),
                    claim_id: format!("claim-exact-{height}"),
                    current_block: height,
                })
                .expect("a payout at exactly the slice must apply");
            assert_eq!(realized_supply(&exact), contribution_slice);
        }

        // --- attester (authorized: a real, quorum-attested decision) ---
        {
            let attester_slice = partition.slice(EmissionRole::Attester);
            let decision = format!("decision-bound-{height}");

            // Over-slice: derive against a slice LARGER than the block's, which
            // is what an inflated producer claim would look like, then apply it
            // against the real ledger. It must mint zero and leave no marker.
            let (mut state, _) = state_with_attested_decision(height, &decision, 91_000u64.wrapping_add(height));
            let inflated = state
                .derive_attester_settlements(
                    &[decision.clone()],
                    height,
                    partition.emission().max(1) + attester_slice,
                )
                .expect("the inflated claim is well-formed; only its size is wrong");
            let outcome = state.apply_transition(
                StateTransition::ApplyAttesterSettlements { records: inflated },
            );
            // REJECTED, not deferred — and that changed with the #2852 fix.
            // An over-slice claim used to be capped by the ledger and quietly
            // dropped; now it fails the derive-and-compare first, because a
            // producer asserting a larger payout than the block's slice supports
            // is asserting something the chain's evidence contradicts, and no
            // later block could make it true. The genuine deferral cases that
            // remain are MAX_SUPPLY and an exhausted slice, neither of which is
            // a claim about what was earned.
            assert!(
                outcome.is_err(),
                "height {height}: an over-slice settlement must be rejected"
            );
            assert_eq!(
                realized_supply(&state),
                0,
                "height {height}: an over-slice settlement must mint zero"
            );
            assert_eq!(
                state.emission_ledger.spent(EmissionRole::Attester),
                0,
                "height {height}: and must consume no slice on its way out"
            );
            // And the terminal marker is NOT written, so the decision stays
            // payable rather than being silently consumed.
            assert!(
                !state.attester_settlements.contains_key(&decision),
                "height {height}: a refused settlement must not be marked settled"
            );

            let (mut exact, _) =
                state_with_attested_decision(height, &decision, 92_000u64.wrapping_add(height));
            let records = authorized_settlement(&exact, &decision, height, attester_slice);
            exact
                .apply_transition(StateTransition::ApplyAttesterSettlements { records })
                .expect("a settlement at exactly the slice must apply");
            assert_eq!(realized_supply(&exact), attester_slice);
        }
    }
}

/// The regression detector for the actual defect.
///
/// Gamification payouts and contribution rewards used to be bounded by two
/// ceilings that could not see each other: gamification was subtracted from the
/// contribution budget, so it bounded contributions while nothing bounded it.
/// They now draw the SAME slice, so a block whose gamification payouts consume
/// the contribution slice has nothing left for contribution rewards — and vice
/// versa. If this test ever passes with the payout minting on top of a full
/// contribution slice, the two ceilings have come apart again.
#[test]
fn b_gamification_and_contribution_rewards_share_one_slice_2846() {
    const HEIGHT: u64 = 12;
    let partition = EmissionPartition::at_height(HEIGHT, None);
    let contribution_slice = partition.slice(EmissionRole::Contribution);
    assert!(contribution_slice > 0, "fixture needs a non-empty slice");

    let (oracle_secret, oracle_id) = oracle_identity(2846_9001);
    let commit_id = "commit-shared-slice";

    let mut state = State::new();
    state
        .apply_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_id.clone()],
            oracle_quorum: 1,
        })
        .expect("oracle configuration must apply");
    state
        .apply_transition(StateTransition::RecordOracleAttestation {
            payload: signed_attestation(commit_id, &oracle_secret, &oracle_id, "shared"),
            current_block: HEIGHT,
        })
        .expect("attestation must apply");
    state.open_emission_block(HEIGHT);

    // The gamification payout takes the entire contribution slice.
    state
        .apply_transition(StateTransition::ApplyGamificationReward {
            address: addr(0x44),
            amount: Balance::new(contribution_slice),
            claim_id: "claim-eats-the-slice".to_string(),
            current_block: HEIGHT,
        })
        .expect("payout must apply");
    assert_eq!(
        state.emission_ledger.remaining(EmissionRole::Contribution),
        0,
        "the payout must consume the whole contribution slice"
    );

    // A quorum-attested contribution reward now has nothing to draw on. It
    // must DEFER — durable record, no terminal marker, no mint — not mint on a
    // second, independent ceiling.
    let record = ContributionRewardRecord {
        address: addr(0x55),
        amount: Balance::new(1),
        reason: format!("contribution:{commit_id}"),
        station_id: "station-shared-slice".to_string(),
        provenance_class: ContributionProvenanceClass::Real,
    };
    state
        .apply_transition(StateTransition::ApplyContributionRewardDecision {
            record: record.clone(),
            current_block: HEIGHT,
        })
        .expect("an unfunded decision must defer, never fail the block");

    assert!(
        state.get_account(&addr(0x55)).is_none(),
        "the contribution reward must not mint from an exhausted slice — the \
         two ceilings have come apart again"
    );
    assert!(
        !state.finalized_contribution_rewards.contains(commit_id),
        "a deferred decision must stay retryable in a later block"
    );
    assert_eq!(
        state.contribution_reward_records.get(commit_id),
        Some(&record),
        "the decision itself must remain durable"
    );
    assert_eq!(
        realized_supply(&state),
        contribution_slice,
        "the block minted exactly the contribution slice, no more"
    );
}

// ---------------------------------------------------------------------------
// (c) Total realized mints, across Phase A paths, are at most E(t)
// ---------------------------------------------------------------------------

#[test]
fn c_total_realized_mints_across_phase_a_paths_never_exceed_emission_2846() {
    for height in bound_heights() {
        let partition = EmissionPartition::at_height(height, None);
        let emission = partition.emission();
        let decision = format!("decision-total-{height}");
        let (mut state, _) = state_with_attested_decision(height, &decision, 93_000u64.wrapping_add(height));

        // Every mint path asks for the WHOLE block emission. Before ADR-003
        // this would have minted roughly three times E(t): the coinbase took
        // E(t), gamification was bounded per-block by nothing, and contribution
        // rewards drew on a separate MAX_BLOCK_EMISSION-derived ceiling.
        let greedy = emission.max(1);

        // The coinbase and the gamification payout SHARE the contribution
        // slice, so between them they can claim it exactly once — which is the
        // whole point of routing them through one ledger.
        let coinbase = state.emission_for_tests().coinbase_allowance(height);
        let payout = partition
            .slice(EmissionRole::Contribution)
            .saturating_sub(coinbase);
        state
            .apply_transition(StateTransition::ApplyReward {
                address: addr(0x61),
                amount: Balance::new(coinbase),
                reason: "coinbase".to_string(),
            })
            .expect("coinbase must apply or defer");
        state
            .apply_transition(StateTransition::ApplyGamificationReward {
                address: addr(0x62),
                amount: Balance::new(payout),
                claim_id: format!("claim-total-{height}"),
                current_block: height,
            })
            .expect("payout must apply or defer");
        let records = authorized_settlement(
            &state,
            &decision,
            height,
            partition.slice(EmissionRole::Attester),
        );
        state
            .apply_transition(StateTransition::ApplyAttesterSettlements { records })
            .expect("settlement must apply or defer");
        // Now every path tries again, greedily, on top of what already landed.
        state
            .apply_transition(StateTransition::ApplyReward {
                address: addr(0x64),
                amount: Balance::new(greedy),
                reason: "coinbase".to_string(),
            })
            .expect("a second coinbase must defer");
        state
            .apply_transition(StateTransition::ApplyGamificationReward {
                address: addr(0x65),
                amount: Balance::new(greedy),
                claim_id: format!("claim-total-second-{height}"),
                current_block: height,
            })
            .expect("a second payout must defer");
        // A second settlement, greedy. It is refused whether or not it names a
        // real decision: the first one already claimed the slice, and this one
        // names a decision that never reached quorum.
        let second = state.apply_transition(StateTransition::ApplyAttesterSettlements {
            records: vec![AttesterSettlementRecord::new(
                format!("decision-total-second-{height}"),
                height,
                vec![AttesterAllocation {
                    attester_id: format!("0x{}", "66".repeat(20)),
                    amount: greedy,
                }],
            )],
        });
        assert!(
            second.is_err(),
            "height {height}: an unearned second settlement must be REJECTED, \
             not merely capped — a supply ceiling is not a mint authorization"
        );

        let minted = realized_supply(&state);
        assert!(
            minted <= emission,
            "height {height}: realized mints {minted} exceed E(t)={emission}"
        );
        assert_eq!(
            state.emission_ledger.total_spent(),
            minted,
            "height {height}: the ledger's spend must equal what was really minted"
        );
        // Per-role, too: no slice was overdrawn on the way to a passing total.
        for role in EMISSION_ROLE_ORDER {
            assert!(
                state.emission_ledger.spent(role) <= partition.slice(role),
                "height {height} {role:?}: spent {} > slice {}",
                state.emission_ledger.spent(role),
                partition.slice(role)
            );
        }
        assert!(minted <= MAX_SUPPLY, "the outer ADR-002 cap still holds");
        // This control has no producer roster and applies no producer settlement
        // or node receipt. Neither slice may be spent by these other mint paths.
        assert_eq!(
            state.emission_ledger.spent(EmissionRole::Producer),
            0,
            "height {height}: without a producer settlement the producer slice must mint zero"
        );
        assert_eq!(
            state.emission_ledger.spent(EmissionRole::Node),
            0,
            "height {height}: the node slice is Phase C and must mint zero"
        );
        // Strictly under E(t) only when the unclaimed slices are actually
        // non-empty. In the terminal region every slice but contribution floors
        // to zero, so "unclaimed" costs nothing and the bound is tight rather
        // than strict — which is the correct behaviour, not a redistribution.
        let unclaimed =
            partition.slice(EmissionRole::Producer) + partition.slice(EmissionRole::Node);
        if unclaimed > 0 {
            assert!(
                minted < partition.emission(),
                "height {height}: with the producer and node slices unclaimed a \
                 block must mint STRICTLY under E(t)"
            );
        }
    }
}

/// **A supply ceiling is not a mint authorization.** (#2852 review)
///
/// This is the negative half of obligation (c), and without it the rest of this
/// file was actively harmful: it asserted that a settlement for an invented
/// decision paying an arbitrary address stayed inside `E(t)` and replayed
/// identically on every surface — which is TRUE, and which blessed the
/// unauthorized path. Proving a theft is well-bounded and deterministic is
/// worse than not testing it, because it reads as a passing security property.
///
/// The record below is deliberately impeccable in every way the first cut
/// checked: canonically sorted, one identity so no duplicates, exactly the
/// attester slice so it is in budget, far under `MAX_SUPPLY`, and it names the
/// block being applied. The only thing wrong with it is that nobody earned it.
#[test]
fn c_a_supply_ceiling_is_not_a_mint_authorization_2846() {
    for height in bound_heights() {
        let partition = EmissionPartition::at_height(height, None);
        let attester_slice = partition.slice(EmissionRole::Attester);
        if attester_slice == 0 {
            // Terminal region: nothing to steal, and the empty-derivation path
            // is covered by the E(t)=0 vector.
            continue;
        }
        let thief_id = format!("0x{}", "de".repeat(20));
        let thief = Address([0xDE; 20]);

        let mut state = state_at(height);
        let forged = AttesterSettlementRecord::new(
            format!("decision-never-happened-{height}"),
            height,
            vec![AttesterAllocation {
                attester_id: thief_id,
                amount: attester_slice,
            }],
        );
        // Every pre-#2852 check passes.
        assert!(forged.is_canonical());
        assert_eq!(forged.total_amount(), attester_slice);
        assert!(forged.total_amount() <= partition.emission());

        let outcome = state.apply_transition(StateTransition::ApplyAttesterSettlements {
            records: vec![forged],
        });

        assert!(
            outcome.is_err(),
            "height {height}: an unearned settlement must be rejected"
        );
        assert_eq!(
            realized_supply(&state),
            0,
            "height {height}: an unearned settlement must mint nothing"
        );
        assert!(
            state.get_account(&thief).is_none(),
            "height {height}: the arbitrary address must not be paid"
        );
        assert!(
            state.attester_settlements.is_empty(),
            "height {height}: a rejected settlement must not be marked — a marked \
             decision id is consumed and can never be settled honestly"
        );
        assert_eq!(
            state.emission_ledger.spent(EmissionRole::Attester),
            0,
            "height {height}: a rejected settlement must not consume slice either"
        );
    }
}

/// The same point one level in: naming a REAL decision is not authorization for
/// an arbitrary payout either (#2852 review).
///
/// A producer that waits for a genuine quorum and then splices itself into that
/// decision's payee list, or skews the split among the attesters who did earn
/// it, is making a claim the chain's own evidence does not support. The skew
/// below keeps the TOTAL identical to the honest settlement, so every
/// slice-and-ceiling assertion in this file would pass it.
#[test]
fn c_a_real_decision_does_not_authorize_an_arbitrary_payout_2846() {
    const HEIGHT: u64 = 6;
    let partition = EmissionPartition::at_height(HEIGHT, None);
    let slice = partition.slice(EmissionRole::Attester);
    let decision = "decision-real-but-tampered";
    let (state, attester_ids) = state_with_attested_decision(HEIGHT, decision, 96_000);

    let honest = authorized_settlement(&state, decision, HEIGHT, slice);
    assert_eq!(honest.len(), 1);
    assert_eq!(honest[0].eligible_commit_count(), 2);
    assert_eq!(honest[0].total_amount(), slice);
    for id in &attester_ids {
        assert!(
            honest[0]
                .allocations
                .iter()
                .any(|allocation| &allocation.attester_id == id),
            "the honest settlement pays exactly the identities that attested"
        );
    }

    // Skew: same payees, same total, wrong division.
    let mut skewed = honest[0].allocations.clone();
    let total: u128 = skewed.iter().map(|a| a.amount).sum();
    skewed[0].amount = total;
    skewed[1].amount = 0;
    let skewed_record = AttesterSettlementRecord::new(decision.to_string(), HEIGHT, skewed);
    assert_eq!(
        skewed_record.total_amount(),
        honest[0].total_amount(),
        "the skew is total-preserving, so a total-only bound would accept it"
    );

    let mut probe = state.clone();
    assert!(
        probe
            .apply_transition(StateTransition::ApplyAttesterSettlements {
                records: vec![skewed_record],
            })
            .is_err(),
        "the rule-5 division is derived from state, not accepted from the producer"
    );
    assert_eq!(probe.emission_ledger.spent(EmissionRole::Attester), 0);
    assert!(probe.attester_settlements.is_empty());

    // And the honest settlement still applies, so the defence is not vacuous.
    let mut good = state;
    good.apply_transition(StateTransition::ApplyAttesterSettlements { records: honest })
        .expect("the derived settlement must apply");
    assert_eq!(realized_supply(&good), slice);
    assert!(good.attester_settlements.contains_key(decision));
}

/// A DEFERRED backlog is still bounded by the block that finally pays it
/// (#2846 review 2).
///
/// The strand fix makes attester pay a durable debt, which introduces a bound
/// obligation this file did not previously have to make: if several decisions
/// go unsettled and pile up, the block that eventually settles them must still
/// respect its OWN attester slice. A debt that accumulated across blocks and
/// then paid out across blocks' worth of emission in one block would be exactly
/// the over-mint the partition exists to prevent — arrived at through the door
/// the fix opened.
///
/// It does not, because the division reads the paying block's remaining slice
/// and divides across ALL owed decisions. The backlog changes how thinly the
/// slice is spread; it never changes how much is emitted.
#[test]
fn c_a_deferred_backlog_is_bounded_by_the_paying_block_2846() {
    const CROSSING_HEIGHT: u64 = 20;
    const PAYING_HEIGHT: u64 = 24;

    // Three decisions cross quorum and NONE is settled — four blocks of
    // attester slice go unclaimed and unemitted.
    let mut state = State::new();
    let lanes: Vec<(PrivateKey, String)> = (0..2u64)
        .map(|index| {
            let mut rng = StdRng::seed_from_u64(97_000 + index);
            let secret = PrivateKey::generate(&mut rng);
            let id = PublicKey::from_private_key(&secret).to_string();
            (secret, id)
        })
        .collect();
    state
        .apply_transition(StateTransition::SetOracleConfig {
            oracle_set: lanes.iter().map(|(_, id)| id.clone()).collect(),
            oracle_quorum: 2,
        })
        .expect("oracle configuration must apply");

    let decisions = ["backlog-a", "backlog-b", "backlog-c"];
    for (d_index, decision) in decisions.iter().enumerate() {
        for (index, (secret, id)) in lanes.iter().enumerate() {
            state
                .apply_transition(StateTransition::RecordOracleAttestation {
                    payload: signed_attestation(
                        decision,
                        secret,
                        id,
                        &format!("backlog-{d_index}-{index}"),
                    ),
                    current_block: CROSSING_HEIGHT + d_index as u64,
                })
                .expect("attestation must apply");
        }
    }
    assert_eq!(
        state.attester_settlement_due.len(),
        3,
        "three crossings, none settled — three debts"
    );
    assert_eq!(
        realized_supply(&state),
        0,
        "unsettled crossings emit nothing at all"
    );

    // The block that finally pays.
    state.open_emission_block(PAYING_HEIGHT);
    let partition = EmissionPartition::at_height(PAYING_HEIGHT, None);
    let slice = partition.slice(EmissionRole::Attester);
    let owed: Vec<String> = state.attester_settlement_due.keys().cloned().collect();
    let records = state
        .derive_attester_settlements(&owed, PAYING_HEIGHT, slice)
        .expect("the backlog must settle");
    assert_eq!(records.len(), 3, "all three settle together");
    assert_eq!(
        records
            .iter()
            .map(AttesterSettlementRecord::total_amount)
            .sum::<u128>(),
        slice,
        "three blocks' worth of debt still pays exactly ONE block's slice"
    );

    state
        .apply_transition(StateTransition::ApplyAttesterSettlements { records })
        .expect("the backlog settlement must apply");

    // The bound obligations, restated for the deferred path.
    assert_eq!(
        realized_supply(&state),
        slice,
        "the paying block emitted its own attester slice and not one unit more"
    );
    assert!(
        realized_supply(&state) <= partition.emission(),
        "and stayed within E(t) of the block that paid"
    );
    assert_eq!(state.emission_ledger.spent(EmissionRole::Attester), slice);
    assert_eq!(
        state.emission_ledger.remaining(EmissionRole::Attester),
        0,
        "the slice is fully claimed, so a second settlement in this block cannot mint"
    );
    assert!(
        state.attester_settlement_due.is_empty(),
        "settling clears the whole debt"
    );
    for decision in decisions {
        assert!(state.attester_settlements.contains_key(decision));
    }
    // The unclaimed slices of the three skipped blocks were NOT carried
    // forward — only the obligation to pay was. Realized issuance runs under
    // ADR-002's curve, never over.
    assert!(
        realized_supply(&state) < partition.emission() * 4,
        "a backlog must not accumulate EMISSION, only the duty to divide one slice"
    );
}

// ---------------------------------------------------------------------------
// (d) Unclaimed slices mint zero
// ---------------------------------------------------------------------------

#[test]
fn d_unclaimed_slices_mint_zero_and_are_never_redistributed_2846() {
    for height in bound_heights() {
        let partition = EmissionPartition::at_height(height, None);
        let mut state = state_at(height);

        // Only the contribution slice is claimed, by the coinbase. Attester,
        // producer and node are not.
        let contribution_slice = partition.slice(EmissionRole::Contribution);
        state
            .apply_transition(StateTransition::ApplyReward {
                address: addr(0x71),
                amount: Balance::new(contribution_slice),
                reason: "coinbase".to_string(),
            })
            .expect("coinbase must apply");

        assert_eq!(
            realized_supply(&state),
            contribution_slice,
            "height {height}: an unclaimed slice must not be emitted"
        );
        for role in [
            EmissionRole::Attester,
            EmissionRole::Producer,
            EmissionRole::Node,
        ] {
            assert_eq!(
                state.emission_ledger.spent(role),
                0,
                "height {height} {role:?}: an unclaimed slice must mint zero"
            );
        }
        // The claimed slice does NOT inherit the unclaimed remainder: realized
        // issuance runs UNDER ADR-002's curve, never over, and never sideways.
        assert!(
            realized_supply(&state) <= partition.emission(),
            "height {height}: realized issuance exceeded E(t)"
        );
        // As in (c): strict only where the unclaimed slices are non-empty. In
        // the terminal region they all floor to zero and the contribution slice
        // is the whole of E(t).
        let unclaimed = partition.slice(EmissionRole::Attester)
            + partition.slice(EmissionRole::Producer)
            + partition.slice(EmissionRole::Node);
        if unclaimed > 0 {
            assert!(
                realized_supply(&state) < partition.emission(),
                "height {height}: with three slices unclaimed the block must \
                 mint STRICTLY under E(t) — a slice was redistributed"
            );
        }
        // This fixture deliberately omits a producer roster and node receipts.
        // Their unclaimed budgets must not be redistributed. The Phase B
        // composition vector below separately proves authorized producer payment.
        assert_eq!(state.emission_ledger.spent(EmissionRole::Producer), 0);
        assert_eq!(state.emission_ledger.spent(EmissionRole::Node), 0);
    }
}

// ---------------------------------------------------------------------------
// (e) Across a reduction boundary, and at E(t) = 0
// ---------------------------------------------------------------------------

#[test]
fn e_bounds_hold_across_a_reduction_boundary_2846() {
    let boundary = first_height_with_periods(1);
    let before = boundary - 1;

    // The vector must actually BE a boundary, or this test proves nothing about
    // reduction boundaries while still passing.
    assert_eq!(emission_periods_at_height(before), 0);
    assert_eq!(emission_periods_at_height(boundary), 1);
    let emission_before = adjusted_block_reward(before) as u128;
    let emission_after = adjusted_block_reward(boundary) as u128;
    assert!(
        emission_after < emission_before,
        "the schedule must actually step down at the boundary: {emission_before} \
         -> {emission_after}"
    );

    // A ledger opened before the boundary offers the OLD slices; re-opening at
    // the boundary re-derives from the NEW E(t). Carrying the old partition
    // across would let the first post-reduction block mint at the pre-reduction
    // rate — the exact shape of over-mint the partition exists to prevent.
    let mut state = state_at(before);
    let old_slice = EmissionPartition::at_height(before, None).slice(EmissionRole::Contribution);
    assert_eq!(
        state.emission_ledger.remaining(EmissionRole::Contribution),
        old_slice
    );
    state
        .apply_transition(StateTransition::ApplyReward {
            address: addr(0x81),
            amount: Balance::new(old_slice),
            reason: "coinbase".to_string(),
        })
        .expect("pre-boundary coinbase must apply");

    state.open_emission_block(boundary);
    let new_slice = EmissionPartition::at_height(boundary, None).slice(EmissionRole::Contribution);
    assert!(new_slice < old_slice, "the slice must step down too");
    assert_eq!(
        state.emission_ledger.remaining(EmissionRole::Contribution),
        new_slice,
        "the post-boundary block must be budgeted from the NEW E(t)"
    );
    assert!(
        state.emission_for_tests().coinbase_allowance(boundary) < state.emission_for_tests().coinbase_allowance(before),
        "and what the producer builds must step down with it"
    );

    // The pre-reduction amount is now over-slice and mints nothing.
    state
        .apply_transition(StateTransition::ApplyReward {
            address: addr(0x82),
            amount: Balance::new(old_slice),
            reason: "coinbase".to_string(),
        })
        .expect("an over-slice coinbase must defer");
    assert!(
        state.get_account(&addr(0x82)).is_none(),
        "a post-reduction block must not mint at the pre-reduction rate"
    );

    // And every obligation still holds at both heights.
    for height in [before, boundary] {
        let partition = EmissionPartition::at_height(height, None);
        assert_eq!(partition.total(), partition.emission());
    }
}

#[test]
fn e_bounds_hold_at_zero_emission_2846() {
    let zero = first_height_with_zero_emission();
    assert_eq!(adjusted_block_reward(zero), 0);
    assert!(
        adjusted_block_reward(zero - 1) > 0,
        "the searched height must be the FIRST zero, or the vector is not the \
         terminal boundary"
    );

    let partition = EmissionPartition::at_height(zero, None);
    assert_eq!(partition.emission(), 0);
    assert_eq!(partition.total(), 0);
    for role in EMISSION_ROLE_ORDER {
        assert_eq!(partition.slice(role), 0, "{role:?} must be zero at E(t)=0");
    }

    let mut state = state_at(zero);
    state
        .apply_transition(StateTransition::ApplyReward {
            address: addr(0x91),
            amount: Balance::new(1),
            reason: "coinbase".to_string(),
        })
        .expect("a terminal-era coinbase must defer, never fail the block");
    state
        .apply_transition(StateTransition::ApplyGamificationReward {
            address: addr(0x92),
            amount: Balance::new(1),
            claim_id: "claim-terminal".to_string(),
            current_block: zero,
        })
        .expect("a terminal-era payout must defer");
    // An authorized settlement in the terminal era: the decision is real, but
    // the slice is zero, so the derivation yields no records at all and nothing
    // is minted or marked.
    let (mut attested, _) = state_with_attested_decision(zero, "decision-terminal", 94_000);
    let derived = attested
        .derive_attester_settlements(&["decision-terminal".to_string()], zero, 0)
        .expect("a real decision derives cleanly even at a zero slice");
    assert!(
        derived.is_empty(),
        "at E(t)=0 there is nothing to divide, so there is no record to apply"
    );
    attested
        .apply_transition(StateTransition::ApplyAttesterSettlements { records: derived })
        .expect("an empty settlement is a no-op");
    assert_eq!(realized_supply(&attested), 0);
    assert!(attested.attester_settlements.is_empty());

    assert_eq!(
        realized_supply(&state),
        0,
        "not one base unit may be minted once the schedule has terminated"
    );
    assert_eq!(state.emission_ledger.total_spent(), 0);
    // All three stay retryable rather than being consumed — though at E(t)=0
    // they will never be payable again, the state must not claim they were.
    assert!(!state.gamification_claims_seen.contains("claim-terminal"));
    assert!(!state.attester_settlements.contains_key("decision-terminal"));
}

// ---------------------------------------------------------------------------
// (f) Producer, P2P applier and WAL replay agree
// ---------------------------------------------------------------------------

/// The three surfaces reach the same state root from the same ordered
/// transition list at the same height.
///
/// The producer applies its batch through `apply_transitions_batch_at_height`;
/// the P2P applier reconstructs the identical list from block metadata and
/// applies it the same way; WAL replay reconstructs it from serialized entries.
/// The WAL leg round-trips every transition through JSON first, because that is
/// what WAL v2 stores (#1103) — a transition that serialized lossily would pass
/// an in-memory comparison and diverge on restart, which is precisely the class
/// of bug this obligation exists to catch.
#[test]
fn f_producer_applier_and_wal_replay_agree_on_every_bound_height_2846() {
    for height in bound_heights() {
        let partition = EmissionPartition::at_height(height, None);
        // All three surfaces start from the SAME authorized pre-state: a real,
        // quorum-attested decision. Since the #2852 review a settlement is only
        // payable against such a decision, so a parity test built on a forged
        // record would prove that three surfaces agree on a rejection — true,
        // but not the obligation.
        let decision = format!("decision-parity-{height}");
        let (seed_state, _) = state_with_attested_decision(height, &decision, 95_000u64.wrapping_add(height));
        let settlement_records = authorized_settlement(
            &seed_state,
            &decision,
            height,
            partition.slice(EmissionRole::Attester),
        );
        let batch: Vec<StateTransition> = vec![
            StateTransition::ApplyReward {
                address: addr(0xA1),
                amount: Balance::new(seed_state.emission_for_tests().coinbase_allowance(height)),
                reason: "coinbase".to_string(),
            },
            StateTransition::ApplyGamificationReward {
                address: addr(0xA2),
                // The remainder of the slice the coinbase just drew from: the
                // two share one budget, so the payout is what is left.
                amount: Balance::new(
                    partition
                        .slice(EmissionRole::Contribution)
                        .saturating_sub(seed_state.emission_for_tests().coinbase_allowance(height)),
                ),
                claim_id: format!("claim-parity-{height}"),
                current_block: height,
            },
            StateTransition::ApplyAttesterSettlements {
                records: settlement_records.clone(),
            },
            // A greedy tail that every surface must refuse identically. A
            // divergence in the REFUSAL is just as forking as one in the mint,
            // and is the easier one to leave untested.
            StateTransition::ApplyReward {
                address: addr(0xA5),
                amount: Balance::new(partition.emission().max(1)),
                reason: "coinbase".to_string(),
            },
        ];

        let mut producer = seed_state.clone();
        producer
            .apply_transitions_batch_at_height(height, batch.clone())
            .expect("producer batch must apply");

        let mut applier = seed_state.clone();
        applier
            .apply_transitions_batch_at_height(height, batch.clone())
            .expect("applier batch must apply");

        let mut wal = seed_state.clone();
        let replayed: Vec<StateTransition> = batch
            .iter()
            .map(|transition| {
                let encoded =
                    serde_json::to_string(transition).expect("WAL entries must serialize");
                serde_json::from_str::<StateTransition>(&encoded)
                    .expect("WAL entries must deserialize")
            })
            .collect();
        wal.apply_transitions_batch_at_height(height, replayed)
            .expect("WAL batch must apply");

        assert_eq!(
            applier.state_root, producer.state_root,
            "height {height}: P2P applier diverged from the producer"
        );
        assert_eq!(
            wal.state_root, producer.state_root,
            "height {height}: WAL replay diverged from the producer"
        );

        // Agreement on the root is necessary but not sufficient evidence for a
        // BOUND: pin the realized total and the per-role spend too, so three
        // surfaces agreeing on an over-mint could not pass.
        for surface in [&producer, &applier, &wal] {
            assert!(
                realized_supply(surface) <= partition.emission(),
                "height {height}: a surface minted past E(t)"
            );
            for role in EMISSION_ROLE_ORDER {
                assert!(surface.emission_ledger.spent(role) <= partition.slice(role));
            }
            assert_eq!(
                surface.emission_ledger.spent(EmissionRole::Producer),
                0,
                "without a producer settlement the producer slice must mint zero on every surface"
            );
            assert_eq!(
                surface.emission_ledger.spent(EmissionRole::Node),
                0,
                "the node slice is unclaimed in Phase A and must mint zero"
            );
        }
        assert_eq!(realized_supply(&applier), realized_supply(&producer));
        assert_eq!(realized_supply(&wal), realized_supply(&producer));
    }
}

/// Phase B composition: authorized producer issuance shares the same aggregate
/// bound with contribution, faucet and attester issuance, including near-cap
/// and terminal-emission controls. Every retry crosses the real apply boundary.
#[test]
fn c_phase_b_four_role_composition_supply_bound_and_remainder_2846() {
    use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
    use wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig;
    for height in bound_heights() {
        for near_cap in [false, true] {
            let partition = EmissionPartition::at_height(height, Some("777"));
            let coinbase = coinbase_allowance(height, Some("777"));
            let headroom = if near_cap {
                coinbase + partition.slice(EmissionRole::Faucet) + 7
            } else { MAX_SUPPLY };
            let initial_supply = MAX_SUPPLY - headroom;
            let mut initial = State::new();
            initial.open_emission_block(1);
            initial.apply_transition(StateTransition::RecordGenesisEmissionConfig {
                config: GenesisEmissionConfig { chain_id: "777".into(), faucet_address: Some(addr(90)) , ..Default::default() },
            }).unwrap();
            let mut rng = StdRng::seed_from_u64(2846_06);
            let secret = PrivateKey::generate(&mut rng);
            let key = PublicKey::from_private_key(&secret).as_bytes().try_into().unwrap();
            initial.seed_genesis_producer_set(vec![ProducerMember {
                producer_id: "composition".into(), public_key: key,
                payout_address: addr(91).to_string(),
            }]).unwrap();
            initial.create_account(addr(99), Balance::new(initial_supply));
            let decision = format!("phase-b-composition-{height}-{near_cap}");
            let (mut state, _) = attest_decision(initial, height, &decision, 96_000u64.wrapping_add(height));
            state.apply_transition(StateTransition::ApplyReward {
                address: addr(92), amount: Balance::new(coinbase), reason: "coinbase".into(),
            }).unwrap();
            state.apply_transition(StateTransition::ApplyFaucetReward).unwrap();
            let expected_producer = partition.slice(EmissionRole::Producer)
                .min(MAX_SUPPLY - state.total_minted_supply());
            let receipt = state.derive_producer_settlement(height, &key);
            assert_eq!(receipt.as_ref().map_or(0, |r| r.amount), expected_producer);
            if let Some(record) = receipt.clone() {
                let mut forged = record.clone();
                forged.amount += 1;
                assert!(state.apply_transition(StateTransition::ApplyProducerSettlement {
                    producer_pubkey: key, record: forged,
                }).is_err());
                let transition = StateTransition::ApplyProducerSettlement { producer_pubkey: key, record };
                state.apply_transition(transition.clone()).unwrap();
                let root = state.state_root;
                state.apply_transition(transition).unwrap();
                assert_eq!(state.state_root, root, "duplicate producer payment changed state");
            }
            assert_eq!(state.emission_ledger.spent(EmissionRole::Producer), expected_producer);
            assert_eq!(state.get_account(&addr(91)).map_or(0, |a| a.balance.into_inner()), expected_producer);
            let records = authorized_settlement(&state, &decision, height, partition.slice(EmissionRole::Attester));
            let attesters = StateTransition::ApplyAttesterSettlements { records };
            state.apply_transition(attesters.clone()).unwrap();
            let before_repeat = state.state_root;
            let _ = state.apply_transition(attesters);
            assert_eq!(state.state_root, before_repeat, "repeated attester claim cannot mint");
            let contribution = state.emission_ledger.remaining(EmissionRole::Contribution)
                .min(MAX_SUPPLY - state.total_minted_supply());
            state.apply_transition(StateTransition::ApplyGamificationReward {
                address: addr(93), amount: Balance::new(contribution),
                claim_id: format!("composition-{height}-{near_cap}"), current_block: height,
            }).unwrap();
            let supply_before_retry = state.total_minted_supply();
            state.apply_transition(StateTransition::ApplyFaucetReward).unwrap();
            state.apply_transition(StateTransition::ApplyReward {
                address: addr(94), amount: Balance::new(partition.emission().max(1)), reason: "coinbase".into(),
            }).unwrap();
            state.apply_transition(StateTransition::ApplyGamificationReward {
                address: addr(95), amount: Balance::new(partition.emission().max(1)),
                claim_id: format!("greedy-{height}-{near_cap}"), current_block: height,
            }).unwrap();
            assert_eq!(state.total_minted_supply(), supply_before_retry);
            let minted = state.total_minted_supply() - initial_supply;
            assert_eq!(state.emission_ledger.total_spent(), minted);
            assert!(minted <= partition.emission());
            assert!(state.total_minted_supply() <= MAX_SUPPLY);
            for role in EMISSION_ROLE_ORDER {
                assert!(state.emission_ledger.spent(role) <= partition.slice(role));
            }
            assert_eq!(state.emission_ledger.spent(EmissionRole::Node), 0);
            if !near_cap {
                for role in [EmissionRole::Contribution, EmissionRole::Producer, EmissionRole::Faucet, EmissionRole::Attester] {
                    assert_eq!(state.emission_ledger.spent(role), partition.slice(role), "height {height}, {role:?}");
                }
            } else if partition.slice(EmissionRole::Producer) > 7 {
                assert_eq!(expected_producer, 7);
                assert_eq!(state.total_minted_supply(), MAX_SUPPLY);
                assert_eq!(state.emission_ledger.remaining(EmissionRole::Producer), partition.slice(EmissionRole::Producer) - 7);
            }
            if partition.emission() == 0 { assert_eq!(minted, 0); }
        }
    }
}

/// E=75 is an explicit partition tie vector, not a claimed schedule height.
/// Apply the real producer transition at genesis height under both chain tables.
#[test]
fn b_authorized_producer_at_genesis_with_75_unit_partition_phase_b() {
    use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
    use wrfcoin_shared_protocol::consensus::proof_of_publishing::EmissionPartitionLedger;
    use wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig;
    for chain_id in ["mainnet", "777"] {
        let mut state = State::new();
        state.open_emission_block(1);
        state.apply_transition(StateTransition::RecordGenesisEmissionConfig {
            config: GenesisEmissionConfig { chain_id: chain_id.into(), faucet_address: None , ..Default::default() },
        }).unwrap();
        let mut rng = StdRng::seed_from_u64(3069_75);
        let secret = PrivateKey::generate(&mut rng);
        let key = PublicKey::from_private_key(&secret).as_bytes().try_into().unwrap();
        state.seed_genesis_producer_set(vec![ProducerMember {
            producer_id: "e75-genesis".into(), public_key: key,
            payout_address: addr(96).to_string(),
        }]).unwrap();
        let partition = EmissionPartition::from_emission(75, Some(chain_id));
        state.emission_ledger = EmissionPartitionLedger::open_with_partition(1, partition);
        let receipt = state.derive_producer_settlement(1, &key).unwrap();
        assert!(receipt.amount > 0);
        assert_eq!(receipt.amount, partition.slice(EmissionRole::Producer));
        let transition = StateTransition::ApplyProducerSettlement { producer_pubkey: key, record: receipt };
        state.apply_transition(transition.clone()).unwrap();
        assert_eq!(state.emission_ledger.spent(EmissionRole::Producer), partition.slice(EmissionRole::Producer));
        assert_eq!(state.get_account(&addr(96)).unwrap().balance.into_inner(), partition.slice(EmissionRole::Producer));
        assert_eq!(state.emission_ledger.total_spent(), realized_supply(&state));
        assert!(realized_supply(&state) <= 75);
        let root = state.state_root;
        state.apply_transition(transition).unwrap();
        assert_eq!(state.state_root, root);
    }
}
