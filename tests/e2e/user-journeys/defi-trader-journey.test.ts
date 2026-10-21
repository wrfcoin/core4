import { test, expect, type Page } from '@playwright/test';
import { USER_JOURNEYS } from './journey-definitions';
import { USER_PERSONAS } from './user-personas';
import { TEST_CONSTANTS } from '../test-data/test-constants';
import { TestDataLifecycleManager } from '../test-data-management/data-lifecycle';
import { MockServiceManager } from '../test-data-management/mock-services';

// DeFi Trader Journey Tests
test.describe('DeFi Participant User Journeys', () => {
  let lifecycleManager: TestDataLifecycleManager;
  let sessionId: string;

  test.beforeEach(async ({ page }) => {
    lifecycleManager = TestDataLifecycleManager.getInstance();
    sessionId = await lifecycleManager.startTestSession(
      page,
      'defi-trader-journey',
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

  // Test for Crypto Newcomer DeFi Journey
  test.describe('Crypto Newcomer DeFi Exploration', () => {
    const journey = USER_JOURNEYS.find(
      j => j.id === 'crypto_newcomer_defi_exploration'
    );
    const persona = USER_PERSONAS.find(p => p.id === 'crypto_newcomer');

    test('should complete first-time DeFi exploration journey', async ({
      page,
    }) => {
      // Create recovery point at start
      await lifecycleManager.createRecoveryPoint(
        sessionId,
        page,
        'Journey start'
      );

      // Step 1: DeFi Education Hub
      await test.step('Access DeFi Education Resources', async () => {
        await page.goto('/defi/learn');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="education-hub"]')
        ).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(3000);

        // Verify education content is present
        await expect(
          page.locator('[data-testid="defi-basics-course"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="yield-farming-guide"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="risk-assessment-tool"]')
        ).toBeVisible();

        // Complete basic course
        await page.click('[data-testid="start-defi-basics"]');

        // Navigate through course modules
        const modules = [
          'what-is-defi',
          'liquidity-pools',
          'yield-farming',
          'risks-and-rewards',
        ];
        for (const module of modules) {
          await expect(
            page.locator(`[data-testid="module-${module}"]`)
          ).toBeVisible();
          await page.click(`[data-testid="complete-module-${module}"]`);
        }

        await expect(
          page.locator('[data-testid="course-completed"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="knowledge-score"]')
        ).toContainText(/[8-9][0-9]%|100%/); // 80%+ score
      });

      // Step 2: Practice Mode
      await test.step('Enable Practice Mode', async () => {
        await page.click('[data-testid="enable-practice-mode"]');
        await expect(
          page.locator('[data-testid="practice-mode-active"]')
        ).toBeVisible();

        // Verify test tokens are available
        await expect(
          page.locator('[data-testid="test-wrf-balance"]')
        ).toContainText('10000');
        await expect(
          page.locator('[data-testid="test-eth-balance"]')
        ).toContainText('10');

        // Verify practice mode disclaimer
        await expect(
          page.locator('[data-testid="practice-mode-disclaimer"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="practice-mode-disclaimer"]')
        ).toContainText('This is practice mode');
      });

      // Step 3: Explore Liquidity Pools
      await test.step('Explore Liquidity Pools', async () => {
        await page.click('[data-testid="nav-liquidity-pools"]');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="liquidity-pools-page"]')
        ).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(2500);

        // Verify pools are displayed with key information
        await expect(
          page.locator('[data-testid="pool-WRF-ETH"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="pool-WRF-CARBON"]')
        ).toBeVisible();

        // Check pool information display
        await expect(
          page.locator('[data-testid="pool-WRF-ETH-apy"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="pool-WRF-ETH-tvl"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="pool-WRF-ETH-fees"]')
        ).toBeVisible();

        // Verify risk indicators for newcomer
        await expect(
          page.locator('[data-testid="pool-risk-low"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="pool-risk-explanation"]')
        ).toBeVisible();

        // Select conservative pool
        await page.click('[data-testid="pool-WRF-ETH"]');
        await expect(
          page.locator('[data-testid="pool-details"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="pool-risk-assessment"]')
        ).toContainText('Low Risk');
      });

      // Step 4: Small Liquidity Provision
      await test.step('Provide Liquidity (Small Amount)', async () => {
        await page.click('[data-testid="add-liquidity-button"]');
        await expect(
          page.locator('[data-testid="liquidity-form"]')
        ).toBeVisible();

        // Enter small amounts appropriate for newcomer
        await page.fill('[data-testid="wrf-amount-input"]', '100');
        await page.fill('[data-testid="eth-amount-input"]', '0.05');

        // Verify slippage warning for newcomer
        await expect(
          page.locator('[data-testid="slippage-info"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="impermanent-loss-warning"]')
        ).toBeVisible();

        // Check price impact
        const priceImpact = await page
          .locator('[data-testid="price-impact"]')
          .textContent();
        expect(parseFloat(priceImpact?.replace('%', '') || '0')).toBeLessThan(
          1
        ); // Less than 1% price impact

        // Approve tokens (practice mode)
        await page.click('[data-testid="approve-wrf-button"]');
        await expect(
          page.locator('[data-testid="wrf-approved"]')
        ).toBeVisible();

        await page.click('[data-testid="approve-eth-button"]');
        await expect(
          page.locator('[data-testid="eth-approved"]')
        ).toBeVisible();

        // Add liquidity
        await page.click('[data-testid="add-liquidity-submit"]');

        // In practice mode, show immediate success
        await expect(
          page.locator('[data-testid="liquidity-success"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="lp-tokens-received"]')
        ).toBeVisible();

        // Verify LP tokens are shown
        const lpTokens = await page
          .locator('[data-testid="lp-tokens-amount"]')
          .textContent();
        expect(parseFloat(lpTokens || '0')).toBeGreaterThan(0);
      });

      // Step 5: Yield Monitoring Setup
      await test.step('Set Up Yield Monitoring', async () => {
        await page.click('[data-testid="setup-monitoring"]');
        await expect(
          page.locator('[data-testid="monitoring-config"]')
        ).toBeVisible();

        // Configure alerts for newcomer (conservative settings)
        await page.check('[data-testid="enable-yield-alerts"]');
        await page.selectOption('[data-testid="alert-threshold"]', '5'); // 5% change threshold
        await page.fill(
          '[data-testid="notification-email"]',
          persona?.profile.name + '@test.wrfcoin.com'
        );

        // Enable beginner-friendly notifications
        await page.check('[data-testid="daily-summary"]');
        await page.check('[data-testid="weekly-report"]');
        await page.check('[data-testid="educational-tips"]');

        await page.click('[data-testid="save-monitoring-settings"]');

        await expect(
          page.locator('[data-testid="monitoring-active"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="dashboard-configured"]')
        ).toBeVisible();

        // Verify monitoring dashboard shows key metrics
        await expect(page.locator('[data-testid="current-apy"]')).toBeVisible();
        await expect(page.locator('[data-testid="daily-yield"]')).toBeVisible();
        await expect(page.locator('[data-testid="total-value"]')).toBeVisible();
        await expect(
          page.locator('[data-testid="impermanent-loss"]')
        ).toBeVisible();
      });

      // Step 6: First Yield Harvest
      await test.step('Harvest First Yield', async () => {
        // In practice mode, simulate yield accumulation
        await page.click('[data-testid="simulate-yield-accumulation"]');
        await expect(
          page.locator('[data-testid="yield-accumulated"]')
        ).toBeVisible();

        // Check rewards dashboard
        await page.click('[data-testid="nav-rewards"]');
        await expect(
          page.locator('[data-testid="claimable-rewards"]')
        ).toBeVisible();

        const claimableAmount = await page
          .locator('[data-testid="rewards-amount"]')
          .textContent();
        expect(
          parseFloat(claimableAmount?.replace(' WRF', '') || '0')
        ).toBeGreaterThan(0);

        const startTime = Date.now();
        await page.click('[data-testid="claim-rewards-button"]');

        await expect(
          page.locator('[data-testid="claim-success"]')
        ).toBeVisible();

        const claimTime = Date.now() - startTime;
        expect(claimTime).toBeLessThan(5000);

        // Verify rewards are added to wallet balance
        await expect(
          page.locator('[data-testid="wallet-balance-updated"]')
        ).toBeVisible();

        // Check transaction history
        await page.click('[data-testid="view-transaction-history"]');
        await expect(
          page.locator('[data-testid="harvest-transaction"]')
        ).toBeVisible();
      });

      // Verify overall journey success criteria
      await test.step('Verify Journey Completion', async () => {
        // Check that user has completed all educational requirements
        await expect(
          page.locator('[data-testid="defi-basics-completed"]')
        ).toBeVisible();

        // Verify user has active position
        await page.goto('/portfolio');
        await expect(
          page.locator('[data-testid="active-liquidity-position"]')
        ).toBeVisible();

        // Check that monitoring is active
        await expect(
          page.locator('[data-testid="monitoring-status-active"]')
        ).toBeVisible();

        // Verify user has earned rewards
        const earnedRewards = await page
          .locator('[data-testid="total-earned-rewards"]')
          .textContent();
        expect(
          parseFloat(earnedRewards?.replace(' WRF', '') || '0')
        ).toBeGreaterThan(0);

        // Check user confidence level (survey or interaction tracking)
        await expect(
          page.locator('[data-testid="user-confidence-level"]')
        ).toContainText(/Confident|Good/);
      });
    });

    test('should handle educational content accessibility', async ({
      page,
    }) => {
      await page.goto('/defi/learn');

      // Test keyboard navigation
      await page.keyboard.press('Tab');
      await expect(page.locator(':focus')).toBeVisible();

      // Test screen reader compatibility
      const course = page.locator('[data-testid="defi-basics-course"]');
      await expect(course).toHaveAttribute('role', 'button');
      await expect(course).toHaveAttribute('aria-label');

      // Test high contrast mode
      await page.emulateMedia({ colorScheme: 'dark' });
      await expect(page.locator('[data-testid="education-hub"]')).toBeVisible();
    });

    test('should provide appropriate warnings for newcomers', async ({
      page,
    }) => {
      await page.goto('/defi/pools');
      await page.click('[data-testid="pool-WRF-ETH"]');
      await page.click('[data-testid="add-liquidity-button"]');

      // Check risk warnings are prominent
      await expect(
        page.locator('[data-testid="newcomer-warning"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="impermanent-loss-explanation"]')
      ).toBeVisible();

      // Verify quiz or confirmation required
      await expect(
        page.locator('[data-testid="risk-understanding-quiz"]')
      ).toBeVisible();

      // Test large amount warning
      await page.fill('[data-testid="wrf-amount-input"]', '5000');
      await expect(
        page.locator('[data-testid="large-amount-warning"]')
      ).toBeVisible();
    });
  });

  // Test for Experienced Trader Journey
  test.describe('Experienced Trader Advanced Strategies', () => {
    const journey = USER_JOURNEYS.find(
      j => j.id === 'experienced_trader_advanced_strategies'
    );

    test.beforeEach(async ({ page }) => {
      // Set up experienced trader context
      await page.evaluate(() => {
        localStorage.setItem('user-experience-level', 'expert');
        localStorage.setItem('defi-mode', 'advanced');
      });
    });

    test('should access advanced trading features', async ({ page }) => {
      // Step 1: Advanced Dashboard Access
      await test.step('Access Advanced Trading Dashboard', async () => {
        await page.goto('/trading/advanced');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="advanced-dashboard"]')
        ).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(2000);

        // Verify advanced features are available
        await expect(
          page.locator('[data-testid="arbitrage-scanner"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="flash-loan-interface"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="advanced-charting"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="risk-management-tools"]')
        ).toBeVisible();
      });

      // Step 2: Arbitrage Opportunity Scanner
      await test.step('Scan for Arbitrage Opportunities', async () => {
        await page.click('[data-testid="arbitrage-scanner"]');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="arbitrage-opportunities"]')
        ).toBeVisible();

        const scanTime = Date.now() - startTime;
        expect(scanTime).toBeLessThan(1500);

        // Verify arbitrage opportunities are displayed
        await expect(
          page.locator('[data-testid="opportunity-list"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="profit-potential"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="risk-assessment"]')
        ).toBeVisible();

        // Check that opportunities are ranked by profit
        const opportunities = await page
          .locator('[data-testid^="opportunity-"]')
          .all();
        expect(opportunities.length).toBeGreaterThan(0);
      });

      // Step 3: Flash Loan Arbitrage Execution
      await test.step('Execute Flash Loan Arbitrage', async () => {
        await page.click('[data-testid="opportunity-1"]');
        await page.click('[data-testid="execute-arbitrage"]');

        // Configure flash loan parameters
        await expect(
          page.locator('[data-testid="flash-loan-config"]')
        ).toBeVisible();
        await page.fill('[data-testid="flash-loan-amount"]', '10000');
        await page.selectOption('[data-testid="flash-loan-provider"]', 'aave');

        // Review arbitrage steps
        await expect(
          page.locator('[data-testid="arbitrage-steps"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="estimated-profit"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="gas-estimation"]')
        ).toBeVisible();

        // Execute (in test mode)
        await page.click('[data-testid="execute-flash-loan"]');

        // Verify execution tracking
        await expect(
          page.locator('[data-testid="execution-progress"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="transaction-hash"]')
        ).toBeVisible();

        // Wait for completion
        await expect(
          page.locator('[data-testid="arbitrage-completed"]')
        ).toBeVisible({ timeout: 30000 });
        await expect(
          page.locator('[data-testid="profit-realized"]')
        ).toBeVisible();
      });

      // Step 4: Portfolio Optimization
      await test.step('Optimize Yield Farming Strategy', async () => {
        await page.click('[data-testid="portfolio-optimizer"]');

        const startTime = Date.now();
        await expect(
          page.locator('[data-testid="optimization-interface"]')
        ).toBeVisible();

        const loadTime = Date.now() - startTime;
        expect(loadTime).toBeLessThan(3000);

        // Run optimization analysis
        await page.click('[data-testid="analyze-portfolio"]');
        await expect(
          page.locator('[data-testid="optimization-results"]')
        ).toBeVisible();

        // Verify optimization suggestions
        await expect(
          page.locator('[data-testid="rebalance-suggestions"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="risk-adjusted-returns"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="diversification-analysis"]')
        ).toBeVisible();

        // Apply optimization
        await page.click('[data-testid="apply-optimization"]');
        await expect(
          page.locator('[data-testid="optimization-applied"]')
        ).toBeVisible();
      });

      // Step 5: Advanced Risk Management
      await test.step('Configure Advanced Risk Management', async () => {
        await page.click('[data-testid="risk-management"]');
        await expect(page.locator('[data-testid="risk-config"]')).toBeVisible();

        // Set up stop-loss conditions
        await page.fill('[data-testid="stop-loss-percentage"]', '10');
        await page.check('[data-testid="enable-auto-rebalancing"]');
        await page.fill('[data-testid="max-position-size"]', '25'); // 25% max per position

        // Configure alerts
        await page.check('[data-testid="impermanent-loss-alerts"]');
        await page.fill('[data-testid="il-threshold"]', '5');

        await page.click('[data-testid="save-risk-settings"]');

        await expect(
          page.locator('[data-testid="risk-management-active"]')
        ).toBeVisible();

        // Test risk management with simulation
        await page.click('[data-testid="test-risk-scenario"]');
        await page.selectOption(
          '[data-testid="scenario-type"]',
          'market-crash'
        );
        await page.click('[data-testid="run-simulation"]');

        await expect(
          page.locator('[data-testid="simulation-results"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="protection-effectiveness"]')
        ).toBeVisible();
      });
    });

    test('should handle advanced feature errors gracefully', async ({
      page,
    }) => {
      await page.goto('/trading/advanced');

      // Test flash loan failure scenario
      await page.click('[data-testid="arbitrage-scanner"]');

      // Mock flash loan failure
      await page.route('**/api/flash-loan/execute', route => {
        route.fulfill({
          status: 400,
          body: JSON.stringify({
            error: 'Insufficient liquidity',
            message: 'Flash loan amount exceeds available liquidity',
          }),
        });
      });

      await page.click('[data-testid="opportunity-1"]');
      await page.click('[data-testid="execute-arbitrage"]');
      await page.fill('[data-testid="flash-loan-amount"]', '1000000');
      await page.click('[data-testid="execute-flash-loan"]');

      await expect(
        page.locator('[data-testid="flash-loan-error"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="error-explanation"]')
      ).toContainText('Insufficient liquidity');
      await expect(
        page.locator('[data-testid="alternative-suggestions"]')
      ).toBeVisible();
    });

    test('should validate advanced user permissions', async ({ page }) => {
      // Test access control for advanced features
      await page.evaluate(() => {
        localStorage.setItem('user-experience-level', 'beginner');
      });

      await page.goto('/trading/advanced');

      // Should redirect or show access denied
      await expect(page.locator('[data-testid="access-denied"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="upgrade-account-prompt"]')
      ).toBeVisible();
    });
  });
});
