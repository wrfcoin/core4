# Wrfcoin Port Architecture (Centralized Registry)

_Generated: 2025-05-29T19:13:31.808Z_ _Registry Version: 1.0.0_ _Status: No conflicts_

## Current Port Assignments

### Frontend Layer (5000-5999)

- Frontend Application: 5000 🔴

### Api Layer (3000-3999)

- Cross-Chain Protocol Server: 3000 🔴
- Missing Endpoints Server: 3001 🔴
- Weather Earnings Transfer: 3003 🔵

### Application Layer (4000-4999)

- Wallet Server: 4000 🔴
- Testnet API Server: 4001 🔴
- Performance Dashboard API: 4002 🔴

### Websocket Layer (8000-8999)

- Cross-Chain WebSocket: 5001 🔴
- WebSocket Server Fixed: 8000 🔵
- Enhanced Cross-Chain Server: 8002 🔴
- Memory Optimized Server: 8003 🔴

### Compute Layer (6000-6999)

- IoT Sensor Networks: 3002 🔴
- Compute Networks Server: 6000 🔴
- AI ML Networks: 6001 🔴
- Enhanced Weather Rewards: 6004 🔵

### Storage Layer (7000-7999)

- Storage Networks: 7000 🔴

### Integration Layer (9000-9999)

- Contract Integration Server: 9000 🔴
- Node Discovery Server: 9001 🔴

## Port Range Utilization

| Category    | Range     | Used | Available |
| ----------- | --------- | ---- | --------- |
| management  | 2000-2999 | 0    | 1000      |
| api         | 3000-3999 | 4    | 996       |
| application | 4000-4999 | 3    | 997       |
| frontend    | 5000-5999 | 2    | 998       |
| compute     | 6000-6999 | 3    | 997       |
| storage     | 7000-7999 | 1    | 999       |
| websocket   | 8000-8999 | 3    | 997       |
| integration | 9000-9999 | 2    | 998       |

## Registry Management

Services are automatically assigned ports within appropriate ranges:

- **Management**: System control and monitoring services
- **API**: REST APIs and protocol servers
- **Application**: Business logic and specialized services
- **Frontend**: User interfaces (port 5000 reserved)
- **Compute**: AI/ML and computational services
- **Storage**: Data storage and management
- **WebSocket**: Real-time communication
- **Integration**: External integrations and discovery

Use the port registry system to register new services and prevent conflicts.
