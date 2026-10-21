//! Integration tests for P2P weather gossip authentication (#862).
//!
//! Tests verify that:
//! 1. A valid station signature results in `P2pEvent::WeatherData` on the receiver.
//! 2. Missing, replayed, and too-fast nonces are rejected before signature verification.
//! 3. A tampered signature is rejected (the shared verifier returns false).
//! 4. A signature from key-A presented with pubkey-B is rejected.
//! 5. Messages with empty/missing signature or pubkey fields are rejected.
//!
//! Tests 2–4 exercise the admission/verifier logic directly (unit-level) since
//! the P2pCommand API always derives pubkey from the provided private key and
//! does not expose a way to inject pre-crafted raw gossipsub payloads.

use std::time::{Duration, Instant};

use libp2p::{PeerId, identity::Keypair};
use rand::SeedableRng;
use tokio::time::timeout;

use super::helpers::allocate_port;
use crate::{
    P2pCommand, P2pConfig, P2pEvent, P2pNode,
    message::ContentRef,
    node::{
        WeatherDataAdmission, WeatherDataAdmissionError, verify_weather_gossip_signature,
        verify_weather_gossip_signature_v2, weather_v2_data_hash_hex,
    },
};
use wrfcoin_shared_protocol::crypto::domain_signing::{
    DomainSigningAction, DomainSigningContext, canonical_signing_message, payload_digest_hex,
    signing_chain_id, signing_network_id,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

/// How long we wait for a valid event to arrive.
const RECV_TIMEOUT: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_key(seed: u64) -> PrivateKey {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    PrivateKey::generate(&mut rng)
}

/// Run the same verification closure used in the WeatherData receive handler.
fn verify_weather_gossip(
    station_id: &str,
    data: &[u8],
    timestamp: i64,
    nonce: &str,
    station_signature: &str,
    station_pubkey: &str,
) -> bool {
    verify_weather_gossip_signature(
        station_id,
        data,
        timestamp,
        nonce,
        station_signature,
        station_pubkey,
    )
}

/// Sign a weather gossip payload with the given key and return (sig_hex, pubkey_hex).
fn sign_weather(
    key: &PrivateKey,
    station_id: &str,
    data: &[u8],
    timestamp: i64,
    nonce: &str,
) -> (String, String) {
    let data_hash = hex::encode(blake3::hash(data).as_bytes());
    let payload_digest = payload_digest_hex(&(station_id, &data_hash));
    // #2065: sign-side resolves the same runtime identity the production verifier
    // uses (signing_chain_id/network_id), so both agree regardless of env state and
    // this test is immune to WRFCOIN_NETWORK_ID bleed from concurrent config tests.
    let cid = signing_chain_id();
    let nid = signing_network_id();
    let context = DomainSigningContext {
        action: DomainSigningAction::WeatherDataGossip,
        signer_id: station_id,
        timestamp,
        nonce: Some(nonce),
        chain_id: &cid,
        network_id: &nid,
    };
    let signing_msg = canonical_signing_message(&context, &payload_digest);
    let sig = key.sign(signing_msg.as_bytes());
    let pubkey_hex = hex::encode(PublicKey::from_private_key(key).as_bytes());
    (sig.to_hex(), pubkey_hex)
}

/// Start a two-node mesh and wait for gossipsub mesh formation.
/// Returns `(handle_a, handle_b)` where A is the sender and B is the receiver.
async fn two_node_mesh() -> (crate::node::P2pHandle, crate::node::P2pHandle) {
    let port_b = allocate_port().await;
    let addr_b: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{port_b}").parse().unwrap();

    let cfg_b = P2pConfig {
        listen_addresses: vec![addr_b.clone()],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let mut handle_b = P2pNode::start(cfg_b).await.expect("node B should start");

    let addr_b_with_peer = addr_b.with(libp2p::multiaddr::Protocol::P2p(handle_b.local_peer_id));

    let cfg_a = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        bootstrap_peers: vec![addr_b_with_peer],
        max_peers_per_subnet: 0,
        ..super::helpers::peering_config()
    };
    let handle_a = P2pNode::start(cfg_a).await.expect("node A should start");

    // Wait for peer connection + gossipsub mesh formation.
    super::helpers::wait_for_gossip_mesh(&mut handle_b, 1, Duration::from_secs(30)).await;

    (handle_a, handle_b)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A correctly signed WeatherData message should arrive as P2pEvent::WeatherData.
#[tokio::test]
async fn valid_signature_accepted() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let (handle_a, mut handle_b) = two_node_mesh().await;

    let station_key = make_key(1);
    let expected_pubkey = hex::encode(PublicKey::from_private_key(&station_key).as_bytes());
    let station_id = "station-valid".to_string();
    let data = b"temp=22.5,hum=60".to_vec();

    handle_a
        .command_tx
        .send(P2pCommand::BroadcastWeatherData {
            station_id: station_id.clone(),
            data: data.clone(),
            station_signing_key: station_key,
        })
        .await
        .expect("send BroadcastWeatherData");

    let received = timeout(RECV_TIMEOUT, async {
        loop {
            match handle_b.event_rx.recv().await {
                Some(P2pEvent::WeatherData {
                    station_id: sid,
                    data: d,
                    station_pubkey: pk,
                }) => {
                    return (sid, d, pk);
                },
                Some(_) => continue,
                None => panic!("node B event channel closed"),
            }
        }
    })
    .await
    .expect("node B should receive WeatherData within timeout");

    assert_eq!(received.0, station_id);
    assert_eq!(received.1, data);
    assert_eq!(received.2, expected_pubkey);

    let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle_b.command_tx.send(P2pCommand::Shutdown).await;
}

/// A tampered signature should be rejected by the verification closure.
#[tokio::test]
async fn invalid_signature_rejected() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let station_key = make_key(2);
    let station_id = "station-tampered";
    let data = b"temp=18.0,hum=55";
    let timestamp = 1_700_000_000i64;
    let nonce = "00112233445566778899aabbccddeeff";

    let (sig_hex, pubkey_hex) = sign_weather(&station_key, station_id, data, timestamp, nonce);

    // Tamper: flip first byte of the signature.
    let mut sig_bytes = hex::decode(&sig_hex).unwrap();
    sig_bytes[0] ^= 0xFF;
    let tampered_sig_hex = hex::encode(&sig_bytes);

    let valid = verify_weather_gossip(
        station_id,
        data,
        timestamp,
        nonce,
        &tampered_sig_hex,
        &pubkey_hex,
    );
    assert!(!valid, "tampered signature should be rejected");

    // Sanity: the untampered signature should pass.
    let valid_untampered =
        verify_weather_gossip(station_id, data, timestamp, nonce, &sig_hex, &pubkey_hex);
    assert!(valid_untampered, "original signature should be valid");
}

