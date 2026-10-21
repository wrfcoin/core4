//! Independent behavioural oracle for the staking parity row (#2330).
//!
//! `parity_shared.rs` compares `native::X` against `shared::X`, but every
//! `native-chain/src/{accounts,identity,staking,weather,invariants}/mod.rs` is a one-line
//! `pub use` re-export of `shared-protocol`, so both sides of every comparison resolve to
//! the *same* item. That catches a fork (two implementations drifting apart) but not a
//! **shared** bug: a defect in `shared::staking::liquid_staking` appears identically on
//! both sides, so `native == shared` still holds and parity passes. The transcribed
//! `expected` constants in `parity_shared.rs` pin a single fixture's output, but they were
//! copied from that same code path and only check one input point.
//!
//! Three rows now have an independent reference:
//!
//! **staking** (#2348) — its transition is pure share-price arithmetic on consensus-critical
//! balances, exactly where a shared rounding or formula bug would silently corrupt token
//! supply. The oracle is the textbook share-pool rule written from its definition as a handful
//! of plain `u128` operations, computed a different way than the production path (a mutable
//! `LiquidStakingPool` threaded through `preview_mint` → `deposit` → `preview_redeem` →
//! `redeem`). Because it does not share production's structure — the struct mutation, the
//! `checked_add`/`checked_sub` bookkeeping, the pre/post-deposit state threading, the
//! `exchange_rate` edge cases — a bug in any of those steps makes production diverge from the
//! oracle even though `native == shared` still holds.
//!
//! **accounts** (#2353) — the `ValidationPipeline`'s nonce / replay / accept state machine: a
//! replay-safety and nonce-monotonicity invariant, higher-value than pure bookkeeping.
//! `parity_shared.rs` pins the sender nonce at two single points; the oracle is the ordered
//! gate (time → replay → signature → sponsorship → nonce, advance-only-on-accept) written
//! straight from its definition and run over a whole sequence, so a shared bug in the ordering
//! or the advance rule diverges from it.
//!
//! **identity** (this change) — the `IdentityRegistry` register / update-level state machine,
//! and the derived `is_station_verified` predicate, which is `verification_level >=
//! StationVerified` — so `Institutional`, a *higher* level, also counts. That `>=` boundary is
//! exactly the kind a shared `==` bug gets wrong. The oracle models the registry as
//! `addr -> rank` with the station-verified test written as an explicit rank comparison,
//! independent of the enum's own derived `Ord`, run over a register/update/downgrade sequence.
//!
//! Deferred (see #2330 and the PR body): an independent oracle for `weather` — its temporal
//! validation is policy-threshold-driven (gauge-overflow bands, elevation, warning/acceptance
//! cutoffs), so a reference would mostly transcribe that policy, the weaker shape. `invariants`
//! is already the most independently-anchored row (baseline constants + the #2310 detection
//! gating), so it is the lowest-value target. Listed explicitly rather than silently dropped.

// The independent reference. Share-pool accounting from its definition:
//   * shares minted for a deposit  = deposit * total_shares / total_assets   (floor),
//     or 1:1 when the pool is empty (no shares or no assets yet);
//   * assets returned for a redeem = shares * total_assets / total_shares    (floor).
// These four lines are the whole spec. They are deliberately NOT the production code path.

/// stWRF minted when `wrf` is deposited into a pool holding (`locked` WRF, `supply` stWRF).
fn oracle_mint(locked: u128, supply: u128, wrf: u128) -> u128 {
    if locked == 0 || supply == 0 {
        wrf // first liquidity: mint 1:1
    } else {
        // wrf * supply / locked, floored. Fixtures are small, so u128 does not overflow;
        // checked_mul makes that assumption explicit rather than silent.
        wrf.checked_mul(supply).expect("oracle mint fits u128") / locked
    }
}

