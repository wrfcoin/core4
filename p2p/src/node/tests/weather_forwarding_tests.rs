//! What a node forwards on the weather topic.
//!
//! Gossipsub runs with `validate_messages()`, so a message is passed on to the
//! node's other peers when the handler reports `Accept` for it. These tests put
//! one node between raw peers ([`super::raw_peer_net`]) and read, on a peer
//! that did not publish, exactly what the node relayed.

use std::sync::atomic::Ordering;

use wrfcoin_shared_protocol::crypto::domain_signing::{
    DomainSigningAction, DomainSigningContext, canonical_signing_message, payload_digest_hex,
    signing_chain_id, signing_network_id,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

use super::raw_peer_net::RawPeerNet;
use crate::P2pEvent;
use crate::message::WrfcoinMessage;

fn station_key(seed: u64) -> PrivateKey {
    use rand::SeedableRng;
    PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(seed))
}

/// A weather message exactly as `P2pCommand::BroadcastWeatherData` builds it:
/// signed by `key`, carrying `key`'s public half.
fn signed_weather(station_id: &str, key: &PrivateKey) -> WrfcoinMessage {
    signed_weather_with(
        station_id,
        key,
        format!("reading-from-{station_id}").into_bytes(),
        crate::node::generate_weather_nonce(),
    )
}

/// [`signed_weather`] with the data and the nonce chosen by the caller.
fn signed_weather_with(
    station_id: &str,
    key: &PrivateKey,
    data: Vec<u8>,
    nonce: String,
) -> WrfcoinMessage {
    let timestamp = chrono::Utc::now().timestamp();
    let data_hash = hex::encode(blake3::hash(&data).as_bytes());
    let payload_digest = payload_digest_hex(&(&station_id.to_string(), &data_hash));
    let chain_id = signing_chain_id();
    let network_id = signing_network_id();
    let context = DomainSigningContext {
        action: DomainSigningAction::WeatherDataGossip,
        signer_id: station_id,
        timestamp,
        nonce: Some(&nonce),
        chain_id: &chain_id,
        network_id: &network_id,
    };
    let signature = key.sign(canonical_signing_message(&context, &payload_digest).as_bytes());
    WrfcoinMessage::WeatherData {
        station_id: station_id.to_string(),
        data,
        timestamp,
        nonce: Some(nonce),
        station_signature: signature.to_hex(),
        station_pubkey: hex::encode(PublicKey::from_private_key(key).as_bytes()),
    }
}

/// The same message with the signature of a different key: well formed, and
/// rejected only by the signature check.
fn weather_with_wrong_signature(station_id: &str) -> WrfcoinMessage {
    let honest = signed_weather(station_id, &station_key(11));
    let other = signed_weather(station_id, &station_key(12));
    match (honest, other) {
        (
            WrfcoinMessage::WeatherData {
                station_id,
                data,
                timestamp,
                nonce,
                station_pubkey,
                ..
            },
            WrfcoinMessage::WeatherData {
                station_signature, ..
            },
        ) => WrfcoinMessage::WeatherData {
            station_id,
            data,
            timestamp,
            nonce,
            station_signature,
            station_pubkey,
        },
        _ => unreachable!("signed_weather builds WeatherData"),
    }
}

/// The same message with no nonce: rejected before the signature check.
fn weather_without_nonce(station_id: &str) -> WrfcoinMessage {
    match signed_weather(station_id, &station_key(13)) {
        WrfcoinMessage::WeatherData {
            station_id,
            data,
            timestamp,
            station_signature,
            station_pubkey,
            ..
        } => WrfcoinMessage::WeatherData {
            station_id,
            data,
            timestamp,
            nonce: None,
            station_signature,
            station_pubkey,
        },
        _ => unreachable!("signed_weather builds WeatherData"),
    }
}

