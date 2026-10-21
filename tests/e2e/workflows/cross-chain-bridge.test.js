const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('Cross-Chain Bridge Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('user1');
    await helpers.connectWallet('user1');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Bridge tokens from Ethereum to Arbitrum', async ({ page }) => {
    // Navigate to cross-chain bridge
    await page.click('[data-testid="nav-bridge"]');
    await page.waitForURL('**/bridge');

    // Verify bridge interface
    await expect(
      page.locator('[data-testid="bridge-interface"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="source-network-select"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="target-network-select"]')
    ).toBeVisible();

    // Configure bridge transfer
    await page.selectOption(
      '[data-testid="source-network-select"]',
      'ethereum'
    );
    await page.selectOption(
      '[data-testid="target-network-select"]',
      'arbitrum'
    );

    // Wait for network configuration to load
    await page.waitForSelector('[data-testid="bridge-status"]');
    await expect(page.locator('[data-testid="bridge-status"]')).toContainText(
      'Ready'
    );

    // Select token to bridge
    await page.selectOption('[data-testid="token-select"]', 'WRF');

    // Check available balance
    const availableBalance = await page.textContent(
      '[data-testid="available-balance"]'
    );
    expect(availableBalance).toMatch(/\d+(\.\d+)?\s*WRF/);

    // Set bridge amount
    const bridgeAmount = '100';
    await page.fill('[data-testid="bridge-amount-input"]', bridgeAmount);

    // Verify bridge details
    await expect(page.locator('[data-testid="bridge-fee"]')).toBeVisible();
    await expect(page.locator('[data-testid="estimated-time"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="minimum-received"]')
    ).toBeVisible();

    const bridgeFee = await page.textContent('[data-testid="bridge-fee"]');
    expect(bridgeFee).toMatch(/\d+(\.\d+)?\s*(ETH|WRF)/);

    // Set destination address (optional - defaults to same address)
    await page.fill(
      '[data-testid="destination-address"]',
      helpers.testData.users.user1.walletAddress
    );

    // Review bridge transaction
    await page.click('[data-testid="review-bridge-button"]');

    const modal = await helpers.waitForModal();

    // Verify bridge summary
    await expect(page.locator('[data-testid="bridge-summary"]')).toBeVisible();
    await expect(page.locator('[data-testid="source-details"]')).toContainText(
      'Ethereum'
    );
    await expect(page.locator('[data-testid="target-details"]')).toContainText(
      'Arbitrum'
    );
    await expect(
      page.locator('[data-testid="amount-to-bridge"]')
    ).toContainText(bridgeAmount);

    // Check security warnings and confirmations
    await expect(page.locator('[data-testid="bridge-warnings"]')).toBeVisible();
    await page.check('[data-testid="acknowledge-risks"]');
    await page.check('[data-testid="confirm-destination"]');

    // Initiate bridge transfer
    await page.click('[data-testid="initiate-bridge-button"]');

    // Authenticate source transaction
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Monitor bridge progress
    await page.waitForURL('**/bridge/transfer/**');
    await expect(page.locator('[data-testid="bridge-status"]')).toContainText(
      'In Progress'
    );
    await expect(page.locator('[data-testid="source-tx-hash"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="progress-indicator"]')
    ).toBeVisible();
  });

  test('Monitor bridge transfer status and completion', async ({ page }) => {
    await page.click('[data-testid="nav-bridge"]');
    await page.click('[data-testid="transfer-history-tab"]');

    // Find active bridge transfer
    await page
      .click('[data-testid="bridge-transfer"][data-status="in-progress"]')
      .first();

    // Verify transfer monitoring interface
    await expect(
      page.locator('[data-testid="transfer-timeline"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="network-confirmations"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="validator-signatures"]')
    ).toBeVisible();

    // Check progress steps
    await expect(
      page.locator('[data-testid="step-source-confirmed"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="step-validators-signed"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="step-target-pending"]')
    ).toBeVisible();

    // Monitor confirmation progress
    const confirmationCount = await page.textContent(
      '[data-testid="confirmation-count"]'
    );
    expect(confirmationCount).toMatch(/\d+\/\d+/);

    // Wait for bridge completion (in test environment, simulate)
    if (await page.locator('[data-testid="claim-tokens-button"]').isVisible()) {
      // Claim tokens on target network
      await page.click('[data-testid="claim-tokens-button"]');

      const claimModal = await helpers.waitForModal();

      // Review claim details
      await expect(page.locator('[data-testid="claim-summary"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="target-network-fee"]')
      ).toBeVisible();

      // Execute claim transaction
      await page.click('[data-testid="execute-claim-button"]');

      // Authenticate claim transaction
      await page.fill(
        '[data-testid="wallet-password-input"]',
        'WalletPass123!'
      );
      await page.click('[data-testid="authenticate-button"]');

      await helpers.waitForNotification('success');
      await claimModal.close();

      // Verify completion
      await page.reload();
      await expect(
        page.locator('[data-testid="transfer-status"]')
      ).toContainText('Completed');
      await expect(
        page.locator('[data-testid="target-tx-hash"]')
      ).toBeVisible();
    }
  });

  test('Bridge NFTs across networks', async ({ page }) => {
    await page.click('[data-testid="nav-bridge"]');
    await page.click('[data-testid="nft-bridge-tab"]');

    // Verify NFT bridge interface
    await expect(
      page.locator('[data-testid="nft-bridge-interface"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="owned-nfts"]')).toBeVisible();

    // Configure NFT bridge
    await page.selectOption(
      '[data-testid="source-network-select"]',
      'ethereum'
    );
    await page.selectOption('[data-testid="target-network-select"]', 'polygon');

    // Select NFT to bridge
    await page.click('[data-testid="nft-item"]').first();

    // Verify NFT details
    await expect(
      page.locator('[data-testid="selected-nft-details"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="nft-metadata"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="nft-contract-address"]')
    ).toBeVisible();

    // Check bridge compatibility
    await page.click('[data-testid="check-nft-compatibility"]');
    await expect(
      page.locator('[data-testid="compatibility-status"]')
    ).toContainText('Compatible');

    // Set destination address
    await page.fill(
      '[data-testid="nft-destination-address"]',
      helpers.testData.users.user1.walletAddress
    );

    // Review NFT bridge
    await page.click('[data-testid="review-nft-bridge"]');

    const modal = await helpers.waitForModal();

    // Verify NFT bridge summary
    await expect(
      page.locator('[data-testid="nft-bridge-summary"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="nft-bridge-fee"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="wrapped-contract-info"]')
    ).toBeVisible();

    // Acknowledge NFT bridge risks
    await page.check('[data-testid="acknowledge-nft-risks"]');
    await page.check('[data-testid="understand-wrapping"]');

    // Initiate NFT bridge
    await page.click('[data-testid="initiate-nft-bridge"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Monitor NFT bridge progress
    await expect(
      page.locator('[data-testid="nft-bridge-status"]')
    ).toContainText('In Progress');
    await expect(page.locator('[data-testid="nft-lock-tx"]')).toBeVisible();
  });

  test('Add custom bridge route', async ({ page }) => {
    // Switch to bridge operator
    await helpers.loginAs('bridge-operator');
    await helpers.connectWallet('bridge-operator');

    await page.click('[data-testid="nav-bridge"]');
    await page.click('[data-testid="bridge-admin-tab"]');

    // Add new bridge route
    await page.click('[data-testid="add-bridge-route"]');

    const modal = await helpers.waitForModal();

    // Configure new route
    await page.selectOption('[data-testid="new-source-network"]', 'avalanche');
    await page.selectOption('[data-testid="new-target-network"]', 'fantom');

    // Set route parameters
    await helpers.fillForm({
      'route-name': 'Avalanche-Fantom Bridge',
      'base-fee': '0.005',
      'fee-percentage': '0.1',
      'min-transfer': '10',
      'max-transfer': '1000000',
      'confirmation-blocks': '12',
    });

    // Configure validator requirements
    await page.fill('[data-testid="min-validator-signatures"]', '5');
    await page.fill('[data-testid="validator-threshold"]', '7');

    // Add validator addresses
    for (let i = 0; i < 3; i++) {
      await page.click('[data-testid="add-validator"]');
      await page.fill(
        `[data-testid="validator-address-${i}"]`,
        `0x${'1'.repeat(40)}`
      );
    }

    // Set security parameters
    await page.fill('[data-testid="daily-limit"]', '500000');
    await page.fill('[data-testid="emergency-delay"]', '3600'); // 1 hour
    await page.check('[data-testid="enable-rate-limiting"]');

    // Submit route configuration
    await page.click('[data-testid="create-bridge-route"]');

    // Authenticate route creation
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'OperatorPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify route appears in list
    await expect(page.locator('[data-testid="bridge-route"]')).toContainText(
      'Avalanche-Fantom'
    );
    await expect(page.locator('[data-testid="route-status"]')).toContainText(
      'Pending Activation'
    );
  });

  test('Bridge liquidity management', async ({ page }) => {
    await page.click('[data-testid="nav-bridge"]');
    await page.click('[data-testid="liquidity-tab"]');

    // Verify liquidity interface
    await expect(page.locator('[data-testid="liquidity-pools"]')).toBeVisible();
    await expect(page.locator('[data-testid="pool-statistics"]')).toBeVisible();

    // Add liquidity to bridge pool
    await page.click('[data-testid="add-liquidity-button"]');

    const modal = await helpers.waitForModal();

    // Select network and token
    await page.selectOption('[data-testid="liquidity-network"]', 'arbitrum');
    await page.selectOption('[data-testid="liquidity-token"]', 'WRF');

    // Set liquidity amount
    const liquidityAmount = '50000';
    await page.fill('[data-testid="liquidity-amount"]', liquidityAmount);

    // Review liquidity provision
    await expect(
      page.locator('[data-testid="liquidity-rewards-apy"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="liquidity-share"]')).toBeVisible();
    await expect(page.locator('[data-testid="lock-period"]')).toBeVisible();

    // Confirm liquidity provision
    await page.click('[data-testid="provide-liquidity"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify liquidity position
    await expect(
      page.locator('[data-testid="my-liquidity-position"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="provided-amount"]')).toContainText(
      liquidityAmount
    );
    await expect(page.locator('[data-testid="earned-rewards"]')).toBeVisible();
  });

  test('Bridge security and fraud detection', async ({ page }) => {
    await page.click('[data-testid="nav-bridge"]');
    await page.click('[data-testid="security-tab"]');

    // Verify security monitoring interface
    await expect(
      page.locator('[data-testid="security-dashboard"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="threat-alerts"]')).toBeVisible();
    await expect(page.locator('[data-testid="audit-log"]')).toBeVisible();

    // Test suspicious transaction detection
    if (
      await page.locator('[data-testid="suspicious-transaction"]').isVisible()
    ) {
      await page.click('[data-testid="suspicious-transaction"]').first();

      // Review security alert
      await expect(page.locator('[data-testid="alert-details"]')).toBeVisible();
      await expect(page.locator('[data-testid="risk-score"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="detection-reason"]')
      ).toBeVisible();

      // Take security action
      await page.click('[data-testid="flag-for-review"]');
      await page.fill(
        '[data-testid="security-notes"]',
        'Unusual transaction pattern detected - requires manual review'
      );
      await page.click('[data-testid="submit-security-flag"]');

      await helpers.waitForNotification('success');
    }

    // Monitor bridge health metrics
    await page.click('[data-testid="health-metrics-tab"]');
    await expect(
      page.locator('[data-testid="validator-uptime"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="failed-transactions"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="average-completion-time"]')
    ).toBeVisible();

    // Check emergency controls
    await page.click('[data-testid="emergency-controls-tab"]');
    await expect(
      page.locator('[data-testid="emergency-pause-button"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="rate-limit-controls"]')
    ).toBeVisible();
  });

  test('Multi-hop bridge routing', async ({ page }) => {
    await page.click('[data-testid="nav-bridge"]');

    // Enable advanced routing
    await page.click('[data-testid="advanced-options"]');
    await page.check('[data-testid="enable-multi-hop"]');

    // Configure multi-hop bridge: Ethereum -> Polygon -> Arbitrum
    await page.selectOption(
      '[data-testid="source-network-select"]',
      'ethereum'
    );
    await page.selectOption(
      '[data-testid="target-network-select"]',
      'arbitrum'
    );

    // Wait for route calculation
    await page.waitForSelector('[data-testid="routing-options"]');

    // Review available routes
    await expect(page.locator('[data-testid="direct-route"]')).toBeVisible();
    await expect(page.locator('[data-testid="multi-hop-route"]')).toBeVisible();

    // Select multi-hop route for better rates
    await page.click('[data-testid="select-multi-hop"]');

    // Review route details
    await expect(page.locator('[data-testid="hop-1"]')).toContainText(
      'Ethereum → Polygon'
    );
    await expect(page.locator('[data-testid="hop-2"]')).toContainText(
      'Polygon → Arbitrum'
    );
    await expect(page.locator('[data-testid="total-time"]')).toBeVisible();
    await expect(page.locator('[data-testid="total-fees"]')).toBeVisible();

    // Set bridge amount
    await page.fill('[data-testid="bridge-amount-input"]', '250');

    // Compare route efficiency
    await expect(page.locator('[data-testid="fee-comparison"]')).toBeVisible();
    await expect(page.locator('[data-testid="time-comparison"]')).toBeVisible();

    // Execute multi-hop bridge
    await page.click('[data-testid="execute-multi-hop"]');

    const modal = await helpers.waitForModal();

    // Acknowledge multi-hop complexity
    await page.check('[data-testid="understand-multi-hop"]');
    await page.check('[data-testid="acknowledge-multiple-txs"]');

    await page.click('[data-testid="confirm-multi-hop"]');

    // Authenticate first hop
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Monitor multi-hop progress
    await expect(page.locator('[data-testid="hop-progress"]')).toBeVisible();
    await expect(page.locator('[data-testid="current-hop"]')).toContainText(
      '1 of 2'
    );
  });

  test('Bridge error handling and recovery', async ({ page }) => {
    await page.click('[data-testid="nav-bridge"]');

    // Test insufficient balance error
    await page.selectOption(
      '[data-testid="source-network-select"]',
      'ethereum'
    );
    await page.selectOption(
      '[data-testid="target-network-select"]',
      'arbitrum'
    );
    await page.selectOption('[data-testid="token-select"]', 'WRF');

    // Enter amount larger than balance
    await page.fill('[data-testid="bridge-amount-input"]', '999999');
    await expect(
      page.locator('[data-testid="insufficient-balance-error"]')
    ).toBeVisible();

    // Test network congestion handling
    await page.fill('[data-testid="bridge-amount-input"]', '100');

    // Simulate network congestion
    if (await page.locator('[data-testid="congestion-warning"]').isVisible()) {
      await expect(
        page.locator('[data-testid="estimated-delay"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="alternative-routes"]')
      ).toBeVisible();
    }

    // Test failed bridge recovery
    await page.click('[data-testid="transfer-history-tab"]');

    if (await page.locator('[data-testid="failed-transfer"]').isVisible()) {
      await page.click('[data-testid="failed-transfer"]').first();

      // Review failure details
      await expect(
        page.locator('[data-testid="failure-reason"]')
      ).toBeVisible();
      await expect(
        page.locator('[data-testid="recovery-options"]')
      ).toBeVisible();

      // Attempt recovery
      await page.click('[data-testid="retry-transfer"]');

      const recoveryModal = await helpers.waitForModal();

      await expect(
        page.locator('[data-testid="recovery-summary"]')
      ).toBeVisible();
      await page.click('[data-testid="confirm-recovery"]');

      await page.fill(
        '[data-testid="wallet-password-input"]',
        'WalletPass123!'
      );
      await page.click('[data-testid="authenticate-button"]');

      await helpers.waitForNotification('success');
      await recoveryModal.close();
    }

    // Test network connection issues
    await page.route('**/api/bridge/**', route => route.abort());
    await page.reload();
    await expect(
      page.locator('[data-testid="bridge-connection-error"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retry-connection-button"]')
    ).toBeVisible();
  });

  test('Bridge accessibility and mobile responsiveness', async ({ page }) => {
    // Test keyboard navigation
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter');

    // Check accessibility violations
    const violations = await helpers.checkAccessibility();
    expect(violations).toHaveLength(0);

    // Test mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });

    await page.click('[data-testid="nav-bridge"]');

    // Mobile-specific elements should be visible
    await expect(
      page.locator('[data-testid="mobile-bridge-interface"]')
    ).toBeVisible();

    // Test mobile bridge form
    await page.selectOption(
      '[data-testid="source-network-select"]',
      'ethereum'
    );
    await page.selectOption(
      '[data-testid="target-network-select"]',
      'arbitrum'
    );

    // Test swipe gestures for network selection
    await page.touchscreen.tap(100, 300);
    await page.touchscreen.tap(275, 300);
  });
});
