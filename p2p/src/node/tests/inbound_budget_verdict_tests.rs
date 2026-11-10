//! Who pays when an inbound byte budget runs out.
//!
//! The node has two inbound ceilings. The per-peer one is about that peer: a
//! peer that exceeds it is at fault. The node-wide one is a sum over every
//! peer: when it runs out, the message that happens to arrive next says
//! nothing about the peer that delivered it.

use std::sync::atomic::Ordering;
use std::time::Duration;

use super::raw_peer_net::RawPeerNet;
use crate::P2pEvent;
use crate::message::WrfcoinMessage;
use crate::preauth_budget::{PreAuthBudget, PreAuthLimits};

const BUDGET_BYTES: usize = 8 * 1024;
const MESSAGE_BYTES: usize = 5 * 1024;

fn block_message(fill: u8) -> WrfcoinMessage {
    WrfcoinMessage::NewBlock {
        block_bytes: vec![fill; MESSAGE_BYTES],
    }
}

fn node_score_of(net: &RawPeerNet, peer: usize) -> f64 {
    let peer_id = net.peers[peer].peer_id();
    net.node
        .swarm
        .behaviour()
        .gossipsub
        .peer_score(&peer_id)
        .expect("the node scores every connected peer")
}

/// One peer uses up the node-wide budget. The next message, from a different
/// peer, is dropped, and that peer keeps its gossip score: the node-wide
/// budget running out is not that peer's doing.
#[tokio::test]
async fn an_exhausted_node_wide_budget_does_not_cost_the_next_peer_score() {
    const FIRST: usize = 0;
    const SECOND: usize = 1;

    let topic = crate::message::TopicManager::new().blocks().clone();
    let mut net = RawPeerNet::start(2, &topic).await;
    // A fresh, small node-wide budget: room for one of these messages.
    net.node.preauth_budget = PreAuthBudget::new(PreAuthLimits {
        max_bytes_total: BUDGET_BYTES,
        verify_window: Duration::from_secs(600),
        ..PreAuthLimits::default()
    });

    net.peers[FIRST].publish(&topic, &block_message(1)).expect("publish the first message");
    let taken = net
        .run_until(|net| net.node.metrics.blocks_received.load(Ordering::Relaxed) == 1)
        .await;
    assert!(taken, "control: the first message fits the budget and is handled");

    let rejected_before = net.node.metrics.messages_rejected.load(Ordering::Relaxed);
    let score_before = node_score_of(&net, SECOND);
    net.peers[SECOND].publish(&topic, &block_message(2)).expect("publish the second message");
    let dropped = net
        .run_until(|net| {
            net.node.metrics.messages_rejected.load(Ordering::Relaxed) == rejected_before + 1
        })
        .await;
    assert!(dropped, "the second message must be dropped: the node-wide budget is used up");
    net.settle(10).await;

    assert_eq!(
        net.node.metrics.blocks_received.load(Ordering::Relaxed),
        1,
        "the dropped message is not handled"
    );
    assert!(
        node_score_of(&net, SECOND) >= score_before,
        "the peer whose message arrived after the node-wide budget ran out must not lose \
         score (before {score_before}, after {})",
        node_score_of(&net, SECOND)
    );
}

/// The per-peer ceiling is the peer's own: exceeding it still costs that peer
/// gossip score and is still reported to the launch layer.
#[tokio::test]
async fn a_peer_over_its_own_inbound_ceiling_is_still_penalised() {
    const SENDER: usize = 0;

    let topic = crate::message::TopicManager::new().blocks().clone();
    let mut net = RawPeerNet::start(1, &topic).await;
    let sender = net.peers[SENDER].peer_id();
    net.node.inbound_rate_limit = super::InboundRateLimit {
        max_bytes: 1024,
        window: Duration::from_secs(600),
    };
    while net.node_events.try_recv().is_ok() {}

    let rejected_before = net.node.metrics.messages_rejected.load(Ordering::Relaxed);
    net.peers[SENDER].publish(&topic, &block_message(3)).expect("publish over the ceiling");
    let dropped = net
        .run_until(|net| {
            net.node.metrics.messages_rejected.load(Ordering::Relaxed) == rejected_before + 1
        })
        .await;
    assert!(dropped, "a message over the per-peer ceiling must be dropped");
    net.settle(10).await;

    let mut penalised = false;
    while let Ok(event) = net.node_events.try_recv() {
        if let P2pEvent::PeerPenalized { peer_id, .. } = event {
            penalised |= peer_id == sender;
        }
    }
    assert!(penalised, "the launch layer must hear about a peer over its own ceiling");
    assert!(
        node_score_of(&net, SENDER) < 0.0,
        "and the peer loses gossip score (score {})",
        node_score_of(&net, SENDER)
    );
}