/// A node relays a weather message only after the message's own checks pass.
///
/// Three raw peers around one node: `SENDER` publishes, `OBSERVER` only
/// listens, `CLOSER` publishes the closing message. The sender publishes, in
/// order: a good message; one with no nonce; one with another key's signature;
/// bytes that do not decode. The test waits until the node's handler has
/// dealt with each bad message, and only then has the closing peer publish a
/// last good message. The node sends to the observer over one connection in
/// the order it decided, so once that last message has arrived, anything the
/// node relayed before it has arrived too.
#[tokio::test]
async fn weather_is_forwarded_only_after_its_checks_pass() {
    const SENDER: usize = 0;
    const OBSERVER: usize = 1;
    const CLOSER: usize = 2;

    let topic = crate::message::TopicManager::new().weather_data().clone();
    let weather = topic.hash();
    let mut net = RawPeerNet::start(3, &topic).await;

    let first_good = signed_weather("station-good-1", &station_key(1));
    let first_good_bytes = first_good.to_bytes().expect("serialize");
    net.peers[SENDER].publish(&topic, &first_good).expect("publish the first good message");
    let relayed = net
        .run_until(|net| net.peers[OBSERVER].received_on(&weather).len() == 1)
        .await;
    assert!(relayed, "control: a correctly signed weather message is relayed to the observer");

    let rejected_before = net.node.metrics.messages_rejected.load(Ordering::Relaxed);

    let no_nonce = weather_without_nonce("station-no-nonce");
    net.peers[SENDER].publish(&topic, &no_nonce).expect("publish the nonce-less message");
    let handled = net
        .run_until(|net| {
            net.node.metrics.messages_rejected.load(Ordering::Relaxed) == rejected_before + 1
        })
        .await;
    assert!(handled, "the node's handler never rejected the nonce-less message");

    let wrong_signature = weather_with_wrong_signature("station-wrong-signature");
    net.peers[SENDER]
        .publish(&topic, &wrong_signature)
        .expect("publish the wrongly signed message");
    let handled = net
        .run_until(|net| net.node.metrics.weather_msgs_invalid_sig.load(Ordering::Relaxed) == 1)
        .await;
    assert!(handled, "the node's handler never rejected the wrongly signed message");

    let undecodable = vec![0xff_u8; 96];
    net.peers[SENDER]
        .publish_bytes(&topic, undecodable.clone())
        .expect("publish bytes that do not decode");
    let handled = net
        .run_until(|net| {
            net.node.metrics.messages_rejected.load(Ordering::Relaxed) == rejected_before + 2
        })
        .await;
    assert!(handled, "the node's handler never refused the undecodable bytes");

    let last_good = signed_weather("station-good-2", &station_key(2));
    let last_good_bytes = last_good.to_bytes().expect("serialize");
    net.peers[CLOSER].publish(&topic, &last_good).expect("publish the closing good message");
    let closed = net
        .run_until(|net| {
            net.peers[OBSERVER]
                .received_on(&weather)
                .iter()
                .any(|bytes| *bytes == last_good_bytes.as_slice())
        })
        .await;
    assert!(closed, "the closing good message never reached the observer");
    net.settle(10).await;

    let label = |bytes: &[u8]| -> String {
        if bytes == first_good_bytes.as_slice() {
            "good-1".into()
        } else if bytes == last_good_bytes.as_slice() {
            "good-2".into()
        } else if bytes == no_nonce.to_bytes().expect("serialize").as_slice() {
            "no-nonce".into()
        } else if bytes == wrong_signature.to_bytes().expect("serialize").as_slice() {
            "wrong-signature".into()
        } else if bytes == undecodable.as_slice() {
            "undecodable".into()
        } else {
            format!("unexpected {} bytes", bytes.len())
        }
    };
    let relayed: Vec<String> = net.peers[OBSERVER]
        .received_on(&weather)
        .into_iter()
        .map(label)
        .collect();
    assert_eq!(
        relayed,
        vec!["good-1".to_string(), "good-2".to_string()],
        "the node must relay only weather messages that passed nonce, admission and signature"
    );

    // The node's own use of the messages is unchanged: both good ones, and
    // only those, reach the launch layer.
    let mut delivered = Vec::new();
    while let Ok(event) = net.node_events.try_recv() {
        if let P2pEvent::WeatherData { station_id, .. } = event {
            delivered.push(station_id);
        }
    }
    assert_eq!(delivered, vec!["station-good-1".to_string(), "station-good-2".to_string()]);

    // The closing peer, in the mesh like the observer, saw the same relays
    // from the sender's side (its own publish is not echoed back).
    let closer_saw: Vec<String> = net.peers[CLOSER]
        .received_on(&weather)
        .into_iter()
        .map(label)
        .collect();
    assert_eq!(closer_saw, vec!["good-1".to_string()]);
}

