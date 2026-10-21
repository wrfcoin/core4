const { defineConfig, devices } = require('@playwright/test');
const config = require('./test-config');

module.exports = defineConfig({
  testDir: '../workflows',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [
    ['html', { outputFolder: '../reports/html-report' }],
    ['json', { outputFile: '../reports/test-results.json' }],
    ['junit', { outputFile: '../reports/junit-results.xml' }],
    ['line'],
  ],
  use: {
    baseURL: config.baseUrls.frontend,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
    actionTimeout: config.timeouts.navigation,
    navigationTimeout: config.timeouts.navigation,
  },

  projects: [
    // Desktop browsers
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
    {
      name: 'webkit',
      use: { ...devices['Desktop Safari'] },
    },

    // Mobile browsers
    {
      name: 'Mobile Chrome',
      use: { ...devices['Pixel 5'] },
    },
    {
      name: 'Mobile Safari',
      use: { ...devices['iPhone 12'] },
    },

    // Tablet
    {
      name: 'iPad',
      use: { ...devices['iPad Pro'] },
    },
  ],

  webServer: [
    // Frontend development server
    {
      command: 'cd ../../../frontend && npm run dev',
      port: 5000,
      reuseExistingServer: !process.env.CI,
      env: {
        NODE_ENV: 'test',
      },
    },
    // Backend API server
    {
      command: 'cd ../../.. && npm run start:graphql',
      port: 4002,
      reuseExistingServer: !process.env.CI,
      env: {
        NODE_ENV: 'test',
      },
    },
  ],

  globalSetup: require.resolve('../utils/global-setup.js'),
  globalTeardown: require.resolve('../utils/global-teardown.js'),

  expect: {
    timeout: 10000,
    toHaveScreenshot: {
      mode: 'css',
      animations: 'disabled',
    },
  },
});
