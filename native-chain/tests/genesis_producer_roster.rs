//! The genesis producer roster: three seated hosts, resolved through the real
//! authority path, and closed against anyone who was not seated. (core4#2971)
//!
//! The set is pinned at `effective_height 0` and every `ApplyProducerSetChange`
//! emitter is `cfg(test)`, so this file is a CUT-TIME input: a host that is not
//! in it at the cut cannot be added later without another cut. Nothing exercised
//! `TESTNET_GENESIS_PRODUCER_SET` before this file — it was compiled but never
//! asserted about.
//!
//! Resolution is deliberately done through `GovernanceProducers` /
//! `ProducerAuthorityPolicy` rather than by reading the JSON and counting: the
//! authorised set at height `H` is "the change with the greatest
//! `effective_height <= H`", and that rule is the thing worth testing. Counting
//! members in a string would pass even if the resolver disagreed.

use wrfcoin_shared_protocol::consensus::producer_authority::{
    GovernanceProducers, ProducerAuthorityDecision, ProducerAuthorityPolicy,
};
use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;

#[path = "../src/bin/launch_api/genesis_producer_pins.rs"]
mod genesis_producer_pins;

use genesis_producer_pins::{
    PAYOUT_ADDRESSES, PLACEHOLDER_PAYOUT_ADDRESS_NOBODY_HAS_CHOSEN,
    PLACEHOLDER_PUBLIC_KEY_NOT_A_REAL_CEREMONY_KEY, TESTNET_GENESIS_PRODUCER_SET, payout_address_for,
};

/// The three hosts ratified on #2976 A1, in roster order.
const EXPECTED_IDS: [&str; 3] = ["n5-node-a", "producer-standby", "mango-mini"];

fn pinned_json() -> serde_json::Value {
    let raw = TESTNET_GENESIS_PRODUCER_SET
        .expect("TESTNET_GENESIS_PRODUCER_SET must be Some — an unpinned set seats nobody");
    serde_json::from_str(raw).expect("the pinned producer set must be valid JSON")
}

/// Parse the pinned artifact into the consensus type.
///
/// Fallible on purpose and `expect`-ing loudly: a roster that does not parse is
/// not a test failure to investigate later, it is a cut that cannot happen.
fn pinned_members() -> Vec<ProducerMember> {
    let v = pinned_json();
    v["members"]
        .as_array()
        .expect("`members` must be an array")
        .iter()
        .map(|m| {
            let hex_key = m["public_key"].as_str().expect("public_key must be a string");
            let bytes = hex::decode(hex_key).expect("public_key must be valid hex");
            let key: [u8; 32] = bytes
                .try_into()
                .expect("public_key must be exactly 32 bytes (ed25519)");
            ProducerMember {
                producer_id: m["producer_id"].as_str().expect("producer_id").to_string(),
                payout_address: m["payout_address"]
                    .as_str()
                    .expect("payout_address")
                    .to_string(),
                public_key: key,
            }
        })
        .collect()
}

fn resolver() -> GovernanceProducers {
    let change = GovernanceProducers::genesis_change(pinned_members());
    GovernanceProducers::try_from_history(&[change])
        .expect("the pinned genesis change must validate — fail-closed history rejects a malformed set")
}

fn authorized_at(height: u64) -> Vec<ProducerMember> {
    match resolver().authorized_producers_at(height) {
        ProducerAuthorityDecision::Enforced(m) => m,
        ProducerAuthorityDecision::NotEnforced => panic!(
            "enforcement must be active at height {height}: the roster is seeded at \
             effective_height 0, so there is no height at which nobody is authorised. \
             NotEnforced here would mean any key could produce."
        ),
    }
}

#[test]
fn exactly_three_producers_are_authorized_at_height_0_and_1() {
    // Height 0 is the empty-chain state and height 1 is the first block that can
    // be gated. Both are asserted because `effective_height 0` is a field on the
    // CHANGE, not a block: "effective at 0" must already be authoritative when
    // block 1 is validated, and an off-by-one there would leave block 1 ungated.
    for height in [0u64, 1u64] {
        let members = authorized_at(height);

        assert_eq!(
            members.len(),
            3,
            "expected exactly 3 seated producers at height {height}, got {}: {:?}",
            members.len(),
            members.iter().map(|m| &m.producer_id).collect::<Vec<_>>()
        );

        let ids: Vec<&str> = members.iter().map(|m| m.producer_id.as_str()).collect();
        assert_eq!(
            ids, EXPECTED_IDS,
            "the seated roster at height {height} is not the ratified three-host set (#2976 A1)"
        );
    }
}

#[test]
fn a_fourth_unseated_key_is_not_authorized_at_any_height() {
    // A roster that admits an unseated producer is worse than no roster: it
    // reads as an access-control decision while enforcing nothing.
    //
    // This key is not derived from any ceremony; it is simply a 32-byte value
    // that is not in the pinned set.
    let unseated: [u8; 32] = [0x7c; 32];

    let seated_keys: Vec<[u8; 32]> = pinned_members().iter().map(|m| m.public_key).collect();
    assert!(
        !seated_keys.contains(&unseated),
        "precondition: the probe key must not coincidentally be a seated key"
    );

    for height in [0u64, 1u64, 2u64, 10_000u64] {
        let members = authorized_at(height);
        assert!(
            !members.iter().any(|m| m.public_key == unseated),
            "an unseated key resolved as authorized at height {height} — the roster is open"
        );
    }
}