/// WRF returned when `stwrf` is redeemed from a pool holding (`locked` WRF, `supply` stWRF).
fn oracle_redeem(locked: u128, supply: u128, stwrf: u128) -> u128 {
    stwrf.checked_mul(locked).expect("oracle redeem fits u128") / supply.max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StakingFlow {
    stwrf_minted: u128,
    wrf_out: u128,
    total_wrf_staked: u128,
    total_stwrf_supply: u128,
    exchange_rate_num: u128,
    exchange_rate_den: u128,
}

/// The expected result of "deposit `wrf_to_stake`, then redeem `stwrf_to_redeem`" starting
/// from `(locked0, supply0)`, computed straight from the share-pool definition above.
fn oracle_flow(locked0: u128, supply0: u128, wrf_to_stake: u128, stwrf_to_redeem: u128) -> StakingFlow {
    let stwrf_minted = oracle_mint(locked0, supply0, wrf_to_stake);
    let locked1 = locked0 + wrf_to_stake;
    let supply1 = supply0 + stwrf_minted;

    let wrf_out = oracle_redeem(locked1, supply1, stwrf_to_redeem);
    let locked2 = locked1 - wrf_out;
    let supply2 = supply1 - stwrf_to_redeem;

    // Mirrors LiquidStakingPool::exchange_rate: (1, 1) while either side is empty.
    let (exchange_rate_num, exchange_rate_den) = if locked2 == 0 || supply2 == 0 {
        (1, 1)
    } else {
        (locked2, supply2)
    };

    StakingFlow {
        stwrf_minted,
        wrf_out,
        total_wrf_staked: locked2,
        total_stwrf_supply: supply2,
        exchange_rate_num,
        exchange_rate_den,
    }
}

/// The same flow through the PRODUCTION path (`native::staking` re-exports `shared::staking`,
/// so this exercises the real shared implementation a shared bug would live in).
fn production_flow(
    locked0: u128,
    supply0: u128,
    wrf_to_stake: u128,
    stwrf_to_redeem: u128,
) -> StakingFlow {
    use crate::common::types::{Address, Balance};
    use crate::staking::liquid_staking::LiquidStakingPool;

    let mut pool = LiquidStakingPool {
        total_wrf_staked: Balance::new(locked0),
        total_stwrf_supply: Balance::new(supply0),
    };
    let position = pool
        .deposit(Address([7; 20]), Balance::new(wrf_to_stake), 10_000)
        .expect("production deposit should succeed on oracle fixtures");
    let wrf_out = pool
        .redeem(Balance::new(stwrf_to_redeem))
        .expect("production redeem should succeed on oracle fixtures");
    let (er_num, er_den) = pool.exchange_rate();

    StakingFlow {
        stwrf_minted: position.stwrf_minted.into_inner(),
        wrf_out: wrf_out.into_inner(),
        total_wrf_staked: pool.total_wrf_staked.into_inner(),
        total_stwrf_supply: pool.total_stwrf_supply.into_inner(),
        exchange_rate_num: er_num.into_inner(),
        exchange_rate_den: er_den.into_inner(),
    }
}

// (name, locked0, supply0, wrf_to_stake, stwrf_to_redeem). Chosen to cover: first-liquidity
// 1:1 minting, the exchange-rate-sensitive fixture parity_shared.rs already pins, two
// floor-rounding points (where a ceil/round/off-by-one shared bug would show), a larger
// multiply, and a redeem-to-empty that drives exchange_rate back to (1, 1).
const STAKING_FIXTURES: [(&str, u128, u128, u128, u128); 5] = [
    ("first_liquidity_one_to_one", 0, 0, 1_000, 500),
    ("exchange_rate_sensitive", 3_000, 2_000, 750, 400),
    ("floor_rounding_mint_and_redeem", 1_000, 333, 100, 33),
    ("large_values_no_overflow", 1_000_000, 999_983, 12_345, 7),
    ("redeem_to_empty_resets_rate", 0, 0, 500, 500),
];

// ---------------------------------------------------------------------------------------
// accounts: the nonce / replay / accept state machine (#2330, second row)
//
// `ValidationPipeline::validate_and_apply` gates a UserOperation through, IN ORDER: time
// window → replay (a set of already-ACCEPTED operation hashes) → signature → sponsorship →
// nonce, and the sender's nonce advances by exactly one ONLY when every gate passes. That
// ordering, and advance-only-on-accept, is the replay-safety / nonce-monotonicity invariant.
// `parity_shared.rs` pins the nonce at two single points; the oracle below is the same rules
// written from their definition, run over a sequence and compared to the real pipeline's
// (outcome, nonce-after) at every step. It is deliberately NOT production's code path.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpOutcome {
    Accepted,
    TimeWindow,
    Replay,
    InvalidSignature,
    NonceMismatch,
}

