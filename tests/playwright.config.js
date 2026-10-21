/**
 * Unified Playwright Configuration for WRFCoin Platform E2E Tests
 * Consolidated from multiple Playwright configurations
 */

const { defineConfig, devices } = require('@playwright/test');

module.exports = defineConfig({
  // Test directory - unified structure
  testDir: './e2e',

  // Test match patterns
  testMatch: ['**/e2e/**/*.test.js', '**/e2e/**/*.spec.js'],

  // Global test settings
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 2 : undefined,

  // Reporting
  reporter: [
    ['html', { outputFolder: '../results/playwright-report' }],
    ['json', { outputFile: '../results/test-results.json' }],
    ['junit', { outputFile: '../results/junit-e2e.xml' }],
  ],

  // Global test configuration
  use: {
    // Base URL for tests
    baseURL: process.env.BASE_URL || 'http://localhost:5001',

    // Test artifacts
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',

    // Browser settings
    ignoreHTTPSErrors: true,

    // Test timeout
    actionTimeout: 30000,
    navigationTimeout: 60000,
  },

  // Test timeout
  timeout: 120000,
  expect: {
    timeout: 30000,
  },

  // Test projects for different scenarios
  projects: [
    // Desktop browsers
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1920, height: 1080 },
      },
    },
    {
      name: 'firefox',
      use: {
        ...devices['Desktop Firefox'],
        viewport: { width: 1920, height: 1080 },
      },
    },
    {
      name: 'webkit',
      use: {
        ...devices['Desktop Safari'],
        viewport: { width: 1920, height: 1080 },
      },
    },

    // Mobile testing
    {
      name: 'mobile-chrome',
      use: { ...devices['Pixel 5'] },
    },
    {
      name: 'mobile-safari',
      use: { ...devices['iPhone 12'] },
    },

    // Specific test suites
    {
      name: 'wallet-tests',
      testMatch: '**/e2e/**/wallet*.test.js',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'governance-tests',
      testMatch: '**/e2e/**/governance*.test.js',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'nft-tests',
      testMatch: '**/e2e/**/nft*.test.js',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'bridge-tests',
      testMatch: '**/e2e/**/bridge*.test.js',
      use: { ...devices['Desktop Chrome'] },
      timeout: 180000, // Longer timeout for cross-chain operations
    },
    {
      name: 'security-tests',
      testMatch: '**/security/**/*.test.js',
      use: { ...devices['Desktop Chrome'] },
      timeout: 300000, // Extended timeout for security tests
    },
  ],

  // Web server configuration
  webServer: [
    {
      command: 'cd frontend && npm run dev -- --port 5001',
      url: 'http://localhost:5001',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
      stdout: 'ignore',
      stderr: 'pipe',
    },
    {
      command: 'cd backend && npm run dev',
      url: 'http://localhost:4001/health',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
      stdout: 'ignore',
      stderr: 'pipe',
    },
  ],

  // Global setup and teardown
  globalSetup: require.resolve('./e2e/utils/global-setup.js'),
  globalTeardown: require.resolve('./e2e/utils/global-teardown.js'),

  // Output directory for test artifacts
  outputDir: '../results/test-artifacts',

  // Metadata
  metadata: {
    platform: 'WRFCoin Platform',
    version: '1.0.0',
    testSuite: 'Unified E2E Test Suite',
  },
});
