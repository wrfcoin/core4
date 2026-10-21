//! Validator-set sourcing for the BFT finality gadget (#1501a).
//!
//! Pure projection from chain [`State`] → the
//! [`Vec<FinalityValidator>`](FinalityValidator) shape required by
//! [`BftFinalityGadget::new`](super::bft_finality::BftFinalityGadget::new).
//!
//! This is the **sourcing** half of the BFT-finality wire-up plan from
//! issue #1501. It does *not* construct or attach a gadget, spawn a vote
//! producer, or change any runtime behaviour — those steps land as
//! follow-ups #1501b–e once the PoS-vs-PoP testnet engine question is
//! settled.
//!
//! Living here (not in `shared-protocol`) because [`FinalityValidator`]
//! is a `consensus-engines` type and `consensus-engines` already depends
//! on `shared-protocol`; pushing the converter the other way would
//! invert the dependency direction.

use wrfcoin_shared_protocol::blockchain::state::State;

use super::bft_finality::FinalityValidator;

/// Project the current chain state into the validator set required by
/// [`BftFinalityGadget::new`](super::bft_finality::BftFinalityGadget::new).
///
/// An account is included iff its `stake_locked` (saturated to `u64`) is
/// at least `min_stake` — the same threshold
/// `ProofOfStake::active_finality_validators` applies to the in-memory
/// validator mirror, so the on-chain-sourced set will agree with the
/// in-memory set wherever both exist.
///
/// The returned vector is sorted by validator id so every node that
/// observes the same `State` constructs an identical gadget validator
/// set. `public_key` is left `None`: the chain state does not store a
/// per-validator BFT signing key, so vote authentication is wired in by
/// the gadget constructor at the call site (#1501b/c).
///
/// `Account.stake_locked` is `Balance` (a `u128` newtype). Values that
/// exceed `u64::MAX` saturate to `u64::MAX` rather than truncating —
/// this is consistent with the `ValidatorInfo::stake` mirror in
/// `pos.rs::debit_validator_stake`, which uses the same saturation rule.
pub fn validators_from_state(state: &State, min_stake: u64) -> Vec<FinalityValidator> {
    let mut out: Vec<FinalityValidator> = state
        .accounts
        .iter()
        .filter_map(|(addr, account)| {
            let raw: u128 = account.stake_locked.into_inner();
            let stake_weight = u64::try_from(raw).unwrap_or(u64::MAX);
            if stake_weight < min_stake {
                return None;
            }
            Some(FinalityValidator {
                id: addr.to_hex(),
                stake_weight,
                public_key: None,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::blockchain::state::{Account, State};
    use wrfcoin_shared_protocol::common::types::{Address, Balance};

    fn addr(byte: u8) -> Address {
        Address::new([byte; 20])
    }

    fn account_with_stake(stake: u128) -> Account {
        let mut a = Account::new(Balance::new(stake));
        a.stake_locked = Balance::new(stake);
        a
    }

    fn state_with(accounts: Vec<(Address, Account)>) -> State {
        let mut s = State::default();
        for (addr, account) in accounts {
            s.accounts.insert(addr, account);
        }
        s
    }

    #[test]
    fn empty_state_yields_empty_set() {
        let s = State::default();
        assert_eq!(validators_from_state(&s, 100), Vec::new());
    }

    #[test]
    fn account_above_threshold_is_included() {
        let s = state_with(vec![(addr(0x01), account_with_stake(500))]);
        let out = validators_from_state(&s, 100);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, addr(0x01).to_hex());
        assert_eq!(out[0].stake_weight, 500);
        assert!(out[0].public_key.is_none());
    }

    #[test]
    fn account_at_threshold_is_included() {
        let s = state_with(vec![(addr(0x02), account_with_stake(100))]);
        let out = validators_from_state(&s, 100);
        assert_eq!(out.len(), 1, "stake == min_stake must be included");
    }

    #[test]
    fn account_below_threshold_is_excluded() {
        let s = state_with(vec![(addr(0x03), account_with_stake(99))]);
        let out = validators_from_state(&s, 100);
        assert!(out.is_empty(), "stake < min_stake must be excluded");
    }

    #[test]
    fn output_is_sorted_by_id_for_determinism() {
        // Insert in reverse-id order; expect ascending hex id in output.
        let s = state_with(vec![
            (addr(0x33), account_with_stake(300)),
            (addr(0x11), account_with_stake(100)),
            (addr(0x22), account_with_stake(200)),
        ]);
        let out = validators_from_state(&s, 100);
        let ids: Vec<&str> = out.iter().map(|v| v.id.as_str()).collect();
        let mut expected = ids.clone();
        expected.sort();
        assert_eq!(ids, expected, "output must be sorted by id");
    }

    #[test]
    fn stake_above_u64_max_saturates() {
        // stake_locked is u128; cap at u64::MAX rather than truncating.
        let huge: u128 = u128::from(u64::MAX) + 1_000;
        let s = state_with(vec![(addr(0x04), account_with_stake(huge))]);
        let out = validators_from_state(&s, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].stake_weight,
            u64::MAX,
            "u128 stake exceeding u64::MAX must saturate, not wrap"
        );
    }

    #[test]
    fn mixed_set_filters_and_sorts() {
        let s = state_with(vec![
            (addr(0xaa), account_with_stake(50)),  // below
            (addr(0xbb), account_with_stake(200)), // above
            (addr(0x10), account_with_stake(100)), // at threshold
            (addr(0x20), account_with_stake(0)),   // zero stake
        ]);
        let out = validators_from_state(&s, 100);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, addr(0x10).to_hex());
        assert_eq!(out[1].id, addr(0xbb).to_hex());
    }
}
