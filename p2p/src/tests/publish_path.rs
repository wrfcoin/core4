//! Regression tests for publish-path size guards in `node.rs`.

use std::time::Duration;

use tokio::time::timeout;

use crate::{message::ContentRef, P2pCommand, P2pConfig, P2pNode};
use rand::SeedableRng;
use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

fn test_station_key() -> PrivateKey {
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    PrivateKey::generate(&mut rng)
}

const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const HARD_CAP_BYTES: usize = 1_048_576;

fn valid_content_ref() -> ContentRef {
    ContentRef {
        data_cid: "QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string(),
        data_hash: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
            .to_string(),
        storage_provider: "ipfs".to_string(),
        data_size: 52_428_800,
    }
}

async fn wait_for_rejections(handle: &crate::node::P2pHandle, expected: u64) {
    timeout(TEST_TIMEOUT, async {
        loop {
            let stats = handle.stats();
            if stats.messages_rejected >= expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("timed out waiting for messages_rejected to increment");
}

#[tokio::test]
async fn publish_path_rejects_oversized_payloads_and_counts_rejections() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let cfg = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().expect("valid multiaddr")],
        bootstrap_peers: Vec::new(),
        inline_data_max: 65_536,
        ..P2pConfig::default()
    };
    let handle = P2pNode::start(cfg).await.expect("node should start");

    // Case 1: Inline cap -- WeatherData payload exceeds inline_data_max.
    handle
        .command_tx
        .send(P2pCommand::BroadcastWeatherData {
            station_id: "station-inline".to_string(),
            data: vec![0xAA; 70_000],
            station_signing_key: test_station_key(),
        })
        .await
        .expect("send inline-oversized weather data");

    wait_for_rejections(&handle, 1).await;
    let stats_after_inline = handle.stats();
    assert_eq!(stats_after_inline.messages_rejected, 1);
    assert_eq!(stats_after_inline.bytes_outbound, 0);
    assert_eq!(stats_after_inline.outbound_weather.bytes, 0);

    // Case 2: Hard cap -- WeatherDataV2 bypasses inline check via content_ref,
    // but serialized message must still be dropped above 1 MiB.
    handle
        .command_tx
        .send(P2pCommand::BroadcastWeatherDataV2 {
            station_id: "station-hardcap".to_string(),
            data: vec![0xBB; HARD_CAP_BYTES],
            content_ref: Some(valid_content_ref()),
            station_signing_key: test_station_key(),
        })
        .await
        .expect("send hard-cap-oversized weather data v2");

    wait_for_rejections(&handle, 2).await;
    let stats_after_hard_cap = handle.stats();
    assert_eq!(stats_after_hard_cap.messages_rejected, 2);
    assert_eq!(stats_after_hard_cap.bytes_outbound, 0);
    assert_eq!(stats_after_hard_cap.outbound_weather.bytes, 0);

    let _ = handle.command_tx.send(P2pCommand::Shutdown).await;
}
