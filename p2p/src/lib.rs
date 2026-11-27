//! WRFCoin P2P networking crate.
//!
//! Provides a libp2p-based gossipsub overlay for propagating blocks,
//! transactions, and weather-data observations across the WRFCoin network.
//!
//! # Quick start
//!
//! ```rust,no_run
//! # async fn demo() -> wrfcoin_p2p::error::P2pResult<()> {
//! use wrfcoin_p2p::{P2pNode, P2pConfig, P2pCommand};
//!
//! let config = P2pConfig::default();
//! let mut handle = P2pNode::start(config).await?;
//!
//! // Broadcast a block.
//! handle.command_tx.send(P2pCommand::BroadcastBlock(vec![1,2,3])).await
//!     .map_err(|_| wrfcoin_p2p::P2pError::ChannelClosed)?;
//! # Ok(())
//! # }
//! ```

pub mod announce;
mod behaviour;
mod bootstrap_redial;
mod gossip_watchdog;
pub mod bonded_fetch;
pub mod bonded_fetch_serve;
pub mod config;
pub mod da_challenge;
pub mod discovered;
pub mod sync_surface;
pub mod error;
pub mod genesis_handshake;
pub mod handshake_guard;
mod handshake_redelivery;
pub mod message;
pub mod metrics;
pub mod node;
pub mod quota;
mod peer;
pub mod preauth_budget;
pub mod identity;
mod pending_broadcast;
pub mod scoring;
pub mod slots;
pub mod sync;
pub mod sync_limits;

#[cfg(test)]
mod tests;

// ---- Convenience re-exports ------------------------------------------------

pub use config::{expected_genesis_hash, P2pConfig};
pub use discovered::{DiscoveredPeer, DiscoveredPeers, DEFAULT_DISCOVERED_PEERS_CAPACITY};
pub use da_challenge::{
    DataAvailabilityChallenge, DataAvailabilityChallengeResponse, DaChallengeError,
    compute_proof, generate_challenge, verify_challenge_response,
    CHALLENGE_FRAGMENT_MAX_BYTES, CHALLENGE_TIMEOUT_SECS,
};
pub use error::{P2pError, P2pResult};
pub use genesis_handshake::{GenesisHandshakeMessage, HandshakeMismatch};
pub use identity::load_or_generate_keypair;
pub use message::{ContentRef, TopicManager, WrfcoinMessage};
pub use metrics::NetworkStats;
pub use sync_limits::{ServeDecision, ServeRejection, SyncServeGovernor, SyncServeQuota};
pub use node::{InitialChainState, P2pClient, P2pCommand, P2pEvent, P2pHandle, P2pNode};

#[cfg(test)]
mod integration_target_registration {
    /// #2888: a new `tests/*.rs` target must be registered in
    /// `scripts/integration_test_ratchet.json` or nothing in CI ever runs it.
    /// This fails at `cargo test -p <this crate>`, i.e. at the point of
    /// addition, not in a later PR's ratchet.
    #[test]
    fn every_integration_target_under_tests_is_in_the_ratchet_inventory() {
        wrfcoin_shared_protocol::integration_inventory::assert_registered(
            env!("CARGO_MANIFEST_DIR"),
            env!("CARGO_PKG_NAME"),
        );
    }
}

#[cfg(test)]
mod dependency_floor_libp2p_057 {
    //! libp2p 0.57 removed two advisory packages from the lock: yamux 0.12
    //! (GHSA-vxx9-2994-q338, compiled unconditionally by libp2p-yamux 0.47)
    //! and the yanked core2 0.4.0 (RUSTSEC-2026-0105, via multihash 0.19.3).
    //! Fail if either comes back.
    const LOCK: &str = include_str!("../../Cargo.lock");

    fn versions<'a>(lock: &'a str, name: &str) -> Vec<&'a str> {
        lock.split("[[package]]")
            .filter_map(|package| {
                let mut package_name = None;
                let mut version = None;
                for line in package.lines() {
                    if let Some(value) = line.strip_prefix("name = \"") {
                        package_name = value.strip_suffix('"');
                    }
                    if let Some(value) = line.strip_prefix("version = \"") {
                        version = value.strip_suffix('"');
                    }
                }
                (package_name == Some(name)).then_some(version).flatten()
            })
            .collect()
    }

    #[test]
    fn no_yamux_012_in_lock() {
        let yamux = versions(LOCK, "yamux");
        assert!(
            !yamux.is_empty(),
            "lock scan found no yamux at all: parser is blind"
        );
        assert!(
            yamux.iter().all(|v| !v.starts_with("0.12.")),
            "yamux 0.12 is back: {yamux:?}"
        );
    }

    #[test]
    fn no_yanked_core2_in_lock() {
        assert!(versions(LOCK, "core2").is_empty(), "yanked core2 is back");
    }

    #[test]
    fn lock_parser_keeps_every_version_and_exact_names() {
        let fixture = "[[package]]\nname = \"yamux\"\nversion = \"0.12.1\"\n\n[[package]]\nname = \"libp2p-yamux\"\nversion = \"0.47.0\"\n\n[[package]]\nname = \"yamux\"\nversion = \"0.14.0\"\n";
        assert_eq!(versions(fixture, "yamux"), ["0.12.1", "0.14.0"]);
        assert!(versions(fixture, "core2").is_empty());
    }
}
