# Getting Started Troubleshooting Guide

This guide helps resolve common issues encountered when setting up the WRFCoin platform for the
first time.

## Quick Diagnostics

Run these commands to check your system status:

```bash
# Check system requirements
node --version && npm --version && docker --version
git --version && rust --version

# Check WRFCoin services
docker-compose ps
curl -f http://localhost:4001/health || echo "Main API not responding"

# Check database connectivity
npm run db:connect || echo "Database connection failed"
```

## Common Setup Issues

### Issue: `npm install` Fails with Permission Errors

**Symptoms:**

```
EACCES: permission denied, mkdir '/usr/local/lib/node_modules'
```

**Solutions:**

1. **Use Node Version Manager (Recommended)**:

   ```bash
   # Install nvm
   curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
   source ~/.bashrc

   # Install and use Node.js
   nvm install 20
   nvm use 20
   npm install
   ```

2. **Fix npm Permissions**:

   ```bash
   mkdir ~/.npm-global
   npm config set prefix '~/.npm-global'
   echo 'export PATH=~/.npm-global/bin:$PATH' >> ~/.bashrc
   source ~/.bashrc
   ```

3. **Use npx for one-time installs**:
   ```bash
   npx npm install
   ```

**Verification:**

```bash
npm --version && node --version
npm list -g --depth=0
```

### Issue: Docker Services Won't Start

**Symptoms:**

```
ERROR: Couldn't connect to Docker daemon
docker-compose: command not found
```

**Solutions:**

1. **Install Docker Desktop** (macOS/Windows):

   - Download from [docker.com](https://www.docker.com/products/docker-desktop)
   - Start Docker Desktop application
   - Verify: `docker run hello-world`

2. **Install Docker Engine** (Linux):

   ```bash
   # Ubuntu/Debian
   curl -fsSL https://get.docker.com -o get-docker.sh
   sudo sh get-docker.sh
   sudo usermod -aG docker $USER
   newgrp docker

   # Install Docker Compose
   sudo curl -L "https://github.com/docker/compose/releases/latest/download/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
   sudo chmod +x /usr/local/bin/docker-compose
   ```

3. **WSL2 Setup** (Windows):

   ```bash
   # Enable WSL2
   wsl --install
   wsl --set-default-version 2

   # Install Docker Desktop with WSL2 backend
   # Ensure "Use WSL2 based engine" is enabled in settings
   ```

**Verification:**

```bash
docker --version
docker-compose --version
docker run hello-world
```

### Issue: Database Connection Failures

**Symptoms:**

```
ECONNREFUSED 127.0.0.1:5432
Cannot connect to PostgreSQL server
```

**Solutions:**

1. **Start Database Services**:

   ```bash
   docker-compose up -d postgres redis

   # Wait for services to be ready
   docker-compose logs -f postgres
   ```

2. **Check Database Configuration**:

   ```bash
   # Verify environment variables
   echo $DATABASE_URL
   echo $REDIS_URL

   # Test connection
   npm run db:connect
   ```

3. **Reset Database State**:

   ```bash
   # Stop and remove containers
   docker-compose down -v

   # Remove database volumes
   docker volume prune -f

   # Restart fresh
   docker-compose up -d
   npm run db:migrate
   npm run db:seed
   ```

**Verification:**

```bash
# Test database connection
psql $DATABASE_URL -c "SELECT version();"

# Test Redis connection
redis-cli -u $REDIS_URL ping
```

### Issue: Port Already in Use

**Symptoms:**

```
Error: Port 4001 is already in use
EADDRINUSE: address already in use :::4001
```

**Solutions:**

1. **Find and Kill Process**:

   ```bash
   # Find process using port
   lsof -i :4001
   netstat -tulpn | grep :4001

   # Kill process (replace PID)
   kill -9 <PID>

   # Or kill all node processes
   pkill -f node
   ```

2. **Use Different Ports**:

   ```bash
   # Set custom ports in .env
   echo "PORT=4002" >> .env
   echo "API_PORT=4002" >> .env

   # Restart services
   docker-compose down && docker-compose up -d
   ```

3. **Check Docker Port Conflicts**:

   ```bash
   # List Docker containers using ports
   docker ps --format "table {{.Names}}\t{{.Ports}}"

   # Stop conflicting containers
   docker stop $(docker ps -q)
   ```

**Verification:**

```bash
curl http://localhost:4001/health
netstat -tulpn | grep LISTEN
```

### Issue: Rust/Cargo Build Failures

**Symptoms:**

```
error: could not find Cargo.toml
rustc: command not found
```

**Solutions:**

1. **Install Rust Toolchain**:

   ```bash
   # Install rustup
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env

   # Install required components
   rustup component add clippy rustfmt
   rustup target add wasm32-unknown-unknown
   ```

2. **Update Rust Version**:

   ```bash
   rustup update stable
   rustup default stable
   rustc --version
   ```

3. **Clear Build Cache**:
   ```bash
   cd blockchain-core
   cargo clean
   cargo build --release
   ```

**Verification:**

```bash
rustc --version
cargo --version
cd blockchain-core && cargo check
```

## Environment-Specific Issues

### macOS Specific

**Issue: Homebrew Dependencies Missing**

```bash
# Install Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install dependencies
brew install node postgresql redis docker docker-compose
```

**Issue: Xcode Command Line Tools**

```bash
xcode-select --install
```

### Windows/WSL2 Specific

**Issue: WSL2 File System Performance**

```bash
# Move project to WSL2 file system
cd /mnt/c/Users/$USER
cp -r /mnt/c/wrfcoin ~/wrfcoin
cd ~/wrfcoin
```

**Issue: Windows PATH Issues**

```bash
# Add to ~/.bashrc
echo 'export PATH="/usr/local/bin:/usr/bin:/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

### Linux Specific

**Issue: Package Dependencies**

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev postgresql-client redis-tools

# CentOS/RHEL
sudo yum install -y gcc gcc-c++ openssl-devel postgresql redis
```

## Validation Checklist

After resolving issues, verify your setup:

- [ ] Node.js 20+ and npm 9+ installed
- [ ] Docker and Docker Compose working
- [ ] Rust toolchain 1.70+ installed
- [ ] PostgreSQL database accessible
- [ ] Redis cache accessible
- [ ] All services starting successfully
- [ ] API health check returning 200
- [ ] No port conflicts
- [ ] Environment variables set

## Next Steps

Once setup is complete:

1. **Run Tests**: `npm test`
2. **Build Project**: `npm run build`
3. **Start Development**: `npm run dev`
4. **Read Documentation**: [Getting Started Guide](../getting-started.md)

## Still Having Issues?

1. **Check Service Logs**:

   ```bash
   docker-compose logs -f
   npm run logs
   ```

2. **Run Diagnostics**:

   ```bash
   npm run health-check
   node scripts/system-health-fix.js
   ```

3. **Get Help**:
   - [Discord #troubleshooting](https://discord.gg/wrfcoin-troubleshooting)
   - [GitHub Issues](https://github.com/wrfcoin/platform/issues/new?template=setup_issue.md)
   - [System Recovery Guide](./emergency/system-recovery.md)

---

**Last Updated**: January 2025 **Difficulty**: Beginner **Estimated Time**: 30-60 minutes