/// One submission's declared properties — enough to predict the pipeline's decision without
/// running it. `resubmit_of` names an earlier submission whose operation is byte-identical
/// (same hash) — the only way a genuine replay arises.
struct Submission {
    label: &'static str,
    nonce: u64,
    sig_valid: bool,
    in_time_window: bool,
    resubmit_of: Option<usize>,
}

/// Independent model of the pipeline's decision and the resulting sender nonce for each
/// submission. `expected` is the sender's next nonce; `accepted` holds the identities of
/// submissions the pipeline accepted (hence the entries in its replay set). Straight from the
/// ordered rules above — the whole spec is the five-branch `if` below.
fn accounts_oracle(subs: &[Submission]) -> Vec<(OpOutcome, u64)> {
    let mut expected: u64 = 0;
    let mut accepted: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(subs.len());
    for (i, s) in subs.iter().enumerate() {
        let identity = s.resubmit_of.unwrap_or(i);
        let outcome = if !s.in_time_window {
            OpOutcome::TimeWindow
        } else if accepted.contains(&identity) {
            OpOutcome::Replay
        } else if !s.sig_valid {
            OpOutcome::InvalidSignature
        } else if s.nonce != expected {
            OpOutcome::NonceMismatch
        } else {
            OpOutcome::Accepted
        };
        if outcome == OpOutcome::Accepted {
            expected += 1;
            accepted.insert(identity);
        }
        out.push((outcome, expected));
    }
    out
}

/// The same sequence through the PRODUCTION pipeline (`native::accounts` re-exports
/// `shared::accounts`, so this exercises the real shared implementation a shared bug lives in).
fn accounts_production(subs: &[Submission]) -> Vec<(OpOutcome, u64)> {
    use crate::accounts::abstraction::{
        AccountValidationError, UserOperation, ValidationContext, ValidationPipeline,
        user_operation_hash,
    };
    use crate::common::types::{Address, BlockNumber, ChainId, Nonce, Timestamp};
    use crate::crypto::hash::hash_bytes;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::{SeedableRng, rngs::StdRng};

    let context = ValidationContext {
        chain_id: ChainId::new(77),
        block_number: BlockNumber::new(100),
        timestamp: Timestamp::new(15),
    };
    let sender = Address([9; 20]);

    // A validly-signed op with declared nonce / signature-validity / time-window. The
    // sig-validity flag also perturbs call_data so two fresh ops never collide by accident.
    let build = |nonce: u64, sig_valid: bool, in_window: bool| -> UserOperation {
        let call_data = vec![0xAA, nonce as u8, u8::from(sig_valid)];
        let call_data_hash: [u8; 32] = hash_bytes(&call_data).into();
        let mut rng = StdRng::seed_from_u64(50_000 + nonce);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        // in-window: [0, 1000] contains t=15; out-of-window: [0, 1] excludes it.
        let valid_until = if in_window { 1_000 } else { 1 };
        let mut op = UserOperation {
            sender,
            nonce: Nonce::new(nonce),
            call_data_hash,
            max_fee_per_gas: 5,
            signer_public_key: pk.as_bytes().to_vec(),
            signature: vec![],
            valid_after: Some(Timestamp::new(0)),
            valid_until: Some(Timestamp::new(valid_until)),
            validation_gas_limit: 1_000_000,
            call_gas_limit: 0,
            paymaster: None,
        };
        let message = user_operation_hash(&op, context);
        op.signature = sk.sign(&message).as_bytes().to_vec();
        if !sig_valid {
            match op.signature.first_mut() {
                Some(b) => *b ^= 0xFF,
                None => op.signature = vec![0u8],
            }
        }
        op
    };

    // A resubmit reuses the byte-identical earlier op (same hash → a real replay).
    let mut ops: Vec<UserOperation> = Vec::with_capacity(subs.len());
    for s in subs {
        let op = match s.resubmit_of {
            Some(k) => ops[k].clone(),
            None => build(s.nonce, s.sig_valid, s.in_time_window),
        };
        ops.push(op);
    }

    let mut pipeline = ValidationPipeline::default();
    let mut out = Vec::with_capacity(subs.len());
    for op in &ops {
        let outcome = match pipeline.validate_and_apply(op, context, None) {
            Ok(_) => OpOutcome::Accepted,
            Err(AccountValidationError::InvalidTimeWindow) => OpOutcome::TimeWindow,
            Err(AccountValidationError::ReplayDetected(_)) => OpOutcome::Replay,
            Err(AccountValidationError::InvalidSignature) => OpOutcome::InvalidSignature,
            Err(AccountValidationError::NonceMismatch { .. }) => OpOutcome::NonceMismatch,
            Err(other) => panic!("nonce-focused fixtures never use sponsorship: {other}"),
        };
        out.push((outcome, pipeline.expected_nonce(&sender).into_inner()));
    }
    out
}

