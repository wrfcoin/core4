//! Genesis handshake protocol for P2P network verification.
//!
//! Defines messages exchanged between peers to verify they share the same
//! network_id, chain_id, genesis_hash **and active signing protocol version**
//! before accepting gossipsub subscriptions.
//!
//! # Why the protocol version is on the wire (#2123 round 3 / #2254)
//!
//! Identity alone is not enough. A V1 binary can load the same persisted V2
//! genesis — its metadata map simply ignores the `protocol_version` key it does
//! not know about — advertise identical `network_id`, `chain_id` and
//! `genesis_hash`, pass the handshake, and keep signing V1 preimages while its
//! upgraded peers sign V2.
//!
//! That is #2076 reconstructed: a producer on one preimage version against a
//! verifier expecting another, with nothing erroring and signatures silently
//! failing to verify. The chain does not fault; it just stops making progress,
//! which is indistinguishable from invalid signatures and took 28 hours to
//! diagnose last time.
//!
//! So version disagreement is refused here, loudly, at the same boundary as
//! identity disagreement.
//!
//! # Reach of the guard
//!
//! The refusal is bilateral between two #2123-or-later binaries: each side
//! compares and each side disconnects. Against a *pre*-#2123 binary only the
//! upgraded side can refuse — the old build has no field to compare and cannot
//! be taught one. That is sufficient, because refusing on either side severs
//! the connection, but it is worth stating plainly: this guard constrains the
//! fleet from the moment one node is upgraded, not from the moment all are.
//! Two pre-#2123 binaries are outside its reach entirely, and are covered
//! instead by the startup gate — neither can reach V2 without a genesis that
//! pins it.

use serde::{Deserialize, Serialize};

/// Wire value for the pre-#2123 protocol version.
///
/// A peer running a binary older than #2123 sends no `protocol_version` field
/// at all. Reading that as V1 is not a lenient fallback — it is the truthful
/// reading, because such a peer *is* signing V1.
///
/// This only takes effect on **self-describing** encodings (JSON), where the
/// field can be observed to be absent. The gossip path uses bincode, which is
/// not self-describing and therefore cannot apply a default: a legacy
/// three-field handshake fails to decode outright there. Both outcomes are
/// refusals — an undecodable message never reaches [`GenesisHandshakeMessage::
/// verify_match`] — but only the first names the versions. See
/// `a_legacy_bincode_handshake_does_not_silently_decode_as_v1`, which pins the
/// measured behaviour rather than assuming the JSON case generalises.
fn legacy_protocol_version() -> u32 {
    1
}

/// The fields in which a remote handshake can differ from the local node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeMismatch {
    pub expected_network_id: String,
    pub got_network_id: String,
    pub expected_chain_id: String,
    pub got_chain_id: String,
    pub expected_genesis_hash: String,
    pub got_genesis_hash: String,
    pub expected_protocol_version: u32,
    pub got_protocol_version: u32,
}

impl HandshakeMismatch {
    /// True when the chain identity agrees and *only* the signing protocol
    /// version differs.
    ///
    /// This is the case worth calling out separately in logs: identity-matched
    /// peers that cannot interoperate are a fleet mid-upgrade, not an attacker
    /// or a misconfigured network.
    pub fn is_version_only(&self) -> bool {
        self.expected_network_id == self.got_network_id
            && self.expected_chain_id == self.got_chain_id
            && self.expected_genesis_hash == self.got_genesis_hash
            && self.expected_protocol_version != self.got_protocol_version
    }
}

const ZERO_GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Genesis handshake message exchanged during the P2P handshake.
///
/// This message is sent by each peer to verify chain identity before
/// joining the gossipsub mesh. Mismatches trigger peer disconnection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenesisHandshakeMessage {
    /// Network identifier (e.g., "mainnet", "testnet").
    pub network_id: String,

    /// Chain identifier (e.g., "wrfcoin-main").
    pub chain_id: String,

    /// SHA-256 hash of the genesis block (32 bytes, hex-encoded).
    pub genesis_hash: String,

    /// The signing protocol version this node is actively signing under.
    ///
    /// Defaults to [`legacy_protocol_version`] when absent, so a pre-#2123 peer
    /// is read as the V1 signer it actually is. See the module docs.
    #[serde(default = "legacy_protocol_version")]
    pub protocol_version: u32,
}

impl GenesisHandshakeMessage {
    /// Create a new genesis handshake message.
    ///
    /// `protocol_version` must be the version the node is *actually signing
    /// under* — callers should derive it from
    /// `SigningProtocolVersion::active()` rather than from any separate
    /// configuration, so the advertised value cannot drift from the signed one.
    pub fn new(
        network_id: String,
        chain_id: String,
        genesis_hash: String,
        protocol_version: u32,
    ) -> Self {
        Self {
            network_id,
            chain_id,
            genesis_hash,
            protocol_version,
        }
    }

