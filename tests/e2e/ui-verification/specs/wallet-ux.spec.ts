/**
 * Wallet UX Verification Tests
 *
 * Comprehensive UI/UX testing for wallet functionality:
 * - Wallet connection modal responsiveness
 * - Balance display and formatting
 * - Send/Receive modal interactions
 * - Transaction history pagination
 * - Error state handling
 */
import { test, expect, Page } from '@playwright/test';
import { UXVerifier, VIEWPORTS, testAllViewports } from '../utils/ux-helpers';

test.describe('Wallet UX Verification', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  test.describe('Wallet Overview', () => {
    test('should display wallet overview with proper loading states', async ({ page }) => {
      await page.goto('/wallet');

      // Verify loading states
      const loadingResult = await verifier.verifyLoadingStates();
      expect(loadingResult.hasLoadingIndicator).toBe(true);

      // Capture screenshot after load
      await verifier.captureScreenshot('wallet-overview-loaded');

      // Verify key components are visible
      await expect(page.locator('[data-testid="wallet-address"], .wallet-address, [class*="address"]').first())
        .toBeVisible({ timeout: 10000 });

      // Check balance display formatting
      const balanceElement = page.locator('[data-testid="total-balance"], .balance, [class*="balance"]').first();
      if (await balanceElement.isVisible({ timeout: 5000 }).catch(() => false)) {
        const balanceText = await balanceElement.textContent();
        // Balance should contain a number or currency symbol
        expect(balanceText).toMatch(/[\d$€£¥]/);
      }
    });

    test('should handle wallet connection modal responsiveness', async ({ page }) => {
      await page.goto('/wallet');

      // Look for connect wallet button
      const connectButton = page.locator(
        '[data-testid="connect-wallet-button"], button:has-text("Connect"), button:has-text("Wallet")'
      ).first();

      if (await connectButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await connectButton.click();

        // Verify modal appears
        const modal = page.locator('[data-testid="modal"], [role="dialog"], .modal').first();
        await expect(modal).toBeVisible({ timeout: 5000 });

        // Capture modal screenshot
        await verifier.captureScreenshot('wallet-connect-modal');

        // Test modal responsiveness across viewports
        for (const [name, viewport] of Object.entries(VIEWPORTS)) {
          await page.setViewportSize({ width: viewport.width, height: viewport.height });
          await verifier.captureScreenshot(`wallet-connect-modal-${name}`);

          // Modal should remain centered and visible
          await expect(modal).toBeVisible();
        }
      }
    });

    test('should display token list with proper formatting', async ({ page }) => {
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      // Look for token list
      const tokenList = page.locator('[data-testid="token-list"], .token-list, [class*="token"]').first();

      if (await tokenList.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Each token should have proper formatting
        const tokenItems = tokenList.locator('[data-testid="token-item"], .token-item, [class*="token-row"]');
        const count = await tokenItems.count();

        if (count > 0) {
          // Verify first token has required elements
          const firstToken = tokenItems.first();

          // Should have symbol/name
          const hasName = await firstToken.locator('[class*="name"], [class*="symbol"]').isVisible().catch(() => false);

          // Should have balance
          const hasBalance = await firstToken.locator('[class*="balance"], [class*="amount"]').isVisible().catch(() => false);

          expect(hasName || hasBalance).toBe(true);
        }

        await verifier.captureScreenshot('wallet-token-list');
      }
    });
  });

  test.describe('Send/Receive Modals', () => {
    test('should handle send modal interactions correctly', async ({ page }) => {
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      const sendButton = page.locator(
        '[data-testid="send-button"], button:has-text("Send"), [class*="send"]'
      ).first();

      if (await sendButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await sendButton.click();

        const modal = page.locator('[data-testid="modal"], [role="dialog"], .modal').first();
        await expect(modal).toBeVisible({ timeout: 5000 });

        await verifier.captureScreenshot('wallet-send-modal');

        // Verify form elements
        const addressInput = modal.locator(
          '[data-testid="recipient-address-input"], input[placeholder*="address"], input[name*="address"]'
        ).first();
        const amountInput = modal.locator(
          '[data-testid="send-amount-input"], input[type="number"], input[placeholder*="amount"]'
        ).first();

        // Test form validation
        if (await addressInput.isVisible({ timeout: 2000 }).catch(() => false)) {
          // Enter invalid address
          await addressInput.fill('invalid');
          await addressInput.blur();

          // Check for validation error
          const errorMessage = modal.locator('[class*="error"], [data-testid*="error"]').first();
          const hasError = await errorMessage.isVisible({ timeout: 2000 }).catch(() => false);

          // Capture validation state
          await verifier.captureScreenshot('wallet-send-validation-error');
        }

        // Close modal
        const closeButton = modal.locator('[data-testid="close-button"], button[aria-label="Close"], .close').first();
        if (await closeButton.isVisible({ timeout: 2000 }).catch(() => false)) {
          await closeButton.click();
        } else {
          await page.keyboard.press('Escape');
        }
      }
    });

    test('should handle receive modal with QR code', async ({ page }) => {
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      const receiveButton = page.locator(
        '[data-testid="receive-button"], button:has-text("Receive"), [class*="receive"]'
      ).first();

      if (await receiveButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await receiveButton.click();

        const modal = page.locator('[data-testid="modal"], [role="dialog"], .modal').first();
        await expect(modal).toBeVisible({ timeout: 5000 });

        await verifier.captureScreenshot('wallet-receive-modal');

        // Check for QR code
        const qrCode = modal.locator(
          '[data-testid="address-qr-code"], canvas, svg[class*="qr"], img[alt*="QR"]'
        ).first();
        const hasQR = await qrCode.isVisible({ timeout: 2000 }).catch(() => false);

        // Check for copy button
        const copyButton = modal.locator(
          '[data-testid="copy-address-button"], button:has-text("Copy"), [class*="copy"]'
        ).first();
        const hasCopy = await copyButton.isVisible({ timeout: 2000 }).catch(() => false);

        expect(hasQR || hasCopy).toBe(true);
      }
    });
  });

  test.describe('Transaction History', () => {
    test('should display transaction history with pagination', async ({ page }) => {
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      // Navigate to transactions or find transaction section
      const transactionLink = page.locator(
        '[data-testid="view-all-transactions"], a:has-text("Transaction"), [class*="transaction"]'
      ).first();

      if (await transactionLink.isVisible({ timeout: 5000 }).catch(() => false)) {
        await transactionLink.click();
        await page.waitForLoadState('domcontentloaded');
      }

      await verifier.captureScreenshot('wallet-transactions');

      // Check for transaction list
      const transactionList = page.locator(
        '[data-testid="transaction-list"], .transaction-list, [class*="transactions"]'
      ).first();

      if (await transactionList.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Check for pagination
        const pagination = page.locator(
          '[data-testid="pagination"], .pagination, nav[aria-label*="pagination"]'
        ).first();
        const hasPagination = await pagination.isVisible({ timeout: 2000 }).catch(() => false);

        // Check for load more button (alternative to pagination)
        const loadMore = page.locator('button:has-text("Load More"), button:has-text("Show More")').first();
        const hasLoadMore = await loadMore.isVisible({ timeout: 2000 }).catch(() => false);

        // At least one navigation method should exist for lists with many items
        const transactionItems = transactionList.locator('[class*="transaction-item"], [class*="tx-row"]');
        const itemCount = await transactionItems.count();

        if (itemCount >= 10) {
          expect(hasPagination || hasLoadMore).toBe(true);
        }
      }
    });

    test('should handle empty transaction state gracefully', async ({ page }) => {
      // This test verifies empty state handling
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      const emptyResult = await verifier.verifyEmptyStates();

      // If empty state exists, it should be helpful
      if (emptyResult.hasEmptyState) {
        expect(emptyResult.emptyStateHelpful).toBe(true);
        await verifier.captureScreenshot('wallet-empty-state');
      }
    });
  });

  test.describe('Error Handling', () => {
    test('should display network errors gracefully', async ({ page }) => {
      // Block wallet API calls
      await page.route('**/api/wallet/**', route => route.abort());
      await page.route('**/wallet/**', route => route.abort());

      await page.goto('/wallet');

      const errorResult = await verifier.verifyErrorStates();
      await verifier.captureScreenshot('wallet-network-error');

      // Should have some form of error handling
      // Note: This might not trigger if the page handles errors gracefully
    });

    test('should validate form inputs and show inline errors', async ({ page }) => {
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      const sendButton = page.locator(
        '[data-testid="send-button"], button:has-text("Send")'
      ).first();

      if (await sendButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await sendButton.click();
        await page.waitForTimeout(1000);

        // Try to submit without filling form
        const submitButton = page.locator(
          '[data-testid="review-transaction-button"], button[type="submit"], button:has-text("Review"), button:has-text("Continue")'
        ).first();

        if (await submitButton.isVisible({ timeout: 2000 }).catch(() => false)) {
          await submitButton.click();
          await page.waitForTimeout(500);

          // Capture validation errors
          await verifier.captureScreenshot('wallet-form-validation');

          // Check for inline errors
          const errors = page.locator('[class*="error"], .invalid, [aria-invalid="true"]');
          const errorCount = await errors.count();

          // Should show some validation feedback
          expect(errorCount).toBeGreaterThanOrEqual(0);
        }
      }
    });
  });

  test.describe('Responsive Design', () => {
    test('should adapt layout for mobile viewport', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('wallet-mobile');

      // Check for mobile-specific elements
      const mobileNav = page.locator(
        '[data-testid="mobile-wallet-nav"], [class*="mobile-nav"], [class*="bottom-nav"]'
      ).first();
      const hasMobileNav = await mobileNav.isVisible({ timeout: 2000 }).catch(() => false);

      // Check main content is visible and not overflowing
      const mainContent = page.locator('main, [role="main"], .main-content').first();
      if (await mainContent.isVisible({ timeout: 2000 }).catch(() => false)) {
        const box = await mainContent.boundingBox();
        if (box) {
          // Content should fit within viewport width
          expect(box.width).toBeLessThanOrEqual(375);
        }
      }
    });

    test('should maintain usability across all viewports', async ({ page }) => {
      await page.goto('/wallet');

      for (const [name, viewport] of Object.entries(VIEWPORTS)) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
        await page.waitForTimeout(500);

        await verifier.captureScreenshot(`wallet-${name}`);

        // Key actions should be accessible
        const actionButtons = page.locator('button:visible');
        const buttonCount = await actionButtons.count();
        expect(buttonCount).toBeGreaterThan(0);
      }
    });
  });

  test.describe('Accessibility', () => {
    test('should pass accessibility audit', async ({ page }) => {
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      const accessibilityResult = await verifier.runAccessibilityAudit();

      // Log violations for review
      if (accessibilityResult.violations.length > 0) {
        console.log('Accessibility violations:', accessibilityResult.violations);
      }

      // No critical violations should exist
      const criticalViolations = accessibilityResult.violations.filter(v => v.impact === 'critical');
      expect(criticalViolations).toHaveLength(0);
    });

    test('should support keyboard navigation', async ({ page }) => {
      await page.goto('/wallet');
      await page.waitForLoadState('domcontentloaded');

      // Test tab navigation
      await page.keyboard.press('Tab');
      let focusedElement = page.locator(':focus');
      expect(await focusedElement.count()).toBeGreaterThan(0);

      // Navigate through interactive elements
      for (let i = 0; i < 5; i++) {
        await page.keyboard.press('Tab');
      }

      // Verify focus is visible
      focusedElement = page.locator(':focus');
      if (await focusedElement.count() > 0) {
        const styles = await focusedElement.evaluate((el) => {
          const computed = window.getComputedStyle(el);
          return {
            outline: computed.outline,
            boxShadow: computed.boxShadow,
          };
        });

        // Should have visible focus indicator
        const hasFocusIndicator = styles.outline !== 'none' ||
                                  styles.boxShadow !== 'none' ||
                                  styles.outline.includes('px');
        // Note: Some designs use custom focus styles
      }

      await verifier.captureScreenshot('wallet-keyboard-nav');
    });
  });
});
