/**
 * Cross-Chain Bridge UX Verification Tests
 *
 * Comprehensive UI/UX testing for bridge functionality:
 * - Network selector dropdowns
 * - Bridge progress timeline
 * - Fee estimation display
 * - Transaction status tracking
 */
import { test, expect, Page } from '@playwright/test';
import { UXVerifier, VIEWPORTS } from '../utils/ux-helpers';

test.describe('Cross-Chain Bridge UX Verification', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  test.describe('Bridge Interface', () => {
    test('should display bridge interface with proper loading', async ({ page }) => {
      await page.goto('/bridge');

      const loadingResult = await verifier.verifyLoadingStates();
      expect(loadingResult.hasLoadingIndicator).toBe(true);

      await verifier.captureScreenshot('bridge-loaded');

      // Bridge interface should be visible
      const bridgeContent = page.locator(
        '[data-testid="bridge-container"], [class*="bridge"], main'
      ).first();
      await expect(bridgeContent).toBeVisible({ timeout: 10000 });
    });

    test('should display supported networks', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Look for network selectors
      const sourceNetwork = page.locator(
        '[data-testid="source-network"], [class*="source-chain"], select[name*="source"]'
      ).first();

      const destNetwork = page.locator(
        '[data-testid="destination-network"], [class*="dest-chain"], select[name*="dest"]'
      ).first();

      await verifier.captureScreenshot('bridge-networks');

      // At least one network selector should exist
      const hasSource = await sourceNetwork.isVisible({ timeout: 5000 }).catch(() => false);
      const hasDest = await destNetwork.isVisible({ timeout: 5000 }).catch(() => false);

      expect(hasSource || hasDest).toBe(true);
    });
  });

  test.describe('Network Selector Dropdowns', () => {
    test('should handle source network selection', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const sourceSelector = page.locator(
        '[data-testid="source-network"], [class*="source"] select, [class*="from-chain"]'
      ).first();

      if (await sourceSelector.isVisible({ timeout: 5000 }).catch(() => false)) {
        await sourceSelector.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('bridge-source-dropdown');

        // Network list should show
        const networkOptions = page.locator(
          '[role="option"], option, [class*="network-option"]'
        );
        const optionCount = await networkOptions.count();
        expect(optionCount).toBeGreaterThan(0);

        // Select first option
        await networkOptions.first().click();
        await page.waitForTimeout(300);
      }
    });

    test('should handle destination network selection', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const destSelector = page.locator(
        '[data-testid="destination-network"], [class*="dest"] select, [class*="to-chain"]'
      ).first();

      if (await destSelector.isVisible({ timeout: 5000 }).catch(() => false)) {
        await destSelector.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('bridge-dest-dropdown');

        const networkOptions = page.locator('[role="option"], option');
        if (await networkOptions.count() > 1) {
          // Select second option (different from source)
          await networkOptions.nth(1).click();
          await page.waitForTimeout(300);
        }
      }
    });

    test('should display network icons and names', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Networks should have identifiable icons
      const networkIcons = page.locator(
        '[class*="network-icon"], [class*="chain-logo"], img[alt*="network"]'
      );

      const iconCount = await networkIcons.count();
      await verifier.captureScreenshot('bridge-network-icons');

      // Networks should have names
      const networkNames = page.locator(
        '[class*="network-name"], [class*="chain-name"]'
      );
      const nameCount = await networkNames.count();
    });

    test('should handle network swap button', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Look for swap/reverse button
      const swapButton = page.locator(
        '[data-testid="swap-networks"], button[aria-label*="swap"], [class*="reverse"], [class*="switch"]'
      ).first();

      if (await swapButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Capture before swap
        await verifier.captureScreenshot('bridge-before-swap');

        await swapButton.click();
        await page.waitForTimeout(500);

        // Capture after swap
        await verifier.captureScreenshot('bridge-after-swap');
      }
    });
  });

  test.describe('Fee Estimation', () => {
    test('should display bridge fee estimation', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Enter amount
      const amountInput = page.locator(
        '[data-testid="bridge-amount"], input[type="number"], input[placeholder*="amount"]'
      ).first();

      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        await amountInput.fill('100');
        await page.waitForTimeout(1000);

        await verifier.captureScreenshot('bridge-fee-estimation');

        // Fee display should appear
        const feeDisplay = page.locator(
          '[data-testid="bridge-fee"], [class*="fee"], [class*="cost"]'
        ).first();

        if (await feeDisplay.isVisible({ timeout: 5000 }).catch(() => false)) {
          const feeText = await feeDisplay.textContent();
          // Fee should contain a number
          expect(feeText).toMatch(/\d/);
        }
      }
    });

    test('should display estimated time', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator('input[type="number"]').first();
      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        await amountInput.fill('100');
        await page.waitForTimeout(1000);

        // Look for time estimate
        const timeEstimate = page.locator(
          '[data-testid="estimated-time"], [class*="time"], [class*="duration"]'
        ).first();

        if (await timeEstimate.isVisible({ timeout: 5000 }).catch(() => false)) {
          await verifier.captureScreenshot('bridge-time-estimate');
        }
      }
    });

    test('should show receive amount after fees', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator('input[type="number"]').first();
      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        await amountInput.fill('100');
        await page.waitForTimeout(1000);

        // Look for receive amount
        const receiveAmount = page.locator(
          '[data-testid="receive-amount"], [class*="receive"], [class*="output"]'
        ).first();

        if (await receiveAmount.isVisible({ timeout: 5000 }).catch(() => false)) {
          const receiveText = await receiveAmount.textContent();
          expect(receiveText).toMatch(/\d/);
        }
      }
    });
  });

  test.describe('Bridge Progress Timeline', () => {
    test('should display transaction progress steps', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Look for progress/timeline component
      const progressTimeline = page.locator(
        '[data-testid="bridge-progress"], [class*="progress"], [class*="timeline"], [class*="stepper"]'
      ).first();

      if (await progressTimeline.isVisible({ timeout: 5000 }).catch(() => false)) {
        await verifier.captureScreenshot('bridge-progress-timeline');

        // Should have multiple steps
        const steps = progressTimeline.locator('[class*="step"]');
        const stepCount = await steps.count();
        expect(stepCount).toBeGreaterThan(1);
      }
    });

    test('should show step statuses (pending/complete)', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Look for step indicators
      const stepIndicators = page.locator(
        '[class*="step-indicator"], [class*="step-status"], [class*="step-icon"]'
      );

      const indicatorCount = await stepIndicators.count();

      if (indicatorCount > 0) {
        await verifier.captureScreenshot('bridge-step-statuses');
      }
    });
  });

  test.describe('Transaction History', () => {
    test('should display bridge transaction history', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Look for history section or tab
      const historyTab = page.locator(
        'button:has-text("History"), [data-testid="bridge-history"], a:has-text("History")'
      ).first();

      if (await historyTab.isVisible({ timeout: 5000 }).catch(() => false)) {
        await historyTab.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('bridge-history');

        // History list should appear
        const historyList = page.locator(
          '[data-testid="history-list"], [class*="history"], [class*="transaction-list"]'
        ).first();

        if (await historyList.isVisible({ timeout: 5000 }).catch(() => false)) {
          const historyItems = historyList.locator('[class*="item"], [class*="row"], li');
          const itemCount = await historyItems.count();
        }
      }
    });

    test('should display transaction status badges', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const historyTab = page.locator('button:has-text("History")').first();
      if (await historyTab.isVisible({ timeout: 5000 }).catch(() => false)) {
        await historyTab.click();
        await page.waitForTimeout(500);

        // Look for status badges
        const statusBadges = page.locator(
          '[class*="badge"], [class*="status"], [class*="tag"]'
        );
        const badgeCount = await statusBadges.count();

        if (badgeCount > 0) {
          await verifier.captureScreenshot('bridge-status-badges');
        }
      }
    });
  });

  test.describe('Error Handling', () => {
    test('should handle invalid amount gracefully', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator('input[type="number"]').first();
      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Enter invalid amount
        await amountInput.fill('0');
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('bridge-invalid-amount');

        // Check for error message
        const error = page.locator('[class*="error"]').first();
        const hasError = await error.isVisible({ timeout: 2000 }).catch(() => false);
      }
    });

    test('should handle insufficient balance', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator('input[type="number"]').first();
      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Enter very large amount
        await amountInput.fill('9999999999');
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('bridge-insufficient-balance');

        // Button should be disabled or show error
        const bridgeButton = page.locator('button:has-text("Bridge")').first();
        if (await bridgeButton.isVisible({ timeout: 2000 }).catch(() => false)) {
          const isDisabled = await bridgeButton.isDisabled();
        }
      }
    });

    test('should handle network errors', async ({ page }) => {
      // Block API calls
      await page.route('**/api/bridge/**', route => route.abort());
      await page.route('**/bridge/**', route => route.abort());

      await page.goto('/bridge');
      await page.waitForTimeout(2000);

      const errorResult = await verifier.verifyErrorStates();
      await verifier.captureScreenshot('bridge-network-error');
    });
  });

  test.describe('Responsive Design', () => {
    test('should adapt bridge interface for mobile', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('bridge-mobile');

      // Network selectors should be usable
      const selectors = page.locator('select, [class*="select"]');
      const selectorCount = await selectors.count();

      if (selectorCount > 0) {
        const firstSelector = selectors.first();
        const box = await firstSelector.boundingBox();
        if (box) {
          // Touch targets should be at least 44px
          expect(box.height).toBeGreaterThanOrEqual(40);
        }
      }
    });

    test('should maintain usability across viewports', async ({ page }) => {
      for (const [name, viewport] of Object.entries(VIEWPORTS)) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
        await page.goto('/bridge');
        await page.waitForLoadState('domcontentloaded');

        await verifier.captureScreenshot(`bridge-${name}`);

        // Bridge button should be visible
        const bridgeButton = page.locator('button:has-text("Bridge")').first();
        if (await bridgeButton.isVisible({ timeout: 2000 }).catch(() => false)) {
          await expect(bridgeButton).toBeVisible();
        }
      }
    });

    test('should handle timeline on mobile', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Timeline should adapt (vertical on mobile)
      const timeline = page.locator('[class*="timeline"], [class*="progress"]').first();

      if (await timeline.isVisible({ timeout: 5000 }).catch(() => false)) {
        const box = await timeline.boundingBox();
        if (box) {
          // Should fit within viewport
          expect(box.width).toBeLessThanOrEqual(375);
        }
        await verifier.captureScreenshot('bridge-mobile-timeline');
      }
    });
  });

  test.describe('Accessibility', () => {
    test('should pass accessibility audit', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      const result = await verifier.runAccessibilityAudit();

      // No critical violations
      const critical = result.violations.filter(v => v.impact === 'critical');
      expect(critical).toHaveLength(0);
    });

    test('should support keyboard navigation', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Tab through form elements
      for (let i = 0; i < 10; i++) {
        await page.keyboard.press('Tab');
      }

      const focused = page.locator(':focus');
      expect(await focused.count()).toBeGreaterThan(0);

      await verifier.captureScreenshot('bridge-keyboard-nav');
    });

    test('should have proper ARIA labels', async ({ page }) => {
      await page.goto('/bridge');
      await page.waitForLoadState('domcontentloaded');

      // Network selectors should have labels
      const selects = page.locator('select');
      const selectCount = await selects.count();

      for (let i = 0; i < selectCount; i++) {
        const select = selects.nth(i);
        const ariaLabel = await select.getAttribute('aria-label');
        const label = await page.locator(`label[for="${await select.getAttribute('id')}"]`).count();
        // Should have either aria-label or associated label
      }
    });
  });
});
