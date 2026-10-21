import { FullConfig } from '@playwright/test';
import { exec } from 'child_process';
import { promisify } from 'util';
import axios from 'axios';

const execAsync = promisify(exec);

async function globalSetup(config: FullConfig) {
  console.log('🚀 Starting WRFCoin E2E Test Environment Setup...');

  // Health check for required services
  const services = [
    { name: 'Frontend', url: 'http://localhost:5001' },
    { name: 'Main API', url: 'http://localhost:4001/health' },
    { name: 'Cross-Chain Protocol', url: 'http://localhost:3000/health' },
    { name: 'Wallet Server', url: 'http://localhost:4000/health' },
  ];

  // Wait for services to be ready
  console.log('⏳ Waiting for services to be ready...');

  for (const service of services) {
    await waitForService(service.name, service.url);
  }

  // Setup test database
  console.log('🗄️ Setting up test database...');
  try {
    await execAsync('npm run db:setup:test');
    console.log('✅ Test database setup completed');
  } catch (error) {
    console.warn('⚠️ Test database setup failed, continuing...');
  }

  // Deploy test contracts if needed
  if (process.env.DEPLOY_TEST_CONTRACTS !== 'false') {
    console.log('📜 Deploying test smart contracts...');
    try {
      await execAsync(
        'npx hardhat run scripts/deploy-test-contracts.js --network localhost'
      );
      console.log('✅ Test contracts deployed');
    } catch (error) {
      console.warn('⚠️ Test contract deployment failed, continuing...');
    }
  }

  // Seed test data
  console.log('🌱 Seeding test data...');
  try {
    await execAsync('npm run seed:test');
    console.log('✅ Test data seeded');
  } catch (error) {
    console.warn('⚠️ Test data seeding failed, continuing...');
  }

  console.log('✅ WRFCoin E2E Test Environment Setup Complete!');
}

async function waitForService(
  name: string,
  url: string,
  maxRetries: number = 30
) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      const response = await axios.get(url, { timeout: 5000 });
      if (response.status === 200) {
        console.log(`✅ ${name} is ready`);
        return;
      }
    } catch (error) {
      console.log(`⏳ Waiting for ${name} (attempt ${i + 1}/${maxRetries})...`);
      await new Promise(resolve => setTimeout(resolve, 2000));
    }
  }

  console.warn(
    `⚠️ ${name} not ready after ${maxRetries} attempts, continuing...`
  );
}

export default globalSetup;
