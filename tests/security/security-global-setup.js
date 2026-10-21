const { chromium } = require('@playwright/test');

/**
 * Global setup for security testing environment
 * Initializes test blockchain network and user accounts
 */
async function globalSetup(config) {
  console.log('🔒 Setting up security testing environment...');

  // Launch browser for setup operations
  const browser = await chromium.launch();
  const context = await browser.newContext();
  const page = await context.newPage();

  try {
    // Wait for frontend to be available
    console.log('⏳ Waiting for frontend to be available...');
    await page.goto('http://localhost:5001', { waitUntil: 'networkidle' });

    // Initialize test accounts and security settings
    console.log('👤 Initializing test accounts...');
    await setupTestAccounts(page);

    // Setup mock blockchain environment
    console.log('⛓️ Setting up test blockchain environment...');
    await setupTestBlockchain(page);

    // Initialize security features
    console.log('🛡️ Initializing security features...');
    await initializeSecurityFeatures(page);

    console.log('✅ Security testing environment setup complete');
  } catch (error) {
    console.error('❌ Failed to setup security testing environment:', error);
    throw error;
  } finally {
    await browser.close();
  }
}

/**
 * Setup test accounts with different security profiles
 */
async function setupTestAccounts(page) {
  // Test account configurations
  const testAccounts = [
    {
      id: 'test-user-1',
      name: 'Standard User',
      privacyLevel: 'STANDARD',
      mevProtection: true,
    },
    {
      id: 'test-user-2',
      name: 'High Privacy User',
      privacyLevel: 'HIGH',
      mevProtection: true,
    },
    {
      id: 'test-user-3',
      name: 'Vulnerable User',
      privacyLevel: 'LOW',
      mevProtection: false,
    },
  ];

  // Store test accounts in browser storage for tests to use
  await page.evaluate(accounts => {
    window.localStorage.setItem('test-accounts', JSON.stringify(accounts));
  }, testAccounts);
}

/**
 * Setup test blockchain environment with realistic transaction data
 */
async function setupTestBlockchain(page) {
  // Mock transaction data for testing
  const mockTransactions = [
    {
      hash: '0x1234567890abcdef1234567890abcdef12345678',
      from: '0x742d35Cc6634C0532925a3b8D0c8b9dE0a3fcA0E',
      to: '0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984',
      value: '1000000000000000000', // 1 ETH
      gasPrice: '20000000000', // 20 Gwei
      gasLimit: '21000',
      data: '0x',
      timestamp: Date.now() - 60000,
      blockNumber: 1000001,
    },
    {
      hash: '0xabcdef1234567890abcdef1234567890abcdef12',
      from: '0x742d35Cc6634C0532925a3b8D0c8b9dE0a3fcA0E',
      to: '0xUniswapV2Router02',
      value: '500000000000000000', // 0.5 ETH
      gasPrice: '25000000000', // 25 Gwei (higher - potential MEV)
      gasLimit: '150000',
      data: '0x7ff36ab5', // swapExactETHForTokens
      timestamp: Date.now() - 30000,
      blockNumber: 1000002,
    },
  ];

  await page.evaluate(transactions => {
    window.localStorage.setItem(
      'mock-transactions',
      JSON.stringify(transactions)
    );
  }, mockTransactions);
}

/**
 * Initialize security features for testing
 */
async function initializeSecurityFeatures(page) {
  const securityConfig = {
    mevProtection: {
      enabled: true,
      thresholds: {
        riskScore: 75,
        gasPrice: 50000000000, // 50 Gwei
      },
    },
    privacySettings: {
      defaultLevel: 'STANDARD',
      messagingEncryption: true,
    },
    alerts: {
      enabled: true,
      levels: ['CRITICAL', 'WARNING', 'INFO'],
    },
  };

  await page.evaluate(config => {
    window.localStorage.setItem('security-config', JSON.stringify(config));
  }, securityConfig);
}

module.exports = globalSetup;
