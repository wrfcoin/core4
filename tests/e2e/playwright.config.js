const { defineConfig, devices } = require('@playwright/test');
const config = require('./config/test-config');

/**
 * @see https://playwright.dev/docs/test-configuration
 */
module.exports = defineConfig({
  testDir: './workflows',

  /* Run tests in files in parallel */
  fullyParallel: true,

  /* Fail the build on CI if you accidentally left test.only in the source code. */
  forbidOnly: !!process.env.CI,

  /* Retry on CI only */
  retries: process.env.CI ? 2 : 0,

  /* Opt out of parallel tests on CI. */
  workers: process.env.CI ? 1 : undefined,

  /* Reporter to use. See https://playwright.dev/docs/test-reporters */
  reporter: [
    ['html', { outputFolder: './reports/html-report' }],
    ['json', { outputFile: './reports/test-results.json' }],
    ['junit', { outputFile: './reports/junit-results.xml' }],
  ],

  /* Shared settings for all the projects below. See https://playwright.dev/docs/api/class-testoptions. */
  use: {
    /* Base URL to use in actions like `await page.goto('/')`. */
    baseURL: 'http://localhost:8080',

    /* Collect trace when retrying the failed test. See https://playwright.dev/docs/trace-viewer */
    trace: 'on-first-retry',

    /* Screenshot on failure */
    screenshot: 'only-on-failure',

    /* Video recording for failed tests */
    video: 'retain-on-failure',

    /* Global timeout for each action */
    actionTimeout: config.timeouts.action,

    /* Global timeout for navigation */
    navigationTimeout: config.timeouts.navigation,
  },

  /* Global setup and teardown */
  globalSetup: require.resolve('./utils/simple-setup.js'),
  globalTeardown: require.resolve('./utils/simple-teardown.js'),

  /* Configure projects for major browsers */
  projects: [
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

    /* Test against mobile viewports. */
    {
      name: 'Mobile Chrome',
      use: { ...devices['Pixel 5'] },
    },
    {
      name: 'Mobile Safari',
      use: { ...devices['iPhone 12'] },
    },

    /* Test against branded browsers. */
    {
      name: 'Microsoft Edge',
      use: { ...devices['Desktop Edge'], channel: 'msedge' },
    },
    {
      name: 'Google Chrome',
      use: { ...devices['Desktop Chrome'], channel: 'chrome' },
    },
  ],

  /* Test specific configurations */
  timeout: config.timeouts.test,
  expect: {
    timeout: config.timeouts.assertion,
  },

  /* Run your local dev server before starting the tests */
  // webServer: [
  //   {
  //     command: 'cd ../../ && npm run start:backend',
  //     url: 'http://localhost:8080/health',
  //     reuseExistingServer: !process.env.CI,
  //     timeout: 120000
  //   }
  // ],

  /* Test directory structure */
  testMatch: ['**/workflows/**/*.test.js', '**/integration/**/*.test.js'],

  /* Output directories */
  outputDir: './reports/test-artifacts',

  /* Metadata */
  metadata: {
    'test-suite': 'WRFCoin E2E Tests',
    version: '1.0.0',
    environment: process.env.E2E_ENV || 'local',
  },
});
