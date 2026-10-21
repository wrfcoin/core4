/**
 * Playwright Configuration for Wallet Testing
 * Optimized for comprehensive wallet functionality testing across browsers
 *
 * Task Master Subtask 8.1: Set up Playwright Environment and Test Fixtures
 * Created: 2025-06-23
 */

const { defineConfig, devices } = require('@playwright/test');

module.exports = defineConfig({
  testDir: '.',
  testMatch: '**/wallet-*.test.js',

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
    ['html', { outputFolder: 'wallet-html-report' }],
    ['json', { outputFile: 'wallet-test-results/results.json' }],
    ['list'],
  ],

  /* Shared settings for all the projects below. See https://playwright.dev/docs/api/class-testoptions. */
  use: {
    /* Base URL to use in actions like `await page.goto('/')`. */
    baseURL: 'http://localhost:5000',

    /* Collect trace when retrying the failed test. See https://playwright.dev/docs/trace-viewer */
    trace: 'on-first-retry',

    /* Take screenshot on failure */
    screenshot: 'only-on-failure',

    /* Record video on failure */
    video: 'retain-on-failure',

    /* Global timeout for each test */
    actionTimeout: 15000,

    /* Global timeout for navigation */
    navigationTimeout: 30000,

    /* Wait for load state */
    waitForLoadState: 'networkidle',

    /* Extra HTTP headers */
    extraHTTPHeaders: {
      Accept: 'application/json',
      'X-Test-Environment': 'playwright-wallet',
    },
  },

  /* Configure projects for major browsers */
  projects: [
    {
      name: 'chromium-wallet',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1280, height: 720 },
      },
    },

    {
      name: 'firefox-wallet',
      use: {
        ...devices['Desktop Firefox'],
        viewport: { width: 1280, height: 720 },
      },
    },

    {
      name: 'webkit-wallet',
      use: {
        ...devices['Desktop Safari'],
        viewport: { width: 1280, height: 720 },
      },
    },

    /* Test against mobile viewports. */
    {
      name: 'mobile-chrome-wallet',
      use: {
        ...devices['Pixel 5'],
      },
    },
    {
      name: 'mobile-safari-wallet',
      use: {
        ...devices['iPhone 12'],
      },
    },

    /* Test against branded browsers. */
    {
      name: 'edge-wallet',
      use: {
        ...devices['Desktop Edge'],
        channel: 'msedge',
      },
    },
    {
      name: 'chrome-wallet',
      use: {
        ...devices['Desktop Chrome'],
        channel: 'chrome',
      },
    },
  ],

  /* Global setup and teardown */
  globalSetup: require.resolve('./wallet-global-setup.js'),
  globalTeardown: require.resolve('./wallet-global-teardown.js'),

  /* Run your local dev server before starting the tests */
  webServer: [
    {
      command: 'npm run dev',
      cwd: '../../frontend',
      port: 5000,
      reuseExistingServer: !process.env.CI,
      timeout: 120000,
    },
    {
      command: 'node wallet_server.js',
      cwd: '../../backend/server',
      port: 4000,
      reuseExistingServer: !process.env.CI,
      timeout: 60000,
    },
  ],

  /* Configure test output */
  outputDir: 'wallet-test-results/',

  /* Configure expect defaults */
  expect: {
    /* Timeout for assertions */
    timeout: 10000,

    /* Screenshots for assertions */
    toHaveScreenshot: { threshold: 0.3 },
    toMatchScreenshot: { threshold: 0.3 },
  },

  /* Test timeout */
  timeout: 60000,
});
