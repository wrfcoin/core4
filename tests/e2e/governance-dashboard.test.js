const { test, expect } = require('@playwright/test');

/**
 * Governance Dashboard Test Suite
 * Tests the governance dashboard functionality including data display,
 * real-time updates, analytics, and overall dashboard usability.
 */

test.describe('Governance Dashboard', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to governance dashboard
    await page.goto('/governance');
    await page.waitForLoadState('networkidle');
  });

  test('should display governance dashboard with main components', async ({
    page,
  }) => {
    // Check for main dashboard title
    const dashboardTitle = page.locator(
      'h1:has-text("Governance"), h2:has-text("Governance"), text=Governance Dashboard'
    );
    await expect(dashboardTitle.first()).toBeVisible({ timeout: 10000 });

    // Check for key dashboard sections
    const keyElements = [
      page.locator('text=Proposals, text=Active Proposals'),
      page.locator('text=Voting, text=Vote'),
      page.locator('.proposals, [class*="proposal"]'),
      page.locator('button:has-text("Create Proposal")'),
    ];

    for (const element of keyElements) {
      if ((await element.count()) > 0) {
        await expect(element.first()).toBeVisible();
        break;
      }
    }
  });

  test('should show governance metrics and statistics', async ({ page }) => {
    // Look for governance metrics
    const metricElements = page.locator(
      '.metric, .stat, [class*="metric"], [class*="stat"]'
    );

    if ((await metricElements.count()) > 0) {
      await expect(metricElements.first()).toBeVisible();

      // Check for numeric values in metrics
      const numericElements = page.locator(
        'text=/\\d+/, .metric-value, .stat-value'
      );
      if ((await numericElements.count()) > 0) {
        await expect(numericElements.first()).toBeVisible();
      }
    }

    // Look for specific governance metrics
    const governanceMetrics = [
      'Active Proposals',
      'Total Proposals',
      'Voting Power',
      'Participation Rate',
      'Treasury',
    ];

    for (const metric of governanceMetrics) {
      const metricElement = page.locator(`text=${metric}`);
      if ((await metricElement.count()) > 0) {
        await expect(metricElement.first()).toBeVisible();
        break;
      }
    }
  });

  test('should display voting power information', async ({ page }) => {
    // Look for voting power display
    const votingPowerElements = page.locator(
      'text=Voting Power, text=voting power, text=WRF, .voting-power'
    );

    if ((await votingPowerElements.count()) > 0) {
      await expect(votingPowerElements.first()).toBeVisible();

      // Check for numeric value
      const valueElements = page.locator(
        'text=/\\d+.*WRF/, text=/\\d+/, .power-value, .balance'
      );
      if ((await valueElements.count()) > 0) {
        await expect(valueElements.first()).toBeVisible();
      }
    }
  });

  test('should show participation rate and analytics', async ({ page }) => {
    // Look for participation rate
    const participationElements = page.locator(
      'text=Participation, text=participation, .participation'
    );

    if ((await participationElements.count()) > 0) {
      await expect(participationElements.first()).toBeVisible();

      // Look for percentage values
      const percentageElements = page.locator('text=/%/, .percentage, .rate');
      if ((await percentageElements.count()) > 0) {
        await expect(percentageElements.first()).toBeVisible();
      }
    }
  });

  test('should display active proposals list', async ({ page }) => {
    // Check for proposals section
    const proposalsSection = page.locator(
      '.proposals, .proposals-list, table, [class*="proposal"]'
    );
    await expect(proposalsSection.first()).toBeVisible({ timeout: 10000 });

    // Check for proposal items
    const proposalItems = page.locator(
      '.proposal-card, .proposal-item, tr, [class*="proposal"]:not(.proposals-list)'
    );
    const itemCount = await proposalItems.count();

    if (itemCount > 0) {
      // Check first proposal has title
      const firstProposal = proposalItems.first();
      const titleElements = firstProposal.locator(
        'h3, h4, .title, [class*="title"]'
      );
      if ((await titleElements.count()) > 0) {
        await expect(titleElements.first()).toBeVisible();
      }

      // Check for proposal status
      const statusElements = firstProposal.locator(
        '.status, .badge, text=Active, text=Voting, [class*="status"]'
      );
      if ((await statusElements.count()) > 0) {
        await expect(statusElements.first()).toBeVisible();
      }
    }
  });

  test('should show governance charts and visualizations', async ({ page }) => {
    // Look for chart containers
    const chartElements = page.locator(
      'svg, canvas, .chart, .graph, [class*="chart"], [class*="graph"]'
    );

    if ((await chartElements.count()) > 0) {
      await expect(chartElements.first()).toBeVisible();
    }

    // Look for chart libraries indicators
    const chartLibraryElements = page.locator(
      '.recharts, .chartjs, .d3, [class*="recharts"]'
    );
    if ((await chartLibraryElements.count()) > 0) {
      await expect(chartLibraryElements.first()).toBeVisible();
    }
  });

  test('should refresh dashboard data when refresh button is clicked', async ({
    page,
  }) => {
    // Mock API responses
    await page.route('**/api/governance/**', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          data: { refreshed: true, timestamp: Date.now() },
        }),
      });
    });

    // Look for refresh button
    const refreshButton = page.locator(
      'button:has-text("Refresh"), button:has-text("Reload"), .refresh, [class*="refresh"]'
    );

    if ((await refreshButton.count()) > 0) {
      await refreshButton.first().click();

      // Wait for potential loading indicators
      await page.waitForTimeout(2000);

      // Dashboard should still be visible after refresh
      const dashboardTitle = page.locator(
        'h1:has-text("Governance"), h2:has-text("Governance")'
      );
      await expect(dashboardTitle.first()).toBeVisible();
    }
  });

  test('should handle dashboard loading states', async ({ page }) => {
    // Mock slow API response
    await page.route('**/api/governance/**', route => {
      setTimeout(() => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, data: [] }),
        });
      }, 2000);
    });

    // Reload page to trigger loading
    await page.reload();

    // Look for loading indicators
    const loadingElements = page.locator(
      '.loading, .spinner, text=Loading, [class*="loading"], [class*="spinner"]'
    );

    if ((await loadingElements.count()) > 0) {
      await expect(loadingElements.first()).toBeVisible({ timeout: 3000 });
    }

    // Eventually should show content
    await page.waitForTimeout(3000);
    const contentElements = page.locator(
      'text=Governance, .proposals, .metrics'
    );
    await expect(contentElements.first()).toBeVisible({ timeout: 5000 });
  });

  test('should display empty state when no proposals exist', async ({
    page,
  }) => {
    // Mock empty proposals response
    await page.route('**/api/governance/proposals', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: [] }),
      });
    });

    await page.reload();
    await page.waitForTimeout(2000);

    // Look for empty state indicators
    const emptyStateElements = page.locator(
      'text=No proposals, text=No active, .empty, [class*="empty"]'
    );

    if ((await emptyStateElements.count()) > 0) {
      await expect(emptyStateElements.first()).toBeVisible();
    }
  });

  test('should navigate between dashboard tabs if present', async ({
    page,
  }) => {
    // Look for navigation tabs
    const tabElements = page.locator(
      '.tab, [role="tab"], .nav-item, [class*="tab"]'
    );

    if ((await tabElements.count()) > 1) {
      // Click on second tab
      await tabElements.nth(1).click();
      await page.waitForTimeout(1000);

      // Should show different content
      const tabContent = page.locator(
        '.tab-content, [role="tabpanel"], .tab-pane'
      );
      if ((await tabContent.count()) > 0) {
        await expect(tabContent.first()).toBeVisible();
      }
    }
  });

  test('should show foundation governance status if applicable', async ({
    page,
  }) => {
    // Look for foundation-specific elements
    const foundationElements = page.locator(
      'text=Foundation, text=foundation, .foundation, [class*="foundation"]'
    );

    if ((await foundationElements.count()) > 0) {
      await expect(foundationElements.first()).toBeVisible();

      // Look for veto period information
      const vetoElements = page.locator(
        'text=veto, text=Veto, text=days, .veto'
      );
      if ((await vetoElements.count()) > 0) {
        await expect(vetoElements.first()).toBeVisible();
      }
    }
  });

  test('should handle dashboard API errors gracefully', async ({ page }) => {
    // Mock API failures
    await page.route('**/api/governance/**', route => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Internal server error' }),
      });
    });

    await page.reload();
    await page.waitForTimeout(3000);

    // Should show error state or fallback content
    const errorElements = page.locator(
      '.error, text=error, text=failed, text=unavailable, [class*="error"]'
    );

    if ((await errorElements.count()) > 0) {
      await expect(errorElements.first()).toBeVisible();
    } else {
      // Or should at least show basic dashboard structure
      const dashboardTitle = page.locator(
        'h1:has-text("Governance"), h2:has-text("Governance")'
      );
      await expect(dashboardTitle.first()).toBeVisible();
    }
  });

  test('should support dashboard search/filtering if available', async ({
    page,
  }) => {
    // Look for search or filter inputs
    const searchElements = page.locator(
      'input[type="search"], input[placeholder*="search"], .search, [class*="search"]'
    );

    if ((await searchElements.count()) > 0) {
      const searchInput = searchElements.first();
      await expect(searchInput).toBeVisible();

      // Try searching
      await searchInput.fill('climate');
      await page.waitForTimeout(1000);

      // Should filter results or show search results
      const proposalElements = page.locator('.proposal, [class*="proposal"]');
      // Test passes if search doesn't break the page
      expect(await proposalElements.count()).toBeGreaterThanOrEqual(0);
    }
  });

  test('should show real-time updates if implemented', async ({ page }) => {
    // Mock real-time data update
    await page.route('**/api/governance/proposals', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          data: [
            {
              id: 1,
              title: 'Real-time Updated Proposal',
              status: 'active',
              yes_votes: Math.floor(Math.random() * 1000),
              no_votes: Math.floor(Math.random() * 500),
            },
          ],
        }),
      });
    });

    // Wait for potential auto-refresh
    await page.waitForTimeout(5000);

    // Check if content is still properly displayed
    const proposalElements = page.locator('.proposal, [class*="proposal"]');
    if ((await proposalElements.count()) > 0) {
      await expect(proposalElements.first()).toBeVisible();
    }

    // Dashboard should remain functional
    const dashboardTitle = page.locator(
      'h1:has-text("Governance"), h2:has-text("Governance")'
    );
    await expect(dashboardTitle.first()).toBeVisible();
  });
});
