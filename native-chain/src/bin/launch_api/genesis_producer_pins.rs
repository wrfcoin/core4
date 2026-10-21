// This module is `#[path]`-included by four targets — `launch_api`,
// `producer_key_pop`, `oracle_key_pop` and the `genesis_producer_roster`
// integration test — and each uses a different subset of it. The payout table
// and the placeholder sentinels are read by the roster test; the binaries do
// not touch them, so without this every one of them emits `dead_code` and the
// `cargo clippy --all-targets --all-features -- -D warnings` gate fails on
// items that are deliberately consumed elsewhere.
#![allow(dead_code)]

//! Canonical producer set compiled into a fresh testnet genesis.
//!
//! Kept in a dependency-free module so both `launch_api` and the physical-key
//! proof-of-possession harness compile the exact same bytes. Do not duplicate
//! these public keys in an operator tool: a proof against a copy would not
//! prove possession of the keys genesis actually seeds.
//!
//! # The roster is three hosts (core4#2971, ratified on #2976 A1)
//!
//! node host + this workstation + mango mac mini. The set is pinned from genesis at
//! `effective_height 0`, and every `ApplyProducerSetChange` emitter is
//! `cfg(test)`, so a host not seated at the cut cannot be added afterwards
//! without another cut. That makes this file a cut-time input, not a runtime
//! config: getting it wrong is not a restart, it is a re-cut.
//!
//! # Payout addresses are per-host and a mismatch halts the chain
//!
//! A node may pay ONLY its own entry's `payout_address`. Paying another
//! member's address is `WrongPayoutAddress` and the chain stops at that block —
//! this has happened before, at height 2. [`payout_address_for`] exists so
//! `docker-compose.cut-miner.yml` can be checked against this file at config
//! time instead of at first mine; see [`PAYOUT_ADDRESSES`].

/// Public key that is NOT a real ceremony key — **retired sentinel** since 2026-09-10.
///
/// `mango-mini` had no provisioned key while its seat was first pinned, so this
/// deterministically-derived, structurally valid ed25519 point let the roster
/// PARSE and the resolver tests exercise a three-member set. It was never
/// seated at a real cut. On 2026-09-10 the owner generated the host's real
/// producer key inside its SoftHSM token (PKCS#11 object `wrfcoin-producer-mango`,
/// `docs/security/PRODUCER_KEY_CEREMONY.md`, third-seat note: token-side EDDSA
/// proof of possession, `EC_POINT` attribute equal to the pinned hex) and that
/// key is seated below.
///
/// Its presence in the set is a hard test failure: see
/// `native-chain/tests/genesis_producer_roster.rs::
/// no_placeholder_key_may_reach_a_real_cut`. The constant stays so that test
/// keeps proving the invented value never comes back — a placeholder that
/// failed silently would be worse than no entry, because the roster would look
/// complete.
///
/// # Do NOT delete this constant when the ceremony lands
///
/// Replace the key in [`TESTNET_GENESIS_PRODUCER_SET`] and leave this constant
/// where it is. It is a **retired sentinel**, not scratch scaffolding:
///
/// - The guard test reads it. Deleting it while keeping the test — which an
///   earlier revision of this doc actually instructed — breaks the test
///   target's compilation, so clearing the placeholder would turn a green test
///   into an unresolved import (#2971 review).
/// - It keeps working after the ceremony. Its job then is to prove this exact
///   value never comes *back* — from a revert, a cherry-pick, or a copy out of
///   git history. Deleting it would throw that away at the precise moment the
///   value stops being obviously wrong to a reader.
///
/// It is two string literals that no production path reads.
pub const PLACEHOLDER_PUBLIC_KEY_NOT_A_REAL_CEREMONY_KEY: &str =
    "42307b1626e83a16b560deadfcd435bf0e9008bf38fee1c763e61a59f5623ecd"; // pragma: allowlist secret -- placeholder, not a key in use

