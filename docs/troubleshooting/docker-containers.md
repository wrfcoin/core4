# Docker & Container Troubleshooting Guide

This guide addresses common Docker and container-related issues in the WRFCoin platform.

## Quick Diagnostics

```bash
# Check Docker status
docker --version && docker-compose --version
docker system info | grep -E "Server Version|Storage Driver|Cgroup"

# Check container status
docker-compose ps
docker stats --no-stream

# Check container logs
docker-compose logs --tail=100
```

## Container Startup Issues

### Issue: Containers Won't Start

**Symptoms:**

```
ERROR: No such service: postgres
Exited (125) - Container failed to start
```

**Solutions:**

1. **Verify Docker Compose File**:

   ```bash
   # Validate compose file syntax
   docker-compose config

   # Check if compose file exists
   ls -la docker-compose*.yml

   # Use correct compose file
   docker-compose -f docker-compose.yml up -d
   ```

2. **Check Image Availability**:

   ```bash
   # Pull required images
   docker-compose pull

   # Build custom images
   docker-compose build --no-cache

   # List available images
   docker images | grep wrfcoin
   ```

3. **Clear Container State**:

   ```bash
   # Stop all containers
   docker-compose down -v

   # Remove orphaned containers
   docker container prune -f

   # Remove unused networks
   docker network prune -f

   # Start fresh
   docker-compose up -d
   ```

**Verification:**

```bash
docker-compose ps
docker-compose logs --tail=10
```

### Issue: Service Dependencies Not Ready

**Symptoms:**

```
Connection refused to postgres:5432
Redis connection timeout
API service keeps restarting
```

**Solutions:**

1. **Check Service Order**:

   ```bash
   # Start core services first
   docker-compose up -d postgres redis

   # Wait for readiness
   docker-compose logs -f postgres | grep "ready to accept"

   # Start dependent services
   docker-compose up -d api
   ```

2. **Use Health Checks**:

   ```bash
   # Check service health
   docker-compose ps | grep "healthy\|unhealthy"

   # Wait for services
   docker-compose exec postgres pg_isready
   docker-compose exec redis redis-cli ping
   ```

3. **Increase Startup Timeouts**:
   ```yaml
   # In docker-compose.yml
   services:
     api:
       depends_on:
         postgres:
           condition: service_healthy
       restart: unless-stopped
   ```

**Verification:**

```bash
# Test service connectivity
curl http://localhost:4001/health
docker-compose exec api npm run health-check
```

## Container Resource Issues

### Issue: Out of Memory Errors

**Symptoms:**

```
Container killed due to memory limit
OOMKilled
npm ERR! errno -12 ENOMEM
```

**Solutions:**

1. **Increase Memory Limits**:

   ```yaml
   # In docker-compose.yml
   services:
     api:
       mem_limit: 2g
       memswap_limit: 2g
   ```

2. **Check System Resources**:

   ```bash
   # Check available memory
   free -h
   docker system df

   # Monitor container usage
   docker stats
   ```

3. **Optimize Container Usage**:

   ```bash
   # Clean up unused resources
   docker system prune -a
   docker volume prune

   # Remove old images
   docker image prune -a
   ```

**Verification:**

```bash
docker stats --no-stream
docker system df
```

### Issue: Disk Space Problems

**Symptoms:**

```
No space left on device
docker: Error response from daemon: thin pool has run out of space
```

**Solutions:**

1. **Clean Docker Resources**:

   ```bash
   # Clean everything (CAUTION: removes volumes)
   docker system prune -a --volumes

   # Selective cleanup
   docker image prune -a
   docker container prune
   docker network prune

   # Remove specific volumes
   docker volume ls | grep wrfcoin
   docker volume rm <volume_name>
   ```

2. **Check Disk Usage**:

   ```bash
   # Check disk space
   df -h

   # Find large files
   du -h --max-depth=1 /var/lib/docker

   # Check Docker space usage
   docker system df -v
   ```

3. **Configure Docker Storage**:

   ```bash
   # Change Docker root directory
   sudo systemctl stop docker
   sudo mkdir /new/docker/location
   sudo rsync -a /var/lib/docker/ /new/docker/location/

   # Update daemon.json
   echo '{"data-root": "/new/docker/location"}' | sudo tee /etc/docker/daemon.json
   sudo systemctl start docker
   ```

**Verification:**

```bash
df -h
docker system df
docker info | grep "Docker Root"
```

## Container Network Issues

### Issue: Service Discovery Problems

**Symptoms:**

```
Could not resolve hostname 'postgres'
Connection refused to api:4001
Network not found
```

**Solutions:**

1. **Check Network Configuration**:

   ```bash
   # List Docker networks
   docker network ls

   # Inspect project network
   docker network inspect wrfcoin_default

   # Check container networking
   docker-compose exec api nslookup postgres
   docker-compose exec api ping redis
   ```

2. **Recreate Network**:

   ```bash
   # Remove containers and network
   docker-compose down
   docker network rm wrfcoin_default

   # Recreate with fresh network
   docker-compose up -d
   ```

3. **Use External Network**:

   ```yaml
   # In docker-compose.yml
   networks:
     wrfcoin:
       external: true

   services:
     api:
       networks:
         - wrfcoin
   ```

**Verification:**

```bash
docker network ls
docker-compose exec api ping postgres
```

