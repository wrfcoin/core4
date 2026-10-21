# WRFCoin Hardware Integration Roadmap
# Weather Data Collection — Contributor Node Peripherals

**Date:** 2026-04-07  
**Status:** Draft  
**Linked issues:** #729 #730 #731 #732 #733 #734 #735 #736 #737 #738 #739 #740 #741 #742 #743 #744 #745 #746 #747

---

## Overview

This roadmap describes the hardware integration path for WRFCoin contributor nodes that want to
collect local weather and environmental data. The reference deployment target is the **node host**
(AMD Ryzen AI 9 HX PRO 370, node container, Proxmox VE at <NODE_LAN_IP>) running the 3-file Docker Compose
stack with named volumes (no ZFS bind mounts).

Hardware is organized into three tiers:

| Tier | Status | Description |
|------|--------|-------------|
| **Tier 1** | Testnet-ready | RTL-SDR USB passthrough on node host; minimal setup, immediate data contribution |
| **Tier 2** | Post-launch | LoRaWAN gateway + Zigbee sensor mesh; extends range and sensor density |
| **Tier 3** | Future | LTE failover, WiFi mesh (802.11s), WiFi AP mode; network resilience and multi-node coordination |

All tiers integrate with the existing `backend` IoT service (port 3002) and feed data through the
`shared-protocol` commit-reveal pipeline using the canonical `wrf.weather.observation.v1` payload
schema (see `docs/iot/CANONICAL_WEATHER_PAYLOAD_SCHEMA_V1.md`).

---

## Tier 1 — Testnet-Ready: RTL-SDR USB Passthrough

### Goal

Passive 433 MHz / 915 MHz ISM-band reception of consumer weather station RF transmissions using
`rtl_433`. Zero active transmit, zero regulatory licensing required. Runs on the node host testnet
node today.

**Issues:** #729, #736, #737, #738, #747

### Hardware BOM

| Component | Example SKU | Est. Cost | Notes |
|-----------|-------------|-----------|-------|
| RTL-SDR dongle | Nooelec NESDR v5 or RTL-SDR Blog V4 | $25–35 | Already in node host hardware manifest |
| Antenna (433/915 MHz) | Nooelec ANT-433 or RTL-SDR telescopic | $10–15 | Swap for outdoor omni for better range |
| USB-A extension cable | 1–3 m USB-A male/female | $5–10 | Move dongle away from RF noise inside chassis |
| (Optional) NOAA APT antenna | 137 MHz V-dipole | $15–25 | Required for issue #737 satellite imagery |

**Total Tier 1 addition to an existing node:** ~$0 (RTL-SDR already present) to ~$75 for a fresh setup.

### Software Dependencies

