# WiFi Mesh Networking — Design Document

**Date:** 2026-04-09
**Status:** Draft
**Issue:** #733
**See also:** [LTE Failover](lte-wan-failover.md), [Hardware Integration Roadmap](../hardware-integration-roadmap.md)

---

## Problem

In neighborhoods with multiple WRFCoin contributor nodes, each node independently uploads weather
data over its own ISP connection. If one node goes offline, its sensing coverage is lost until it
recovers. There is no local coordination, no redundant coverage, and no shared backhaul.

## Solution

An IEEE 802.11s WiFi mesh between nearby contributor nodes provides:

1. **Local gossip** — sensor data propagates across the mesh before upload, so any online node can
   submit observations on behalf of a temporarily offline neighbor.
2. **Backhaul sharing** — a single LTE gateway (issue #731) can serve as the mesh's WAN exit when
   all ISP links drop.
3. **Cross-validation** — overlapping sensor coverage zones enable spatial consistency checks
   (issue #808) between mesh peers.

## Scope (this PR)

- Protocol types for mesh peers, link state, and topology snapshots in `shared-protocol`
- Design context for how mesh integrates with the existing libp2p P2P layer
- Validation and tests

Out of scope:

- Actual 802.11s / batman-adv configuration (lives in `infra/`)
- P2P crate changes (future PR)
- LTE mesh gateway bridging (issue #734)

## Architecture

```
        Internet
          │
    ┌─────┼─────┐   ┌──────────┐   ┌──────────┐
    │  Node A   │   │  Node B  │   │  Node C  │
    │  (ISP-A)  │───│  (ISP-B) │───│  (ISP-C) │
    │  gateway  │   │          │   │          │
    └───────────┘   └──────────┘   └──────────┘
         │ 802.11s       │ 802.11s       │
         └───────────────┴───────────────┘
              Local WiFi Mesh Network
```

Each node runs a mesh interface (wlan1 in 802.11s mode) alongside its regular WAN connection.
The libp2p P2P layer discovers mesh-local peers via mDNS and tags them with a `MeshLink`
transport hint so the gossip layer can prioritize local propagation.

### Mesh vs WAN Peer Classification

| Property | Mesh peer | WAN peer |
|----------|-----------|----------|
| Transport | 802.11s / local LAN | Internet (TCP/QUIC) |
| Latency | < 5 ms | 20–200 ms |
| Trust | Higher (physical proximity) | Standard |
| Data sharing | Sensor gossip + block relay | Block relay only |
| Authentication | WRFCoin node identity (ed25519) | Same |

### Mesh Link Quality

Each mesh link carries quality metrics:

| Metric | Source | Range |
|--------|--------|-------|
| `rssi_dbm` | 802.11s station info | -100 to 0 dBm |
| `tx_bitrate_mbps` | nl80211 station dump | 1–600 Mbps |
| `expected_throughput_kbps` | batman-adv OGM | 0–100,000 |
| `hop_count` | batman-adv TT | 1–10 |

### State Machine

Mesh peer links follow a simple lifecycle:

```
  Discovered ──► Authenticating ──► Connected ──► Disconnected
       ▲                                │              │
       └────────────────────────────────┴──────────────┘
                        (rediscovery)
```

### Security Model

Mesh peers authenticate using the same ed25519 node identity as WAN peers. The mesh network is
physically local (802.11s range ~100–300m) which limits the attack surface, but cryptographic
authentication prevents rogue nodes from injecting fabricated sensor data into the local gossip.

### Integration with libp2p

The mesh does NOT replace libp2p. Instead:

1. mDNS discovers mesh-local peer IDs on the 802.11s interface.
2. libp2p connects to mesh peers via their local IP (low-latency path).
3. `PeerInfo` is extended (future PR) with a `MeshLinkInfo` annotation.
4. Gossipsub can optionally prioritize mesh peers for weather observation topics.

### Data Budget

Mesh traffic is local (no ISP bandwidth cost):

| Traffic type | Per peer | Peers (typical) | Total mesh traffic |
|-------------|----------|-----------------|-------------------|
| Weather gossip | ~1 KB/block | 3 | ~3 KB/block |
| Block headers | ~0.5 KB/block | 3 | ~1.5 KB/block |
| Mesh heartbeat | ~100 B/30s | 3 | ~0.3 KB/30s |
| batman-adv OGM | ~64 B/s | N/A (broadcast) | ~64 B/s |

At the 10-minute block tier: ~650 KB/day of mesh-only traffic. Negligible.

---

_Last updated: 2026-04-09_
