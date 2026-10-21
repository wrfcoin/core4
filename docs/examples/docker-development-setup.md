# Docker Development Setup

Complete Docker development environment for Wrfcoin blockchain platform.

## Quick Start

```bash
# Clone the repository
git clone https://github.com/wrfcoin/platform.git
cd wrfcoin-platform

# Start development environment
docker-compose up -d

# Check status
docker-compose ps
```

## Docker Compose Configuration

### Main Development Stack

```yaml
# docker-compose.yml
version: '3.8'

services:
  # PostgreSQL Database
  postgres:
    image: postgres:16
    environment:
      POSTGRES_DB: wrfcoin_dev
      POSTGRES_USER: wrfcoin
      POSTGRES_PASSWORD: development
    ports:
      - '5432:5432'
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./backend/drizzle/migrations:/docker-entrypoint-initdb.d
    healthcheck:
      test: ['CMD-SHELL', 'pg_isready -U wrfcoin']
      interval: 10s
      timeout: 5s
      retries: 5

  # Redis Cache
  redis:
    image: redis:7-alpine
    ports:
      - '6379:6379'
    volumes:
      - redis_data:/data
    command: redis-server --appendonly yes

  # Blockchain Core (Rust)
  blockchain-core:
    build:
      context: ./blockchain-core
      dockerfile: Dockerfile.dev
    ports:
      - '9000:9000' # P2P network
      - '8080:8080' # API server
    environment:
      - RUST_LOG=debug
      - DATABASE_URL=postgres://wrfcoin:development@postgres:5432/wrfcoin_dev
      - REDIS_URL=redis://redis:6379
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_started
    volumes:
      - ./blockchain-core:/app
      - blockchain_data:/app/data
    command: cargo watch -x run

  # Backend Services (Node.js)
  backend:
    build:
      context: ./backend
      dockerfile: Dockerfile.dev
    ports:
      - '3001:3001' # Enhanced API
      - '4000:4000' # Wallet Server
      - '8000:8000' # Governance Server
    environment:
      - NODE_ENV=development
      - DATABASE_URL=postgres://wrfcoin:development@postgres:5432/wrfcoin_dev
      - REDIS_URL=redis://redis:6379
      - BLOCKCHAIN_RPC_URL=http://blockchain-core:8080
    depends_on:
      - postgres
      - redis
      - blockchain-core
    volumes:
      - ./backend:/app
      - /app/node_modules
    command: npm run dev

  # Frontend Application (React)
  frontend:
    build:
      context: ./frontend
      dockerfile: Dockerfile.dev
    ports:
      - '5000:5000'
    environment:
      - VITE_API_URL=http://localhost:3001
      - VITE_WS_URL=ws://localhost:5001
    depends_on:
      - backend
    volumes:
      - ./frontend:/app
      - /app/node_modules
    command: npm run dev

  # Cross-Chain Services
  cross-chain:
    build:
      context: ./backend/server
      dockerfile: Dockerfile.cross-chain
    ports:
      - '3000:3000' # Cross-chain protocol
      - '5001:5001' # WebSocket server
    environment:
      - NODE_ENV=development
      - DATABASE_URL=postgres://wrfcoin:development@postgres:5432/wrfcoin_dev
    depends_on:
      - postgres
      - blockchain-core
    volumes:
      - ./backend/server:/app

  # IoT Sensor Networks
  iot-sensors:
    build:
      context: ./backend/server
      dockerfile: Dockerfile.iot
    ports:
      - '3002:3002'
    environment:
      - NODE_ENV=development
      - DATABASE_URL=postgres://wrfcoin:development@postgres:5432/wrfcoin_dev
    depends_on:
      - postgres
    volumes:
      - ./backend/server:/app

  # Development Tools
  pgadmin:
    image: dpage/pgadmin4:latest
    environment:
      PGADMIN_DEFAULT_EMAIL: admin@wrfcoin.dev
      PGADMIN_DEFAULT_PASSWORD: development
    ports:
      - '5050:80'
    depends_on:
      - postgres

volumes:
  postgres_data:
  redis_data:
  blockchain_data:

networks:
  default:
    name: wrfcoin-dev
```

## Dockerfiles

### Blockchain Core (Rust)

```dockerfile
# blockchain-core/Dockerfile.dev
FROM rust:1.70

WORKDIR /app

# Install development dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-watch for hot reloading
RUN cargo install cargo-watch

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./

# Create dummy main to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy source code
COPY . .

# Expose ports
EXPOSE 8080 9000

# Development command (overridden in docker-compose)
CMD ["cargo", "run"]
```

### Backend Services (Node.js)

```dockerfile
# backend/Dockerfile.dev
FROM node:20

WORKDIR /app

# Install development tools
RUN npm install -g nodemon tsx

# Copy package files
COPY package*.json ./

# Install dependencies
RUN npm install

# Copy source code
COPY . .

# Expose ports
EXPOSE 3001 4000 8000

# Development command
CMD ["npm", "run", "dev"]
```

### Frontend (React)

```dockerfile
# frontend/Dockerfile.dev
FROM node:20

WORKDIR /app

# Copy package files
COPY package*.json ./

# Install dependencies
RUN npm install

# Copy source code
COPY . .

# Expose port
EXPOSE 5000

# Development command
CMD ["npm", "run", "dev"]
```

## Development Scripts

### Package.json Scripts

