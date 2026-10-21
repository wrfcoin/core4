# WRFCoin Port Registry Documentation

This document provides a comprehensive reference for all network ports used by the WRFCoin platform's microservices architecture.

## Overview

The WRFCoin platform operates 20+ specialized microservices, each assigned a unique port for communication. This document serves as the authoritative reference for port allocation and should be consulted when:

- Adding new services
- Troubleshooting connectivity issues
- Configuring firewalls or load balancers
- Setting up development environments

## Port Allocation Map

### Core API Services (3000-3003)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 3000 | Cross-Chain Protocol Server | API | Multi-blockchain bridges and cross-chain operations | `/health` |
| 3001 | Missing Endpoints Server | API | Fallback endpoint handler | `/health` |
| 3002 | IoT Sensor Networks | Compute | Environmental sensor management (20+ platforms) | `/health` |
| 3003 | Weather Earnings Transfer | API | Weather station reward transfers | `/health` |

### Wallet & Application Services (4000-4002)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 4000 | Wallet Server | Application | BIP-39/44 wallet operations with HSM integration | `/health` |
| 4001 | Main API Server (Testnet) | Application | Primary application endpoints and GraphQL API | `/health` |
| 4002 | Performance Dashboard API | Application | Performance metrics and monitoring dashboard | `/health` |

### Frontend & WebSocket Services (5000-5001)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 5000 | Frontend Application | Frontend | React/TypeScript web interface | N/A |
| 5001 | Cross-Chain WebSocket | WebSocket | Real-time cross-chain event streaming | `/health` |

### Database Services (5432, 6379)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 5432 | PostgreSQL Database | Database | Consolidated 21-table schema | Native |
| 6379 | Redis Cache | Cache | Performance optimization and sessions | Native |

### Compute Services (6000-6004)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 6000 | Compute Networks Server | Compute | Distributed computing and task coordination | `/health` |
| 6001 | AI/ML Networks Server | Compute | Machine learning model deployment and inference | `/health` |
| 6003 | Compute Contribution Server | Compute | Computational power contribution and rewards | `/health` |
| 6004 | Enhanced Weather Rewards | Compute | Advanced weather station reward calculations | `/health` |

### Storage Services (7000-7002)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 7000 | Storage Networks Server | Storage | IPFS, Filecoin, Arweave integration | `/health` |
| 7002 | NAS Storage Network | Storage | Network-attached storage management | `/health` |

### Governance & WebSocket Services (8000-8003)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 8000 | WebSocket Server / Governance | WebSocket | DAO proposals, voting, treasury management | `/health` |
| 8002 | Enhanced Cross-Chain Server | WebSocket | Advanced cross-chain operations | `/health` |
| 8003 | Memory Optimized Server | WebSocket | Optimized WebSocket for high-load scenarios | `/health` |

### Integration & Discovery Services (9000-9001)

| Port | Service | Category | Description | Health Endpoint |
|------|---------|----------|-------------|-----------------|
| 9000 | Contract Integration Server | Integration | Smart contract deployment and interaction | `/health` |
| 9001 | Node Discovery Server | Integration | P2P network coordination and peer discovery | `/health` |

## Port Categories

### API Ports (3000-3999)
Reserved for REST API and external-facing services.

### Application Ports (4000-4999)
Reserved for core application servers and internal APIs.

### Frontend Ports (5000-5999)
Reserved for frontend applications and WebSocket connections.

### Compute Ports (6000-6999)
Reserved for distributed computing, AI/ML, and processing services.

### Storage Ports (7000-7999)
Reserved for storage networks and data persistence services.

### WebSocket/Governance Ports (8000-8999)
Reserved for real-time communication and governance services.

### Integration Ports (9000-9999)
Reserved for service discovery, integration, and monitoring services.

## Environment Variables

Services should use environment variables for port configuration:

```bash
# Core Services
CROSS_CHAIN_PORT=3000
IOT_SENSOR_PORT=3002
WALLET_PORT=4000
MAIN_API_PORT=4001
FRONTEND_PORT=5000

# Compute Services
COMPUTE_NETWORKS_PORT=6000
AI_ML_PORT=6001

# Storage Services
STORAGE_NETWORKS_PORT=7000

# Governance Services
GOVERNANCE_PORT=8000
WEBSOCKET_PORT=8000

# Discovery Services
NODE_DISCOVERY_PORT=9001

# Database Services
POSTGRES_PORT=5432
REDIS_PORT=6379
```

## Configuration Files

### Primary Configuration
- `backend/config/port-registry.json` - Central port allocation registry
- `backend/config/microservices.json` - Service architecture with port mappings

### Docker Configuration
- `infra/docker/docker-compose.yml` - Container port mappings
- `infra/docker/docker-compose.microservices.yml` - Microservices port mappings

## Validation

Run the port validation script to check for conflicts:

```bash
node scripts/validate-ports.js
```

This script checks:
- No duplicate port allocations
- All registered services have valid ports
- Ports are within expected ranges
- Health endpoints are accessible

## Reserved Ports

The following ports are reserved and should not be used for new services:

| Port | Reason |
|------|--------|
| 80 | HTTP standard |
| 443 | HTTPS standard |
| 8080 | API Gateway / Load Balancer |
| 9090 | Prometheus metrics |
| 3000 | Already allocated to Cross-Chain Protocol |

## Adding New Services

When adding a new service:

1. Check available ports in the appropriate category range
2. Update `backend/config/port-registry.json`
3. Update `backend/config/microservices.json` if applicable
4. Add environment variable to `.env.example`
5. Run `node scripts/validate-ports.js` to verify no conflicts
6. Update this documentation

## Troubleshooting

### Port Conflicts

If you encounter `EADDRINUSE` errors:

```bash
# Find process using the port
lsof -i :PORT_NUMBER

# Or on Windows
netstat -ano | findstr :PORT_NUMBER
```

### Service Discovery

Check if a service is running:

```bash
curl http://localhost:PORT/health
```

### Docker Port Mapping

Verify Docker port mappings:

```bash
docker ps --format "table {{.Names}}\t{{.Ports}}"
```

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2025-05-29 | Initial port registry creation |
| 1.1.0 | 2026-01-21 | Added documentation and validation |

---

_Last updated: January 2026_
