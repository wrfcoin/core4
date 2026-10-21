//! #3277 S5: the live chain's schedule-extension inputs come from its block 1
//! and the binary's compiled-in tables, nothing else.
//!
//! - `install_protocol_from_block1` on a block 1 whose hash is the live
//!   777/testnet genesis installs the base schedule WITH the bootstrap row
//!   (`schedule_extension` at `H_ENABLE`).
//! - `install_schedule_extension_authority_from_block1` then builds the
//!   authority from the bootstrap row (identity) and the D1 pin (allowlist)
//!   — F1: no other input exists.
//! - A re-cut chain that reuses 777/testnet has another block 1 and gets
//!   neither (child process: the installs are first-wins and process-wide).
//!
//! Run: `cargo test -p wrfcoin-shared-protocol --test schedule_extension_bootstrap_3277`

use std::str::FromStr;

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE, feature_active,
    install_protocol_from_block1, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    SCHEDULE_EXTENSION_FEATURE, install_schedule_extension_authority_from_block1,
    schedule_extension_authority,
};

const LIVE_777_BLOCK1: &str = "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash
const RECUT_BLOCK1: &str = "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";
const D1_KEY: &str = "e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00"; // pragma: allowlist secret -- public governance KEY

/// A block 1 like the live chain's: no activation schedule, v1, and (for
/// the test) the given durable hash.
fn block1(hash: &str) -> Block {
    let mut block = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    block
        .metadata
        .insert("protocol_version".to_string(), "v1".to_string());
    block.set_hash(Hash::from_str(hash).expect("hash"));
    block
}

#[test]
fn the_live_block1_installs_the_bootstrap_and_the_pinned_authority() {
    let block = block1(LIVE_777_BLOCK1);
    install_protocol_from_block1(&block).expect("block 1 resolves");
    let base = try_feature_schedule().expect("base installed");
    assert_eq!(
        base.activation_height(SCHEDULE_EXTENSION_FEATURE),
        Some(H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE),
        "the bootstrap row is part of the base"
    );
    assert_eq!(base.entries().len(), 1, "and nothing else: block 1 schedules nothing");
    if H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE > 0 {
        assert!(!feature_active(
            ConsensusFeature::ScheduleExtension,
            H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE - 1
        ));
    }
    assert!(feature_active(
        ConsensusFeature::ScheduleExtension,
        H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE
    ));

    assert!(schedule_extension_authority().is_none(), "nothing before the install");
    let authority =
        install_schedule_extension_authority_from_block1(LIVE_777_BLOCK1, None).expect("777 has a pin");
    assert_eq!(authority.chain_id(), "777");
    assert_eq!(authority.network_id(), "testnet");
    assert_eq!(authority.genesis_hash(), LIVE_777_BLOCK1);
    assert_eq!(authority.allowlist(), [D1_KEY.to_string()]);
    assert_eq!(schedule_extension_authority(), Some(authority));
}

#[test]
fn a_recut_chain_reusing_the_id_gets_neither() {
    const CHILD: &str = "WRFCOIN_SCHEDULE_EXTENSION_RECUT_3277_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "a_recut_chain_reusing_the_id_gets_neither", "--nocapture"])
            .env(CHILD, "1")
            .status()
            .expect("re-exec child");
        assert!(status.success(), "isolated re-cut child failed");
        return;
    }
    install_protocol_from_block1(&block1(RECUT_BLOCK1)).expect("block 1 resolves");
    let base = try_feature_schedule().expect("base installed");
    assert_eq!(base.activation_height(SCHEDULE_EXTENSION_FEATURE), None);
    assert!(!feature_active(ConsensusFeature::ScheduleExtension, u64::MAX));
    assert_eq!(install_schedule_extension_authority_from_block1(RECUT_BLOCK1, None), None);
    assert!(schedule_extension_authority().is_none(), "every extension fails closed");
}
