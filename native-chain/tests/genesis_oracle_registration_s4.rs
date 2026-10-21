//! S4 — semantic unit controls for the genesis oracle registrations
//! (#2055 / #2464 / #724).
//!
//! # This file is NOT the staged-state gate
//!
//! An earlier revision of this file claimed to be it, with "producer", "peer"
//! and "restart" legs. It was not. Every one of those legs called a local
//! helper that parsed each transaction and **manufactured**
//! `StateTransition::ApplyOracleRegistration` directly, then applied the batch
//! to a `Blockchain::default()`. No block was mined, serialized, validated, or
//! passed through the applier — and because the helper never consulted
//! `verify_oracle_registration_authorization`, deleting a registration's
//! governance signature left the "peer" and "restart" legs GREEN. The tests
//! fabricated the exact registry/quorum/signing-key preconditions they then
//! asserted: the #908/#934 manufactured-preconditions class.
//!
//! **The gate now lives in `native-chain/src/bin/launch_api/p2p_events.rs`,
//! module `tests::genesis_oracle_set_s4`**, where the real seams are reachable:
//! `create_pop_block_with_state` for the producer and `apply_p2p_block` for the
//! peer, with the restart leg replaying the storage the applier itself wrote
//! and the sync leg snapshotting the state the applier itself accepted. Those
//! seams are private to the `launch_api` binary, which is why an integration
//! test cannot host them.
//!
//! # What remains here, and why it is still worth running
//!
//! Two properties of the *payload*, which need no block to be true and are
//! cheaper to read here than inside the P2P module:
//!
//!   * the ABSOLUTE-set semantics of `oracle_set` (#2055 Option A), including
//!     the ceremony footgun where a second registration naming only its own
//!     identity evicts the first; and
//!   * the governance authorization binding — that an unsigned or field-edited
//!     registration does not authorize.
//!
//! Neither is evidence that block 1 works. They are the semantics the gate
//! depends on, asserted in isolation so a failure points at the payload rather
//! than at the block pipeline.
//!
//! # VECTOR-BACKED vs NEEDS REAL KEYS
//!
//! Seeded keypairs stand in for the ceremony's output. The properties asserted
//! here are properties of the code, and substituting different ed25519 keys
//! does not change any of them. What the ceremony must still supply — two
//! operator identities, three signing pubkeys, and the governance pubkey that has
//! to be installed in `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS` on **every** node
//! before block 1 is cut — is enumerated on the gate module.

use rand::SeedableRng;
use rand::rngs::StdRng;

use wrfcoin_native_chain::blockchain::state::{State, StateTransition};
use wrfcoin_native_chain::blockchain::transaction::{Transaction, TransactionType};
use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};

/// Seeds are fixed so the suite is reproducible, and match the gate module's.
const SEED_ORACLE_A: u64 = 2055_0001;
const SEED_ORACLE_B: u64 = 2055_0002;
const SEED_ORACLE_C: u64 = 2055_0003;
const SEED_ORACLE_A_SIGNING: u64 = 2055_0011;
const SEED_ORACLE_B_SIGNING: u64 = 2055_0012;
const SEED_ORACLE_C_SIGNING: u64 = 2055_0013;
const SEED_GOVERNANCE: u64 = 2055_0215;
const SEED_ATTACKER: u64 = 2055_0666;

const EPOCH: u64 = 1;
const ACTIVATED_AT: i64 = 1_700_000_000;

fn keypair(seed: u64) -> (PrivateKey, String) {
    let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(seed));
    let hex = PublicKey::from_private_key(&sk).to_hex();
    (sk, hex)
}

/// Install the governance allowlist for the duration of a test.
///
/// `setenv` mutates the process-wide `environ` array, so concurrent writers
/// race at the libc level regardless of the value written — hence the shared
/// lock used by every oracle test in this workspace (#2290/#2405 fallout).
fn with_governance_env<T>(gov_pubkey: &str, run: impl FnOnce() -> T) -> T {
    use wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
    // The workspace's shared lock is `#[cfg(test)] pub(crate)`, so an
    // integration test cannot reach it. This binary is a separate process, so a
    // local lock is sufficient: it serialises the threads that actually race.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let previous = std::env::var(ORACLE_GOVERNANCE_PUBKEYS_ENV).ok();
    unsafe { std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, gov_pubkey) };
    let out = run();
    match previous {
        Some(v) => unsafe { std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, v) },
        None => unsafe { std::env::remove_var(ORACLE_GOVERNANCE_PUBKEYS_ENV) },
    }
    out
}

