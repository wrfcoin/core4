//! #3277 S5, core4#3368 review P2-1/P3-2: the boot gate for a chain with
//! blocks, the path every live node takes onto B3 when it restarts on its
//! volume.
//!
//! - On a PERSISTED chain whose block 1 is the live chain's,
//!   `upgrade::arm_block1_protocol_at_boot` installs the base schedule WITH
//!   the bootstrap (`schedule_extension` at `H_ENABLE`) and the pinned
//!   authority. RED control MS5: the gate passes `None` for the block-1 hash.
//! - It fails CLOSED: a storage error, an empty archive, or a lowest block
//!   that is not block 1 refuses boot and installs nothing. RED control:
//!   the old `.ok().flatten()` read, which booted with neither.
//!
//! Each scenario runs in its own re-exec child: the base schedule and the
//! authority are process-wide and first-install-wins.

use std::path::Path;
use std::str::FromStr;

use wrfcoin_native_chain::storage::Storage;
use wrfcoin_native_chain::upgrade::{
    arm_block1_protocol_at_boot, arm_block1_protocol_from_lowest_block,
};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE, SigningProtocolVersion,
    try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    SCHEDULE_EXTENSION_FEATURE, schedule_extension_authority,
};

const LIVE_777_BLOCK1: &str = "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash
const SCENARIO: &str = "WRFCOIN_BOOT_3277_SCENARIO";
const TIP: u64 = 3;

fn identity() -> ChainIdentity {
    ChainIdentity::new("777", "testnet")
}

/// A block like the live chain's at `height`: v1, no activation schedule;
/// block 1 carries the live chain's durable hash.
fn block_at(height: u64, prev: Hash) -> Block {
    let mut block = Block::new(1, prev, Vec::new(), 1, height);
    if height == 1 {
        block
            .metadata
            .insert("protocol_version".to_string(), "v1".to_string());
        block.set_hash(Hash::from_str(LIVE_777_BLOCK1).expect("hash"));
    } else {
        let hash = block.calculate_hash();
        block.set_hash(hash);
    }
    block
}

async fn persist_live_like_chain(path: &Path) {
    let mut storage = Storage::open(path, 100).expect("open");
    storage.persist_genesis_state(&State::new()).await.expect("anchor");
    let mut prev = Hash::default();
    for height in 1..=TIP {
        let block = block_at(height, prev);
        prev = block.hash.clone().expect("hash");
        storage
            .persist_block(block, Vec::new(), None)
            .await
            .expect("persist block");
    }
}

fn assert_nothing_installed() {
    assert!(try_feature_schedule().is_none(), "a refused boot installs no schedule");
    assert!(schedule_extension_authority().is_none(), "a refused boot installs no authority");
}

fn assert_refused(result: Result<SigningProtocolVersion, String>) {
    let err = result.expect_err("boot must refuse (fail-closed)");
    assert!(err.contains("refusing to boot"), "{err}");
    assert_nothing_installed();
}

async fn child(scenario: &str) {
    match scenario {
        "persisted_live_block1" => {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("wrfcoin.redb");
            persist_live_like_chain(&path).await;
            let storage = Storage::open(&path, 100).expect("reopen");
            assert_nothing_installed();

            let version =
                arm_block1_protocol_at_boot(&storage, None, &identity()).expect("boot gate");
            assert_eq!(version, SigningProtocolVersion::V1);
            assert_eq!(
                try_feature_schedule()
                    .and_then(|base| base.activation_height(SCHEDULE_EXTENSION_FEATURE)),
                Some(H_ENABLE_777_MUST_BE_SET_BEFORE_MERGE),
                "boot merges the block-1-keyed bootstrap into the base schedule"
            );
            let authority = schedule_extension_authority().expect("boot installs the authority");
            assert_eq!(authority.genesis_hash(), LIVE_777_BLOCK1);
            assert_eq!(authority.chain_id(), "777");
            assert_eq!(authority.network_id(), "testnet");
            assert_eq!(authority.allowlist().len(), 1);
        },
        "storage_error" => assert_refused(arm_block1_protocol_from_lowest_block(
            Err("simulated redb read failure".to_string()),
            None,
            &identity(),
        )),
        "empty_archive" => {
            assert_refused(arm_block1_protocol_from_lowest_block(Ok(None), None, &identity()))
        },
        "lowest_block_not_block1" => assert_refused(arm_block1_protocol_from_lowest_block(
            Ok(Some(block_at(5, Hash::default()))),
            None,
            &identity(),
        )),
        other => panic!("unknown scenario {other}"),
    }
}

fn run_in_child(test: &str, scenario: &str) {
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", test, "--nocapture"])
        .env(SCENARIO, scenario)
        .status()
        .expect("re-exec child");
    assert!(status.success(), "the isolated boot child ({scenario}) failed");
}

macro_rules! boot_case {
    ($name:ident, $scenario:literal) => {
        #[tokio::test]
        async fn $name() {
            if let Ok(scenario) = std::env::var(SCENARIO) {
                child(&scenario).await;
                return;
            }
            run_in_child(stringify!($name), $scenario);
        }
    };
}

boot_case!(boot_on_a_persisted_live_chain_installs_bootstrap_and_authority, "persisted_live_block1");
boot_case!(boot_refuses_on_a_block1_storage_error, "storage_error");
boot_case!(boot_refuses_on_an_empty_archive, "empty_archive");
boot_case!(boot_refuses_when_the_lowest_block_is_not_block1, "lowest_block_not_block1");
