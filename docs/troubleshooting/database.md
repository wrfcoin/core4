# Database Troubleshooting Guide

This guide covers PostgreSQL and Redis database issues commonly encountered in the WRFCoin platform.

## Quick Diagnostics

```bash
# Check database services
docker-compose ps | grep -E "postgres|redis"

# Test connections
npm run db:connect
redis-cli ping

# Check database status
docker-compose exec postgres pg_isready -U postgres
docker-compose exec redis redis-cli info server
```

## PostgreSQL Issues

### Issue: Cannot Connect to PostgreSQL

**Symptoms:**

```
ECONNREFUSED 127.0.0.1:5432
Connection to database failed
psql: could not connect to server
```

**Solutions:**

1. **Verify Service Status**:

   ```bash
   # Check if PostgreSQL is running
   docker-compose ps postgres

   # Check service logs
   docker-compose logs postgres

   # Restart PostgreSQL
   docker-compose restart postgres
   ```

2. **Check Connection Configuration**:

   ```bash
   # Verify environment variables
   echo $DATABASE_URL
   grep DATABASE .env

   # Test connection string
   psql "$DATABASE_URL" -c "SELECT version();"
   ```

3. **Verify Network Connectivity**:

   ```bash
   # Test port accessibility
   telnet localhost 5432
   nc -zv localhost 5432

   # Check Docker network
   docker-compose exec api ping postgres
   ```

**Verification:**

```bash
npm run db:connect
psql $DATABASE_URL -c "SELECT current_database();"
```

### Issue: Database Migrations Failing

**Symptoms:**

```
Migration failed: relation already exists
Cannot run migration: database is locked
Timeout during migration
```

**Solutions:**

1. **Check Migration Status**:

   ```bash
   # View migration history
   npm run db:migration:status

   # Check migration table
   psql $DATABASE_URL -c "SELECT * FROM drizzle.__drizzle_migrations ORDER BY id;"
   ```

2. **Reset Migration State**:

   ```bash
   # Rollback last migration
   npm run db:migration:down

   # Reset all migrations (CAUTION)
   npm run db:reset
   npm run db:migrate
   ```

3. **Manual Migration Fix**:

   ```bash
   # Connect to database
   psql $DATABASE_URL

   # Check conflicting objects
   \dt  -- List tables
   \d table_name  -- Describe table structure

   # Drop problematic objects if safe
   DROP TABLE IF EXISTS problematic_table CASCADE;
   ```

**Verification:**

```bash
npm run db:migrate
npm run db:seed
```

### Issue: Database Performance Problems

**Symptoms:**

```
Slow query performance
Connection timeouts
High CPU usage
```

**Solutions:**

1. **Analyze Query Performance**:

   ```sql
   -- Enable query logging
   ALTER SYSTEM SET log_statement = 'all';
   SELECT pg_reload_conf();

   -- Find slow queries
   SELECT query, mean_exec_time, calls
   FROM pg_stat_statements
   ORDER BY mean_exec_time DESC;

   -- Check table sizes
   SELECT schemaname, tablename,
          pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) as size
   FROM pg_tables
   ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;
   ```

2. **Optimize Database Configuration**:

   ```sql
   -- Increase connection limits
   ALTER SYSTEM SET max_connections = 200;

   -- Optimize memory settings
   ALTER SYSTEM SET shared_buffers = '256MB';
   ALTER SYSTEM SET effective_cache_size = '1GB';

   -- Reload configuration
   SELECT pg_reload_conf();
   ```

3. **Maintenance Operations**:

   ```bash
   # Run database maintenance
   docker-compose exec postgres psql -U postgres -d wrfcoin -c "VACUUM ANALYZE;"

   # Update table statistics
   docker-compose exec postgres psql -U postgres -d wrfcoin -c "ANALYZE;"

   # Reindex tables
   docker-compose exec postgres psql -U postgres -d wrfcoin -c "REINDEX DATABASE wrfcoin;"
   ```

**Verification:**

```bash
# Check performance metrics
docker-compose exec postgres psql -U postgres -c "SELECT * FROM pg_stat_activity;"
```

### Issue: Database Corruption or Data Loss

**Symptoms:**

```
Corrupted database blocks
Data inconsistencies
Tables missing after restart
```

**Solutions:**

1. **Check Database Integrity**:

   ```sql
   -- Check for corruption
   SELECT pg_database_size('wrfcoin');

   -- Verify table integrity
   \dt+  -- List tables with sizes

   -- Check for orphaned data
   SELECT schemaname, tablename, n_dead_tup, n_live_tup
   FROM pg_stat_user_tables;
   ```

