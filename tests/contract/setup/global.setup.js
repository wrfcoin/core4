/**
 * Global Contract Testing Setup
 * Initializes the contract testing environment and ensures all dependencies are ready
 */

const { execSync } = require('child_process');
const { setupAllProviders } = require('./pact.setup');
const path = require('path');

// Environment setup for contract testing
const setupTestEnvironment = () => {
  // Set test environment variables
  process.env.NODE_ENV = 'test';
  process.env.DATABASE_URL =
    'postgresql://postgres:testpass@localhost:5434/wrfcoin_test';
  process.env.WEATHER_API_URL = 'http://localhost:3010';
  process.env.BLOCKCHAIN_RPC_URL = 'http://localhost:8545';
  process.env.CARBON_ORACLE_URL = 'http://localhost:3020';
  process.env.PACT_BROKER_BASE_URL = 'http://localhost:9292';
  process.env.PACT_BROKER_USERNAME = 'admin';
  if (!process.env.PACT_BROKER_PASSWORD) {
    throw new Error(
      'PACT_BROKER_PASSWORD must be set. See .env.example for instructions.'
    );
  }

  console.log('Contract testing environment variables set');
};

// Check if required services are running
const checkServiceHealth = async (url, serviceName, timeout = 30000) => {
  const startTime = Date.now();

  while (Date.now() - startTime < timeout) {
    try {
      const response = await fetch(`${url}/health`);
      if (response.ok) {
        console.log(`✅ ${serviceName} is healthy`);
        return true;
      }
    } catch (error) {
      // Service not ready yet, wait and retry
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
  }

  throw new Error(`❌ ${serviceName} failed health check after ${timeout}ms`);
};

// Wait for all mock services to be ready
const waitForMockServices = async () => {
  console.log('Waiting for mock services to be ready...');

  const services = [
    { url: 'http://localhost:3010', name: 'Weather API Mock' },
    { url: 'http://localhost:8545', name: 'Blockchain RPC Mock' },
    { url: 'http://localhost:3020', name: 'Carbon Oracle Mock' },
  ];

  const healthChecks = services.map(service =>
    checkServiceHealth(service.url, service.name)
  );

  await Promise.all(healthChecks);
  console.log('All mock services are ready');
};

// Set up test database schema
const setupTestDatabase = async () => {
  console.log('Setting up test database schema...');

  try {
    // Run database migrations for test environment
    const migrationsPath = path.resolve(
      __dirname,
      '../../..',
      'backend/drizzle'
    );

    // Check if drizzle-kit is available and run migrations
    try {
      execSync('npx drizzle-kit push:pg', {
        env: {
          ...process.env,
          DATABASE_URL:
            'postgresql://postgres:testpass@localhost:5434/wrfcoin_test',
        },
        stdio: 'pipe',
        cwd: path.resolve(__dirname, '../../..'),
      });
      console.log('✅ Test database schema ready');
    } catch (migrationError) {
      console.warn(
        '⚠️  Database migrations failed, continuing with existing schema'
      );
      console.warn('Migration error:', migrationError.message);
    }
  } catch (error) {
    console.error('Failed to set up test database:', error);
    throw error;
  }
};

// Start Docker services for contract testing
const startDockerServices = async () => {
  console.log('Starting Docker services for contract testing...');

  try {
    const dockerComposePath = path.resolve(
      __dirname,
      '../docker-compose.pact.yml'
    );

    // Start the services (excluding test runner)
    execSync(
      `docker-compose -f "${dockerComposePath}" up -d --remove-orphans pact-broker postgres weather-api-mock blockchain-rpc-mock carbon-oracle-mock test-postgres`,
      {
        stdio: 'inherit',
        cwd: path.resolve(__dirname, '..'),
      }
    );

    console.log('✅ Docker services started');

    // Wait a bit for services to initialize
    await new Promise(resolve => setTimeout(resolve, 5000));
  } catch (error) {
    console.error('Failed to start Docker services:', error);
    throw error;
  }
};

// Check if Docker services are already running
const checkDockerServices = async () => {
  try {
    execSync('docker-compose -f docker-compose.pact.yml ps', {
      cwd: path.resolve(__dirname, '..'),
      stdio: 'pipe',
    });
    return true;
  } catch (error) {
    return false;
  }
};

// Main setup function
const globalSetup = async () => {
  console.log('🚀 Starting contract testing global setup...');

  try {
    // 1. Setup test environment variables
    setupTestEnvironment();

    // 2. Check if Docker services are already running, start if needed
    const servicesRunning = await checkDockerServices();
    if (!servicesRunning) {
      await startDockerServices();
    } else {
      console.log('✅ Docker services already running');
    }

    // 3. Wait for mock services to be ready
    await waitForMockServices();

    // 4. Setup test database
    await setupTestDatabase();

    // 5. Setup Pact providers
    await setupAllProviders();

    console.log('✅ Contract testing global setup completed successfully');
  } catch (error) {
    console.error('❌ Contract testing global setup failed:', error);

    // Cleanup on failure
    try {
      const dockerComposePath = path.resolve(
        __dirname,
        '../docker-compose.pact.yml'
      );
      execSync(`docker-compose -f "${dockerComposePath}" down`, {
        stdio: 'inherit',
        cwd: path.resolve(__dirname, '..'),
      });
    } catch (cleanupError) {
      console.error('Failed to cleanup Docker services:', cleanupError);
    }

    throw error;
  }
};

module.exports = globalSetup;
