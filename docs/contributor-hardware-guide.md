# WRFCoin Contributor Hardware Guide

**Date:** 2026-04-09
**Status:** Draft
**Linked issue:** #738
**See also:** [Hardware Integration Roadmap](hardware-integration-roadmap.md)

---

## Who This Guide Is For

You want to run a WRFCoin contributor node and earn WRF tokens by submitting weather observations
to the network. This guide covers what hardware to buy, how to set it up, and what economic return
to expect at each tier.

---

## Hardware Tiers at a Glance

| Tier | Additional Cost | What You Get |
|------|----------------|--------------|
| **Tier 1 — Minimum Viable** | ~$0 | Any Docker-capable machine + WRFCoin mobile app |
| **Tier 2 — RTL-SDR Enhanced** | ~$35--75 | Passive reception of neighborhood weather stations |
| **Tier 3 — Local Sensor Mesh** | ~$150--300 | Zigbee + LoRaWAN sensors you own and control |
| **Tier 4 — Resilient Area Node** | ~$300+ | LTE failover, WiFi mesh, maximum coverage |

Each tier stacks on the previous one. Start with Tier 1, add peripherals as you see the ROI.

---

## Tier 1 — Minimum Viable Node ($0 Additional)

### What You Need

- Any machine that runs Docker (Linux, macOS, or Windows with WSL2)
- Internet connection
- The WRFCoin mobile app on your phone (barometer + GPS)

### What You Earn

The mobile app contributes barometric pressure readings from your phone's sensor. These are valid
weather observations that flow through the commit-reveal pipeline and earn WRF tokens.

### Setup

1. Install Docker and Docker Compose on your machine.
2. Pull the WRFCoin node stack:
   ```bash
   cd infra/testnet
   docker compose up -d
   ```
3. Install the WRFCoin mobile app and link it to your node's API endpoint
   (`http://<your-node-ip>:4001`).
4. Observations are submitted automatically while the app is running.

---

## Tier 2 — RTL-SDR Enhanced Sensing ($35--75)

### What You Need

| Component | Example | Est. Cost |
|-----------|---------|-----------|
| RTL-SDR USB dongle | RTL-SDR Blog V4, Nooelec NESDR v5 | $25--35 |
| 433/915 MHz antenna | Nooelec ANT-433, RTL-SDR telescopic | $10--15 |
| USB extension cable | 1--3 m USB-A male/female | $5--10 |

### What It Does

The RTL-SDR dongle passively receives 433 MHz and 915 MHz ISM-band transmissions from consumer
weather stations (Acurite, LaCrosse, Oregon Scientific, Davis, Fine Offset) within a few hundred
meters of your location. The `rtl_433` software decodes these signals into structured weather data
(temperature, humidity, pressure, wind, rain) which your node submits to the WRFCoin network.

This is **receive-only** — no transmitting, no license required anywhere in the world.

### What You Earn