/// A V2 weather message signed by `signer`, carrying `claimed`'s public key.
/// With `signer == claimed` it is what `P2pCommand::BroadcastWeatherDataV2`
/// builds.
fn weather_v2(station_id: &str, signer: &PrivateKey, claimed: &PrivateKey) -> WrfcoinMessage {
    let data = format!("v2-reading-from-{station_id}").into_bytes();
    let timestamp = chrono::Utc::now().timestamp();
    let nonce = crate::node::generate_weather_nonce();
    let data_hash = crate::node::weather_v2_data_hash_hex(&data, None);
    let payload_digest = payload_digest_hex(&(station_id, &data_hash));
    let chain_id = signing_chain_id();
    let network_id = signing_network_id();
    let context = DomainSigningContext {
        action: DomainSigningAction::WeatherDataGossip,
        signer_id: station_id,
        timestamp,
        nonce: Some(&nonce),
        chain_id: &chain_id,
        network_id: &network_id,
    };
    let signature = signer.sign(canonical_signing_message(&context, &payload_digest).as_bytes());
    WrfcoinMessage::WeatherDataV2 {
        station_id: station_id.to_string(),
        data,
        timestamp,
        content_ref: None,
        nonce,
        station_signature: signature.to_hex(),
        station_pubkey: hex::encode(PublicKey::from_private_key(claimed).as_bytes()),
    }
}

/// The V2 weather kind follows the same rule: relayed when its signature
/// verifies, not relayed when it does not.
#[tokio::test]
async fn weather_v2_is_forwarded_only_after_its_checks_pass() {
    const SENDER: usize = 0;
    const OBSERVER: usize = 1;
    const CLOSER: usize = 2;

    let topic = crate::message::TopicManager::new().weather_data().clone();
    let weather = topic.hash();
    let mut net = RawPeerNet::start(3, &topic).await;

    let first_good = weather_v2("station-v2-good-1", &station_key(21), &station_key(21));
    let first_good_bytes = first_good.to_bytes().expect("serialize");
    net.peers[SENDER].publish(&topic, &first_good).expect("publish the first good message");
    let relayed = net
        .run_until(|net| net.peers[OBSERVER].received_on(&weather).len() == 1)
        .await;
    assert!(relayed, "control: a correctly signed V2 weather message is relayed");

    let wrong_signature = weather_v2("station-v2-wrong", &station_key(22), &station_key(23));
    net.peers[SENDER]
        .publish(&topic, &wrong_signature)
        .expect("publish the wrongly signed message");
    let handled = net
        .run_until(|net| net.node.metrics.weather_msgs_invalid_sig.load(Ordering::Relaxed) == 1)
        .await;
    assert!(handled, "the node's handler never rejected the wrongly signed V2 message");

    let last_good = weather_v2("station-v2-good-2", &station_key(24), &station_key(24));
    let last_good_bytes = last_good.to_bytes().expect("serialize");
    net.peers[CLOSER].publish(&topic, &last_good).expect("publish the closing good message");
    let closed = net
        .run_until(|net| {
            net.peers[OBSERVER]
                .received_on(&weather)
                .iter()
                .any(|bytes| *bytes == last_good_bytes.as_slice())
        })
        .await;
    assert!(closed, "the closing good V2 message never reached the observer");
    net.settle(10).await;

    let relayed: Vec<&[u8]> = net.peers[OBSERVER].received_on(&weather);
    assert_eq!(
        relayed,
        vec![first_good_bytes.as_slice(), last_good_bytes.as_slice()],
        "the node must relay only V2 weather messages whose signature verified"
    );
}

