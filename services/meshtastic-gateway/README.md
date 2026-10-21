# Meshtastic Gateway Service

Bridges LoRa mesh networks with the WRFCoin backend for weather data collection and mesh participation rewards.

## Overview

This Python microservice acts as a gateway between Meshtastic-capable LoRa nodes (e.g., Muzi R1 Neo) and the WRFCoin backend. It:

- Collects environmental and air-quality data from LoRa mesh nodes
- Forwards validated observations to the WRFCoin backend
- Enables mesh participation rewards for node operators
- Supports optional off-grid transaction relay

## Quick Start

### Development Mode (Mock Meshtastic)

```bash
# Create virtual environment
python -m venv venv
source venv/bin/activate  # Linux/Mac
# venv\Scripts\activate   # Windows

# Install dependencies
pip install -r requirements.txt

# Run with mock Meshtastic interface
MESHTASTIC_GW_MESHTASTIC_MODE=mock uvicorn src.gateway.main:app --reload --port 8001
```

### Docker

```bash
docker build -t wrfcoin-meshtastic-gateway .
docker run -p 8001:8001 \
  -e MESHTASTIC_GW_BACKEND_URL=http://host.docker.internal:4001 \
  wrfcoin-meshtastic-gateway
```

## Configuration

All configuration is via environment variables with the `MESHTASTIC_GW_` prefix:

| Variable | Default | Description |
|----------|---------|-------------|
| `MESHTASTIC_GW_GATEWAY_ID` | `gw-dev-001` | Gateway identifier |
| `MESHTASTIC_GW_BACKEND_URL` | `http://localhost:4001` | WRFCoin backend URL |
| `MESHTASTIC_GW_BACKEND_API_KEY` | `` | API key for backend |
| `MESHTASTIC_GW_MESHTASTIC_MODE` | `mock` | Mode: `mock`, `serial`, `tcp` |
| `MESHTASTIC_GW_MESHTASTIC_HOST` | `localhost` | TCP mode host |
| `MESHTASTIC_GW_MESHTASTIC_PORT` | `4403` | TCP mode port |
| `MESHTASTIC_GW_SERIAL_PORT` | `/dev/ttyUSB0` | Serial mode port |
| `MESHTASTIC_GW_OBSERVATION_INTERVAL_SECONDS` | `30` | Mock observation interval |
| `MESHTASTIC_GW_COMMAND_POLL_INTERVAL_SECONDS` | `10` | Backend command poll interval |
| `MESHTASTIC_GW_LOG_LEVEL` | `INFO` | Logging level |
| `MESHTASTIC_GW_HOST` | `0.0.0.0` | Server bind host |
| `MESHTASTIC_GW_PORT` | `8001` | Server port |

## API Endpoints

### Health & Status

- `GET /health` - Health check for container orchestration
- `GET /status` - Detailed gateway status
- `GET /nodes` - List all known mesh nodes
- `GET /nodes/{node_id}` - Get specific node details

### Backend Endpoints (on WRFCoin backend)

The gateway communicates with these backend endpoints:

- `POST /api/meshtastic/observation` - Submit weather observations
- `POST /api/meshtastic/tx-chunk` - Submit transaction chunks
- `GET /api/meshtastic/commands` - Poll for commands

## WRF Payload Format

The WRFCoin payload envelope format for LoRa transmission:

```json
{
  "t": "wrf",
  "v": 1,
  "kind": "obs",
  "body": {
    "ts": 1704067200,
    "lat": 37.335,
    "lon": -121.89,
    "tmp": 15.5,
    "hum": 65.0,
    "prs": 1013.25,
    "bat": 80
  }
}
```

## Testing

```bash
# Run all tests
pytest

# Run with coverage
pytest --cov=src.gateway --cov-report=html

# Run specific test file
pytest tests/test_gateway.py -v
```

## Project Structure

```
services/meshtastic-gateway/
├── src/
│   └── gateway/
│       ├── __init__.py
│       ├── main.py              # FastAPI application
│       ├── config.py            # Configuration settings
│       ├── meshtastic/
│       │   ├── __init__.py
│       │   ├── interface.py     # Abstract interface
│       │   └── mock.py          # Mock implementation
│       ├── api/
│       │   ├── __init__.py
│       │   └── routes.py        # API endpoints
│       └── models/
│           ├── __init__.py
│           └── payloads.py      # Pydantic models
├── tests/
│   └── test_gateway.py
├── requirements.txt
├── Dockerfile
└── README.md
```

## Next Milestones

- **Milestone 2 (Task 107)**: WRF payload encoding/decoding, observation validation
- **Milestone 3 (Task 108)**: Node registration and identity mapping
- **Milestone 4 (Task 109)**: Validation pipeline and storage
- **Milestone 5 (Task 110)**: Reward engine integration
- **Milestone 6 (Task 111)**: LoRa transaction relay