| Component | Version | Purpose |
|-----------|---------|---------|
| `rtl_433` Docker image | `hertzg/rtl_433:latest` | Decode ISM-band sensor packets |
| `mosquitto` or broker | 2.x | MQTT relay for `rtl_433` JSON output |
| WRFCoin ingestion sidecar | TBD (issue #736) | Validate, normalize, submit to backend |
| `node_exporter` | 1.7+ | Host disk/CPU/temp metrics (issue #747) |
| `rtlsdr-airband` | optional | For NOAA APT reception (issue #737) |

### node host Docker Compose Integration

Add a `rtl433` service to `infra/testnet/docker-compose.override.yml`:

```yaml
services:
  rtl433:
    image: hertzg/rtl_433:latest
    devices:
      - /dev/bus/usb:/dev/bus/usb   # USB passthrough via Proxmox IOMMU/USB device assignment
    command: >
      -F mqtt://mqtt:1883
      -M utc -M level
      -C si
    depends_on:
      - mqtt
    restart: unless-stopped

  mqtt:
    image: eclipse-mosquitto:2
    volumes:
      - mqtt_data:/mosquitto/data
      - mqtt_logs:/mosquitto/log
    ports:
      - "1883:1883"
    restart: unless-stopped

  node-exporter:
    image: prom/node-exporter:v1.7.0
    pid: host
    volumes:
      - /proc:/host/proc:ro
      - /sys:/host/sys:ro
      - /:/rootfs:ro
    command:
      - '--path.procfs=/host/proc'
      - '--path.sysfs=/host/sys'
      - '--collector.filesystem.mount-points-exclude=^/(sys|proc|dev|host|etc)($$|/)'
    restart: unless-stopped

volumes:
  mqtt_data:
  mqtt_logs:
```

> **Proxmox USB passthrough:** In the Proxmox UI for node container, add the RTL-SDR USB device under
> Resources → Add → USB Device (by vendor ID `0bda:2832` for RTL-SDR).

### core4 Integration Points

| Module | File | Role |
|--------|------|------|
| `shared-protocol` | `src/weather/validation.rs` | Physics range checks on decoded sensor readings |
| `shared-protocol` | `src/contributions/commit_reveal.rs` | `ApplyReveal` validates `wrf.weather.observation.v1` payloads |
| `backend` IoT service | `server/services/weather-ingestion-validator.js` | Source credibility, freshness, anti-fraud |
| `backend` weather earnings | port 8080 | Reward distribution for validated observations |

The ingestion sidecar from issue #736 reads `rtl_433` JSON from MQTT, maps to
`wrf.weather.observation.v1`, and posts to `POST /api/weather/observations` on the backend API
(port 4001).

### Linked Issues

- **#729** — RTL-SDR integration: `rtl_433` Docker passthrough
- **#736** — RTL-SDR ingestion microservice: JSON → backend pipeline
- **#737** — NOAA APT satellite imagery via RTL-SDR
- **#738** — Contributor hardware guide (this document is a precursor)
- **#747** — Miner dashboard host metrics: `node_exporter` integration

---

## Tier 2 — Post-Launch: LoRaWAN Gateway + Zigbee Sensor Mesh

### Goal

Active local sensor networks: Zigbee mesh for dense indoor/outdoor sensor clusters (up to ~100 m),
LoRaWAN gateway for kilometer-scale rural coverage. Enables the node operator to control their own
sensing infrastructure rather than passively receiving third-party transmissions.

**Issues:** #730, #732, #735

### Hardware BOM

#### LoRaWAN (issue #732)

| Component | Example SKU | Est. Cost | Notes |
|-----------|-------------|-----------|-------|
| LoRa USB gateway / concentrator | RAK2287 USB or Seeed WM1302 | $80–150 | Connects via USB or SPI hat |
| High-gain LoRa antenna | 868 MHz / 915 MHz 5 dBi omni | $20–40 | Outdoor mount for maximum range |
| LoRa end-node sensors | RAK WisBlock, Dragino LDDS75, LHT65 (×N) | $25–60 each | Temperature, humidity, pressure, soil |

**LoRaWAN per-gateway BOM total:** ~$120–250 (gateway) + sensors

#### Zigbee (issue #730)

| Component | Example SKU | Est. Cost | Notes |
|-----------|-------------|-----------|-------|
| Zigbee USB coordinator | SONOFF Zigbee 3.0 Dongle Plus (EFR32MG21) | $20 | Already in node host hardware manifest |
| Zigbee sensors | SONOFF SNZB-02D, Aqara WSDCGQ11LM (×N) | $10–20 each | Temp/humidity; ~$15 average |
| Zigbee soil sensors | MOES ZSS-ZK-TH or FCC approved alt | $15–25 each | Outdoor/garden |

**Zigbee BOM total (10-sensor mesh):** ~$170–220

### Software Dependencies

| Component | Version | Purpose |
|-----------|---------|---------|
| `zigbee2mqtt` | 1.36+ | Zigbee coordinator → MQTT bridge |
| `chirpstack` | 4.x | LoRaWAN network server (Docker) |
| `chirpstack-gateway-bridge` | 4.x | Gateway ↔ ChirpStack protocol bridge |
| `node-red` (optional) | 3.x | Visual flow editor for sensor → WRFCoin pipeline |

### node host Docker Compose Integration

```yaml
services:
  zigbee2mqtt:
    image: koenkk/zigbee2mqtt:latest
    volumes:
      - zigbee2mqtt_data:/app/data
    devices:
      - /dev/ttyUSB0:/dev/ttyACM0   # Adjust to actual Zigbee dongle path
    environment:
      - TZ=UTC
    depends_on:
      - mqtt
    restart: unless-stopped

  chirpstack:
    image: chirpstack/chirpstack:4
    volumes:
      - chirpstack_data:/etc/chirpstack
    ports:
      - "8080:8080"    # ChirpStack UI (use internal network only — conflicts with weather-earnings on 8080; remap to 8180)
    depends_on:
      - mqtt
    restart: unless-stopped

  chirpstack-gateway-bridge:
    image: chirpstack/chirpstack-gateway-bridge:4
    depends_on:
      - mqtt
    restart: unless-stopped

volumes:
  zigbee2mqtt_data:
  chirpstack_data:
```

> **Port conflict:** ChirpStack UI defaults to 8080; the WRFCoin weather-earnings service also runs
> on 8080. Remap ChirpStack to 8180 in the override file.

### core4 Integration Points

| Module | File | Role |
|--------|------|------|
| `backend` IoT service | `server/services/iot-sensor-network.js` | LoRaWAN and Zigbee platform connectors |
| `backend` IoT service | port 3002 endpoints | `POST /sensors/register-device`, `POST /sensors/aggregate-data` |
| `shared-protocol` | `src/weather/validation.rs` | Validates readings from Zigbee/LoRa sensors same as RTL-SDR |
| `native-chain` consensus | reward weighting | `NodeCapabilityProfile` (issue #735) scores nodes with Zigbee/LoRa active |

The `NodeCapabilityProfile` struct (issue #735) tracks which peripherals are registered and active.
Nodes with Tier 2 hardware should receive a capability multiplier in PoP reward scoring.

### Linked Issues

- **#730** — Zigbee coordinator: Zigbee2MQTT in Docker
- **#732** — LoRaWAN gateway: ChirpStack + LoRa concentrator
- **#735** — Contributor capability scoring: `NodeCapabilityProfile` reward multipliers

---

## Tier 3 — Future: LTE Failover, WiFi Mesh, WiFi AP Mode

### Goal

Network resilience and multi-node coordination. LTE/5G provides WAN failover when the contributor's
home ISP drops. WiFi mesh (802.11s) interconnects nearby contributor nodes so a single LTE gateway
covers an entire local cluster. WiFi AP mode exposes a direct SSID for mobile app connectivity.

**Issues:** #731, #733, #734, #739

### Hardware BOM

| Component | Example SKU | Est. Cost | Notes |
|-----------|-------------|-----------|-------|
| USB LTE modem | Huawei HiLink E3372h, GL.iNet GL-XE300 | $40–80 | SIM + data plan extra |
| 802.11s WiFi adapter (mesh) | TP-Link TL-WN722N v1, Alfa AWUS036ACM | $15–35 | Must support AP + mesh mode |
| Dual-function USB WiFi (AP mode) | Alfa AWUS036ACH | $30–50 | For issue #739 direct connectivity |
| Prepaid SIM / data plan | Carrier-dependent | $5–20/mo | IoT data SIM preferred |

**Tier 3 hardware addition:** ~$55–165 (LTE modem) + $15–50 (WiFi adapter) per node

### Software Dependencies

| Component | Version | Purpose |
|-----------|---------|---------|
| `ModemManager` + `NetworkManager` | system | LTE modem management |
| `batman-adv` kernel module | 2024.x | IEEE 802.11s mesh routing |
| `hostapd` | 2.x | WiFi AP mode (issue #739) |
| Custom mesh coordinator | TBD (issue #733) | Gossip protocol bridge between mesh nodes |

### Architecture

```
                      Internet
                          │
                    ┌─────┴──────┐
                    │  node host     │  ← LTE failover active when ISP drops (#731, #734)
                    │  (node container)   │
                    └─────┬──────┘
                          │ 802.11s WiFi mesh
              ┌───────────┼───────────┐
              │           │           │
        Neighbor     Neighbor     Neighbor    ← Community contributor nodes (#733)
        node A       node B       node C
              │
        Direct WiFi AP (#739) ← Mobile app / additional sensors
```

During ISP outage, the node host LTE modem becomes the mesh gateway. All mesh neighbors route
WRFCoin traffic through it until the primary ISP recovers. This is transparent to the
blockchain — nodes remain online and continue submitting weather observations.

### core4 Integration Points

| Module | File / Interface | Role |
|--------|-----------------|------|
| `p2p` | `src/network.rs` | Mesh nodes remain reachable via P2P; no code change needed if IP connectivity is maintained |
| `native-chain` | `/readyz` endpoint | `NODE_SYNC_STALE_AFTER_SECS = 600`; LTE failover must restore connectivity within this window |
| `backend` IoT service | port 3002 | Mobile app direct connections via AP mode submit to same backend API |
| `NodeCapabilityProfile` | issue #735 | LTE failover uptime metric feeds capability score |

### Linked Issues

- **#731** — LTE/5G USB modem: WAN failover
- **#733** — WiFi mesh networking: 802.11s inter-node
- **#734** — Mesh resilience via LTE gateway: cellular bridge for mesh during outages
- **#739** — WiFi AP mode: direct device connectivity

---

## Cross-Tier: Capability Scoring (Issue #735)

The `NodeCapabilityProfile` struct tracks hardware presence across all tiers and feeds reward
multipliers into the PoP consensus engine.

### Proposed Profile Fields

```rust
pub struct NodeCapabilityProfile {
    /// Tier 1
    pub rtl_sdr_active: bool,
    pub rtl_sdr_sensor_count_24h: u32,    // decoded packets in last 24h

    /// Tier 2
    pub zigbee_active: bool,
    pub zigbee_sensor_count: u32,          // registered sensors
    pub lorawan_active: bool,
    pub lorawan_device_count: u32,

    /// Tier 3
    pub lte_failover_active: bool,
    pub mesh_peer_count: u32,
    pub wifi_ap_active: bool,

    /// Composite
    pub capability_score: u32,             // 0–100
    pub uptime_30d_pct: f32,
}
```

Capability score bands and reward multipliers are governance-controlled parameters (DAO proposal
required before activation). Suggested starting values:

| Band | Score | Multiplier |
|------|-------|-----------|
| Minimum viable | 0–19 | 1.00× |
| Tier 1 active | 20–39 | 1.15× |
| Tier 2 partial | 40–59 | 1.30× |
| Tier 2 full | 60–79 | 1.50× |
| Tier 3 + mesh | 80–100 | 1.75× |

---

## Cross-Tier: Contributor Hardware Guide (Issue #738)

A companion `docs/hardware-guide.md` document (separate from this roadmap) should cover:

- **Economic ROI estimates** per tier at current WRF emission rates
- **Step-by-step setup** for the Tier 1 RTL-SDR Docker container on node host
- **Troubleshooting** for USB passthrough in Proxmox LXC containers
- **Sensor compatibility matrix** (tested Zigbee and LoRa sensor models)
- **RF regulations** note: RTL-SDR is receive-only; LoRa transmit at 868/915 MHz requires
  compliance with regional EIRP limits (check local regulations before deploying LoRa end-nodes)

---

## Issues Not Hardware-Specific (#740–#747 Context)

The following issues in the #729–#747 range address code quality and CI, not hardware integration:

| Issue | Title | Notes |
|-------|-------|-------|
| #740 | Remove 44 cargo check warnings | Rust workspace cleanup — handled separately |
| #741 | Network partition recovery and state convergence | Protocol tests — handled separately |
| #742 | cargo-fuzz targets + proptest for consensus | Fuzz testing — handled separately |
| #743 | Replace 3 consensus-crashing panics | Panic hardening — handled separately |
| #744 | Reproducible builds | Toolchain pinning — affects all tiers (node binary determinism) |
| #745 | CI pipeline non-functional (billing limits) | CI health — tracked in project_ci_health.md |
| #746 | Semantic versioning and release process | Operator UX — affects deployment of hardware service images |
| #747 | Miner dashboard host metrics | `node_exporter` covered in Tier 1 above |

Issues #744 and #746 have indirect hardware relevance: deterministic builds (#744) ensure all
contributor nodes run identical binaries, and semantic versioning (#746) lets operators know which
image version includes the hardware service integrations.

---

## Implementation Sequencing

```
Now (testnet)
  └── Tier 1: RTL-SDR
        ├── USB passthrough in Proxmox node container (manual step — Proxmox UI)
        ├── Add rtl433 + mqtt + node_exporter to docker-compose.override.yml
        ├── Build ingestion sidecar (#736)
        └── Wire Grafana dashboard to node_exporter (#747)

Post-launch (Q3 2026)
  └── Tier 2: Zigbee + LoRaWAN
        ├── Add zigbee2mqtt + chirpstack to docker-compose.override.yml
        ├── Implement NodeCapabilityProfile in shared-protocol (#735)
        ├── DAO proposal: capability score reward multipliers
        └── Publish contributor hardware guide (#738)

Future (Q4 2026+)
  └── Tier 3: LTE + mesh + AP
        ├── LTE modem integration (#731)
        ├── 802.11s mesh coordination (#733, #734)
        └── WiFi AP sidecar (#739)
```

---

## Deployment Constraints

- **Named Docker volumes only** — no ZFS bind mounts (node container uses unprivileged LXC)
- **Local builds only** — never push images to ghcr.io or any registry
- **3-file compose stack** — `docker-compose.yml`, `docker-compose.override.yml`,
  and `docker-compose.prod.yml` in `infra/testnet/`
- **USB passthrough** — USB devices must be added via Proxmox UI (Resources → USB Device) for
  node container; they appear as `/dev/ttyUSB*` or `/dev/bus/usb` inside the container
- **`NODE_SYNC_STALE_AFTER_SECS = 600`** — hardware integrations must not block the node event
  loop; all sensor ingestion runs in separate containers/processes

---

_Last updated: 2026-04-07_