/// A signature from key-A presented with pubkey-B should be rejected.
#[tokio::test]
async fn wrong_pubkey_rejected() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let key_a = make_key(10);
    let key_b = make_key(20);

    let station_id = "station-wrong-key";
    let data = b"temp=19.5";
    let timestamp = 1_700_000_100i64;
    let nonce = "102132435465768798a9babbdcddedef";

    // Sign with key_a but advertise pubkey_b.
    let (sig_from_a, _pubkey_a) = sign_weather(&key_a, station_id, data, timestamp, nonce);
    let pubkey_b_hex = hex::encode(PublicKey::from_private_key(&key_b).as_bytes());

    let valid = verify_weather_gossip(
        station_id,
        data,
        timestamp,
        nonce,
        &sig_from_a,
        &pubkey_b_hex,
    );
    assert!(
        !valid,
        "signature from key-A with pubkey-B should be rejected"
    );

    // Sanity: correct pairing should pass.
    let (sig_a, pubkey_a_hex) = sign_weather(&key_a, station_id, data, timestamp, nonce);
    let valid_correct =
        verify_weather_gossip(station_id, data, timestamp, nonce, &sig_a, &pubkey_a_hex);
    assert!(valid_correct, "correct key pair should be valid");
}

#[test]
fn weather_admission_rejects_missing_replayed_and_rate_limited_before_verify() {
    let peer = PeerId::from(Keypair::generate_ed25519().public());
    let mut admission = WeatherDataAdmission::default();
    let now = Instant::now();

    assert_eq!(
        admission.admit(peer, "station-replay", None, now),
        Err(WeatherDataAdmissionError::MissingNonce)
    );
    assert_eq!(
        admission.admit(peer, "station-replay", Some("not-a-hex-nonce"), now),
        Err(WeatherDataAdmissionError::InvalidNonce)
    );

    let nonce = "abcdef0123456789abcdef0123456789";
    assert_eq!(
        admission.admit(peer, "station-replay", Some(nonce), now),
        Ok(())
    );
    assert_eq!(
        admission.admit(
            peer,
            "station-replay",
            Some(nonce),
            now + Duration::from_secs(1),
        ),
        Err(WeatherDataAdmissionError::Replay)
    );

    let fresh_nonce = "00112233445566778899aabbccddeeff";
    assert_eq!(
        admission.admit(
            peer,
            "station-replay",
            Some(fresh_nonce),
            now + Duration::from_millis(1),
        ),
        Err(WeatherDataAdmissionError::RateLimited)
    );
}