/// The node's gossipsub score for a raw peer. Starts at zero; an invalid
/// delivery (a `Reject` verdict) takes it below zero.
fn node_score_of(net: &RawPeerNet, peer: usize) -> f64 {
    let peer_id = net.peers[peer].peer_id();
    net.node
        .swarm
        .behaviour()
        .gossipsub
        .peer_score(&peer_id)
        .expect("the node scores every connected peer")
}

/// Whether a weather message is new to a node depends on what that node has
/// already seen, not on the peer that delivers it. A second message with the
/// same station and nonce is refused as a replay, and the peer that relayed it
/// in good faith must not lose score for it: enough of those and the node
/// would stop listening to that peer altogether, blocks included.
#[tokio::test]
async fn a_replayed_weather_nonce_does_not_cost_the_relayer_score() {
    const FIRST: usize = 0;
    const RELAYER: usize = 1;

    let topic = crate::message::TopicManager::new().weather_data().clone();
    let weather = topic.hash();
    let mut net = RawPeerNet::start(2, &topic).await;

    // Two validly signed messages for one station and nonce, different data:
    // two distinct gossip messages that collide only in the node's replay set.
    let key = station_key(31);
    let nonce = crate::node::generate_weather_nonce();
    let first = signed_weather_with("station-replay", &key, b"first".to_vec(), nonce.clone());
    let second = signed_weather_with("station-replay", &key, b"second".to_vec(), nonce);
    let second_bytes = second.to_bytes().expect("serialize");

    net.peers[FIRST].publish(&topic, &first).expect("publish the first message");
    let seen = net
        .run_until(|net| net.node.metrics.weather_msgs_received.load(Ordering::Relaxed) == 1)
        .await;
    assert!(seen, "control: the node accepted the first message and recorded its nonce");

    let rejected_before = net.node.metrics.messages_rejected.load(Ordering::Relaxed);
    let score_before = node_score_of(&net, RELAYER);
    net.peers[RELAYER].publish(&topic, &second).expect("relay the second message");
    let handled = net
        .run_until(|net| {
            net.node.metrics.messages_rejected.load(Ordering::Relaxed) == rejected_before + 1
        })
        .await;
    assert!(handled, "the node's handler never refused the replayed nonce");
    net.settle(10).await;

    assert!(
        node_score_of(&net, RELAYER) >= score_before,
        "relaying a message the node had already seen the nonce of must not lower the \
         relayer's score (before {score_before}, after {})",
        node_score_of(&net, RELAYER)
    );
    assert!(
        !net.peers[FIRST]
            .received_on(&weather)
            .iter()
            .any(|bytes| *bytes == second_bytes.as_slice()),
        "the replayed message is still not forwarded"
    );
}

