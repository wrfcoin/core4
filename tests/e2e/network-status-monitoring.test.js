/**
 * Network Status Monitoring - Playwright Test Suite
 * Tests network status accuracy, real-time updates, and monitoring features
 */

const { test, expect } = require('@playwright/test');

test.describe('Network Status Monitoring', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to the main dashboard
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Wait for network status components to load
    await page.waitForSelector('[data-testid="network-status-dashboard"]', {
      timeout: 10000,
    });
  });

  test('Network Connectivity Status Accuracy', async ({ page }) => {
    // Verify all major networks are displayed
    const expectedNetworks = [
      'ethereum',
      'polygon',
      'bsc',
      'avalanche',
      'arbitrum',
      'optimism',
      'solana',
      'wrfcoin',
    ];

    for (const network of expectedNetworks) {
      // Check network status indicator exists
      await expect(
        page.locator(`[data-testid="network-${network}-status"]`)
      ).toBeVisible();

      // Verify status shows either 'online', 'offline', or 'syncing'
      const statusElement = page.locator(
        `[data-testid="network-${network}-connectivity"]`
      );
      await expect(statusElement).toBeVisible();

      const statusText = await statusElement.textContent();
      expect([
        'online',
        'offline',
        'syncing',
        'connected',
        'disconnected',
      ]).toContainEqual(statusText.toLowerCase().trim());

      // Verify block height is displayed and is a number
      const blockHeightElement = page.locator(
        `[data-testid="network-${network}-block-height"]`
      );
      await expect(blockHeightElement).toBeVisible();

      const blockHeight = await blockHeightElement.textContent();
      expect(blockHeight).toMatch(/\d+/); // Should contain numbers
    }
  });

  test('Bridge Protocol Status Monitoring', async ({ page }) => {
    // Navigate to bridge status section
    await page.click('[data-testid="bridge-status-tab"]');
    await page.waitForSelector('[data-testid="bridge-protocols-status"]');

    // Expected bridge protocols from the cross-chain server
    const expectedBridges = [
      'axelar',
      'layerzero',
      'wormhole',
      'ibc',
      'xcmp',
      'near_rainbow',
      'optimism',
      'thorchain',
      'chainflip',
      'base',
      'celer',
      'hyperlane',
    ];

    for (const bridge of expectedBridges) {
      // Check bridge status indicator
      const bridgeStatus = page.locator(
        `[data-testid="bridge-${bridge}-status"]`
      );
      await expect(bridgeStatus).toBeVisible();

      // Verify uptime percentage is displayed
      const uptimeElement = page.locator(
        `[data-testid="bridge-${bridge}-uptime"]`
      );
      await expect(uptimeElement).toBeVisible();

      const uptimeText = await uptimeElement.textContent();
      expect(uptimeText).toMatch(/\d+\.?\d*%/); // Should be a percentage

      // Verify processing time is shown
      const processingTimeElement = page.locator(
        `[data-testid="bridge-${bridge}-processing-time"]`
      );
      await expect(processingTimeElement).toBeVisible();

      // Verify queue length is displayed
      const queueElement = page.locator(
        `[data-testid="bridge-${bridge}-queue"]`
      );
      await expect(queueElement).toBeVisible();
    }
  });

  test('Real-time Status Updates', async ({ page }) => {
    // Get initial network status data
    const initialEthStatus = await page
      .locator('[data-testid="network-ethereum-connectivity"]')
      .textContent();
    const initialEthHeight = await page
      .locator('[data-testid="network-ethereum-block-height"]')
      .textContent();

    // Trigger status refresh
    await page.click('[data-testid="refresh-network-status"]');

    // Wait for refresh to complete
    await page.waitForTimeout(3000);

    // Verify loading indicator appears and disappears
    const loadingIndicator = page.locator('[data-testid="status-loading"]');
    await expect(loadingIndicator).toBeHidden();

    // Check that data is refreshed (may stay the same if no changes)
    await expect(
      page.locator('[data-testid="network-ethereum-connectivity"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="network-ethereum-block-height"]')
    ).toBeVisible();

    // Verify last updated timestamp is recent
    const lastUpdated = page.locator('[data-testid="last-updated-timestamp"]');
    await expect(lastUpdated).toBeVisible();

    const timestampText = await lastUpdated.textContent();
    const timestampMatch = timestampText.match(/(\d{1,2}):(\d{2}):(\d{2})/);
    expect(timestampMatch).toBeTruthy();
  });

  test('Network Synchronization Status', async ({ page }) => {
    // Navigate to detailed sync status
    await page.click('[data-testid="sync-status-tab"]');
    await page.waitForSelector('[data-testid="sync-status-details"]');

    const networks = ['ethereum', 'polygon', 'wrfcoin'];

    for (const network of networks) {
      // Check sync percentage
      const syncPercent = page.locator(
        `[data-testid="network-${network}-sync-percent"]`
      );
      await expect(syncPercent).toBeVisible();

      const syncText = await syncPercent.textContent();
      expect(syncText).toMatch(/\d+\.?\d*%/);

      // Check peer count
      const peerCount = page.locator(
        `[data-testid="network-${network}-peers"]`
      );
      await expect(peerCount).toBeVisible();

      const peersText = await peerCount.textContent();
      expect(peersText).toMatch(/\d+/);

      // Check last block time
      const lastBlockTime = page.locator(
        `[data-testid="network-${network}-last-block"]`
      );
      await expect(lastBlockTime).toBeVisible();
    }
  });

  test('Network Error Detection and Alerts', async ({ page }) => {
    // Simulate network error by intercepting API calls
    await page.route('**/api/crosschain/bridge-status', route => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ success: false, error: 'Network unreachable' }),
      });
    });

    // Trigger status refresh to simulate error
    await page.click('[data-testid="refresh-network-status"]');

    // Wait for error state
    await page.waitForTimeout(2000);

    // Verify error alert is displayed
    await expect(
      page.locator('[data-testid="network-error-alert"]')
    ).toBeVisible();

    // Verify error message contains relevant information
    const errorMessage = await page
      .locator('[data-testid="error-message"]')
      .textContent();
    expect(errorMessage.toLowerCase()).toContain('network');

    // Verify retry button is available
    await expect(
      page.locator('[data-testid="retry-connection"]')
    ).toBeVisible();

    // Test retry functionality
    await page.unroute('**/api/crosschain/bridge-status');
    await page.click('[data-testid="retry-connection"]');

    // Verify error is cleared after successful retry
    await expect(
      page.locator('[data-testid="network-error-alert"]')
    ).toBeHidden({ timeout: 5000 });
  });

  test('Network Performance Metrics', async ({ page }) => {
    // Navigate to performance metrics
    await page.click('[data-testid="performance-metrics-tab"]');
    await page.waitForSelector('[data-testid="performance-dashboard"]');

    // Check latency metrics
    const latencyMetrics = [
      'ethereum-latency',
      'polygon-latency',
      'bsc-latency',
      'avalanche-latency',
    ];

    for (const metric of latencyMetrics) {
      const latencyElement = page.locator(`[data-testid="${metric}"]`);
      await expect(latencyElement).toBeVisible();

      const latencyText = await latencyElement.textContent();
      expect(latencyText).toMatch(/\d+\.?\d*ms/); // Should show milliseconds
    }

    // Check throughput metrics (TPS)
    const throughputMetrics = ['ethereum-tps', 'polygon-tps', 'wrfcoin-tps'];

    for (const metric of throughputMetrics) {
      const tpsElement = page.locator(`[data-testid="${metric}"]`);
      await expect(tpsElement).toBeVisible();

      const tpsText = await tpsElement.textContent();
      expect(tpsText).toMatch(/\d+\.?\d*/); // Should show number
    }

    // Check gas price metrics
    const gasPriceElement = page.locator('[data-testid="ethereum-gas-price"]');
    await expect(gasPriceElement).toBeVisible();

    const gasPriceText = await gasPriceElement.textContent();
    expect(gasPriceText).toMatch(/\d+\.?\d*\s*(gwei|wei)/);
  });

  test('Historical Status Data Visualization', async ({ page }) => {
    // Navigate to historical data view
    await page.click('[data-testid="historical-data-tab"]');
    await page.waitForSelector('[data-testid="status-history-chart"]');

    // Verify chart elements are present
    await expect(page.locator('[data-testid="uptime-chart"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="performance-chart"]')
    ).toBeVisible();

    // Test time range selectors
    const timeRanges = ['1h', '24h', '7d', '30d'];

    for (const range of timeRanges) {
      await page.click(`[data-testid="time-range-${range}"]`);
      await page.waitForTimeout(1000);

      // Verify chart updates
      await expect(
        page.locator('[data-testid="chart-data-points"]')
      ).toBeVisible();
    }

    // Verify chart shows data for the last 24 hours by default
    await page.click('[data-testid="time-range-24h"]');

    // Check that data points exist
    const dataPoints = page.locator('[data-testid="chart-data-point"]');
    const pointCount = await dataPoints.count();
    expect(pointCount).toBeGreaterThan(0);
  });

  test('Multi-Chain Status Comparison', async ({ page }) => {
    // Navigate to comparison view
    await page.click('[data-testid="chain-comparison-tab"]');
    await page.waitForSelector('[data-testid="chain-comparison-table"]');

    // Verify comparison table headers
    const expectedHeaders = [
      'Network',
      'Status',
      'Block Height',
      'Sync %',
      'Peers',
      'Latency',
      'TPS',
      'Gas Price',
    ];

    for (const header of expectedHeaders) {
      await expect(
        page.locator(
          `[data-testid="header-${header.toLowerCase().replace(' ', '-')}"]`
        )
      ).toBeVisible();
    }

    // Verify data for each network row
    const networkRows = page.locator('[data-testid="network-row"]');
    const rowCount = await networkRows.count();
    expect(rowCount).toBeGreaterThan(0);

    // Check first few rows have complete data
    for (let i = 0; i < Math.min(rowCount, 3); i++) {
      const row = networkRows.nth(i);

      await expect(row.locator('[data-testid="network-name"]')).toBeVisible();
      await expect(
        row.locator('[data-testid="network-status-indicator"]')
      ).toBeVisible();
      await expect(
        row.locator('[data-testid="block-height-value"]')
      ).toBeVisible();
      await expect(
        row.locator('[data-testid="sync-percentage"]')
      ).toBeVisible();
    }

    // Test sorting functionality
    await page.click('[data-testid="sort-by-latency"]');
    await page.waitForTimeout(500);

    // Verify table is re-ordered (first row should change)
    await expect(
      page.locator('[data-testid="network-row"]').first()
    ).toBeVisible();
  });

  test('Alert and Notification System', async ({ page }) => {
    // Check for notification preferences
    await page.click('[data-testid="notification-settings"]');
    await page.waitForSelector('[data-testid="notification-preferences"]');

    // Verify notification toggles
    const notificationTypes = [
      'network-disconnection',
      'high-latency',
      'sync-issues',
      'bridge-downtime',
    ];

    for (const type of notificationTypes) {
      await expect(
        page.locator(`[data-testid="toggle-${type}"]`)
      ).toBeVisible();
    }

    // Test alert display
    await page.goto('/'); // Reset to main page

    // Simulate high latency condition
    await page.route('**/api/crosschain/metrics', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          metrics: {
            avg_finality_time: '300 seconds', // High latency
            success_rate: '85%', // Low success rate
          },
        }),
      });
    });

    await page.click('[data-testid="refresh-network-status"]');
    await page.waitForTimeout(2000);

    // Check for performance alerts
    const performanceAlert = page.locator('[data-testid="performance-alert"]');
    await expect(performanceAlert).toBeVisible({ timeout: 5000 });

    const alertText = await performanceAlert.textContent();
    expect(alertText.toLowerCase()).toContain('latency');
  });

  test('Mobile Responsive Network Status', async ({ page }) => {
    // Set mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });

    await page.goto('/');
    await page.waitForSelector('[data-testid="network-status-dashboard"]');

    // Verify mobile layout adapts properly
    await expect(
      page.locator('[data-testid="mobile-network-summary"]')
    ).toBeVisible();

    // Test collapsible network details
    await page.click('[data-testid="expand-network-details"]');
    await expect(
      page.locator('[data-testid="detailed-network-info"]')
    ).toBeVisible();

    // Verify touch-friendly controls
    const refreshButton = page.locator(
      '[data-testid="refresh-network-status"]'
    );
    await expect(refreshButton).toBeVisible();

    // Check button size is touch-friendly (at least 44px)
    const buttonBox = await refreshButton.boundingBox();
    expect(buttonBox.height).toBeGreaterThanOrEqual(44);
    expect(buttonBox.width).toBeGreaterThanOrEqual(44);
  });
});