/// Payout address that NOBODY HAS CHOSEN — **retired sentinel** since 2026-09-10.
///
/// `mango-mini` originally had no owner-designated payout address, and this
/// value was invented to give the entry a well-formed field so the roster
/// parsed; it is not an address anyone controls. The owner designated the real
/// address (`0xc536d202…4a990`, ceremony packet Step 5: software ed25519 key,
/// private PEM verified in the password manager, local copies shredded) on
/// 2026-09-10 and it is seated below. This constant stays so the guard keeps
/// proving the invented value never comes back.
///
/// It is called out separately from the placeholder KEY because the two fail
/// differently and the payout one is easier to miss. A wrong key makes a slot
/// that cannot sign — visible immediately. A wrong payout address lets the host
/// sign fine and then halts the chain at its FIRST MINE with
/// `WrongPayoutAddress`, which has happened before at height 2. Seating an
/// invented address would mint that outage into immutable genesis state.
///
/// How a real one is made (the same way the two seated addresses were, on
/// 2026-07-24): a software ed25519 keypair generated offline with openssl, the
/// address being `0x` + the first 20 bytes of SHA3-256 over the raw 32-byte
/// public key, lowercase hex — `docs/security/PRODUCER_CEREMONY_PACKET.md`,
/// Step 5, including the store-then-re-derive-then-shred order. NOT
/// `wrfcoin wallet new-address`, which hashes differently and would misdirect
/// every reward. The private PEM is the only thing that can ever spend the
/// rewards and the set is immutable after the cut, so the password-manager
/// copy must be verified before the local file is destroyed.
///
/// Guarded by the same test as the placeholder key. It is also a **retired
/// sentinel** on the same terms — see
/// [`PLACEHOLDER_PUBLIC_KEY_NOT_A_REAL_CEREMONY_KEY`]: replace the address in
/// [`TESTNET_GENESIS_PRODUCER_SET`], leave this constant in place.
pub const PLACEHOLDER_PAYOUT_ADDRESS_NOBODY_HAS_CHOSEN: &str =
    "0x2f1a7c04d3b95e86ac41d0f6b8e7259c3a5d84b1";

/// `(producer_id, payout_address)` for every seated member, in roster order.
///
/// Duplicated deliberately in a machine-readable form: the JSON below is the
/// consensus artifact, and this is the lookup table an operator check can use
/// without a JSON parser. `payout_addresses_match_the_pinned_set` asserts the
/// two never drift apart.
pub const PAYOUT_ADDRESSES: &[(&str, &str)] = &[
    ("n5-node-a", "0x680d51f7543ca70029866956376306d47ce60ed0"),
    ("producer-standby", "0x39ee2e595a55c71256282fe5c42a398594985b1a"),
    ("mango-mini", "0xc536d2026661522cb8631b3c723d4af43784a990"),
];

/// The payout address this host must mine to, by `producer_id`.
///
/// Returns `None` for an unseated id rather than a default — there is no safe
/// default. A caller that cannot resolve its own id is misconfigured and must
/// refuse to mine, not guess.
pub fn payout_address_for(producer_id: &str) -> Option<&'static str> {
    PAYOUT_ADDRESSES
        .iter()
        .find(|(id, _)| *id == producer_id)
        .map(|(_, addr)| *addr)
}

pub const TESTNET_GENESIS_PRODUCER_SET: Option<&str> = Some(concat!(
    r#"{
  "version": 1,
  "members": [
    {
      "producer_id": "n5-node-a",
      "payout_address": "0x680d51f7543ca70029866956376306d47ce60ed0",
      "public_key": ""#,
    "1e5180e7add06bbe07cf2698ec9d4882ae1cfb54b13de67bae92a4e625ceec1a", // pragma: allowlist secret -- public genesis key
    r#""
    },
    {
      "producer_id": "producer-standby",
      "payout_address": "0x39ee2e595a55c71256282fe5c42a398594985b1a",
      "public_key": ""#,
    "cd77a1f439ec6896727f585e81228795c8008c8f8db7763bb9c42afa8d119aaa", // pragma: allowlist secret -- public genesis key
    r#""
    },
    {
      "producer_id": "mango-mini",
      "payout_address": "0xc536d2026661522cb8631b3c723d4af43784a990",
      "public_key": ""#,
    "1f7a4ec7699e361390cb89297a2b4441e676b17c95b122063eda25b1f64e0f1e", // pragma: allowlist secret -- public genesis key (mango-mini, SoftHSM PKCS#11 object `wrfcoin-producer-mango`, 2026-09-10)
    r#""
    }
  ]
}"#
));
