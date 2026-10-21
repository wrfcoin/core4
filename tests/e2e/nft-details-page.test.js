const { test, expect } = require('@playwright/test');

test.describe('NFT Details Page Functionality Tests (Subtask 7.3)', () => {
  let testNFT;

  test.beforeEach(async ({ page }) => {
    // Define comprehensive test NFT data
    testNFT = {
      nftId: 'detail-test-001',
      name: 'Advanced Climate Prediction Model v3.2',
      description:
        'A sophisticated machine learning model for long-term climate prediction using ensemble methods and deep neural networks. This model incorporates satellite data, ground-based observations, and historical climate records to generate accurate forecasts for regional and global climate patterns.',
      creator: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9fABCD',
      owner: '0x742d35Cc6634C0532925a3b8D8aE7b8e4B9fABCD',
      createdAt: '2024-01-15T10:30:00.000Z',
      contentUri: 'ipfs://QmYjtig7VJQ6XsnUjqqJvj7QaMcCAwtrgNdahSiFofrE7o',
      computationType: 'CLIMATE_SIMULATION',
      computationComplexity: 9,
      region: 'Global',
      isVerified: true,
      resourceRequirements: {
        cpu: 16,
        memory: 32768,
        disk: 500,
        gpu: true,
      },
      metadata: {
        latitude: 40.7128,
        longitude: -74.006,
        accuracy: 94.5,
        lastUpdated: '2024-01-15T10:30:00.000Z',
      },
    };

    // Mock NFT details API
    await page.route('**/api/computational/nft/detail-test-001', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(testNFT),
      });
    });

    // Mock related tasks API
    await page.route(
      '**/api/computational/tasks?nftId=detail-test-001',
      route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify([
            {
              taskId: 'task-001',
              title: 'Climate Model Execution',
              description: 'Execute climate prediction model for Q1 2024',
              status: 'COMPLETED',
              createdAt: '2024-01-20T09:00:00.000Z',
            },
            {
              taskId: 'task-002',
              title: 'Model Validation',
              description: 'Validate model results against historical data',
              status: 'PROCESSING',
              createdAt: '2024-01-21T14:30:00.000Z',
            },
            {
              taskId: 'task-003',
              title: 'Performance Optimization',
              description: 'Optimize model for better performance',
              status: 'PENDING',
              createdAt: '2024-01-22T11:15:00.000Z',
            },
          ]),
        });
      }
    );

    // Navigate to NFT details page
    await page.goto('http://localhost:5000/nft/detail-test-001');
    await page.waitForLoadState('networkidle');
  });

  test.describe('NFT Details Page Structure', () => {
    test('should display main NFT details container', async ({ page }) => {
      // Check for main container
      await expect(page.locator('.nft-detail-container')).toBeVisible();

      // Verify header section
      await expect(page.locator('.nft-detail-header')).toBeVisible();

      // Check title and ID
      const titleSection = page.locator('.nft-detail-title');
      await expect(titleSection.locator('h2')).toHaveText(testNFT.name);
      await expect(titleSection.locator('.nft-id')).toContainText(
        testNFT.nftId
      );

      // Verify back button
      await expect(
        page.locator('.nft-detail-actions button').filter({ hasText: /back/i })
      ).toBeVisible();

      console.log('✅ NFT details page structure verified');
    });

    test('should display NFT visualization section', async ({ page }) => {
      // Check visualization section
      const visualizationSection = page.locator('.nft-detail-visualization');
      await expect(visualizationSection).toBeVisible();

      // Check for image or placeholder
      const hasImage = (await visualizationSection.locator('img').count()) > 0;
      const hasPlaceholder =
        (await visualizationSection.locator('.nft-placeholder').count()) > 0;

      expect(hasImage || hasPlaceholder).toBe(true);

      if (hasImage) {
        const image = visualizationSection.locator('img');
        await expect(image).toBeVisible();

        const src = await image.getAttribute('src');
        expect(src).toContain('ipfs.io/ipfs/'); // Should convert IPFS URI
        console.log('✅ NFT image displayed correctly');
      } else {
        const placeholder = visualizationSection.locator('.nft-placeholder');
        await expect(placeholder).toBeVisible();
        await expect(placeholder).toContainText(
          /computational.*nft.*visualization/i
        );
        console.log('✅ NFT placeholder displayed correctly');
      }
    });

    test('should display comprehensive NFT information', async ({ page }) => {
      // Check description section
      const descriptionSection = page
        .locator('.info-section')
        .filter({ hasText: /description/i });
      await expect(descriptionSection).toBeVisible();
      await expect(descriptionSection.locator('p')).toContainText(
        testNFT.description
      );

      // Check computation details section
      const computationSection = page
        .locator('.info-section')
        .filter({ hasText: /computation.*details/i });
      await expect(computationSection).toBeVisible();

      // Verify computation type
      const typeInfo = computationSection
        .locator('.info-item')
        .filter({ hasText: /type/i });
      await expect(typeInfo.locator('.info-value')).toContainText(
        /climate.*simulation/i
      );

      // Verify complexity meter
      const complexityInfo = computationSection
        .locator('.info-item')
        .filter({ hasText: /complexity/i });
      await expect(complexityInfo.locator('.complexity-meter')).toBeVisible();
      await expect(complexityInfo.locator('.complexity-fill')).toBeVisible();
      await expect(complexityInfo).toContainText('9/10');

      // Verify region
      const regionInfo = computationSection
        .locator('.info-item')
        .filter({ hasText: /region/i });
      await expect(regionInfo.locator('.info-value')).toContainText(
        testNFT.region
      );

      // Verify verification status
      const verificationInfo = computationSection
        .locator('.info-item')
        .filter({ hasText: /verified/i });
      await expect(
        verificationInfo.locator('.verification-status.verified')
      ).toBeVisible();
      await expect(verificationInfo).toContainText('Yes');

      console.log('✅ Computation details section verified');
    });

    test('should display resource requirements correctly', async ({ page }) => {
      const resourceSection = page
        .locator('.info-section')
        .filter({ hasText: /resource.*requirements/i });
      await expect(resourceSection).toBeVisible();

      // Check CPU requirements
      const cpuInfo = resourceSection
        .locator('.info-item')
        .filter({ hasText: /cpu/i });
      await expect(cpuInfo.locator('.info-value')).toContainText('16 cores');

      // Check Memory requirements
      const memoryInfo = resourceSection
        .locator('.info-item')
        .filter({ hasText: /memory/i });
      await expect(memoryInfo.locator('.info-value')).toContainText('32 GB');

      // Check Disk requirements
      const diskInfo = resourceSection
        .locator('.info-item')
        .filter({ hasText: /disk/i });
      await expect(diskInfo.locator('.info-value')).toContainText('500 GB');

      // Check GPU requirements
      const gpuInfo = resourceSection
        .locator('.info-item')
        .filter({ hasText: /gpu/i });
      await expect(gpuInfo.locator('.info-value')).toContainText('Required');

      console.log('✅ Resource requirements section verified');
    });

    test('should display location information when available', async ({
      page,
    }) => {
      const locationSection = page
        .locator('.info-section')
        .filter({ hasText: /location/i });
      await expect(locationSection).toBeVisible();

      // Check coordinates
      const coordinates = locationSection.locator('.coordinates span');
      await expect(coordinates).toContainText('40.7128, -74.0060');

      // Check map link
      const mapLink = locationSection.locator('.map-link');
      await expect(mapLink).toBeVisible();
      await expect(mapLink).toHaveAttribute('href', /openstreetmap\.org/);
      await expect(mapLink).toHaveAttribute('target', '_blank');

      console.log('✅ Location information verified');
    });

    test('should display ownership information', async ({ page }) => {
      const ownershipSection = page
        .locator('.sidebar-section')
        .filter({ hasText: /ownership/i });
      await expect(ownershipSection).toBeVisible();

      // Check creator
      const creatorInfo = ownershipSection
        .locator('.owner-item')
        .filter({ hasText: /creator/i });
      await expect(creatorInfo.locator('.owner-value')).toContainText(
        testNFT.creator
      );

      // Check current owner
      const ownerInfo = ownershipSection
        .locator('.owner-item')
        .filter({ hasText: /current.*owner/i });
      await expect(ownerInfo.locator('.owner-value')).toContainText(
        testNFT.owner
      );

      console.log('✅ Ownership information verified');
    });
  });

  test.describe('Related Tasks Section', () => {
    test('should display tasks section with task list', async ({ page }) => {
      const tasksSection = page
        .locator('.sidebar-section')
        .filter({ hasText: /tasks/i });
      await expect(tasksSection).toBeVisible();

      // Check tasks list
      const tasksList = tasksSection.locator('.tasks-list');
      await expect(tasksList).toBeVisible();

      // Verify task items
      const taskItems = tasksList.locator('.task-item');
      const taskCount = await taskItems.count();
      expect(taskCount).toBe(3);

      console.log(`✅ Found ${taskCount} tasks in the tasks section`);
    });

    test('should display individual task details correctly', async ({
      page,
    }) => {
      const taskItems = page.locator('.task-item');

      // Check first task (completed)
      const firstTask = taskItems.first();
      await expect(firstTask.locator('.task-title')).toContainText(
        'Climate Model Execution'
      );
      await expect(firstTask.locator('.status-badge.completed')).toBeVisible();
      await expect(firstTask.locator('.task-description')).toContainText(
        'Execute climate prediction model'
      );

      // Check second task (processing)
      const secondTask = taskItems.nth(1);
      await expect(secondTask.locator('.task-title')).toContainText(
        'Model Validation'
      );
      await expect(
        secondTask.locator('.status-badge.processing')
      ).toBeVisible();

      // Check third task (pending)
      const thirdTask = taskItems.nth(2);
      await expect(thirdTask.locator('.task-title')).toContainText(
        'Performance Optimization'
      );
      await expect(thirdTask.locator('.status-badge.pending')).toBeVisible();

      console.log('✅ All task details verified');
    });

    test('should provide navigation to individual task details', async ({
      page,
    }) => {
      const taskItems = page.locator('.task-item');
      const firstTask = taskItems.first();

      // Check task link
      const taskLink = firstTask.locator('.task-link');
      await expect(taskLink).toBeVisible();
      await expect(taskLink).toHaveText('View Details');

      // Verify link href
      const href = await taskLink.getAttribute('href');
      expect(href).toContain('/computational/task/task-001');

      console.log('✅ Task navigation links verified');
    });

    test('should display task dates correctly', async ({ page }) => {
      const taskItems = page.locator('.task-item');

      for (let i = 0; i < (await taskItems.count()); i++) {
        const task = taskItems.nth(i);
        const dateElement = task.locator('.task-date');

        await expect(dateElement).toBeVisible();

        const dateText = await dateElement.textContent();
        expect(dateText).toMatch(/\d{1,2}\/\d{1,2}\/\d{4}/); // MM/DD/YYYY format
      }

      console.log('✅ Task dates formatted correctly');
    });
  });

  test.describe('Navigation and User Interactions', () => {
    test('should navigate back when back button is clicked', async ({
      page,
    }) => {
      // Record initial URL
      const detailsUrl = page.url();

      // Click back button
      const backButton = page
        .locator('.nft-detail-actions button')
        .filter({ hasText: /back/i });
      await backButton.click();

      // Wait for navigation
      await page.waitForLoadState('networkidle');

      // Verify URL changed
      const newUrl = page.url();
      expect(newUrl).not.toBe(detailsUrl);

      console.log(`✅ Back navigation: ${detailsUrl} → ${newUrl}`);
    });

    test('should handle external link clicks correctly', async ({ page }) => {
      // Test map link
      const mapLink = page.locator('.map-link');

      if ((await mapLink.count()) > 0) {
        // Check that external links open in new tab
        await expect(mapLink).toHaveAttribute('target', '_blank');
        await expect(mapLink).toHaveAttribute('rel', 'noopener noreferrer');

        console.log('✅ External links configured correctly');
      }
    });

    test('should handle image loading errors gracefully', async ({ page }) => {
      // Navigate to a new test NFT with broken image
      const brokenImageNFT = {
        ...testNFT,
        nftId: 'broken-image-test',
        contentUri: 'https://example.com/nonexistent-image.jpg',
      };

      await page.route('**/api/computational/nft/broken-image-test', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(brokenImageNFT),
        });
      });

      // Block the broken image request
      await page.route('https://example.com/nonexistent-image.jpg', route => {
        route.fulfill({ status: 404, body: 'Not found' });
      });

      await page.goto('http://localhost:5000/nft/broken-image-test');
      await page.waitForLoadState('networkidle');

      // Verify fallback to placeholder
      const placeholder = page.locator('.nft-placeholder');
      await expect(placeholder).toBeVisible();

      console.log('✅ Broken image handled gracefully');
    });
  });

  test.describe('Error Handling', () => {
    test('should handle NFT not found error', async ({ page }) => {
      // Mock 404 response
      await page.route('**/api/computational/nft/nonexistent-nft', route => {
        route.fulfill({
          status: 404,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'NFT not found' }),
        });
      });

      await page.goto('http://localhost:5000/nft/nonexistent-nft');
      await page.waitForTimeout(2000);

      // Check for error state
      const errorContainer = page.locator('.nft-detail-error');
      await expect(errorContainer).toBeVisible();

      const errorHeading = errorContainer.locator('h2');
      await expect(errorHeading).toContainText(/nft.*not.*found/i);

      // Check navigation options
      const goBackButton = errorContainer
        .locator('button')
        .filter({ hasText: /go.*back/i });
      const browseButton = errorContainer
        .locator('a')
        .filter({ hasText: /browse.*nft/i });

      await expect(goBackButton).toBeVisible();
      await expect(browseButton).toBeVisible();

      console.log('✅ NFT not found error handled correctly');
    });

    test('should handle server errors gracefully', async ({ page }) => {
      // Mock server error
      await page.route('**/api/computational/nft/server-error-test', route => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Internal server error' }),
        });
      });

      await page.goto('http://localhost:5000/nft/server-error-test');
      await page.waitForTimeout(2000);

      // Check for error state
      const errorContainer = page.locator('.nft-detail-error');
      await expect(errorContainer).toBeVisible();

      const errorMessage = errorContainer.locator('p');
      await expect(errorMessage).toContainText(/failed.*load.*nft/i);

      console.log('✅ Server error handled correctly');
    });

    test('should handle missing tasks gracefully', async ({ page }) => {
      // Mock empty tasks response
      await page.route(
        '**/api/computational/tasks?nftId=detail-test-001',
        route => {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify([]),
          });
        }
      );

      await page.reload();
      await page.waitForLoadState('networkidle');

      // Check for no tasks message
      const noTasksMessage = page.locator('.no-tasks');
      await expect(noTasksMessage).toBeVisible();
      await expect(noTasksMessage.locator('p')).toContainText(
        /no.*tasks.*associated/i
      );

      console.log('✅ Empty tasks state handled correctly');
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

        // Verify main elements are still visible
        await expect(page.locator('.nft-detail-container')).toBeVisible();
        await expect(page.locator('.nft-detail-header')).toBeVisible();
        await expect(page.locator('.nft-detail-content')).toBeVisible();

        // Check layout adaptation
        const mainSection = page.locator('.nft-detail-main');
        const sidebarSection = page.locator('.nft-detail-sidebar');

        if ((await mainSection.count()) > 0) {
          await expect(mainSection).toBeVisible();
        }

        if ((await sidebarSection.count()) > 0) {
          await expect(sidebarSection).toBeVisible();
        }

        // Take screenshot for verification
        await page.screenshot({
          path: `test-results/nft-details-responsive-${viewport.name}-${viewport.width}x${viewport.height}.png`,
          fullPage: true,
        });

        console.log(`✅ ${viewport.name} responsive test passed`);
      }

      // Reset viewport
      await page.setViewportSize({ width: 1200, height: 800 });
    });
  });

  test.describe('Performance', () => {
    test('should load NFT details within acceptable time', async ({ page }) => {
      const startTime = Date.now();

      await page.goto('http://localhost:5000/nft/detail-test-001');
      await page.waitForLoadState('networkidle');

      const loadTime = Date.now() - startTime;

      // Should load within 3 seconds
      expect(loadTime).toBeLessThan(3000);

      console.log(`✅ NFT details page loaded in ${loadTime}ms`);
    });

    test('should handle complex NFT data efficiently', async ({ page }) => {
      // Create NFT with very detailed metadata
      const complexNFT = {
        ...testNFT,
        nftId: 'complex-nft-test',
        description: 'A'.repeat(2000), // Very long description
        metadata: {
          ...testNFT.metadata,
          additionalData: {
            parameters: Array.from({ length: 100 }, (_, i) => ({
              param: i,
              value: Math.random(),
            })),
            history: Array.from({ length: 50 }, (_, i) => ({
              timestamp: new Date(Date.now() - i * 86400000).toISOString(),
              event: `Event ${i}`,
              details: `Details for event ${i}`,
            })),
          },
        },
      };

      await page.route('**/api/computational/nft/complex-nft-test', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(complexNFT),
        });
      });

      const startTime = Date.now();

      await page.goto('http://localhost:5000/nft/complex-nft-test');
      await page.waitForLoadState('networkidle');

      const loadTime = Date.now() - startTime;

      // Should still load efficiently
      expect(loadTime).toBeLessThan(5000);

      // Verify content is displayed
      await expect(page.locator('.nft-detail-container')).toBeVisible();

      console.log(`✅ Complex NFT data handled in ${loadTime}ms`);
    });
  });
});
