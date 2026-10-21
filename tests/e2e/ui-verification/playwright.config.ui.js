/**
 * Playwright UI Verification Configuration
 *
 * Extended configuration for comprehensive UI/UX testing with:
 * - Screenshots on every test
 * - Full-page traces
 * - Multiple viewports (desktop, tablet, mobile)
 * - Accessibility auditing
 * - Output to gitignored directories
 */
const { defineConfig, devices } = require('@playwright/test');
const path = require('path');

// Base config for shared settings
const baseConfig = require('../config/test-config');

// Output directories (all gitignored)
const OUTPUT_BASE = path.join(__dirname);
const SCREENSHOT_DIR = path.join(OUTPUT_BASE, 'screenshots');
const TRACE_DIR = path.join(OUTPUT_BASE, 'traces');
const REPORT_DIR = path.join(OUTPUT_BASE, 'reports');
const ACCESSIBILITY_DIR = path.join(OUTPUT_BASE, 'accessibility');

module.exports = defineConfig({
  testDir: './specs',

  /* Run tests in files in parallel */
  fullyParallel: false, // Sequential for UI verification to capture consistent state

  /* Fail the build on CI if you accidentally left test.only in the source code */
  forbidOnly: !!process.env.CI,

  /* Retry on CI only */
  retries: process.env.CI ? 1 : 0,

  /* Limit parallel workers for UI verification */
  workers: process.env.CI ? 1 : 2,

  /* Reporter configuration */
  reporter: [
    ['html', { outputFolder: path.join(REPORT_DIR, 'html-report'), open: 'never' }],
    ['json', { outputFile: path.join(REPORT_DIR, 'ui-verification-results.json') }],
    ['list'],
  ],

  /* Shared settings for all projects */
  use: {
    /* Base URL - frontend dev server */
    baseURL: process.env.UI_TEST_URL || 'http://localhost:5000',

    /* Always capture trace for debugging */
    trace: 'on',

    /* Screenshot on every test for visual verification */
    screenshot: 'on',

    /* Video recording for all tests */
    video: 'on',

    /* Global timeout for each action */
    actionTimeout: 15000,

    /* Global timeout for navigation */
    navigationTimeout: 30000,

    /* Collect accessibility violations */
    ignoreHTTPSErrors: true,
  },

  /* Global setup and teardown */
  globalSetup: require.resolve('./utils/ui-global-setup.js'),
  globalTeardown: require.resolve('./utils/ui-global-teardown.js'),

  /* Configure projects for different viewports */
  projects: [
    // Desktop viewports
    {
      name: 'desktop-chrome',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1920, height: 1080 },
      },
    },
    {
      name: 'desktop-firefox',
      use: {
        ...devices['Desktop Firefox'],
        viewport: { width: 1440, height: 900 },
      },
    },

    // Tablet viewport
    {
      name: 'tablet-ipad',
      use: {
        ...devices['iPad Pro'],
        viewport: { width: 768, height: 1024 },
      },
    },

    // Mobile viewports
    {
      name: 'mobile-iphone',
      use: {
        ...devices['iPhone 13'],
        viewport: { width: 390, height: 844 },
      },
    },
    {
      name: 'mobile-android',
      use: {
        ...devices['Pixel 5'],
        viewport: { width: 393, height: 851 },
      },
    },
  ],

  /* Test timeout */
  timeout: 60000,

  /* Expect timeout */
  expect: {
    timeout: 10000,
    /* Threshold for visual comparisons */
    toHaveScreenshot: {
      threshold: 0.2,
      maxDiffPixelRatio: 0.1,
    },
  },

  /* Output directories */
  outputDir: path.join(REPORT_DIR, 'test-artifacts'),

  /* Metadata */
  metadata: {
    'test-suite': 'WRFCoin UI/UX Verification',
    version: '1.0.0',
    environment: process.env.E2E_ENV || 'local',
  },
});