### Issue: Port Binding Failures

**Symptoms:**

```
Port 4001 is already allocated
bind: address already in use
```

**Solutions:**

1. **Find Conflicting Process**:

   ```bash
   # Check port usage
   netstat -tulpn | grep :4001
   lsof -i :4001

   # Kill conflicting process
   sudo kill -9 <PID>

   # Or stop Docker containers
   docker ps | grep 4001
   docker stop <container_id>
   ```

2. **Change Port Mapping**:

   ```yaml
   # In docker-compose.yml
   services:
     api:
       ports:
         - '4002:4001' # host:container
   ```

3. **Use Host Network Mode**:
   ```yaml
   # In docker-compose.yml (Linux only)
   services:
     api:
       network_mode: host
   ```

**Verification:**

```bash
docker-compose ps
netstat -tulpn | grep LISTEN
curl http://localhost:4001/health
```

## Container Performance Issues

### Issue: Slow Container Performance

**Solutions:**

1. **Check Resource Limits**:

   ```bash
   # Monitor container stats
   docker stats

   # Check container limits
   docker inspect <container> | grep -A 20 "HostConfig"
   ```

2. **Optimize Docker Settings**:

   ```bash
   # Increase CPU limits
   docker-compose up -d --scale api=2

   # Use performance mount options
   # Add to docker-compose.yml volumes:
   # - ./:/app:delegated  # macOS
   # - ./:/app:cached     # Better for read-heavy
   ```

3. **Use BuildKit for Faster Builds**:
   ```bash
   export DOCKER_BUILDKIT=1
   export COMPOSE_DOCKER_CLI_BUILD=1
   docker-compose build --parallel
   ```

**Verification:**

```bash
docker stats --no-stream
time docker-compose build
```

## Container Data Issues

### Issue: Data Not Persisting

**Symptoms:**

```
Database data lost after restart
Redis cache empty
Volume mount not working
```

**Solutions:**

1. **Check Volume Configuration**:

   ```bash
   # List volumes
   docker volume ls | grep wrfcoin

   # Inspect volume
   docker volume inspect wrfcoin_postgres_data

   # Check mount points
   docker-compose exec postgres mount | grep /var/lib/postgresql
   ```

2. **Fix Volume Permissions**:

   ```bash
   # Check permissions
   docker-compose exec postgres ls -la /var/lib/postgresql/data

   # Fix ownership
   docker-compose exec postgres chown -R postgres:postgres /var/lib/postgresql/data
   ```

3. **Backup and Restore Data**:

   ```bash
   # Backup database
   docker-compose exec postgres pg_dump -U postgres wrfcoin > backup.sql

   # Restore database
   docker-compose exec -T postgres psql -U postgres wrfcoin < backup.sql
   ```

**Verification:**

```bash
docker volume ls
docker-compose exec postgres psql -U postgres -c "\l"
```

## Multi-Service Issues

### Issue: Microservices Communication Failures

**Solutions:**

1. **Check Service Registry**:

   ```bash
   # Verify all services are up
   docker-compose ps

   # Test inter-service communication
   docker-compose exec api curl http://cross-chain:3000/health
   docker-compose exec api curl http://compute:6000/health
   ```

2. **Check Service Discovery**:

   ```bash
   # Test DNS resolution
   docker-compose exec api nslookup cross-chain
   docker-compose exec api nslookup compute

   # Check network connectivity
   docker-compose exec api telnet postgres 5432
   ```

3. **Review Service Dependencies**:
   ```yaml
   # In docker-compose.yml
   services:
     api:
       depends_on:
         - postgres
         - redis
         - cross-chain
   ```

## Monitoring and Debugging

### Container Logs Analysis

```bash
# View all service logs
docker-compose logs -f

# View specific service logs
docker-compose logs -f postgres
docker-compose logs -f api --tail=100

# Search logs for errors
docker-compose logs | grep -i error
docker-compose logs | grep -i "connection refused"

# Export logs for analysis
docker-compose logs > wrfcoin-container-logs.txt
```

### Health Check Commands

```bash
# Run comprehensive health check
node scripts/comprehensive-port-audit.js

# Check individual services
curl http://localhost:4001/health      # Main API
curl http://localhost:3000/health      # Cross-chain
curl http://localhost:6000/health      # Compute
curl http://localhost:8000/health      # Governance

# Database connectivity
npm run db:connect
redis-cli ping
```

### Container Debugging

```bash
# Enter container shell
docker-compose exec api bash
docker-compose exec postgres psql -U postgres

# Debug container startup
docker-compose up --no-deps api
docker logs <container_id> -f

# Check container configuration
docker inspect <container_id>
docker-compose config
```

## Prevention and Best Practices

### Container Maintenance

```bash
# Regular cleanup (weekly)
docker system prune
docker volume prune

# Update images (monthly)
docker-compose pull
docker-compose build --no-cache

# Health monitoring
docker stats --no-stream
docker system df
```

### Development Best Practices

1. **Use .dockerignore**:

   ```
   node_modules
   .git
   *.log
   coverage/
   ```

2. **Multi-stage builds** for production
3. **Resource limits** in compose files
4. **Health checks** for all services
5. **Proper signal handling** in applications

---

**Last Updated**: January 2025 **Difficulty**: Intermediate **Estimated Resolution Time**: 15-45
minutes
