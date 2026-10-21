# LTE/5G WAN Failover — Design Document

**Date:** 2026-04-09
**Status:** Draft
**Issue:** #731
**See also:** [Hardware Integration Roadmap](../hardware-integration-roadmap.md), [Contributor Hardware Guide](../contributor-hardware-guide.md)

---

## Problem

A contributor node's uptime directly determines its publishing score and reward share. When the
operator's home ISP drops, the node goes offline — it stops producing blocks, misses commit-reveal
windows, and its `/readyz` endpoint returns 503 after `NODE_SYNC_STALE_AFTER_SECS` (600 s). An LTE
USB modem with a prepaid SIM ($40--80 hardware, $5--20/mo data) can provide automatic WAN failover
so the node stays connected during ISP outages.

## Scope

This design covers:

1. **Failover state machine** — detect primary WAN loss, transition to LTE, recover when primary
   returns.
2. **Modem interface trait** — abstraction over USB modem control (signal, connect, disconnect) so
   the detection logic is testable without hardware.
3. **Metered-connection awareness** — when on cellular, the node should reduce bandwidth-heavy
   operations (large block history sync, storage provider uploads) while continuing consensus
   participation and weather data submission.

Out of scope for this PR:

- Actual NetworkManager/systemd-networkd configuration (that lives in `infra/`)
- WiFi mesh bridging (issues #733, #734)
- P2P protocol changes

## Architecture

```
┌───────────────────────────────────────────────────────┐
│  native-chain node process                            │
│                                                       │
│  ┌─────────────────────┐   ┌────────────────────┐    │
│  │ WanFailoverMonitor  │──►│ API: /node/network  │    │
│  │                     │   │      /readyz        │    │
│  │  state machine      │   └────────────────────┘    │
│  │  probe loop         │                              │
│  └──────┬──────────────┘                              │
│         │ WanEvent callbacks                          │
│         ▼                                             │
│  ┌─────────────────────┐                              │
│  │ P2P / consensus     │  (reduce activity when       │
│  │ weather submission  │   on metered connection)      │
│  └─────────────────────┘                              │
└───────────────────────────────────────────────────────┘
         │                        │
    ┌────┴────┐              ┌────┴────┐
    │ Primary │              │   LTE   │
    │  WAN    │              │  Modem  │
    │ (eth0)  │              │ (wwan0) │
    └─────────┘              └─────────┘
```

## State Machine

```
            ┌──────────────────┐
   ┌───────►│  PrimaryOnline   │◄─────────────┐
   │        └────────┬─────────┘              │
   │                 │ consecutive probe       │ primary probe
   │                 │ failures ≥ threshold    │ succeeds
   │                 ▼                         │
   │        ┌──────────────────┐              │
   │        │    Probing       │──────────────┘
   │        │  (grace period)  │
   │        └────────┬─────────┘
   │                 │ grace period
   │                 │ expires
   │                 ▼
   │        ┌──────────────────┐
   │        │ FailoverActive   │──────────────┐
   │        │  (on LTE)        │              │ primary recovers
   │        └────────┬─────────┘              │
   │                 │ LTE also fails         │
   │                 ▼                         │
   │        ┌──────────────────┐              │
   └────────│  Disconnected    │──────────────┘
            └──────────────────┘
```

### State definitions

| State | Meaning | Node behavior |
|-------|---------|---------------|
| `PrimaryOnline` | Primary WAN is healthy | Normal operation, full bandwidth |
| `Probing` | Primary probe failed, within grace period | Normal operation, probing both interfaces |
| `FailoverActive` | Switched to LTE backup | Metered mode: consensus + weather OK, large syncs deferred |
| `Disconnected` | Both interfaces down | Offline; retry both interfaces on exponential backoff |

### Transition parameters

| Parameter | Default | Rationale |
|-----------|---------|-----------|
| `probe_interval` | 15 s | Frequent enough to detect outages quickly |
| `probe_failure_threshold` | 3 | 3 consecutive failures (45 s) before entering Probing |
| `grace_period` | 60 s | Time in Probing before triggering failover |
| `primary_recovery_threshold` | 3 | 3 consecutive primary successes to switch back |
| `probe_targets` | `1.1.1.1:443`, `8.8.8.8:443` | TCP connect probes to well-known anycast IPs |
| `probe_timeout` | 5 s | Per-probe TCP connect timeout |

The total detection-to-failover latency is `(threshold × interval) + grace = 45 + 60 = 105 s`,
well within the 600 s `NODE_SYNC_STALE_AFTER_SECS` window.

## Modem Interface

```rust
pub trait ModemInterface: Send + Sync {
    /// Check whether the modem hardware is present and recognized.
    fn is_present(&self) -> bool;
    /// Signal strength in dBm (negative). None if no signal or modem absent.
    fn signal_strength_dbm(&self) -> Option<i32>;
    /// Current registration status.
    fn registration_status(&self) -> ModemRegistration;
    /// Request the modem to establish a data connection.
    fn connect(&self) -> Result<(), String>;
    /// Request the modem to disconnect.
    fn disconnect(&self) -> Result<(), String>;
}
```

The production implementation will call `mmcli` (ModemManager CLI) or read from
`/sys/class/net/wwan0/`. The test implementation is a mock with controllable state.

## Metered-Connection Policy

When `FailoverActive`:

| Operation | Policy |
|-----------|--------|
| P2P consensus messages | **Allow** — small packets, critical for uptime |
| Weather observation submission | **Allow** — small payloads, core mission |
| Block header sync | **Allow** — lightweight |
| Full block body sync (catch-up) | **Defer** until primary recovers |
| Storage provider uploads (IPFS) | **Defer** |
| Large RPC responses | **Truncate** or paginate more aggressively |

Estimated cellular data usage for consensus-only + weather: **< 50 MB/day**.

## API Surface

### `GET /node/network/failover`

Returns the current failover state:

```json
{
  "state": "PrimaryOnline",
  "primary_interface": "eth0",
  "failover_interface": "wwan0",
  "is_metered": false,
  "modem_signal_dbm": -67,
  "last_primary_probe_ok": "2026-04-09T12:00:00Z",
  "failover_events_24h": 0,
  "uptime_primary_pct_24h": 100.0
}
```

### Integration with `/readyz`

The readyz endpoint should include a `network` check that degrades to `warn` (not `fail`) when on
failover, since the node is still operational.

## Data Usage Estimate

| Traffic type | Per block | Blocks/day (10-min tier) | Daily |
|-------------|-----------|-------------------------|-------|
| Consensus messages | ~2 KB | 144 | ~0.3 MB |
| Weather observations | ~1 KB × 6 per block | 144 | ~0.9 MB |
| Block headers | ~0.5 KB | 144 | ~0.07 MB |
| P2P gossip overhead | ~5 KB | 144 | ~0.7 MB |
| **Total** | | | **~2 MB** |

At the 10-minute block tier (heights 25,000--99,999), a cellular-only node uses roughly 2 MB/day
for core operations. A 1 GB/month prepaid data plan provides ~16× headroom.

---

_Last updated: 2026-04-09_