2. **Restore from Backup**:

   ```bash
   # List available backups
   ls -la backups/database/

   # Stop application services
   docker-compose stop api cross-chain compute

   # Restore database
   docker-compose exec postgres dropdb -U postgres wrfcoin
   docker-compose exec postgres createdb -U postgres wrfcoin
   docker-compose exec -T postgres psql -U postgres wrfcoin < backups/database/latest.sql
   ```

3. **Emergency Recovery**:

   ```bash
   # Create emergency backup
   npm run db:backup:emergency

   # Reset to known good state
   docker-compose down -v
   docker volume rm wrfcoin_postgres_data
   docker-compose up -d postgres

   # Restore from backup
   npm run db:restore:latest
   ```

**Verification:**

```bash
npm run db:validate
psql $DATABASE_URL -c "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public';"
```

## Redis Issues

### Issue: Redis Connection Problems

**Symptoms:**

```
Redis connection timeout
ECONNREFUSED 127.0.0.1:6379
Redis server not responding
```

**Solutions:**

1. **Check Redis Service**:

   ```bash
   # Verify Redis is running
   docker-compose ps redis

   # Check Redis logs
   docker-compose logs redis

   # Restart Redis
   docker-compose restart redis
   ```

2. **Test Redis Connectivity**:

   ```bash
   # Test local connection
   redis-cli ping

   # Test connection string
   redis-cli -u $REDIS_URL ping

   # Test from application container
   docker-compose exec api redis-cli -h redis ping
   ```

3. **Check Redis Configuration**:

   ```bash
   # View Redis config
   redis-cli CONFIG GET "*"

   # Check memory usage
   redis-cli INFO memory

   # Check client connections
   redis-cli INFO clients
   ```

**Verification:**

```bash
redis-cli ping
redis-cli SET test "Hello WRFCoin" && redis-cli GET test
```

### Issue: Redis Memory Problems

**Symptoms:**

```
Redis out of memory
OOM command not allowed
Performance degradation
```

**Solutions:**

1. **Check Memory Usage**:

   ```bash
   # Redis memory info
   redis-cli INFO memory

   # Check key distribution
   redis-cli --bigkeys

   # Memory usage by key pattern
   redis-cli MEMORY USAGE user:*
   ```

2. **Configure Memory Limits**:

   ```bash
   # Set memory limit
   redis-cli CONFIG SET maxmemory 512mb

   # Set eviction policy — pick the correct one for this instance:
   #
   #   volatile-lru  (DEFAULT for any instance that holds security-critical
   #                  state such as JWT/session revocation keys, rate-limit
   #                  counters that gate authn, or idempotency tokens).
   #                  Only keys with a TTL are eligible for eviction — keys
   #                  without a TTL (or with a long TTL that has not yet
   #                  elapsed) are preserved over short-lived cache entries.
   #
   #   noeviction    (for instances where eviction is never acceptable;
   #                  writes will fail with OOM instead of silently dropping
   #                  keys. Use when you would rather reject traffic than
   #                  lose any state.)
   #
   #   allkeys-lru   (ONLY for pure caches where no single key is
   #                  security-critical and every key can be re-derived.
   #                  Do NOT use on any instance shared with revocation,
   #                  session, or rate-limit state — an evicted revocation
   #                  key silently re-validates a revoked token.)
   redis-cli CONFIG SET maxmemory-policy volatile-lru

   # Save configuration
   redis-cli CONFIG REWRITE
   ```

   See [`docs/security/REDIS_EVICTION_POLICY.md`](../security/REDIS_EVICTION_POLICY.md)
   for the full invariant, rationale, and the list of current core4 Redis
   configurations. Tracked in wrfcoin/core4#1203.

3. **Clean Up Redis Data**:

   ```bash
   # Remove expired keys
   redis-cli EVAL "return redis.call('del', unpack(redis.call('keys', ARGV[1])))" 0 "expired:*"

   # Clear specific patterns
   redis-cli EVAL "return redis.call('del', unpack(redis.call('keys', ARGV[1])))" 0 "cache:*"

   # Flush all data (CAUTION)
   redis-cli FLUSHDB
   ```

**Verification:**

```bash
redis-cli INFO memory | grep used_memory_human
redis-cli DBSIZE
```

### Issue: Redis Performance Issues

**Solutions:**

1. **Monitor Redis Performance**:

   ```bash
   # Monitor commands in real-time
   redis-cli MONITOR

   # Check slow queries
   redis-cli SLOWLOG GET 10

   # Reset slow log
   redis-cli SLOWLOG RESET
   ```

