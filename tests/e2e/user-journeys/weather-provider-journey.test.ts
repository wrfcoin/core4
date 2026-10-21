import { test, expect, type Page } from '@playwright/test';
import { USER_JOURNEYS } from './journey-definitions';
import { USER_PERSONAS } from './user-personas';
import { TEST_CONSTANTS } from '../test-data/test-constants';

// Weather Provider Journey Tests
test.describe('Weather Data Provider User Journeys', () => {
  // Test for IoT Newcomer Setup Journey
  test.describe('IoT Newcomer First-Time Setup', () => {
    const journey = USER_JOURNEYS.find(j => j.id === 'iot_newcomer_setup');
    const persona = USER_PERSONAS.find(p => p.id === 'weather_iot_newcomer');

    test.beforeEach(async ({ page }) => {
      // Set up test context for newcomer persona
      await page.goto('/');
      await page.waitForLoadState('networkidle');
    });

    test('should complete full IoT newcomer onboarding flow', async ({
      page,
    }) => {
      // Step 1: Landing Page Visit
      await test.step('Visit WRFCoin Platform', async () => {
        const startTime = Date.now();
        await page.goto('/');
        await expect(page.locator('[data-testid="page-loaded"]')).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(
          journey?.steps[0].performance?.maxLoadTime || 3000
        );

        // Verify landing page has clear call-to-action for weather providers
        await expect(
          page.locator('[data-testid="weather-provider-cta"]')
        ).toBeVisible();
        await expect(
          page.locator('text=Start Earning from Weather Data')
        ).toBeVisible();
      });

      // Step 2: Account Registration
      await test.step('Create User Account', async () => {
        await page.click('[data-testid="get-started-button"]');
        await expect(
          page.locator('[data-testid="registration-form"]')
        ).toBeVisible();

        // Fill registration form
        const testEmail = `newcomer-${Date.now()}@test.wrfcoin.com`;
        await page.fill('[data-testid="email-input"]', testEmail);
        await page.fill('[data-testid="password-input"]', 'SecurePassword123!');
        await page.fill(
          '[data-testid="confirm-password-input"]',
          'SecurePassword123!'
        );
        await page.check('[data-testid="terms-checkbox"]');

        await page.click('[data-testid="register-button"]');

        // In test environment, skip email verification
        if (
          await page.locator('[data-testid="skip-verification"]').isVisible()
        ) {
          await page.click('[data-testid="skip-verification"]');
        }

        await expect(
          page.locator('[data-testid="registration-success"]')
        ).toBeVisible();
      });

      // Step 3: Wallet Setup
      await test.step('Set Up Crypto Wallet', async () => {
        await page.click('[data-testid="setup-wallet-button"]');
        await expect(
          page.locator('[data-testid="wallet-setup-options"]')
        ).toBeVisible();

        // Choose platform wallet for newcomer (easier option)
        await page.click('[data-testid="create-platform-wallet"]');
        await page.fill(
          '[data-testid="wallet-password"]',
          'WalletPassword123!'
        );
        await page.fill(
          '[data-testid="confirm-wallet-password"]',
          'WalletPassword123!'
        );

        await page.click('[data-testid="create-wallet-button"]');

        // Wait for wallet creation and test token allocation
        await expect(
          page.locator('[data-testid="wallet-created-success"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="test-tokens-allocated"]')
        ).toBeVisible();

        // Verify wallet connection
        await expect(
          page.locator('[data-testid="wallet-connected"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="wallet-balance"]')
        ).toContainText('WRF');
      });

      // Step 4: Device Registration
      await test.step('Register Weather Station', async () => {
        await page.click('[data-testid="nav-devices"]');
        await expect(
          page.locator('[data-testid="devices-page-loaded"]')
        ).toBeVisible();

        await page.click('[data-testid="add-device-button"]');
        await expect(
          page.locator('[data-testid="device-registration-form"]')
        ).toBeVisible();

        // Fill device information
        await page.selectOption(
          '[data-testid="device-type-select"]',
          'acurite-atlas'
        );
        await page.fill(
          '[data-testid="device-name-input"]',
          'My First Weather Station'
        );
        await page.fill(
          '[data-testid="device-serial-input"]',
          'TEST-DEVICE-001'
        );

        // Mock device verification for testing
        await page.check('[data-testid="mock-device-verification"]');

        await page.click('[data-testid="register-device-button"]');

        await expect(
          page.locator('[data-testid="device-registered-success"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="setup-instructions"]')
        ).toBeVisible();
      });

      // Step 5: Location Verification
      await test.step('Verify Device Location', async () => {
        await page.click('[data-testid="verify-location-button"]');
        await expect(
          page.locator('[data-testid="location-verification-form"]')
        ).toBeVisible();

        // Use preset test location for consistency
        await page.click('[data-testid="use-test-location"]');
        await page.fill('[data-testid="location-name"]', 'Test Location NYC');

        // Verify location on map
        await expect(
          page.locator('[data-testid="location-map"]')
        ).toBeVisible();
        await page.click('[data-testid="confirm-location-button"]');

        await expect(
          page.locator('[data-testid="location-verified-success"]')
        ).toBeVisible();

        // Verify device appears on network map
        await page.click('[data-testid="view-network-map"]');
        await expect(
          page.locator('[data-testid="device-on-map"]')
        ).toBeVisible();
      });

      // Step 6: Data Flow Test
      await test.step('Test Data Transmission', async () => {
        await page.click('[data-testid="test-data-transmission"]');

        // Simulate data transmission in test environment
        await page.click('[data-testid="simulate-data-transmission"]');

        // Wait for data to appear in dashboard
        await page.waitForSelector('[data-testid="live-weather-data"]', {
          timeout: 10000,
        });
        await expect(
          page.locator('[data-testid="data-quality-score"]')
        ).toBeVisible();

        // Verify data quality metrics
        const qualityScore = await page
          .locator('[data-testid="data-quality-score"]')
          .textContent();
        expect(parseInt(qualityScore?.replace('%', '') || '0')).toBeGreaterThan(
          70
        );
      });

      // Step 7: Reward Setup
      await test.step('Configure Reward Settings', async () => {
        await page.click('[data-testid="configure-rewards"]');
        await expect(
          page.locator('[data-testid="reward-configuration"]')
        ).toBeVisible();

        // Configure automatic rewards
        await page.check('[data-testid="automatic-rewards"]');
        await page.selectOption('[data-testid="payout-frequency"]', 'daily');

        await page.click('[data-testid="save-reward-settings"]');

        await expect(
          page.locator('[data-testid="reward-settings-saved"]')
        ).toBeVisible();

        // Trigger test payout
        await page.click('[data-testid="trigger-test-payout"]');
        await expect(
          page.locator('[data-testid="test-payout-success"]')
        ).toBeVisible();
      });

      // Step 8: First Reward Claim
      await test.step('Claim First Rewards', async () => {
        // In test environment, simulate reward accumulation
        await page.click('[data-testid="simulate-reward-period"]');

        // Check rewards dashboard
        await page.click('[data-testid="nav-rewards"]');
        await expect(
          page.locator('[data-testid="available-rewards"]')
        ).toBeVisible();

        const startTime = Date.now();
        await page.click('[data-testid="claim-rewards-button"]');

        await expect(
          page.locator('[data-testid="reward-claim-success"]')
        ).toBeVisible();

        const claimTime = Date.now() - startTime;
        expect(claimTime).toBeLessThan(5000);

        // Verify reward tokens in wallet
        await expect(
          page.locator('[data-testid="wallet-balance"]')
        ).toContainText('WRF');

        // Check that balance increased
        const finalBalance = await page
          .locator('[data-testid="wrf-balance"]')
          .textContent();
        expect(
          parseFloat(finalBalance?.replace(' WRF', '') || '0')
        ).toBeGreaterThan(0);
      });
    });

    test('should handle registration errors gracefully', async ({ page }) => {
      await page.goto('/register');

      // Test duplicate email scenario
      await page.fill(
        '[data-testid="email-input"]',
        'existing@test.wrfcoin.com'
      );
      await page.fill('[data-testid="password-input"]', 'Password123!');
      await page.fill('[data-testid="confirm-password-input"]', 'Password123!');
      await page.check('[data-testid="terms-checkbox"]');

      await page.click('[data-testid="register-button"]');

      await expect(page.locator('[data-testid="error-message"]')).toContainText(
        'Email already exists'
      );
      await expect(
        page.locator('[data-testid="registration-form"]')
      ).toBeVisible();
    });

    test('should handle wallet connection failures', async ({ page }) => {
      await page.goto('/wallet-setup');

      // Simulate wallet connection failure
      await page.route('**/api/wallet/connect', route => route.abort());

      await page.click('[data-testid="connect-metamask"]');

      await expect(
        page.locator('[data-testid="wallet-connection-error"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="try-again-button"]')
      ).toBeVisible();

      // Verify fallback options are available
      await expect(
        page.locator('[data-testid="create-platform-wallet"]')
      ).toBeVisible();
    });

    test('should handle device registration edge cases', async ({ page }) => {
      await page.goto('/devices/add');

      // Test unsupported device type
      await page.fill(
        '[data-testid="device-serial-input"]',
        'UNSUPPORTED-DEVICE-001'
      );
      await page.selectOption('[data-testid="device-type-select"]', 'custom');

      await page.click('[data-testid="register-device-button"]');

      await expect(
        page.locator('[data-testid="device-verification-required"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="manual-verification-form"]')
      ).toBeVisible();
    });
  });

  // Test for Professional Meteorologist Journey
  test.describe('Professional Meteorologist Multi-Station Integration', () => {
    const journey = USER_JOURNEYS.find(
      j => j.id === 'meteorologist_advanced_setup'
    );

    test.beforeEach(async ({ page }) => {
      // Set up admin/professional test context
      await page.goto('/');
      await page.evaluate(() => {
        localStorage.setItem('test-user-type', 'professional');
        localStorage.setItem('api-access-level', 'enterprise');
      });
    });

    test('should complete professional meteorologist setup flow', async ({
      page,
    }) => {
      // Step 1: API Access Request
      await test.step('Request Enterprise API Access', async () => {
        await page.goto('/api-access');

        await page.fill(
          '[data-testid="organization-name"]',
          'Test Weather Research Institute'
        );
        await page.fill(
          '[data-testid="api-use-case"]',
          'Professional weather monitoring and research'
        );
        await page.selectOption('[data-testid="access-level"]', 'enterprise');

        await page.click('[data-testid="submit-api-request"]');

        // In test environment, auto-approve
        await page.click('[data-testid="auto-approve-test"]');

        await expect(
          page.locator('[data-testid="api-access-approved"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="api-key-generated"]')
        ).toBeVisible();
      });

      // Step 2: Bulk Device Import
      await test.step('Import Multiple Weather Stations', async () => {
        await page.goto('/devices/bulk-import');

        // Upload test CSV file
        const testCSV = `device_id,device_type,location_lat,location_lon,location_name
TEST-STATION-001,professional_aws,40.7128,-74.0060,NYC Station 1
TEST-STATION-002,professional_aws,40.7589,-73.9851,NYC Station 2
TEST-STATION-003,professional_aws,40.6892,-74.0445,NYC Station 3`;

        await page.setInputFiles('[data-testid="csv-upload"]', {
          name: 'test-stations.csv',
          mimeType: 'text/csv',
          buffer: Buffer.from(testCSV),
        });

        await page.click('[data-testid="upload-csv-button"]');

        await expect(
          page.locator('[data-testid="bulk-import-success"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="devices-imported-count"]')
        ).toContainText('3');

        // Verify all devices are listed
        await page.click('[data-testid="view-imported-devices"]');
        await expect(
          page.locator('[data-testid="device-TEST-STATION-001"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="device-TEST-STATION-002"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="device-TEST-STATION-003"]')
        ).toBeVisible();
      });

      // Step 3: Advanced Quality Setup
      await test.step('Configure Quality Assurance Parameters', async () => {
        await page.goto('/quality-assurance');

        // Configure custom QA rules
        await page.fill('[data-testid="temperature-tolerance"]', '2.0');
        await page.fill('[data-testid="humidity-tolerance"]', '5.0');
        await page.fill('[data-testid="pressure-tolerance"]', '1.0');

        // Set up alert thresholds
        await page.check('[data-testid="enable-quality-alerts"]');
        await page.fill(
          '[data-testid="alert-email"]',
          'meteorologist@test.wrfcoin.com'
        );
        await page.selectOption('[data-testid="alert-frequency"]', 'immediate');

        await page.click('[data-testid="save-qa-settings"]');

        await expect(
          page.locator('[data-testid="qa-settings-saved"]')
        ).toBeVisible();

        // Test alert system
        await page.click('[data-testid="test-alert-system"]');
        await expect(
          page.locator('[data-testid="test-alert-sent"]')
        ).toBeVisible();
      });

      // Step 4: API Integration Test
      await test.step('Test API Data Integration', async () => {
        await page.goto('/api-integration');

        // Display API documentation and test interface
        await expect(
          page.locator('[data-testid="api-documentation"]')
        ).toBeVisible();

        // Test API with sample data
        await page.click('[data-testid="test-api-integration"]');

        const testData = {
          device_id: 'TEST-STATION-001',
          timestamp: new Date().toISOString(),
          temperature: 22.5,
          humidity: 65,
          pressure: 1013.25,
          wind_speed: 12.3,
          wind_direction: 180,
        };

        await page.fill(
          '[data-testid="api-test-data"]',
          JSON.stringify(testData, null, 2)
        );
        await page.click('[data-testid="submit-test-data"]');

        await expect(
          page.locator('[data-testid="api-test-success"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="data-processing-confirmed"]')
        ).toBeVisible();

        // Verify data appears in dashboard
        await page.goto('/dashboard');
        await expect(
          page.locator('[data-testid="recent-data-TEST-STATION-001"]')
        ).toBeVisible();
      });

      // Step 5: Analytics Dashboard Setup
      await test.step('Configure Advanced Analytics', async () => {
        await page.goto('/analytics/dashboard');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="analytics-dashboard"]')
        ).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(4000);

        // Create custom dashboard
        await page.click('[data-testid="create-custom-dashboard"]');

        // Add performance metrics widgets
        await page.click('[data-testid="add-widget-button"]');
        await page.selectOption(
          '[data-testid="widget-type"]',
          'data-quality-trends'
        );
        await page.click('[data-testid="add-widget"]');

        await page.click('[data-testid="add-widget-button"]');
        await page.selectOption(
          '[data-testid="widget-type"]',
          'device-performance'
        );
        await page.click('[data-testid="add-widget"]');

        await page.click('[data-testid="add-widget-button"]');
        await page.selectOption(
          '[data-testid="widget-type"]',
          'reward-analytics'
        );
        await page.click('[data-testid="add-widget"]');

        await page.click('[data-testid="save-dashboard"]');

        await expect(
          page.locator('[data-testid="dashboard-saved-success"]')
        ).toBeVisible();

        // Verify all widgets are displayed
        await expect(
          page.locator('[data-testid="widget-data-quality-trends"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="widget-device-performance"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="widget-reward-analytics"]')
        ).toBeVisible();
      });
    });

    test('should handle API rate limiting gracefully', async ({ page }) => {
      await page.goto('/api-integration');

      // Simulate rate limit scenario
      await page.route('**/api/weather-data', route => {
        route.fulfill({
          status: 429,
          headers: {
            'X-RateLimit-Limit': '1000',
            'X-RateLimit-Remaining': '0',
            'X-RateLimit-Reset': String(Math.floor(Date.now() / 1000) + 3600),
          },
          body: JSON.stringify({ error: 'Rate limit exceeded' }),
        });
      });

      await page.click('[data-testid="test-api-integration"]');

      await expect(
        page.locator('[data-testid="rate-limit-warning"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="rate-limit-reset-time"]')
      ).toBeVisible();
    });

    test('should validate CSV import data format', async ({ page }) => {
      await page.goto('/devices/bulk-import');

      // Upload invalid CSV
      const invalidCSV = `invalid,header,format
invalid,data,format`;

      await page.setInputFiles('[data-testid="csv-upload"]', {
        name: 'invalid.csv',
        mimeType: 'text/csv',
        buffer: Buffer.from(invalidCSV),
      });

      await page.click('[data-testid="upload-csv-button"]');

      await expect(
        page.locator('[data-testid="csv-format-error"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="csv-format-help"]')
      ).toBeVisible();
    });
  });
});
