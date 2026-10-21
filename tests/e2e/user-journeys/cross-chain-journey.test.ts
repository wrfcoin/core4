import { test, expect, type Page } from '@playwright/test';
import { USER_JOURNEYS } from './journey-definitions';
import { USER_PERSONAS } from './user-personas';
import { TEST_CONSTANTS } from '../test-data/test-constants';
import { TestDataLifecycleManager } from '../test-data-management/data-lifecycle';

// Cross-Chain User Journey Tests
test.describe('Cross-Chain User Journeys', () => {
  let lifecycleManager: TestDataLifecycleManager;
  let sessionId: string;

  test.beforeEach(async ({ page }) => {
    lifecycleManager = TestDataLifecycleManager.getInstance();
    sessionId = await lifecycleManager.startTestSession(
      page,
      'cross-chain-journey',
      {
        isolateUsers: true,
        isolateWallets: true,
        isolateTransactions: true,
        isolateDevices: false,
        useRandomPrefixes: true,
        cleanup: 'test-end',
      }
    );
  });

  test.afterEach(async () => {
    if (sessionId) {
      await lifecycleManager.endTestSession(sessionId);
    }
  });

  // Multi-Chain Investor Journey
  test.describe('Multi-Chain Asset Bridging', () => {
    const journey = USER_JOURNEYS.find(
      j => j.id === 'multi_chain_asset_bridge'
    );
    const persona = USER_PERSONAS.find(p => p.id === 'multi_chain_investor');

    test('should complete full cross-chain asset bridging journey', async ({
      page,
    }) => {
      // Create recovery point at start
      await lifecycleManager.createRecoveryPoint(
        sessionId,
        page,
        'Cross-chain journey start'
      );

      // Step 1: Bridge Dashboard Access
      await test.step('Access Cross-Chain Bridge Dashboard', async () => {
        await page.goto('/bridge');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="bridge-dashboard"]')
        ).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(2500);

        // Connect multiple wallets for different chains
        await page.click('[data-testid="connect-ethereum-wallet"]');
        await expect(
          page.locator('[data-testid="ethereum-wallet-connected"]')
        ).toBeVisible();

        await page.click('[data-testid="connect-polygon-wallet"]');
        await expect(
          page.locator('[data-testid="polygon-wallet-connected"]')
        ).toBeVisible();

        // Verify multi-chain portfolio view
        await expect(
          page.locator('[data-testid="ethereum-balance"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="polygon-balance"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="total-portfolio-value"]')
        ).toBeVisible();

        // Check supported chains are displayed
        await expect(
          page.locator('[data-testid="supported-chains"]')
        ).toBeVisible();
        const chainCount = await page
          .locator('[data-testid^="chain-"]')
          .count();
        expect(chainCount).toBeGreaterThanOrEqual(4); // At least 4 chains supported
      });

      // Step 2: Bridge Route Analysis
      await test.step('Analyze Bridge Routes and Fees', async () => {
        await page.click('[data-testid="start-bridge-transaction"]');
        await expect(page.locator('[data-testid="bridge-form"]')).toBeVisible();

        // Select source and destination chains
        await page.selectOption('[data-testid="source-chain"]', 'ethereum');
        await page.selectOption('[data-testid="destination-chain"]', 'polygon');
        await page.selectOption('[data-testid="token-select"]', 'WRF');
        await page.fill('[data-testid="amount-input"]', '1000');

        const startTime = Date.now();
        await page.click('[data-testid="get-bridge-quotes"]');

        await expect(
          page.locator('[data-testid="bridge-routes"]')
        ).toBeVisible();

        const routeAnalysisTime = Date.now() - startTime;
        expect(routeAnalysisTime).toBeLessThan(3000);

        // Verify multiple route options are displayed
        const routes = await page.locator('[data-testid^="route-"]').all();
        expect(routes.length).toBeGreaterThanOrEqual(2);

        // Check route information completeness
        for (let i = 0; i < Math.min(routes.length, 3); i++) {
          await expect(
            page.locator(`[data-testid="route-${i}-fee"]`)
          ).toBeVisible();
          await expect(
            page.locator(`[data-testid="route-${i}-time"]`)
          ).toBeVisible();
          await expect(
            page.locator(`[data-testid="route-${i}-security"]`)
          ).toBeVisible();
          await expect(
            page.locator(`[data-testid="route-${i}-provider"]`)
          ).toBeVisible();
        }

        // Verify route comparison features
        await expect(
          page.locator('[data-testid="route-comparison-table"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="recommended-route"]')
        ).toBeVisible();

        // Select optimal route (lowest fee + reasonable time)
        await page.click('[data-testid="select-recommended-route"]');
        await expect(
          page.locator('[data-testid="route-selected"]')
        ).toBeVisible();
      });

      // Step 3: Bridge Transaction Execution
      await test.step('Execute Cross-Chain Bridge Transaction', async () => {
        // Review transaction details
        await expect(
          page.locator('[data-testid="transaction-summary"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="source-amount"]')
        ).toContainText('1000 WRF');
        await expect(
          page.locator('[data-testid="destination-amount"]')
        ).toBeVisible();
        await expect(page.locator('[data-testid="bridge-fee"]')).toBeVisible();
        await expect(
          page.locator('[data-testid="estimated-time"]')
        ).toBeVisible();

        // Check security warnings and confirmations
        await expect(
          page.locator('[data-testid="bridge-security-info"]')
        ).toBeVisible();
        await page.check('[data-testid="understand-risks-checkbox"]');
        await page.check('[data-testid="confirm-details-checkbox"]');

        // Execute bridge transaction
        await page.click('[data-testid="execute-bridge"]');

        // Handle wallet confirmation (mock)
        await expect(
          page.locator('[data-testid="wallet-confirmation-pending"]')
        ).toBeVisible();
        await page.click('[data-testid="confirm-in-wallet"]');

        // Verify transaction initiated
        await expect(
          page.locator('[data-testid="bridge-transaction-initiated"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="transaction-hash"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="tracking-url"]')
        ).toBeVisible();

        // Get tracking information
        const trackingUrl = await page
          .locator('[data-testid="tracking-url"]')
          .getAttribute('href');
        expect(trackingUrl).toContain('/bridge/track/');

        const txHash = await page
          .locator('[data-testid="transaction-hash"]')
          .textContent();
        expect(txHash).toMatch(/^0x[a-fA-F0-9]{64}$/);
      });

      // Step 4: Bridge Transaction Monitoring
      await test.step('Monitor Bridge Transaction Progress', async () => {
        // Navigate to tracking page
        await page.click('[data-testid="track-transaction"]');
        await expect(
          page.locator('[data-testid="bridge-tracking"]')
        ).toBeVisible();

        // Verify tracking stages are displayed
        await expect(
          page.locator('[data-testid="stage-lock-tokens"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="stage-validate-bridge"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="stage-mint-tokens"]')
        ).toBeVisible();

        // Simulate bridge progress
        await page.click('[data-testid="simulate-bridge-progress"]');

        // Check progress updates
        await expect(
          page.locator('[data-testid="stage-lock-tokens-completed"]')
        ).toBeVisible({ timeout: 5000 });
        await expect(
          page.locator('[data-testid="stage-validate-bridge-pending"]')
        ).toBeVisible();

        // Wait for validation
        await expect(
          page.locator('[data-testid="stage-validate-bridge-completed"]')
        ).toBeVisible({ timeout: 10000 });
        await expect(
          page.locator('[data-testid="stage-mint-tokens-pending"]')
        ).toBeVisible();

        // Complete bridge
        await expect(
          page.locator('[data-testid="stage-mint-tokens-completed"]')
        ).toBeVisible({ timeout: 15000 });
        await expect(
          page.locator('[data-testid="bridge-completed"]')
        ).toBeVisible();

        // Verify completion details
        await expect(
          page.locator('[data-testid="destination-transaction-hash"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="final-amount-received"]')
        ).toContainText('WRF');

        const totalTime = await page
          .locator('[data-testid="total-bridge-time"]')
          .textContent();
        expect(totalTime).toMatch(/\d+:\d+/); // Should show time in MM:SS format
      });

      // Step 5: Portfolio Rebalancing
      await test.step('Rebalance Cross-Chain Portfolio', async () => {
        await page.goto('/portfolio');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="cross-chain-portfolio"]')
        ).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(5000);

        // Verify updated balances after bridge
        await expect(
          page.locator('[data-testid="ethereum-wrf-balance"]')
        ).not.toContainText('1000'); // Should be reduced
        await expect(
          page.locator('[data-testid="polygon-wrf-balance"]')
        ).toContainText('WRF'); // Should show received tokens

        // Access portfolio optimizer
        await page.click('[data-testid="optimize-portfolio"]');
        await expect(
          page.locator('[data-testid="portfolio-optimizer"]')
        ).toBeVisible();

        // Analyze current allocation
        await page.click('[data-testid="analyze-allocation"]');
        await expect(
          page.locator('[data-testid="allocation-analysis"]')
        ).toBeVisible();

        // Check risk distribution
        await expect(
          page.locator('[data-testid="chain-risk-distribution"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="yield-opportunities"]')
        ).toBeVisible();

        // Apply optimization recommendations
        await page.click('[data-testid="apply-recommendations"]');
        await expect(
          page.locator('[data-testid="optimization-applied"]')
        ).toBeVisible();

        // Verify portfolio is better balanced
        const ethereumAllocation = await page
          .locator('[data-testid="ethereum-allocation-percentage"]')
          .textContent();
        const polygonAllocation = await page
          .locator('[data-testid="polygon-allocation-percentage"]')
          .textContent();

        const ethPercent = parseFloat(
          ethereumAllocation?.replace('%', '') || '0'
        );
        const polyPercent = parseFloat(
          polygonAllocation?.replace('%', '') || '0'
        );

        // Neither chain should have more than 60% allocation (reasonable diversification)
        expect(ethPercent).toBeLessThan(60);
        expect(polyPercent).toBeLessThan(60);
      });

      // Verify overall journey success criteria
      await test.step('Verify Journey Completion', async () => {
        // Check bridge transaction completed successfully
        await expect(
          page.locator('[data-testid="recent-bridge-success"]')
        ).toBeVisible();

        // Verify cross-chain balances are correct
        const totalValue = await page
          .locator('[data-testid="total-portfolio-value"]')
          .textContent();
        expect(
          parseFloat(totalValue?.replace(/[^0-9.]/g, '') || '0')
        ).toBeGreaterThan(0);

        // Check portfolio optimization completed
        await expect(
          page.locator('[data-testid="portfolio-optimized"]')
        ).toBeVisible();

        // Verify user has learned cross-chain operations
        await expect(
          page.locator('[data-testid="bridge-experience-level"]')
        ).toContainText(/Intermediate|Advanced/);

        // Check transaction history
        await page.click('[data-testid="view-transaction-history"]');
        await expect(
          page.locator('[data-testid="bridge-transaction-history"]')
        ).toBeVisible();
      });
    });

    test('should handle bridge failure scenarios gracefully', async ({
      page,
    }) => {
      await page.goto('/bridge');
      await page.click('[data-testid="start-bridge-transaction"]');

      // Set up bridge transaction
      await page.selectOption('[data-testid="source-chain"]', 'ethereum');
      await page.selectOption('[data-testid="destination-chain"]', 'polygon');
      await page.fill('[data-testid="amount-input"]', '1000');

      // Mock bridge failure
      await page.route('**/api/bridge/execute', route => {
        route.fulfill({
          status: 400,
          body: JSON.stringify({
            error: 'Bridge capacity exceeded',
            message:
              'Current bridge capacity is insufficient for this transaction',
          }),
        });
      });

      await page.click('[data-testid="get-bridge-quotes"]');
      await page.click('[data-testid="execute-bridge"]');

      // Verify error handling
      await expect(page.locator('[data-testid="bridge-error"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="error-explanation"]')
      ).toContainText('capacity exceeded');
      await expect(page.locator('[data-testid="retry-options"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="alternative-routes"]')
      ).toBeVisible();
    });

    test('should validate bridge transaction limits', async ({ page }) => {
      await page.goto('/bridge');
      await page.click('[data-testid="start-bridge-transaction"]');

      // Test minimum amount validation
      await page.selectOption('[data-testid="source-chain"]', 'ethereum');
      await page.selectOption('[data-testid="destination-chain"]', 'polygon');
      await page.fill('[data-testid="amount-input"]', '0.001'); // Very small amount

      await page.click('[data-testid="get-bridge-quotes"]');
      await expect(
        page.locator('[data-testid="minimum-amount-error"]')
      ).toBeVisible();

      // Test maximum amount validation
      await page.fill('[data-testid="amount-input"]', '10000000'); // Very large amount
      await page.click('[data-testid="get-bridge-quotes"]');
      await expect(
        page.locator('[data-testid="maximum-amount-error"]')
      ).toBeVisible();
    });

    test('should handle network switching correctly', async ({ page }) => {
      await page.goto('/bridge');

      // Connect to Ethereum wallet
      await page.click('[data-testid="connect-ethereum-wallet"]');
      await expect(
        page.locator('[data-testid="ethereum-wallet-connected"]')
      ).toBeVisible();

      // Start bridge to Polygon
      await page.click('[data-testid="start-bridge-transaction"]');
      await page.selectOption('[data-testid="source-chain"]', 'ethereum');
      await page.selectOption('[data-testid="destination-chain"]', 'polygon');

      // Mock network switch request
      await expect(
        page.locator('[data-testid="network-switch-required"]')
      ).toBeVisible();
      await page.click('[data-testid="switch-to-polygon"]');

      // Verify network switch handling
      await expect(
        page.locator('[data-testid="switching-network"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="polygon-network-active"]')
      ).toBeVisible();
    });
  });

  // NFT Collector Journey
  test.describe('Cross-Chain NFT Operations', () => {
    const persona = USER_PERSONAS.find(p => p.id === 'nft_collector');

    test('should handle cross-chain NFT trading', async ({ page }) => {
      await page.goto('/nft/marketplace');
      await expect(
        page.locator('[data-testid="nft-marketplace"]')
      ).toBeVisible();

      // Filter for computational NFTs
      await page.click('[data-testid="filter-computational-nfts"]');
      await expect(
        page.locator('[data-testid="computational-nft-list"]')
      ).toBeVisible();

      // Select an NFT from different chain
      await page.click('[data-testid="nft-from-polygon"]');
      await expect(page.locator('[data-testid="nft-details"]')).toBeVisible();

      // Verify cross-chain purchase flow
      await page.click('[data-testid="buy-nft-button"]');
      await expect(
        page.locator('[data-testid="cross-chain-purchase-info"]')
      ).toBeVisible();

      // Check bridge integration for payment
      await expect(
        page.locator('[data-testid="bridge-payment-option"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="estimated-total-cost"]')
      ).toBeVisible();
    });

    test('should validate NFT authenticity across chains', async ({ page }) => {
      await page.goto('/nft/verify');

      // Enter NFT contract address and token ID
      await page.fill(
        '[data-testid="contract-address"]',
        '0x1234567890123456789012345678901234567890'
      );
      await page.fill('[data-testid="token-id"]', '123');
      await page.selectOption('[data-testid="chain-select"]', 'polygon');

      await page.click('[data-testid="verify-nft"]');

      // Verify authenticity check results
      await expect(
        page.locator('[data-testid="authenticity-result"]')
      ).toBeVisible();
      await expect(page.locator('[data-testid="nft-metadata"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="ownership-history"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="cross-chain-provenance"]')
      ).toBeVisible();
    });
  });

  // Performance tests for cross-chain operations
  test.describe('Cross-Chain Performance', () => {
    test('should maintain performance under load', async ({ page }) => {
      await page.goto('/bridge');

      // Test rapid route calculations
      const routes = ['ethereum-polygon', 'ethereum-bsc', 'polygon-avalanche'];

      for (const route of routes) {
        const [source, destination] = route.split('-');

        await page.selectOption('[data-testid="source-chain"]', source);
        await page.selectOption(
          '[data-testid="destination-chain"]',
          destination
        );
        await page.fill('[data-testid="amount-input"]', '100');

        const startTime = Date.now();
        await page.click('[data-testid="get-bridge-quotes"]');
        await expect(
          page.locator('[data-testid="bridge-routes"]')
        ).toBeVisible();

        const responseTime = Date.now() - startTime;
        expect(responseTime).toBeLessThan(5000); // Should respond within 5 seconds
      }
    });

    test('should handle concurrent bridge operations', async ({
      page,
      context,
    }) => {
      // Open multiple tabs for concurrent operations
      const page2 = await context.newPage();

      // Start bridge operations in both tabs
      await Promise.all([page.goto('/bridge'), page2.goto('/bridge')]);

      // Both should work independently
      await Promise.all([
        page.click('[data-testid="start-bridge-transaction"]'),
        page2.click('[data-testid="start-bridge-transaction"]'),
      ]);

      await Promise.all([
        expect(page.locator('[data-testid="bridge-form"]')).toBeVisible(),
        expect(page2.locator('[data-testid="bridge-form"]')).toBeVisible(),
      ]);
    });
  });
});
