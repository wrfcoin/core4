//! #2712 — an oracle-governance rotation performed by APPENDING leaves the
//! retired key fully authorized, and nothing reports that the set grew.
//!
//! ## Status of this file: it is a DEMONSTRATION, and it PASSES
//!
//! #2712 was filed from a source read and explicitly marked UNPROVEN. This file
//! executes it. **These tests passing is the finding** — they characterise the
//! behaviour that exists today, at the gate where it matters, and they are not an
//! endorsement of it.
//!
//! A passing test is not a gate. Nothing here will turn red the day an operator
//! makes this mistake in production; the artifact that would do that is a
//! declared-expectation check on the allowlist, which does not exist and whose
//! design is out of scope under the genesis-cut freeze. What this file buys is
//! narrower and still worth having: the claim stops being a source read, and if
//! anyone later changes the resolution semantics, `appending_rotation_*` fails and
//! forces the change to be deliberate.
//!
//! ## Why the gate, not the helper
//!
//! `is_authorized_oracle_governance` is the obvious target and it would be the
//! weaker test. The real question is whether a transaction signed by the retired
//! key is *authorized*, so these tests drive
//! `Transaction::verify_oracle_registration_authorization` — the predicate the
//! producer self-screen and every peer's `verify_block` both call
//! (`transaction.rs:2973`). A pass here is an accepted registration's
//! authorization leg, not a helper returning `true`.
//!
//! ## Control-first (#2711 shape)
//!
//! Two controls run before the observation and **must** pass:
//!
//! * `control_a_...` — the retired key authorizes while it is the whole allowlist.
//!   Without this the fixture might be signing wrongly and every later `false`
//!   would be a broken signature, not a rejection.
//! * `control_b_...` — after a REPLACING rotation the retired key is refused.
//!   Without this the predicate might return `true` for everything, and the
//!   observation would be vacuous.
//!
//! Only with both green does the observation mean what it says. That is the same
//! reason #2711 runs #2701's anti-vacuity tests before the ceiling assertion: a
//! red — or here a green — must never be the harness wearing a finding's clothes.

use rand::SeedableRng;
use rand::rngs::StdRng;
use wrfcoin_native_chain::blockchain::transaction::{Transaction, TransactionType};
use wrfcoin_native_chain::crypto::signature::{PrivateKey, PublicKey};

const SEED_ORACLE: u64 = 0x2712_0001;
const SEED_ORACLE_SIGNING: u64 = 0x2712_0002;
/// The governance key being retired by the rotation.
const SEED_GOV_OLD: u64 = 0x2712_0003;
/// The governance key the rotation is introducing.
const SEED_GOV_NEW: u64 = 0x2712_0004;

const EPOCH: u64 = 1;
const ACTIVATED_AT: i64 = 1_700_000_000;

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Install a raw allowlist value for the duration of `run`.
///
/// Takes the raw string rather than a single key, because the whole point is what
/// happens when the value contains MORE THAN ONE entry.
///
/// `setenv` mutates the process-wide `environ` array, so concurrent writers race
/// at the libc level regardless of the value written. The workspace's shared lock
/// is `#[cfg(test)] pub(crate)` and unreachable from an integration test; this
/// binary is its own process, so a local lock serialises the threads that
/// actually race (same reasoning as `genesis_oracle_registration_s4.rs:80`).
fn with_allowlist<T>(raw: &str, run: impl FnOnce() -> T) -> T {
    use wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let previous = std::env::var(ORACLE_GOVERNANCE_PUBKEYS_ENV).ok();
    unsafe { std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, raw) };
    let out = run();
    match previous {
        Some(v) => unsafe { std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, v) },
        None => unsafe { std::env::remove_var(ORACLE_GOVERNANCE_PUBKEYS_ENV) },
    }
    out
}

/// A genesis oracle registration authorized by `signer`.
fn registration_signed_by(signer: &PrivateKey) -> Transaction {
    let (_, oracle_id) = keypair(SEED_ORACLE);
    let (_, oracle_signing) = keypair(SEED_ORACLE_SIGNING);
    Transaction::new_oracle_registration(
        vec![oracle_id.to_hex()],
        1,
        &oracle_id.to_hex(),
        &oracle_signing.to_hex(),
        EPOCH,
        ACTIVATED_AT,
        signer,
    )
}

// ---------------------------------------------------------------------------
// CONTROLS — both must pass, or the observation below means nothing
// ---------------------------------------------------------------------------

/// CONTROL A. Before the rotation, the soon-to-be-retired key is the entire
/// allowlist and its signature authorizes.
///
/// If this fails, the fixture is not producing a valid governance signature and
/// every `false` in this file would be a broken signature rather than a refusal.
#[test]
fn control_a_retired_key_authorizes_while_it_is_the_whole_allowlist() {
    let (old_sk, old_pk) = keypair(SEED_GOV_OLD);
    let tx = registration_signed_by(&old_sk);
    assert_eq!(tx.tx_type, TransactionType::OracleRegistration);

    let authorized = with_allowlist(&old_pk.to_hex(), || {
        tx.verify_oracle_registration_authorization()
    });

    assert!(
        authorized,
        "CONTROL A FAILED — the governance key did not authorize even as the sole \
         allowlist entry. The fixture is broken, so nothing else in this file is \
         evidence about #2712 in either direction."
    );
}

