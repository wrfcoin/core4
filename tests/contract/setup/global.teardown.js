/**
 * Global Contract Testing Teardown
 * Cleans up the contract testing environment and stops all services
 */

const { execSync } = require('child_process');
const { teardownAllProviders } = require('./pact.setup');
const path = require('path');

// Cleanup test database
const cleanupTestDatabase = async () => {
  console.log('Cleaning up test database...');

  try {
    // Drop test database to ensure clean state for next run
    const { Pool } = require('pg');

    const pool = new Pool({
      host: 'localhost',
      port: 5434,
      user: 'postgres',
      password: 'testpass',
      database: 'postgres', // Connect to postgres db to drop test db
    });

    try {
      // Terminate existing connections to test database
      await pool.query(`
        SELECT pg_terminate_backend(pid)
        FROM pg_stat_activity
        WHERE datname = 'wrfcoin_test' AND pid <> pg_backend_pid()
      `);

      // Drop and recreate test database
      await pool.query('DROP DATABASE IF EXISTS wrfcoin_test');
      await pool.query('CREATE DATABASE wrfcoin_test');

      console.log('✅ Test database cleaned up');
    } catch (dbError) {
      console.warn('⚠️  Database cleanup warning:', dbError.message);
    } finally {
      await pool.end();
    }
  } catch (error) {
    console.warn('⚠️  Failed to cleanup test database:', error.message);
  }
};

// Stop Docker services
const stopDockerServices = async () => {
  console.log('Stopping Docker services...');

  try {
    const dockerComposePath = path.resolve(
      __dirname,
      '../docker-compose.pact.yml'
    );

    // Stop and remove containers
    execSync(`docker-compose -f "${dockerComposePath}" down --remove-orphans`, {
      stdio: 'inherit',
      cwd: path.resolve(__dirname, '..'),
    });

    console.log('✅ Docker services stopped');
  } catch (error) {
    console.warn('⚠️  Failed to stop Docker services:', error.message);
  }
};

// Cleanup Pact files
const cleanupPactFiles = async () => {
  console.log('Cleaning up Pact files...');

  try {
    const fs = require('fs').promises;
    const pactsDir = path.resolve(__dirname, '../pacts');
    const logsDir = path.resolve(__dirname, '../logs');

    // Clean pact files
    try {
      const pactFiles = await fs.readdir(pactsDir);
      for (const file of pactFiles) {
        if (file.endsWith('.json')) {
          await fs.unlink(path.join(pactsDir, file));
        }
      }
    } catch (error) {
      if (error.code !== 'ENOENT') {
        console.warn('⚠️  Failed to clean pact files:', error.message);
      }
    }

    // Clean log files
    try {
      const logFiles = await fs.readdir(logsDir);
      for (const file of logFiles) {
        if (file.endsWith('.log')) {
          await fs.unlink(path.join(logsDir, file));
        }
      }
    } catch (error) {
      if (error.code !== 'ENOENT') {
        console.warn('⚠️  Failed to clean log files:', error.message);
      }
    }

    console.log('✅ Pact files cleaned up');
  } catch (error) {
    console.warn('⚠️  Failed to cleanup Pact files:', error.message);
  }
};

// Publish Pact contracts if in CI environment
const publishPactContracts = async () => {
  if (process.env.CI !== 'true') {
    console.log('Skipping Pact contract publishing (not in CI environment)');
    return;
  }

  console.log('Publishing Pact contracts to broker...');

  try {
    const { Publisher } = require('@pact-foundation/pact');

    const publisher = new Publisher({
      pactFilesOrDirs: [path.resolve(__dirname, '../pacts')],
      pactBroker: process.env.PACT_BROKER_BASE_URL || 'http://localhost:9292',
      pactBrokerUsername: process.env.PACT_BROKER_USERNAME,
      pactBrokerPassword: process.env.PACT_BROKER_PASSWORD,
      consumerVersion: process.env.npm_package_version || '1.0.0',
      tags: [process.env.NODE_ENV || 'test', 'contract-tests'],
    });

    await publisher.publishPacts();
    console.log('✅ Pact contracts published to broker');
  } catch (error) {
    console.warn('⚠️  Failed to publish Pact contracts:', error.message);
  }
};

// Generate contract test report
const generateTestReport = async () => {
  console.log('Generating contract test report...');

  try {
    const fs = require('fs').promises;
    const pactsDir = path.resolve(__dirname, '../pacts');
    const reportsDir = path.resolve(__dirname, '../../results');

    // Ensure reports directory exists
    await fs.mkdir(reportsDir, { recursive: true });

    // Read all pact files and generate summary
    const pactFiles = await fs.readdir(pactsDir).catch(() => []);
    const pactContracts = [];

    for (const file of pactFiles) {
      if (file.endsWith('.json')) {
        try {
          const pactContent = await fs.readFile(
            path.join(pactsDir, file),
            'utf8'
          );
          const pact = JSON.parse(pactContent);
          pactContracts.push({
            consumer: pact.consumer.name,
            provider: pact.provider.name,
            interactions: pact.interactions.length,
            file: file,
          });
        } catch (error) {
          console.warn(`⚠️  Failed to read pact file ${file}:`, error.message);
        }
      }
    }

    const report = {
      timestamp: new Date().toISOString(),
      summary: {
        totalContracts: pactContracts.length,
        totalInteractions: pactContracts.reduce(
          (sum, pact) => sum + pact.interactions,
          0
        ),
      },
      contracts: pactContracts,
      environment: {
        nodeVersion: process.version,
        testEnvironment: process.env.NODE_ENV,
        pactSpecVersion: 3,
      },
    };

    await fs.writeFile(
      path.join(reportsDir, 'contract-test-report.json'),
      JSON.stringify(report, null, 2)
    );

    console.log('✅ Contract test report generated');
    console.log(
      `📊 Contract Summary: ${report.summary.totalContracts} contracts, ${report.summary.totalInteractions} interactions`
    );
  } catch (error) {
    console.warn('⚠️  Failed to generate test report:', error.message);
  }
};

// Main teardown function
const globalTeardown = async () => {
  console.log('🧹 Starting contract testing global teardown...');

  try {
    // 1. Teardown Pact providers
    await teardownAllProviders();

    // 2. Publish contracts if in CI
    await publishPactContracts();

    // 3. Generate test report
    await generateTestReport();

    // 4. Cleanup test database
    await cleanupTestDatabase();

    // 5. Stop Docker services
    await stopDockerServices();

    // 6. Cleanup Pact files (optional, keep for debugging)
    if (process.env.CLEANUP_PACT_FILES === 'true') {
      await cleanupPactFiles();
    }

    console.log('✅ Contract testing global teardown completed successfully');
  } catch (error) {
    console.error('❌ Contract testing global teardown failed:', error);

    // Force cleanup Docker services even if other steps failed
    try {
      await stopDockerServices();
    } catch (cleanupError) {
      console.error('Failed final Docker cleanup:', cleanupError);
    }

    // Don't throw error during teardown to avoid masking test failures
    console.warn('⚠️  Some cleanup steps failed, but continuing...');
  }
};

module.exports = globalTeardown;
