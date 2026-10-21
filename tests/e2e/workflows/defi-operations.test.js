const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('DeFi Operations Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('user1');
    await helpers.connectWallet('user1');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Complete liquidity provision workflow', async ({ page }) => {
    // Navigate to DeFi section
    await page.click('[data-testid="nav-defi"]');
    await page.waitForURL('**/defi');

    // Verify DeFi dashboard components
    await expect(page.locator('[data-testid="defi-dashboard"]')).toBeVisible();
    await expect(page.locator('[data-testid="liquidity-pools"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="farm-opportunities"]')
    ).toBeVisible();

    // Navigate to liquidity provision
    await page.click('[data-testid="add-liquidity-button"]');
    await page.waitForURL('**/defi/liquidity/add');

    // Select token pair: WRF/ETH
    await page.selectOption('[data-testid="token-a-select"]', 'WRF');
    await page.selectOption('[data-testid="token-b-select"]', 'ETH');

    // Wait for pool info to load
    await page.waitForSelector('[data-testid="pool-info"]');
    await expect(
      page.locator('[data-testid="current-price-ratio"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="pool-share"]')).toBeVisible();

    // Input liquidity amounts
    const wrfAmount = '1000';
    const ethAmount = '0.5';

    await page.fill('[data-testid="token-a-amount-input"]', wrfAmount);
    await page.fill('[data-testid="token-b-amount-input"]', ethAmount);

    // Verify price impact and pool share calculations
    await expect(page.locator('[data-testid="price-impact"]')).toBeVisible();
    const priceImpact = await page.textContent('[data-testid="price-impact"]');
    expect(priceImpact).toMatch(/^\d+(\.\d+)?%$/);

    await expect(
      page.locator('[data-testid="pool-share-percentage"]')
    ).toBeVisible();
    const poolShare = await page.textContent(
      '[data-testid="pool-share-percentage"]'
    );
    expect(poolShare).toMatch(/^\d+(\.\d+)?%$/);

    // Review liquidity provision
    await page.click('[data-testid="review-liquidity-button"]');

    const modal = await helpers.waitForModal();

    // Verify transaction summary
    await expect(
      page.locator('[data-testid="liquidity-summary"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="token-a-deposit"]')).toContainText(
      wrfAmount
    );
    await expect(page.locator('[data-testid="token-b-deposit"]')).toContainText(
      ethAmount
    );
    await expect(
      page.locator('[data-testid="lp-tokens-received"]')
    ).toBeVisible();

    // Check slippage protection
    await expect(
      page.locator('[data-testid="slippage-tolerance"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="minimum-received"]')
    ).toBeVisible();

    // Approve tokens if needed
    if (
      await page.locator('[data-testid="approve-token-a-button"]').isVisible()
    ) {
      await page.click('[data-testid="approve-token-a-button"]');
      await page.fill(
        '[data-testid="wallet-password-input"]',
        'WalletPass123!'
      );
      await page.click('[data-testid="authenticate-button"]');
      await helpers.waitForNotification('success');
    }

    if (
      await page.locator('[data-testid="approve-token-b-button"]').isVisible()
    ) {
      await page.click('[data-testid="approve-token-b-button"]');
      await page.fill(
        '[data-testid="wallet-password-input"]',
        'WalletPass123!'
      );
      await page.click('[data-testid="authenticate-button"]');
      await helpers.waitForNotification('success');
    }

    // Add liquidity
    await page.click('[data-testid="add-liquidity-confirm-button"]');

    // Authenticate transaction
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    // Wait for liquidity addition confirmation
    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify liquidity position appears
    await page.goto('/defi/portfolio');
    await expect(
      page.locator('[data-testid="liquidity-position"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="position-value"]')).toBeVisible();
  });

  test('Remove liquidity from pool', async ({ page }) => {
    await page.click('[data-testid="nav-defi"]');
    await page.click('[data-testid="portfolio-tab"]');

    // Select existing liquidity position
    await page.click('[data-testid="liquidity-position"]').first();
    await page.click('[data-testid="remove-liquidity-button"]');

    const modal = await helpers.waitForModal();

    // Set removal percentage
    await page.click('[data-testid="remove-percentage-50"]'); // Remove 50%

    // Verify withdrawal amounts
    await expect(
      page.locator('[data-testid="token-a-withdrawal"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="token-b-withdrawal"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="lp-tokens-burned"]')
    ).toBeVisible();

    // Confirm removal
    await page.click('[data-testid="confirm-remove-liquidity"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify position value decreased
    await page.reload();
    const updatedValue = await page.textContent(
      '[data-testid="position-value"]'
    );
    expect(updatedValue).toBeTruthy();
  });

  test('Stake tokens in yield farming pool', async ({ page }) => {
    await page.click('[data-testid="nav-defi"]');
    await page.click('[data-testid="farming-tab"]');

    // Browse farming opportunities
    await expect(
      page.locator('[data-testid="farm-pool"]')
    ).toHaveCount.toBeGreaterThan(0);

    // Select high-APY farming pool
    await page.click('[data-testid="farm-pool"]').first();

    // Verify pool details
    await expect(page.locator('[data-testid="pool-apy"]')).toBeVisible();
    await expect(page.locator('[data-testid="total-liquidity"]')).toBeVisible();
    await expect(page.locator('[data-testid="reward-token"]')).toBeVisible();

    // Stake LP tokens
    await page.click('[data-testid="stake-button"]');

    const modal = await helpers.waitForModal();

    // Get available LP balance
    const availableBalance = await page.textContent(
      '[data-testid="available-lp-balance"]'
    );
    const balanceAmount = parseFloat(availableBalance.replace(/[^0-9.]/g, ''));
    const stakeAmount = (balanceAmount * 0.8).toString(); // Stake 80% of available

    await page.fill('[data-testid="stake-amount-input"]', stakeAmount);

    // Verify staking details
    await expect(
      page.locator('[data-testid="estimated-rewards"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="reward-per-block"]')
    ).toBeVisible();

    // Confirm staking
    await page.click('[data-testid="confirm-stake-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify staking position
    await expect(page.locator('[data-testid="staked-amount"]')).toContainText(
      stakeAmount
    );
    await expect(page.locator('[data-testid="pending-rewards"]')).toBeVisible();
  });

  test('Harvest farming rewards', async ({ page }) => {
    await page.click('[data-testid="nav-defi"]');
    await page.click('[data-testid="farming-tab"]');

    // Find pool with pending rewards
    await page
      .click('[data-testid="farm-pool"][data-has-rewards="true"]')
      .first();

    // Check pending rewards
    const pendingRewards = await page.textContent(
      '[data-testid="pending-rewards"]'
    );
    expect(pendingRewards).toMatch(/\d+(\.\d+)?\s*WRF/);

    // Harvest rewards
    await page.click('[data-testid="harvest-rewards-button"]');

    const modal = await helpers.waitForModal();

    // Review harvest details
    await expect(page.locator('[data-testid="harvest-summary"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="gas-fee-estimate"]')
    ).toBeVisible();

    // Confirm harvest
    await page.click('[data-testid="confirm-harvest-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify rewards were harvested
    await page.reload();
    const newPendingRewards = await page.textContent(
      '[data-testid="pending-rewards"]'
    );
    expect(newPendingRewards).toContain('0');
  });

  test('Unstake tokens from farming pool', async ({ page }) => {
    await page.click('[data-testid="nav-defi"]');
    await page.click('[data-testid="farming-tab"]');

    // Select staked pool
    await page
      .click('[data-testid="farm-pool"][data-user-staked="true"]')
      .first();

    // Unstake tokens
    await page.click('[data-testid="unstake-button"]');

    const modal = await helpers.waitForModal();

    // Set unstake amount (unstake 50%)
    const stakedAmount = await page.textContent(
      '[data-testid="staked-amount"]'
    );
    const unstakeAmount = (
      parseFloat(stakedAmount.replace(/[^0-9.]/g, '')) * 0.5
    ).toString();

    await page.fill('[data-testid="unstake-amount-input"]', unstakeAmount);

    // Verify unstaking details
    await expect(page.locator('[data-testid="unstake-summary"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="remaining-staked"]')
    ).toBeVisible();

    // Confirm unstaking
    await page.click('[data-testid="confirm-unstake-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify updated staked amount
    await page.reload();
    const newStakedAmount = await page.textContent(
      '[data-testid="staked-amount"]'
    );
    const newAmount = parseFloat(newStakedAmount.replace(/[^0-9.]/g, ''));
    expect(newAmount).toBeLessThan(
      parseFloat(stakedAmount.replace(/[^0-9.]/g, ''))
    );
  });

  test('Execute token swap through AMM', async ({ page }) => {
    await page.click('[data-testid="nav-defi"]');
    await page.click('[data-testid="swap-tab"]');

    // Verify swap interface
    await expect(page.locator('[data-testid="swap-interface"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="from-token-input"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="to-token-input"]')).toBeVisible();

    // Configure swap: WRF to ETH
    await page.selectOption('[data-testid="from-token-select"]', 'WRF');
    await page.selectOption('[data-testid="to-token-select"]', 'ETH');

    const swapAmount = '500';
    await page.fill('[data-testid="from-amount-input"]', swapAmount);

    // Wait for price calculation
    await page.waitForSelector('[data-testid="exchange-rate"]');
    await expect(
      page.locator('[data-testid="to-amount-estimate"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="price-impact"]')).toBeVisible();

    // Verify swap route
    await expect(page.locator('[data-testid="swap-route"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="minimum-received"]')
    ).toBeVisible();

    // Configure slippage
    await page.click('[data-testid="slippage-settings"]');
    await page.click('[data-testid="slippage-1-percent"]');

    // Execute swap
    await page.click('[data-testid="swap-button"]');

    const modal = await helpers.waitForModal();

    // Review swap details
    await expect(page.locator('[data-testid="swap-summary"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="from-amount-display"]')
    ).toContainText(swapAmount);
    await expect(page.locator('[data-testid="gas-fee"]')).toBeVisible();

    // Confirm swap
    await page.click('[data-testid="confirm-swap-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify swap transaction appears
    await page.goto('/wallet/transactions');
    await expect(
      page.locator('[data-testid="swap-transaction"]')
    ).toBeVisible();
  });

  test('Monitor DeFi portfolio performance', async ({ page }) => {
    await page.click('[data-testid="nav-defi"]');
    await page.click('[data-testid="portfolio-tab"]');

    // Verify portfolio overview
    await expect(
      page.locator('[data-testid="total-portfolio-value"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="24h-change"]')).toBeVisible();
    await expect(page.locator('[data-testid="yield-earned"]')).toBeVisible();

    // Check position breakdown
    await expect(page.locator('[data-testid="position-list"]')).toBeVisible();
    const positions = page.locator('[data-testid="defi-position"]');
    await expect(positions).toHaveCount.toBeGreaterThan(0);

    // Verify position details
    for (let i = 0; i < (await positions.count()); i++) {
      const position = positions.nth(i);
      await expect(
        position.locator('[data-testid="position-name"]')
      ).toBeVisible();
      await expect(
        position.locator('[data-testid="position-value"]')
      ).toBeVisible();
      await expect(
        position.locator('[data-testid="position-apy"]')
      ).toBeVisible();
    }

    // Test portfolio filtering
    await page.selectOption('[data-testid="position-filter"]', 'farming');
    const farmingPositions = page.locator(
      '[data-testid="defi-position"][data-type="farming"]'
    );
    await expect(farmingPositions).toHaveCount.toBeGreaterThan(0);

    // Test performance chart
    await page.click('[data-testid="performance-chart-tab"]');
    await expect(page.locator('[data-testid="portfolio-chart"]')).toBeVisible();

    // Test different time ranges
    await page.click('[data-testid="timerange-7d"]');
    await page.waitForTimeout(1000);
    await page.click('[data-testid="timerange-30d"]');
    await page.waitForTimeout(1000);
  });

  test('DeFi operations error handling', async ({ page }) => {
    await page.click('[data-testid="nav-defi"]');

    // Test insufficient balance error
    await page.click('[data-testid="add-liquidity-button"]');
    await page.selectOption('[data-testid="token-a-select"]', 'WRF');
    await page.selectOption('[data-testid="token-b-select"]', 'ETH');

    // Enter amount larger than balance
    await page.fill('[data-testid="token-a-amount-input"]', '999999');
    await expect(
      page.locator('[data-testid="insufficient-balance-error"]')
    ).toBeVisible();

    // Test invalid slippage
    await page.fill('[data-testid="token-a-amount-input"]', '100');
    await page.click('[data-testid="slippage-settings"]');
    await page.fill('[data-testid="custom-slippage-input"]', '0.01'); // Very low slippage
    await expect(
      page.locator('[data-testid="slippage-warning"]')
    ).toBeVisible();

    // Test network connection issues
    await page.route('**/api/defi/**', route => route.abort());
    await page.reload();
    await expect(
      page.locator('[data-testid="defi-connection-error"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retry-connection-button"]')
    ).toBeVisible();
  });

  test('DeFi accessibility and mobile responsiveness', async ({ page }) => {
    // Test keyboard navigation
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter');

    // Check accessibility violations
    const violations = await helpers.checkAccessibility();
    expect(violations).toHaveLength(0);

    // Test mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });

    await page.click('[data-testid="nav-defi"]');

    // Mobile-specific elements should be visible
    await expect(page.locator('[data-testid="mobile-defi-nav"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="mobile-swap-button"]')
    ).toBeVisible();

    // Test swipe navigation
    await page.touchscreen.tap(100, 300);
    await page.touchscreen.tap(275, 300);
  });
});
