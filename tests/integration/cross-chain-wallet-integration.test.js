/**
 * Integration Tests: Cross-Chain Bridge ↔ Wallet
 * Tests interactions between cross-chain bridge and wallet components
 */

const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../e2e/utils/test-helpers');
const { TestDatabase } = require('../utils/test-database');
const { MockServices } = require('../utils/mock-services');

test.describe('Cross-Chain Bridge and Wallet Integration', () => {
  let helpers;
  let testDb;
  let mockServices;

  test.beforeAll(async () => {
    testDb = new TestDatabase();
    await testDb.setup();

    mockServices = new MockServices();
    await mockServices.startAll();
  });

  test.afterAll(async () => {
    await mockServices.stopAll();
    await testDb.teardown();
  });

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('user1');
  });

  test('Bridge WRF tokens to Ethereum with wallet balance updates', async ({
    page,
  }) => {
    // Step 1: Check initial wallet balances
    await helpers.navigateTo('/wallet');
    const initialBalances = await helpers.getWalletBalance();
    expect(initialBalances.WRF).toBeGreaterThan(100);

    // Step 2: Navigate to cross-chain bridge
    await helpers.navigateTo('/bridge');

    // Select source and destination chains
    await page.selectOption('[data-testid="source-chain-select"]', 'wrfcoin');
    await page.selectOption(
      '[data-testid="destination-chain-select"]',
      'ethereum'
    );

    // Step 3: Connect destination wallet (Ethereum)
    await page.click('[data-testid="connect-destination-wallet"]');

    // Select MetaMask option
    await page.click('[data-testid="metamask-option"]');

    // Mock MetaMask connection
    await helpers.mockWalletConnection(
      'ethereum',
      helpers.testData.users.user1.ethereumAddress
    );

    await expect(
      page.locator('[data-testid="ethereum-wallet-connected"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="destination-address"]')
    ).toContainText(helpers.testData.users.user1.ethereumAddress);

    // Step 4: Set bridge amount and review
    const bridgeAmount = '50';
    await page.fill('[data-testid="bridge-amount-input"]', bridgeAmount);

    // Check bridge fees
    const bridgeFee = await page
      .locator('[data-testid="bridge-fee"]')
      .textContent();
    const feeValue = parseFloat(bridgeFee.replace(/[^0-9.]/g, ''));

    // Verify sufficient balance
    await expect(
      page.locator('[data-testid="sufficient-balance-check"]')
    ).toBeVisible();

    // Step 5: Initiate bridge transaction
    await page.click('[data-testid="review-bridge-button"]');

    // Review bridge details
    await expect(
      page.locator('[data-testid="bridge-review-modal"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="review-amount"]')).toContainText(
      bridgeAmount
    );
    await expect(page.locator('[data-testid="review-fee"]')).toContainText(
      bridgeFee
    );
    await expect(
      page.locator('[data-testid="review-destination"]')
    ).toContainText(helpers.testData.users.user1.ethereumAddress);

    // Step 6: Confirm and sign bridge transaction
    await page.click('[data-testid="confirm-bridge-button"]');

    // Sign WRF chain transaction (lock tokens)
    await page.click('[data-testid="sign-lock-transaction"]');

    const lockTxHash = await page
      .locator('[data-testid="lock-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(lockTxHash);

    // Step 7: Wait for bridge processing
    await expect(
      page.locator('[data-testid="bridge-processing"]')
    ).toBeVisible();

    // Mock bridge confirmation (in real scenario this would be from bridge validators)
    await helpers.mockBridgeConfirmation(lockTxHash, 'ethereum');

    // Wait for Ethereum transaction
    await expect(
      page.locator('[data-testid="ethereum-tx-hash"]')
    ).toBeVisible();
    const ethTxHash = await page
      .locator('[data-testid="ethereum-tx-hash"]')
      .textContent();

    // Step 8: Verify bridge completion
    await expect(page.locator('[data-testid="bridge-complete"]')).toBeVisible();
    await expect(page.locator('[data-testid="bridge-status"]')).toContainText(
      'Completed'
    );

    // Step 9: Verify wallet balance updates
    await helpers.navigateTo('/wallet');
    const finalBalances = await helpers.getWalletBalance();

    // WRF balance should be reduced by bridge amount + fees
    expect(finalBalances.WRF).toBe(
      initialBalances.WRF - parseFloat(bridgeAmount) - feeValue
    );

    // Should show bridged tokens in multi-chain view
    await page.click('[data-testid="multi-chain-view"]');
    await expect(
      page.locator('[data-testid="ethereum-wrf-balance"]')
    ).toContainText(bridgeAmount);

    // Step 10: Verify bridge transaction history
    await page.click('[data-testid="transaction-history-tab"]');
    const bridgeTx = page
      .locator('[data-testid="transaction-item"]')
      .filter({ hasText: 'Bridge Out' });
    await expect(bridgeTx).toBeVisible();
    await expect(bridgeTx).toContainText(bridgeAmount);
    await expect(bridgeTx).toContainText('To: Ethereum');
    await expect(bridgeTx.locator('[data-testid="tx-status"]')).toContainText(
      'Confirmed'
    );
  });

  test('Bridge tokens from Ethereum back to WRF with atomic swap', async ({
    page,
  }) => {
    // Step 1: Setup - ensure user has WRF tokens on Ethereum
    await helpers.setupCrossChainBalance('ethereum', 'WRF', '75');

    // Step 2: Navigate to bridge with reverse direction
    await helpers.navigateTo('/bridge');

    await page.selectOption('[data-testid="source-chain-select"]', 'ethereum');
    await page.selectOption(
      '[data-testid="destination-chain-select"]',
      'wrfcoin'
    );

    // Connect Ethereum wallet
    await page.click('[data-testid="connect-source-wallet"]');
    await helpers.mockWalletConnection(
      'ethereum',
      helpers.testData.users.user1.ethereumAddress
    );

    // Step 3: Check Ethereum WRF balance
    await expect(page.locator('[data-testid="source-balance"]')).toContainText(
      '75'
    );

    // Step 4: Set bridge amount
    const bridgeAmount = '30';
    await page.fill('[data-testid="bridge-amount-input"]', bridgeAmount);

    // Verify atomic swap option is selected
    await expect(
      page.locator('[data-testid="atomic-swap-enabled"]')
    ).toBeChecked();

    // Check swap time estimate
    const swapTime = await page
      .locator('[data-testid="swap-time-estimate"]')
      .textContent();
    expect(swapTime).toContain('minutes');

    // Step 5: Initiate atomic swap
    await page.click('[data-testid="initiate-atomic-swap"]');

    // Atomic swap details
    await expect(
      page.locator('[data-testid="atomic-swap-modal"]')
    ).toBeVisible();
    const swapHash = await page
      .locator('[data-testid="swap-hash"]')
      .textContent();
    const timelock = await page
      .locator('[data-testid="swap-timelock"]')
      .textContent();

    // Step 6: Confirm atomic swap setup
    await page.click('[data-testid="confirm-atomic-swap"]');

    // Sign Ethereum transaction (create HTLC)
    await page.click('[data-testid="sign-ethereum-htlc"]');

    const ethHTLCTxHash = await page
      .locator('[data-testid="eth-htlc-tx-hash"]')
      .textContent();
    await helpers.waitForEthereumTransaction(ethHTLCTxHash);

    // Step 7: Reveal secret and claim on WRF chain
    await expect(page.locator('[data-testid="htlc-created"]')).toBeVisible();

    // Automatically reveal secret (in real scenario user would do this manually)
    await page.click('[data-testid="reveal-secret-button"]');

    const secret = await page
      .locator('[data-testid="revealed-secret"]')
      .textContent();
    expect(secret).toBeTruthy();

    // Sign WRF chain claim transaction
    await page.click('[data-testid="claim-on-wrf-chain"]');
    await page.click('[data-testid="sign-claim-transaction"]');

    const wrfClaimTxHash = await page
      .locator('[data-testid="wrf-claim-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(wrfClaimTxHash);

    // Step 8: Verify atomic swap completion
    await expect(
      page.locator('[data-testid="atomic-swap-complete"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="swap-status"]')).toContainText(
      'Completed'
    );

    // Step 9: Verify balances updated
    await helpers.navigateTo('/wallet');

    // Check WRF chain balance increased
    const wrfBalance = await helpers.getWalletBalance();
    expect(wrfBalance.WRF).toBeGreaterThan(parseFloat(bridgeAmount) - 1); // Allow for fees

    // Check Ethereum balance decreased
    await page.click('[data-testid="multi-chain-view"]');
    const ethBalance = await page
      .locator('[data-testid="ethereum-wrf-balance"]')
      .textContent();
    expect(parseFloat(ethBalance)).toBe(75 - parseFloat(bridgeAmount));
  });

  test('Multi-asset bridge operation with portfolio balancing', async ({
    page,
  }) => {
    // Step 1: Setup multi-asset balances
    await helpers.setupCrossChainBalance('polygon', 'USDC', '1000');
    await helpers.setupCrossChainBalance('bsc', 'BNB', '5');

    // Step 2: Navigate to portfolio bridge
    await helpers.navigateTo('/bridge/portfolio');

    // View current portfolio distribution
    await expect(
      page.locator('[data-testid="portfolio-overview"]')
    ).toBeVisible();

    const portfolioValue = await page
      .locator('[data-testid="total-portfolio-value"]')
      .textContent();
    expect(parseFloat(portfolioValue.replace(/[^0-9.]/g, ''))).toBeGreaterThan(
      0
    );

    // Step 3: Set up portfolio rebalancing
    await page.click('[data-testid="rebalance-portfolio-button"]');

    // Target allocation
    const targetAllocation = {
      WRF: '40',
      USDC: '30',
      BNB: '20',
      ETH: '10',
    };

    for (const [asset, percentage] of Object.entries(targetAllocation)) {
      await page.fill(
        `[data-testid="target-${asset.toLowerCase()}-input"]`,
        percentage
      );
    }

    // Step 4: Calculate required bridges
    await page.click('[data-testid="calculate-rebalancing"]');

    await expect(
      page.locator('[data-testid="rebalancing-plan"]')
    ).toBeVisible();

    // Review bridge operations needed
    const bridgeOps = await page
      .locator('[data-testid="bridge-operation"]')
      .count();
    expect(bridgeOps).toBeGreaterThan(0);

    // Step 5: Execute multi-asset rebalancing
    await page.click('[data-testid="execute-rebalancing"]');

    // Confirm batch bridge operations
    await expect(
      page.locator('[data-testid="batch-bridge-modal"]')
    ).toBeVisible();
    await page.click('[data-testid="confirm-batch-bridges"]');

    // Step 6: Sign multiple transactions sequentially
    const bridgeCount = await page
      .locator('[data-testid="pending-bridge"]')
      .count();

    for (let i = 0; i < bridgeCount; i++) {
      await page.click(`[data-testid="sign-bridge-${i}"]`);

      const txHash = await page
        .locator(`[data-testid="bridge-tx-${i}"]`)
        .textContent();
      await helpers.waitForTransaction(txHash);

      await expect(
        page.locator(`[data-testid="bridge-${i}-complete"]`)
      ).toBeVisible();
    }

    // Step 7: Verify portfolio rebalancing
    await expect(
      page.locator('[data-testid="rebalancing-complete"]')
    ).toBeVisible();

    // Check final allocation matches targets
    await helpers.navigateTo('/wallet/portfolio');

    for (const [asset, targetPercent] of Object.entries(targetAllocation)) {
      const actualPercent = await page
        .locator(`[data-testid="${asset.toLowerCase()}-allocation"]`)
        .textContent();
      const actualValue = parseFloat(actualPercent.replace(/[^0-9.]/g, ''));

      // Allow 2% variance for fees and slippage
      expect(actualValue).toBeGreaterThan(parseFloat(targetPercent) - 2);
      expect(actualValue).toBeLessThan(parseFloat(targetPercent) + 2);
    }
  });

  test('Bridge failure recovery with wallet protection', async ({ page }) => {
    // Step 1: Setup bridge operation that will fail
    await helpers.navigateTo('/bridge');

    await page.selectOption('[data-testid="source-chain-select"]', 'wrfcoin');
    await page.selectOption(
      '[data-testid="destination-chain-select"]',
      'ethereum'
    );

    const bridgeAmount = '25';
    await page.fill('[data-testid="bridge-amount-input"]', bridgeAmount);

    // Step 2: Initiate bridge that will fail at destination
    await page.click('[data-testid="review-bridge-button"]');
    await page.click('[data-testid="confirm-bridge-button"]');
    await page.click('[data-testid="sign-lock-transaction"]');

    const lockTxHash = await page
      .locator('[data-testid="lock-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(lockTxHash);

    // Step 3: Simulate bridge failure at destination
    await helpers.mockBridgeFailure(lockTxHash, 'destination_chain_error');

    // Wait for failure detection
    await expect(page.locator('[data-testid="bridge-failed"]')).toBeVisible();
    await expect(page.locator('[data-testid="failure-reason"]')).toContainText(
      'destination_chain_error'
    );

    // Step 4: Initiate recovery process
    await page.click('[data-testid="recover-bridge-button"]');

    // Recovery options modal
    await expect(
      page.locator('[data-testid="recovery-options-modal"]')
    ).toBeVisible();

    // Choose to unlock tokens on source chain
    await page.click('[data-testid="unlock-tokens-option"]');

    // Step 5: Sign recovery transaction
    await page.click('[data-testid="confirm-recovery"]');
    await page.click('[data-testid="sign-unlock-transaction"]');

    const unlockTxHash = await page
      .locator('[data-testid="unlock-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(unlockTxHash);

    // Step 6: Verify tokens returned to wallet
    await expect(
      page.locator('[data-testid="recovery-complete"]')
    ).toBeVisible();

    await helpers.navigateTo('/wallet');
    const balance = await helpers.getWalletBalance();

    // Tokens should be restored (minus only the recovery fee)
    const recoveryFee = 0.1; // Estimated recovery fee
    expect(balance.WRF).toBeGreaterThan(parseFloat(bridgeAmount) - recoveryFee);

    // Step 7: Verify bridge failure in transaction history
    await page.click('[data-testid="transaction-history-tab"]');

    const failedBridge = page
      .locator('[data-testid="transaction-item"]')
      .filter({ hasText: 'Bridge Failed' });
    await expect(failedBridge).toBeVisible();

    const recoveryTx = page
      .locator('[data-testid="transaction-item"]')
      .filter({ hasText: 'Bridge Recovery' });
    await expect(recoveryTx).toBeVisible();
    await expect(recoveryTx.locator('[data-testid="tx-status"]')).toContainText(
      'Confirmed'
    );
  });
});
