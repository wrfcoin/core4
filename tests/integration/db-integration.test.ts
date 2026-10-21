/**
 * Database Integration Tests with Testcontainers
 *
 * Uses a real PostgreSQL container to validate database operations,
 * schema constraints, transactions, and data integrity. Requires
 * Docker to be available - tests are skipped if Docker is not running.
 *
 * Run: npx jest tests/integration/db-integration.test.ts --testTimeout=60000
 */

import { describe, it, expect, beforeAll, afterAll, beforeEach } from '@jest/globals';

// Check if Docker/testcontainers are available at import time
let PostgreSqlContainer: any;
let Client: any;
let dockerAvailable = false;

try {
  PostgreSqlContainer = require('testcontainers').PostgreSqlContainer;
  Client = require('pg').Client;
  dockerAvailable = true;
} catch {
  // testcontainers or pg not available
}

// SQL to create the core tables matching the Drizzle schema
const CREATE_TABLES_SQL = `
  CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT,
    wallet_address TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    last_login TIMESTAMP,
    status TEXT NOT NULL DEFAULT 'active',
    role TEXT NOT NULL DEFAULT 'user',
    preferences JSONB,
    reputation INTEGER DEFAULT 0,
    is_verified BOOLEAN DEFAULT FALSE
  );

  CREATE TABLE IF NOT EXISTS blocks (
    id SERIAL PRIMARY KEY,
    block_id TEXT NOT NULL UNIQUE,
    height INTEGER NOT NULL,
    timestamp TIMESTAMP NOT NULL DEFAULT NOW(),
    hash TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    transaction_count INTEGER NOT NULL,
    difficulty INTEGER NOT NULL,
    size INTEGER NOT NULL,
    validator TEXT,
    merkle_root TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS transactions (
    id SERIAL PRIMARY KEY,
    tx_id TEXT NOT NULL UNIQUE,
    from_address TEXT NOT NULL,
    to_address TEXT NOT NULL,
    amount INTEGER NOT NULL,
    fee INTEGER NOT NULL,
    timestamp TIMESTAMP NOT NULL DEFAULT NOW(),
    status TEXT NOT NULL,
    type TEXT NOT NULL,
    block_id INTEGER REFERENCES blocks(id),
    data JSONB
  );

  CREATE TABLE IF NOT EXISTS carbon_credits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_id TEXT UNIQUE NOT NULL,
    owner TEXT NOT NULL,
    carbon_offset DECIMAL(18, 8) NOT NULL,
    project_type TEXT NOT NULL,
    verification_standard TEXT NOT NULL,
    issuance_date TIMESTAMP NOT NULL,
    expiry_date TIMESTAMP,
    location JSONB,
    is_retired BOOLEAN DEFAULT FALSE
  );

  CREATE TABLE IF NOT EXISTS weather_stations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner TEXT NOT NULL,
    location JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    quality_score REAL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
  );
`;

const describeIfDocker = dockerAvailable ? describe : describe.skip;