/// CONTROL B. A REPLACING rotation — the safe action — refuses the retired key.
///
/// This is the anti-vacuity control. If the predicate returned `true`
/// unconditionally, the observation below would be a tautology. It also pins the
/// contrast that makes #2712 a defect rather than a preference: replace and append
/// are one comma apart and produce opposite security outcomes.
#[test]
fn control_b_replacing_rotation_refuses_the_retired_key() {
    let (old_sk, _old_pk) = keypair(SEED_GOV_OLD);
    let (_new_sk, new_pk) = keypair(SEED_GOV_NEW);
    let tx = registration_signed_by(&old_sk);

    let authorized = with_allowlist(&new_pk.to_hex(), || {
        tx.verify_oracle_registration_authorization()
    });

    assert!(
        !authorized,
        "CONTROL B FAILED — the retired key still authorized after a REPLACING \
         rotation, which means this predicate cannot reject anything and the \
         observation below is vacuous. Report the run as VOID, not as a finding."
    );
}

// ---------------------------------------------------------------------------
// OBSERVATION — #2712 itself
// ---------------------------------------------------------------------------

/// THE FINDING. Rotating by APPENDING leaves the retired key authorized.
///
/// `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=<old>,<new>` is the literal expression of
/// "add the new key", which is what a human means during a rotation. The safe
/// action is `=<new>` alone, which requires knowing the old key must be actively
/// removed. Nothing in the variable, the code, or the startup output says so, and
/// `oracle_governance_pubkeys` (`domain_signing.rs:114`) applies no cardinality
/// check — it splits, trims, lowercases, drops empties, and authorizes against
/// whatever remains.
///
/// So this assertion passing is the exposure: after the rotation the operator
/// believes they performed, the retired key still signs valid registrations, and
/// the only record that it is still live is the environment variable itself.
#[test]
fn appending_rotation_leaves_the_retired_key_authorized() {
    let (old_sk, old_pk) = keypair(SEED_GOV_OLD);
    let (_new_sk, new_pk) = keypair(SEED_GOV_NEW);
    let tx = registration_signed_by(&old_sk);

    let appended = format!("{},{}", old_pk.to_hex(), new_pk.to_hex());
    let authorized = with_allowlist(&appended, || tx.verify_oracle_registration_authorization());

    assert!(
        authorized,
        "#2712 REFUTED — the retired key was rejected after an appending rotation, \
         so a mechanism exists that the source read missed. That is a REAL result \
         and must be reported as loudly as a confirmation: update #2712 to say the \
         allowlist is bound by something, and name it."
    );
}

/// The same exposure with the operator's likely formatting: whitespace after the
/// comma, and mixed case pasted from a key ceremony transcript.
///
/// Separated from the assertion above so a formatting-tolerance change cannot
/// silently alter the headline result: if normalisation ever stops being applied,
/// this fails and the one above does not.
#[test]
fn appending_rotation_survives_whitespace_and_case_normalisation() {
    let (old_sk, old_pk) = keypair(SEED_GOV_OLD);
    let (_new_sk, new_pk) = keypair(SEED_GOV_NEW);
    let tx = registration_signed_by(&old_sk);

    let appended = format!("{}, {}", old_pk.to_hex().to_uppercase(), new_pk.to_hex());
    let authorized = with_allowlist(&appended, || tx.verify_oracle_registration_authorization());

    assert!(
        authorized,
        "the retired key was rejected when the appended list carried whitespace or \
         upper-case hex — the exposure is narrower than #2712 claims and depends on \
         exact formatting. Say so in the issue."
    );
}

/// Fail-closed is intact, and #2712 says so — recorded here so the issue's
/// **direction** claim is checked rather than trusted.
///
/// #2712 asserts the risk is WIDENING (the set only grows silently) and that
/// #2523's inverse mistake — an empty allowlist reading as "allow everything" —
/// is not repeated. If this ever fails, the defect is far worse than #2712
/// describes and the issue's severity is wrong.
#[test]
fn control_c_empty_allowlist_authorizes_nothing() {
    let (old_sk, _old_pk) = keypair(SEED_GOV_OLD);
    let tx = registration_signed_by(&old_sk);

    let authorized = with_allowlist("", || tx.verify_oracle_registration_authorization());
    assert!(
        !authorized,
        "an EMPTY allowlist authorized a registration — fail-open. #2712 states the \
         opposite and would be understated, not overstated."
    );

    let authorized_commas = with_allowlist(" , , ", || {
        tx.verify_oracle_registration_authorization()
    });
    assert!(
        !authorized_commas,
        "an allowlist of only separators authorized a registration — the empty-entry \
         filter is not fail-closed."
    );
}
