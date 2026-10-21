/**
 * End-to-End User Journey: Weather Data to NFT Monetization
 * Tests complete flow from weather station setup to NFT creation and sales
 */

const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');
const { TestDatabase } = require('../../utils/test-database');
const { MockServices } = require('../../utils/mock-services');

test.describe('Weather Data to NFT Monetization Journey', () => {
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

  test('Complete weather station to NFT monetization workflow', async ({
    page,
  }) => {
    // === PHASE 1: WEATHER STATION SETUP ===

    // Step 1: Connect first weather station
    await helpers.navigateTo('/weather-stations');
    await page.click('[data-testid="add-weather-station-button"]');

    const stationData1 = {
      name: 'Urban Climate Station Alpha',
      type: 'ambient-weather',
      location: 'New York City',
      lat: '40.7128',
      lng: '-74.0060',
      apiKey: 'test-api-alpha-12345',
      appKey: 'test-app-alpha-67890',
    };

    await helpers.fillForm({
      'station-name': stationData1.name,
      'api-key': stationData1.apiKey,
      'application-key': stationData1.appKey,
      'location-lat': stationData1.lat,
      'location-lng': stationData1.lng,
    });

    await page.selectOption(
      '[data-testid="station-type-select"]',
      stationData1.type
    );
    await page.click('[data-testid="connect-station-button"]');

    await helpers.waitForNotification('success');
    await expect(
      page.locator(`[data-testid="station-${stationData1.name}"]`)
    ).toBeVisible();

    // Step 2: Start collecting weather data
    await helpers.mockWeatherDataCollection(stationData1.name, [
      {
        timestamp: '2025-01-01T00:00:00Z',
        temperature: 32.5,
        humidity: 78,
        pressure: 1013.25,
        windSpeed: 5.2,
      },
      {
        timestamp: '2025-01-01T01:00:00Z',
        temperature: 31.8,
        humidity: 80,
        pressure: 1012.8,
        windSpeed: 4.8,
      },
      {
        timestamp: '2025-01-01T02:00:00Z',
        temperature: 31.2,
        humidity: 82,
        pressure: 1012.3,
        windSpeed: 4.5,
      },
    ]);

    // Verify data collection
    await page.click(`[data-testid="view-station-${stationData1.name}"]`);
    await expect(
      page.locator('[data-testid="data-points-count"]')
    ).toContainText('3');
    await expect(page.locator('[data-testid="earnings-amount"]')).toContainText(
      'WRF'
    );

    // === PHASE 2: DATA QUALITY ENHANCEMENT ===

    // Step 3: Add second station for triangulation
    await helpers.navigateTo('/weather-stations');
    await page.click('[data-testid="add-weather-station-button"]');

    const stationData2 = {
      name: 'Urban Climate Station Beta',
      type: 'weatherflow',
      location: 'Manhattan South',
      lat: '40.7089',
      lng: '-74.0184',
    };

    await helpers.setupWeatherStation(stationData2);

    // Step 4: Collect correlated data
    await helpers.mockWeatherDataCollection(stationData2.name, [
      {
        timestamp: '2025-01-01T00:00:00Z',
        temperature: 33.1,
        humidity: 76,
        pressure: 1013.1,
        windSpeed: 5.8,
      },
      {
        timestamp: '2025-01-01T01:00:00Z',
        temperature: 32.3,
        humidity: 78,
        pressure: 1012.6,
        windSpeed: 5.2,
      },
      {
        timestamp: '2025-01-01T02:00:00Z',
        temperature: 31.8,
        humidity: 80,
        pressure: 1012.1,
        windSpeed: 4.9,
      },
    ]);

    // Step 5: Generate high-value dataset
    await helpers.navigateTo('/weather-stations/analytics');

    // Verify data correlation
    await expect(
      page.locator('[data-testid="station-correlation"]')
    ).toBeVisible();
    const correlationScore = await page
      .locator('[data-testid="correlation-score"]')
      .textContent();
    expect(parseFloat(correlationScore)).toBeGreaterThan(0.85);

    // Generate premium dataset
    await page.click('[data-testid="generate-premium-dataset"]');
    await page.fill(
      '[data-testid="dataset-name"]',
      'Manhattan Microclimate Study - January 2025'
    );
    await page.fill(
      '[data-testid="dataset-description"]',
      'High-resolution microclimate data from two urban stations with validated correlation'
    );

    await page.selectOption('[data-testid="dataset-quality"]', 'premium');
    await page.click('[data-testid="generate-dataset-button"]');

    await expect(
      page.locator('[data-testid="dataset-generated"]')
    ).toBeVisible();
    const datasetId = await page
      .locator('[data-testid="dataset-id"]')
      .textContent();

    // === PHASE 3: NFT CREATION AND ENHANCEMENT ===

    // Step 6: Create NFT from premium dataset
    await helpers.navigateTo('/nft-marketplace/create');
    await page.click('[data-testid="import-from-dataset"]');

    // Select the generated dataset
    await page.selectOption('[data-testid="dataset-select"]', datasetId);

    // Auto-populate NFT metadata from dataset
    await expect(page.locator('[data-testid="nft-title"]')).toHaveValue(
      'Manhattan Microclimate Study - January 2025'
    );

    // Enhance NFT with additional metadata
    await page.fill(
      '[data-testid="nft-tags"]',
      'urban-climate, microclimate, manhattan, premium-data, validated'
    );
    await page.fill('[data-testid="royalty-percentage"]', '8');

    // Set pricing based on data quality
    const suggestedPrice = await page
      .locator('[data-testid="suggested-price"]')
      .textContent();
    const priceValue = parseFloat(suggestedPrice.replace(/[^0-9.]/g, ''));
    expect(priceValue).toBeGreaterThan(50); // Premium data should suggest higher price

    await page.fill('[data-testid="nft-price"]', priceValue.toString());

    // Step 7: Add data provenance and certificates
    await page.click('[data-testid="add-provenance-section"]');

    // Station verification
    await expect(
      page.locator('[data-testid="station-alpha-verified"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="station-beta-verified"]')
    ).toBeVisible();

    // Data quality certificate
    await page.click('[data-testid="generate-quality-certificate"]');
    await expect(
      page.locator('[data-testid="quality-certificate-generated"]')
    ).toBeVisible();

    // ISO compliance badge
    await page.click('[data-testid="add-iso-compliance"]');
    await page.selectOption('[data-testid="iso-standard"]', 'ISO20022');
    await expect(page.locator('[data-testid="iso-badge-added"]')).toBeVisible();

    // Step 8: Mint premium NFT
    await page.click('[data-testid="preview-nft-button"]');

    // Verify preview shows all enhancements
    const preview = page.locator('[data-testid="nft-preview-modal"]');
    await expect(
      preview.locator('[data-testid="data-points-count"]')
    ).toContainText('6');
    await expect(
      preview.locator('[data-testid="quality-score"]')
    ).toBeVisible();
    await expect(
      preview.locator('[data-testid="iso-compliance-badge"]')
    ).toBeVisible();

    await page.click('[data-testid="mint-nft-button"]');
    await page.click('[data-testid="approve-mint-button"]');

    const mintTxHash = await page
      .locator('[data-testid="mint-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(mintTxHash);

    const nftTokenId = await page
      .locator('[data-testid="nft-token-id"]')
      .textContent();

    // === PHASE 4: MARKETPLACE OPTIMIZATION ===

    // Step 9: List NFT with dynamic pricing
    await page.waitForURL('**/my-nfts');
    const premiumNft = page.locator(`[data-testid="owned-nft-${nftTokenId}"]`);
    await premiumNft.click();

    await page.click('[data-testid="list-nft-button"]');

    // Enable dynamic pricing
    await page.check('[data-testid="enable-dynamic-pricing"]');
    await page.fill('[data-testid="base-price"]', priceValue.toString());
    await page.fill('[data-testid="price-adjustment-factor"]', '0.1'); // 10% adjustment based on demand

    // Set auction parameters
    await page.check('[data-testid="enable-auction"]');
    await page.fill('[data-testid="auction-duration"]', '7'); // 7 days
    await page.fill(
      '[data-testid="reserve-price"]',
      (priceValue * 0.8).toString()
    );

    await page.click('[data-testid="confirm-listing-button"]');
    await page.click('[data-testid="approve-listing-button"]');

    const listingTxHash = await page
      .locator('[data-testid="listing-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(listingTxHash);

    // === PHASE 5: MARKETING AND DISCOVERY ===

    // Step 10: Promote NFT in marketplace
    await helpers.navigateTo('/nft-marketplace');

    // Verify NFT appears in premium section
    await page.click('[data-testid="filter-premium"]');
    const premiumListing = page.locator(
      `[data-testid="nft-card-${nftTokenId}"]`
    );
    await expect(premiumListing).toBeVisible();

    // Check featured placement due to quality
    await expect(
      premiumListing.locator('[data-testid="featured-badge"]')
    ).toBeVisible();
    await expect(
      premiumListing.locator('[data-testid="quality-score"]')
    ).toBeVisible();

    // Step 11: Engage with community
    await premiumListing.click();

    // Add detailed description for buyers
    await page.click('[data-testid="edit-description-button"]');
    await page.fill(
      '[data-testid="additional-description"]',
      'This premium dataset represents the first validated microclimate study of Manhattan urban heat islands. ' +
        'Data includes synchronized measurements from two strategically placed stations with 98.7% correlation. ' +
        'Perfect for climate research, urban planning, and environmental impact studies.'
    );
    await page.click('[data-testid="save-description"]');

    // === PHASE 6: SALES AND ROYALTY TRACKING ===

    // Step 12: Monitor sales activity
    await helpers.navigateTo('/creator-dashboard');

    // Check NFT performance metrics
    await expect(page.locator('[data-testid="nfts-listed"]')).toContainText(
      '1'
    );
    await expect(page.locator('[data-testid="total-views"]')).toBeVisible();

    // Simulate market interest
    await helpers.mockNFTInteraction(nftTokenId, {
      views: 45,
      favorites: 12,
      shares: 8,
      inquiries: 3,
    });

    await page.reload();
    await expect(page.locator('[data-testid="nft-engagement"]')).toContainText(
      '45'
    ); // views

    // Step 13: Receive and process sale
    await helpers.mockNFTPurchase(nftTokenId, {
      buyer: helpers.testData.users.user2.walletAddress,
      salePrice: priceValue * 1.2, // 20% above listing due to auction
      saleType: 'auction_win',
    });

    // Verify sale notification
    await helpers.waitForNotification('nft-sold');

    // Check creator dashboard for sale
    await page.reload();
    await expect(page.locator('[data-testid="nfts-sold"]')).toContainText('1');
    await expect(
      page.locator('[data-testid="total-sales-volume"]')
    ).toContainText((priceValue * 1.2).toString());

    // === PHASE 7: REINVESTMENT AND SCALING ===

    // Step 14: Use proceeds to expand weather network
    await helpers.navigateTo('/wallet');

    const finalBalance = await helpers.getWalletBalance();
    expect(finalBalance.WRF).toBeGreaterThan(priceValue); // Should include sale proceeds

    // Check transaction history
    await page.click('[data-testid="transaction-history-tab"]');

    const saleTransaction = page
      .locator('[data-testid="transaction-item"]')
      .filter({ hasText: 'NFT Sale' });
    await expect(saleTransaction).toBeVisible();
    await expect(saleTransaction).toContainText((priceValue * 1.2).toString());

    // Step 15: Plan network expansion
    await helpers.navigateTo('/weather-stations/expansion');

    // Use sale proceeds to fund new stations
    await page.click('[data-testid="plan-expansion-button"]');
    await page.fill(
      '[data-testid="expansion-budget"]',
      (priceValue * 0.8).toString()
    );

    const expansionPlan = await page.locator(
      '[data-testid="expansion-recommendations"]'
    );
    await expect(expansionPlan).toBeVisible();

    const recommendedStations = await expansionPlan
      .locator('[data-testid="recommended-station"]')
      .count();
    expect(recommendedStations).toBeGreaterThan(0);

    // === VERIFICATION: COMPLETE MONETIZATION CYCLE ===

    // Step 16: Verify complete weather-to-NFT pipeline
    await helpers.navigateTo('/dashboard/analytics');

    // Check monetization metrics
    await expect(
      page.locator('[data-testid="weather-stations-active"]')
    ).toContainText('2');
    await expect(
      page.locator('[data-testid="datasets-generated"]')
    ).toContainText('1');
    await expect(page.locator('[data-testid="nfts-created"]')).toContainText(
      '1'
    );
    await expect(page.locator('[data-testid="nfts-sold"]')).toContainText('1');

    // Calculate ROI
    const totalEarnings = await page
      .locator('[data-testid="total-weather-earnings"]')
      .textContent();
    const nftSaleValue = await page
      .locator('[data-testid="total-nft-sales"]')
      .textContent();

    const totalRevenue =
      parseFloat(totalEarnings.replace(/[^0-9.]/g, '')) +
      parseFloat(nftSaleValue.replace(/[^0-9.]/g, ''));

    expect(totalRevenue).toBeGreaterThan(100); // Successful monetization

    // Verify future royalty stream
    await expect(
      page.locator('[data-testid="future-royalties-enabled"]')
    ).toBeVisible();
    const royaltyRate = await page
      .locator('[data-testid="royalty-rate"]')
      .textContent();
    expect(royaltyRate).toContain('8%');

    console.log(
      '✅ Weather data to NFT monetization journey completed successfully'
    );
    console.log(`📊 Total revenue generated: ${totalRevenue} WRF`);
    console.log(`🏭 Weather stations: 2 active`);
    console.log(`💎 NFTs: 1 created and sold`);
    console.log(`💰 Ongoing royalties: 8% of future sales`);
  });

  test('Collaborative weather network with shared NFT revenues', async ({
    page,
    context,
  }) => {
    // Test collaborative approach where multiple users contribute to shared datasets

    // === PHASE 1: NETWORK FORMATION ===

    // User 1: Create weather network proposal
    await helpers.navigateTo('/weather-stations/networks');
    await page.click('[data-testid="create-network-button"]');

    const networkData = {
      name: 'Manhattan Climate Collective',
      description:
        'Collaborative weather monitoring network for Manhattan microclimate research',
      revenueShare: 'proportional', // Based on data contribution
      qualityThreshold: '95', // 95% data quality requirement
    };

    await helpers.fillForm({
      'network-name': networkData.name,
      'network-description': networkData.description,
      'quality-threshold': networkData.qualityThreshold,
    });

    await page.selectOption(
      '[data-testid="revenue-model"]',
      networkData.revenueShare
    );
    await page.click('[data-testid="create-network-button"]');

    const networkId = await page
      .locator('[data-testid="network-id"]')
      .textContent();

    // === PHASE 2: MULTI-USER PARTICIPATION ===

    // User 2: Join network with station
    const page2 = await context.newPage();
    const helpers2 = new TestHelpers(page2);
    await helpers2.loginAs('user2');

    await helpers2.navigateTo(`/weather-stations/networks/${networkId}`);
    await page2.click('[data-testid="join-network-button"]');

    // Add station to network
    await helpers2.setupWeatherStation({
      name: 'Collective Station East',
      networkId: networkId,
    });

    // User 3: Join with complementary station
    const page3 = await context.newPage();
    const helpers3 = new TestHelpers(page3);
    await helpers3.loginAs('user3');

    await helpers3.navigateTo(`/weather-stations/networks/${networkId}`);
    await page3.click('[data-testid="join-network-button"]');

    await helpers3.setupWeatherStation({
      name: 'Collective Station West',
      networkId: networkId,
    });

    // === PHASE 3: COLLABORATIVE DATA COLLECTION ===

    // Simulate synchronized data collection from all stations
    const timeWindow = '2025-01-15T00:00:00Z';

    // All users collect data simultaneously
    await helpers.mockNetworkDataCollection(networkId, [
      {
        station: 'User1 Station',
        user: 'user1',
        dataPoints: 24,
        quality: 98.5,
      },
      {
        station: 'Collective Station East',
        user: 'user2',
        dataPoints: 24,
        quality: 97.2,
      },
      {
        station: 'Collective Station West',
        user: 'user3',
        dataPoints: 23,
        quality: 96.8,
      },
    ]);

    // === PHASE 4: COLLABORATIVE NFT CREATION ===

    // Generate network-wide dataset
    await helpers.navigateTo(
      `/weather-stations/networks/${networkId}/analytics`
    );

    await page.click('[data-testid="generate-network-dataset"]');

    const networkDataset = {
      name: 'Manhattan Collective Climate Dataset - January 2025',
      description:
        'Comprehensive climate dataset from 3-station collaborative network',
      contributors: 3,
      totalDataPoints: 71,
    };

    // Verify dataset quality
    await expect(
      page.locator('[data-testid="network-quality-score"]')
    ).toContainText('97.5');

    // Create collaborative NFT
    await page.click('[data-testid="create-collaborative-nft"]');

    // NFT metadata includes all contributors
    await expect(
      page.locator('[data-testid="contributor-user1"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="contributor-user2"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="contributor-user3"]')
    ).toBeVisible();

    // Set collaborative pricing
    await page.fill('[data-testid="base-price"]', '150');
    await page.selectOption('[data-testid="revenue-split"]', 'proportional');

    await page.click('[data-testid="mint-collaborative-nft"]');

    // All users must approve minting
    await page.click('[data-testid="approve-as-user1"]');

    await page2.reload();
    await page2.click('[data-testid="approve-as-user2"]');

    await page3.reload();
    await page3.click('[data-testid="approve-as-user3"]');

    // Wait for multi-sig minting
    const collaborativeNftId = await page
      .locator('[data-testid="collaborative-nft-id"]')
      .textContent();

    // === PHASE 5: SHARED REVENUE DISTRIBUTION ===

    // Mock NFT sale
    await helpers.mockNFTPurchase(collaborativeNftId, {
      salePrice: 180, // 20% above base price
      saleType: 'direct_purchase',
    });

    // Verify revenue distribution
    const expectedShares = {
      user1: 180 * (24 / 71) * 0.985, // Proportional to data contribution and quality
      user2: 180 * (24 / 71) * 0.972,
      user3: 180 * (23 / 71) * 0.968,
    };

    // Check each user's earnings
    for (const [user, expectedEarning] of Object.entries(expectedShares)) {
      const userPage =
        user === 'user1' ? page : user === 'user2' ? page2 : page3;
      const userHelpers =
        user === 'user1' ? helpers : user === 'user2' ? helpers2 : helpers3;

      await userHelpers.navigateTo('/wallet');
      await userPage.click('[data-testid="transaction-history-tab"]');

      const revenueShare = userPage
        .locator('[data-testid="transaction-item"]')
        .filter({
          hasText: 'Collaborative NFT Revenue',
        });

      await expect(revenueShare).toBeVisible();

      const earnedAmount = await revenueShare
        .locator('[data-testid="amount"]')
        .textContent();
      const earnedValue = parseFloat(earnedAmount.replace(/[^0-9.]/g, ''));

      // Allow 5% variance for calculation precision
      expect(earnedValue).toBeGreaterThan(expectedEarning * 0.95);
      expect(earnedValue).toBeLessThan(expectedEarning * 1.05);
    }

    // === VERIFICATION: COLLABORATIVE SUCCESS ===

    // Check network performance metrics
    await helpers.navigateTo(
      `/weather-stations/networks/${networkId}/performance`
    );

    await expect(
      page.locator('[data-testid="network-stations"]')
    ).toContainText('3');
    await expect(
      page.locator('[data-testid="collaborative-nfts"]')
    ).toContainText('1');
    await expect(
      page.locator('[data-testid="total-network-revenue"]')
    ).toContainText('180');

    // Verify future collaboration incentives
    await expect(
      page.locator('[data-testid="network-reputation"]')
    ).toBeVisible();
    const networkRating = await page
      .locator('[data-testid="network-rating"]')
      .textContent();
    expect(parseFloat(networkRating)).toBeGreaterThan(4.5); // High rating for successful collaboration

    await page2.close();
    await page3.close();

    console.log(
      '✅ Collaborative weather network with shared NFT revenues completed successfully'
    );
  });
});