2. **Optimize Redis Settings**:

   ```bash
   # Disable disk persistence for cache-only usage
   redis-cli CONFIG SET save ""

   # Optimize for memory
   redis-cli CONFIG SET hash-max-ziplist-entries 512
   redis-cli CONFIG SET hash-max-ziplist-value 64
   ```

3. **Connection Pool Optimization**:
   ```javascript
   // In application configuration
   const redisConfig = {
     host: 'redis',
     port: 6379,
     maxRetriesPerRequest: 3,
     retryDelayOnFailover: 100,
     lazyConnect: true,
     keepAlive: 30000,
     family: 4,
     db: 0,
   };
   ```

## Database Administration

### Backup and Recovery

```bash
# Automated backup script
#!/bin/bash
DATE=$(date +%Y%m%d_%H%M%S)

# PostgreSQL backup
docker-compose exec postgres pg_dump -U postgres wrfcoin > "backups/postgres_${DATE}.sql"

# Redis backup
docker-compose exec redis redis-cli BGSAVE
docker cp $(docker-compose ps -q redis):/data/dump.rdb "backups/redis_${DATE}.rdb"

# Compress backups
gzip "backups/postgres_${DATE}.sql"
gzip "backups/redis_${DATE}.rdb"
```

### Monitoring Commands

```bash
# Database monitoring script
#!/bin/bash

echo "=== PostgreSQL Status ==="
docker-compose exec postgres pg_isready -U postgres
docker-compose exec postgres psql -U postgres -c "SELECT version();"

echo "=== Redis Status ==="
docker-compose exec redis redis-cli ping
docker-compose exec redis redis-cli INFO server | grep redis_version

echo "=== Connection Counts ==="
docker-compose exec postgres psql -U postgres -c "SELECT count(*) FROM pg_stat_activity;"
docker-compose exec redis redis-cli INFO clients | grep connected_clients

echo "=== Database Sizes ==="
docker-compose exec postgres psql -U postgres -c "SELECT pg_database_size('wrfcoin') as size;"
docker-compose exec redis redis-cli DBSIZE
```

### Performance Tuning

```bash
# PostgreSQL performance tuning
cat > database-tuning.sql << EOF
-- Connection settings
ALTER SYSTEM SET max_connections = 200;
ALTER SYSTEM SET shared_buffers = '256MB';
ALTER SYSTEM SET effective_cache_size = '1GB';
ALTER SYSTEM SET maintenance_work_mem = '64MB';
ALTER SYSTEM SET checkpoint_completion_target = 0.9;
ALTER SYSTEM SET wal_buffers = '16MB';
ALTER SYSTEM SET default_statistics_target = 100;

-- Reload configuration
SELECT pg_reload_conf();
EOF

docker-compose exec -T postgres psql -U postgres < database-tuning.sql
```

### Security Configuration

```bash
# Secure database configuration
cat > database-security.sql << EOF
-- Create application user
CREATE USER wrfcoin_app WITH ENCRYPTED PASSWORD 'secure_password';
GRANT CONNECT ON DATABASE wrfcoin TO wrfcoin_app;
GRANT USAGE ON SCHEMA public TO wrfcoin_app;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO wrfcoin_app;
GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO wrfcoin_app;

-- Set row-level security
ALTER TABLE users ENABLE ROW LEVEL SECURITY;
EOF

docker-compose exec -T postgres psql -U postgres -d wrfcoin < database-security.sql
```

## Emergency Procedures

### Database Recovery Checklist

1. **Stop Application Services**:

   ```bash
   docker-compose stop api cross-chain compute governance
   ```

2. **Assess Damage**:

   ```bash
   npm run db:validate
   redis-cli ping
   ```

3. **Restore from Backup**:

   ```bash
   npm run db:restore:latest
   ```

4. **Verify Data Integrity**:

   ```bash
   npm run db:test
   npm run db:seed:check
   ```

5. **Restart Services**:
   ```bash
   docker-compose up -d
   ```

### Contact Information

- **Critical Issues**: [Emergency Support](mailto:support@wrfcoin.com)
- **Community Help**: [Discord #database](https://discord.gg/wrfcoin-database)
- **Bug Reports**:
  [GitHub Issues](https://github.com/wrfcoin/platform/issues/new?template=database_issue.md)

---

**Last Updated**: January 2025 **Difficulty**: Intermediate to Advanced **Estimated Resolution
Time**: 15-60 minutes
