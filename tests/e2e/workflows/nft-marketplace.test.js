const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('NFT Marketplace Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('user1');
    await helpers.connectWallet('user1');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Browse and filter NFT marketplace', async ({ page }) => {
    // Navigate to NFT marketplace
    await page.click('[data-testid="nav-nft"]');
    await page.waitForURL('**/nft/marketplace');

    // Verify marketplace components
    await expect(page.locator('[data-testid="nft-grid"]')).toBeVisible();
    await expect(page.locator('[data-testid="filter-sidebar"]')).toBeVisible();
    await expect(page.locator('[data-testid="search-bar"]')).toBeVisible();

    // Test category filtering
    await page.click('[data-testid="filter-computational"]');

    // Should filter to computational NFTs only
    const nftItems = page.locator('[data-testid="nft-item"]');
    await expect(nftItems).toHaveCount.toBeGreaterThan(0);

    for (let i = 0; i < (await nftItems.count()); i++) {
      const category = await nftItems.nth(i).getAttribute('data-category');
      expect(category).toBe('computational');
    }

    // Test price range filtering
    await page.fill('[data-testid="min-price-input"]', '10');
    await page.fill('[data-testid="max-price-input"]', '100');
    await page.click('[data-testid="apply-price-filter"]');

    // Verify filtered results
    await page.waitForTimeout(1000); // Wait for filter to apply
    const filteredItems = page.locator('[data-testid="nft-item"]');
    await expect(filteredItems).toHaveCount.toBeGreaterThan(0);

    // Test sorting options
    await page.selectOption(
      '[data-testid="sort-dropdown"]',
      'price-low-to-high'
    );
    await page.waitForTimeout(1000);

    // Verify sorting (check first few items are in ascending price order)
    const firstPrice = await page.textContent(
      '[data-testid="nft-item"]:first-child [data-testid="nft-price"]'
    );
    const secondPrice = await page.textContent(
      '[data-testid="nft-item"]:nth-child(2) [data-testid="nft-price"]'
    );

    const firstPriceNum = parseFloat(firstPrice.replace(/[^\d.]/g, ''));
    const secondPriceNum = parseFloat(secondPrice.replace(/[^\d.]/g, ''));
    expect(firstPriceNum).toBeLessThanOrEqual(secondPriceNum);
  });

  test('View NFT details and metadata', async ({ page }) => {
    await page.click('[data-testid="nav-nft"]');

    // Click on first NFT to view details
    await page.click('[data-testid="nft-item"]').first();
    await page.waitForURL('**/nft/details/**');

    // Verify NFT details page components
    await expect(page.locator('[data-testid="nft-image"]')).toBeVisible();
    await expect(page.locator('[data-testid="nft-title"]')).toBeVisible();
    await expect(page.locator('[data-testid="nft-description"]')).toBeVisible();
    await expect(page.locator('[data-testid="nft-price"]')).toBeVisible();
    await expect(page.locator('[data-testid="nft-owner"]')).toBeVisible();

    // Check metadata attributes
    await expect(page.locator('[data-testid="nft-attributes"]')).toBeVisible();
    const attributes = page.locator('[data-testid="attribute-item"]');
    await expect(attributes).toHaveCount.toBeGreaterThan(0);

    // Verify computational NFT specific details
    if (
      await page.locator('[data-testid="computation-requirements"]').isVisible()
    ) {
      await expect(page.locator('[data-testid="cpu-cores"]')).toBeVisible();
      await expect(page.locator('[data-testid="memory-gb"]')).toBeVisible();
      await expect(page.locator('[data-testid="storage-gb"]')).toBeVisible();
      await expect(
        page.locator('[data-testid="estimated-duration"]')
      ).toBeVisible();
    }

    // Check transaction history
    await page.click('[data-testid="transaction-history-tab"]');
    await expect(
      page.locator('[data-testid="transaction-list"]')
    ).toBeVisible();

    // Check activity feed
    await page.click('[data-testid="activity-tab"]');
    await expect(page.locator('[data-testid="activity-feed"]')).toBeVisible();
  });

  test('Purchase NFT from marketplace', async ({ page }) => {
    await page.click('[data-testid="nav-nft"]');

    // Find and click on an NFT for sale
    await page
      .click('[data-testid="nft-item"][data-status="for-sale"]')
      .first();

    // Verify purchase button is available
    await expect(page.locator('[data-testid="buy-now-button"]')).toBeVisible();

    const nftPrice = await page.textContent('[data-testid="nft-price"]');
    const priceAmount = parseFloat(nftPrice.replace(/[^\d.]/g, ''));

    await page.click('[data-testid="buy-now-button"]');

    // Purchase confirmation modal
    const modal = await helpers.waitForModal();

    await expect(
      page.locator('[data-testid="purchase-summary"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="total-cost"]')).toContainText(
      priceAmount.toString()
    );

    // Check gas fee estimation
    await expect(page.locator('[data-testid="gas-fee"]')).toBeVisible();

    // Confirm purchase
    await page.click('[data-testid="confirm-purchase-button"]');

    // Authenticate transaction
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    // Wait for transaction to be submitted
    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify purchase appears in transaction history
    await page.reload();
    await expect(page.locator('[data-testid="owner-badge"]')).toContainText(
      'You'
    );
    await expect(page.locator('[data-testid="buy-now-button"]')).toBeHidden();
  });

  test('Create and list computational NFT', async ({ page }) => {
    await page.click('[data-testid="nav-nft"]');
    await page.click('[data-testid="create-nft-button"]');

    await page.waitForURL('**/nft/create');

    // Select computational NFT type
    await page.click('[data-testid="nft-type-computational"]');

    // Fill basic NFT information
    await helpers.fillForm({
      'nft-name': 'Weather Modeling Job #001',
      'nft-description':
        'High-resolution weather modeling computation for North America',
      'computation-type': 'WEATHER_MODELING',
    });

    // Upload NFT image/thumbnail
    await helpers.uploadFile(
      '[data-testid="nft-image-upload"]',
      './fixtures/mock-nft-image.jpg'
    );

    // Set computational requirements
    await helpers.fillForm({
      'cpu-cores': '8',
      'memory-gb': '32',
      'storage-gb': '500',
      'gpu-units': '2',
      'estimated-duration': '7200',
    });

    // Set payment details
    await helpers.fillForm({
      'payment-amount': '100',
      'payment-token': 'WRF',
    });

    // Add metadata attributes
    await page.click('[data-testid="add-attribute-button"]');
    await helpers.fillForm({
      'attribute-name-0': 'Resolution',
      'attribute-value-0': 'High',
    });

    await page.click('[data-testid="add-attribute-button"]');
    await helpers.fillForm({
      'attribute-name-1': 'Region',
      'attribute-value-1': 'North America',
    });

    // Preview NFT before creation
    await page.click('[data-testid="preview-nft-button"]');

    const previewModal = await helpers.waitForModal();
    await expect(page.locator('[data-testid="preview-image"]')).toBeVisible();
    await expect(page.locator('[data-testid="preview-name"]')).toContainText(
      'Weather Modeling Job #001'
    );
    await previewModal.close();

    // Create NFT
    await page.click('[data-testid="create-nft-button"]');

    // Authenticate NFT creation transaction
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    // Wait for NFT creation
    await helpers.waitForNotification('success');

    // Should redirect to NFT details page
    await page.waitForURL('**/nft/details/**');
    await expect(page.locator('[data-testid="nft-title"]')).toContainText(
      'Weather Modeling Job #001'
    );
    await expect(page.locator('[data-testid="owner-badge"]')).toContainText(
      'You'
    );
  });

  test('Make offer on NFT', async ({ page }) => {
    await page.click('[data-testid="nav-nft"]');

    // Find NFT not owned by current user
    await page.click('[data-testid="nft-item"][data-owner="other"]').first();

    // Make offer
    await page.click('[data-testid="make-offer-button"]');

    const modal = await helpers.waitForModal();

    // Fill offer details
    await page.fill('[data-testid="offer-amount-input"]', '50');
    await page.selectOption('[data-testid="offer-token-select"]', 'WRF');

    // Set offer expiration
    await page.fill('[data-testid="offer-expiration"]', '2024-08-01');

    // Review offer
    await expect(page.locator('[data-testid="offer-summary"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="offer-amount-display"]')
    ).toContainText('50 WRF');

    // Submit offer
    await page.click('[data-testid="submit-offer-button"]');

    // Authenticate offer transaction
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify offer appears in offers list
    await page.click('[data-testid="offers-tab"]');
    await expect(page.locator('[data-testid="your-offer"]')).toBeVisible();
    await expect(page.locator('[data-testid="offer-amount"]')).toContainText(
      '50 WRF'
    );
  });

  test('Accept offer as NFT owner', async ({ page }) => {
    // Switch to user who owns NFTs
    await helpers.loginAs('provider');
    await helpers.connectWallet('provider');

    await page.click('[data-testid="nav-nft"]');
    await page.click('[data-testid="my-nfts-tab"]');

    // Click on NFT with pending offers
    await page
      .click('[data-testid="nft-item"][data-has-offers="true"]')
      .first();

    // Go to offers tab
    await page.click('[data-testid="offers-tab"]');

    // Review pending offers
    await expect(
      page.locator('[data-testid="pending-offer"]')
    ).toHaveCount.toBeGreaterThan(0);

    // Accept first offer
    await page.click('[data-testid="accept-offer-button"]').first();

    const modal = await helpers.waitForModal();

    // Review sale details
    await expect(page.locator('[data-testid="sale-summary"]')).toBeVisible();
    await expect(page.locator('[data-testid="sale-amount"]')).toBeVisible();
    await expect(page.locator('[data-testid="platform-fee"]')).toBeVisible();
    await expect(page.locator('[data-testid="net-proceeds"]')).toBeVisible();

    // Confirm sale
    await page.click('[data-testid="confirm-sale-button"]');

    // Authenticate transaction
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify NFT ownership has transferred
    await page.reload();
    await expect(page.locator('[data-testid="owner-badge"]')).not.toContainText(
      'You'
    );
  });

  test('List NFT for auction', async ({ page }) => {
    await page.click('[data-testid="nav-nft"]');
    await page.click('[data-testid="my-nfts-tab"]');

    // Select owned NFT
    await page.click('[data-testid="nft-item"]').first();

    // List for auction
    await page.click('[data-testid="list-for-auction-button"]');

    const modal = await helpers.waitForModal();

    // Set auction parameters
    await page.fill('[data-testid="starting-bid-input"]', '25');
    await page.fill('[data-testid="reserve-price-input"]', '75');
    await page.selectOption('[data-testid="auction-token-select"]', 'WRF');

    // Set auction duration
    await page.selectOption('[data-testid="auction-duration-select"]', '7'); // 7 days

    // Preview auction listing
    await expect(page.locator('[data-testid="auction-summary"]')).toBeVisible();

    // Create auction
    await page.click('[data-testid="create-auction-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify auction is live
    await page.reload();
    await expect(page.locator('[data-testid="auction-badge"]')).toBeVisible();
    await expect(page.locator('[data-testid="current-bid"]')).toContainText(
      '25 WRF'
    );
    await expect(page.locator('[data-testid="auction-timer"]')).toBeVisible();
  });

  test('Place bid on auction', async ({ page }) => {
    // Switch to different user for bidding
    await helpers.loginAs('user2');
    await helpers.connectWallet('user2');

    await page.click('[data-testid="nav-nft"]');

    // Find auction NFT
    await page.click('[data-testid="filter-auction"]');
    await page.click('[data-testid="nft-item"][data-status="auction"]').first();

    // Place bid
    await page.click('[data-testid="place-bid-button"]');

    const modal = await helpers.waitForModal();

    // Get current bid amount
    const currentBidText = await page.textContent(
      '[data-testid="current-bid"]'
    );
    const currentBid = parseFloat(currentBidText.replace(/[^\d.]/g, ''));
    const newBid = (currentBid + 5).toString();

    await page.fill('[data-testid="bid-amount-input"]', newBid);

    // Verify bid is above minimum increment
    await expect(page.locator('[data-testid="bid-validation"]')).toContainText(
      'Valid bid'
    );

    // Submit bid
    await page.click('[data-testid="submit-bid-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify bid appears in bid history
    await page.click('[data-testid="bid-history-tab"]');
    await expect(page.locator('[data-testid="your-bid"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="current-high-bid"]')
    ).toContainText(newBid);
  });

  test('NFT marketplace search functionality', async ({ page }) => {
    await page.click('[data-testid="nav-nft"]');

    // Test basic search
    await page.fill('[data-testid="search-bar"]', 'weather');
    await page.press('[data-testid="search-bar"]', 'Enter');

    // Should filter results containing "weather"
    const searchResults = page.locator('[data-testid="nft-item"]');
    await expect(searchResults).toHaveCount.toBeGreaterThan(0);

    // Test advanced search filters
    await page.click('[data-testid="advanced-search-button"]');

    const searchModal = await helpers.waitForModal();

    // Filter by collection
    await page.selectOption(
      '[data-testid="collection-filter"]',
      'computational-jobs'
    );

    // Filter by price range
    await page.fill('[data-testid="min-price"]', '10');
    await page.fill('[data-testid="max-price"]', '500');

    // Filter by traits
    await page.click('[data-testid="trait-filter-computation-type"]');
    await page.check('[data-testid="trait-weather-modeling"]');

    // Apply filters
    await page.click('[data-testid="apply-search-filters"]');
    await searchModal.waitForClose();

    // Verify filtered results
    await expect(
      page.locator('[data-testid="search-results-count"]')
    ).toBeVisible();
    const filteredResults = page.locator('[data-testid="nft-item"]');

    // All results should match the filters
    for (let i = 0; i < (await filteredResults.count()); i++) {
      const nftItem = filteredResults.nth(i);
      const category = await nftItem.getAttribute('data-category');
      expect(category).toBe('computational');
    }
  });

  test('NFT marketplace performance and accessibility', async ({ page }) => {
    await page.click('[data-testid="nav-nft"]');

    // Measure marketplace load performance
    const performance = await helpers.measurePerformance();
    expect(performance.load).toBeLessThan(4000);
    expect(performance.firstContentfulPaint).toBeLessThan(2000);

    // Test lazy loading of NFT images
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.waitForTimeout(2000);

    // Additional NFTs should load
    const nftItems = page.locator('[data-testid="nft-item"]');
    await expect(nftItems).toHaveCount.toBeGreaterThan(20);

    // Check accessibility
    const violations = await helpers.checkAccessibility();
    expect(violations).toHaveLength(0);

    // Test keyboard navigation
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter'); // Should interact with focused element

    // Test mobile responsiveness
    await page.setViewportSize({ width: 375, height: 667 });

    await expect(
      page.locator('[data-testid="mobile-filter-button"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="mobile-nft-grid"]')).toBeVisible();
  });
});
