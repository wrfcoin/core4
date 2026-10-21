/**
 * Test Database Utilities
 * Provides database setup, teardown, and data management for tests
 */

const { Pool } = require('pg');
const { TEST_DATABASE } = require('../test.config');
const path = require('path');
const fs = require('fs').promises;

class TestDatabase {
  constructor() {
    this.pool = null;
    this.schema = TEST_DATABASE.schema;
  }

  /**
   * Initialize database connection pool
   */
  async connect() {
    if (this.pool) return this.pool;

    this.pool = new Pool({
      host: TEST_DATABASE.host,
      port: TEST_DATABASE.port,
      database: TEST_DATABASE.database,
      user: TEST_DATABASE.user,
      password: TEST_DATABASE.password,
      max: 5,
      idleTimeoutMillis: 30000,
      connectionTimeoutMillis: 2000,
    });

    // Test connection
    try {
      const client = await this.pool.connect();
      await client.query('SELECT NOW()');
      client.release();
      console.log('✅ Test database connected');
    } catch (error) {
      console.error('❌ Test database connection failed:', error);
      throw error;
    }

    return this.pool;
  }

  /**
   * Create test schema for isolated test execution
   */
  async createSchema() {
    const client = await this.pool.connect();

    try {
      // Create schema if not exists
      await client.query(`CREATE SCHEMA IF NOT EXISTS ${this.schema}`);

      // Set search path
      await client.query(`SET search_path TO ${this.schema}, public`);

      console.log(`✅ Test schema '${this.schema}' created`);
    } finally {
      client.release();
    }
  }

  /**
   * Run migrations for test schema
   */
  async runMigrations() {
    const client = await this.pool.connect();

    try {
      // Set search path
      await client.query(`SET search_path TO ${this.schema}, public`);

      // Create migrations table
      await client.query(`
        CREATE TABLE IF NOT EXISTS migrations (
          id SERIAL PRIMARY KEY,
          filename VARCHAR(255) NOT NULL UNIQUE,
          executed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )
      `);

      // Get migration files
      const migrationsDir = path.join(
        __dirname,
        '../../backend/drizzle/migrations'
      );
      const files = await fs.readdir(migrationsDir);
      const sqlFiles = files.filter(f => f.endsWith('.sql')).sort();

      // Run each migration
      for (const file of sqlFiles) {
        // Check if already executed
        const result = await client.query(
          'SELECT 1 FROM migrations WHERE filename = $1',
          [file]
        );

        if (result.rows.length === 0) {
          // Read and execute migration
          const sql = await fs.readFile(path.join(migrationsDir, file), 'utf8');
          await client.query(sql);

          // Record migration
          await client.query('INSERT INTO migrations (filename) VALUES ($1)', [
            file,
          ]);

          console.log(`✅ Migration executed: ${file}`);
        }
      }
    } finally {
      client.release();
    }
  }

  /**
   * Seed test data
   */
  async seedData(seedFile = 'default') {
    const client = await this.pool.connect();

    try {
      // Set search path
      await client.query(`SET search_path TO ${this.schema}, public`);

      // Load seed data
      const seedPath = path.join(__dirname, `../fixtures/seeds/${seedFile}.js`);
      const seedData = require(seedPath);

      // Execute seed data
      await seedData.seed(client);

      console.log(`✅ Test data seeded: ${seedFile}`);
    } finally {
      client.release();
    }
  }

  /**
   * Clean all data from schema (between tests)
   */
  async cleanData() {
    const client = await this.pool.connect();

    try {
      // Get all tables in schema
      const result = await client.query(
        `
        SELECT tablename FROM pg_tables 
        WHERE schemaname = $1 
        AND tablename != 'migrations'
      `,
        [this.schema]
      );

      // Truncate all tables
      for (const row of result.rows) {
        await client.query(
          `TRUNCATE TABLE ${this.schema}.${row.tablename} CASCADE`
        );
      }

      console.log(`✅ Test data cleaned`);
    } finally {
      client.release();
    }
  }

  /**
   * Drop test schema
   */
  async dropSchema() {
    const client = await this.pool.connect();

    try {
      await client.query(`DROP SCHEMA IF EXISTS ${this.schema} CASCADE`);
      console.log(`✅ Test schema '${this.schema}' dropped`);
    } finally {
      client.release();
    }
  }

  /**
   * Close database connection
   */
  async disconnect() {
    if (this.pool) {
      await this.pool.end();
      this.pool = null;
      console.log('✅ Test database disconnected');
    }
  }

  /**
   * Transaction helper for tests
   */
  async transaction(callback) {
    const client = await this.pool.connect();

    try {
      await client.query('BEGIN');
      await client.query(`SET search_path TO ${this.schema}, public`);

      const result = await callback(client);

      await client.query('COMMIT');
      return result;
    } catch (error) {
      await client.query('ROLLBACK');
      throw error;
    } finally {
      client.release();
    }
  }

  /**
   * Query helper
   */
  async query(text, params) {
    const client = await this.pool.connect();

    try {
      await client.query(`SET search_path TO ${this.schema}, public`);
      return await client.query(text, params);
    } finally {
      client.release();
    }
  }

  /**
   * Get test user by role
   */
  async getTestUser(role = 'user') {
    const result = await this.query(
      'SELECT * FROM users WHERE role = $1 LIMIT 1',
      [role]
    );
    return result.rows[0];
  }

  /**
   * Create test wallet
   */
  async createTestWallet(userId, address) {
    const result = await this.query(
      'INSERT INTO wallets (user_id, address, encrypted_private_key) VALUES ($1, $2, $3) RETURNING *',
      [userId, address, 'encrypted_test_key']
    );
    return result.rows[0];
  }

  /**
   * Create test NFT
   */
  async createTestNFT(creatorId, metadata) {
    const result = await this.query(
      'INSERT INTO nfts (creator_id, name, description, metadata, token_id) VALUES ($1, $2, $3, $4, $5) RETURNING *',
      [
        creatorId,
        metadata.name,
        metadata.description,
        metadata,
        `test_nft_${Date.now()}`,
      ]
    );
    return result.rows[0];
  }
}

// Singleton instance
let instance = null;

module.exports = {
  getTestDatabase() {
    if (!instance) {
      instance = new TestDatabase();
    }
    return instance;
  },

  TestDatabase,
};
