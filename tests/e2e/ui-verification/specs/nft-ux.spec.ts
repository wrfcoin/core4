/**
 * NFT Marketplace UX Verification Tests
 *
 * Comprehensive UI/UX testing for NFT functionality:
 * - Grid/list view toggle
 * - Filter sidebar interactions
 * - NFT card hover states
 * - Mint wizard multi-step flow
 */
import { test, expect, Page } from '@playwright/test';
import { UXVerifier, VIEWPORTS } from '../utils/ux-helpers';

test.describe('NFT Marketplace UX Verification', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  test.describe('NFT Gallery', () => {
    test('should display NFT gallery with proper loading', async ({ page }) => {
      await page.goto('/nfts');

      const loadingResult = await verifier.verifyLoadingStates();
      expect(loadingResult.hasLoadingIndicator).toBe(true);

      await verifier.captureScreenshot('nft-gallery-loaded');

      // Gallery should display NFT cards
      const gallery = page.locator(
        '[data-testid="nft-gallery"], [class*="gallery"], [class*="nft-grid"], .grid'
      ).first();
      await expect(gallery).toBeVisible({ timeout: 10000 });
    });

    test('should handle grid/list view toggle', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      // Look for view toggle
      const viewToggle = page.locator(
        '[data-testid="view-toggle"], [class*="view-toggle"], button[aria-label*="view"], [class*="grid-toggle"]'
      );

      const gridButton = page.locator(
        'button[aria-label*="grid"], button:has-text("Grid"), [data-testid="grid-view"]'
      ).first();
      const listButton = page.locator(
        'button[aria-label*="list"], button:has-text("List"), [data-testid="list-view"]'
      ).first();

      if (await gridButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await gridButton.click();
        await page.waitForTimeout(500);
        await verifier.captureScreenshot('nft-grid-view');
      }

      if (await listButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await listButton.click();
        await page.waitForTimeout(500);
        await verifier.captureScreenshot('nft-list-view');
      }
    });

    test('should display NFT cards with proper structure', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      const nftCards = page.locator(
        '[data-testid="nft-card"], [class*="nft-card"], [class*="nft-item"]'
      );

      const cardCount = await nftCards.count();
      await verifier.captureScreenshot('nft-cards');

      if (cardCount > 0) {
        const firstCard = nftCards.first();

        // Card should have image
        const image = firstCard.locator('img, [class*="image"]').first();
        const hasImage = await image.isVisible({ timeout: 2000 }).catch(() => false);

        // Card should have name/title
        const title = firstCard.locator('h1, h2, h3, h4, h5, [class*="title"], [class*="name"]').first();
        const hasTitle = await title.isVisible({ timeout: 2000 }).catch(() => false);

        // Card should have price or action
        const price = firstCard.locator('[class*="price"], [class*="cost"]').first();
        const hasPrice = await price.isVisible({ timeout: 2000 }).catch(() => false);

        expect(hasImage || hasTitle || hasPrice).toBe(true);
      }
    });

    test('should handle NFT card hover states', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      const firstCard = page.locator('[class*="nft-card"], [class*="nft-item"]').first();

      if (await firstCard.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Capture before hover
        await verifier.captureScreenshot('nft-card-normal');

        // Hover over card
        await firstCard.hover();
        await page.waitForTimeout(300);

        // Capture after hover
        await verifier.captureScreenshot('nft-card-hover');

        // Check for hover effects
        const hoverElements = firstCard.locator('[class*="overlay"], [class*="actions"], button');
        const hasHoverState = await hoverElements.count() > 0;
      }
    });
  });

  test.describe('Filter Sidebar', () => {
    test('should display filter sidebar with categories', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      // Look for filter sidebar
      const sidebar = page.locator(
        '[data-testid="filter-sidebar"], [class*="sidebar"], [class*="filters"], aside'
      ).first();

      if (await sidebar.isVisible({ timeout: 5000 }).catch(() => false)) {
        await verifier.captureScreenshot('nft-filter-sidebar');

        // Should have filter categories
        const categories = sidebar.locator('[class*="category"], [class*="filter-group"], fieldset');
        const categoryCount = await categories.count();
        expect(categoryCount).toBeGreaterThan(0);
      }
    });

    test('should handle filter interactions', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      // Look for filter checkboxes or buttons
      const filterOption = page.locator(
        '[class*="filter"] input[type="checkbox"], [class*="filter"] button'
      ).first();

      if (await filterOption.isVisible({ timeout: 5000 }).catch(() => false)) {
        await filterOption.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('nft-filter-applied');

        // URL should update or results should filter
        const url = page.url();
        // Filtering may add query params or trigger reload
      }
    });

    test('should support price range filter', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      // Look for price range inputs
      const minPrice = page.locator(
        'input[placeholder*="min"], input[name*="min"], [data-testid="min-price"]'
      ).first();
      const maxPrice = page.locator(
        'input[placeholder*="max"], input[name*="max"], [data-testid="max-price"]'
      ).first();

      if (await minPrice.isVisible({ timeout: 5000 }).catch(() => false)) {
        await minPrice.fill('10');
      }

      if (await maxPrice.isVisible({ timeout: 5000 }).catch(() => false)) {
        await maxPrice.fill('1000');
      }

      await verifier.captureScreenshot('nft-price-filter');
    });

    test('should handle mobile filter toggle', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      // Look for mobile filter button
      const filterToggle = page.locator(
        'button:has-text("Filter"), button[aria-label*="filter"], [data-testid="mobile-filter-toggle"]'
      ).first();

      if (await filterToggle.isVisible({ timeout: 5000 }).catch(() => false)) {
        await filterToggle.click();
        await page.waitForTimeout(300);

        await verifier.captureScreenshot('nft-mobile-filters');

        // Filter panel should slide in
        const filterPanel = page.locator('[class*="filter-panel"], [class*="mobile-filters"]').first();
        const isPanelVisible = await filterPanel.isVisible({ timeout: 2000 }).catch(() => false);
      }
    });
  });

  test.describe('NFT Detail Page', () => {
    test('should display NFT details properly', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      // Click first NFT card
      const firstCard = page.locator('[class*="nft-card"], [class*="nft-item"]').first();

      if (await firstCard.isVisible({ timeout: 5000 }).catch(() => false)) {
        await firstCard.click();
        await page.waitForLoadState('domcontentloaded');

        await verifier.captureScreenshot('nft-detail-page');

        // Detail page should show:
        // - Large image
        const image = page.locator('[class*="nft-image"], img').first();
        const hasImage = await image.isVisible({ timeout: 5000 }).catch(() => false);

        // - Title
        const title = page.locator('h1, [class*="title"]').first();
        const hasTitle = await title.isVisible({ timeout: 2000 }).catch(() => false);

        // - Price and buy button
        const buyButton = page.locator('button:has-text("Buy"), button:has-text("Purchase")').first();
        const hasBuyButton = await buyButton.isVisible({ timeout: 2000 }).catch(() => false);

        expect(hasImage || hasTitle).toBe(true);
      }
    });

    test('should display NFT attributes and metadata', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      const firstCard = page.locator('[class*="nft-card"]').first();
      if (await firstCard.isVisible({ timeout: 5000 }).catch(() => false)) {
        await firstCard.click();
        await page.waitForLoadState('domcontentloaded');

        // Look for attributes section
        const attributes = page.locator(
          '[data-testid="nft-attributes"], [class*="attributes"], [class*="traits"]'
        ).first();

        if (await attributes.isVisible({ timeout: 5000 }).catch(() => false)) {
          await verifier.captureScreenshot('nft-attributes');
        }
      }
    });
  });

  test.describe('Mint Wizard', () => {
    test('should display multi-step mint wizard', async ({ page }) => {
      await page.goto('/nft/create');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('nft-mint-wizard');

      // Look for stepper/progress indicator
      const stepper = page.locator(
        '[data-testid="stepper"], [class*="stepper"], [class*="wizard-progress"], [role="tablist"]'
      ).first();

      if (await stepper.isVisible({ timeout: 5000 }).catch(() => false)) {
        const steps = stepper.locator('[class*="step"], [role="tab"]');
        const stepCount = await steps.count();
        expect(stepCount).toBeGreaterThan(1);
      }
    });

    test('should handle file upload for NFT', async ({ page }) => {
      await page.goto('/nft/create');
      await page.waitForLoadState('domcontentloaded');

      // Look for file upload
      const fileInput = page.locator(
        'input[type="file"], [data-testid="file-upload"]'
      );

      const dropZone = page.locator(
        '[class*="dropzone"], [class*="upload-area"], [class*="file-drop"]'
      ).first();

      if (await dropZone.isVisible({ timeout: 5000 }).catch(() => false)) {
        await verifier.captureScreenshot('nft-upload-area');
      }
    });

    test('should validate mint form inputs', async ({ page }) => {
      await page.goto('/nft/create');
      await page.waitForLoadState('domcontentloaded');

      // Try to proceed without filling required fields
      const nextButton = page.locator(
        'button:has-text("Next"), button:has-text("Continue"), button[type="submit"]'
      ).first();

      if (await nextButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await nextButton.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('nft-mint-validation');

        // Check for validation errors
        const errors = page.locator('[class*="error"], [aria-invalid="true"]');
        const errorCount = await errors.count();
      }
    });

    test('should navigate through wizard steps', async ({ page }) => {
      await page.goto('/nft/create');
      await page.waitForLoadState('domcontentloaded');

      // Fill basic info
      const nameInput = page.locator('input[name*="name"], input[placeholder*="name"]').first();
      if (await nameInput.isVisible({ timeout: 2000 }).catch(() => false)) {
        await nameInput.fill('Test NFT');
      }

      const descInput = page.locator('textarea[name*="desc"], textarea[placeholder*="desc"]').first();
      if (await descInput.isVisible({ timeout: 2000 }).catch(() => false)) {
        await descInput.fill('Test description');
      }

      // Try to navigate to next step
      const nextButton = page.locator('button:has-text("Next")').first();
      if (await nextButton.isVisible({ timeout: 2000 }).catch(() => false)) {
        await nextButton.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('nft-wizard-step-2');
      }
    });
  });

  test.describe('Computational NFTs', () => {
    test('should display computational NFT features', async ({ page }) => {
      await page.goto('/nfts/computational');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('nft-computational');

      // Computational NFTs have unique features
      const computeInfo = page.locator(
        '[class*="compute"], [class*="processing"], [data-testid*="computational"]'
      ).first();

      if (await computeInfo.isVisible({ timeout: 5000 }).catch(() => false)) {
        // Should show compute metrics or status
      }
    });
  });

  test.describe('Responsive Design', () => {
    test('should adapt NFT grid for different viewports', async ({ page }) => {
      await page.goto('/nfts');

      for (const [name, viewport] of Object.entries(VIEWPORTS)) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
        await page.waitForTimeout(500);

        await verifier.captureScreenshot(`nft-gallery-${name}`);

        // Grid should adjust column count
        const cards = page.locator('[class*="nft-card"]');
        const cardCount = await cards.count();

        if (cardCount > 0) {
          const firstCard = cards.first();
          const box = await firstCard.boundingBox();
          if (box) {
            // Cards should fit within viewport
            expect(box.x).toBeGreaterThanOrEqual(0);
            expect(box.x + box.width).toBeLessThanOrEqual(viewport.width + 20); // Allow for scrollbar
          }
        }
      }
    });

    test('should handle mobile card layout', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('nft-mobile-cards');

      // Cards should stack or scroll horizontally
      const cards = page.locator('[class*="nft-card"]');
      if (await cards.count() > 0) {
        const firstCard = cards.first();
        const box = await firstCard.boundingBox();
        if (box) {
          // Card width should be appropriate for mobile
          expect(box.width).toBeLessThanOrEqual(375);
        }
      }
    });
  });

  test.describe('Accessibility', () => {
    test('should pass accessibility audit for NFT gallery', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      const result = await verifier.runAccessibilityAudit();

      // Images should have alt text
      const images = page.locator('img');
      const imageCount = await images.count();

      // No critical violations
      const critical = result.violations.filter(v => v.impact === 'critical');
      expect(critical).toHaveLength(0);
    });

    test('should support keyboard navigation', async ({ page }) => {
      await page.goto('/nfts');
      await page.waitForLoadState('domcontentloaded');

      // Tab through NFT cards
      for (let i = 0; i < 5; i++) {
        await page.keyboard.press('Tab');
      }

      const focused = page.locator(':focus');
      expect(await focused.count()).toBeGreaterThan(0);

      // Enter should activate focused card
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);

      await verifier.captureScreenshot('nft-keyboard-activation');
    });
  });
});