#[test]
fn weather_signature_is_bound_to_nonce() {
    let station_key = make_key(70);
    let station_id = "station-nonce-bound";
    let data = b"temp=20.0";
    let timestamp = 1_700_000_150i64;
    let nonce = "cafebabecafebabecafebabecafebabe";
    let other_nonce = "feedfacefeedfacefeedfacefeedface";

    let (sig_hex, pubkey_hex) = sign_weather(&station_key, station_id, data, timestamp, nonce);

    assert!(
        verify_weather_gossip(station_id, data, timestamp, nonce, &sig_hex, &pubkey_hex),
        "signature should verify with the signed nonce"
    );
    assert!(
        !verify_weather_gossip(
            station_id,
            data,
            timestamp,
            other_nonce,
            &sig_hex,
            &pubkey_hex,
        ),
        "replaying the signature under another nonce must fail"
    );
}

/// Each invalid WeatherData message from the same peer increments its `SyncPeerScore`
/// invalid-weather-sig counter; after 3 violations the peer is marked as banned.
#[test]
fn peer_score_increments_on_invalid_sig() {
    use crate::scoring::SyncPeerScore;

    let mut score = SyncPeerScore::default();

    assert_eq!(score.invalid_weather_sigs(), 0);
    assert!(!score.is_banned());

    score.record_invalid_weather_sig();
    assert_eq!(score.invalid_weather_sigs(), 1);
    assert!(!score.is_banned());

    score.record_invalid_weather_sig();
    assert_eq!(score.invalid_weather_sigs(), 2);
    assert!(!score.is_banned());

    score.record_invalid_weather_sig();
    assert_eq!(score.invalid_weather_sigs(), 3);
    assert!(
        score.is_banned(),
        "peer should be banned after 3 invalid weather sigs"
    );
}

/// Messages with empty or invalid signature/pubkey fields should be rejected.
#[tokio::test]
async fn missing_fields_rejected() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let station_id = "station-missing";
    let data = b"temp=21.0";
    let timestamp = 1_700_000_200i64;
    let nonce = "1234567890abcdef1234567890abcdef";

    // Case 1: both fields empty.
    let valid1 = verify_weather_gossip(station_id, data, timestamp, nonce, "", "");
    assert!(!valid1, "empty sig/pubkey must be rejected");

    // Case 2: invalid hex (non-hex characters).
    let valid2 = verify_weather_gossip(station_id, data, timestamp, nonce, "ZZZZ", "ZZZZ");
    assert!(!valid2, "non-hex sig/pubkey must be rejected");

    // Case 3: correct byte length but random bytes (not a valid signature).
    let random_sig_hex = hex::encode([0xAA_u8; 64]);
    let real_key = make_key(50);
    let real_pubkey_hex = hex::encode(PublicKey::from_private_key(&real_key).as_bytes());
    let valid3 = verify_weather_gossip(
        station_id,
        data,
        timestamp,
        nonce,
        &random_sig_hex,
        &real_pubkey_hex,
    );
    assert!(!valid3, "random sig bytes should fail verification");

    // Sanity: a properly signed message must pass.
    let (real_sig_hex, real_pubkey_hex) =
        sign_weather(&real_key, station_id, data, timestamp, nonce);
    let valid4 = verify_weather_gossip(
        station_id,
        data,
        timestamp,
        nonce,
        &real_sig_hex,
        &real_pubkey_hex,
    );
    assert!(valid4, "valid sig/pubkey must pass verification");
}