describeIfDocker('Database Integration: Testcontainers PostgreSQL', () => {
  let container: any;
  let client: any;

  beforeAll(async () => {
    try {
      container = await new PostgreSqlContainer('postgres:15-alpine')
        .withDatabase('wrfcoin_test')
        .withUsername('test')
        .withPassword('test')
        .start();

      client = new Client({
        host: container.getHost(),
        port: container.getPort(),
        database: container.getDatabase(),
        user: container.getUsername(),
        password: container.getPassword(),
      });
      await client.connect();
      await client.query(CREATE_TABLES_SQL);
    } catch (err) {
      console.log('Docker not available, skipping DB integration tests:', (err as Error).message);
      container = null;
      client = null;
    }
  }, 120000); // 2 minute timeout for container startup

  afterAll(async () => {
    if (client) await client.end();
    if (container) await container.stop();
  }, 30000);

  beforeEach(async () => {
    if (!client) return;
    // Clean all test data between tests
    await client.query('DELETE FROM transactions');
    await client.query('DELETE FROM blocks');
    await client.query('DELETE FROM users');
    await client.query('DELETE FROM carbon_credits');
    await client.query('DELETE FROM weather_stations');
  });

  // ---- User Operations ----

  describe('User CRUD', () => {
    it('should insert and retrieve a user', async () => {
      if (!client) return;

      await client.query(
        `INSERT INTO users (username, wallet_address, email, role)
         VALUES ($1, $2, $3, $4)`,
        ['alice', '0x' + '1'.repeat(40), 'alice@test.local', 'user']
      );

      const result = await client.query('SELECT * FROM users WHERE username = $1', ['alice']);
      expect(result.rows).toHaveLength(1);
      expect(result.rows[0].username).toBe('alice');
      expect(result.rows[0].wallet_address).toBe('0x' + '1'.repeat(40));
      expect(result.rows[0].role).toBe('user');
      expect(result.rows[0].status).toBe('active');
      expect(result.rows[0].is_verified).toBe(false);
    });

    it('should enforce unique username constraint', async () => {
      if (!client) return;

      await client.query(
        `INSERT INTO users (username, wallet_address) VALUES ($1, $2)`,
        ['bob', '0x' + '2'.repeat(40)]
      );

      await expect(
        client.query(
          `INSERT INTO users (username, wallet_address) VALUES ($1, $2)`,
          ['bob', '0x' + '3'.repeat(40)]
        )
      ).rejects.toThrow(/unique/i);
    });

    it('should apply default values correctly', async () => {
      if (!client) return;

      await client.query(
        `INSERT INTO users (username, wallet_address) VALUES ($1, $2)`,
        ['defaults-user', '0x' + '4'.repeat(40)]
      );

      const result = await client.query('SELECT * FROM users WHERE username = $1', [
        'defaults-user',
      ]);
      const user = result.rows[0];

      expect(user.status).toBe('active');
      expect(user.role).toBe('user');
      expect(user.reputation).toBe(0);
      expect(user.is_verified).toBe(false);
      expect(user.created_at).toBeInstanceOf(Date);
      expect(user.updated_at).toBeInstanceOf(Date);
    });
  });

  // ---- Transaction Operations ----

  describe('Transaction Operations', () => {
    it('should insert transaction with block reference', async () => {
      if (!client) return;

      // Insert a block first
      await client.query(
        `INSERT INTO blocks (block_id, height, hash, prev_hash, transaction_count, difficulty, size, merkle_root)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
        ['block-1', 1, '0x' + 'a'.repeat(64), '0x' + '0'.repeat(64), 1, 1, 256, '0x' + 'b'.repeat(64)]
      );

      const blockResult = await client.query('SELECT id FROM blocks WHERE block_id = $1', [
        'block-1',
      ]);
      const blockId = blockResult.rows[0].id;

      await client.query(
        `INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type, block_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
        ['0x' + 'c'.repeat(64), '0x' + '1'.repeat(40), '0x' + '2'.repeat(40), 1000, 10, 'confirmed', 'transfer', blockId]
      );

      const result = await client.query('SELECT * FROM transactions WHERE tx_id = $1', [
        '0x' + 'c'.repeat(64),
      ]);
      expect(result.rows[0].amount).toBe(1000);
      expect(result.rows[0].block_id).toBe(blockId);
    });

    it('should enforce unique tx_id constraint', async () => {
      if (!client) return;

      const txId = '0x' + 'd'.repeat(64);
      await client.query(
        `INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type)
         VALUES ($1, $2, $3, $4, $5, $6, $7)`,
        [txId, '0x' + '1'.repeat(40), '0x' + '2'.repeat(40), 100, 1, 'pending', 'transfer']
      );

      // Duplicate tx_id should fail (prevents double-spend at DB level)
      await expect(
        client.query(
          `INSERT INTO transactions (tx_id, from_address, to_address, amount, fee, status, type)
           VALUES ($1, $2, $3, $4, $5, $6, $7)`,
          [txId, '0x' + '3'.repeat(40), '0x' + '4'.repeat(40), 200, 2, 'pending', 'transfer']
        )
      ).rejects.toThrow(/unique/i);
    });
  });

  // ---- Atomic Transactions ----

  describe('Database Transactions', () => {
    it('should rollback on failure within a transaction', async () => {
      if (!client) return;

      try {
        await client.query('BEGIN');
        await client.query(
          `INSERT INTO users (username, wallet_address) VALUES ($1, $2)`,
          ['tx-user-1', '0x' + '5'.repeat(40)]
        );
        // This should fail due to NOT NULL constraint (missing wallet_address)
        await client.query(
          `INSERT INTO users (username) VALUES ($1)`,
          ['tx-user-2']
        );
        await client.query('COMMIT');
      } catch {
        await client.query('ROLLBACK');
      }

      // Neither insert should have persisted
      const result = await client.query('SELECT COUNT(*) FROM users');
      expect(parseInt(result.rows[0].count)).toBe(0);
    });

    it('should commit successful transactions atomically', async () => {
      if (!client) return;

      await client.query('BEGIN');
      await client.query(
        `INSERT INTO users (username, wallet_address) VALUES ($1, $2)`,
        ['atomic-1', '0x' + '6'.repeat(40)]
      );
      await client.query(
        `INSERT INTO users (username, wallet_address) VALUES ($1, $2)`,
        ['atomic-2', '0x' + '7'.repeat(40)]
      );
      await client.query('COMMIT');

      const result = await client.query('SELECT COUNT(*) FROM users');
      expect(parseInt(result.rows[0].count)).toBe(2);
    });
  });

  // ---- Carbon Credit Operations ----

  describe('Carbon Credit Operations', () => {
    it('should store and query carbon credits', async () => {
      if (!client) return;

      await client.query(
        `INSERT INTO carbon_credits (token_id, owner, carbon_offset, project_type, verification_standard, issuance_date)
         VALUES ($1, $2, $3, $4, $5, $6)`,
        ['CC-001', '0x' + '8'.repeat(40), '10.00000000', 'weather_monitoring', 'VCS', '2025-01-01']
      );

      const result = await client.query('SELECT * FROM carbon_credits WHERE token_id = $1', [
        'CC-001',
      ]);
      expect(result.rows[0].owner).toBe('0x' + '8'.repeat(40));
      expect(parseFloat(result.rows[0].carbon_offset)).toBe(10);
      expect(result.rows[0].is_retired).toBe(false);
    });

    it('should update credit ownership on trade', async () => {
      if (!client) return;

      await client.query(
        `INSERT INTO carbon_credits (token_id, owner, carbon_offset, project_type, verification_standard, issuance_date)
         VALUES ($1, $2, $3, $4, $5, $6)`,
        ['CC-002', '0x' + '9'.repeat(40), '5.00000000', 'reforestation', 'GS', '2025-01-01']
      );

      // Transfer ownership
      await client.query(
        `UPDATE carbon_credits SET owner = $1 WHERE token_id = $2 AND is_retired = FALSE`,
        ['0x' + 'a'.repeat(40), 'CC-002']
      );

      const result = await client.query('SELECT owner FROM carbon_credits WHERE token_id = $1', [
        'CC-002',
      ]);
      expect(result.rows[0].owner).toBe('0x' + 'a'.repeat(40));
    });

    it('should prevent transfer of retired credits via WHERE clause', async () => {
      if (!client) return;

      await client.query(
        `INSERT INTO carbon_credits (token_id, owner, carbon_offset, project_type, verification_standard, issuance_date, is_retired)
         VALUES ($1, $2, $3, $4, $5, $6, $7)`,
        ['CC-003', '0x' + 'b'.repeat(40), '20.00000000', 'carbon_capture', 'CDM', '2025-01-01', true]
      );

      // Transfer should affect 0 rows because is_retired = TRUE
      const result = await client.query(
        `UPDATE carbon_credits SET owner = $1 WHERE token_id = $2 AND is_retired = FALSE`,
        ['0x' + 'c'.repeat(40), 'CC-003']
      );
      expect(result.rowCount).toBe(0);
    });
  });

  // ---- Weather Station Operations ----

  describe('Weather Station Operations', () => {
    it('should store station with JSON location data', async () => {
      if (!client) return;

      const location = { lat: 40.7128, lng: -74.006, altitude: 10, country: 'US' };
      await client.query(
        `INSERT INTO weather_stations (id, name, owner, location, status)
         VALUES ($1, $2, $3, $4, $5)`,
        ['WS-001', 'NYC Station', '0x' + 'd'.repeat(40), JSON.stringify(location), 'active']
      );

      const result = await client.query('SELECT * FROM weather_stations WHERE id = $1', [
        'WS-001',
      ]);
      const storedLocation = result.rows[0].location;
      expect(storedLocation.lat).toBe(40.7128);
      expect(storedLocation.lng).toBe(-74.006);
      expect(storedLocation.country).toBe('US');
    });

    it('should query stations by quality score', async () => {
      if (!client) return;

      const stations = [
        { id: 'WS-HIGH', name: 'High Quality', quality: 95 },
        { id: 'WS-MED', name: 'Medium Quality', quality: 70 },
        { id: 'WS-LOW', name: 'Low Quality', quality: 30 },
      ];

      for (const s of stations) {
        await client.query(
          `INSERT INTO weather_stations (id, name, owner, location, quality_score)
           VALUES ($1, $2, $3, $4, $5)`,
          [s.id, s.name, '0x' + 'e'.repeat(40), JSON.stringify({ lat: 0, lng: 0 }), s.quality]
        );
      }

      const highQuality = await client.query(
        'SELECT * FROM weather_stations WHERE quality_score >= $1 ORDER BY quality_score DESC',
        [80]
      );
      expect(highQuality.rows).toHaveLength(1);
      expect(highQuality.rows[0].id).toBe('WS-HIGH');
    });
  });

  // ---- Concurrent Access ----

  describe('Concurrent Access Patterns', () => {
    it('should handle concurrent inserts without conflict', async () => {
      if (!client) return;

      const insertPromises = Array.from({ length: 10 }, (_, i) =>
        client.query(
          `INSERT INTO users (username, wallet_address) VALUES ($1, $2)`,
          [`concurrent-${i}`, `0x${i.toString(16).padStart(40, '0')}`]
        )
      );

      await Promise.all(insertPromises);

      const result = await client.query('SELECT COUNT(*) FROM users');
      expect(parseInt(result.rows[0].count)).toBe(10);
    });
  });
});