// A single sender's op sequence exercising every gate and the advance-only-on-accept rule:
// accept, replay of that accept (its nonce is now stale, but replay is checked first),
// nonce-too-high, valid-nonce-but-bad-signature, valid-nonce-but-out-of-window, then two more
// accepts. Only the accepts may move the nonce.
fn accounts_fixture() -> Vec<Submission> {
    vec![
        Submission { label: "accept nonce 0", nonce: 0, sig_valid: true, in_time_window: true, resubmit_of: None },
        Submission { label: "replay of the accepted op (stale nonce, replay wins)", nonce: 0, sig_valid: true, in_time_window: true, resubmit_of: Some(0) },
        Submission { label: "nonce too high", nonce: 5, sig_valid: true, in_time_window: true, resubmit_of: None },
        Submission { label: "valid nonce but bad signature", nonce: 1, sig_valid: false, in_time_window: true, resubmit_of: None },
        Submission { label: "valid nonce but outside time window", nonce: 1, sig_valid: true, in_time_window: false, resubmit_of: None },
        Submission { label: "accept nonce 1", nonce: 1, sig_valid: true, in_time_window: true, resubmit_of: None },
        Submission { label: "accept nonce 2", nonce: 2, sig_valid: true, in_time_window: true, resubmit_of: None },
    ]
}

// ---------------------------------------------------------------------------------------
// identity: the verification-level state machine (#2330, third row)
//
// IdentityRegistry is register / update-level / lookup. `register` refuses to overwrite an
// existing address (AlreadyRegistered) and `update_verification_level` refuses a missing one
// (NotFound). The consensus-relevant derived predicate is `is_station_verified`, defined as
// `verification_level >= StationVerified` — so `Institutional`, a HIGHER level, also counts.
// That `>=` boundary is the subtle part a shared `==` bug gets wrong. The oracle models the
// registry as `addr -> rank` and writes the station-verified test as an explicit rank
// comparison, NOT by calling the enum's derived `Ord` — so it is independent of production.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdOutcome {
    Registered,
    AlreadyExists,
    Updated,
    NotFound,
}

// Ranks of the four verification levels, stated here from their meaning rather than read off
// the enum's declaration order — this is what makes the station-verified test independent.
const RANK_BASIC: u8 = 0;
const RANK_KYC: u8 = 1;
const RANK_STATION: u8 = 2;
const RANK_INSTITUTIONAL: u8 = 3;

enum IdStep {
    Register { addr: u8, rank: u8 },
    Update { addr: u8, rank: u8 },
}

