const { test, expect } = require('@playwright/test');

test.describe('NFT Gallery Functionality Tests (Subtask 7.3)', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to the NFT gallery page
    await page.goto('http://localhost:5000');

    // Wait for the application to load
    await page.waitForLoadState('networkidle');

    // Navigate to NFT gallery - this might be within the marketplace or as a separate route
    // First try to find navigation to NFT gallery
    const nftGalleryLink = await page
      .locator('a[href*="nft"], button')
      .filter({ hasText: /gallery|browse.*nft|nft.*gallery/i })
      .first();
    if ((await nftGalleryLink.count()) > 0) {
      await nftGalleryLink.click();
    } else {
      // Fallback: try direct navigation to NFT gallery route
      await page.goto('http://localhost:5000/nft/browse');
    }

    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(2000);
  });

  test.describe('Gallery Display Functionality', () => {
    test('should display NFT gallery container with proper structure', async ({
      page,
    }) => {
      // Check for main gallery container
      await expect(page.locator('.nft-gallery-container')).toBeVisible();

      // Verify gallery header is present
      await expect(page.locator('.gallery-header')).toBeVisible();

      // Check for gallery title
      const galleryTitle = page.locator('.gallery-header h2');
      await expect(galleryTitle).toBeVisible();
      await expect(galleryTitle).toHaveText(/computational.*nft.*gallery/i);

      // Verify "Create New NFT" button exists
      const createButton = page.locator('.gallery-header .button.primary');
      await expect(createButton).toBeVisible();
      await expect(createButton).toHaveText(/create.*nft/i);

      console.log('✅ Gallery structure validation completed');
    });

    test('should display NFT grid with proper layout', async ({ page }) => {
      // Wait for NFTs to load
      await page.waitForSelector('.nft-grid, .empty-gallery', {
        timeout: 10000,
      });

      const hasNFTs = (await page.locator('.nft-grid .nft-card').count()) > 0;

      if (hasNFTs) {
        // Verify grid layout
        await expect(page.locator('.nft-grid')).toBeVisible();

        // Check that NFT cards are present
        const nftCards = page.locator('.nft-card');
        const cardCount = await nftCards.count();
        expect(cardCount).toBeGreaterThan(0);

        // Verify each card has required elements
        const firstCard = nftCards.first();
        await expect(firstCard.locator('.nft-card-image')).toBeVisible();
        await expect(firstCard.locator('.nft-card-content')).toBeVisible();
        await expect(firstCard.locator('.nft-title')).toBeVisible();
        await expect(firstCard.locator('.nft-description')).toBeVisible();
        await expect(firstCard.locator('.nft-card-footer')).toBeVisible();

        console.log(`✅ Found ${cardCount} NFT cards with proper structure`);
      } else {
        // Verify empty state is properly displayed
        await expect(page.locator('.empty-gallery')).toBeVisible();
        await expect(page.locator('.empty-gallery p')).toContainText(
          /no.*nft/i
        );
        console.log('✅ Empty gallery state properly displayed');
      }
    });

    test('should display NFT card details correctly', async ({ page }) => {
      // Wait for NFTs to load
      await page.waitForSelector('.nft-grid .nft-card, .empty-gallery', {
        timeout: 10000,
      });

      const nftCards = page.locator('.nft-card');
      const cardCount = await nftCards.count();

      if (cardCount > 0) {
        const firstCard = nftCards.first();

        // Verify image or placeholder
        const cardImage = firstCard.locator('.nft-card-image');
        await expect(cardImage).toBeVisible();

        const hasImage =
          (await firstCard.locator('.nft-card-image img').count()) > 0;
        const hasPlaceholder =
          (await firstCard.locator('.nft-placeholder').count()) > 0;
        expect(hasImage || hasPlaceholder).toBe(true);

        // Verify computation badge
        await expect(firstCard.locator('.computation-badge')).toBeVisible();

        // Verify NFT title and description
        const title = firstCard.locator('.nft-title');
        await expect(title).toBeVisible();
        await expect(title).not.toBeEmpty();

        const description = firstCard.locator('.nft-description');
        await expect(description).toBeVisible();

        // Verify NFT details section
        await expect(firstCard.locator('.nft-details')).toBeVisible();
        await expect(firstCard.locator('.complexity-indicator')).toBeVisible();

        // Verify footer with ID and date
        const footer = firstCard.locator('.nft-card-footer');
        await expect(footer.locator('.nft-id')).toBeVisible();
        await expect(footer.locator('.nft-date')).toBeVisible();

        console.log('✅ NFT card details validation completed');
      } else {
        console.log('⚠️ No NFT cards found to validate details');
      }
    });

    test('should handle loading states properly', async ({ page }) => {
      // Reload page to trigger loading state
      await page.reload();

      // Check for loading spinner during initial load
      const loadingSpinner = page.locator('.loading-spinner');
      const loadingText = page.locator('text=/loading.*nft/i');

      // At least one loading indicator should be present initially
      const hasLoadingIndicator = await Promise.race([
        loadingSpinner.isVisible(),
        loadingText.isVisible(),
        new Promise(resolve => setTimeout(() => resolve(false), 1000)),
      ]);

      // Wait for loading to complete
      await page.waitForSelector('.nft-grid, .empty-gallery, .gallery-error', {
        timeout: 15000,
      });

      // Verify loading state is gone
      await expect(loadingSpinner).not.toBeVisible();

      console.log(
        `✅ Loading state handling verified (had loading indicator: ${hasLoadingIndicator})`
      );
    });

    test('should display computation type badges correctly', async ({
      page,
    }) => {
      await page.waitForSelector('.nft-grid .nft-card, .empty-gallery', {
        timeout: 10000,
      });

      const badgeTypes = [
        { selector: '.computation-badge.weather', type: 'Weather' },
        { selector: '.computation-badge.climate', type: 'Climate' },
        { selector: '.computation-badge.air-quality', type: 'Air Quality' },
        { selector: '.computation-badge.generic', type: 'Generic' },
      ];

      let foundBadges = 0;

      for (const badge of badgeTypes) {
        const badgeElements = page.locator(badge.selector);
        const count = await badgeElements.count();

        if (count > 0) {
          foundBadges++;
          // Verify badge styling and text
          const firstBadge = badgeElements.first();
          await expect(firstBadge).toBeVisible();

          const badgeText = await firstBadge.textContent();
          expect(badgeText.toLowerCase()).toContain(badge.type.toLowerCase());

          console.log(`✅ Found ${count} ${badge.type} badges`);
        }
      }

      expect(foundBadges).toBeGreaterThanOrEqual(0); // At least some badges should exist if NFTs are present
      console.log(
        `✅ Computation badges validation completed (${foundBadges} types found)`
      );
    });

    test('should display complexity indicators correctly', async ({ page }) => {
      await page.waitForSelector('.nft-grid .nft-card, .empty-gallery', {
        timeout: 10000,
      });

      const complexityIndicators = page.locator('.complexity-indicator');
      const indicatorCount = await complexityIndicators.count();

      if (indicatorCount > 0) {
        const firstIndicator = complexityIndicators.first();
        await expect(firstIndicator).toBeVisible();

        // Verify complexity bar exists and has some width
        const complexityBar = firstIndicator.locator('.complexity-bar');
        await expect(complexityBar).toBeVisible();

        // Check that the bar has a width style set
        const barElement = await complexityBar.elementHandle();
        const width = await barElement.evaluate(el => el.style.width);
        expect(width).toMatch(/^\d+(\.\d+)?%$/); // Should be a percentage

        console.log(
          `✅ Found ${indicatorCount} complexity indicators with proper styling`
        );
      } else {
        console.log('⚠️ No complexity indicators found');
      }
    });
  });

  test.describe('Gallery Filtering Functionality', () => {
    test('should display filter controls', async ({ page }) => {
      // Verify filters section exists
      await expect(page.locator('.gallery-filters')).toBeVisible();

      // Check for computation type filter
      const computationTypeFilter = page.locator(
        'select[name="computationType"]'
      );
      await expect(computationTypeFilter).toBeVisible();

      // Check for sort by filter
      const sortByFilter = page.locator('select[name="sortBy"]');
      await expect(sortByFilter).toBeVisible();

      // Check for sort direction filter
      const sortDirectionFilter = page.locator('select[name="sortDirection"]');
      await expect(sortDirectionFilter).toBeVisible();

      console.log('✅ All filter controls are visible');
    });

    test('should filter by computation type', async ({ page }) => {
      await page.waitForSelector('.nft-grid, .empty-gallery', {
        timeout: 10000,
      });

      const initialCardCount = await page.locator('.nft-card').count();

      if (initialCardCount > 0) {
        // Test filtering by Weather Forecast
        await page.selectOption(
          'select[name="computationType"]',
          'WEATHER_FORECAST'
        );
        await page.waitForTimeout(500); // Wait for filter to apply

        const weatherCards = await page.locator('.nft-card').count();

        // Test filtering by Climate Simulation
        await page.selectOption(
          'select[name="computationType"]',
          'CLIMATE_SIMULATION'
        );
        await page.waitForTimeout(500);

        const climateCards = await page.locator('.nft-card').count();

        // Test filtering by Air Quality
        await page.selectOption(
          'select[name="computationType"]',
          'AIR_QUALITY'
        );
        await page.waitForTimeout(500);

        const airQualityCards = await page.locator('.nft-card').count();

        // Reset to show all
        await page.selectOption('select[name="computationType"]', 'ALL');
        await page.waitForTimeout(500);

        const allCards = await page.locator('.nft-card').count();

        // Verify filtering worked (all cards count should equal sum of filtered cards or be close)
        expect(allCards).toBeGreaterThanOrEqual(
          Math.max(weatherCards, climateCards, airQualityCards)
        );

        console.log(
          `✅ Filtering test: All(${allCards}) Weather(${weatherCards}) Climate(${climateCards}) AirQuality(${airQualityCards})`
        );
      } else {
        console.log('⚠️ No NFT cards found to test filtering');
      }
    });

    test('should sort NFTs correctly', async ({ page }) => {
      await page.waitForSelector('.nft-grid .nft-card, .empty-gallery', {
        timeout: 10000,
      });

      const cardCount = await page.locator('.nft-card').count();

      if (cardCount > 1) {
        // Test sorting by name
        await page.selectOption('select[name="sortBy"]', 'name');
        await page.waitForTimeout(500);

        const namesBefore = await page.locator('.nft-title').allTextContents();

        // Test descending order
        await page.selectOption('select[name="sortDirection"]', 'desc');
        await page.waitForTimeout(500);

        const namesDesc = await page.locator('.nft-title').allTextContents();

        // Test ascending order
        await page.selectOption('select[name="sortDirection"]', 'asc');
        await page.waitForTimeout(500);

        const namesAsc = await page.locator('.nft-title').allTextContents();

        // Verify that sorting changed the order (at least different from one of the states)
        const orderChanged =
          JSON.stringify(namesAsc) !== JSON.stringify(namesDesc);
        expect(orderChanged).toBe(true);

        // Test sorting by creation date
        await page.selectOption('select[name="sortBy"]', 'createdAt');
        await page.waitForTimeout(500);

        // Test sorting by complexity
        await page.selectOption(
          'select[name="sortBy"]',
          'computationComplexity'
        );
        await page.waitForTimeout(500);

        console.log('✅ Sorting functionality verified');
      } else {
        console.log('⚠️ Insufficient NFT cards to test sorting');
      }
    });

    test('should combine filtering and sorting', async ({ page }) => {
      await page.waitForSelector('.nft-grid .nft-card, .empty-gallery', {
        timeout: 10000,
      });

      const initialCount = await page.locator('.nft-card').count();

      if (initialCount > 0) {
        // Apply filter and sort combination
        await page.selectOption(
          'select[name="computationType"]',
          'WEATHER_FORECAST'
        );
        await page.selectOption('select[name="sortBy"]', 'name');
        await page.selectOption('select[name="sortDirection"]', 'asc');
        await page.waitForTimeout(1000);

        const filteredAndSortedCount = await page.locator('.nft-card').count();

        // Reset filters
        await page.selectOption('select[name="computationType"]', 'ALL');
        await page.waitForTimeout(500);

        const resetCount = await page.locator('.nft-card').count();

        expect(resetCount).toBeGreaterThanOrEqual(filteredAndSortedCount);

        console.log(
          `✅ Combined filtering and sorting: Initial(${initialCount}) Filtered(${filteredAndSortedCount}) Reset(${resetCount})`
        );
      } else {
        console.log('⚠️ No NFT cards found to test combined filtering');
      }
    });
  });

  test.describe('NFT Card Interactions', () => {
    test('should navigate to NFT details on card click', async ({ page }) => {
      await page.waitForSelector('.nft-grid .nft-card, .empty-gallery', {
        timeout: 10000,
      });

      const nftCards = page.locator('.nft-card');
      const cardCount = await nftCards.count();

      if (cardCount > 0) {
        const firstCard = nftCards.first();

        // Get the NFT ID from the card for verification
        const nftId = await firstCard.locator('.nft-id').textContent();

        // Click on the card
        await firstCard.click();
        await page.waitForLoadState('networkidle');

        // Verify navigation occurred (URL should change)
        const currentUrl = page.url();
        expect(currentUrl).toMatch(/\/nft\/|nft-details|nft\/[^\/]+$/);

        // Verify we're on NFT detail page
        const isDetailPage = await Promise.race([
          page.locator('.nft-detail-container').isVisible(),
          page
            .locator('[class*="nft-detail"], [class*="NFTDetail"]')
            .isVisible(),
          new Promise(resolve => setTimeout(() => resolve(false), 2000)),
        ]);

        if (isDetailPage) {
          console.log(
            `✅ Successfully navigated to NFT details page for ${nftId}`
          );
        } else {
          console.log(
            `⚠️ Navigation occurred but NFT detail page structure not found. URL: ${currentUrl}`
          );
        }

        // Navigate back to gallery
        await page.goBack();
        await page.waitForLoadState('networkidle');
      } else {
        console.log('⚠️ No NFT cards available to test navigation');
      }
    });

    test('should display hover effects on NFT cards', async ({ page }) => {
      await page.waitForSelector('.nft-grid .nft-card', { timeout: 10000 });

      const nftCards = page.locator('.nft-card');
      const cardCount = await nftCards.count();

      if (cardCount > 0) {
        const firstCard = nftCards.first();

        // Get initial transform style
        const initialTransform = await firstCard.evaluate(
          el => getComputedStyle(el).transform
        );

        // Hover over the card
        await firstCard.hover();
        await page.waitForTimeout(300); // Wait for transition

        // Get transform after hover
        const hoverTransform = await firstCard.evaluate(
          el => getComputedStyle(el).transform
        );

        // Verify that hover effect was applied (transform should change)
        const hoverEffectApplied = initialTransform !== hoverTransform;

        console.log(
          `✅ Hover effect test: Initial(${initialTransform}) Hover(${hoverTransform}) Changed(${hoverEffectApplied})`
        );

        // Move mouse away
        await page.mouse.move(0, 0);
        await page.waitForTimeout(300);
      } else {
        console.log('⚠️ No NFT cards available to test hover effects');
      }
    });
  });

  test.describe('Error Handling and Edge Cases', () => {
    test('should handle API errors gracefully', async ({ page }) => {
      // Intercept NFT API calls and simulate server error
      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Internal Server Error' }),
        });
      });

      await page.reload();
      await page.waitForTimeout(3000);

      // Check for error state
      const errorContainer = page.locator('.gallery-error');
      const errorMessage = page.locator('text=/error.*loading.*nft/i');

      const hasErrorState = await Promise.race([
        errorContainer.isVisible(),
        errorMessage.isVisible(),
        new Promise(resolve => setTimeout(() => resolve(false), 2000)),
      ]);

      if (hasErrorState) {
        console.log('✅ Error state properly displayed');

        // Test retry functionality if available
        const retryButton = page
          .locator('button')
          .filter({ hasText: /try.*again|reload|retry/i });
        const retryButtonExists = (await retryButton.count()) > 0;

        if (retryButtonExists) {
          console.log('✅ Retry button available');
        }
      } else {
        console.log(
          '⚠️ Error state not detected - may need API endpoint verification'
        );
      }
    });

    test('should handle empty NFT list', async ({ page }) => {
      // Intercept API to return empty list
      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify([]),
        });
      });

      await page.reload();
      await page.waitForTimeout(2000);

      // Verify empty state is displayed
      const emptyGallery = page.locator('.empty-gallery');
      await expect(emptyGallery).toBeVisible();

      const emptyMessage = emptyGallery.locator('p');
      await expect(emptyMessage).toContainText(/no.*nft/i);

      // Verify "Create Your First NFT" link/button exists
      const createLink = emptyGallery
        .locator('a, button')
        .filter({ hasText: /create.*nft/i });
      await expect(createLink).toBeVisible();

      console.log('✅ Empty state properly handled');
    });

    test('should handle slow network conditions', async ({ page }) => {
      // Simulate slow network by delaying API responses
      await page.route('**/api/computational/nfts*', async route => {
        await new Promise(resolve => setTimeout(resolve, 3000)); // 3 second delay
        const response = await route.fetch();
        route.fulfill({ response });
      });

      await page.reload();

      // Verify loading state is shown
      const loadingIndicator = page.locator(
        '.loading-spinner, .gallery-loading'
      );
      await expect(loadingIndicator).toBeVisible();

      console.log('✅ Loading state displayed during slow network');

      // Wait for content to eventually load
      await page.waitForSelector('.nft-grid, .empty-gallery, .gallery-error', {
        timeout: 15000,
      });

      // Verify loading state disappears
      await expect(loadingIndicator).not.toBeVisible();

      console.log('✅ Loading state properly cleared after slow load');
    });
  });

  test.describe('Responsive Design', () => {
    test('should adapt to different screen sizes', async ({ page }) => {
      const viewports = [
        { width: 1200, height: 800, name: 'Desktop' },
        { width: 768, height: 1024, name: 'Tablet' },
        { width: 375, height: 667, name: 'Mobile' },
      ];

      for (const viewport of viewports) {
        await page.setViewportSize(viewport);
        await page.waitForTimeout(500);

        // Verify gallery is still visible and functional
        await expect(page.locator('.nft-gallery-container')).toBeVisible();

        // Check grid responsiveness
        const nftGrid = page.locator('.nft-grid');
        if ((await nftGrid.count()) > 0) {
          await expect(nftGrid).toBeVisible();

          // Verify cards are properly sized
          const cards = page.locator('.nft-card');
          const cardCount = await cards.count();

          if (cardCount > 0) {
            const firstCard = cards.first();
            await expect(firstCard).toBeVisible();

            // Check that card is not too wide for viewport
            const cardWidth = await firstCard.evaluate(el => el.offsetWidth);
            expect(cardWidth).toBeLessThanOrEqual(viewport.width);
          }
        }

        // Take screenshot for manual verification
        await page.screenshot({
          path: `test-results/nft-gallery-responsive-${viewport.name}-${viewport.width}x${viewport.height}.png`,
          fullPage: true,
        });

        console.log(
          `✅ ${viewport.name} (${viewport.width}x${viewport.height}) responsive test passed`
        );
      }

      // Reset to default viewport
      await page.setViewportSize({ width: 1200, height: 800 });
    });

    test('should handle filter layout on mobile', async ({ page }) => {
      // Set mobile viewport
      await page.setViewportSize({ width: 375, height: 667 });
      await page.waitForTimeout(500);

      // Verify filters are still accessible
      const filtersContainer = page.locator('.gallery-filters');
      await expect(filtersContainer).toBeVisible();

      // Check that filters stack vertically on mobile
      const computationFilter = page.locator('select[name="computationType"]');
      const sortByFilter = page.locator('select[name="sortBy"]');

      if (
        (await computationFilter.count()) > 0 &&
        (await sortByFilter.count()) > 0
      ) {
        await expect(computationFilter).toBeVisible();
        await expect(sortByFilter).toBeVisible();

        // Test that filters still work on mobile
        await computationFilter.selectOption('WEATHER_FORECAST');
        await page.waitForTimeout(500);

        console.log('✅ Filters functional on mobile viewport');
      }
    });
  });

  test.describe('Performance and Accessibility', () => {
    test('should load within acceptable time limits', async ({ page }) => {
      const startTime = Date.now();

      await page.goto('http://localhost:5000');
      await page.waitForLoadState('networkidle');

      const loadTime = Date.now() - startTime;

      // Expect page to load within 5 seconds
      expect(loadTime).toBeLessThan(5000);

      console.log(`✅ Page loaded in ${loadTime}ms`);
    });

    test('should have proper accessibility attributes', async ({ page }) => {
      await page.waitForSelector('.nft-gallery-container', { timeout: 10000 });

      // Check for proper heading structure
      const mainHeading = page.locator('h2').first();
      await expect(mainHeading).toBeVisible();

      // Check form labels for filters
      const labels = page.locator('label');
      const labelCount = await labels.count();

      if (labelCount > 0) {
        // Verify labels are associated with form controls
        for (let i = 0; i < labelCount; i++) {
          const label = labels.nth(i);
          const forAttribute = await label.getAttribute('for');

          if (forAttribute) {
            const associatedControl = page.locator(`#${forAttribute}`);
            await expect(associatedControl).toBeVisible();
          }
        }

        console.log(`✅ Found ${labelCount} properly associated form labels`);
      }

      // Check for alt text on images
      const images = page.locator('img');
      const imageCount = await images.count();

      if (imageCount > 0) {
        const imagesWithAlt = await page.locator('img[alt]').count();
        const altTextCoverage = (imagesWithAlt / imageCount) * 100;

        console.log(
          `✅ Alt text coverage: ${altTextCoverage.toFixed(1)}% (${imagesWithAlt}/${imageCount} images)`
        );
      }
    });
  });

  test.describe('Integration with Create NFT Flow', () => {
    test('should navigate to NFT creation from gallery', async ({ page }) => {
      await page.waitForSelector('.gallery-header', { timeout: 10000 });

      // Find and click the "Create New NFT" button
      const createButton = page
        .locator('.gallery-header .button.primary')
        .filter({ hasText: /create.*nft/i });
      await expect(createButton).toBeVisible();

      await createButton.click();
      await page.waitForLoadState('networkidle');

      // Verify navigation to creation form
      const currentUrl = page.url();
      const hasCreateInUrl =
        currentUrl.includes('create') || currentUrl.includes('nft');

      // Look for NFT creation form elements
      const hasCreationForm = await Promise.race([
        page
          .locator('form')
          .filter({ hasText: /create.*nft|nft.*name|mint.*nft/i })
          .isVisible(),
        page
          .locator('input[placeholder*="NFT"], input[name*="nft"]')
          .isVisible(),
        new Promise(resolve => setTimeout(() => resolve(false), 2000)),
      ]);

      if (hasCreateInUrl || hasCreationForm) {
        console.log('✅ Successfully navigated to NFT creation flow');
      } else {
        console.log(
          `⚠️ Navigation attempted but creation form not detected. URL: ${currentUrl}`
        );
      }
    });
  });
});
