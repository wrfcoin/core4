//! Property-based tests for consensus and state code (#723).
//!
//! These properties exercise balance arithmetic, state determinism, nonce
//! monotonicity, and serialization roundtrips under random inputs.

use proptest::prelude::*;
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::Address;
use wrf_primitives::Balance;

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

/// Integer square root for property C verification.
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = (n as f64).sqrt() as u64;
    // Newton's method correction — the f64 sqrt may be off by one.
    loop {
        let x1 = (x + n / x) / 2;
        if x1 >= x {
            return x;
        }
        x = x1;
    }
}

// ---------------------------------------------------------------------------
// Property A: Balance checked arithmetic never panics
// ---------------------------------------------------------------------------
proptest! {
    #[test]
    fn balance_checked_arithmetic_never_panics(a in 0u128..=u128::MAX, b in 0u128..=u128::MAX) {
        let ba = Balance::new(a);
        let bb = Balance::new(b);

        // These must always return Some or None — never panic.
        let _ = ba.checked_add(bb);
        let _ = ba.checked_sub(bb);
    }
}

// ---------------------------------------------------------------------------
// Property B: State root is deterministic
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn state_root_is_deterministic(
        bal_a in 1_000u128..1_000_000,
        bal_b in 0u128..1_000_000,
        transfer_pct in 1u64..100u64,
    ) {
        let amount = Balance::new(((bal_a as u64) * transfer_pct / 100).max(1) as u128);

        let build = |label: &str| -> State {
            let _ = label;
            let mut s = State::new();
            s.create_account(addr(1), Balance::new(bal_a));
            s.create_account(addr(2), Balance::new(bal_b));

            let t = StateTransition::BalanceTransfer {
                from: addr(1),
                to: addr(2),
                amount,
                nonce: Some(0),
                current_block: 0,
            };
            s.apply_transition(t).unwrap();
            s.recalculate_state_root();
            s
        };

        let sa = build("a");
        let sb = build("b");

        prop_assert_eq!(sa.state_root, sb.state_root);
    }
}

// ---------------------------------------------------------------------------
// Property C: isqrt is correct for all inputs
// ---------------------------------------------------------------------------
proptest! {
    #[test]
    fn isqrt_is_correct(n in 0u64..=u64::MAX) {
        let r = isqrt(n);
        // r*r <= n
        prop_assert!(
            (r as u128) * (r as u128) <= n as u128,
            "isqrt({n}) = {r}, but {r}^2 > {n}"
        );
        // (r+1)^2 > n  (use u128 to avoid overflow)
        let r1 = (r as u128) + 1;
        prop_assert!(
            r1 * r1 > n as u128,
            "isqrt({n}) = {r}, but ({r}+1)^2 <= {n}"
        );
    }
}

// ---------------------------------------------------------------------------
// Property D: Nonce monotonicity under sequential transfers
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn nonce_monotonicity(count in 1u64..20) {
        // Give the sender enough balance for `count` transfers of 1 unit each.
        let initial = Balance::new((count + 1) as u128 * 100);
        let mut state = State::new();
        state.create_account(addr(1), initial);
        state.create_account(addr(2), Balance::new(0));

        let block_height: u64 = 0;

        for i in 0..count {
            let t = StateTransition::BalanceTransfer {
                from: addr(1),
                to: addr(2),
                amount: Balance::new(1),
                nonce: Some(i),
                current_block: block_height,
            };
            state.apply_transition(t).unwrap();
        }

        let final_nonce = state.get_account(&addr(1)).unwrap().nonce;
        prop_assert_eq!(final_nonce, count, "nonce should equal number of transfers");
    }
}

// ---------------------------------------------------------------------------
// Property E: Transaction JSON serialization roundtrip
// ---------------------------------------------------------------------------
proptest! {
    #[test]
    fn transaction_json_roundtrip(
        fee in 0u64..1_000_000,
        amount in 0u64..1_000_000,
        nonce in 0u64..1_000_000,
        version in 1u32..10,
    ) {
        use wrfcoin_shared_protocol::blockchain::transaction::{
            Transaction, TransactionInput, TransactionOutput,
        };
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let input = TransactionInput {
            txid: Hash::default(),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        };
        let output = TransactionOutput {
            amount,
            address: "addr1".to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        };

        let mut tx = Transaction::new(vec![input], vec![output]);
        tx.fee = fee;
        tx.nonce = nonce;
        tx.version = version;
        tx.set_id();

        let json = serde_json::to_string(&tx).unwrap();
        let tx2: Transaction = serde_json::from_str(&json).unwrap();

        // Transaction doesn't derive PartialEq — compare via re-serialization.
        let json2 = serde_json::to_string(&tx2).unwrap();
        prop_assert_eq!(json, json2, "JSON roundtrip should be stable");

        // Hashes must also match.
        prop_assert_eq!(tx.calculate_hash(), tx2.calculate_hash());
    }
}