/// A node on an older release forwards weather messages before checking them.
/// When one arrives here with no nonce or a signature that does not verify,
/// this node drops it, but the peer that passed it on is not the one at fault
/// and must not lose score for it.
#[tokio::test]
async fn relaying_a_bad_weather_message_does_not_cost_the_relayer_score() {
    const RELAYER: usize = 0;
    const OBSERVER: usize = 1;

    let topic = crate::message::TopicManager::new().weather_data().clone();
    let weather = topic.hash();
    let mut net = RawPeerNet::start(2, &topic).await;
    let score_before = node_score_of(&net, RELAYER);
    let rejected_before = net.node.metrics.messages_rejected.load(Ordering::Relaxed);

    net.peers[RELAYER]
        .publish(&topic, &weather_without_nonce("station-relayed-no-nonce"))
        .expect("relay the nonce-less message");
    let handled = net
        .run_until(|net| {
            net.node.metrics.messages_rejected.load(Ordering::Relaxed) == rejected_before + 1
        })
        .await;
    assert!(handled, "the node's handler never refused the nonce-less message");

    net.peers[RELAYER]
        .publish(&topic, &weather_with_wrong_signature("station-relayed-wrong-signature"))
        .expect("relay the wrongly signed message");
    let handled = net
        .run_until(|net| net.node.metrics.weather_msgs_invalid_sig.load(Ordering::Relaxed) == 1)
        .await;
    assert!(handled, "the node's handler never refused the wrongly signed message");

    net.peers[RELAYER]
        .publish(
            &topic,
            &weather_v2("station-relayed-v2-wrong", &station_key(41), &station_key(42)),
        )
        .expect("relay the wrongly signed V2 message");
    let handled = net
        .run_until(|net| net.node.metrics.weather_msgs_invalid_sig.load(Ordering::Relaxed) == 2)
        .await;
    assert!(handled, "the node's handler never refused the wrongly signed V2 message");
    net.settle(10).await;

    assert!(
        node_score_of(&net, RELAYER) >= score_before,
        "relaying weather messages this node drops must not lower the relayer's score \
         (before {score_before}, after {})",
        node_score_of(&net, RELAYER)
    );
    assert!(
        net.peers[OBSERVER].received_on(&weather).is_empty(),
        "none of the dropped messages is forwarded"
    );
}

/// A weather signature is checked against a key the sender supplies, and the
/// peer a bad one arrives from is whoever relayed it. A node on an older
/// release relays weather before checking it, so delivering messages with bad
/// signatures says nothing about the delivering peer. This node counts them
/// and drops them; it keeps no score against that peer and reports no penalty
/// for it to the launch layer (which disconnects peers it has penalised
/// enough).
#[tokio::test]
async fn bad_weather_signatures_bring_no_penalty_on_the_delivering_peer() {
    const RELAYER: usize = 0;
    const OBSERVER: usize = 1;
    const BAD_MESSAGES: u64 = 6;

    let topic = crate::message::TopicManager::new().weather_data().clone();
    let weather = topic.hash();
    let mut net = RawPeerNet::start(2, &topic).await;
    let relayer = net.peers[RELAYER].peer_id();
    // Drop what the handshakes and mesh set-up emitted.
    while net.node_events.try_recv().is_ok() {}

    for round in 0..BAD_MESSAGES {
        // A fresh station id each time, so the per-station rate limit never
        // hides a message from the signature check. Both wire versions.
        let station = format!("station-bad-signature-{round}");
        let message = if round % 2 == 0 {
            weather_with_wrong_signature(&station)
        } else {
            weather_v2(&station, &station_key(50 + round), &station_key(150 + round))
        };
        net.peers[RELAYER].publish(&topic, &message).expect("relay a wrongly signed message");
        let counted = net
            .run_until(|net| {
                net.node.metrics.weather_msgs_invalid_sig.load(Ordering::Relaxed) == round + 1
            })
            .await;
        assert!(counted, "bad signature {round} was never counted: the event must stay visible");
    }
    net.settle(10).await;

    let mut penalties = Vec::new();
    while let Ok(event) = net.node_events.try_recv() {
        if let P2pEvent::PeerPenalized { peer_id, reason, banned } = event {
            penalties.push((peer_id, reason, banned));
        }
    }
    assert_eq!(
        penalties,
        Vec::new(),
        "no penalty for the delivering peer may reach the launch layer"
    );
    assert!(
        net.node.weather_peer_scores.get(&relayer).is_none(),
        "the node must keep no weather score against the delivering peer"
    );
    assert!(
        node_score_of(&net, RELAYER) >= 0.0,
        "and its gossip score is untouched"
    );
    assert!(
        net.peers[OBSERVER].received_on(&weather).is_empty(),
        "none of the bad messages is forwarded"
    );
}