```json
{
  "scripts": {
    "docker:up": "docker-compose up -d",
    "docker:down": "docker-compose down",
    "docker:logs": "docker-compose logs -f",
    "docker:build": "docker-compose build",
    "docker:clean": "docker-compose down -v && docker system prune -f",
    "dev:full": "docker-compose up",
    "dev:backend": "docker-compose up postgres redis backend",
    "dev:frontend": "docker-compose up frontend",
    "dev:blockchain": "docker-compose up postgres redis blockchain-core",
    "test:integration": "docker-compose -f docker-compose.test.yml up --abort-on-container-exit",
    "migrate": "docker-compose exec backend npm run db:migrate",
    "seed": "docker-compose exec backend npm run db:seed"
  }
}
```

### Environment Configuration

```bash
# .env.development
NODE_ENV=development
DATABASE_URL=postgres://wrfcoin:development@localhost:5432/wrfcoin_dev
REDIS_URL=redis://localhost:6379
BLOCKCHAIN_RPC_URL=http://localhost:8080

# API Keys (replace with your own)
WEATHER_API_KEY=your_weather_api_key
IPFS_API_KEY=your_ipfs_api_key
ETHEREUM_RPC_URL=https://eth-goerli.g.alchemy.com/v2/your_key

# Development settings
DEBUG=wrfcoin:*
LOG_LEVEL=debug
HOT_RELOAD=true
```

## Hot Reloading Configuration

### Rust (blockchain-core)

```toml
# .cargo/config.toml
[build]
target-dir = "/tmp/target"

[env]
RUST_LOG = "debug"
```

### Node.js (backend)

```json
{
  "nodemonConfig": {
    "watch": ["src/**/*", "server/**/*"],
    "ext": "ts,js,json",
    "ignore": ["node_modules", "dist", "logs"],
    "exec": "tsx src/index.ts",
    "env": {
      "NODE_ENV": "development"
    }
  }
}
```

### React (frontend)

```javascript
// vite.config.js
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    host: '0.0.0.0',
    port: 5000,
    hmr: {
      port: 5000,
    },
    watch: {
      usePolling: true,
    },
  },
  optimizeDeps: {
    include: ['@emotion/react', '@emotion/styled', '@mui/material'],
  },
});
```

## IDE Configuration

### VSCode Settings

```json
{
  "editor.formatOnSave": true,
  "editor.codeActionsOnSave": {
    "source.fixAll.eslint": true
  },
  "rust-analyzer.cargo.target": "x86_64-unknown-linux-gnu",
  "rust-analyzer.checkOnSave.command": "clippy",
  "typescript.preferences.importModuleSpecifier": "relative",
  "eslint.workingDirectories": ["backend", "frontend"],
  "docker.containers.groupBy": "compose"
}
```

### Extensions

```json
{
  "recommendations": [
    "rust-lang.rust-analyzer",
    "ms-vscode.vscode-typescript-next",
    "bradlc.vscode-tailwindcss",
    "ms-azuretools.vscode-docker",
    "esbenp.prettier-vscode",
    "dbaeumer.vscode-eslint"
  ]
}
```

## Testing Setup

### Integration Tests

```yaml
# docker-compose.test.yml
version: '3.8'

services:
  postgres-test:
    image: postgres:16
    environment:
      POSTGRES_DB: wrfcoin_test
      POSTGRES_USER: test
      POSTGRES_PASSWORD: test
    tmpfs:
      - /var/lib/postgresql/data

  test-runner:
    build:
      context: .
      dockerfile: Dockerfile.test
    environment:
      - NODE_ENV=test
      - DATABASE_URL=postgres://test:test@postgres-test:5432/wrfcoin_test
    depends_on:
      - postgres-test
    command: npm run test:integration
```

## Performance Monitoring

### Development Metrics

```yaml
# Add to docker-compose.yml
prometheus:
  image: prom/prometheus:latest
  ports:
    - '9090:9090'
  volumes:
    - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml

grafana:
  image: grafana/grafana:latest
  ports:
    - '3000:3000'
  environment:
    - GF_SECURITY_ADMIN_PASSWORD=development
  volumes:
    - grafana_data:/var/lib/grafana
```

## Debugging

### Remote Debugging

```bash
# Debug Node.js backend
docker-compose exec backend node --inspect=0.0.0.0:9229 src/index.js

# Debug Rust blockchain core
docker-compose exec blockchain-core rust-gdb target/debug/wrfcoin-core
```

### Log Aggregation

```bash
# View all logs
docker-compose logs -f

# View specific service logs
docker-compose logs -f backend

# Search logs
docker-compose logs | grep "ERROR"
```

## Maintenance

### Database Management

```bash
# Reset database
docker-compose down postgres
docker volume rm wrfcoin-platform_postgres_data
docker-compose up postgres

# Backup database
docker-compose exec postgres pg_dump -U wrfcoin wrfcoin_dev > backup.sql

# Restore database
docker-compose exec -T postgres psql -U wrfcoin wrfcoin_dev < backup.sql
```

### Clean Up

```bash
# Remove all containers and volumes
docker-compose down -v

# Clean up Docker system
docker system prune -af

# Reset development environment
make clean && make setup
```

## Troubleshooting

### Common Issues

1. **Port conflicts**: Check if ports are already in use
2. **Volume permissions**: Ensure Docker has access to project directory
3. **Memory issues**: Increase Docker memory allocation
4. **Network issues**: Reset Docker networks

### Health Checks

```bash
# Check all services
docker-compose ps

# Test API endpoints
curl http://localhost:3001/health
curl http://localhost:8080/health

# Test database connection
docker-compose exec postgres psql -U wrfcoin -d wrfcoin_dev -c "SELECT 1;"
```

This Docker development setup provides a complete, reproducible environment for Wrfcoin development
with hot reloading, debugging capabilities, and integrated testing.
