/**
 * Global Test Setup
 * Runs once before all tests
 */

const { getTestDatabase } = require('./test-database');
const { getMockServiceManager } = require('./mock-services');
const { TEST_ENV, CI_MODE, setupTestEnvironment } = require('../test.config');
const fs = require('fs').promises;
const path = require('path');

module.exports = async function globalSetup() {
  console.log('\n🚀 Starting Global Test Setup...\n');
  console.log(`Environment: ${TEST_ENV}`);
  console.log(`CI Mode: ${CI_MODE}`);
  console.log(`Workers: ${process.env.TEST_WORKER_ID || '1'}\n`);

  try {
    // 1. Setup test environment configuration
    await setupTestEnvironment();

    // 2. Initialize test database
    if (TEST_ENV === 'local' || TEST_ENV === 'test') {
      const db = getTestDatabase();
      await db.connect();
      await db.createSchema();
      await db.runMigrations();
      await db.seedData('default');
    }

    // 3. Start mock services
    if (TEST_ENV === 'local' || TEST_ENV === 'test') {
      const mockManager = getMockServiceManager();
      await mockManager.startAll();
    }

    // 4. Create test report directory
    const reportDir = path.join(__dirname, '../reports');
    await fs.mkdir(reportDir, { recursive: true });

    // 5. Create test artifacts directory
    const artifactsDir = path.join(__dirname, '../artifacts');
    await fs.mkdir(artifactsDir, { recursive: true });

    // 6. Generate auth state files for Playwright
    await generateAuthStates();

    // 7. Clear previous test results
    if (CI_MODE) {
      await clearPreviousResults();
    }

    console.log('\n✅ Global Test Setup Complete\n');

    // Return a teardown function
    return async () => {
      // This will be called by Playwright after all tests
      await require('./global-teardown')();
    };
  } catch (error) {
    console.error('❌ Global Test Setup Failed:', error);
    throw error;
  }
};

/**
 * Generate authentication state files for different user roles
 */
async function generateAuthStates() {
  const { TEST_DATA } = require('../test.config');
  const stateDir = path.join(__dirname, '../fixtures');

  await fs.mkdir(stateDir, { recursive: true });

  // Generate auth states for different roles
  const authStates = {
    'auth-user.json': {
      cookies: [],
      origins: [
        {
          origin: 'http://localhost:3000',
          localStorage: [
            {
              name: 'auth-token',
              value: generateMockToken(TEST_DATA.users.user),
            },
            {
              name: 'user-role',
              value: 'user',
            },
          ],
        },
      ],
    },
    'auth-wallet.json': {
      cookies: [],
      origins: [
        {
          origin: 'http://localhost:3000',
          localStorage: [
            {
              name: 'auth-token',
              value: generateMockToken(TEST_DATA.users.user),
            },
            {
              name: 'wallet-address',
              value: TEST_DATA.wallets.primary.address,
            },
            {
              name: 'wallet-connected',
              value: 'true',
            },
          ],
        },
      ],
    },
    'auth-validator.json': {
      cookies: [],
      origins: [
        {
          origin: 'http://localhost:3000',
          localStorage: [
            {
              name: 'auth-token',
              value: generateMockToken(TEST_DATA.users.validator),
            },
            {
              name: 'user-role',
              value: 'validator',
            },
          ],
        },
      ],
    },
    'auth-admin.json': {
      cookies: [],
      origins: [
        {
          origin: 'http://localhost:3000',
          localStorage: [
            {
              name: 'auth-token',
              value: generateMockToken(TEST_DATA.users.admin),
            },
            {
              name: 'user-role',
              value: 'admin',
            },
          ],
        },
      ],
    },
  };

  for (const [filename, state] of Object.entries(authStates)) {
    await fs.writeFile(
      path.join(stateDir, filename),
      JSON.stringify(state, null, 2)
    );
  }

  console.log('✅ Authentication states generated');
}

/**
 * Generate mock JWT token
 */
function generateMockToken(user) {
  // Simple mock token for testing
  const payload = {
    sub: user.email,
    role: user.role,
    exp: Math.floor(Date.now() / 1000) + 24 * 60 * 60, // 24 hours
  };

  // Base64 encode (not a real JWT, just for testing)
  return Buffer.from(JSON.stringify(payload)).toString('base64');
}

/**
 * Clear previous test results
 */
async function clearPreviousResults() {
  const dirs = [
    path.join(__dirname, '../reports'),
    path.join(__dirname, '../artifacts'),
    path.join(__dirname, '../../coverage'),
  ];

  for (const dir of dirs) {
    try {
      const files = await fs.readdir(dir);
      for (const file of files) {
        if (file !== '.gitkeep') {
          await fs.rm(path.join(dir, file), { recursive: true, force: true });
        }
      }
    } catch (error) {
      // Directory might not exist
    }
  }

  console.log('✅ Previous test results cleared');
}