// ---------------------------------------------------------------------------
// WeatherDataV2 station-signature tests (#1969)
// ---------------------------------------------------------------------------

/// Sign a V2 weather gossip payload, using the canonical V2 data-hash
/// (inline `data` or, when present, the content_ref's `data_hash`).
fn sign_weather_v2(
    key: &PrivateKey,
    station_id: &str,
    data: &[u8],
    content_ref: Option<&ContentRef>,
    timestamp: i64,
    nonce: &str,
) -> (String, String) {
    let data_hash = weather_v2_data_hash_hex(data, content_ref);
    let payload_digest = payload_digest_hex(&(station_id, &data_hash));
    // #2065: sign-side resolves the same runtime identity the production verifier
    // uses (signing_chain_id/network_id), so both agree regardless of env state and
    // this test is immune to WRFCOIN_NETWORK_ID bleed from concurrent config tests.
    let cid = signing_chain_id();
    let nid = signing_network_id();
    let context = DomainSigningContext {
        action: DomainSigningAction::WeatherDataGossip,
        signer_id: station_id,
        timestamp,
        nonce: Some(nonce),
        chain_id: &cid,
        network_id: &nid,
    };
    let signing_msg = canonical_signing_message(&context, &payload_digest);
    let sig = key.sign(signing_msg.as_bytes());
    let pubkey_hex = hex::encode(PublicKey::from_private_key(key).as_bytes());
    (sig.to_hex(), pubkey_hex)
}

/// A correctly signed WeatherDataV2 message should arrive as
/// `P2pEvent::WeatherDataV2` (end-to-end through the node signing path).
#[tokio::test]
async fn valid_signature_accepted_v2() {
    let _multi_node_guard = super::helpers::serialize_multi_node_test();
    let _ = env_logger::builder().is_test(true).try_init();

    let (handle_a, mut handle_b) = two_node_mesh().await;

    let station_key = make_key(101);
    let station_id = "station-v2-valid".to_string();
    let data = b"temp=23.5,hum=61".to_vec();

    handle_a
        .command_tx
        .send(P2pCommand::BroadcastWeatherDataV2 {
            station_id: station_id.clone(),
            data: data.clone(),
            content_ref: None,
            station_signing_key: station_key,
        })
        .await
        .expect("send BroadcastWeatherDataV2");

    let received = timeout(RECV_TIMEOUT, async {
        loop {
            match handle_b.event_rx.recv().await {
                Some(P2pEvent::WeatherDataV2 {
                    station_id: sid,
                    data: d,
                    content_ref: cr,
                }) => return (sid, d, cr),
                Some(P2pEvent::PeerPenalized { reason, .. }) => {
                    panic!("valid V2 message was penalized: {reason}")
                },
                Some(_) => continue,
                None => panic!("node B event channel closed"),
            }
        }
    })
    .await
    .expect("node B should receive WeatherDataV2 within timeout");

    assert_eq!(received.0, station_id);
    assert_eq!(received.1, data);
    assert!(received.2.is_none());

    let _ = handle_a.command_tx.send(P2pCommand::Shutdown).await;
    let _ = handle_b.command_tx.send(P2pCommand::Shutdown).await;
}