    /// Serialize to bincode bytes for transmission on the wire.
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bincode bytes received on the wire.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }

    /// Returns true when the handshake is missing a launch-safe chain identity.
    pub fn has_placeholder_identity(&self) -> bool {
        self.network_id.trim().is_empty()
            || self.chain_id.trim().is_empty()
            || self.genesis_hash.trim().is_empty()
            || self.genesis_hash == ZERO_GENESIS_HASH
    }

    /// Verify that `remote` matches this (local) handshake.
    ///
    /// Returns `Ok(())` when both handshakes have non-placeholder identities
    /// and all four fields agree, or `Err(HandshakeMismatch)` so the caller
    /// can log and disconnect.
    ///
    /// `protocol_version` is checked on exactly the same footing as the three
    /// identity fields — not as a warning, not as a soft preference. Two nodes
    /// on the same chain but different signing versions produce signatures the
    /// other cannot verify, so allowing them to peer buys nothing and costs a
    /// silent halt (#2076). Refusing is the correct outcome.
    pub fn verify_match(&self, remote: &GenesisHandshakeMessage) -> Result<(), HandshakeMismatch> {
        if !self.has_placeholder_identity()
            && !remote.has_placeholder_identity()
            && self.network_id == remote.network_id
            && self.chain_id == remote.chain_id
            && self.genesis_hash == remote.genesis_hash
            && self.protocol_version == remote.protocol_version
        {
            Ok(())
        } else {
            Err(HandshakeMismatch {
                expected_network_id: self.network_id.clone(),
                got_network_id: remote.network_id.clone(),
                expected_chain_id: self.chain_id.clone(),
                got_chain_id: remote.chain_id.clone(),
                expected_genesis_hash: self.genesis_hash.clone(),
                got_genesis_hash: remote.genesis_hash.clone(),
                expected_protocol_version: self.protocol_version,
                got_protocol_version: remote.protocol_version,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_handshake_serialization_roundtrip() {
        let msg = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-main".to_string(),
            "a1b2c3d4e5f6".to_string(),
            1,
        );

        let bytes = msg.to_bytes().unwrap();
        let decoded = GenesisHandshakeMessage::from_bytes(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn genesis_handshake_roundtrip_via_serde_json() {
        let msg = GenesisHandshakeMessage::new(
            "testnet".to_string(),
            "wrfcoin-test".to_string(),
            "deadbeef".to_string(),
            1,
        );

        let json = serde_json::to_string(&msg).unwrap();
        let decoded: GenesisHandshakeMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn verify_match_succeeds_for_identical_messages() {
        let local = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-main".to_string(),
            "a1b2c3d4".to_string(),
            1,
        );
        assert!(local.verify_match(&local.clone()).is_ok());
    }

    #[test]
    fn verify_match_fails_when_both_messages_use_zero_genesis_placeholder() {
        let local = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-main".to_string(),
            ZERO_GENESIS_HASH.to_string(),
            1,
        );
        let remote = local.clone();

        let err = local.verify_match(&remote).unwrap_err();

        assert_eq!(err.expected_genesis_hash, ZERO_GENESIS_HASH);
        assert_eq!(err.got_genesis_hash, ZERO_GENESIS_HASH);
    }

    #[test]
    fn verify_match_fails_when_remote_identity_is_empty() {
        let local = GenesisHandshakeMessage::new(
            "testnet".to_string(),
            "wrfcoin-testnet".to_string(),
            "aabbccdd".to_string(),
            1,
        );
        let remote = GenesisHandshakeMessage::new(
            String::new(),
            "wrfcoin-testnet".to_string(),
            "aabbccdd".to_string(),
            1,
        );

        let err = local.verify_match(&remote).unwrap_err();

        assert_eq!(err.expected_network_id, "testnet");
        assert_eq!(err.got_network_id, "");
    }

    #[test]
    fn verify_match_fails_on_network_id_mismatch() {
        let local = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-main".to_string(),
            "a1b2c3d4".to_string(),
            1,
        );
        let remote = GenesisHandshakeMessage::new(
            "testnet".to_string(),
            "wrfcoin-main".to_string(),
            "a1b2c3d4".to_string(),
            1,
        );
        let err = local.verify_match(&remote).unwrap_err();
        assert_eq!(err.expected_network_id, "mainnet");
        assert_eq!(err.got_network_id, "testnet");
    }

    #[test]
    fn verify_match_fails_on_genesis_hash_mismatch() {
        let local = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-main".to_string(),
            "aabbccdd".to_string(),
            1,
        );
        let remote = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-main".to_string(),
            "deadbeef".to_string(),
            1,
        );
        let err = local.verify_match(&remote).unwrap_err();
        assert_eq!(err.expected_genesis_hash, "aabbccdd");
        assert_eq!(err.got_genesis_hash, "deadbeef");
    }

    #[test]
    fn verify_match_fails_on_chain_id_mismatch() {
        let local = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-main".to_string(),
            "aabbccdd".to_string(),
            1,
        );
        let remote = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-fork".to_string(),
            "aabbccdd".to_string(),
            1,
        );
        let err = local.verify_match(&remote).unwrap_err();
        assert_eq!(err.expected_chain_id, "wrfcoin-main");
        assert_eq!(err.got_chain_id, "wrfcoin-fork");
    }

    // ---- signing protocol version on the wire (#2123 round 3 / #2254) ----

    /// Pre-#2123 encoding of the handshake: the three identity fields only.
    #[derive(Serialize)]
    struct LegacyHandshake {
        network_id: String,
        chain_id: String,
        genesis_hash: String,
    }

    fn v2_node() -> GenesisHandshakeMessage {
        GenesisHandshakeMessage::new(
            "testnet".to_string(),
            "wrfcoin-testnet".to_string(),
            "a3301def".to_string(),
            2,
        )
    }

    fn v1_node() -> GenesisHandshakeMessage {
        GenesisHandshakeMessage::new(
            "testnet".to_string(),
            "wrfcoin-testnet".to_string(),
            "a3301def".to_string(),
            1,
        )
    }

    /// **The regression this round is about.** Same `network_id`, same
    /// `chain_id`, same `genesis_hash` — differing only in active signing
    /// version — must not peer, in either direction.
    #[test]
    fn two_nodes_on_one_chain_with_different_protocol_versions_cannot_peer() {
        let v2 = v2_node();
        let v1 = v1_node();

        // Identity really is identical; the version is the only difference.
        assert_eq!(v2.network_id, v1.network_id);
        assert_eq!(v2.chain_id, v1.chain_id);
        assert_eq!(v2.genesis_hash, v1.genesis_hash);
        assert_ne!(v2.protocol_version, v1.protocol_version);

        let v2_sees_v1 = v2.verify_match(&v1).unwrap_err();
        let v1_sees_v2 = v1.verify_match(&v2).unwrap_err();

        assert_eq!(v2_sees_v1.expected_protocol_version, 2);
        assert_eq!(v2_sees_v1.got_protocol_version, 1);
        assert_eq!(v1_sees_v2.expected_protocol_version, 1);
        assert_eq!(v1_sees_v2.got_protocol_version, 2);

        // Both sides refuse: the guard does not depend on which peer dialled.
        assert!(v2_sees_v1.is_version_only());
        assert!(v1_sees_v2.is_version_only());
    }

    #[test]
    fn matching_protocol_versions_still_peer() {
        assert!(v2_node().verify_match(&v2_node()).is_ok());
        assert!(v1_node().verify_match(&v1_node()).is_ok());
    }

    #[test]
    fn an_identity_mismatch_is_not_reported_as_version_only() {
        let local = v2_node();
        let remote = GenesisHandshakeMessage::new(
            "mainnet".to_string(),
            "wrfcoin-testnet".to_string(),
            "a3301def".to_string(),
            2,
        );
        let err = local.verify_match(&remote).unwrap_err();
        assert!(
            !err.is_version_only(),
            "network_id differs — an operator must not be told this is a version skew"
        );
    }

    /// A self-describing encoding (JSON) reads a pre-#2123 peer as the V1
    /// signer it actually is, so the refusal names the versions.
    #[test]
    fn a_legacy_json_handshake_decodes_as_v1_and_is_refused_by_a_v2_node() {
        let legacy = LegacyHandshake {
            network_id: "testnet".to_string(),
            chain_id: "wrfcoin-testnet".to_string(),
            genesis_hash: "a3301def".to_string(),
        };
        let json = serde_json::to_string(&legacy).unwrap();
        let decoded: GenesisHandshakeMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.protocol_version, 1, "absent field means V1");

        let err = v2_node().verify_match(&decoded).unwrap_err();
        assert!(err.is_version_only());
        assert_eq!(err.got_protocol_version, 1);
    }

    /// bincode is **not** self-describing, so `serde(default)` cannot fill in a
    /// field that was never on the wire. This test pins what actually happens
    /// on the gossip encoding rather than assuming the JSON behaviour carries
    /// over: a pre-#2123 handshake fails to decode outright.
    ///
    /// That is still a refusal — an undecodable message never reaches
    /// `verify_match`, so the peer cannot be accepted — but it is a *decode*
    /// refusal, not a named version mismatch. The named mismatch is what two
    /// #2123-or-later binaries get, which is the realistic mid-upgrade case
    /// (chain-wide activation means the fleet shares a build and differs only
    /// in resolved version).
    #[test]
    fn a_legacy_bincode_handshake_does_not_silently_decode_as_v1() {
        let legacy = LegacyHandshake {
            network_id: "testnet".to_string(),
            chain_id: "wrfcoin-testnet".to_string(),
            genesis_hash: "a3301def".to_string(),
        };
        let bytes = bincode::serialize(&legacy).unwrap();

        let decoded = GenesisHandshakeMessage::from_bytes(&bytes);

        assert!(
            decoded.is_err(),
            "a truncated legacy handshake must not decode into a well-formed \
             message; it would have to invent a protocol_version to do so"
        );
    }

    #[test]
    fn protocol_version_survives_the_bincode_round_trip() {
        let msg = v2_node();
        let decoded = GenesisHandshakeMessage::from_bytes(&msg.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded.protocol_version, 2);
        assert_eq!(msg, decoded);
    }
}
