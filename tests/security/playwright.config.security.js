const { defineConfig, devices } = require('@playwright/test');

/**
 * Playwright configuration for Security Feature Testing
 * Configures test environment for comprehensive security validation
 */
module.exports = defineConfig({
  testDir: './security',
  testMatch: ['**/security/**/*.test.js'],
  fullyParallel: false, // Security tests should run sequentially to avoid interference
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0, // Fewer retries for security tests
  workers: 1, // Single worker to ensure test isolation
  reporter: [
    ['html', { outputFolder: 'playwright-security-report' }],
    ['json', { outputFile: 'security-test-results.json' }],
    ['line'],
  ],
  timeout: 60000, // Longer timeout for security tests

  use: {
    baseURL: 'http://localhost:5001',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    // Additional security-specific configuration
    actionTimeout: 30000,
    navigationTimeout: 30000,
    // Capture more detailed logs for security analysis
    launchOptions: {
      args: ['--enable-logging', '--log-level=0'],
    },
  },

  projects: [
    {
      name: 'security-chromium',
      use: {
        ...devices['Desktop Chrome'],
        // Enable additional security features for testing
        permissions: ['notifications', 'clipboard-read', 'clipboard-write'],
        // Test with different viewport sizes for responsive security features
        viewport: { width: 1280, height: 720 },
      },
    },
    {
      name: 'security-firefox',
      use: {
        ...devices['Desktop Firefox'],
        viewport: { width: 1280, height: 720 },
      },
    },
  ],

  // Global setup and teardown for security testing environment
  globalSetup: require.resolve('./security-global-setup.js'),
  globalTeardown: require.resolve('./security-global-teardown.js'),

  webServer: [
    {
      command: 'cd frontend && npm run dev -- --port 5001',
      url: 'http://localhost:5001',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
    },
    {
      command: 'node backend/server/main_api_server.js',
      url: 'http://localhost:4001/health',
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
    },
  ],
});
