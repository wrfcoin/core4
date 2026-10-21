const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('Wallet Operations Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('user1');
    await helpers.connectWallet('user1');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Complete wallet overview and balance display', async ({ page }) => {
    // Navigate to wallet page
    await page.click('[data-testid="nav-wallet"]');
    await page.waitForURL('**/wallet');

    // Verify wallet overview components
    await expect(page.locator('[data-testid="wallet-address"]')).toBeVisible();
    await expect(page.locator('[data-testid="total-balance"]')).toBeVisible();
    await expect(page.locator('[data-testid="token-list"]')).toBeVisible();

    // Check individual token balances
    await expect(page.locator('[data-testid="wrf-balance"]')).toBeVisible();
    await expect(page.locator('[data-testid="eth-balance"]')).toBeVisible();

    // Verify balance amounts are displayed correctly
    const wrfBalance = await page.textContent('[data-testid="wrf-balance"]');
    expect(wrfBalance).toMatch(/^\d+(\.\d+)?\s*WRF$/);

    // Check recent transactions section
    await expect(
      page.locator('[data-testid="recent-transactions"]')
    ).toBeVisible();

    // Verify wallet actions are available
    await expect(page.locator('[data-testid="send-button"]')).toBeVisible();
    await expect(page.locator('[data-testid="receive-button"]')).toBeVisible();
    await expect(page.locator('[data-testid="swap-button"]')).toBeVisible();
  });

  test('Send tokens to another address', async ({ page }) => {
    await page.click('[data-testid="nav-wallet"]');
    await page.click('[data-testid="send-button"]');

    // Send modal should appear
    const modal = await helpers.waitForModal();

    // Fill send form
    const recipientAddress = helpers.testData.users.user2.walletAddress;
    const sendAmount = '10.5';

    await page.fill(
      '[data-testid="recipient-address-input"]',
      recipientAddress
    );
    await page.fill('[data-testid="send-amount-input"]', sendAmount);

    // Select token (WRF should be default)
    await page.selectOption('[data-testid="token-select"]', 'WRF');

    // Verify gas fee estimation
    await expect(
      page.locator('[data-testid="gas-fee-estimate"]')
    ).toBeVisible();
    const gasFee = await page.textContent('[data-testid="gas-fee-estimate"]');
    expect(gasFee).toMatch(/^\d+(\.\d+)?\s*ETH$/);

    // Review transaction details
    await page.click('[data-testid="review-transaction-button"]');

    await expect(
      page.locator('[data-testid="transaction-summary"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="recipient-display"]')
    ).toContainText(recipientAddress);
    await expect(page.locator('[data-testid="amount-display"]')).toContainText(
      sendAmount
    );

    // Confirm transaction
    await page.click('[data-testid="confirm-send-button"]');

    // Enter wallet password
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    // Wait for transaction to be submitted
    await helpers.waitForNotification('success');

    // Verify transaction appears in pending state
    await modal.waitForClose();
    await page.reload(); // Refresh to see updated balance and transactions

    await expect(
      page.locator('[data-testid="pending-transaction"]')
    ).toBeVisible();

    // Wait for transaction confirmation (mock in test environment)
    await page.waitForTimeout(5000);
    await page.reload();

    // Verify transaction is confirmed
    await expect(
      page.locator('[data-testid="confirmed-transaction"]')
    ).toBeVisible();
  });

  test('Receive tokens and display QR code', async ({ page }) => {
    await page.click('[data-testid="nav-wallet"]');
    await page.click('[data-testid="receive-button"]');

    // Receive modal should appear
    const modal = await helpers.waitForModal();

    // Should display wallet address
    const walletAddress = helpers.testData.users.user1.walletAddress;
    await expect(page.locator('[data-testid="receive-address"]')).toContainText(
      walletAddress
    );

    // Should display QR code
    await expect(page.locator('[data-testid="address-qr-code"]')).toBeVisible();

    // Test copy address functionality
    await page.click('[data-testid="copy-address-button"]');
    await helpers.waitForNotification('success');

    // Test share functionality
    await page.click('[data-testid="share-address-button"]');
    await expect(page.locator('[data-testid="share-options"]')).toBeVisible();

    await modal.close();
  });

  test('Token swap functionality', async ({ page }) => {
    await page.click('[data-testid="nav-wallet"]');
    await page.click('[data-testid="swap-button"]');

    // Swap modal should appear
    const modal = await helpers.waitForModal();

    // Configure swap: WRF to ETH
    await page.selectOption('[data-testid="from-token-select"]', 'WRF');
    await page.selectOption('[data-testid="to-token-select"]', 'ETH');

    const swapAmount = '100';
    await page.fill('[data-testid="from-amount-input"]', swapAmount);

    // Wait for price calculation
    await page.waitForSelector('[data-testid="swap-rate"]');
    const swapRate = await page.textContent('[data-testid="swap-rate"]');
    expect(swapRate).toMatch(/1 WRF = \d+(\.\d+)? ETH/);

    // Verify estimated output
    await expect(
      page.locator('[data-testid="to-amount-estimate"]')
    ).toBeVisible();

    // Check slippage settings
    await page.click('[data-testid="slippage-settings-button"]');
    await expect(
      page.locator('[data-testid="slippage-tolerance"]')
    ).toBeVisible();
    await page.click('[data-testid="slippage-1-percent"]'); // Set 1% slippage

    // Review swap
    await page.click('[data-testid="review-swap-button"]');

    await expect(page.locator('[data-testid="swap-summary"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="minimum-received"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="price-impact"]')).toBeVisible();

    // Confirm swap
    await page.click('[data-testid="confirm-swap-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    // Wait for swap confirmation
    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify swap appears in transaction history
    await page.reload();
    await expect(
      page.locator('[data-testid="swap-transaction"]')
    ).toBeVisible();
  });

  test('Transaction history and filtering', async ({ page }) => {
    await page.click('[data-testid="nav-wallet"]');
    await page.click('[data-testid="view-all-transactions"]');

    // Should navigate to transactions page
    await page.waitForURL('**/wallet/transactions');

    // Verify transaction list
    await expect(
      page.locator('[data-testid="transaction-list"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="transaction-item"]')
    ).toHaveCount.toBeGreaterThan(0);

    // Test filtering by transaction type
    await page.click('[data-testid="filter-dropdown"]');
    await page.click('[data-testid="filter-sends"]');

    // Should filter to only send transactions
    const sendTransactions = page.locator(
      '[data-testid="transaction-item"][data-type="send"]'
    );
    await expect(sendTransactions).toHaveCount.toBeGreaterThan(0);

    // Test date range filtering
    await page.click('[data-testid="date-filter-button"]');
    await page.fill('[data-testid="start-date"]', '2024-07-01');
    await page.fill('[data-testid="end-date"]', '2024-07-14');
    await page.click('[data-testid="apply-date-filter"]');

    // Test search functionality
    await page.fill(
      '[data-testid="transaction-search"]',
      helpers.testData.users.user2.walletAddress
    );
    await page.press('[data-testid="transaction-search"]', 'Enter');

    // Should filter transactions involving the searched address
    await expect(
      page.locator('[data-testid="transaction-item"]')
    ).toHaveCount.toBeGreaterThan(0);

    // Test transaction details view
    await page.click('[data-testid="transaction-item"]').first();

    const detailModal = await helpers.waitForModal();
    await expect(
      page.locator('[data-testid="transaction-hash"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="transaction-status"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="block-number"]')).toBeVisible();
    await expect(page.locator('[data-testid="gas-used"]')).toBeVisible();

    await detailModal.close();
  });

  test('Wallet security features', async ({ page }) => {
    await page.click('[data-testid="nav-wallet"]');
    await page.click('[data-testid="wallet-settings-button"]');

    // Security settings modal
    const modal = await helpers.waitForModal();

    // Test change wallet password
    await page.click('[data-testid="change-password-tab"]');

    await page.fill('[data-testid="current-password"]', 'WalletPass123!');
    await page.fill('[data-testid="new-password"]', 'NewWalletPass123!');
    await page.fill(
      '[data-testid="confirm-new-password"]',
      'NewWalletPass123!'
    );

    await page.click('[data-testid="update-password-button"]');
    await helpers.waitForNotification('success');

    // Test backup seed phrase
    await page.click('[data-testid="backup-seed-tab"]');

    await page.fill(
      '[data-testid="wallet-password-verify"]',
      'NewWalletPass123!'
    );
    await page.click('[data-testid="reveal-seed-phrase"]');

    await expect(
      page.locator('[data-testid="seed-phrase-display"]')
    ).toBeVisible();
    const seedPhrase = await page.textContent(
      '[data-testid="seed-phrase-display"]'
    );
    expect(seedPhrase.split(' ')).toHaveLength(12);

    // Test export private key
    await page.click('[data-testid="export-private-key-tab"]');

    await page.fill(
      '[data-testid="wallet-password-verify"]',
      'NewWalletPass123!'
    );
    await page.click('[data-testid="export-private-key-button"]');

    await expect(
      page.locator('[data-testid="private-key-warning"]')
    ).toBeVisible();
    await page.check('[data-testid="understand-risks-checkbox"]');
    await page.click('[data-testid="confirm-export-button"]');

    await expect(
      page.locator('[data-testid="private-key-display"]')
    ).toBeVisible();

    await modal.close();
  });

  test('Multi-token portfolio management', async ({ page }) => {
    await page.click('[data-testid="nav-wallet"]');

    // Add custom token
    await page.click('[data-testid="add-token-button"]');

    const modal = await helpers.waitForModal();

    // Test adding token by contract address
    const customTokenAddress = '0x1234567890123456789012345678901234567890';
    await page.fill('[data-testid="token-address-input"]', customTokenAddress);
    await page.click('[data-testid="add-custom-token-button"]');

    // Should auto-populate token details (mocked)
    await expect(page.locator('[data-testid="token-symbol"]')).toHaveValue(
      'CUSTOM'
    );
    await expect(page.locator('[data-testid="token-decimals"]')).toHaveValue(
      '18'
    );

    await page.click('[data-testid="confirm-add-token"]');
    await helpers.waitForNotification('success');
    await modal.close();

    // Verify new token appears in token list
    await expect(
      page.locator('[data-testid="custom-token-balance"]')
    ).toBeVisible();

    // Test hiding/showing tokens
    await page.click('[data-testid="manage-tokens-button"]');

    await page.uncheck('[data-testid="token-visibility-custom"]');
    await page.click('[data-testid="save-token-preferences"]');

    // Custom token should be hidden
    await expect(
      page.locator('[data-testid="custom-token-balance"]')
    ).toBeHidden();

    // Test portfolio value calculation
    await expect(
      page.locator('[data-testid="total-portfolio-value"]')
    ).toBeVisible();
    const portfolioValue = await page.textContent(
      '[data-testid="total-portfolio-value"]'
    );
    expect(portfolioValue).toMatch(/\$\d+(\.\d+)?/);
  });

  test('Wallet error handling and edge cases', async ({ page }) => {
    await page.click('[data-testid="nav-wallet"]');

    // Test send with insufficient balance
    await page.click('[data-testid="send-button"]');

    await page.fill(
      '[data-testid="recipient-address-input"]',
      helpers.testData.users.user2.walletAddress
    );
    await page.fill('[data-testid="send-amount-input"]', '999999'); // Very large amount

    await page.click('[data-testid="review-transaction-button"]');
    await expect(
      page.locator('[data-testid="insufficient-balance-error"]')
    ).toBeVisible();

    // Test send with invalid address
    await page.fill(
      '[data-testid="recipient-address-input"]',
      'invalid-address'
    );
    await expect(
      page.locator('[data-testid="invalid-address-error"]')
    ).toBeVisible();

    // Test network connection issues
    await page.route('**/api/wallet/**', route => route.abort());

    await page.reload();
    await expect(
      page.locator('[data-testid="network-error-message"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retry-connection-button"]')
    ).toBeVisible();
  });

  test('Wallet accessibility and mobile responsiveness', async ({ page }) => {
    // Test keyboard navigation
    await page.keyboard.press('Tab'); // Should focus on first focusable element
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter'); // Should activate focused element

    // Check accessibility violations
    const violations = await helpers.checkAccessibility();
    expect(violations).toHaveLength(0);

    // Test mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });

    await page.click('[data-testid="nav-wallet"]');

    // Mobile-specific elements should be visible
    await expect(
      page.locator('[data-testid="mobile-wallet-nav"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="mobile-send-button"]')
    ).toBeVisible();

    // Test swipe gestures (if supported)
    await page.touchscreen.tap(100, 300);
    await page.touchscreen.tap(275, 300); // Swipe gesture simulation
  });
});