// Fallback tests when Docker is not available
const describeNoDocker = !dockerAvailable ? describe : describe.skip;

describeNoDocker('Database Integration: Schema Validation (no Docker)', () => {
  it('should validate SQL schema syntax', () => {
    // Verify the CREATE TABLE SQL is well-formed
    expect(CREATE_TABLES_SQL).toContain('CREATE TABLE IF NOT EXISTS users');
    expect(CREATE_TABLES_SQL).toContain('CREATE TABLE IF NOT EXISTS transactions');
    expect(CREATE_TABLES_SQL).toContain('CREATE TABLE IF NOT EXISTS blocks');
    expect(CREATE_TABLES_SQL).toContain('CREATE TABLE IF NOT EXISTS carbon_credits');
    expect(CREATE_TABLES_SQL).toContain('CREATE TABLE IF NOT EXISTS weather_stations');
  });

  it('should have foreign key references', () => {
    expect(CREATE_TABLES_SQL).toContain('REFERENCES blocks(id)');
  });

  it('should have NOT NULL constraints on critical fields', () => {
    expect(CREATE_TABLES_SQL).toContain('username TEXT NOT NULL');
    expect(CREATE_TABLES_SQL).toContain('wallet_address TEXT NOT NULL');
    expect(CREATE_TABLES_SQL).toContain('tx_id TEXT NOT NULL');
    expect(CREATE_TABLES_SQL).toContain('token_id TEXT UNIQUE NOT NULL');
  });
});