#[test]
fn every_seated_member_has_a_distinct_key_and_payout_address() {
    // Two members sharing a key makes the two hosts indistinguishable to the
    // authority check; two sharing a payout address silently redirects one
    // host's coinbase to the other's.
    let members = pinned_members();

    for i in 0..members.len() {
        for j in (i + 1)..members.len() {
            assert_ne!(
                members[i].public_key, members[j].public_key,
                "{} and {} share a public key",
                members[i].producer_id, members[j].producer_id
            );
            assert_ne!(
                members[i].payout_address, members[j].payout_address,
                "{} and {} share a payout address",
                members[i].producer_id, members[j].producer_id
            );
            assert_ne!(
                members[i].producer_id, members[j].producer_id,
                "duplicate producer_id"
            );
        }
    }
}

#[test]
fn payout_addresses_match_the_pinned_set() {
    // `PAYOUT_ADDRESSES` exists so an operator check (docker-compose.cut-miner.yml)
    // can resolve THIS host's payout without a JSON parser. A lookup table that
    // drifts from the consensus artifact is worse than no table: it would answer
    // confidently and wrongly, and a wrong payout halts the chain at its first
    // mine (it has, at height 2). This is the anti-drift assertion.
    let members = pinned_members();

    assert_eq!(
        PAYOUT_ADDRESSES.len(),
        members.len(),
        "PAYOUT_ADDRESSES has {} entries but the pinned set has {}",
        PAYOUT_ADDRESSES.len(),
        members.len()
    );

    for m in &members {
        let looked_up = payout_address_for(&m.producer_id).unwrap_or_else(|| {
            panic!(
                "payout_address_for({:?}) returned None for a SEATED member — an operator \
                 check would be unable to resolve its own payout and would have to guess",
                m.producer_id
            )
        });
        assert_eq!(
            looked_up, m.payout_address,
            "payout address drift for {}: table says {}, pinned set says {}",
            m.producer_id, looked_up, m.payout_address
        );
    }

    assert!(
        payout_address_for("not-a-seated-host").is_none(),
        "payout_address_for must return None for an unseated id rather than a default — \
         there is no safe default, and a caller that cannot resolve its id must refuse to mine"
    );
}

/// **Green since 2026-09-10**, when `mango-mini`'s real SoftHSM-backed producer
/// key and owner-designated payout address replaced both placeholders; it was
/// RED by design while the seat carried them.
///
/// The mechanism stands: a placeholder must not be able to reach a real cut. A
/// placeholder that failed silently would be worse than an absent entry,
/// because the roster would look complete while seating a key nobody controls.
/// If a seat is ever pinned with a placeholder again, this goes red again.
///
/// **Do not delete this test, and do not delete the placeholder constants
/// either.** An earlier revision of this comment said to delete the constant,
/// which would have broken this target's compilation — the test imports both
/// and references them below, so clearing the placeholder would have produced
/// an unresolved import instead of a green test (#2971 review). The constants
/// stay as retired sentinels: after the ceremony their job is to prove those
/// exact values never come back from a revert, a cherry-pick, or a copy out of
/// git history.
#[test]
fn no_placeholder_key_may_reach_a_real_cut() {
    let raw = TESTNET_GENESIS_PRODUCER_SET.expect("pinned set must be Some");

    let placeholder_members: Vec<String> = pinned_members()
        .iter()
        .filter(|m| hex::encode(m.public_key) == PLACEHOLDER_PUBLIC_KEY_NOT_A_REAL_CEREMONY_KEY)
        .map(|m| m.producer_id.clone())
        .collect();

    let placeholder_payouts: Vec<String> = pinned_members()
        .iter()
        .filter(|m| m.payout_address == PLACEHOLDER_PAYOUT_ADDRESS_NOBODY_HAS_CHOSEN)
        .map(|m| m.producer_id.clone())
        .collect();

    // Both halves are checked here on purpose. A placeholder KEY makes a slot
    // that cannot sign — obvious the moment it is asked to. A placeholder
    // PAYOUT lets the host sign perfectly and halts the chain at its first
    // mine. The second is the quieter failure, so it must not depend on
    // somebody remembering to look for it.
    assert!(
        !raw.contains(PLACEHOLDER_PAYOUT_ADDRESS_NOBODY_HAS_CHOSEN),
        "PLACEHOLDER PAYOUT ADDRESS IS SEATED IN THE GENESIS ROSTER.\n\n\
         member(s) holding it: {placeholder_payouts:?}\n\n\
         Nobody has chosen this address; it was invented so the entry would \
         parse. A seated host whose payout_address is wrong signs fine and then \
         halts the chain at its first mine (WrongPayoutAddress) — this has \
         happened at height 2. The owner must designate the real address before \
         the cut."
    );

    assert!(
        !raw.contains(PLACEHOLDER_PUBLIC_KEY_NOT_A_REAL_CEREMONY_KEY),
        "PLACEHOLDER KEY IS SEATED IN THE GENESIS ROSTER.\n\n\
         member(s) holding it: {placeholder_members:?}\n\n\
         This key is not from any ceremony and nobody possesses its private half. \
         Seating it would create a producer slot that can never sign, permanently, \
         because the set is immutable after the cut.\n\n\
         This failure is the intended state until the ceremony for that host has \
         been run (owner-only, docs/security/PRODUCER_KEY_CEREMONY.md). Replace the \
         key in genesis_producer_pins.rs and remove the placeholder constant."
    );
}