/// Apply registrations the way the applier's extractor would — one
/// `ApplyOracleRegistration` per transaction, in transaction order, through the
/// shared batch entry point.
///
/// This is a **local** reconstruction of the extractor's mapping, used only to
/// observe absolute-set semantics under a chosen order. It deliberately does
/// not consult the governance signature, so it must never be treated as
/// evidence that a registration was accepted — that is the gate's job, and the
/// reason this helper is confined to the two semantic tests below.
fn apply_registration_payloads(state: &mut State, txs: &[Transaction]) {
    let transitions: Vec<StateTransition> = txs
        .iter()
        .map(|tx| {
            let p = tx
                .parse_oracle_registration()
                .expect("every tx here is a well-formed OracleRegistration");
            StateTransition::ApplyOracleRegistration {
                oracle_set: p.oracle_set,
                oracle_quorum: p.oracle_quorum,
                identity: p.identity,
                public_key: p.public_key,
                epoch: p.epoch,
                activated_at: p.activated_at,
            }
        })
        .collect();
    state
        .apply_transitions_batch(transitions)
        .expect("registrations must apply");
}

// ---------------------------------------------------------------------------
// 1. ABSOLUTE-SET SEMANTICS — the ceremony footgun, proven rather than warned
// ---------------------------------------------------------------------------

/// The `oracle_set` field is ABSOLUTE, not a delta. A second registration that
/// names only its own identity therefore does not "add" that oracle — it
/// REPLACES the set and evicts the first.
///
/// This is the highest-consequence mistake available while drafting block 1,
/// because block 1 is unrepeatable: the chain would come up with one oracle and
/// a quorum that can never be met, and the only remedy is another cut.
///
/// The gate module carries the sharper form of this hazard — that even a
/// *correctly cumulative* pair is unsafe if the two registrations do not both
/// name the final set, because the block builder does not preserve the order
/// transactions were staged in. See
/// `block_1_converges_regardless_of_producer_transaction_order`.
#[test]
fn second_registration_with_a_non_cumulative_set_evicts_the_first_oracle() {
    let (_sk_a, id_a) = keypair(SEED_ORACLE_A);
    let (_sk_b, id_b) = keypair(SEED_ORACLE_B);
    let (_sk_ak, signing_a) = keypair(SEED_ORACLE_A_SIGNING);
    let (_sk_bk, signing_b) = keypair(SEED_ORACLE_B_SIGNING);
    let (gov_sk, gov_pk) = keypair(SEED_GOVERNANCE);

    with_governance_env(&gov_pk, || {
        let tx_a = Transaction::new_oracle_registration(
            vec![id_a.clone()],
            1,
            &id_a,
            &signing_a,
            EPOCH,
            ACTIVATED_AT,
            &gov_sk,
        );
        // The mistake: B's registration names only B.
        let tx_b_wrong = Transaction::new_oracle_registration(
            vec![id_b.clone()],
            1,
            &id_b,
            &signing_b,
            EPOCH,
            ACTIVATED_AT,
            &gov_sk,
        );

        let mut state = State::default();
        apply_registration_payloads(&mut state, &[tx_a, tx_b_wrong]);

        assert!(
            !state.oracle_set.contains(&id_a),
            "FOOTGUN: a non-cumulative second registration evicts the first oracle"
        );
        assert!(state.oracle_set.contains(&id_b));
        assert_eq!(
            state.oracle_set.len(),
            1,
            "the set is REPLACED, not extended — block 1 would seat one oracle"
        );

        // Contrast: when BOTH registrations name the final set, the end state is
        // correct and — proven in the gate module — independent of their order.
        let final_set = vec![id_a.clone(), id_b.clone()];
        let good = [
            Transaction::new_oracle_registration(
                final_set.clone(),
                2,
                &id_a,
                &signing_a,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            ),
            Transaction::new_oracle_registration(
                final_set,
                2,
                &id_b,
                &signing_b,
                EPOCH,
                ACTIVATED_AT,
                &gov_sk,
            ),
        ];
        let mut correct = State::default();
        apply_registration_payloads(&mut correct, &good);
        assert_eq!(correct.oracle_quorum, 2);
        assert_eq!(correct.oracle_set.len(), 2);
        assert!(correct.oracle_set.contains(&id_a) && correct.oracle_set.contains(&id_b));
    });
}

