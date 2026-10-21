const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('Weather Integration Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('weather-provider');
    await helpers.connectWallet('weather-provider');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Connect weather station via API integration', async ({ page }) => {
    // Navigate to weather integration section
    await page.click('[data-testid="nav-weather"]');
    await page.waitForURL('**/weather');

    // Verify weather dashboard
    await expect(
      page.locator('[data-testid="weather-dashboard"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="connected-stations"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="data-streams"]')).toBeVisible();

    // Add new weather station
    await page.click('[data-testid="add-station-button"]');
    await page.waitForURL('**/weather/connect');

    // Select weather station type
    await expect(page.locator('[data-testid="station-types"]')).toBeVisible();
    await page.click('[data-testid="station-type-ambient-weather"]');

    // Fill connection details
    const stationData = {
      name: 'North Campus Weather Station',
      location: 'New York, NY',
      latitude: '40.7128',
      longitude: '-74.0060',
      apiKey: 'ambient-api-key-12345',
      applicationKey: 'ambient-app-key-67890',
      macAddress: '00:0E:C6:23:45:67',
    };

    await helpers.fillForm({
      'station-name': stationData.name,
      'station-location': stationData.location,
      latitude: stationData.latitude,
      longitude: stationData.longitude,
      'api-key': stationData.apiKey,
      'application-key': stationData.applicationKey,
      'mac-address': stationData.macAddress,
    });

    // Configure data collection settings
    await page.check('[data-testid="collect-temperature"]');
    await page.check('[data-testid="collect-humidity"]');
    await page.check('[data-testid="collect-pressure"]');
    await page.check('[data-testid="collect-wind-speed"]');
    await page.check('[data-testid="collect-rainfall"]');

    // Set collection frequency
    await page.selectOption('[data-testid="collection-frequency"]', '300'); // 5 minutes

    // Test connection
    await page.click('[data-testid="test-connection-button"]');
    await page.waitForSelector('[data-testid="connection-test-result"]');

    const testResult = await page.textContent(
      '[data-testid="connection-test-result"]'
    );
    expect(testResult).toContain('successful');

    // Verify sample data preview
    await expect(
      page.locator('[data-testid="sample-data-preview"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="temperature-reading"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="humidity-reading"]')
    ).toBeVisible();

    // Save weather station configuration
    await page.click('[data-testid="save-station-button"]');

    // Authenticate
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'ProviderPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');

    // Verify station appears in dashboard
    await page.waitForURL('**/weather');
    await expect(page.locator('[data-testid="station-card"]')).toBeVisible();
    await expect(page.locator('[data-testid="station-name"]')).toContainText(
      stationData.name
    );
    await expect(page.locator('[data-testid="station-status"]')).toContainText(
      'Connected'
    );
  });

  test('Monitor real-time weather data streams', async ({ page }) => {
    await page.click('[data-testid="nav-weather"]');

    // Select connected weather station
    await page.click('[data-testid="station-card"]').first();
    await page.waitForURL('**/weather/station/**');

    // Verify real-time data interface
    await expect(page.locator('[data-testid="real-time-data"]')).toBeVisible();
    await expect(page.locator('[data-testid="data-charts"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="current-readings"]')
    ).toBeVisible();

    // Check current weather readings
    await expect(
      page.locator('[data-testid="current-temperature"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="current-humidity"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="current-pressure"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="current-wind-speed"]')
    ).toBeVisible();

    // Verify data format and ranges
    const temperature = await page.textContent(
      '[data-testid="current-temperature"]'
    );
    expect(temperature).toMatch(/^-?\d+(\.\d+)?°[CF]$/);

    const humidity = await page.textContent('[data-testid="current-humidity"]');
    expect(humidity).toMatch(/^\d+(\.\d+)?%$/);

    // Test real-time updates
    const initialTimestamp = await page.textContent(
      '[data-testid="last-update-timestamp"]'
    );

    // Wait for data refresh (typically 5 minutes, but test environment may be faster)
    await page.waitForTimeout(10000);
    await page.reload();

    const updatedTimestamp = await page.textContent(
      '[data-testid="last-update-timestamp"]'
    );
    expect(updatedTimestamp).not.toBe(initialTimestamp);

    // Test historical data charts
    await page.click('[data-testid="chart-24h"]');
    await expect(
      page.locator('[data-testid="temperature-chart"]')
    ).toBeVisible();

    await page.click('[data-testid="chart-7d"]');
    await page.waitForTimeout(2000);

    await page.click('[data-testid="chart-30d"]');
    await page.waitForTimeout(2000);

    // Test data export
    await page.click('[data-testid="export-data-button"]');

    const modal = await helpers.waitForModal();

    await page.selectOption('[data-testid="export-format"]', 'csv');
    await page.selectOption('[data-testid="export-timerange"]', '7d');
    await page.click('[data-testid="export-confirm-button"]');

    await helpers.waitForNotification('success');
    await modal.close();
  });

  test('Validate weather data quality and accuracy', async ({ page }) => {
    await page.click('[data-testid="nav-weather"]');
    await page.click('[data-testid="data-quality-tab"]');

    // Verify data quality dashboard
    await expect(
      page.locator('[data-testid="data-quality-overview"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="quality-score"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="validation-status"]')
    ).toBeVisible();

    // Check quality metrics
    const qualityScore = await page.textContent(
      '[data-testid="quality-score"]'
    );
    expect(qualityScore).toMatch(/^\d+(\.\d+)?%$/);

    // Verify validation checks
    await expect(
      page.locator('[data-testid="range-validation"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="consistency-check"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="anomaly-detection"]')
    ).toBeVisible();

    // Test anomaly detection
    await page.click('[data-testid="anomaly-detection-tab"]');

    if (await page.locator('[data-testid="detected-anomaly"]').isVisible()) {
      // Review detected anomalies
      const anomalies = page.locator('[data-testid="anomaly-item"]');

      for (let i = 0; i < Math.min(3, await anomalies.count()); i++) {
        const anomaly = anomalies.nth(i);
        await expect(
          anomaly.locator('[data-testid="anomaly-type"]')
        ).toBeVisible();
        await expect(
          anomaly.locator('[data-testid="anomaly-timestamp"]')
        ).toBeVisible();
        await expect(
          anomaly.locator('[data-testid="anomaly-severity"]')
        ).toBeVisible();
      }
    }

    // Test data correction workflow
    if (await page.locator('[data-testid="flagged-data-point"]').isVisible()) {
      await page.click('[data-testid="flagged-data-point"]').first();

      const correctionModal = await helpers.waitForModal();

      // Review flagged data
      await expect(
        page.locator('[data-testid="original-value"]')
      ).toBeVisible();
      await expect(page.locator('[data-testid="flag-reason"]')).toBeVisible();

      // Apply correction if needed
      await page.fill('[data-testid="corrected-value-input"]', '25.5');
      await page.fill(
        '[data-testid="correction-reason"]',
        'Sensor calibration adjustment'
      );

      await page.click('[data-testid="apply-correction-button"]');
      await helpers.waitForNotification('success');
      await correctionModal.close();
    }

    // Configure validation rules
    await page.click('[data-testid="validation-rules-tab"]');

    await expect(
      page.locator('[data-testid="temperature-range-rule"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="humidity-range-rule"]')
    ).toBeVisible();

    // Test custom validation rule
    await page.click('[data-testid="add-validation-rule-button"]');

    const ruleModal = await helpers.waitForModal();

    await page.selectOption('[data-testid="rule-parameter"]', 'wind-speed');
    await page.selectOption('[data-testid="rule-type"]', 'range-check');
    await page.fill('[data-testid="min-value"]', '0');
    await page.fill('[data-testid="max-value"]', '200');

    await page.click('[data-testid="save-validation-rule"]');
    await helpers.waitForNotification('success');
    await ruleModal.close();
  });

  test('Configure weather data monetization', async ({ page }) => {
    await page.click('[data-testid="nav-weather"]');
    await page.click('[data-testid="monetization-tab"]');

    // Verify monetization dashboard
    await expect(
      page.locator('[data-testid="monetization-overview"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="earnings-summary"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="data-pricing"]')).toBeVisible();

    // Configure data pricing
    await page.click('[data-testid="configure-pricing-button"]');

    const modal = await helpers.waitForModal();

    // Set pricing for different data types
    await helpers.fillForm({
      'temperature-price': '0.01',
      'humidity-price': '0.01',
      'pressure-price': '0.015',
      'wind-speed-price': '0.02',
      'rainfall-price': '0.025',
    });

    // Set pricing tiers
    await page.selectOption('[data-testid="pricing-model"]', 'tiered');

    await helpers.fillForm({
      'tier1-requests': '1000',
      'tier1-price': '0.01',
      'tier2-requests': '10000',
      'tier2-price': '0.008',
      'tier3-requests': 'unlimited',
      'tier3-price': '0.005',
    });

    // Configure access permissions
    await page.check('[data-testid="allow-historical-access"]');
    await page.check('[data-testid="allow-real-time-access"]');
    await page.check('[data-testid="allow-bulk-downloads"]');

    // Set data sharing agreements
    await page.selectOption('[data-testid="license-type"]', 'commercial');
    await page.fill(
      '[data-testid="attribution-requirement"]',
      'Weather data provided by North Campus Weather Station'
    );

    // Save pricing configuration
    await page.click('[data-testid="save-pricing-config"]');

    // Authenticate
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'ProviderPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Review earnings dashboard
    await expect(page.locator('[data-testid="total-earnings"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="data-requests-count"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="revenue-chart"]')).toBeVisible();

    // Test marketplace listing
    await page.click('[data-testid="list-on-marketplace-button"]');

    const listingModal = await helpers.waitForModal();

    await page.fill(
      '[data-testid="marketplace-title"]',
      'High-Quality NYC Weather Data'
    );
    await page.fill(
      '[data-testid="marketplace-description"]',
      'Real-time and historical weather data from Manhattan with 99.9% uptime'
    );

    await page.check('[data-testid="feature-real-time"]');
    await page.check('[data-testid="feature-historical"]');
    await page.check('[data-testid="feature-api-access"]');

    await page.click('[data-testid="create-marketplace-listing"]');
    await helpers.waitForNotification('success');
    await listingModal.close();
  });

  test('Manage weather data access and subscriptions', async ({ page }) => {
    await page.click('[data-testid="nav-weather"]');
    await page.click('[data-testid="subscriptions-tab"]');

    // Verify subscription management interface
    await expect(
      page.locator('[data-testid="active-subscriptions"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="subscription-requests"]')
    ).toBeVisible();

    // Review subscription requests
    if (
      await page.locator('[data-testid="subscription-request"]').isVisible()
    ) {
      await page.click('[data-testid="subscription-request"]').first();

      const modal = await helpers.waitForModal();

      // Review request details
      await expect(
        page.locator('[data-testid="subscriber-info"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="requested-data-types"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="usage-requirements"]')
      ).toBeVisible();

      // Approve subscription
      await page.click('[data-testid="approve-subscription-button"]');

      // Set custom terms if needed
      await page.fill('[data-testid="custom-rate-limit"]', '1000');
      await page.selectOption('[data-testid="subscription-duration"]', '12'); // 12 months

      await page.click('[data-testid="confirm-approval-button"]');
      await helpers.waitForNotification('success');
      await modal.close();
    }

    // Monitor active subscriptions
    const subscriptions = page.locator('[data-testid="active-subscription"]');

    if ((await subscriptions.count()) > 0) {
      // Check subscription details
      await subscriptions.first().click();

      await expect(
        page.locator('[data-testid="subscription-usage"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="data-consumption"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="revenue-generated"]')
      ).toBeVisible();

      // Test usage analytics
      await page.click('[data-testid="usage-analytics-tab"]');
      await expect(page.locator('[data-testid="usage-chart"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="peak-usage-times"]')
      ).toBeVisible();
    }

    // Configure subscription alerts
    await page.click('[data-testid="subscription-alerts-tab"]');

    await page.check('[data-testid="alert-high-usage"]');
    await page.check('[data-testid="alert-payment-due"]');
    await page.check('[data-testid="alert-subscription-expiry"]');

    await page.fill('[data-testid="usage-threshold"]', '80'); // 80% of limit

    await page.click('[data-testid="save-alert-settings"]');
    await helpers.waitForNotification('success');
  });

  test('IoT device direct integration', async ({ page }) => {
    await page.click('[data-testid="nav-weather"]');
    await page.click('[data-testid="iot-devices-tab"]');

    // Add IoT device directly
    await page.click('[data-testid="add-iot-device-button"]');

    const modal = await helpers.waitForModal();

    // Select device type
    await page.click('[data-testid="device-type-raspberry-pi"]');

    // Configure device connection
    await helpers.fillForm({
      'device-name': 'Rooftop Weather Sensor',
      'device-id': 'rpi-weather-001',
      'mqtt-broker': 'mqtt.wrfcoin.io',
      'mqtt-topic': 'weather/rpi-001/data',
      'api-endpoint': 'https://api.wrfcoin.io/weather/ingest',
      'api-key': 'iot-api-key-98765',
    });

    // Configure sensor mapping
    await page.click('[data-testid="add-sensor-mapping"]');
    await helpers.fillForm({
      'sensor-type-0': 'temperature',
      'sensor-pin-0': 'GPIO18',
      'sensor-model-0': 'DHT22',
    });

    await page.click('[data-testid="add-sensor-mapping"]');
    await helpers.fillForm({
      'sensor-type-1': 'humidity',
      'sensor-pin-1': 'GPIO18',
      'sensor-model-1': 'DHT22',
    });

    // Test device connection
    await page.click('[data-testid="test-iot-connection"]');
    await page.waitForSelector('[data-testid="connection-status"]');

    const connectionStatus = await page.textContent(
      '[data-testid="connection-status"]'
    );
    expect(connectionStatus).toContain('connected');

    // Save IoT device configuration
    await page.click('[data-testid="save-iot-device"]');
    await helpers.waitForNotification('success');
    await modal.close();

    // Verify device appears in list
    await expect(page.locator('[data-testid="iot-device-card"]')).toBeVisible();
    await expect(page.locator('[data-testid="device-status"]')).toContainText(
      'Online'
    );

    // Test device data flow
    await page.click('[data-testid="iot-device-card"]').first();
    await expect(
      page.locator('[data-testid="device-data-stream"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="raw-sensor-data"]')).toBeVisible();
  });

  test('Weather data marketplace integration', async ({ page }) => {
    // Switch to data consumer perspective
    await helpers.loginAs('user1');
    await helpers.connectWallet('user1');

    await page.click('[data-testid="nav-weather"]');
    await page.click('[data-testid="marketplace-tab"]');

    // Browse weather data marketplace
    await expect(
      page.locator('[data-testid="weather-marketplace"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="data-provider-list"]')
    ).toBeVisible();

    // Filter providers by location
    await page.fill('[data-testid="location-search"]', 'New York');
    await page.click('[data-testid="search-providers-button"]');

    // Select a data provider
    await page.click('[data-testid="provider-card"]').first();

    // Review provider details
    await expect(page.locator('[data-testid="provider-info"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="data-quality-rating"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="pricing-info"]')).toBeVisible();
    await expect(page.locator('[data-testid="sample-data"]')).toBeVisible();

    // Subscribe to data feed
    await page.click('[data-testid="subscribe-button"]');

    const modal = await helpers.waitForModal();

    // Select subscription plan
    await page.click('[data-testid="plan-premium"]');

    // Configure subscription
    await page.check('[data-testid="data-type-temperature"]');
    await page.check('[data-testid="data-type-humidity"]');
    await page.check('[data-testid="data-type-pressure"]');

    await page.selectOption('[data-testid="update-frequency"]', '300'); // 5 minutes
    await page.selectOption('[data-testid="subscription-duration"]', '6'); // 6 months

    // Review subscription cost
    await expect(
      page.locator('[data-testid="subscription-cost"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="monthly-cost"]')).toBeVisible();

    // Confirm subscription
    await page.click('[data-testid="confirm-subscription-button"]');

    // Authenticate payment
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify subscription is active
    await page.click('[data-testid="my-subscriptions-tab"]');
    await expect(
      page.locator('[data-testid="active-subscription"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="subscription-status"]')
    ).toContainText('Active');
  });

  test('Weather integration error handling', async ({ page }) => {
    await page.click('[data-testid="nav-weather"]');

    // Test API connection failure
    await page.click('[data-testid="add-station-button"]');
    await page.click('[data-testid="station-type-ambient-weather"]');

    // Enter invalid API credentials
    await helpers.fillForm({
      'station-name': 'Test Station',
      'api-key': 'invalid-api-key',
      'application-key': 'invalid-app-key',
    });

    await page.click('[data-testid="test-connection-button"]');
    await expect(
      page.locator('[data-testid="connection-error"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="error-message"]')).toContainText(
      'authentication failed'
    );

    // Test network timeout
    await page.route('**/api/weather/**', route => route.abort());
    await page.reload();
    await expect(
      page.locator('[data-testid="weather-connection-error"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retry-connection-button"]')
    ).toBeVisible();

    // Test data validation errors
    await page.unroute('**/api/weather/**');
    if (await page.locator('[data-testid="station-card"]').isVisible()) {
      await page.click('[data-testid="station-card"]').first();

      // Simulate invalid data range
      await page.click('[data-testid="data-quality-tab"]');
      if (
        await page.locator('[data-testid="data-validation-error"]').isVisible()
      ) {
        await expect(
          page.locator('[data-testid="validation-error-message"]')
        ).toBeVisible();
      }
    }
  });

  test('Weather integration accessibility and mobile responsiveness', async ({
    page,
  }) => {
    // Test keyboard navigation
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter');

    // Check accessibility violations
    const violations = await helpers.checkAccessibility();
    expect(violations).toHaveLength(0);

    // Test mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });

    await page.click('[data-testid="nav-weather"]');

    // Mobile-specific elements should be visible
    await expect(
      page.locator('[data-testid="mobile-weather-nav"]')
    ).toBeVisible();

    // Test mobile data charts
    if (await page.locator('[data-testid="station-card"]').isVisible()) {
      await page.click('[data-testid="station-card"]').first();
      await expect(
        page.locator('[data-testid="mobile-chart-container"]')
      ).toBeVisible();
    }
  });
});