/// Independent model: for each step, the outcome, whether the touched address is
/// station-verified, and its current rank. The whole spec is the register/update branches
/// plus `rank >= RANK_STATION`.
fn identity_oracle(steps: &[IdStep]) -> Vec<(IdOutcome, bool, Option<u8>)> {
    use std::collections::HashMap;
    let mut reg: HashMap<u8, u8> = HashMap::new();
    let mut out = Vec::with_capacity(steps.len());
    for step in steps {
        let (outcome, addr) = match *step {
            IdStep::Register { addr, rank } => {
                if reg.contains_key(&addr) {
                    (IdOutcome::AlreadyExists, addr) // register never overwrites
                } else {
                    reg.insert(addr, rank);
                    (IdOutcome::Registered, addr)
                }
            },
            IdStep::Update { addr, rank } => {
                if reg.contains_key(&addr) {
                    reg.insert(addr, rank);
                    (IdOutcome::Updated, addr)
                } else {
                    (IdOutcome::NotFound, addr) // update of a missing address changes nothing
                }
            },
        };
        let current = reg.get(&addr).copied();
        let station_verified = current.map(|r| r >= RANK_STATION).unwrap_or(false);
        out.push((outcome, station_verified, current));
    }
    out
}

/// The same sequence through the PRODUCTION registry (`native::identity` re-exports
/// `shared::identity`, so this exercises the real shared implementation a shared bug lives in).
fn identity_production(steps: &[IdStep]) -> Vec<(IdOutcome, bool, Option<u8>)> {
    use crate::common::types::{Address, Timestamp};
    use crate::identity::registry::{
        ContributorIdentity, IdentityError, IdentityRegistry, VerificationLevel,
    };

    fn to_level(rank: u8) -> VerificationLevel {
        match rank {
            RANK_BASIC => VerificationLevel::Basic,
            RANK_KYC => VerificationLevel::KycVerified,
            RANK_STATION => VerificationLevel::StationVerified,
            RANK_INSTITUTIONAL => VerificationLevel::Institutional,
            other => panic!("unknown verification rank {other}"),
        }
    }
    fn from_level(level: VerificationLevel) -> u8 {
        match level {
            VerificationLevel::Basic => RANK_BASIC,
            VerificationLevel::KycVerified => RANK_KYC,
            VerificationLevel::StationVerified => RANK_STATION,
            VerificationLevel::Institutional => RANK_INSTITUTIONAL,
        }
    }
    let addr = |b: u8| Address([b; 20]);

    let mut registry = IdentityRegistry::default();
    let mut out = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let (outcome, b) = match *step {
            IdStep::Register { addr: b, rank } => {
                let res = registry.register(ContributorIdentity {
                    contributor_id: format!("id-{b}-{i}"),
                    address: addr(b),
                    verification_level: to_level(rank),
                    updated_at: Timestamp::new(100 + i as u64),
                });
                let o = match res {
                    Ok(()) => IdOutcome::Registered,
                    Err(IdentityError::AlreadyRegistered(_)) => IdOutcome::AlreadyExists,
                    Err(other) => panic!("unexpected register error: {other}"),
                };
                (o, b)
            },
            IdStep::Update { addr: b, rank } => {
                let res = registry.update_verification_level(
                    addr(b),
                    to_level(rank),
                    Timestamp::new(200 + i as u64),
                );
                let o = match res {
                    Ok(()) => IdOutcome::Updated,
                    Err(IdentityError::NotFound(_)) => IdOutcome::NotFound,
                    Err(other) => panic!("unexpected update error: {other}"),
                };
                (o, b)
            },
        };
        let current = registry.get(&addr(b)).map(|id| from_level(id.verification_level));
        let station_verified = registry.is_station_verified(&addr(b));
        out.push((outcome, station_verified, current));
    }
    out
}