RTL-SDR observations carry RF provenance metadata (frequency, signal strength, device ID) that is
validated by the `has_valid_rf_provenance()` check in `shared-protocol`. Observations with valid RF
provenance are eligible for a reward bonus once capability scoring is activated via DAO governance
(see [Capability Scoring](#capability-scoring-and-reward-multipliers) below).

Even before the bonus activates, each decoded weather station contributes additional unique
observations to your node's publishing score, increasing your share of block rewards.

### Setup on the node host Reference Node

The reference deployment target is the MINISFORUM node host (AMD Ryzen AI 9 HX PRO 370) running
Proxmox VE with an LXC container (node container).

#### 1. USB Passthrough (Proxmox)

In the Proxmox web UI for node container:

1. Navigate to **Resources > Add > USB Device**.
2. Select the RTL-SDR dongle (vendor ID `0bda:2832` for most RTL2832U-based devices).
3. Restart the container.
4. Verify inside the container: `lsusb | grep RTL` should show the device.

#### 2. Add Services to Docker Compose

Add the following to `infra/testnet/docker-compose.override.yml`:

```yaml
services:
  rtl433:
    image: hertzg/rtl_433:latest
    devices:
      - /dev/bus/usb:/dev/bus/usb
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

volumes:
  mqtt_data:
  mqtt_logs:
```

#### 3. Start and Verify

```bash
cd infra/testnet
docker compose up -d rtl433 mqtt
# Watch decoded packets:
docker compose logs -f rtl433
```

You should see JSON lines for each decoded sensor packet. If no packets appear within a few
minutes, try moving the antenna closer to a window or outside.

#### 4. Ingestion Pipeline

The ingestion sidecar (issue #736, in progress) reads decoded JSON from the MQTT broker, maps it to
the canonical `wrf.weather.observation.v1` payload schema, and POSTs it to the backend API on
port 4001. Until that sidecar is available, RTL-SDR data is captured in MQTT but not yet
automatically submitted to the chain.

### Troubleshooting

| Symptom | Fix |
|---------|-----|
| `lsusb` shows nothing | Check Proxmox USB passthrough; restart node container |
| `rtl_433` exits with "no supported devices found" | Another process holds the USB device; stop any competing `rtl_sdr` or SDR# instances |
| No packets decoded | Move antenna; check that neighbor stations exist on 433/915 MHz |
| High noise floor | Use the USB extension cable to move the dongle away from the node host chassis |

---

## Tier 3 — Local Sensor Mesh ($150--300)

### What You Need (Zigbee)

| Component | Example | Est. Cost |
|-----------|---------|-----------|
| Zigbee USB coordinator | SONOFF Zigbee 3.0 Dongle Plus (EFR32MG21) | $20 |
| Zigbee temp/humidity sensors | SONOFF SNZB-02D, Aqara WSDCGQ11LM (x5--10) | $10--20 each |

### What You Need (LoRaWAN)

| Component | Example | Est. Cost |
|-----------|---------|-----------|
| LoRa USB gateway | RAK2287 USB, Seeed WM1302 | $80--150 |
| LoRa antenna (5 dBi omni) | 868 MHz or 915 MHz outdoor | $20--40 |
| LoRa end-node sensors | RAK WisBlock, Dragino LHT65 (xN) | $25--60 each |

### What It Does

- **Zigbee** creates a dense mesh of sensors within ~100 m. Place them indoors, outdoors, in
  gardens — each contributes unique micro-climate data.
- **LoRaWAN** covers kilometer-scale distances. A single gateway on your roof can aggregate data
  from sensors deployed across a rural area.

You **own** these sensors — unlike Tier 2 where you passively receive third-party stations, Tier 3
data is fully under your control.

### RF Regulations Note

- **Zigbee** (2.4 GHz): License-free worldwide at low power.
- **LoRa** (868/915 MHz): License-free ISM band, but regional EIRP (transmit power) limits apply.
  Check your country's regulations before deploying LoRa end-nodes. The gateway is receive-dominant
  but does transmit downlink commands.

### Setup

#### Zigbee

Add to `docker-compose.override.yml`:

```yaml
services:
  zigbee2mqtt:
    image: koenkk/zigbee2mqtt:latest
    volumes:
      - zigbee2mqtt_data:/app/data
    devices:
      - /dev/ttyUSB0:/dev/ttyACM0  # Adjust to your Zigbee dongle path
    environment:
      - TZ=UTC
    depends_on:
      - mqtt
    restart: unless-stopped

volumes:
  zigbee2mqtt_data:
```

Pair sensors through the Zigbee2MQTT web UI. Each sensor's readings are published to the MQTT
broker and follow the same ingestion path as RTL-SDR data.

#### LoRaWAN

Add to `docker-compose.override.yml`:

```yaml
services:
  chirpstack:
    image: chirpstack/chirpstack:4
    volumes:
      - chirpstack_data:/etc/chirpstack
    ports:
      - "8180:8080"  # Remapped from 8080 to avoid conflict with weather-earnings service
    depends_on:
      - mqtt
    restart: unless-stopped

  chirpstack-gateway-bridge:
    image: chirpstack/chirpstack-gateway-bridge:4
    depends_on:
      - mqtt
    restart: unless-stopped

volumes:
  chirpstack_data:
```

Register your gateway and devices through the ChirpStack web UI at `http://<node-ip>:8180`.

### Tested Sensor Compatibility

| Sensor | Protocol | Fields | Status |
|--------|----------|--------|--------|
| SONOFF SNZB-02D | Zigbee | Temperature, humidity | Verified |
| Aqara WSDCGQ11LM | Zigbee | Temperature, humidity, pressure | Verified |
| MOES ZSS-ZK-TH | Zigbee | Soil temperature, soil moisture | Tested |
| Dragino LHT65 | LoRaWAN | Temperature, humidity | Verified |
| Dragino LDDS75 | LoRaWAN | Distance (water level) | Tested |
| RAK WisBlock (RAK1901) | LoRaWAN | Temperature, humidity | Tested |

---

## Tier 4 — Resilient Area Node ($300+)

### What You Add

| Component | Example | Est. Cost |
|-----------|---------|-----------|
| LTE USB modem | Huawei E3372h, GL.iNet GL-XE300 | $40--80 |
| Prepaid data SIM | Carrier-dependent (IoT plan) | $5--20/mo |
| WiFi mesh adapter | Alfa AWUS036ACM (802.11s capable) | $15--35 |
| High-gain antennas | Outdoor omni for RTL-SDR and LoRa | $20--50 |

### What It Does

- **LTE failover** keeps your node online when your home ISP drops. The node automatically routes
  WRFCoin traffic over cellular until the primary connection recovers.
- **WiFi mesh (802.11s)** interconnects nearby contributor nodes so a single LTE gateway can cover
  an entire local cluster of nodes.
- **High-gain antennas** extend RTL-SDR reception and LoRa range significantly.

### Setup

LTE and mesh networking require host-level configuration (ModemManager, NetworkManager,
batman-adv kernel module). These are documented in the individual issue tracks:

- LTE failover: issue #731
- WiFi mesh: issues #733, #734
- WiFi AP mode: issue #739

---

## Reference Build: MINISFORUM node host

The project's reference contributor node:

| Spec | Detail |
|------|--------|
| CPU | AMD Ryzen AI 9 HX PRO 370 |
| RAM | 96 GB DDR5 |
| Storage | 4 TB NVMe + 2x 10 TB HDD |
| OS | Proxmox VE 8.x, LXC container node container |
| Network | 2.5 GbE wired + WiFi 6E |
| USB | 4x USB-A 3.2 (for RTL-SDR, Zigbee, LoRa, LTE) |

This machine runs the full Tier 4 stack: WRFCoin node, RTL-SDR, Zigbee coordinator, LoRa gateway,
and LTE failover simultaneously. It also serves as a validator and storage node.

### Deployment Constraints

- **Named Docker volumes only** — no ZFS bind mounts (node container uses unprivileged LXC).
- **Local builds only** — never push Docker images to ghcr.io or any registry.
- **3-file compose stack** — `docker-compose.yml`, `docker-compose.override.yml`, and
  `docker-compose.prod.yml` in `infra/testnet/`.
- **USB passthrough** — add USB devices via Proxmox UI (Resources > Add > USB Device).

---

## Expected Rewards and ROI

### Emission Schedule

WRFCoin uses a block-time progression schedule where block rewards decrease as block time
shortens, keeping daily emission approximately constant:

| Height Range | Block Time | Reward per Block | Blocks per Day | Daily Emission |
|-------------|------------|-----------------|----------------|----------------|
| 0--999 | 4 hours | 50 WRF | 6 | 300 WRF |
| 1,000--4,999 | 2 hours | 25 WRF | 12 | 300 WRF |
| 5,000--24,999 | 1 hour | 12 WRF | 24 | 288 WRF |
| 25,000--99,999 | 10 min | 2 WRF | 144 | 288 WRF |

Rewards are distributed across publishing roles (Analyzer 35%, Collector 30%, Keeper 25%,
Offsetter 10%) based on each node's publishing score in the Proof of Publishing consensus.

### ROI Estimates by Tier

These estimates assume a single-operator testnet (your node is the only contributor). On a
multi-operator network, rewards are split proportionally by publishing score.

| Tier | Hardware Cost | Monthly WRF Earned (est.) | Payback Period |
|------|-------------|--------------------------|----------------|
| Tier 1 (app only) | $0 | ~2,700 WRF | Immediate |
| Tier 2 (RTL-SDR) | $35--75 | ~4,050 WRF (+50% from additional stations) | < 1 month |
| Tier 3 (sensors) | $150--300 | ~5,400 WRF (+100% from owned sensors) | 1--2 months |
| Tier 4 (full) | $300+ | ~5,400 WRF + uptime bonus | 2--3 months |

**Important caveats:**

- WRF tokens have no market value during testnet. These figures describe token accumulation, not
  fiat returns.
- The "+50%" and "+100%" figures are illustrative projections based on additional unique
  observations increasing your publishing score share. Actual percentages depend on network
  conditions, number of competing contributors, and governance parameters.
- Capability score reward multipliers (1.15x--1.75x) are not yet active. They require a DAO
  governance proposal before activation.

### Capability Scoring and Reward Multipliers

The `NodeCapabilityProfile` (issue #735) will track which peripherals are active on each node and
apply reward multipliers. Proposed bands (subject to DAO governance vote):

| Band | Capability Score | Multiplier |
|------|-----------------|------------|
| Minimum viable | 0--19 | 1.00x |
| Tier 1 active (RTL-SDR) | 20--39 | 1.15x |
| Tier 2 partial (some sensors) | 40--59 | 1.30x |
| Tier 2 full (Zigbee + LoRa) | 60--79 | 1.50x |
| Tier 3 + mesh | 80--100 | 1.75x |

---

## Monitoring Your Node

The `GET /node/metrics/host` endpoint on the native-chain API (port 8556) returns live host
metrics:

```bash
curl http://localhost:8556/node/metrics/host | jq .
```

Response includes:

- **Memory**: total, used, free (MiB), utilization percent
- **Swap**: total, used (MiB)
- **CPU**: per-core usage, average usage percent
- **Disks**: per-mount total, used, available (GiB), utilization percent

Alert thresholds to watch:

| Metric | Warning | Critical |
|--------|---------|----------|
| Disk usage | > 80% | > 95% |
| Memory usage | > 85% | > 95% |
| Swap active | Any swap used | > 50% of swap |
| CPU temperature | > 80 C | > 90 C |

---

## Further Reading

- [Hardware Integration Roadmap](hardware-integration-roadmap.md) — detailed technical roadmap
  with Docker Compose snippets and core4 integration points
- [Canonical Weather Payload Schema](iot/CANONICAL_WEATHER_PAYLOAD_SCHEMA_V1.md) — the
  `wrf.weather.observation.v1` format all sensor data must conform to
- Issue #729 — RTL-SDR integration tracker
- Issue #735 — Node capability scoring
- Issue #747 — Miner dashboard host metrics

---

_Last updated: 2026-04-09_
