//! #3277 F4 (review-s2 on #3300): a node that booted on an EMPTY volume
//! resolves its base feature schedule from cut-time configuration
//! (`WRFCOIN_GENESIS_ACTIVATION_SCHEDULE`). Once block 1 is synced, the base
//! must be re-resolved from block 1 itself, so consensus never keeps
//! depending on a node-local setting.
//!
//! Own process: it installs the process-wide base through the uncut path.

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    base_feature_schedule_is_provisional, feature_active, install_uncut_chain_protocol,
    reresolve_base_feature_schedule_from_block1, resolve_uncut_chain_protocol_with_schedule,
    try_feature_schedule, ChainIdentity, ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY,
};

fn block1(schedule: Option<&str>) -> Block {
    let mut block = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    if let Some(raw) = schedule {
        block
            .metadata
            .insert(GENESIS_ACTIVATION_SCHEDULE_KEY.to_string(), raw.to_string());
    }
    block
}

#[test]
fn the_base_is_re_resolved_from_block_1_exactly_once() {
    // Boot on an empty volume with a LOCAL schedule: session_keys at 100.
    let identity = ChainIdentity::new("777", "testnet");
    let resolved =
        resolve_uncut_chain_protocol_with_schedule(None, &identity, Some("100:session_keys"))
            .expect("uncut protocol resolves");
    install_uncut_chain_protocol(resolved);
    assert!(base_feature_schedule_is_provisional());
    assert!(feature_active(ConsensusFeature::SessionKeys, 100));

    // The synced block 1 (produced by the network, not by this node's config)
    // schedules paymaster at 200 and NOT session_keys.
    let schedule =
        reresolve_base_feature_schedule_from_block1(&block1(Some("200:paymaster"))).unwrap();
    assert_eq!(schedule.activation_height("paymaster"), Some(200));
    assert_eq!(schedule.activation_height("session_keys"), None);
    assert!(!base_feature_schedule_is_provisional());

    // Consensus now answers from block 1, not from the local configuration.
    assert!(!feature_active(ConsensusFeature::SessionKeys, 100));
    assert!(feature_active(ConsensusFeature::Paymaster, 200));
    assert_eq!(
        try_feature_schedule().unwrap().activation_height("paymaster"),
        Some(200)
    );

    // Re-resolution happens once: a later "block 1" cannot swap the base.
    let again = reresolve_base_feature_schedule_from_block1(&block1(None)).unwrap();
    assert_eq!(again.activation_height("paymaster"), Some(200));
    assert!(feature_active(ConsensusFeature::Paymaster, 200));
}
