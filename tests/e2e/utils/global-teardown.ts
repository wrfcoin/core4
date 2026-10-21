import { FullConfig } from '@playwright/test';
import { exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);

async function globalTeardown(config: FullConfig) {
  console.log('🧹 Starting WRFCoin E2E Test Environment Teardown...');

  // Cleanup test data
  console.log('🗑️ Cleaning up test data...');
  try {
    await execAsync('npm run cleanup:test');
    console.log('✅ Test data cleaned up');
  } catch (error) {
    console.warn('⚠️ Test data cleanup failed');
  }

  // Stop background services if they were started during setup
  if (process.env.CLEANUP_SERVICES === 'true') {
    console.log('🛑 Stopping test services...');
    try {
      await execAsync('pkill -f "hardhat node"');
      console.log('✅ Test blockchain stopped');
    } catch (error) {
      console.warn('⚠️ Could not stop test blockchain');
    }
  }

  // Generate test report summary
  console.log('📊 Generating test report summary...');
  try {
    // This could be expanded to generate custom reports
    console.log('✅ Test reports available in tests/e2e/reports/');
  } catch (error) {
    console.warn('⚠️ Report generation failed');
  }

  console.log('✅ WRFCoin E2E Test Environment Teardown Complete!');
}

export default globalTeardown;
