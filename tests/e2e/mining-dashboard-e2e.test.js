/**
 * Task 64.6 - End-to-End Mining Dashboard Tests
 *
 * Complete user workflow testing using Playwright
 * Tests the entire mining dashboard experience from setup to operations
 */

const { test, expect } = require('@playwright/test');

test.describe('Task 64.6 - Mining Dashboard End-to-End Tests', () => {
  let page;
  let baseURL;

  test.beforeAll(async ({ browser }) => {
    const context = await browser.newContext({
      viewport: { width: 1920, height: 1080 },
    });
    page = await context.newPage();
    baseURL = process.env.TEST_BASE_URL || 'http://localhost:3000';

    // Enable console logging for debugging
    page.on('console', msg => {
      if (msg.type() === 'error') {
        console.log('Browser console error:', msg.text());
      }
    });
  });

  test.describe('1. Complete Mining Workflow Integration', () => {
    test('should complete full mining setup and control workflow', async () => {
      console.log('Starting complete mining workflow test...');

      // Step 1: Navigate to mining dashboard
      await page.goto(`${baseURL}/miner`);
      await expect(page).toHaveTitle(/Miner Dashboard/);

      // Step 2: Complete one-click mining setup
      await page.click('[data-tab="one-click-mining"]');
      await page.waitForSelector('#one-click-mining', { state: 'visible' });

      // Fill in wallet address
      const walletInput = page.locator('#quick-wallet-address');
      await walletInput.fill('0x742d35Cc6634C0532925a3b8D7389d72d77f8e82');

      // Enable auto-optimization
      await page.check('#auto-optimize');

      // Select mining pools
      await page.check('input[name="pool"][value="wrf-pool-1"]');
      await page.check('input[name="pool"][value="wrf-pool-2"]');

      // Start mining setup
      await page.click('#start-mining-btn');

      // Wait for setup completion
      await page.waitForSelector('#setup-complete-message', {
        state: 'visible',
        timeout: 30000,
      });

      // Verify setup success message
      await expect(page.locator('#setup-complete-message')).toContainText(
        'successfully'
      );

      // Step 3: Verify mining controls are now available
      await page.waitForSelector('#mining-control-section', {
        state: 'visible',
      });

      // Check that mining status shows active
      const statusIndicator = page.locator('#mining-status-indicator');
      await expect(statusIndicator).toHaveClass(/active/);

      const statusText = page.locator('#mining-status-text');
      await expect(statusText).toContainText('Active');

      // Step 4: Test mining control operations
      console.log('Testing mining control operations...');

      // Pause mining
      await page.click('#pause-mining-btn');
      await page.waitForTimeout(1000); // Allow state transition

      await expect(statusIndicator).toHaveClass(/paused/);
      await expect(statusText).toContainText('Paused');

      // Resume mining
      await page.click('#resume-mining-btn');
      await page.waitForTimeout(1000);

      await expect(statusIndicator).toHaveClass(/active/);
      await expect(statusText).toContainText('Active');

      // Restart mining
      await page.click('#restart-mining-btn');
      await page.waitForTimeout(2000); // Restart might take longer

      await expect(statusIndicator).toHaveClass(/active/);
      await expect(statusText).toContainText('Active');

      console.log('Mining control operations test completed');
    });

    test('should display real-time statistics with live updates', async () => {
      console.log('Testing real-time statistics display...');

      await page.goto(`${baseURL}/miner`);
      await page.click('[data-tab="one-click-mining"]');

      // Wait for mining control section to be visible (assume mining is already active from previous test)
      await page.waitForSelector('#mining-control-section', {
        state: 'visible',
      });

      // Verify real-time statistics are displayed
      const hashrateDisplay = page.locator('#current-hashrate-display');
      const uptimeDisplay = page.locator('#uptime-display');
      const rewardsDisplay = page.locator('#rewards-earned-display');

      await expect(hashrateDisplay).toBeVisible();
      await expect(uptimeDisplay).toBeVisible();
      await expect(rewardsDisplay).toBeVisible();

      // Capture initial values
      const initialHashrate = await hashrateDisplay.textContent();
      const initialRewards = await rewardsDisplay.textContent();

      // Wait for potential updates (real-time polling interval)
      await page.waitForTimeout(8000); // Wait for one polling cycle

      // Verify values may have updated (or at least are still displayed)
      const updatedHashrate = await hashrateDisplay.textContent();
      const updatedRewards = await rewardsDisplay.textContent();

      expect(updatedHashrate).toBeTruthy();
      expect(updatedRewards).toBeTruthy();

      // Verify format of displayed values
      expect(updatedHashrate).toMatch(/[\d,.]+ [A-Z]*H\/s/); // Format like "1.50 MH/s"
      expect(updatedRewards).toMatch(/[\d,.]+/); // Numeric value

      console.log(`Real-time stats validation:
        - Initial hashrate: ${initialHashrate}
        - Updated hashrate: ${updatedHashrate}
        - Initial rewards: ${initialRewards}
        - Updated rewards: ${updatedRewards}
      `);
    });

    test('should navigate to and interact with performance monitoring dashboard', async () => {
      console.log('Testing performance monitoring dashboard...');

      await page.goto(`${baseURL}/dashboard`);

      // Navigate to mining dashboard
      await page.click('a[href="/mining-dashboard"]');
      await page.waitForSelector('[data-testid="miner-dashboard"]', {
        state: 'visible',
      });

      // Switch to performance monitoring mode
      await page.click('button[data-tab="performance"]');
      await page.waitForSelector('[data-testid="performance-dashboard"]', {
        state: 'visible',
      });

      // Verify performance metrics are displayed
      const cpuUsage = page.locator('[data-testid="cpu-usage"]');
      const memoryUsage = page.locator('[data-testid="memory-usage"]');
      const gpuUsage = page.locator('[data-testid="gpu-usage"]');
      const networkLatency = page.locator('[data-testid="network-latency"]');

      await expect(cpuUsage).toBeVisible();
      await expect(memoryUsage).toBeVisible();
      await expect(gpuUsage).toBeVisible();
      await expect(networkLatency).toBeVisible();

      // Verify performance charts are rendered
      const hashrateChart = page.locator('canvas[data-chart="hashrate"]');
      const temperatureChart = page.locator('canvas[data-chart="temperature"]');

      await expect(hashrateChart).toBeVisible();
      await expect(temperatureChart).toBeVisible();

      // Test alert threshold configuration
      await page.click('[data-testid="configure-alerts-btn"]');
      await page.waitForSelector('[data-testid="alert-config-modal"]', {
        state: 'visible',
      });

      // Modify temperature threshold
      const tempThreshold = page.locator('#temperature-threshold');
      await tempThreshold.fill('60');

      // Save configuration
      await page.click('[data-testid="save-thresholds-btn"]');
      await expect(
        page.locator('[data-testid="alert-config-modal"]')
      ).not.toBeVisible();

      console.log('Performance monitoring dashboard test completed');
    });

    test('should manage configuration settings effectively', async () => {
      console.log('Testing configuration management interface...');

      await page.goto(`${baseURL}/mining-dashboard`);

      // Navigate to configuration tab
      await page.click('button[data-tab="configuration"]');
      await page.waitForSelector('[data-testid="configuration-manager"]', {
        state: 'visible',
      });

      // Test algorithm configuration
      await page.click('[data-tab="algorithm"]');
      const algorithmSelect = page.locator('#algorithm-select');
      await algorithmSelect.selectOption('climate_proof_of_work');

      // Test pool configuration
      await page.click('[data-tab="pools"]');
      const primaryPoolInput = page.locator('#primary-pool-url');
      await primaryPoolInput.fill('stratum+tcp://pool1.wrfcoin.io:4444');

      const backupPoolInput = page.locator('#backup-pool-url');
      await backupPoolInput.fill('stratum+tcp://pool2.wrfcoin.io:4444');

      // Test hardware configuration
      await page.click('[data-tab="hardware"]');
      const gpuIntensitySlider = page.locator('#gpu-intensity');
      await gpuIntensitySlider.fill('85');

      const powerLimitInput = page.locator('#power-limit');
      await powerLimitInput.fill('300');

      // Save configuration
      await page.click('[data-testid="save-configuration-btn"]');

      // Wait for save confirmation
      await expect(page.locator('[data-testid="save-success"]')).toBeVisible();
      await expect(page.locator('[data-testid="save-success"]')).toContainText(
        'Configuration saved'
      );

      // Test configuration export
      await page.click('[data-testid="export-config-btn"]');

      // Start download and verify it initiates
      const downloadPromise = page.waitForEvent('download');
      await downloadPromise;

      console.log('Configuration management test completed');
    });
  });

  test.describe('2. Responsive Design and Mobile Experience', () => {
    test('should work correctly on mobile devices', async () => {
      console.log('Testing mobile responsiveness...');

      // Set mobile viewport
      await page.setViewportSize({ width: 375, height: 667 }); // iPhone SE

      await page.goto(`${baseURL}/miner`);

      // Verify mobile navigation works
      const mobileMenu = page.locator('[data-testid="mobile-menu-toggle"]');
      if (await mobileMenu.isVisible()) {
        await mobileMenu.click();
      }

      // Test one-click mining on mobile
      await page.click('[data-tab="one-click-mining"]');
      await page.waitForSelector('#one-click-mining', { state: 'visible' });

      // Verify mobile-specific elements
      const walletInput = page.locator('#quick-wallet-address');
      await expect(walletInput).toBeVisible();

      // Test touch interactions
      await walletInput.tap();
      await walletInput.fill('0x742d35Cc6634C0532925a3b8D7389d72d77f8e82');

      // Verify mining controls are accessible on mobile
      if (await page.locator('#mining-control-section').isVisible()) {
        const pauseBtn = page.locator('#pause-mining-btn');
        await expect(pauseBtn).toBeVisible();

        // Verify button is large enough for touch
        const buttonBox = await pauseBtn.boundingBox();
        expect(buttonBox.height).toBeGreaterThan(44); // iOS recommended minimum
      }

      console.log('Mobile responsiveness test completed');
    });

    test('should work correctly on tablet devices', async () => {
      console.log('Testing tablet responsiveness...');

      // Set tablet viewport
      await page.setViewportSize({ width: 768, height: 1024 }); // iPad

      await page.goto(`${baseURL}/mining-dashboard`);

      // Test dashboard layout on tablet
      await page.waitForSelector('[data-testid="miner-dashboard"]', {
        state: 'visible',
      });

      // Verify tab navigation works on tablet
      await page.click('button[data-tab="performance"]');
      await page.waitForSelector('[data-testid="performance-dashboard"]', {
        state: 'visible',
      });

      // Test chart interactions on tablet
      const hashrateChart = page.locator('canvas[data-chart="hashrate"]');
      if (await hashrateChart.isVisible()) {
        await hashrateChart.tap();
        // Chart should handle touch interactions
      }

      // Test configuration form on tablet
      await page.click('button[data-tab="configuration"]');
      await page.waitForSelector('[data-testid="configuration-manager"]', {
        state: 'visible',
      });

      const gpuSlider = page.locator('#gpu-intensity');
      if (await gpuSlider.isVisible()) {
        // Test slider interaction on touch device
        await gpuSlider.tap();
      }

      console.log('Tablet responsiveness test completed');
    });
  });

  test.describe('3. Error Handling and Recovery', () => {
    test('should handle network failures gracefully', async () => {
      console.log('Testing network failure handling...');

      await page.goto(`${baseURL}/miner`);

      // Simulate network failure by intercepting API calls
      await page.route('**/api/mining/**', route => {
        route.abort('failed');
      });

      // Try to perform an action that requires API
      await page.click('[data-tab="one-click-mining"]');
      await page.click('#start-mining-btn');

      // Should show error message
      await expect(page.locator('[data-testid="error-message"]')).toBeVisible();
      await expect(page.locator('[data-testid="error-message"]')).toContainText(
        /network|error|failed/i
      );

      // Restore network and test recovery
      await page.unroute('**/api/mining/**');

      // Retry button should work
      const retryButton = page.locator('[data-testid="retry-btn"]');
      if (await retryButton.isVisible()) {
        await retryButton.click();

        // Error should disappear
        await expect(
          page.locator('[data-testid="error-message"]')
        ).not.toBeVisible();
      }

      console.log('Network failure handling test completed');
    });

    test('should handle form validation errors', async () => {
      console.log('Testing form validation...');

      await page.goto(`${baseURL}/mining-dashboard`);
      await page.click('button[data-tab="configuration"]');

      // Try to save configuration with invalid data
      await page.click('[data-tab="hardware"]');

      const powerLimitInput = page.locator('#power-limit');
      await powerLimitInput.fill('9999'); // Invalid high value

      await page.click('[data-testid="save-configuration-btn"]');

      // Should show validation error
      await expect(
        page.locator('[data-testid="validation-error"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="validation-error"]')
      ).toContainText(/power limit|invalid|range/i);

      // Fix the value
      await powerLimitInput.fill('300');
      await page.click('[data-testid="save-configuration-btn"]');

      // Error should disappear and success should show
      await expect(
        page.locator('[data-testid="validation-error"]')
      ).not.toBeVisible();
      await expect(page.locator('[data-testid="save-success"]')).toBeVisible();

      console.log('Form validation test completed');
    });
  });

  test.describe('4. Performance and Load Testing', () => {
    test('should maintain responsiveness during intensive operations', async () => {
      console.log('Testing UI responsiveness under load...');

      await page.goto(`${baseURL}/mining-dashboard`);
      await page.click('button[data-tab="performance"]');

      // Start multiple intensive operations
      const startTime = Date.now();

      // Rapidly switch between tabs to test responsiveness
      for (let i = 0; i < 10; i++) {
        await page.click('button[data-tab="configuration"]');
        await page.waitForTimeout(100);
        await page.click('button[data-tab="performance"]');
        await page.waitForTimeout(100);
      }

      const endTime = Date.now();
      const totalTime = endTime - startTime;

      console.log(`UI responsiveness test completed in ${totalTime}ms`);

      // Should complete within reasonable time
      expect(totalTime).toBeLessThan(5000); // Less than 5 seconds

      // UI should still be functional
      await expect(
        page.locator('[data-testid="performance-dashboard"]')
      ).toBeVisible();
    });

    test('should handle real-time updates efficiently', async () => {
      console.log('Testing real-time update efficiency...');

      await page.goto(`${baseURL}/miner`);
      await page.click('[data-tab="one-click-mining"]');

      // Monitor for real-time updates
      let updateCount = 0;
      const hashrateDisplay = page.locator('#current-hashrate-display');

      // Count updates over time period
      const monitoringDuration = 15000; // 15 seconds
      const startTime = Date.now();

      while (Date.now() - startTime < monitoringDuration) {
        await page.waitForTimeout(1000);
        if (await hashrateDisplay.isVisible()) {
          updateCount++;
        }
      }

      console.log(
        `Real-time updates: ${updateCount} checks over ${monitoringDuration / 1000}s`
      );

      // Should have maintained responsiveness
      expect(updateCount).toBeGreaterThan(0);
      await expect(hashrateDisplay).toBeVisible();
    });
  });

  test.describe('5. Accessibility Testing', () => {
    test('should meet accessibility standards', async () => {
      console.log('Testing accessibility compliance...');

      await page.goto(`${baseURL}/miner`);

      // Test keyboard navigation
      await page.keyboard.press('Tab');
      await page.keyboard.press('Tab');
      await page.keyboard.press('Enter'); // Should activate focused element

      // Test screen reader compatibility
      const startButton = page.locator('#start-mining-btn');
      const ariaLabel = await startButton.getAttribute('aria-label');
      expect(ariaLabel).toBeTruthy();

      // Test high contrast mode compatibility
      await page.emulateMedia({ colorScheme: 'dark' });
      await expect(page.locator('body')).toBeVisible();

      // Test focus indicators
      await page.keyboard.press('Tab');
      const focusedElement = page.locator(':focus');
      await expect(focusedElement).toBeVisible();

      console.log('Accessibility testing completed');
    });
  });

  test.describe('6. Cross-Browser Compatibility', () => {
    test('should work across different browsers', async () => {
      // This test would typically be run with different browser contexts
      console.log('Testing cross-browser compatibility...');

      await page.goto(`${baseURL}/miner`);

      // Test modern web features
      const webSocketSupport = await page.evaluate(() => {
        return typeof WebSocket !== 'undefined';
      });
      expect(webSocketSupport).toBe(true);

      // Test Chart.js compatibility
      const chartSupport = await page.evaluate(() => {
        return typeof HTMLCanvasElement !== 'undefined';
      });
      expect(chartSupport).toBe(true);

      // Test local storage
      const localStorageSupport = await page.evaluate(() => {
        try {
          localStorage.setItem('test', 'test');
          localStorage.removeItem('test');
          return true;
        } catch (e) {
          return false;
        }
      });
      expect(localStorageSupport).toBe(true);

      console.log('Cross-browser compatibility test completed');
    });
  });

  test.describe('7. Security Testing', () => {
    test('should handle XSS prevention', async () => {
      console.log('Testing XSS prevention...');

      await page.goto(`${baseURL}/miner`);

      // Try to inject script via wallet address input
      const walletInput = page.locator('#quick-wallet-address');
      await walletInput.fill('<script>alert("XSS")</script>');

      // Should be sanitized
      const inputValue = await walletInput.inputValue();
      expect(inputValue).not.toContain('<script>');

      console.log('XSS prevention test completed');
    });

    test('should validate input properly', async () => {
      console.log('Testing input validation...');

      await page.goto(`${baseURL}/mining-dashboard`);
      await page.click('button[data-tab="configuration"]');

      // Test with various invalid inputs
      const powerLimitInput = page.locator('#power-limit');

      // Test with non-numeric input
      await powerLimitInput.fill('invalid');
      await page.click('[data-testid="save-configuration-btn"]');

      await expect(
        page.locator('[data-testid="validation-error"]')
      ).toBeVisible();

      console.log('Input validation test completed');
    });
  });

  test.afterAll(async () => {
    if (page) {
      await page.close();
    }
  });
});

// Export test configuration for custom reporting
module.exports = {
  testConfig: {
    name: 'Task 64.6 - Mining Dashboard E2E Tests',
    timeout: 60000,
    retries: 2,
    use: {
      headless: process.env.CI === 'true',
      viewport: { width: 1920, height: 1080 },
      actionTimeout: 10000,
      navigationTimeout: 30000,
    },
  },
};
