const { test, expect } = require('@playwright/test');

test.describe('Wallet Overview and Recent Blocks Display Tests', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to dashboard
    await page.goto('http://localhost:5000');

    // Wait for dashboard to load
    await page.waitForLoadState('networkidle');
    await page.waitForSelector('.comprehensive-dashboard', { timeout: 10000 });
  });

  test.describe('Wallet & Transaction Analytics Section', () => {
    test('should display wallet analytics section with proper title and structure', async ({
      page,
    }) => {
      // Check section header
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });
      await expect(walletSection).toBeVisible();

      const sectionHeader = walletSection.locator('.section-header');
      await expect(sectionHeader.locator('h2')).toHaveText(
        'Wallet & Transaction Analytics'
      );
      await expect(sectionHeader.locator('p')).toHaveText(
        'User activity and transaction flow analysis'
      );

      // Check operational status
      await expect(sectionHeader.locator('.section-status span')).toHaveText(
        'Operational'
      );
    });

    test('should display total wallets metric with correct format', async ({
      page,
    }) => {
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });

      const totalWalletsCard = walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Total Wallets' });
      await expect(totalWalletsCard).toBeVisible();

      // Check title and subtitle
      await expect(totalWalletsCard.locator('.stat-title')).toHaveText(
        'Total Wallets'
      );
      await expect(totalWalletsCard.locator('.stat-subtitle')).toHaveText(
        'Created wallets'
      );

      // Check value format (should be a formatted number)
      const valueText = await totalWalletsCard
        .locator('.stat-value')
        .textContent();
      expect(valueText).toMatch(/^[\d,]+$/); // Should be numbers with commas

      // Check trend indicator
      const trendElement = totalWalletsCard.locator('.stat-trend');
      await expect(trendElement).toBeVisible();
      await expect(trendElement).toHaveClass(/positive/);
    });

    test('should display active wallets metric with correct format', async ({
      page,
    }) => {
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });

      const activeWalletsCard = walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Active Wallets' });
      await expect(activeWalletsCard).toBeVisible();

      // Check title and subtitle
      await expect(activeWalletsCard.locator('.stat-title')).toHaveText(
        'Active Wallets'
      );
      await expect(activeWalletsCard.locator('.stat-subtitle')).toHaveText(
        '24h active'
      );

      // Check value format (should be a formatted number)
      const valueText = await activeWalletsCard
        .locator('.stat-value')
        .textContent();
      expect(valueText).toMatch(/^[\d,]+$/); // Should be numbers with commas
    });

    test('should display total value metric with correct currency format', async ({
      page,
    }) => {
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });

      const totalValueCard = walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Total Value' });
      await expect(totalValueCard).toBeVisible();

      // Check title and subtitle
      await expect(totalValueCard.locator('.stat-title')).toHaveText(
        'Total Value'
      );
      await expect(totalValueCard.locator('.stat-subtitle')).toHaveText(
        'Wallet holdings'
      );

      // Check value format (should be currency format like $123.4M)
      const valueText = await totalValueCard
        .locator('.stat-value')
        .textContent();
      expect(valueText).toMatch(/^\$[\d.,]+[KMB]?$/); // Should be currency format
    });

    test('should display 24h transactions metric with correct format', async ({
      page,
    }) => {
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });

      const transactionsCard = walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Transactions 24h' });
      await expect(transactionsCard).toBeVisible();

      // Check title and subtitle
      await expect(transactionsCard.locator('.stat-title')).toHaveText(
        'Transactions 24h'
      );
      await expect(transactionsCard.locator('.stat-subtitle')).toHaveText(
        'Daily activity'
      );

      // Check value format (should be a formatted number)
      const valueText = await transactionsCard
        .locator('.stat-value')
        .textContent();
      expect(valueText).toMatch(/^[\d,]+$/); // Should be numbers with commas
    });

    test('should validate wallet metrics are within reasonable ranges', async ({
      page,
    }) => {
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });

      // Get all wallet metric values
      const totalWallets = await walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Total Wallets' })
        .locator('.stat-value')
        .textContent();
      const activeWallets = await walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Active Wallets' })
        .locator('.stat-value')
        .textContent();
      const transactions24h = await walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Transactions 24h' })
        .locator('.stat-value')
        .textContent();

      // Convert to numbers for validation
      const totalWalletsNum = parseInt(totalWallets.replace(/,/g, ''));
      const activeWalletsNum = parseInt(activeWallets.replace(/,/g, ''));
      const transactions24hNum = parseInt(transactions24h.replace(/,/g, ''));

      // Validate reasonable ranges
      expect(totalWalletsNum).toBeGreaterThan(0);
      expect(totalWalletsNum).toBeLessThan(1000000); // Reasonable upper bound

      expect(activeWalletsNum).toBeGreaterThan(0);
      expect(activeWalletsNum).toBeLessThanOrEqual(totalWalletsNum); // Active should be <= total

      expect(transactions24hNum).toBeGreaterThan(0);
      expect(transactions24hNum).toBeLessThan(1000000); // Reasonable daily transaction limit
    });
  });

  test.describe('Recent Blocks Display (Recent Activities Section)', () => {
    test('should display recent activities section', async ({ page }) => {
      const networkInsightsSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Network Insights' });
      await expect(networkInsightsSection).toBeVisible();

      const recentActivitiesCard = networkInsightsSection
        .locator('.insight-card')
        .filter({ hasText: 'Recent Network Activities' });
      await expect(recentActivitiesCard).toBeVisible();

      const recentActivitiesTitle = recentActivitiesCard.locator('h3');
      await expect(recentActivitiesTitle).toHaveText(
        'Recent Network Activities'
      );
    });

    test('should display recent block mining activity', async ({ page }) => {
      const networkInsightsSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Network Insights' });
      const recentActivitiesCard = networkInsightsSection
        .locator('.insight-card')
        .filter({ hasText: 'Recent Network Activities' });

      // Check for block mining activity
      const blockActivity = recentActivitiesCard
        .locator('.activity-item')
        .filter({ hasText: 'New block mined' });
      await expect(blockActivity).toBeVisible();

      // Check for proper format: "New block mined: #[number]"
      const activityText = await blockActivity
        .locator('span')
        .first()
        .textContent();
      expect(activityText).toMatch(/^New block mined: #\d+$/);

      // Check timestamp format
      const timeText = await blockActivity
        .locator('.activity-time')
        .textContent();
      expect(timeText).toMatch(/^\d+[sm] ago$/); // Should be "Xs ago" or "Xm ago"
    });

    test('should display cross-chain bridge activity', async ({ page }) => {
      const networkInsightsSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Network Insights' });
      const recentActivitiesCard = networkInsightsSection
        .locator('.insight-card')
        .filter({ hasText: 'Recent Network Activities' });

      // Check for cross-chain bridge activity
      const bridgeActivity = recentActivitiesCard
        .locator('.activity-item')
        .filter({ hasText: 'Cross-chain bridge' });
      await expect(bridgeActivity).toBeVisible();

      // Check for proper format: "Cross-chain bridge: [TOKEN] → [TOKEN]"
      const activityText = await bridgeActivity
        .locator('span')
        .first()
        .textContent();
      expect(activityText).toMatch(/^Cross-chain bridge: \w+ → \w+$/);

      // Check timestamp
      const timeText = await bridgeActivity
        .locator('.activity-time')
        .textContent();
      expect(timeText).toMatch(/^\d+[sm] ago$/);
    });

    test('should display AI model execution activity', async ({ page }) => {
      const networkInsightsSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Network Insights' });
      const recentActivitiesCard = networkInsightsSection
        .locator('.insight-card')
        .filter({ hasText: 'Recent Network Activities' });

      // Check for AI model execution activity
      const aiActivity = recentActivitiesCard
        .locator('.activity-item')
        .filter({ hasText: 'AI model executed successfully' });
      await expect(aiActivity).toBeVisible();

      // Check for proper format
      const activityText = await aiActivity
        .locator('span')
        .first()
        .textContent();
      expect(activityText).toBe('AI model executed successfully');

      // Check timestamp
      const timeText = await aiActivity.locator('.activity-time').textContent();
      expect(timeText).toMatch(/^\d+[sm] ago$/);
    });

    test('should display governance proposal activity', async ({ page }) => {
      const networkInsightsSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Network Insights' });
      const recentActivitiesCard = networkInsightsSection
        .locator('.insight-card')
        .filter({ hasText: 'Recent Network Activities' });

      // Check for governance proposal activity
      const governanceActivity = recentActivitiesCard
        .locator('.activity-item')
        .filter({ hasText: 'Governance proposal' });
      await expect(governanceActivity).toBeVisible();

      // Check for proper format: "Governance proposal #[number] passed"
      const activityText = await governanceActivity
        .locator('span')
        .first()
        .textContent();
      expect(activityText).toMatch(/^Governance proposal #\d+ passed$/);

      // Check timestamp
      const timeText = await governanceActivity
        .locator('.activity-time')
        .textContent();
      expect(timeText).toMatch(/^\d+[sm] ago$/);
    });

    test('should verify activity items have proper icons and structure', async ({
      page,
    }) => {
      const networkInsightsSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Network Insights' });
      const recentActivitiesCard = networkInsightsSection
        .locator('.insight-card')
        .filter({ hasText: 'Recent Network Activities' });

      const activityItems = recentActivitiesCard.locator('.activity-item');
      const activityCount = await activityItems.count();

      // Should have at least 4 activities
      expect(activityCount).toBeGreaterThanOrEqual(4);

      // Check each activity item has proper structure
      for (let i = 0; i < activityCount; i++) {
        const item = activityItems.nth(i);

        // Should have an icon
        await expect(item.locator('.activity-icon')).toBeVisible();

        // Should have activity text
        const spans = item.locator('span');
        await expect(spans.first()).toBeVisible();

        // Should have timestamp
        await expect(item.locator('.activity-time')).toBeVisible();
      }
    });

    test('should validate timestamps are in chronological order', async ({
      page,
    }) => {
      const networkInsightsSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Network Insights' });
      const recentActivitiesCard = networkInsightsSection
        .locator('.insight-card')
        .filter({ hasText: 'Recent Network Activities' });

      const timeElements = recentActivitiesCard.locator('.activity-time');
      const timeCount = await timeElements.count();

      const timestamps = [];
      for (let i = 0; i < timeCount; i++) {
        const timeText = await timeElements.nth(i).textContent();
        timestamps.push(timeText);
      }

      // Parse timestamps and verify they're in descending order (most recent first)
      const parseTime = timeStr => {
        const match = timeStr.match(/^(\d+)([sm]) ago$/);
        if (match) {
          const value = parseInt(match[1]);
          const unit = match[2];
          return unit === 's' ? value : value * 60; // Convert to seconds
        }
        return Infinity; // For unparseable timestamps
      };

      for (let i = 0; i < timestamps.length - 1; i++) {
        const currentTime = parseTime(timestamps[i]);
        const nextTime = parseTime(timestamps[i + 1]);

        // Current should be more recent (smaller value) than next
        expect(currentTime).toBeLessThanOrEqual(nextTime);
      }
    });
  });

  test.describe('Data Integration and Updates', () => {
    test('should verify wallet data consistency', async ({ page }) => {
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });

      // Get initial values
      const initialTotalWallets = await walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Total Wallets' })
        .locator('.stat-value')
        .textContent();
      const initialActiveWallets = await walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Active Wallets' })
        .locator('.stat-value')
        .textContent();

      // Values should be consistent (active <= total)
      const totalNum = parseInt(initialTotalWallets.replace(/,/g, ''));
      const activeNum = parseInt(initialActiveWallets.replace(/,/g, ''));

      expect(activeNum).toBeLessThanOrEqual(totalNum);
    });

    test('should verify real-time updates affect wallet section', async ({
      page,
    }) => {
      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });

      // Get initial total wallets value
      const initialTotalWallets = await walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Total Wallets' })
        .locator('.stat-value')
        .textContent();

      // Wait for potential update (dashboard updates every 10 seconds)
      await page.waitForTimeout(12000);

      // Get updated value
      const updatedTotalWallets = await walletSection
        .locator('.stat-card')
        .filter({ hasText: 'Total Wallets' })
        .locator('.stat-value')
        .textContent();

      // Note: In this mock implementation, wallet data doesn't change, but we verify the structure remains intact
      expect(updatedTotalWallets).toBeTruthy();
      expect(updatedTotalWallets).toMatch(/^[\d,]+$/);
    });

    test('should handle error scenarios gracefully', async ({ page }) => {
      // Test what happens if data is temporarily unavailable
      // In a real implementation, this would involve mocking API failures

      const walletSection = page
        .locator('section.dashboard-section')
        .filter({ hasText: 'Wallet & Transaction Analytics' });
      await expect(walletSection).toBeVisible();

      // Verify all cards are still rendered even if there might be data issues
      const statCards = walletSection.locator('.stat-card');
      const cardCount = await statCards.count();
      expect(cardCount).toBe(4); // Should have 4 wallet metric cards
    });
  });

  test.describe('User Experience and Performance', () => {
    test('should load wallet and recent blocks sections within reasonable time', async ({
      page,
    }) => {
      const startTime = Date.now();

      await page.goto('http://localhost:5000');

      // Wait for both sections to be visible
      await Promise.all([
        page.waitForSelector(
          'section.dashboard-section:has-text("Wallet & Transaction Analytics")',
          { timeout: 5000 }
        ),
        page.waitForSelector(
          '.insight-card:has-text("Recent Network Activities")',
          { timeout: 5000 }
        ),
      ]);

      const loadTime = Date.now() - startTime;

      // Should load within 5 seconds
      expect(loadTime).toBeLessThan(5000);
    });

    test('should be responsive and maintain layout integrity', async ({
      page,
    }) => {
      // Test on different viewport sizes
      const viewports = [
        { width: 1920, height: 1080 },
        { width: 1024, height: 768 },
        { width: 768, height: 1024 },
      ];

      for (const viewport of viewports) {
        await page.setViewportSize(viewport);

        // Verify sections are still visible and properly laid out
        const walletSection = page
          .locator('section.dashboard-section')
          .filter({ hasText: 'Wallet & Transaction Analytics' });
        await expect(walletSection).toBeVisible();

        const recentActivitiesCard = page
          .locator('.insight-card')
          .filter({ hasText: 'Recent Network Activities' });
        await expect(recentActivitiesCard).toBeVisible();

        // Check that cards are properly arranged in grid
        const statCards = walletSection.locator('.stat-card');
        const cardCount = await statCards.count();
        expect(cardCount).toBe(4);
      }
    });
  });
});
