const { chromium } = require('@playwright/test');
const config = require('../config/test-config');

async function globalSetup() {
  console.log('🚀 Starting E2E test environment setup...');

  // Start blockchain network if needed
  if (process.env.E2E_START_BLOCKCHAIN !== 'false') {
    await startBlockchainNetwork();
  }

  // Deploy smart contracts
  if (process.env.E2E_DEPLOY_CONTRACTS !== 'false') {
    await deploySmartContracts();
  }

  // Seed test data
  if (process.env.E2E_SEED_DATA !== 'false') {
    await seedTestData();
  }

  // Warm up services
  await warmUpServices();

  console.log('✅ E2E test environment setup complete');
}

async function startBlockchainNetwork() {
  console.log('⛓️ Starting local blockchain network...');

  const { spawn } = require('child_process');

  return new Promise((resolve, reject) => {
    const hardhat = spawn('npx', ['hardhat', 'node'], {
      cwd: process.cwd(),
      stdio: 'pipe',
    });

    let started = false;

    hardhat.stdout.on('data', data => {
      const output = data.toString();
      console.log(`Hardhat: ${output}`);

      if (
        output.includes('Started HTTP and WebSocket JSON-RPC server') &&
        !started
      ) {
        started = true;
        console.log('✅ Blockchain network started');
        resolve();
      }
    });

    hardhat.stderr.on('data', data => {
      console.error(`Hardhat Error: ${data.toString()}`);
    });

    hardhat.on('close', code => {
      if (code !== 0 && !started) {
        reject(new Error(`Hardhat process exited with code ${code}`));
      }
    });

    // Store process ID for cleanup
    process.env.HARDHAT_PID = hardhat.pid;

    // Timeout after 30 seconds
    setTimeout(() => {
      if (!started) {
        hardhat.kill();
        reject(new Error('Hardhat network failed to start within 30 seconds'));
      }
    }, 30000);
  });
}

async function deploySmartContracts() {
  console.log('📄 Deploying smart contracts...');

  const { spawn } = require('child_process');

  return new Promise((resolve, reject) => {
    const deploy = spawn(
      'npx',
      ['hardhat', 'run', 'scripts/deploy.js', '--network', 'localhost'],
      {
        cwd: process.cwd(),
        stdio: 'pipe',
      }
    );

    let output = '';

    deploy.stdout.on('data', data => {
      output += data.toString();
      console.log(`Deploy: ${data.toString()}`);
    });

    deploy.stderr.on('data', data => {
      console.error(`Deploy Error: ${data.toString()}`);
    });

    deploy.on('close', code => {
      if (code === 0) {
        console.log('✅ Smart contracts deployed');

        // Extract contract addresses from output
        const addresses = extractContractAddresses(output);
        process.env.CONTRACT_ADDRESSES = JSON.stringify(addresses);

        resolve();
      } else {
        reject(new Error(`Contract deployment failed with code ${code}`));
      }
    });
  });
}

async function seedTestData() {
  console.log('🌱 Seeding test data...');

  const browser = await chromium.launch();
  const context = await browser.newContext();
  const page = await context.newPage();

  try {
    // Wait for backend to be ready
    await waitForService(`${config.baseUrls.backend}/health`, 30000);

    // Seed users
    await seedUsers(page);

    // Seed weather stations
    await seedWeatherStations(page);

    // Seed initial tokens
    await seedTokens(page);

    console.log('✅ Test data seeded successfully');
  } catch (error) {
    console.error('❌ Failed to seed test data:', error);
    throw error;
  } finally {
    await browser.close();
  }
}

async function seedUsers(page) {
  const testData = require('../fixtures/test-data.json');

  for (const [userType, userData] of Object.entries(testData.users)) {
    try {
      await page.request.post(`${config.baseUrls.backend}/api/test/users`, {
        data: {
          ...userData,
          userType,
        },
      });
      console.log(`✅ Created test user: ${userType}`);
    } catch (error) {
      console.warn(`⚠️ Failed to create user ${userType}:`, error.message);
    }
  }
}

async function seedWeatherStations(page) {
  const testData = require('../fixtures/test-data.json');

  for (const [stationId, stationData] of Object.entries(
    testData.weatherStations
  )) {
    try {
      await page.request.post(
        `${config.baseUrls.backend}/api/test/weather-stations`,
        {
          data: stationData,
        }
      );
      console.log(`✅ Created weather station: ${stationId}`);
    } catch (error) {
      console.warn(
        `⚠️ Failed to create weather station ${stationId}:`,
        error.message
      );
    }
  }
}

async function seedTokens(page) {
  const testData = require('../fixtures/test-data.json');

  try {
    // Mint initial tokens for test users
    for (const [userType, userData] of Object.entries(testData.users)) {
      await page.request.post(
        `${config.baseUrls.backend}/api/test/mint-tokens`,
        {
          data: {
            address: userData.walletAddress,
            amount: '10000', // 10,000 WRF tokens
            token: 'WRF',
          },
        }
      );
      console.log(`✅ Minted tokens for: ${userType}`);
    }
  } catch (error) {
    console.warn('⚠️ Failed to mint test tokens:', error.message);
  }
}

async function warmUpServices() {
  console.log('🔥 Warming up services...');

  const services = [
    { name: 'Frontend', url: config.baseUrls.frontend },
    { name: 'Backend API', url: `${config.baseUrls.backend}/health` },
    { name: 'GraphQL', url: `${config.baseUrls.graphql}/health` },
  ];

  for (const service of services) {
    try {
      await waitForService(service.url, 30000);
      console.log(`✅ ${service.name} is ready`);
    } catch (error) {
      console.warn(`⚠️ ${service.name} not ready:`, error.message);
    }
  }
}

async function waitForService(url, timeout = 30000) {
  const startTime = Date.now();

  while (Date.now() - startTime < timeout) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch (error) {
      // Service not ready yet, continue waiting
    }

    await new Promise(resolve => setTimeout(resolve, 1000));
  }

  throw new Error(`Service at ${url} not ready within ${timeout}ms`);
}

function extractContractAddresses(deployOutput) {
  const addresses = {};
  const addressRegex = /(\w+) deployed to: (0x[a-fA-F0-9]{40})/g;
  let match;

  while ((match = addressRegex.exec(deployOutput)) !== null) {
    addresses[match[1]] = match[2];
  }

  return addresses;
}

module.exports = globalSetup;
