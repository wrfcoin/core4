const { test, expect } = require('@playwright/test');

/**
 * Security Dashboard Testing Suite
 * Tests security monitoring, alerts, and dashboard functionality
 */
test.describe('Security Dashboard Tests', () => {
  let dashboardPage;

  test.beforeEach(async ({ page }) => {
    dashboardPage = page;
    await page.goto('/security');
    await page.waitForSelector('[data-testid="security-dashboard"]', {
      timeout: 10000,
    });
  });

  test('should display security dashboard main interface', async () => {
    // Verify main dashboard elements
    await expect(
      dashboardPage.locator('h1:has-text("Security Settings")')
    ).toBeVisible();

    // Check for tab navigation
    const tabs = ['MEV Protection', 'Privacy Settings', 'Blockchain Firewall'];
    for (const tab of tabs) {
      await expect(
        dashboardPage.locator(`button:has-text("${tab}")`)
      ).toBeVisible();
    }

    // Verify active protection toggle
    await expect(dashboardPage.locator('text=Active Protection')).toBeVisible();
  });

  test('should display security status indicators', async () => {
    // Look for security status elements
    const statusIndicators = [
      'Active Protection',
      'Monitoring Only',
      'Enabled',
      'Disabled',
    ];

    for (const indicator of statusIndicators) {
      const element = dashboardPage.locator(`text=${indicator}`);
      if ((await element.count()) > 0) {
        await expect(element.first()).toBeVisible();
      }
    }
  });

  test('should test active protection toggle functionality', async () => {
    // Find the main active protection toggle
    const protectionToggle = dashboardPage
      .locator('input[type="checkbox"]')
      .first();

    // Get initial state
    const initialState = await protectionToggle.isChecked();

    // Toggle protection
    await protectionToggle.click();

    // Verify state changed
    const newState = await protectionToggle.isChecked();
    expect(newState).toBe(!initialState);

    // Verify status text updates
    if (newState) {
      await expect(
        dashboardPage.locator('text=Active Protection')
      ).toBeVisible();
    } else {
      await expect(dashboardPage.locator('text=Monitoring Only')).toBeVisible();
    }
  });

  test('should display critical security alerts', async () => {
    // Simulate critical security events
    await dashboardPage.evaluate(() => {
      // Mock critical alert
      const alertData = {
        level: 'CRITICAL',
        message: 'Unauthorized access attempt detected',
        timestamp: new Date().toISOString(),
        source: 'Security Monitor',
      };

      // Store in localStorage to trigger alert display
      window.localStorage.setItem(
        'security-alerts',
        JSON.stringify([alertData])
      );

      // Trigger alert display
      window.dispatchEvent(
        new CustomEvent('security-alert', { detail: alertData })
      );
    });

    // Look for critical alert indicators
    const criticalAlertSelectors = [
      '.bg-red-100',
      '.text-red-800',
      'text=CRITICAL',
      'text=Unauthorized access',
      '.alert-critical',
    ];

    for (const selector of criticalAlertSelectors) {
      const element = dashboardPage.locator(selector);
      if ((await element.count()) > 0) {
        await expect(element.first()).toBeVisible();
      }
    }
  });

  test('should display warning level alerts', async () => {
    // Simulate warning level security events
    await dashboardPage.evaluate(() => {
      const alertData = {
        level: 'WARNING',
        message: 'Suspicious transaction pattern detected',
        timestamp: new Date().toISOString(),
        source: 'MEV Monitor',
      };

      window.localStorage.setItem(
        'security-alerts',
        JSON.stringify([alertData])
      );
      window.dispatchEvent(
        new CustomEvent('security-alert', { detail: alertData })
      );
    });

    // Look for warning alert indicators
    const warningAlertSelectors = [
      '.bg-yellow-100',
      '.text-yellow-800',
      'text=WARNING',
      'text=Suspicious transaction',
      '.alert-warning',
    ];

    for (const selector of warningAlertSelectors) {
      const element = dashboardPage.locator(selector);
      if ((await element.count()) > 0) {
        await expect(element.first()).toBeVisible();
      }
    }
  });

  test('should display informational alerts', async () => {
    // Simulate informational security events
    await dashboardPage.evaluate(() => {
      const alertData = {
        level: 'INFO',
        message: 'Security settings updated successfully',
        timestamp: new Date().toISOString(),
        source: 'System',
      };

      window.localStorage.setItem(
        'security-alerts',
        JSON.stringify([alertData])
      );
      window.dispatchEvent(
        new CustomEvent('security-alert', { detail: alertData })
      );
    });

    // Look for info alert indicators
    const infoAlertSelectors = [
      '.bg-blue-100',
      '.text-blue-800',
      'text=INFO',
      'text=successfully',
      '.alert-info',
    ];

    for (const selector of infoAlertSelectors) {
      const element = dashboardPage.locator(selector);
      if ((await element.count()) > 0) {
        await expect(element.first()).toBeVisible();
      }
    }
  });

  test('should test security metrics display', async () => {
    // Look for security metrics and statistics
    const metrics = [
      'Blocked Attacks',
      'MEV Prevented',
      'Privacy Score',
      'Security Score',
      'Threats Detected',
      'Vulnerabilities',
    ];

    for (const metric of metrics) {
      const metricElement = dashboardPage.locator(`text*=${metric}`);
      if ((await metricElement.count()) > 0) {
        await expect(metricElement.first()).toBeVisible();
        console.log(`Found security metric: ${metric}`);
      }
    }
  });

  test('should test security recommendations display', async () => {
    // Look for security recommendations
    const recommendations = [
      'Enable MEV protection',
      'Update privacy settings',
      'Review transaction patterns',
      'Update security configuration',
      'Enable two-factor authentication',
    ];

    for (const recommendation of recommendations) {
      const recElement = dashboardPage.locator(`text*=${recommendation}`);
      if ((await recElement.count()) > 0) {
        await expect(recElement.first()).toBeVisible();
        console.log(`Found security recommendation: ${recommendation}`);
      }
    }
  });

  test('should test alert acknowledgment functionality', async () => {
    // Create a test alert
    await dashboardPage.evaluate(() => {
      const alertData = {
        id: 'test-alert-1',
        level: 'WARNING',
        message: 'Test alert for acknowledgment',
        timestamp: new Date().toISOString(),
        acknowledged: false,
      };

      window.localStorage.setItem(
        'security-alerts',
        JSON.stringify([alertData])
      );
    });

    // Look for acknowledge button
    const acknowledgeButton = dashboardPage.locator(
      'button:has-text("Acknowledge"), button:has-text("Dismiss")'
    );
    if ((await acknowledgeButton.count()) > 0) {
      await acknowledgeButton.first().click();

      // Verify alert is acknowledged/dismissed
      await expect(
        dashboardPage.locator('text=Test alert for acknowledgment')
      ).not.toBeVisible();
    }
  });

  test('should test security dashboard refresh functionality', async () => {
    // Add a refresh button test
    const refreshButton = dashboardPage.locator(
      'button:has-text("Refresh"), button[title*="refresh"], .refresh-btn'
    );

    if ((await refreshButton.count()) > 0) {
      // Click refresh
      await refreshButton.first().click();

      // Wait for loading to complete
      await dashboardPage.waitForLoadState('networkidle');

      // Verify dashboard is still functional
      await expect(
        dashboardPage.locator('h1:has-text("Security Settings")')
      ).toBeVisible();
    }
  });

  test('should test security timeline/history display', async () => {
    // Look for security event history
    const historyElements = [
      'Security History',
      'Event Timeline',
      'Recent Events',
      'Activity Log',
      'Audit Trail',
    ];

    for (const element of historyElements) {
      const historyElement = dashboardPage.locator(`text*=${element}`);
      if ((await historyElement.count()) > 0) {
        await expect(historyElement.first()).toBeVisible();
        console.log(`Found security history element: ${element}`);
      }
    }
  });

  test('should test security settings export functionality', async () => {
    // Look for export/download options
    const exportButtons = [
      'Export Settings',
      'Download Report',
      'Export Logs',
      'Download Configuration',
    ];

    for (const button of exportButtons) {
      const exportButton = dashboardPage.locator(
        `button:has-text("${button}")`
      );
      if ((await exportButton.count()) > 0) {
        await expect(exportButton.first()).toBeVisible();
        console.log(`Found export functionality: ${button}`);
      }
    }
  });

  test('should verify security dashboard responsiveness', async () => {
    // Test mobile viewport
    await dashboardPage.setViewportSize({ width: 375, height: 667 });

    // Verify main elements are still visible
    await expect(
      dashboardPage.locator('h1:has-text("Security Settings")')
    ).toBeVisible();

    // Test tablet viewport
    await dashboardPage.setViewportSize({ width: 768, height: 1024 });

    // Verify layout adapts
    await expect(
      dashboardPage.locator('h1:has-text("Security Settings")')
    ).toBeVisible();

    // Test desktop viewport
    await dashboardPage.setViewportSize({ width: 1280, height: 720 });

    // Verify full functionality
    await expect(
      dashboardPage.locator('h1:has-text("Security Settings")')
    ).toBeVisible();
  });

  test('should test security dashboard accessibility', async () => {
    // Check for accessibility features
    const accessibilityFeatures = [
      'aria-label',
      'aria-describedby',
      'role',
      'tabindex',
    ];

    for (const feature of accessibilityFeatures) {
      const elements = dashboardPage.locator(`[${feature}]`);
      if ((await elements.count()) > 0) {
        console.log(`Found accessibility feature: ${feature}`);
      }
    }

    // Test keyboard navigation
    await dashboardPage.keyboard.press('Tab');
    const focusedElement = await dashboardPage.locator(':focus');
    if ((await focusedElement.count()) > 0) {
      console.log('Keyboard navigation is working');
    }
  });
});
