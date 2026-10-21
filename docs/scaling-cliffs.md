# WRFCoin Scaling Cliff Analysis

Component-by-component failure analysis for the 12 → 12,000 station trajectory.
Tracks #841. This is a living document — update as cliffs are addressed.

_Last updated: 2026-03-30 (Session 39)_

---

## Summary Table

| Component | At 12 | First Cliff | Cliff Point | Status |
|-----------|-------|-------------|-------------|--------|
| P2P gossip | OK | flood_publish O(N²) bandwidth | ~100 nodes | Config: mesh_n env-var tunable |
| Block size | OK | 8 MB exceeded by observations | ~5 obs/station | **Fixed → 32 MB** |
| Mempool | OK | 5K tx cap overflows | ~500 stations | **Fixed → 50K** |
| Slashing cap | OK | 3 events/block drops 99%+ | ~100 stations | **Fixed → 100/block + pruning** |
| State root sort | OK | O(N log N) sort per block | ~5K stations | **Fixed → BTreeMap (pre-sorted)** |
| WebSocket buffer | OK | 256-event overflow | ~100 subscribers | **Fixed → 4,096** |
| max_tx_per_block | OK | 2K cap blocks 12K stations | ~1K stations | TODO: dynamic sizing |
| Observation batching | OK | 12K txs vs ~40 batches | ~1K stations | TODO: batch protocol |
| State pruning | OK | 378 GB/yr at 12K stations | ~1K stations | TODO: data availability layer |
| API architecture | OK | single-process bottleneck | ~1K clients | TODO: connection pooling |

---

## Detailed Analysis

### P2P Layer (`p2p/src/config.rs`)

**Current config:**
- `max_transmit_size`: 1 MiB (gossipsub RPC limit)
- `inline_data_max`: 64 KiB (CID reference threshold)
- `mesh_n`: 4 (target peers, env-tunable: `WRFCOIN_GOSSIPSUB_MESH_N`)
- `mesh_n_high`: 8 (prune trigger)
- `flood_publish`: true (env-tunable: `WRFCOIN_GOSSIPSUB_FLOOD_PUBLISH`)

**Cliff at ~100 nodes:** `flood_publish: true` sends every message to every known peer.
At 100+ nodes this creates O(N²) bandwidth. Operators should set
`WRFCOIN_GOSSIPSUB_FLOOD_PUBLISH=false` and increase `WRFCOIN_GOSSIPSUB_MESH_N` as the
network grows.

**Future work:** Dynamic mesh sizing (`mesh_n = log2(peer_count).max(4)`), regional
topic sharding at 1K+ nodes.

### Consensus / Mempool (`shared-protocol/src/consensus/`, `shared-protocol/src/mempool/`)

**Current config:**
- `MAX_BLOCK_SIZE`: 32 MB (raised from 8 MB in this PR)
- `MEMPOOL_CAPACITY`: 50,000 (raised from 5,000 in this PR)
- `max_tx_per_block`: ~2,000 (parameterized per consensus engine)

**Cliff at ~1K stations:** `max_tx_per_block` cap of 2,000 blocks 12,000 stations
submitting per interval. `get_transactions_by_type()` does O(N) linear scans per type
(6 calls per block = O(6N)).

**Future work:** Dynamic `max_tx_per_block` based on block time, per-type mempool
indexes (`BTreeMap<TransactionType, Vec<Transaction>>` instead of single map).

### Slashing Engine (`shared-protocol/src/security/slashing.rs`)

**Current config:**
- `MAX_SLASHING_EVENTS_PER_BLOCK`: 100 (raised from 3 in this PR)
- `low_quality_counters`: FxHashMap with `prune_counters()` method (added in this PR)
- Scaled penalties: `base / sqrt(oracle_count)`, clamped [1, 100]

**Cliff at ~1K stations (mitigated):** Old cap of 3 events/block meant 99%+ of
violations were silently dropped. New cap of 100 handles correlated weather events
while still preventing mass-slashing cascades. Counter pruning prevents unbounded
map growth.

**Future work:** Per-participant rate limiting (max 1 slash per participant per N blocks),
deferred event queue for skipped slashes.

### State Root Computation (`shared-protocol/src/blockchain/state.rs:1700`)

**Current approach:** SHA3-256 over concatenated state material — accounts, NFTs,
commits, reveals, quality events, audit events, oracles, weather observations.

**Optimization applied:** `recent_weather_observations` changed from `FxHashMap` to
`BTreeMap` — eliminates O(N log N) sort per block since BTreeMap maintains sorted
order on insertion.

**Cliff at ~5K stations:** State root hashes all observations every block.
At 12,000 stations: ~960 KB weather data + 170K string comparisons per block.

**Future work:** Merkle tree sharding (separate roots for accounts/weather/oracles),
lazy root caching (only recompute on mutation), observation windowing (keep last
N=1000 entries by recency).

### API Server (`native-chain/src/bin/launch_api/`)

**Current architecture:** actix-web with `Arc<RwLock<Blockchain>>` state, single
broadcast channel for WebSocket events.

**WebSocket buffer:** 4,096 events (raised from 256 in this PR). At 12K stations,
expect ~12K reward events per block — buffer still overflows but handles 100+
subscriber scenarios without drops.

**Cliff at ~1K clients:** Single RwLock<Mempool> contention on concurrent reads,
`get_all_transactions()` clones all Arc pointers.

**Future work:** Connection pooling (deadpool), sharded mempool reads, atomic
pending-tx counter, tiered WebSocket buffering (critical vs. reward events).

### Weather Observation Storage

**Current:** Each observation is an independent transaction (~300 bytes for
commit + reveal). Stored inline in blocks.

**Storage growth:**
- 12 stations: 3.6 KB/block → 113 MB/yr
- 1,200 stations: 360 KB/block → 11 GB/yr
- 12,000 stations: 3.6 MB/block → 113 GB/yr (observations only)

**Cliff at ~1K stations:** Gossip overhead of 12K individual transactions vs
~40 batched messages.

**Future work:** Observation batching protocol, bincode serialization (3-5x
compression), regional batching, data availability layer for historical data.

---

## Change Log

| Date | Changes | PR |
|------|---------|-----|
| 2026-03-30 | Initial analysis + near-term fixes (block size, mempool, slashing, state root, WS buffer) | #TBD |
