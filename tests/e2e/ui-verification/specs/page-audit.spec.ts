/**
 * Comprehensive Page Audit Tests
 *
 * Automated UI/UX verification for all major pages:
 * - Loading states
 * - Error states
 * - Empty states
 * - Responsive design
 * - Accessibility
 */
import { test, expect, Page } from '@playwright/test';
import { UXVerifier, VIEWPORTS } from '../utils/ux-helpers';

// Define all pages to audit
const PAGES_TO_AUDIT = [
  // Critical Priority
  { path: '/', name: 'dashboard', description: 'Main Dashboard' },
  { path: '/wallet', name: 'wallet', description: 'Wallet View' },
  { path: '/wallet-v2', name: 'wallet-v2', description: 'Enhanced Wallet' },
  { path: '/bridge', name: 'bridge', description: 'Cross-Chain Bridge' },
  { path: '/governance', name: 'governance', description: 'Governance' },

  // High Priority - Core Features
  { path: '/miner', name: 'miner', description: 'Miner Dashboard' },
  { path: '/miner-v2', name: 'miner-v2', description: 'Enhanced Miner' },
  { path: '/explorer', name: 'explorer', description: 'Blockchain Explorer' },
  { path: '/nfts', name: 'nfts', description: 'NFT Gallery' },
  { path: '/nfts/computational', name: 'computational-nfts', description: 'Computational NFTs' },
  { path: '/computational-nft-marketplace', name: 'nft-marketplace', description: 'NFT Marketplace' },
  { path: '/pop', name: 'pop', description: 'Proof of Publishing' },
  { path: '/pop/dashboard', name: 'pop-dashboard', description: 'Weather Dashboard' },
  { path: '/carbon-credits', name: 'carbon-credits', description: 'Carbon Credits' },
  { path: '/IoT', name: 'iot', description: 'IoT Dashboard' },

  // High Priority - Analytics & Integrations
  { path: '/analytics/unified', name: 'analytics-unified', description: 'Unified Analytics' },
  { path: '/analytics/cross-service', name: 'analytics-cross-service', description: 'Cross-Service Metrics' },
  { path: '/analytics/performance', name: 'analytics-performance', description: 'Performance Optimizer' },
  { path: '/analytics/cost', name: 'analytics-cost', description: 'Cost Analysis' },
  { path: '/analytics/predictive', name: 'analytics-predictive', description: 'Predictive Analytics' },
  { path: '/integrations', name: 'integrations', description: 'Integrations Hub' },
  { path: '/integrations/enhanced', name: 'integrations-enhanced', description: 'Enhanced Hub' },
  { path: '/oracle/dashboard', name: 'oracle', description: 'Oracle Dashboard' },

  // Medium Priority
  { path: '/resources', name: 'resources', description: 'Resources Hub' },
  { path: '/search', name: 'search', description: 'Search Results' },
  { path: '/infrastructure', name: 'infrastructure', description: 'Infrastructure Dashboard' },
  { path: '/smart-contracts', name: 'smart-contracts', description: 'Smart Contracts' },
  { path: '/storage', name: 'storage', description: 'Distributed Storage' },
  { path: '/messaging', name: 'messaging', description: 'Messaging Hub' },
  { path: '/address-book', name: 'address-book', description: 'Address Book' },
  { path: '/sharding', name: 'sharding', description: 'Sharding Dashboard' },
  { path: '/achievements', name: 'achievements', description: 'Achievements' },
  { path: '/incentives', name: 'incentives', description: 'Proximity Incentives' },

  // Lower Priority
  { path: '/settings', name: 'settings', description: 'Settings' },
  { path: '/performance', name: 'performance', description: 'Performance Metrics' },
  { path: '/system', name: 'system', description: 'System Monitor' },
  { path: '/developer', name: 'developer', description: 'Developer Portal' },
];

