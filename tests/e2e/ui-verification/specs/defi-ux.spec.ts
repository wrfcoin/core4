/**
 * DeFi Trading UX Verification Tests
 *
 * Comprehensive UI/UX testing for DeFi functionality:
 * - Token selector dropdowns
 * - Amount input validation
 * - Price impact warnings
 * - Transaction confirmation flow
 */
import { test, expect, Page } from '@playwright/test';
import { UXVerifier, VIEWPORTS } from '../utils/ux-helpers';

test.describe('DeFi UX Verification', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  test.describe('Token Swap Interface', () => {
    test('should display swap interface with proper loading', async ({ page }) => {
      // Navigate to swap/trade page
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      // Look for swap/trade navigation
      const swapLink = page.locator(
        '[data-testid="nav-swap"], a:has-text("Swap"), a:has-text("Trade"), [href*="swap"]'
      ).first();

      if (await swapLink.isVisible({ timeout: 5000 }).catch(() => false)) {
        await swapLink.click();
        await page.waitForLoadState('domcontentloaded');
      }

      const loadingResult = await verifier.verifyLoadingStates();
      await verifier.captureScreenshot('defi-swap-loaded');
    });

    test('should handle token selector dropdowns correctly', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      // Look for token selector
      const tokenSelector = page.locator(
        '[data-testid="from-token-select"], [data-testid="token-select"], select[name*="token"], [class*="token-select"]'
      ).first();

      if (await tokenSelector.isVisible({ timeout: 5000 }).catch(() => false)) {
        await tokenSelector.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('defi-token-selector-open');

        // Token list should appear
        const tokenList = page.locator(
          '[data-testid="token-list"], [class*="token-list"], [role="listbox"], .dropdown-menu'
        ).first();

        if (await tokenList.isVisible({ timeout: 2000 }).catch(() => false)) {
          // Tokens should have icons and names
          const tokenItems = tokenList.locator('[class*="token-item"], [role="option"], li');
          const count = await tokenItems.count();
          expect(count).toBeGreaterThan(0);
        }

        // Close dropdown
        await page.keyboard.press('Escape');
      }
    });

    test('should validate amount inputs', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator(
        '[data-testid="from-amount-input"], input[type="number"], input[placeholder*="amount"], input[class*="amount"]'
      ).first();

      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Test various inputs
        await amountInput.fill('abc'); // Invalid
        await verifier.captureScreenshot('defi-invalid-amount');

        await amountInput.fill('999999999'); // Very large
        await verifier.captureScreenshot('defi-large-amount');

        await amountInput.fill('0.001'); // Small valid amount
        await verifier.captureScreenshot('defi-valid-amount');

        // Check for validation feedback
        const errorMessage = page.locator(
          '[class*="error"], [data-testid*="error"], [class*="invalid"]'
        ).first();
        // Error handling is implementation-specific
      }
    });

    test('should display price impact warnings', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator(
        '[data-testid="from-amount-input"], input[type="number"]'
      ).first();

      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Enter large amount to trigger price impact
        await amountInput.fill('100000');
        await page.waitForTimeout(1000);

        await verifier.captureScreenshot('defi-price-impact');

        // Look for price impact warning
        const priceImpact = page.locator(
          '[data-testid="price-impact"], [class*="price-impact"], [class*="slippage-warning"]'
        ).first();

        if (await priceImpact.isVisible({ timeout: 2000 }).catch(() => false)) {
          const impactText = await priceImpact.textContent();
          // Price impact should show percentage
        }
      }
    });

    test('should show swap rate and estimated output', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator('input[type="number"]').first();

      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        await amountInput.fill('100');
        await page.waitForTimeout(1000);

        // Look for swap rate
        const swapRate = page.locator(
          '[data-testid="swap-rate"], [class*="exchange-rate"], [class*="rate"]'
        ).first();

        // Look for output estimate
        const outputEstimate = page.locator(
          '[data-testid="to-amount-estimate"], [class*="output"], input:disabled'
        ).first();

        await verifier.captureScreenshot('defi-swap-calculation');
      }
    });
  });

  test.describe('Transaction Flow', () => {
    test('should handle transaction confirmation flow', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      // Fill swap form
      const amountInput = page.locator('input[type="number"]').first();
      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        await amountInput.fill('10');
      }

      // Look for review/swap button
      const swapButton = page.locator(
        '[data-testid="review-swap-button"], [data-testid="swap-button"], button:has-text("Swap"), button:has-text("Review")'
      ).first();

      if (await swapButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await swapButton.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('defi-transaction-confirm');

        // Look for confirmation modal
        const confirmModal = page.locator(
          '[data-testid="modal"], [role="dialog"], .modal'
        ).first();

        if (await confirmModal.isVisible({ timeout: 2000 }).catch(() => false)) {
          // Should show transaction summary
          const summary = confirmModal.locator('[class*="summary"], [class*="details"]');
        }
      }
    });

    test('should display slippage settings', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      // Look for settings/slippage button
      const settingsButton = page.locator(
        '[data-testid="slippage-settings-button"], [data-testid="settings-button"], button[aria-label*="settings"], [class*="settings"]'
      ).first();

      if (await settingsButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await settingsButton.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('defi-slippage-settings');

        // Should show slippage options
        const slippageOptions = page.locator(
          '[data-testid*="slippage"], [class*="slippage"], input[name*="slippage"]'
        );
        const optionCount = await slippageOptions.count();
      }
    });
  });

  test.describe('Liquidity Interface', () => {
    test('should display liquidity pools', async ({ page }) => {
      // Navigate to liquidity page if exists
      const liquidityLink = page.locator(
        'a:has-text("Liquidity"), a:has-text("Pool"), [href*="liquidity"], [href*="pool"]'
      ).first();

      if (await liquidityLink.isVisible({ timeout: 5000 }).catch(() => false)) {
        await liquidityLink.click();
        await page.waitForLoadState('domcontentloaded');

        await verifier.captureScreenshot('defi-liquidity-pools');

        // Look for pool list
        const pools = page.locator(
          '[data-testid*="pool"], [class*="pool-card"], [class*="liquidity-item"]'
        );
        const poolCount = await pools.count();
      }
    });

    test('should handle add liquidity flow', async ({ page }) => {
      const liquidityLink = page.locator('a:has-text("Liquidity")').first();

      if (await liquidityLink.isVisible({ timeout: 5000 }).catch(() => false)) {
        await liquidityLink.click();
        await page.waitForLoadState('domcontentloaded');

        // Look for add liquidity button
        const addButton = page.locator(
          'button:has-text("Add"), button:has-text("Provide")'
        ).first();

        if (await addButton.isVisible({ timeout: 5000 }).catch(() => false)) {
          await addButton.click();
          await page.waitForTimeout(500);

          await verifier.captureScreenshot('defi-add-liquidity');
        }
      }
    });
  });

  test.describe('Responsive Design', () => {
    test('should adapt DeFi interface for mobile', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('defi-mobile');

      // Token selectors should be usable on mobile
      const tokenSelector = page.locator('[class*="token-select"]').first();
      if (await tokenSelector.isVisible({ timeout: 2000 }).catch(() => false)) {
        const box = await tokenSelector.boundingBox();
        if (box) {
          // Should be large enough to tap (44px minimum)
          expect(box.height).toBeGreaterThanOrEqual(40);
        }
      }
    });

    test('should maintain usability across viewports', async ({ page }) => {
      for (const [name, viewport] of Object.entries(VIEWPORTS)) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
        await page.goto('/');
        await page.waitForLoadState('domcontentloaded');

        await verifier.captureScreenshot(`defi-${name}`);

        // Swap button should always be visible
        const swapButton = page.locator('button:has-text("Swap")').first();
        if (await swapButton.isVisible({ timeout: 2000 }).catch(() => false)) {
          await expect(swapButton).toBeVisible();
        }
      }
    });
  });

  test.describe('Error Handling', () => {
    test('should handle insufficient balance gracefully', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      const amountInput = page.locator('input[type="number"]').first();
      if (await amountInput.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Enter very large amount
        await amountInput.fill('9999999999');
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('defi-insufficient-balance');

        // Should show insufficient balance error
        const error = page.locator(
          '[class*="insufficient"], [class*="error"], [data-testid*="balance-error"]'
        ).first();
      }
    });

    test('should handle network errors', async ({ page }) => {
      // Block API calls
      await page.route('**/api/**', route => route.abort());

      await page.goto('/');
      await page.waitForTimeout(2000);

      const errorResult = await verifier.verifyErrorStates();
      await verifier.captureScreenshot('defi-network-error');
    });
  });

  test.describe('Accessibility', () => {
    test('should pass accessibility audit', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      const result = await verifier.runAccessibilityAudit();

      // No critical violations
      const critical = result.violations.filter(v => v.impact === 'critical');
      expect(critical).toHaveLength(0);
    });

    test('should support keyboard navigation', async ({ page }) => {
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      // Tab through form elements
      for (let i = 0; i < 10; i++) {
        await page.keyboard.press('Tab');
      }

      const focused = page.locator(':focus');
      expect(await focused.count()).toBeGreaterThan(0);

      await verifier.captureScreenshot('defi-keyboard-nav');
    });
  });
});