// One address walked up and down the level ladder, plus a rejected duplicate register and a
// rejected update of a missing address. Exercises: register; duplicate-register-rejected
// (level unchanged); update; the `>= StationVerified` boundary (Institutional still counts);
// update-missing → NotFound; register-after; and a downgrade that flips station_verified off.
fn identity_fixture() -> Vec<IdStep> {
    vec![
        IdStep::Register { addr: 1, rank: RANK_BASIC },
        IdStep::Register { addr: 1, rank: RANK_KYC },
        IdStep::Update { addr: 1, rank: RANK_STATION },
        IdStep::Update { addr: 1, rank: RANK_INSTITUTIONAL },
        IdStep::Update { addr: 2, rank: RANK_STATION },
        IdStep::Register { addr: 2, rank: RANK_INSTITUTIONAL },
        IdStep::Update { addr: 1, rank: RANK_KYC },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property #2330 asks for: production must match an *independent* computation, not
    /// just itself. A shared bug in `preview_mint`/`deposit`/`preview_redeem`/`redeem` (which
    /// `native` re-exports, so `parity_shared`'s `native == shared` stays green) makes this
    /// fail, because the oracle does not share production's arithmetic path.
    #[test]
    fn staking_deposit_redeem_matches_independent_oracle() {
        for (name, locked0, supply0, wrf, redeem) in STAKING_FIXTURES {
            let oracle = oracle_flow(locked0, supply0, wrf, redeem);
            let production = production_flow(locked0, supply0, wrf, redeem);
            assert_eq!(
                production, oracle,
                "staking deposit/redeem diverged from the independent share-pool oracle on \
                 fixture '{name}' (inputs: locked0={locked0}, supply0={supply0}, \
                 wrf_to_stake={wrf}, stwrf_to_redeem={redeem}). A shared bug in \
                 shared::staking::liquid_staking would produce exactly this — parity's \
                 native==shared check cannot see it (#2330)."
            );
        }
    }

    /// The oracle must be able to REJECT a wrong result, or matching it proves nothing.
    /// On a non-empty pool the share rule mints 750 * 2000 / 3000 = 500 stWRF — not the 750
    /// a 1:1 (buggy) mint would produce. Pinning that distinction shows the oracle has
    /// discriminating power, so `staking_deposit_redeem_matches_independent_oracle` is a real
    /// check and not vacuously true.
    #[test]
    fn oracle_has_discriminating_power() {
        let flow = oracle_flow(3_000, 2_000, 750, 400);
        assert_eq!(flow.stwrf_minted, 500, "share-priced mint on a seeded pool");
        assert_ne!(
            flow.stwrf_minted, 750,
            "oracle must distinguish share-priced minting from a 1:1 bug, or it has no teeth"
        );
        // Floor rounding is observable, not swallowed: 100 * 333 / 1000 = 33 (33.3 floored).
        assert_eq!(oracle_mint(1_000, 333, 100), 33);
    }

    /// Cross-check the oracle against production's own documented unit tests
    /// (`initial_deposit_mints_one_to_one`, `redeem_reduces_pool_balances`) so the oracle is
    /// anchored to behaviour the shared crate independently asserts, not only to this file.
    #[test]
    fn oracle_agrees_with_shared_documented_unit_cases() {
        // initial_deposit_mints_one_to_one: 1000 WRF into an empty pool → 1000 stWRF.
        assert_eq!(oracle_mint(0, 0, 1_000), 1_000);
        // redeem_reduces_pool_balances: from (2000, 2000), redeem 500 → 500 WRF out.
        assert_eq!(oracle_redeem(2_000, 2_000, 500), 500);
    }

    // ---- accounts nonce / replay / accept state machine (#2330, second row) ----

    /// The #2330 property for accounts: the real pipeline's (outcome, nonce-after) at every
    /// step of a sequence must match an independent model of the ordered gate. A shared bug in
    /// `shared::accounts::abstraction` — reordering the gates, or advancing the nonce on a
    /// rejected op — makes this fail, while `parity_shared`'s native==shared check stays green.
    #[test]
    fn accounts_nonce_replay_matches_independent_oracle() {
        let subs = accounts_fixture();
        let oracle = accounts_oracle(&subs);
        let production = accounts_production(&subs);
        for (i, s) in subs.iter().enumerate() {
            assert_eq!(
                production[i], oracle[i],
                "accounts pipeline diverged from the independent nonce/replay oracle at step \
                 {i} ('{}'): production={:?} oracle={:?}. A shared bug in \
                 shared::accounts::abstraction would produce exactly this — parity's \
                 native==shared check cannot see it (#2330).",
                s.label, production[i], oracle[i]
            );
        }
    }

    /// The oracle must be able to REJECT a wrong result. This hand-checked sequence pins the
    /// invariant that matters: accept advances the nonce by exactly one; replay of an accepted
    /// op wins over its now-stale nonce; nonce-mismatch, bad-signature and out-of-window each
    /// reject WITHOUT advancing. If a rejected op advanced the nonce the final value would be
    /// past 3 — so this is a real check, not vacuously true.
    #[test]
    fn accounts_oracle_has_discriminating_power() {
        let expected: Vec<(OpOutcome, u64)> = vec![
            (OpOutcome::Accepted, 1),
            (OpOutcome::Replay, 1),
            (OpOutcome::NonceMismatch, 1),
            (OpOutcome::InvalidSignature, 1),
            (OpOutcome::TimeWindow, 1),
            (OpOutcome::Accepted, 2),
            (OpOutcome::Accepted, 3),
        ];
        assert_eq!(accounts_oracle(&accounts_fixture()), expected);
        // Exactly three accepts among seven submissions: the four rejects did not move it.
        assert_eq!(accounts_oracle(&accounts_fixture()).last().unwrap().1, 3);
    }

    // ---- identity verification-level state machine (#2330, third row) ----

    /// The #2330 property for identity: the real registry's (outcome, station_verified, rank)
    /// at every step must match an independent model. A shared bug in
    /// `shared::identity::registry` — `==` instead of `>=` in `is_station_verified`, or
    /// `register` overwriting, or a level mis-mapping — makes this fail, while
    /// `parity_shared`'s native==shared check stays green.
    #[test]
    fn identity_levels_match_independent_oracle() {
        let steps = identity_fixture();
        let oracle = identity_oracle(&steps);
        let production = identity_production(&steps);
        for i in 0..steps.len() {
            assert_eq!(
                production[i], oracle[i],
                "identity registry diverged from the independent verification-level oracle at \
                 step {i}: production={:?} oracle={:?}. A shared bug in \
                 shared::identity::registry would produce exactly this — parity's native==shared \
                 check cannot see it (#2330).",
                production[i], oracle[i]
            );
        }
    }

    /// The oracle must be able to REJECT a wrong result. The decisive rows are the two where
    /// the address is at `Institutional` (rank 3): station_verified must be TRUE, because the
    /// predicate is `>= StationVerified`, not `== StationVerified`. Pinning that — plus the
    /// duplicate-register-leaves-level-unchanged and downgrade-flips-off rows — shows the check
    /// is real, and is exactly what a shared `==` bug would break.
    #[test]
    fn identity_oracle_has_discriminating_power() {
        let expected: Vec<(IdOutcome, bool, Option<u8>)> = vec![
            (IdOutcome::Registered, false, Some(RANK_BASIC)),
            (IdOutcome::AlreadyExists, false, Some(RANK_BASIC)), // duplicate register: level unchanged
            (IdOutcome::Updated, true, Some(RANK_STATION)),
            (IdOutcome::Updated, true, Some(RANK_INSTITUTIONAL)), // Institutional >= Station ⇒ verified
            (IdOutcome::NotFound, false, None),                   // update of a never-registered addr
            (IdOutcome::Registered, true, Some(RANK_INSTITUTIONAL)),
            (IdOutcome::Updated, false, Some(RANK_KYC)),          // downgrade below Station flips it off
        ];
        assert_eq!(identity_oracle(&identity_fixture()), expected);
        // The `>=` boundary, stated bluntly: Institutional counts, Kyc does not.
        assert!(RANK_INSTITUTIONAL >= RANK_STATION);
        assert!(RANK_KYC < RANK_STATION);
    }
}
