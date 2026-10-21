const { defineConfig, devices } = require('@playwright/test');

module.exports = defineConfig({
  testDir: './consolidated-tests/e2e',
  testMatch: ['**/consolidated-tests/e2e/**/*.test.js'],
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'html',
  use: {
    baseURL: 'http://localhost:5001',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },

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
  ],

  webServer: {
    command: 'cd frontend && npm run dev -- --port 5001',
    url: 'http://localhost:5001',
    reuseExistingServer: !process.env.CI,
  },
});
