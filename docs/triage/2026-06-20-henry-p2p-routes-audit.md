# P2P / launch_api Routes Security Audit

**Date:** 2026-06-20  
**Branch:** henry/p2p-routes-audit-r6 (from origin/main)  
**Auditor:** Henry (AI Bot)  
**Scope:** `p2p/` and `native-chain/src/bin/launch_api/` — unauthenticated/unbounded endpoints, DoS vectors, missing input validation

---

## Findings

| # | File : Line | Severity | Status | Fix Sketch |
|---|-------------|----------|--------|------------|
| 1 | `native-chain/src/bin/launch_api/routes/network.rs:288` | **HIGH** | REAL — confirmed on origin/main; PR #1792 closed without merge | Add `require_role(Operator)` guard to `get_network_peer_reputation`. **FIXED in this branch.** |
| 2 | `native-chain/src/bin/launch_api/routes/network.rs:301` | **MEDIUM** | REAL — confirmed on origin/main; no prior fix PR | Add `require_role(Reader)` guard to `get_network_stats`; exposes uptime, byte counters, mesh-peer-by-topic map useful for targeted attacks. No wire change. |
| 3 | `native-chain/src/bin/launch_api/routes/network.rs:208` | **LOW** | REAL — confirmed on origin/main; no prior fix PR | Add `require_role(Reader)` guard to `get_network_peer_id`; returns `peer_id`, `listen_addrs`, `external_addr` — aids peer impersonation/targeting. No wire change. |

---

## Detail

### Finding 1 — `/network/peers/reputation` exposes full graylist table (HIGH) **[FIXED]**

- **Location:** `native-chain/src/bin/launch_api/routes/network.rs:288` (`get_network_peer_reputation`)
- **Feature gate:** `#[cfg(feature = "p2p")]`
- **Verified REAL on origin/main:** handler had no `require_role` call, returning `total`, `graylisted`, and the full `peers` array (per-peer penalty history, ban count, reputation score, graylist status).
- **Prior fix attempt:** PR #1792 ("operator reputation route") was opened but closed without merge.
- **Fix applied:** Added `req: HttpRequest`, `state: web::Data<AppState>` parameters and `require_role(&req, &state, ApiRole::Operator)` gate. No response format change.
- **Tests:** Existing unit test converted to pure snapshot-ordering test; two new integration tests added to `tests/network_routes.rs` covering 401 (no token), 403 (Reader token), and 200 (Operator token with correct payload).

### Finding 2 — `/network/stats` leaks traffic counters unauthenticated (MEDIUM) [NOT YET FIXED]

- **Location:** `native-chain/src/bin/launch_api/routes/network.rs:301` (`get_network_stats`)
- **Feature gate:** `#[cfg(feature = "p2p")]`
- **Verified REAL on origin/main:** handler accesses `state.p2p` without any `require_role` call.
- **Payload exposed:** `bytes_inbound`, `bytes_outbound`, `messages_received`, `messages_sent`, `tx_gossip_sent`, `tx_gossip_received`, `weather_packets_forwarded`, `uptime_secs`, `mesh_peers_by_topic` (topic → peer count map).
- **Risk:** `mesh_peers_by_topic` combined with `bytes_*` gives an attacker a continuous traffic fingerprint useful for eclipse-attack timing and topic-specific flood calibration.
- **Fix sketch:** Add `require_role(Reader)` guard — same pattern as Finding 1 but with Reader-level access (stats are read-only operational data, not operator-sensitive). No wire change.

### Finding 3 — `/network/peer_id` leaks node identity unauthenticated (LOW) [NOT YET FIXED]

- **Location:** `native-chain/src/bin/launch_api/routes/network.rs:208` (`get_network_peer_id`)
- **Feature gate:** `#[cfg(feature = "p2p")]`
- **Verified REAL on origin/main:** no `require_role` call.
- **Payload exposed:** `peer_id` (libp2p PeerId), `listen_addrs` (multiaddr list), `external_addr`.
- **Risk:** Stable PeerId + listen addresses enable targeted Sybil neighbors or eclipse via DHT manipulation. Lower severity than #1/#2 because `peer_id` is also broadcast in gossipsub, but the listen addresses are not.
- **Fix sketch:** Add `require_role(Reader)` guard. No wire change.

---

## Out of Scope / Already Fixed

| Endpoint | Status |
|----------|--------|
| `/network/dht/peers` | Fixed: PR #1831 merged — `require_role(Reader)` added |
| `/network/audit-log` | Fixed: already gated `require_role(Operator)` |
| `/ws` WebSocket | OK: auth via bearer token in `resolve_ws_principal` |
| `/operator/partition-mode` | OK: `require_role(Operator)` |
| `/debug/seed-account` | OK: `debug_seed_api_enabled` flag + `require_role(Operator)` |
| P2P gossipsub message size | OK: `MAX_MESSAGE_BYTES: 1 MiB` hard cap + `validate_inline_size()` |
| WS subscription limits | OK: `MAX_SUBSCRIPTIONS_PER_CONN: 32` |
| Rate limiter | OK: per-IP sliding window, trusted-proxy XFF protection |

---

## Commit

Code fix (Finding 1) and this report committed on `henry/p2p-routes-audit-r6`. Findings 2 and 3 are left for follow-up PRs.