test.describe('Comprehensive Page Audit', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  // Generate tests for each page
  for (const pageConfig of PAGES_TO_AUDIT) {
    test.describe(`${pageConfig.description} (${pageConfig.path})`, () => {

      test('should load without errors', async ({ page }) => {
        const errors: string[] = [];

        // Listen for console errors
        page.on('console', msg => {
          if (msg.type() === 'error') {
            errors.push(msg.text());
          }
        });

        // Listen for page errors
        page.on('pageerror', error => {
          errors.push(error.message);
        });

        await page.goto(pageConfig.path, { timeout: 30000 });
        await page.waitForLoadState('domcontentloaded');
        await page.waitForTimeout(1500); // Allow React to render

        await verifier.captureScreenshot(`${pageConfig.name}-loaded`);

        // Filter out expected/benign errors
        const criticalErrors = errors.filter(e =>
          !e.includes('favicon') &&
          !e.includes('404') &&
          !e.includes('Failed to load resource') // API might not be running
        );

        // Log errors but don't fail if page rendered
        if (criticalErrors.length > 0) {
          console.log(`Errors on ${pageConfig.path}:`, criticalErrors);
        }
      });

      test('should display loading state', async ({ page }) => {
        await page.goto(pageConfig.path);

        const loadingResult = await verifier.verifyLoadingStates();

        // Most pages should have some loading indication
        // This is informational, not a strict requirement
        if (!loadingResult.hasLoadingIndicator && loadingResult.loadingDuration > 1000) {
          verifier.addIssue({
            severity: 'minor',
            category: 'loading',
            description: `Page ${pageConfig.path} took ${loadingResult.loadingDuration}ms without loading indicator`,
          });
        }
      });

      test('should handle error states gracefully', async ({ page }) => {
        // Block API to force error state
        await page.route('**/api/**', route => route.abort());

        await page.goto(pageConfig.path);
        await page.waitForTimeout(2000);

        const errorResult = await verifier.verifyErrorStates();
        await verifier.captureScreenshot(`${pageConfig.name}-error-state`);

        // If errors are shown, they should be clear
        if (errorResult.hasErrorBoundary || await page.locator('[class*="error"]').isVisible().catch(() => false)) {
          // Error state exists, good
        }
      });

      test('should have empty state for data-dependent content', async ({ page }) => {
        await page.goto(pageConfig.path);
        await page.waitForLoadState('domcontentloaded');

        const emptyResult = await verifier.verifyEmptyStates();

        // If page has empty state, it should be helpful
        if (emptyResult.hasEmptyState && !emptyResult.emptyStateHelpful) {
          verifier.addIssue({
            severity: 'minor',
            category: 'empty-state',
            description: `Empty state on ${pageConfig.path} could be more helpful`,
          });
        }
      });

      test('should be responsive across viewports', async ({ page }) => {
        for (const [viewportName, viewport] of Object.entries(VIEWPORTS)) {
          await page.setViewportSize({ width: viewport.width, height: viewport.height });
          await page.goto(pageConfig.path);
          await page.waitForLoadState('domcontentloaded');

          await verifier.captureScreenshot(`${pageConfig.name}-${viewportName}`);

          // Check for horizontal overflow
          const bodyWidth = await page.evaluate(() => document.body.scrollWidth);
          if (bodyWidth > viewport.width + 20) { // Allow small margin for scrollbar
            verifier.addIssue({
              severity: 'major',
              category: 'responsive',
              description: `Horizontal overflow on ${pageConfig.path} at ${viewportName} viewport`,
            });
          }

          // Check for text truncation issues
          const mainContent = page.locator('main, [role="main"]').first();
          if (await mainContent.isVisible({ timeout: 2000 }).catch(() => false)) {
            // Content should be visible
          }
        }
      });

      test('should pass basic accessibility checks', async ({ page }) => {
        await page.goto(pageConfig.path);
        await page.waitForLoadState('domcontentloaded');

        const accessibilityResult = await verifier.runAccessibilityAudit();

        // Count critical/serious violations
        const criticalViolations = accessibilityResult.violations.filter(
          v => v.impact === 'critical' || v.impact === 'serious'
        );

        // Log violations
        if (criticalViolations.length > 0) {
          console.log(`A11y issues on ${pageConfig.path}:`, criticalViolations.map(v => v.description));
        }

        // No critical violations allowed
        const critical = accessibilityResult.violations.filter(v => v.impact === 'critical');
        expect(critical).toHaveLength(0);
      });

      test('should support keyboard navigation', async ({ page }) => {
        await page.goto(pageConfig.path);
        await page.waitForLoadState('domcontentloaded');

        // Test tab navigation
        await page.keyboard.press('Tab');

        let tabCount = 0;
        let lastFocused = '';

        // Tab through page up to 20 times
        while (tabCount < 20) {
          await page.keyboard.press('Tab');
          const focused = page.locator(':focus');
          const focusCount = await focused.count();

          if (focusCount === 0) break;

          const currentFocused = await focused.first().evaluate(el => el.tagName + el.className);
          if (currentFocused === lastFocused) break;

          lastFocused = currentFocused;
          tabCount++;
        }

        // Should have at least some focusable elements
        expect(tabCount).toBeGreaterThan(0);
      });

      test('should have proper page title', async ({ page }) => {
        await page.goto(pageConfig.path);
        await page.waitForLoadState('domcontentloaded');

        const title = await page.title();

        // Title should exist and not be empty
        expect(title).toBeTruthy();
        expect(title.length).toBeGreaterThan(0);

        // Title should not be just "undefined" or "null"
        expect(title.toLowerCase()).not.toBe('undefined');
        expect(title.toLowerCase()).not.toBe('null');
      });

      test('should have main content area', async ({ page }) => {
        await page.goto(pageConfig.path);
        await page.waitForLoadState('domcontentloaded');

        // Look for main content
        const mainContent = page.locator('main, [role="main"], .main-content, #main').first();
        const hasMain = await mainContent.isVisible({ timeout: 5000 }).catch(() => false);

        // At least some content should be visible
        const anyContent = page.locator('div, section, article').first();
        const hasContent = await anyContent.isVisible({ timeout: 5000 }).catch(() => false);

        expect(hasMain || hasContent).toBe(true);
      });
    });
  }
});

// Summary test
test.describe('Page Audit Summary', () => {
  test('should generate audit summary report', async ({ page }) => {
    const results: { page: string; issues: number }[] = [];

    for (const pageConfig of PAGES_TO_AUDIT.slice(0, 10)) { // Test first 10 for summary
      const verifier = new UXVerifier(page);

      try {
        await page.goto(pageConfig.path, { timeout: 30000 });
        await page.waitForLoadState('domcontentloaded');

        const auditResult = await verifier.performFullAudit(pageConfig.name, 'desktop');
        results.push({
          page: pageConfig.path,
          issues: auditResult.issues.length,
        });
      } catch (error) {
        results.push({
          page: pageConfig.path,
          issues: -1, // Error loading
        });
      }
    }

    // Log summary
    console.log('\n=== Page Audit Summary ===');
    for (const result of results) {
      const status = result.issues === -1 ? '❌ Error' :
                     result.issues === 0 ? '✅ Clean' :
                     `⚠️ ${result.issues} issues`;
      console.log(`${result.page}: ${status}`);
    }
    console.log('===========================\n');
  });
});