/// The V2 verifier accepts a correctly signed inline payload and rejects
/// tampered data, a tampered station_id, a mismatched pubkey, and garbage sigs.
#[test]
fn v2_verifier_accept_and_reject_paths() {
    let station_key = make_key(102);
    let other_key = make_key(103);
    let station_id = "station-v2-verify";
    let data = b"temp=15.0,hum=80";
    let timestamp = 1_700_001_000i64;
    let nonce = "0011223344556677889900aabbccddee"; // pragma: allowlist secret

    let (sig_hex, pubkey_hex) =
        sign_weather_v2(&station_key, station_id, data, None, timestamp, nonce);

    // Happy path.
    assert!(
        verify_weather_gossip_signature_v2(
            station_id, data, None, timestamp, nonce, &sig_hex, &pubkey_hex
        ),
        "valid V2 signature should verify"
    );

    // Tampered data: same sig over different bytes must fail.
    assert!(
        !verify_weather_gossip_signature_v2(
            station_id,
            b"temp=99.9,hum=10",
            None,
            timestamp,
            nonce,
            &sig_hex,
            &pubkey_hex,
        ),
        "tampered V2 data must be rejected"
    );

    // Tampered station_id must fail.
    assert!(
        !verify_weather_gossip_signature_v2(
            "station-v2-attacker",
            data,
            None,
            timestamp,
            nonce,
            &sig_hex,
            &pubkey_hex,
        ),
        "tampered V2 station_id must be rejected"
    );

    // Wrong pubkey (sig from station_key, advertise other_key's pubkey).
    let other_pubkey_hex = hex::encode(PublicKey::from_private_key(&other_key).as_bytes());
    assert!(
        !verify_weather_gossip_signature_v2(
            station_id,
            data,
            None,
            timestamp,
            nonce,
            &sig_hex,
            &other_pubkey_hex,
        ),
        "V2 sig presented under the wrong pubkey must be rejected"
    );

    // Garbage / empty signature fields.
    assert!(
        !verify_weather_gossip_signature_v2(station_id, data, None, timestamp, nonce, "", ""),
        "empty V2 sig/pubkey must be rejected"
    );
    assert!(
        !verify_weather_gossip_signature_v2(
            station_id, data, None, timestamp, nonce, "ZZZZ", "ZZZZ"
        ),
        "non-hex V2 sig/pubkey must be rejected"
    );
}

/// A V2 signature binds to the content_ref's data_hash when offloaded; swapping
/// the CID's claimed digest must invalidate the signature.
#[test]
fn v2_signature_binds_to_content_ref_hash() {
    let station_key = make_key(104);
    let station_id = "station-v2-cid";
    // Inline `data` is empty for an offloaded payload.
    let data: &[u8] = b"";
    let timestamp = 1_700_002_000i64;
    let nonce = "ffeeddccbbaa99887766554433221100"; // pragma: allowlist secret

    let cref = ContentRef {
        data_cid: "ipfs:QmValidCidExample".to_string(),
        data_hash: hex::encode([0x11_u8; 32]),
        storage_provider: "ipfs".to_string(),
        data_size: 4096,
    };

    let (sig_hex, pubkey_hex) =
        sign_weather_v2(&station_key, station_id, data, Some(&cref), timestamp, nonce);

    assert!(
        verify_weather_gossip_signature_v2(
            station_id,
            data,
            Some(&cref),
            timestamp,
            nonce,
            &sig_hex,
            &pubkey_hex,
        ),
        "offloaded V2 signature should verify against its content_ref"
    );

    // Swap the claimed digest -> signature no longer covers it.
    let tampered_cref = ContentRef {
        data_hash: hex::encode([0x22_u8; 32]),
        ..cref.clone()
    };
    assert!(
        !verify_weather_gossip_signature_v2(
            station_id,
            data,
            Some(&tampered_cref),
            timestamp,
            nonce,
            &sig_hex,
            &pubkey_hex,
        ),
        "swapping the content_ref data_hash must invalidate the V2 signature"
    );
}
