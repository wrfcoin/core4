# Wrfcoin Deployment Guide

This guide provides detailed instructions for deploying Wrfcoin Core in various environments, from
development to production. Whether you're setting up a testnet node for development, a mainnet node
for participation in the Wrfcoin network, or a production deployment for an application, this
document will guide you through the process.

## Table of Contents

1. [Deployment Overview](#deployment-overview)
2. [Prerequisites](#prerequisites)
3. [Local Deployment](#local-deployment)
4. [Server Deployment](#server-deployment)
5. [Cloud Deployment](#cloud-deployment)
6. [Docker Deployment](#docker-deployment)
7. [Configuration](#configuration)
8. [Database Setup](#database-setup)
9. [Security Considerations](#security-considerations)
10. [Monitoring and Maintenance](#monitoring-and-maintenance)
11. [Troubleshooting](#troubleshooting)

## Deployment Overview

Wrfcoin Core can be deployed in several different configurations:

1. **Development Mode**: For local development and testing with testnet configuration
2. **Production Mode**: For running a mainnet node in the Wrfcoin network
3. **API-Only Mode**: For applications that only need to interact with the blockchain API
4. **Full Node**: Complete node with all components (blockchain, API, WebSocket, frontend)

The deployment process varies slightly for each configuration, but the core steps remain consistent.

## Prerequisites

Before deploying Wrfcoin Core, ensure you have:

1. **Operating System**: Linux (Ubuntu 22.04+ recommended), macOS, or Windows 10/11
2. **Hardware Requirements**:

   - CPU: 4+ cores (8+ recommended for production)
   - RAM: 8GB+ (16GB+ recommended for production)
   - Storage: 100GB+ SSD (500GB+ recommended for production)
   - Network: Stable internet connection with 10+ Mbps bandwidth

3. **Software Requirements**:

   - Node.js 20.x or later
   - Rust 1.70 or later
   - PostgreSQL 15 or later
   - Git
   - Docker (optional, for containerized deployment)

4. **Network Requirements**:
   - Open ports: 8545 (API), 8546 (WebSocket), 5000 (Frontend)
   - Firewall rules for allowing incoming connections on these ports

## Local Deployment

### Setting Up a Local Development Environment

Follow these steps to deploy Wrfcoin Core locally for development:

1. **Clone the repository**:

```bash
git clone https://github.com/wrfcoin/wrfcoin-core.git
cd wrfcoin-core
```

2. **Install dependencies**:

```bash
npm install
```

3. **Set up environment variables**:

Create a `.env` file in the root directory with the following variables:

```
# Network Configuration
NETWORK=testnet
NODE_ENV=development

# Database Configuration
DATABASE_URL=postgres://username:password@localhost:5432/wrfcoin

# API Configuration
API_PORT=8545
API_HOST=0.0.0.0

# WebSocket Configuration
WS_PORT=8546
WS_HOST=0.0.0.0

# Frontend Configuration
FRONTEND_PORT=5000
FRONTEND_HOST=0.0.0.0
```

4. **Set up the database**:

```bash
# Create database
createdb wrfcoin

# Push schema
npm run db:push
```

5. **Start the development environment**:

```bash
# Start all services
npm run start:testnet
```

This will start the API server, WebSocket server, and frontend development server.

### Running a Local Testnet

To run a local testnet for development and testing:

```bash
# Start testnet
npm run start:testnet
```

This script launches all required services with testnet configuration:

- API Server (http://localhost:8545)
- WebSocket Server (ws://localhost:8546)
- Frontend Server (http://localhost:5000)

## Server Deployment

### Deploying on a Dedicated Server

For production deployments on a dedicated server, follow these steps:

1. **Prepare the server**:

```bash
# Update system packages
sudo apt update
sudo apt upgrade -y

# Install required dependencies
sudo apt install -y build-essential git curl postgresql postgresql-contrib

# Install Node.js
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt install -y nodejs

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

2. **Clone and set up the repository**:

```bash
git clone https://github.com/wrfcoin/wrfcoin-core.git
cd wrfcoin-core
npm install
```

3. **Configure PostgreSQL**:

```bash
# Create user and database
sudo -u postgres createuser -P wrfcoin_user
sudo -u postgres createdb -O wrfcoin_user wrfcoin_db

# Set up environment variables
echo "DATABASE_URL=postgres://wrfcoin_user:your_password@localhost:5432/wrfcoin_db" > .env
```

4. **Configure environment**:

Create a `.env.production` file with production settings:

```
# Network Configuration
NETWORK=mainnet
NODE_ENV=production

# Database Configuration
DATABASE_URL=postgres://wrfcoin_user:your_password@localhost:5432/wrfcoin_db

# API Configuration
API_PORT=8545
API_HOST=0.0.0.0

# WebSocket Configuration
WS_PORT=8546
WS_HOST=0.0.0.0

# Frontend Configuration
FRONTEND_PORT=5000
FRONTEND_HOST=0.0.0.0
```

5. **Set up database schema**:

```bash
npm run db:push
```

6. **Build the application**:

```bash
npm run build
```

7. **Set up a service for automatic startup and management**:

Create a systemd service file:

```bash
sudo nano /etc/systemd/system/wrfcoin.service
```

Add the following content:

```
[Unit]
Description=Wrfcoin Core
After=network.target postgresql.service

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/path/to/wrfcoin-core
ExecStart=/usr/bin/npm run start:mainnet
Restart=on-failure
RestartSec=10
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=wrfcoin
Environment=NODE_ENV=production

[Install]
WantedBy=multi-user.target
```

8. **Enable and start the service**:

```bash
sudo systemctl enable wrfcoin
sudo systemctl start wrfcoin
```

9. **Check the service status**:

```bash
sudo systemctl status wrfcoin
```

### Setting Up Reverse Proxy with Nginx

To serve Wrfcoin through a domain with SSL, set up Nginx as a reverse proxy:

1. **Install Nginx**:

```bash
sudo apt install -y nginx
```

2. **Configure Nginx**:

Create a new site configuration:

```bash
sudo nano /etc/nginx/sites-available/wrfcoin
```

Add the following content:

```nginx
server {
    listen 80;
    server_name your-domain.com;

    location / {
        proxy_pass http://localhost:5000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
    }

    location /api {
        proxy_pass http://localhost:8545;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
    }

    location /ws {
        proxy_pass http://localhost:8546;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
    }
}
```

3. **Enable the site and restart Nginx**:

```bash
sudo ln -s /etc/nginx/sites-available/wrfcoin /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl restart nginx
```

4. **Set up SSL with Let's Encrypt**:

```bash
sudo apt install -y certbot python3-certbot-nginx
sudo certbot --nginx -d your-domain.com
```

## Cloud Deployment

### Deploying on AWS

For deploying Wrfcoin Core on AWS:

1. **Launch an EC2 instance**:

   - Amazon Linux 2 or Ubuntu Server 22.04 LTS
   - t3.large or better (2+ vCPUs, 8+ GB RAM)
   - 100+ GB EBS volume

2. **Configure security group**:

   - Allow SSH (port 22)
   - Allow HTTP (port 80)
   - Allow HTTPS (port 443)
   - Allow custom ports for API (8545) and WebSocket (8546) if needed

3. **Follow the server deployment steps** from the previous section.

4. **Configure AWS resources**:
   - Set up an Elastic IP for a static address
   - Configure Route 53 for domain management
   - Set up CloudWatch for monitoring and alerts

### Deploying on Google Cloud Platform

For deploying on GCP:

1. **Create a Compute Engine VM instance**:

   - Ubuntu 22.04 LTS
   - e2-standard-2 or better (2+ vCPUs, 8+ GB RAM)
   - 100+ GB SSD persistent disk

2. **Configure firewall rules**:

   - Allow SSH (port 22)
   - Allow HTTP (port 80)
   - Allow HTTPS (port 443)
   - Allow custom ports for API (8545) and WebSocket (8546) if needed

3. **Follow the server deployment steps** from the previous section.

## Docker Deployment

### Using Docker Compose

For containerized deployment, Wrfcoin Core provides Docker Compose configuration:

1. **Create a docker-compose.yml file**:

```yaml
version: '3.8'

services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_USER: wrfcoin
      POSTGRES_PASSWORD: wrfcoinpassword
      POSTGRES_DB: wrfcoin
    volumes:
      - postgres_data:/var/lib/postgresql/data
    networks:
      - wrfcoin_network

  wrfcoin:
    build:
      context: .
      dockerfile: Dockerfile
    depends_on:
      - postgres
    environment:
      - NODE_ENV=production
      - NETWORK=mainnet
      - DATABASE_URL=postgres://wrfcoin:wrfcoinpassword@postgres:5432/wrfcoin
      - API_PORT=8545
      - API_HOST=0.0.0.0
      - WS_PORT=8546
      - WS_HOST=0.0.0.0
      - FRONTEND_PORT=5000
      - FRONTEND_HOST=0.0.0.0
    ports:
      - '5000:5000'
      - '8545:8545'
      - '8546:8546'
    networks:
      - wrfcoin_network

networks:
  wrfcoin_network:
    driver: bridge

volumes:
  postgres_data:
```

2. **Create a Dockerfile**:

```Dockerfile
FROM rust:1.70 as rust-builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin wrfcoin-core

FROM node:20
WORKDIR /app
COPY . .
COPY --from=rust-builder /app/target/release/wrfcoin-core /app/wrfcoin-core
RUN npm ci
RUN npm run build

EXPOSE 5000 8545 8546
CMD ["npm", "run", "start:mainnet"]
```

3. **Build and start the containers**:

```bash
docker-compose up -d
```

This will start a PostgreSQL database and the Wrfcoin Core application in containers.

## Configuration

### Configuration Files

Wrfcoin Core uses several configuration files:

1. **Environment Variables** (`.env`): Basic settings for the application.
2. **Network Configuration** (`config/mainnet.js` and `config/testnet.js`): Network-specific
   configurations.
3. **Application Configuration** (`config/index.js`): Application-wide settings.

### Network-Specific Configuration

To customize network settings, modify the appropriate configuration file:

For testnet configuration, edit `config/testnet.js`:

```javascript
module.exports = {
  network: 'testnet',
  startingDifficulty: 4,
  blockReward: 50,
  blockTime: 10000, // 10 seconds
  // Other testnet-specific settings
};
```

For mainnet configuration, edit `config/mainnet.js`:

```javascript
module.exports = {
  network: 'mainnet',
  startingDifficulty: 6,
  blockReward: 50,
  blockTime: 60000, // 60 seconds
  // Other mainnet-specific settings
};
```

### Environment Variables

Key environment variables for configuration:

| Variable        | Description                  | Example                             |
| --------------- | ---------------------------- | ----------------------------------- |
| `NODE_ENV`      | Environment mode             | `development`, `production`         |
| `NETWORK`       | Network type                 | `testnet`, `mainnet`                |
| `DATABASE_URL`  | PostgreSQL connection string | `postgres://user:pass@host:5432/db` |
| `API_PORT`      | API server port              | `8545`                              |
| `API_HOST`      | API server host              | `0.0.0.0`                           |
| `WS_PORT`       | WebSocket server port        | `8546`                              |
| `WS_HOST`       | WebSocket server host        | `0.0.0.0`                           |
| `FRONTEND_PORT` | Frontend server port         | `5000`                              |
| `FRONTEND_HOST` | Frontend server host         | `0.0.0.0`                           |
| `LOG_LEVEL`     | Logging verbosity            | `info`, `debug`, `error`            |

## Database Setup

### PostgreSQL Setup and Optimization

For optimal PostgreSQL performance:

1. **Install PostgreSQL**:

```bash
sudo apt install -y postgresql postgresql-contrib
```

2. **Create user and database**:

```bash
sudo -u postgres createuser -P wrfcoin_user
sudo -u postgres createdb -O wrfcoin_user wrfcoin_db
```

3. **Optimize PostgreSQL configuration**:

Edit the PostgreSQL configuration file (typically located at
`/etc/postgresql/15/main/postgresql.conf`):

```conf
# Memory settings
shared_buffers = 2GB                  # 25% of available RAM
work_mem = 128MB                      # Adjust based on system RAM
maintenance_work_mem = 256MB          # For maintenance operations

# Query planner
effective_cache_size = 6GB            # 75% of available RAM
random_page_cost = 1.1                # Assuming SSD storage

# Write Ahead Log
wal_buffers = 16MB                    # Larger for high-write systems
synchronous_commit = off              # For better performance, less durability

# Checkpointing
checkpoint_timeout = 15min            # Less frequent checkpoints
max_wal_size = 2GB                    # Larger WAL size

# Connection settings
max_connections = 100                 # Adjust as needed
```

4. **Set up the database schema**:

```bash
cd wrfcoin-core
npm run db:push
```

5. **Create database indexes**:

The default schema includes indexes for common queries, but additional indexes may be beneficial for
specific deployment scenarios.

6. **Set up regular database backups**:

```bash
# Add to crontab
(crontab -l 2>/dev/null; echo "0 2 * * * pg_dump -U wrfcoin_user wrfcoin_db > /path/to/backups/wrfcoin_$(date +\%Y\%m\%d).sql") | crontab -
```

## Security Considerations

### Node Security

For securing your Wrfcoin node:

1. **Use a firewall**:

```bash
sudo ufw enable
sudo ufw allow ssh
sudo ufw allow http
sudo ufw allow https
sudo ufw allow 8545/tcp  # API
sudo ufw allow 8546/tcp  # WebSocket
```

2. **Set up fail2ban** to protect against brute force attacks:

```bash
sudo apt install -y fail2ban
```

3. **Keep all software updated**:

```bash
sudo apt update
sudo apt upgrade -y
npm audit fix
```

4. **Use secure environment variables**:

Store sensitive information in environment variables, not in code:

```bash
# Example: Setting a sensitive API key
export API_KEY="your-secret-key"
```

5. **Set up SSL certificates** for all exposed services.

### API Security

Protect your API server:

1. **Implement rate limiting** to prevent abuse:

```javascript
const rateLimit = require('express-rate-limit');

const apiLimiter = rateLimit({
  windowMs: 15 * 60 * 1000, // 15 minutes
  max: 100, // 100 requests per window
  message: 'Too many requests from this IP, please try again later.',
});

app.use('/api', apiLimiter);
```

2. **Set up CORS protection**:

```javascript
const cors = require('cors');

const corsOptions = {
  origin: 'https://your-domain.com',
  optionsSuccessStatus: 200,
};

app.use(cors(corsOptions));
```

3. **Use helmet** for HTTP header security:

```javascript
const helmet = require('helmet');
app.use(helmet());
```

## Monitoring and Maintenance

### Monitoring Setup

For monitoring your Wrfcoin deployment:

1. **Set up application logging**:

```javascript
const winston = require('winston');

const logger = winston.createLogger({
  level: process.env.LOG_LEVEL || 'info',
  format: winston.format.combine(winston.format.timestamp(), winston.format.json()),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: 'logs/error.log', level: 'error' }),
    new winston.transports.File({ filename: 'logs/combined.log' }),
  ],
});
```

2. **Set up system monitoring** with Prometheus and Grafana:

Install Prometheus Node Exporter:

```bash
sudo apt install -y prometheus-node-exporter
```

3. **Configure alerts** for critical events:

```yaml
# In prometheus.yml
alerting:
  alertmanagers:
    - static_configs:
        - targets: ['localhost:9093']

rule_files:
  - 'alerts.yml'
```

### Backup and Recovery

Implement regular backups:

1. **Database backups**:

```bash
# Daily backup script
#!/bin/bash
BACKUP_DIR="/path/to/backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
pg_dump -U wrfcoin_user wrfcoin_db > "$BACKUP_DIR/wrfcoin_$TIMESTAMP.sql"

# Keep only the last 30 backups
find "$BACKUP_DIR" -type f -name "wrfcoin_*.sql" -mtime +30 -exec rm {} \;
```

2. **Configuration backups**:

Regularly back up your `.env` files and custom configurations.

3. **Recovery testing**:

Periodically test your backup restoration process:

```bash
# Restore from backup
psql -U wrfcoin_user -d wrfcoin_db_test < "backup_file.sql"
```

### Scaling Considerations

For high-load environments:

1. **Vertical scaling**: Increase CPU, RAM, and disk resources.
2. **Horizontal scaling**: Deploy multiple API nodes behind a load balancer.
3. **Database scaling**: Consider read replicas for database-heavy operations.
4. **Caching layer**: Implement Redis for caching frequent queries.

## Troubleshooting

### Common Deployment Issues

1. **Database connection errors**:

   - Check PostgreSQL is running: `sudo systemctl status postgresql`
   - Verify connection string: `psql "postgres://user:pass@host:5432/db"`
   - Check network connectivity: `telnet host 5432`

2. **API server not starting**:

   - Check logs: `tail -f logs/combined.log`
   - Verify port availability: `netstat -tulpn | grep 8545`
   - Check system resources: `htop`

3. **WebSocket connectivity issues**:
   - Test WebSocket connection: `wscat -c ws://localhost:8546`
   - Check for proxy interference
   - Verify client-side code

### Logging and Debugging

For effective debugging:

1. **Enable debug logging**:

```bash
# In .env file
LOG_LEVEL=debug
```

2. **Check application logs**:

```bash
tail -f logs/combined.log
```

3. **Check system logs**:

```bash
journalctl -u wrfcoin -f
```

4. **Database query logging**:

Enable query logging in PostgreSQL for debugging database issues:

```sql
ALTER SYSTEM SET log_statement = 'all';
SELECT pg_reload_conf();
```

---

For additional help with deployment issues, please join our
[Discord channel](https://discord.gg/wrfcoin) or create an issue on our
[GitHub repository](https://github.com/wrfcoin/wrfcoin-core).