// ---------------------------------------------------------------------------
// 2. AUTHORIZATION BINDING — what the gate's rejection legs rely on
// ---------------------------------------------------------------------------

/// A genesis registration authorizes only when an **allowlisted** governance
/// key signed **these exact fields**. The producer self-screen and the peer's
/// `verify_block` both call `verify_oracle_registration_authorization`, so
/// every way this returns `false` is a way block 1 gets rejected — see the
/// gate's `tampered_genesis_registration_is_rejected_by_the_{producer,peer}`.
#[test]
fn genesis_registration_authorization_is_bound_to_the_governance_key_and_fields() {
    let (_sk_a, id_a) = keypair(SEED_ORACLE_A);
    let (_sk_b, id_b) = keypair(SEED_ORACLE_B);
    let (_sk_c, id_c) = keypair(SEED_ORACLE_C);
    let (_sk_ak, signing_a) = keypair(SEED_ORACLE_A_SIGNING);
    let (_sk_ck, _signing_c) = keypair(SEED_ORACLE_C_SIGNING);
    let (gov_sk, gov_pk) = keypair(SEED_GOVERNANCE);
    let (attacker_sk, _attacker_pk) = keypair(SEED_ATTACKER);

    let registration = |signer: &PrivateKey| {
        Transaction::new_oracle_registration(
            vec![id_a.clone(), id_b.clone(), id_c.clone()],
            2,
            &id_a,
            &signing_a,
            EPOCH,
            ACTIVATED_AT,
            signer,
        )
    };

    with_governance_env(&gov_pk, || {
        let good = registration(&gov_sk);
        assert_eq!(good.tx_type, TransactionType::OracleRegistration);
        assert!(
            good.verify_oracle_registration_authorization(),
            "the allowlisted governance key must authorize"
        );

        assert!(
            !registration(&attacker_sk).verify_oracle_registration_authorization(),
            "a key that is not on the allowlist must not authorize, however \
             well-formed its signature"
        );

        // Field binding: edit the payload after signing and the signature no
        // longer covers it. This is the mutation the gate drives through the
        // producer and peer seams.
        for (what, mutate) in [
            (
                "stripped signature",
                Box::new(|p: &mut serde_json::Value| {
                    p.as_object_mut().unwrap().remove("auth_signature");
                }) as Box<dyn Fn(&mut serde_json::Value)>,
            ),
            (
                "widened set",
                Box::new(|p: &mut serde_json::Value| {
                    p["oracle_set"] =
                        serde_json::json!([id_a.clone(), id_b.clone(), id_c.clone(), "deadbeef"]);
                }),
            ),
            (
                "lowered quorum",
                Box::new(|p: &mut serde_json::Value| {
                    p["oracle_quorum"] = serde_json::json!(1);
                }),
            ),
            (
                "redirected signing key",
                Box::new(|p: &mut serde_json::Value| {
                    p["public_key"] = serde_json::json!(id_b.clone());
                }),
            ),
        ] {
            let mut edited = good.clone();
            let mut payload: serde_json::Value =
                serde_json::from_str(edited.data.as_deref().expect("payload present"))
                    .expect("payload is JSON");
            mutate(&mut payload);
            edited.data = Some(payload.to_string());
            edited.set_id();
            assert!(
                !edited.verify_oracle_registration_authorization(),
                "{what}: a registration edited after signing must not authorize"
            );
        }
    });
}
