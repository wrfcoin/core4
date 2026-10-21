/**
 * Integration Tests: Wallet ↔ NFT Marketplace
 * Tests interactions between wallet and NFT marketplace components
 */

const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../e2e/utils/test-helpers');
const { TestDatabase } = require('../utils/test-database');
const { MockServices } = require('../utils/mock-services');

test.describe('Wallet and NFT Marketplace Integration', () => {
  let helpers;
  let testDb;
  let mockServices;
  let walletAddress;
  let nftTokenId;

  test.beforeAll(async () => {
    // Setup test environment
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
    walletAddress = helpers.testData.users.user1.walletAddress;
  });

  test('Complete NFT purchase flow with wallet balance update', async ({
    page,
  }) => {
    // Step 1: Check initial wallet balance
    await helpers.navigateTo('/wallet');
    const initialBalance = await helpers.getWalletBalance();
    expect(initialBalance.WRF).toBeGreaterThan(100);

    // Step 2: Navigate to NFT marketplace
    await helpers.navigateTo('/nft-marketplace');
    await expect(page.locator('[data-testid="nft-grid"]')).toBeVisible();

    // Step 3: Select an NFT to purchase
    const nftCard = page.locator('[data-testid="nft-card"]').first();
    const nftPrice = await nftCard
      .locator('[data-testid="nft-price"]')
      .textContent();
    const priceValue = parseFloat(nftPrice.replace(/[^0-9.]/g, ''));

    await nftCard.click();
    await page.waitForURL('**/nft/*');

    // Step 4: Verify NFT details page
    await expect(page.locator('[data-testid="nft-title"]')).toBeVisible();
    await expect(page.locator('[data-testid="nft-owner"]')).toBeVisible();
    await expect(page.locator('[data-testid="buy-nft-button"]')).toBeVisible();

    // Step 5: Initiate purchase
    await page.click('[data-testid="buy-nft-button"]');

    // Purchase confirmation modal
    const modal = page.locator('[data-testid="purchase-confirmation-modal"]');
    await expect(modal).toBeVisible();
    await expect(modal.locator('[data-testid="purchase-price"]')).toContainText(
      nftPrice
    );

    // Verify wallet has sufficient balance
    const walletBalance = await modal
      .locator('[data-testid="wallet-balance"]')
      .textContent();
    expect(parseFloat(walletBalance.replace(/[^0-9.]/g, ''))).toBeGreaterThan(
      priceValue
    );

    // Step 6: Confirm purchase with wallet signature
    await page.click('[data-testid="confirm-purchase-button"]');

    // Wallet signature prompt
    await expect(
      page.locator('[data-testid="wallet-signature-prompt"]')
    ).toBeVisible();
    await page.click('[data-testid="sign-transaction-button"]');

    // Step 7: Wait for transaction confirmation
    const txNotification = page.locator(
      '[data-testid="transaction-notification"]'
    );
    await expect(txNotification).toBeVisible();
    const txHash = await txNotification
      .locator('[data-testid="tx-hash"]')
      .textContent();

    // Wait for blockchain confirmation
    await helpers.waitForTransaction(txHash);

    // Step 8: Verify NFT ownership transfer
    await page.waitForURL('**/my-nfts');
    await expect(
      page.locator(`[data-testid="owned-nft-${nftTokenId}"]`)
    ).toBeVisible();

    // Step 9: Verify wallet balance update
    await helpers.navigateTo('/wallet');
    const finalBalance = await helpers.getWalletBalance();

    // Balance should be reduced by NFT price + gas fees
    expect(finalBalance.WRF).toBeLessThan(initialBalance.WRF - priceValue);
    expect(finalBalance.WRF).toBeGreaterThan(
      initialBalance.WRF - priceValue - 1
    ); // Allow up to 1 WRF for gas

    // Step 10: Verify transaction history
    await page.click('[data-testid="transaction-history-tab"]');
    const latestTx = page.locator('[data-testid="transaction-item"]').first();
    await expect(latestTx).toContainText('NFT Purchase');
    await expect(latestTx).toContainText(nftPrice);
    await expect(latestTx.locator('[data-testid="tx-status"]')).toContainText(
      'Confirmed'
    );
  });

  test('Create and list NFT with wallet gas fee deduction', async ({
    page,
  }) => {
    // Step 1: Navigate to create NFT
    await helpers.navigateTo('/nft-marketplace/create');
    const initialBalance = await helpers.getWalletBalance();

    // Step 2: Fill NFT creation form
    const nftData = {
      title: 'Test Weather Data NFT',
      description: 'High-value weather data collection from station XYZ',
      category: 'weather-data',
      price: '50',
      royalty: '5',
    };

    await helpers.fillForm({
      'nft-title': nftData.title,
      'nft-description': nftData.description,
      'nft-price': nftData.price,
      'royalty-percentage': nftData.royalty,
    });

    await page.selectOption(
      '[data-testid="nft-category-select"]',
      nftData.category
    );

    // Upload file
    await helpers.uploadFile(
      '[data-testid="nft-file-upload"]',
      './fixtures/weather-data.json'
    );

    // Step 3: Preview and mint
    await page.click('[data-testid="preview-nft-button"]');

    // Verify preview
    await expect(
      page.locator('[data-testid="nft-preview-modal"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="preview-title"]')).toContainText(
      nftData.title
    );

    // Step 4: Mint NFT
    await page.click('[data-testid="mint-nft-button"]');

    // Wallet approval for minting
    await expect(
      page.locator('[data-testid="wallet-approval-modal"]')
    ).toBeVisible();
    const estimatedGas = await page
      .locator('[data-testid="estimated-gas-fee"]')
      .textContent();
    const gasValue = parseFloat(estimatedGas.replace(/[^0-9.]/g, ''));

    await page.click('[data-testid="approve-mint-button"]');

    // Step 5: Wait for minting transaction
    const mintTxHash = await page
      .locator('[data-testid="mint-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(mintTxHash);

    // Step 6: Verify NFT created and balance updated
    await page.waitForURL('**/my-nfts');
    const newNft = page
      .locator('[data-testid="owned-nft"]')
      .filter({ hasText: nftData.title });
    await expect(newNft).toBeVisible();

    nftTokenId = await newNft.getAttribute('data-token-id');

    // Check wallet balance decreased by gas fees
    const postMintBalance = await helpers.getWalletBalance();
    expect(postMintBalance.WRF).toBeLessThan(initialBalance.WRF);
    expect(postMintBalance.WRF).toBeGreaterThan(
      initialBalance.WRF - gasValue - 0.5
    ); // Allow margin

    // Step 7: List NFT on marketplace
    await newNft.click();
    await page.click('[data-testid="list-nft-button"]');

    // Set listing parameters
    await page.fill('[data-testid="listing-price-input"]', nftData.price);
    await page.click('[data-testid="confirm-listing-button"]');

    // Approve listing transaction
    await page.click('[data-testid="approve-listing-button"]');

    const listTxHash = await page
      .locator('[data-testid="listing-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(listTxHash);

    // Step 8: Verify NFT is listed on marketplace
    await helpers.navigateTo('/nft-marketplace');
    const listedNft = page
      .locator('[data-testid="nft-card"]')
      .filter({ hasText: nftData.title });
    await expect(listedNft).toBeVisible();
    await expect(listedNft.locator('[data-testid="nft-price"]')).toContainText(
      nftData.price
    );

    // Step 9: Verify final wallet balance
    const finalBalance = await helpers.getWalletBalance();
    expect(finalBalance.WRF).toBeLessThan(postMintBalance.WRF); // Additional gas for listing
  });

  test('NFT transfer between wallets with ownership verification', async ({
    page,
    context,
  }) => {
    // Step 1: Create NFT as user1
    await helpers.createNFT({
      title: 'Transfer Test NFT',
      price: '25',
    });

    const nftTokenId = await page
      .locator('[data-testid="owned-nft"]')
      .last()
      .getAttribute('data-token-id');

    // Step 2: Initiate transfer
    await page.click(`[data-testid="nft-actions-${nftTokenId}"]`);
    await page.click('[data-testid="transfer-nft-option"]');

    // Step 3: Enter recipient address (user2)
    const recipientAddress = helpers.testData.users.user2.walletAddress;
    await page.fill(
      '[data-testid="recipient-address-input"]',
      recipientAddress
    );

    // Verify recipient address
    await page.click('[data-testid="verify-address-button"]');
    await expect(
      page.locator('[data-testid="address-valid-indicator"]')
    ).toBeVisible();

    // Step 4: Confirm transfer
    await page.click('[data-testid="confirm-transfer-button"]');
    await page.click('[data-testid="sign-transfer-button"]');

    const transferTxHash = await page
      .locator('[data-testid="transfer-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(transferTxHash);

    // Step 5: Verify NFT removed from user1's collection
    await helpers.navigateTo('/my-nfts');
    await expect(
      page.locator(`[data-testid="owned-nft-${nftTokenId}"]`)
    ).not.toBeVisible();

    // Step 6: Login as user2 in new context
    const page2 = await context.newPage();
    const helpers2 = new TestHelpers(page2);
    await helpers2.loginAs('user2');

    // Step 7: Verify NFT in user2's collection
    await helpers2.navigateTo('/my-nfts');
    await expect(
      page2.locator(`[data-testid="owned-nft-${nftTokenId}"]`)
    ).toBeVisible();

    // Verify ownership details
    await page2.click(`[data-testid="owned-nft-${nftTokenId}"]`);
    await expect(
      page2.locator('[data-testid="nft-owner-address"]')
    ).toContainText(recipientAddress);

    // Step 8: Verify transfer history
    await page2.click('[data-testid="nft-history-tab"]');
    const transferEvent = page2
      .locator('[data-testid="history-event"]')
      .first();
    await expect(transferEvent).toContainText('Transfer');
    await expect(transferEvent).toContainText(
      helpers.testData.users.user1.walletAddress
    );
    await expect(transferEvent).toContainText(recipientAddress);

    await page2.close();
  });

  test('Batch NFT operations with wallet gas optimization', async ({
    page,
  }) => {
    // Step 1: Create multiple NFTs for batch operations
    const nftIds = [];
    for (let i = 0; i < 3; i++) {
      await helpers.createNFT({
        title: `Batch Test NFT ${i + 1}`,
        price: '10',
      });

      const nftId = await page
        .locator('[data-testid="owned-nft"]')
        .last()
        .getAttribute('data-token-id');
      nftIds.push(nftId);
    }

    // Step 2: Navigate to batch operations
    await helpers.navigateTo('/my-nfts');
    await page.click('[data-testid="batch-select-mode"]');

    // Select NFTs for batch listing
    for (const nftId of nftIds) {
      await page.click(`[data-testid="select-nft-${nftId}"]`);
    }

    await expect(page.locator('[data-testid="selected-count"]')).toContainText(
      '3'
    );

    // Step 3: Batch list NFTs
    await page.click('[data-testid="batch-list-button"]');

    // Set batch listing price
    await page.fill('[data-testid="batch-price-input"]', '15');

    // Check gas optimization
    const individualGas = await page
      .locator('[data-testid="individual-gas-estimate"]')
      .textContent();
    const batchGas = await page
      .locator('[data-testid="batch-gas-estimate"]')
      .textContent();

    const individualGasValue = parseFloat(
      individualGas.replace(/[^0-9.]/g, '')
    );
    const batchGasValue = parseFloat(batchGas.replace(/[^0-9.]/g, ''));

    // Batch should be cheaper than individual transactions
    expect(batchGasValue).toBeLessThan(individualGasValue * 0.7);

    // Step 4: Execute batch listing
    await page.click('[data-testid="confirm-batch-listing"]');
    await page.click('[data-testid="sign-batch-transaction"]');

    const batchTxHash = await page
      .locator('[data-testid="batch-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(batchTxHash);

    // Step 5: Verify all NFTs are listed
    await helpers.navigateTo('/nft-marketplace');

    for (let i = 0; i < 3; i++) {
      const listedNft = page
        .locator('[data-testid="nft-card"]')
        .filter({ hasText: `Batch Test NFT ${i + 1}` });
      await expect(listedNft).toBeVisible();
      await expect(
        listedNft.locator('[data-testid="nft-price"]')
      ).toContainText('15');
    }
  });

  test('NFT royalty payments integration with wallet', async ({
    page,
    context,
  }) => {
    // Step 1: Create NFT with royalties as creator (user1)
    await helpers.createNFT({
      title: 'Royalty Test NFT',
      price: '100',
      royalty: '10', // 10% royalty
    });

    const nftTokenId = await page
      .locator('[data-testid="owned-nft"]')
      .last()
      .getAttribute('data-token-id');
    const creatorAddress = helpers.testData.users.user1.walletAddress;

    // Step 2: List NFT on marketplace
    await helpers.listNFT(nftTokenId, '100');

    // Step 3: Buy NFT as user2
    const page2 = await context.newPage();
    const helpers2 = new TestHelpers(page2);
    await helpers2.loginAs('user2');

    await helpers2.navigateTo('/nft-marketplace');
    await page2.click(`[data-testid="nft-card-${nftTokenId}"]`);
    await page2.click('[data-testid="buy-nft-button"]');
    await page2.click('[data-testid="confirm-purchase-button"]');
    await page2.click('[data-testid="sign-transaction-button"]');

    const purchaseTxHash = await page2
      .locator('[data-testid="tx-hash"]')
      .textContent();
    await helpers2.waitForTransaction(purchaseTxHash);

    // Step 4: User2 lists NFT for resale
    await helpers2.navigateTo('/my-nfts');
    await helpers2.listNFT(nftTokenId, '150');

    // Step 5: Buy NFT as user3 (secondary sale)
    const page3 = await context.newPage();
    const helpers3 = new TestHelpers(page3);
    await helpers3.loginAs('user3');

    await helpers3.navigateTo('/nft-marketplace');
    await page3.click(`[data-testid="nft-card-${nftTokenId}"]`);

    // Verify royalty information is displayed
    await expect(
      page3.locator('[data-testid="creator-royalty"]')
    ).toContainText('10%');
    await expect(page3.locator('[data-testid="royalty-amount"]')).toContainText(
      '15 WRF'
    ); // 10% of 150

    await page3.click('[data-testid="buy-nft-button"]');
    await page3.click('[data-testid="confirm-purchase-button"]');
    await page3.click('[data-testid="sign-transaction-button"]');

    const resaleTxHash = await page3
      .locator('[data-testid="tx-hash"]')
      .textContent();
    await helpers3.waitForTransaction(resaleTxHash);

    // Step 6: Verify royalty payment to creator (user1)
    await helpers.navigateTo('/wallet');
    await page.click('[data-testid="transaction-history-tab"]');

    // Find royalty payment transaction
    const royaltyTx = page
      .locator('[data-testid="transaction-item"]')
      .filter({ hasText: 'Royalty Payment' });
    await expect(royaltyTx).toBeVisible();
    await expect(royaltyTx).toContainText('15 WRF');
    await expect(royaltyTx.locator('[data-testid="tx-type"]')).toContainText(
      'Incoming'
    );

    // Step 7: Verify creator earnings dashboard
    await helpers.navigateTo('/creator-dashboard');
    await expect(
      page.locator('[data-testid="total-royalties-earned"]')
    ).toContainText('15 WRF');
    await expect(
      page.locator('[data-testid="royalty-transactions-count"]')
    ).toContainText('1');

    await page2.close();
    await page3.close();
  });
});
