# Wrfcoin Port Architecture

_Generated: 2025-05-29T19:08:06.340Z_

## Port Allocation Strategy

### Foundation Layer

- Database Setup: No specific port (uses DATABASE_URL)

### Protocol Layer (3000-3999, 6000-7999)

- Cross-Chain Protocol Server: 3000
- Storage Networks Server: 6002
- Compute Networks Server: 6000
- IoT Sensor Networks: 6003

### API Layer (3000-3999, 8000-8999)

- Unified API Server: 3001
- Weather Earnings Transfer: 6005
- Enhanced Weather Rewards: 6004
- AI/ML Networks: 6001

### WebSocket Layer (8000-8999, 5000-5999)

- Main WebSocket Server: 8080
- Cross-Chain WebSocket: 8000

### Integration Layer (9000-9999)

- Contract Integration Server: 9000
- Node Discovery Server: 9001

### Application Layer (5000-5999)

- Frontend Application: 5000
- AcuRite Collector: Process-based (no port)

### Management Layer (2000-2999)

- Service Control Panel: 2001
- Memory Optimized Server: Process-based (no port)

## Port Range Guidelines

| Category         | Range     | Purpose                          |
| ---------------- | --------- | -------------------------------- |
| Management       | 2000-2999 | Service control, monitoring      |
| API Services     | 3000-3999 | REST APIs, protocol servers      |
| Applications     | 5000-5999 | Frontend, user interfaces        |
| Compute Services | 6000-6999 | AI/ML, compute networks          |
| Storage/Data     | 7000-7999 | Storage networks, data services  |
| WebSockets       | 8000-8999 | Real-time communication          |
| Integrations     | 9000-9999 | External integrations, discovery |

## Conflict Resolution

This architecture ensures:

- No port conflicts between services
- Logical grouping by functionality
- Clear separation of concerns
- Easy identification of service types by port range

## Health Check Endpoints

All HTTP services expose `/health` endpoints for monitoring and DAG orchestration.
