# Wrfcoin Port Architecture (Comprehensive Audit)

_Generated: 2025-05-29T19:08:57.871Z_ _Status: 7 conflicts detected, 12 undocumented services_

## Current Port Assignments

### API Layer (3000-3999)

- Cross-Chain Protocol Server: 3000
- Missing Endpoints Server / Unified API Server: 3001

### Application Layer (4000-4999)

- Wallet Server: 4000
- Testnet API Server: 4001
- Performance Dashboard API: 4002
- WebSocket Server Fixed: 4002 ⚠️ CONFLICT

### Frontend Layer (5000-5999)

- Frontend Application: 5000
- Cross-Chain WebSocket: 5001

### Compute Layer (6000-6999)

- Compute Networks Server: 6000
- AI/ML Networks: 6001

### Storage Layer (7000-7999)

- Storage Networks: 7000

### WebSocket Layer (8000-8999)

- Enhanced Cross-Chain Server: 8002
- Memory Optimized Server: 8003

### Integration Layer (9000-9999)

- Contract Integration Server: 9000
- Node Discovery Server: 9001

## Detected Issues

### Port Conflicts

- Port 3001: Missing Endpoints Server, Unified API Server, Unified API Server Port 3001
- Port 4002: Performance Dashboard API, WebSocket Server Fixed
- Port 4001: Testnet API Server, Testnet API Server Port 4001
- Port 8001: enhanced weather rewards, main api server
- Port 8000: governance server, iot sensor networks server, main api server
- Port 3003: main api server, weather earnings transfer
- Port 5004: server, server

### Undocumented Services

- enhanced weather rewards: 8001 (backend/server/enhanced_weather_rewards.js)
- governance server: 8000 (backend/server/governance_server.js)
- iot sensor networks server: 8000 (backend/server/iot_sensor_networks_server.js)
- main api server: 8001 (backend/server/main_api_server.js)
- main api server: 3003 (backend/server/main_api_server.js)
- main api server: 8000 (backend/server/main_api_server.js)
- server: 5002 (backend/server/server.js)
- server: 5005 (backend/server/server.js)
- server: 5004 (backend/server/server.js)
- server: 5004 (backend/server/server.js)
- weather earnings transfer: 3003 (backend/server/weather_earnings_transfer.js)
- weather station connector: 4200 (backend/server/weather_station_connector.js)

## Recommendations

1. **Resolve Port 4002 Conflict**: Performance Dashboard API and WebSocket Server Fixed both use
   port 4002
2. **Update Documentation**: Add all active services to port architecture docs
3. **Standardize Naming**: Ensure workflow names match service purposes
4. **Range Compliance**: Move services to appropriate port ranges

## Health Check Endpoints

All HTTP services should expose `/health` endpoints for monitoring.
