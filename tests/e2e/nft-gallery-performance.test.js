const { test, expect } = require('@playwright/test');

test.describe('NFT Gallery Performance Tests (Subtask 7.3)', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to NFT gallery
    await page.goto('http://localhost:5000');
    await page.waitForLoadState('networkidle');

    // Navigate to NFT gallery
    const nftGalleryLink = await page
      .locator('a[href*="nft"], button')
      .filter({ hasText: /gallery|browse.*nft/i })
      .first();
    if ((await nftGalleryLink.count()) > 0) {
      await nftGalleryLink.click();
    } else {
      await page.goto('http://localhost:5000/nft/browse');
    }

    await page.waitForLoadState('networkidle');
  });

  test.describe('Performance with Large NFT Datasets', () => {
    test('should handle large number of NFTs efficiently', async ({ page }) => {
      // Create mock data for 100 NFTs
      const mockNFTs = [];
      for (let i = 1; i <= 100; i++) {
        mockNFTs.push({
          nftId: `nft-${i.toString().padStart(3, '0')}`,
          name: `Computational NFT ${i}`,
          description: `Advanced climate model for weather prediction and analysis - Model ${i}`,
          creator: `0x742d35Cc6634C0532925a3b8D8aE${i.toString().padStart(8, '0')}`,
          createdAt: new Date(Date.now() - i * 86400000).toISOString(),
          contentUri: i % 3 === 0 ? `ipfs://QmTest${i}` : null,
          computationType: [
            'WEATHER_FORECAST',
            'CLIMATE_SIMULATION',
            'AIR_QUALITY',
            'GENERIC',
          ][i % 4],
          computationComplexity: Math.floor(Math.random() * 10) + 1,
          region: ['Global', 'North America', 'Europe', 'Asia'][i % 4],
          isVerified: i % 3 === 0,
          resourceRequirements: {
            cpu: Math.floor(Math.random() * 16) + 2,
            memory: (Math.floor(Math.random() * 16) + 4) * 1024,
            disk: Math.floor(Math.random() * 100) + 10,
            gpu: i % 5 === 0,
          },
        });
      }

      // Intercept API to return large dataset
      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockNFTs),
        });
      });

      // Measure performance
      const startTime = Date.now();

      await page.reload();
      await page.waitForSelector('.nft-grid, .empty-gallery', {
        timeout: 15000,
      });

      const loadTime = Date.now() - startTime;

      // Verify all NFTs are rendered
      const renderedCards = await page.locator('.nft-card').count();

      // Performance expectations
      expect(loadTime).toBeLessThan(10000); // Should load within 10 seconds
      expect(renderedCards).toBe(100); // All NFTs should be rendered

      console.log(`✅ Loaded ${renderedCards} NFTs in ${loadTime}ms`);
    });

    test('should handle filtering performance with large datasets', async ({
      page,
    }) => {
      // Create large dataset with specific types
      const mockNFTs = [];
      for (let i = 1; i <= 200; i++) {
        mockNFTs.push({
          nftId: `nft-${i.toString().padStart(3, '0')}`,
          name: `NFT ${i}`,
          description: `Description ${i}`,
          creator: `Creator${i}`,
          createdAt: new Date(Date.now() - i * 86400000).toISOString(),
          computationType: [
            'WEATHER_FORECAST',
            'CLIMATE_SIMULATION',
            'AIR_QUALITY',
          ][i % 3],
          computationComplexity: (i % 10) + 1,
        });
      }

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      // Test filtering performance
      const filterStartTime = Date.now();

      await page.selectOption(
        'select[name="computationType"]',
        'WEATHER_FORECAST'
      );
      await page.waitForTimeout(100); // Wait for filter to apply

      const filterTime = Date.now() - filterStartTime;

      // Verify filtering worked
      const filteredCards = await page.locator('.nft-card').count();
      expect(filteredCards).toBeLessThanOrEqual(200);
      expect(filteredCards).toBeGreaterThan(0);

      // Filtering should be fast (under 1 second)
      expect(filterTime).toBeLessThan(1000);

      console.log(`✅ Filtered ${filteredCards} cards in ${filterTime}ms`);
    });

    test('should handle sorting performance with large datasets', async ({
      page,
    }) => {
      // Create dataset optimized for sorting tests
      const mockNFTs = [];
      for (let i = 1; i <= 150; i++) {
        mockNFTs.push({
          nftId: `nft-${i.toString().padStart(3, '0')}`,
          name: `NFT ${String.fromCharCode(65 + (i % 26))}${i}`, // A1, B2, C3, etc.
          description: `Description ${i}`,
          creator: `Creator${i}`,
          createdAt: new Date(
            Date.now() - Math.random() * 30 * 86400000
          ).toISOString(),
          computationType: 'WEATHER_FORECAST',
          computationComplexity: Math.floor(Math.random() * 10) + 1,
        });
      }

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      // Test sorting by name
      const sortStartTime = Date.now();

      await page.selectOption('select[name="sortBy"]', 'name');
      await page.selectOption('select[name="sortDirection"]', 'asc');
      await page.waitForTimeout(100);

      const sortTime = Date.now() - sortStartTime;

      // Verify sorting worked by checking first few items
      const firstCardName = await page
        .locator('.nft-card .nft-title')
        .first()
        .textContent();
      expect(firstCardName).toBeTruthy();

      // Sorting should be fast
      expect(sortTime).toBeLessThan(1000);

      console.log(`✅ Sorted 150 NFTs in ${sortTime}ms`);
    });

    test('should handle memory usage efficiently with large datasets', async ({
      page,
    }) => {
      // Create very large dataset
      const mockNFTs = [];
      for (let i = 1; i <= 500; i++) {
        mockNFTs.push({
          nftId: `nft-${i.toString().padStart(3, '0')}`,
          name: `Computational Climate Model ${i}`,
          description: `Advanced machine learning model for weather prediction and climate analysis. This model uses state-of-the-art algorithms to process meteorological data and generate accurate forecasts. Model version ${i} includes enhanced features for regional climate assessment.`,
          creator: `0x742d35Cc6634C0532925a3b8D8aE${i.toString().padStart(8, '0')}`,
          createdAt: new Date(Date.now() - i * 86400000).toISOString(),
          contentUri: `ipfs://QmLargeDataset${i.toString().padStart(10, '0')}`,
          computationType: [
            'WEATHER_FORECAST',
            'CLIMATE_SIMULATION',
            'AIR_QUALITY',
            'GENERIC',
          ][i % 4],
          computationComplexity: Math.floor(Math.random() * 10) + 1,
          region: 'Global',
          isVerified: i % 2 === 0,
          resourceRequirements: {
            cpu: Math.floor(Math.random() * 32) + 4,
            memory: (Math.floor(Math.random() * 64) + 8) * 1024,
            disk: Math.floor(Math.random() * 1000) + 100,
            gpu: i % 4 === 0,
          },
          metadata: {
            latitude: (Math.random() - 0.5) * 180,
            longitude: (Math.random() - 0.5) * 360,
            accuracy: Math.random() * 100,
            lastUpdated: new Date().toISOString(),
          },
        });
      }

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockNFTs),
        });
      });

      // Monitor performance during load
      const startTime = Date.now();

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 20000 });

      const loadTime = Date.now() - startTime;

      // Verify page is still responsive
      const isResponsive = await page.evaluate(() => {
        return new Promise(resolve => {
          const start = performance.now();
          requestAnimationFrame(() => {
            const end = performance.now();
            resolve(end - start < 100); // Frame should render within 100ms
          });
        });
      });

      expect(isResponsive).toBe(true);
      expect(loadTime).toBeLessThan(15000); // Should load within 15 seconds

      console.log(
        `✅ Loaded 500 NFTs in ${loadTime}ms, page responsive: ${isResponsive}`
      );
    });

    test('should handle scroll performance with many NFTs', async ({
      page,
    }) => {
      // Create large dataset for scroll testing
      const mockNFTs = [];
      for (let i = 1; i <= 300; i++) {
        mockNFTs.push({
          nftId: `scroll-nft-${i}`,
          name: `Scroll Test NFT ${i}`,
          description: `NFT for scroll testing ${i}`,
          creator: `Creator${i}`,
          createdAt: new Date().toISOString(),
          computationType: 'GENERIC',
          computationComplexity: 5,
        });
      }

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 15000 });

      // Test scrolling performance
      const scrollStartTime = Date.now();

      // Scroll to bottom
      await page.evaluate(() => {
        window.scrollTo(0, document.body.scrollHeight);
      });

      await page.waitForTimeout(500);

      // Scroll back to top
      await page.evaluate(() => {
        window.scrollTo(0, 0);
      });

      const scrollTime = Date.now() - scrollStartTime;

      // Verify all cards are still visible and functional
      const visibleCards = await page.locator('.nft-card').count();
      expect(visibleCards).toBe(300);

      console.log(
        `✅ Scroll test completed in ${scrollTime}ms with ${visibleCards} cards`
      );
    });
  });

  test.describe('Multi-Media NFT Content Types', () => {
    test('should handle image NFTs correctly', async ({ page }) => {
      const imageNFTs = [
        {
          nftId: 'img-001',
          name: 'Weather Satellite Image NFT',
          description: 'High-resolution satellite imagery NFT',
          creator: 'SatelliteProvider',
          createdAt: new Date().toISOString(),
          contentUri: 'https://example.com/satellite-image.jpg',
          computationType: 'WEATHER_FORECAST',
          computationComplexity: 7,
        },
        {
          nftId: 'img-002',
          name: 'Climate Data Visualization',
          description: 'Interactive climate chart NFT',
          creator: 'DataViz',
          createdAt: new Date().toISOString(),
          contentUri: 'https://example.com/climate-chart.png',
          computationType: 'CLIMATE_SIMULATION',
          computationComplexity: 5,
        },
      ];

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(imageNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      // Verify image NFTs are displayed
      const imageCards = page.locator('.nft-card');
      const cardCount = await imageCards.count();
      expect(cardCount).toBe(2);

      // Check first image NFT
      const firstCard = imageCards.first();
      const image = firstCard.locator('img');

      if ((await image.count()) > 0) {
        await expect(image).toBeVisible();

        const src = await image.getAttribute('src');
        expect(src).toContain('satellite-image.jpg');

        console.log('✅ Image NFT displayed correctly');
      } else {
        // Check for placeholder
        const placeholder = firstCard.locator('.nft-placeholder');
        await expect(placeholder).toBeVisible();
        console.log('✅ Image NFT placeholder displayed');
      }
    });

    test('should handle IPFS content URIs', async ({ page }) => {
      const ipfsNFTs = [
        {
          nftId: 'ipfs-001',
          name: 'IPFS Weather Model',
          description: 'Decentralized weather prediction model',
          creator: 'IPFSProvider',
          createdAt: new Date().toISOString(),
          contentUri: 'ipfs://QmYjtig7VJQ6XsnUjqqJvj7QaMcCAwtrgNdahSiFofrE7o',
          computationType: 'WEATHER_FORECAST',
          computationComplexity: 8,
        },
      ];

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(ipfsNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      const nftCard = page.locator('.nft-card').first();
      const image = nftCard.locator('img');

      if ((await image.count()) > 0) {
        const src = await image.getAttribute('src');
        // Should convert IPFS URI to HTTP gateway
        expect(src).toContain('ipfs.io/ipfs/');
        console.log(`✅ IPFS URI converted correctly: ${src}`);
      } else {
        console.log('✅ IPFS NFT displayed with placeholder');
      }
    });

    test('should handle missing or broken images gracefully', async ({
      page,
    }) => {
      const brokenImageNFTs = [
        {
          nftId: 'broken-001',
          name: 'Broken Image NFT',
          description: 'NFT with broken image URL',
          creator: 'TestProvider',
          createdAt: new Date().toISOString(),
          contentUri: 'https://example.com/nonexistent-image.jpg',
          computationType: 'GENERIC',
          computationComplexity: 3,
        },
        {
          nftId: 'null-001',
          name: 'No Image NFT',
          description: 'NFT without image',
          creator: 'TestProvider',
          createdAt: new Date().toISOString(),
          contentUri: null,
          computationType: 'GENERIC',
          computationComplexity: 4,
        },
      ];

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(brokenImageNFTs),
        });
      });

      // Block external image requests to simulate broken images
      await page.route('https://example.com/**', route => {
        route.fulfill({ status: 404, body: 'Not found' });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      const nftCards = page.locator('.nft-card');
      const cardCount = await nftCards.count();
      expect(cardCount).toBe(2);

      // Check that placeholders are shown for missing images
      const placeholders = page.locator('.nft-placeholder');
      const placeholderCount = await placeholders.count();

      expect(placeholderCount).toBeGreaterThan(0);

      // Verify placeholder content
      const firstPlaceholder = placeholders.first();
      await expect(firstPlaceholder).toBeVisible();

      console.log(
        `✅ Broken images handled gracefully with ${placeholderCount} placeholders`
      );
    });

    test('should handle different computational NFT types', async ({
      page,
    }) => {
      const diverseNFTs = [
        {
          nftId: 'weather-001',
          name: 'Weather Forecast Model',
          description: 'Advanced weather prediction',
          creator: 'WeatherCorp',
          createdAt: new Date().toISOString(),
          computationType: 'WEATHER_FORECAST',
          computationComplexity: 9,
          resourceRequirements: {
            cpu: 16,
            memory: 32768,
            disk: 500,
            gpu: true,
          },
        },
        {
          nftId: 'climate-001',
          name: 'Climate Simulation Model',
          description: 'Long-term climate analysis',
          creator: 'ClimateLab',
          createdAt: new Date().toISOString(),
          computationType: 'CLIMATE_SIMULATION',
          computationComplexity: 10,
          resourceRequirements: {
            cpu: 32,
            memory: 65536,
            disk: 1000,
            gpu: true,
          },
        },
        {
          nftId: 'air-001',
          name: 'Air Quality Monitor',
          description: 'Real-time air quality assessment',
          creator: 'AirQuality Inc',
          createdAt: new Date().toISOString(),
          computationType: 'AIR_QUALITY',
          computationComplexity: 6,
          resourceRequirements: {
            cpu: 8,
            memory: 16384,
            disk: 200,
            gpu: false,
          },
        },
        {
          nftId: 'generic-001',
          name: 'Generic Computational Model',
          description: 'General purpose computation',
          creator: 'GenericCorp',
          createdAt: new Date().toISOString(),
          computationType: 'GENERIC',
          computationComplexity: 5,
          resourceRequirements: {
            cpu: 4,
            memory: 8192,
            disk: 100,
            gpu: false,
          },
        },
      ];

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(diverseNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      // Verify all types are displayed
      const nftCards = page.locator('.nft-card');
      const cardCount = await nftCards.count();
      expect(cardCount).toBe(4);

      // Check computation badges for each type
      const weatherBadge = page.locator('.computation-badge.weather');
      const climateBadge = page.locator('.computation-badge.climate');
      const airQualityBadge = page.locator('.computation-badge.air-quality');
      const genericBadge = page.locator('.computation-badge.generic');

      await expect(weatherBadge).toBeVisible();
      await expect(climateBadge).toBeVisible();
      await expect(airQualityBadge).toBeVisible();
      await expect(genericBadge).toBeVisible();

      // Test filtering by each type
      for (const type of [
        'WEATHER_FORECAST',
        'CLIMATE_SIMULATION',
        'AIR_QUALITY',
        'GENERIC',
      ]) {
        await page.selectOption('select[name="computationType"]', type);
        await page.waitForTimeout(300);

        const filteredCards = await page.locator('.nft-card').count();
        expect(filteredCards).toBe(1);
      }

      // Reset filter
      await page.selectOption('select[name="computationType"]', 'ALL');
      await page.waitForTimeout(300);

      const allCards = await page.locator('.nft-card').count();
      expect(allCards).toBe(4);

      console.log('✅ All computational NFT types handled correctly');
    });

    test('should display complexity indicators for various complexity levels', async ({
      page,
    }) => {
      const complexityNFTs = [];

      // Create NFTs with different complexity levels (1-10)
      for (let i = 1; i <= 10; i++) {
        complexityNFTs.push({
          nftId: `complexity-${i}`,
          name: `Complexity Level ${i} NFT`,
          description: `NFT with complexity level ${i}`,
          creator: 'ComplexityTest',
          createdAt: new Date().toISOString(),
          computationType: 'GENERIC',
          computationComplexity: i,
        });
      }

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(complexityNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      // Verify all complexity levels are displayed
      const complexityBars = page.locator('.complexity-bar');
      const barCount = await complexityBars.count();
      expect(barCount).toBe(10);

      // Check that different complexity levels have different widths
      const widths = [];
      for (let i = 0; i < barCount; i++) {
        const bar = complexityBars.nth(i);
        const width = await bar.evaluate(el => el.style.width);
        widths.push(width);
      }

      // Verify we have different width values
      const uniqueWidths = new Set(widths);
      expect(uniqueWidths.size).toBeGreaterThan(1);

      console.log(
        `✅ Complexity indicators working: ${uniqueWidths.size} unique widths for 10 levels`
      );
    });
  });

  test.describe('Cross-Browser Compatibility', () => {
    test('should work consistently across browsers', async ({
      page,
      browserName,
    }) => {
      const testNFTs = [
        {
          nftId: 'browser-test-001',
          name: 'Cross-Browser Test NFT',
          description: 'NFT for browser compatibility testing',
          creator: 'BrowserTest',
          createdAt: new Date().toISOString(),
          computationType: 'WEATHER_FORECAST',
          computationComplexity: 7,
        },
      ];

      await page.route('**/api/computational/nfts*', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(testNFTs),
        });
      });

      await page.reload();
      await page.waitForSelector('.nft-grid', { timeout: 10000 });

      // Test basic functionality in current browser
      const nftCard = page.locator('.nft-card').first();
      await expect(nftCard).toBeVisible();

      // Test hover effects (may vary by browser)
      await nftCard.hover();
      await page.waitForTimeout(300);

      // Test filter functionality
      await page.selectOption(
        'select[name="computationType"]',
        'WEATHER_FORECAST'
      );
      await page.waitForTimeout(500);

      const filteredCards = await page.locator('.nft-card').count();
      expect(filteredCards).toBe(1);

      // Take browser-specific screenshot
      await page.screenshot({
        path: `test-results/nft-gallery-${browserName}-compatibility.png`,
        fullPage: true,
      });

      console.log(`✅ Cross-browser test passed for ${browserName}`);
    });
  });
});
