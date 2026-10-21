/**
 * Wallet Testing Global Setup
 * Prepares test environment for wallet functionality testing
 *
 * Task Master Subtask 8.1: Set up Playwright Environment and Test Fixtures
 * Created: 2025-06-23
 */

const { chromium } = require('@playwright/test');
const path = require('path');
const fs = require('fs');

async function globalSetup(config) {
  console.log('🚀 Starting Wallet Testing Global Setup...');

  // Create test results directory
  const testResultsDir = path.join(__dirname, 'wallet-test-results');
  if (!fs.existsSync(testResultsDir)) {
    fs.mkdirSync(testResultsDir, { recursive: true });
  }

  // Create screenshots directory
  const screenshotsDir = path.join(testResultsDir, 'screenshots');
  if (!fs.existsSync(screenshotsDir)) {
    fs.mkdirSync(screenshotsDir, { recursive: true });
  }

  // Launch browser for pre-setup
  const browser = await chromium.launch();
  const context = await browser.newContext();
  const page = await context.newPage();

  try {
    console.log('📋 Verifying test environment...');

    // Check if frontend is running
    try {
      await page.goto('http://localhost:5000', { timeout: 30000 });
      console.log('✅ Frontend server is accessible');
    } catch (error) {
      console.warn('⚠️  Frontend server not accessible, tests may fail');
    }

    // Check if wallet API is running
    try {
      const response = await page.request.get('http://localhost:4000/health');
      if (response.ok()) {
        console.log('✅ Wallet API server is accessible');
      }
    } catch (error) {
      console.warn('⚠️  Wallet API server not accessible, some tests may fail');
    }

    // Prepare test data
    const testDataDir = path.join(__dirname, 'test-data');
    if (!fs.existsSync(testDataDir)) {
      fs.mkdirSync(testDataDir, { recursive: true });
    }

    // Create mock wallet data
    const mockWalletData = {
      testWallets: [
        {
          id: 'test-wallet-1',
          address: 'wrf717d72395303fcb2aed4df368f32f1078d18e9cc',
          balance: '8.60 WRF',
          network: 'testnet',
          created: new Date().toISOString(),
        },
      ],
      testTransactions: [
        {
          id: 'tx-1',
          type: 'receive',
          amount: '2.50',
          currency: 'WRF',
          timestamp: new Date().toISOString(),
          status: 'confirmed',
        },
        {
          id: 'tx-2',
          type: 'send',
          amount: '1.00',
          currency: 'WRF',
          timestamp: new Date(Date.now() - 3600000).toISOString(),
          status: 'confirmed',
        },
      ],
      testAssets: [
        {
          symbol: 'WRF',
          balance: '8.60',
          usdValue: '$12.45',
          change24h: '+2.3%',
        },
        {
          symbol: 'ETH',
          balance: '0.0025',
          usdValue: '$5.20',
          change24h: '-1.1%',
        },
        {
          symbol: 'BTC',
          balance: '0.00015',
          usdValue: '$8.80',
          change24h: '+0.5%',
        },
      ],
    };

    fs.writeFileSync(
      path.join(testDataDir, 'mock-wallet-data.json'),
      JSON.stringify(mockWalletData, null, 2)
    );

    console.log('✅ Test data prepared');

    // Create baseline screenshots for visual regression testing
    try {
      await page.goto('http://localhost:5000/wallet', { timeout: 30000 });
      await page.waitForLoadState('networkidle');

      // Take baseline screenshot
      await page.screenshot({
        path: path.join(screenshotsDir, 'wallet-baseline.png'),
        fullPage: true,
      });

      console.log('✅ Baseline screenshots captured');
    } catch (error) {
      console.warn(
        '⚠️  Could not capture baseline screenshots:',
        error.message
      );
    }

    // Validate test environment configuration
    console.log('📊 Test Environment Summary:');
    console.log(`   • Frontend URL: http://localhost:5000`);
    console.log(`   • Wallet API URL: http://localhost:4000`);
    console.log(`   • Test Results Dir: ${testResultsDir}`);
    console.log(`   • Screenshots Dir: ${screenshotsDir}`);
    console.log(`   • Test Data Dir: ${testDataDir}`);

    console.log('✅ Wallet Testing Global Setup Complete!');
  } catch (error) {
    console.error('❌ Global setup failed:', error);
    throw error;
  } finally {
    await context.close();
    await browser.close();
  }
}

module.exports = globalSetup;
