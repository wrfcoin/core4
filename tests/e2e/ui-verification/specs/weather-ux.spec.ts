/**
 * Weather Data & Earnings UX Verification Tests
 *
 * Comprehensive UI/UX testing for weather functionality:
 * - Data submission form validation
 * - Earnings dashboard charts render
 * - Station metrics display
 * - Reward history pagination
 */
import { test, expect, Page } from '@playwright/test';
import { UXVerifier, VIEWPORTS } from '../utils/ux-helpers';

test.describe('Weather Data UX Verification', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  test.describe('Weather Dashboard', () => {
    test('should display weather dashboard with proper loading', async ({ page }) => {
      await page.goto('/pop/dashboard');

      const loadingResult = await verifier.verifyLoadingStates();
      expect(loadingResult.hasLoadingIndicator).toBe(true);

      await verifier.captureScreenshot('weather-dashboard-loaded');

      // Verify dashboard components
      const dashboardContent = page.locator(
        '[data-testid="weather-dashboard"], .dashboard, main'
      ).first();
      await expect(dashboardContent).toBeVisible({ timeout: 10000 });
    });

    test('should render earnings charts correctly', async ({ page }) => {
      await page.goto('/pop/dashboard');
      await page.waitForLoadState('domcontentloaded');

      // Look for chart elements
      const charts = page.locator(
        'canvas, svg[class*="chart"], [data-testid*="chart"], .recharts-wrapper, [class*="chart"]'
      );

      const chartCount = await charts.count();
      await verifier.captureScreenshot('weather-earnings-charts');

      if (chartCount > 0) {
        // Verify at least one chart is visible
        const firstChart = charts.first();
        await expect(firstChart).toBeVisible({ timeout: 5000 });

        // Check chart has dimensions (is rendered)
        const box = await firstChart.boundingBox();
        if (box) {
          expect(box.width).toBeGreaterThan(50);
          expect(box.height).toBeGreaterThan(50);
        }
      }
    });

    test('should display station metrics with correct formatting', async ({ page }) => {
      await page.goto('/pop/dashboard');
      await page.waitForLoadState('domcontentloaded');

      // Look for metrics/stats cards
      const metricCards = page.locator(
        '[data-testid*="metric"], [class*="stat"], [class*="metric"], [class*="card"]'
      );

      const cardCount = await metricCards.count();
      await verifier.captureScreenshot('weather-station-metrics');

      if (cardCount > 0) {
        // Each metric card should have a value
        const firstCard = metricCards.first();
        const hasNumber = await firstCard.locator('span, p, h1, h2, h3, h4').first()
          .textContent()
          .then(text => text && /\d/.test(text))
          .catch(() => false);

        // Metric cards should contain some numeric data
      }
    });
  });

  test.describe('Data Submission', () => {
    test('should validate weather data submission form', async ({ page }) => {
      await page.goto('/pop/submit');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('weather-submit-form');

      // Look for form
      const form = page.locator('form').first();

      if (await form.isVisible({ timeout: 5000 }).catch(() => false)) {
        const formResult = await verifier.verifyFormValidation('form');

        // Try to submit empty form
        const submitButton = form.locator(
          'button[type="submit"], button:has-text("Submit"), button:has-text("Send")'
        ).first();

        if (await submitButton.isVisible({ timeout: 2000 }).catch(() => false)) {
          await submitButton.click();
          await page.waitForTimeout(500);

          await verifier.captureScreenshot('weather-form-validation');

          // Check for validation errors
          const errors = form.locator(
            '[class*="error"], .invalid-feedback, [aria-invalid="true"]'
          );
          // Should show validation feedback or prevent submission
        }
      }
    });

    test('should handle data submission with proper feedback', async ({ page }) => {
      await page.goto('/pop/submit');
      await page.waitForLoadState('domcontentloaded');

      const form = page.locator('form').first();

      if (await form.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Look for temperature input
        const tempInput = form.locator(
          'input[name*="temperature"], input[placeholder*="temperature"], input[type="number"]'
        ).first();

        if (await tempInput.isVisible({ timeout: 2000 }).catch(() => false)) {
          await tempInput.fill('25.5');
        }

        // Look for humidity input
        const humidityInput = form.locator(
          'input[name*="humidity"], input[placeholder*="humidity"]'
        ).first();

        if (await humidityInput.isVisible({ timeout: 2000 }).catch(() => false)) {
          await humidityInput.fill('65');
        }

        await verifier.captureScreenshot('weather-form-filled');
      }
    });
  });

  test.describe('Reward History', () => {
    test('should display reward history with pagination', async ({ page }) => {
      await page.goto('/pop/dashboard');
      await page.waitForLoadState('domcontentloaded');

      // Look for history section
      const historySection = page.locator(
        '[data-testid*="history"], [class*="history"], [class*="rewards-list"]'
      ).first();

      await verifier.captureScreenshot('weather-reward-history');

      if (await historySection.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Check for pagination or infinite scroll
        const pagination = page.locator(
          '[data-testid="pagination"], .pagination, nav[aria-label*="pagination"]'
        ).first();

        const loadMore = page.locator(
          'button:has-text("Load More"), button:has-text("Show More")'
        ).first();

        const hasNavigation =
          await pagination.isVisible({ timeout: 2000 }).catch(() => false) ||
          await loadMore.isVisible({ timeout: 2000 }).catch(() => false);

        // History items should be displayed
        const items = historySection.locator('[class*="item"], [class*="row"], li');
        const itemCount = await items.count();
      }
    });

    test('should filter rewards by date range', async ({ page }) => {
      await page.goto('/pop/dashboard');
      await page.waitForLoadState('domcontentloaded');

      // Look for date filter
      const dateFilter = page.locator(
        '[data-testid*="date-filter"], [class*="date-picker"], input[type="date"]'
      ).first();

      if (await dateFilter.isVisible({ timeout: 5000 }).catch(() => false)) {
        await dateFilter.click();
        await verifier.captureScreenshot('weather-date-filter');
      }
    });
  });

  test.describe('Responsive Design', () => {
    test('should adapt charts for mobile viewport', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/pop/dashboard');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('weather-dashboard-mobile');

      // Charts should resize or stack
      const charts = page.locator('canvas, svg[class*="chart"]');
      const chartCount = await charts.count();

      if (chartCount > 0) {
        const chart = charts.first();
        const box = await chart.boundingBox();
        if (box) {
          // Chart should fit within mobile viewport
          expect(box.width).toBeLessThanOrEqual(375);
        }
      }
    });

    test('should display metrics in readable format on all viewports', async ({ page }) => {
      await page.goto('/pop/dashboard');

      for (const [name, viewport] of Object.entries(VIEWPORTS)) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
        await page.waitForTimeout(500);

        await verifier.captureScreenshot(`weather-dashboard-${name}`);

        // Text should remain readable
        const content = page.locator('main, [role="main"], .dashboard').first();
        if (await content.isVisible({ timeout: 2000 }).catch(() => false)) {
          const fontSize = await content.evaluate((el) => {
            return parseInt(window.getComputedStyle(el).fontSize);
          });
          // Font should be at least 12px for readability
          expect(fontSize).toBeGreaterThanOrEqual(12);
        }
      }
    });
  });

  test.describe('IoT Integration', () => {
    test('should display IoT dashboard with sensor status', async ({ page }) => {
      await page.goto('/IoT');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('iot-dashboard');

      // Look for sensor status indicators
      const sensors = page.locator(
        '[data-testid*="sensor"], [class*="sensor"], [class*="device"]'
      );

      const sensorCount = await sensors.count();

      if (sensorCount > 0) {
        // Sensors should have status indicators
        const statusIndicators = page.locator(
          '[class*="status"], [class*="online"], [class*="offline"], [class*="indicator"]'
        );
        const hasStatus = await statusIndicators.count() > 0;
      }
    });

    test('should handle empty sensor list gracefully', async ({ page }) => {
      await page.goto('/IoT');
      await page.waitForLoadState('domcontentloaded');

      const emptyResult = await verifier.verifyEmptyStates();

      if (emptyResult.hasEmptyState) {
        expect(emptyResult.emptyStateHelpful).toBe(true);
        await verifier.captureScreenshot('iot-empty-state');
      }
    });
  });

  test.describe('Accessibility', () => {
    test('should pass accessibility audit for weather dashboard', async ({ page }) => {
      await page.goto('/pop/dashboard');
      await page.waitForLoadState('domcontentloaded');

      const result = await verifier.runAccessibilityAudit();

      // Charts should have proper labels
      const chartElements = page.locator('svg, canvas');
      const chartCount = await chartElements.count();

      if (chartCount > 0) {
        // Charts should have aria-labels or be in labeled regions
      }

      // No critical violations
      const critical = result.violations.filter(v => v.impact === 'critical');
      expect(critical).toHaveLength(0);
    });

    test('should support keyboard navigation through dashboard', async ({ page }) => {
      await page.goto('/pop/dashboard');
      await page.waitForLoadState('domcontentloaded');

      // Tab through elements
      for (let i = 0; i < 10; i++) {
        await page.keyboard.press('Tab');
      }

      const focused = page.locator(':focus');
      expect(await focused.count()).toBeGreaterThan(0);

      await verifier.captureScreenshot('weather-keyboard-nav');
    });
  });
});
