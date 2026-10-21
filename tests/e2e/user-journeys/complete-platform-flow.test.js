/**
 * End-to-End User Journey: Complete Platform Flow
 * Tests full user journey from registration to advanced platform usage
 */

const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');
const { TestDatabase } = require('../../utils/test-database');
const { MockServices } = require('../../utils/mock-services');

test.describe('Complete Platform User Journey', () => {
  let helpers;
  let testDb;
  let mockServices;
  let userData;

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
    userData = {
      email: helpers.generateRandomData('email'),
      password: 'CompleteJourney123!',
      firstName: 'Journey',
      lastName: 'User',
    };
  });

  test('Complete platform journey: Registration → Weather Station → NFT → Governance → Bridge', async ({
    page,
  }) => {
    // === PHASE 1: USER ONBOARDING ===

    // Step 1: Landing page and registration
    await helpers.navigateTo('/');
    await expect(page.locator('[data-testid="landing-hero"]')).toBeVisible();

    await page.click('[data-testid="get-started-button"]');
    await page.waitForURL('**/register');

    // Complete registration
    await helpers.fillForm({
      'first-name': userData.firstName,
      'last-name': userData.lastName,
      email: userData.email,
      password: userData.password,
      'confirm-password': userData.password,
    });

    await page.check('[data-testid="agree-terms-checkbox"]');
    await page.click('[data-testid="register-button"]');

    // Skip email verification for E2E test
    await page.goto('/wallet-setup');

    // Step 2: Wallet creation
    await page.click('[data-testid="create-new-wallet-button"]');
    await page.waitForSelector('[data-testid="seed-phrase-display"]');

    const seedPhrase = await page.textContent(
      '[data-testid="seed-phrase-display"]'
    );
    expect(seedPhrase.split(' ')).toHaveLength(12);

    await page.check('[data-testid="seed-phrase-backed-up-checkbox"]');
    await page.click('[data-testid="continue-button"]');

    // Confirm seed phrase (simplified for E2E)
    const seedWords = seedPhrase.split(' ');
    await page.fill('[data-testid="seed-word-3"]', seedWords[2]);
    await page.fill('[data-testid="seed-word-6"]', seedWords[5]);
    await page.fill('[data-testid="seed-word-9"]', seedWords[8]);

    await page.click('[data-testid="confirm-seed-phrase-button"]');

    // Set wallet password
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.fill(
      '[data-testid="confirm-wallet-password-input"]',
      'WalletPass123!'
    );
    await page.click('[data-testid="set-wallet-password-button"]');

    // Step 3: Complete onboarding to dashboard
    await page.goto('/dashboard'); // Skip KYC for E2E
    await expect(page.locator('[data-testid="wallet-balance"]')).toBeVisible();

    // Add test funds to wallet for journey
    await helpers.addTestFunds('WRF', '1000');

    // === PHASE 2: WEATHER STATION SETUP ===

    // Step 4: Connect weather station
    await page.click('[data-testid="connect-weather-station-button"]');

    const stationModal = await helpers.waitForModal();
    await page.click('[data-testid="station-type-ambient-weather"]');

    await helpers.fillForm({
      'station-name': 'Journey Test Station',
      'api-key': 'test-api-key-journey',
      'application-key': 'test-app-key-journey',
      'location-lat': '40.7128',
      'location-lng': '-74.0060',
    });

    await page.click('[data-testid="connect-station-button"]');
    await helpers.waitForNotification('success');
    await stationModal.close();

    // Step 5: Verify weather data earning
    await helpers.navigateTo('/weather-stations');
    await expect(
      page.locator('[data-testid="station-Journey Test Station"]')
    ).toBeVisible();

    // Simulate data submission and earning
    await helpers.mockWeatherDataSubmission('Journey Test Station', {
      temperature: 72.5,
      humidity: 65,
      pressure: 1013.25,
    });

    // Check earnings
    await page.click('[data-testid="view-earnings"]');
    await expect(page.locator('[data-testid="total-earnings"]')).toContainText(
      'WRF'
    );

    // === PHASE 3: NFT MARKETPLACE PARTICIPATION ===

    // Step 6: Create and list weather data NFT
    await helpers.navigateTo('/nft-marketplace/create');

    const nftData = {
      title: 'Premium Weather Dataset - Journey Station',
      description:
        'High-quality weather data from Manhattan location with hourly readings',
      category: 'weather-data',
      price: '75',
    };

    await helpers.fillForm({
      'nft-title': nftData.title,
      'nft-description': nftData.description,
      'nft-price': nftData.price,
    });

    await page.selectOption(
      '[data-testid="nft-category-select"]',
      nftData.category
    );
    await helpers.uploadFile(
      '[data-testid="nft-file-upload"]',
      './fixtures/weather-data.json'
    );

    await page.click('[data-testid="mint-nft-button"]');
    await page.click('[data-testid="approve-mint-button"]');

    // Wait for minting
    const mintTxHash = await page
      .locator('[data-testid="mint-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(mintTxHash);

    // Step 7: List NFT on marketplace
    await page.waitForURL('**/my-nfts');
    const myNft = page
      .locator('[data-testid="owned-nft"]')
      .filter({ hasText: nftData.title });
    await myNft.click();

    await page.click('[data-testid="list-nft-button"]');
    await page.fill('[data-testid="listing-price-input"]', nftData.price);
    await page.click('[data-testid="confirm-listing-button"]');
    await page.click('[data-testid="approve-listing-button"]');

    const listTxHash = await page
      .locator('[data-testid="listing-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(listTxHash);

    // Step 8: Buy another NFT from marketplace
    await helpers.navigateTo('/nft-marketplace');

    // Find available NFT (not our own)
    const availableNft = page
      .locator('[data-testid="nft-card"]')
      .filter({ hasNotText: nftData.title })
      .first();

    await availableNft.click();
    await page.click('[data-testid="buy-nft-button"]');
    await page.click('[data-testid="confirm-purchase-button"]');
    await page.click('[data-testid="sign-transaction-button"]');

    const purchaseTxHash = await page
      .locator('[data-testid="tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(purchaseTxHash);

    // === PHASE 4: GOVERNANCE PARTICIPATION ===

    // Step 9: Create governance proposal
    await helpers.navigateTo('/governance/create');

    const proposalData = {
      title: 'Enhance Weather Station Rewards for Urban Areas',
      description:
        'Proposal to increase WRF token rewards for weather stations in urban areas with high data demand',
      category: 'economic',
      duration: '7',
    };

    await helpers.fillForm({
      'proposal-title': proposalData.title,
      'proposal-description': proposalData.description,
      'voting-duration': proposalData.duration,
    });

    await page.selectOption(
      '[data-testid="proposal-category"]',
      proposalData.category
    );
    await page.fill('[data-testid="stake-amount-input"]', '100');

    await page.click('[data-testid="submit-proposal-button"]');
    await page.click('[data-testid="approve-stake-button"]');
    await page.click('[data-testid="sign-stake-transaction"]');

    const stakeTxHash = await page
      .locator('[data-testid="stake-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(stakeTxHash);

    const proposalId = await page
      .locator('[data-testid="proposal-id"]')
      .textContent();

    // Step 10: Vote on own proposal
    await page.click('[data-testid="vote-for-button"]');
    await page.click('[data-testid="confirm-vote-button"]');
    await page.click('[data-testid="sign-vote-transaction"]');

    const voteTxHash = await page
      .locator('[data-testid="vote-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(voteTxHash);

    // Step 11: Delegate voting power for future proposals
    await helpers.navigateTo('/governance/delegation');

    const delegateAddress = helpers.testData.users.validator1.walletAddress;
    await page.fill('[data-testid="delegate-address-input"]', delegateAddress);
    await page.fill('[data-testid="delegation-amount-input"]', '200');

    await page.click('[data-testid="delegate-tokens-button"]');
    await page.click('[data-testid="confirm-delegation-button"]');

    const delegationTxHash = await page
      .locator('[data-testid="delegation-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(delegationTxHash);

    // === PHASE 5: CROSS-CHAIN OPERATIONS ===

    // Step 12: Bridge tokens to Ethereum
    await helpers.navigateTo('/bridge');

    await page.selectOption('[data-testid="source-chain-select"]', 'wrfcoin');
    await page.selectOption(
      '[data-testid="destination-chain-select"]',
      'ethereum'
    );

    // Connect Ethereum wallet
    await page.click('[data-testid="connect-destination-wallet"]');
    await helpers.mockWalletConnection(
      'ethereum',
      helpers.testData.users.user1.ethereumAddress
    );

    const bridgeAmount = '100';
    await page.fill('[data-testid="bridge-amount-input"]', bridgeAmount);

    await page.click('[data-testid="review-bridge-button"]');
    await page.click('[data-testid="confirm-bridge-button"]');
    await page.click('[data-testid="sign-lock-transaction"]');

    const bridgeTxHash = await page
      .locator('[data-testid="lock-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(bridgeTxHash);

    // Mock bridge completion
    await helpers.mockBridgeConfirmation(bridgeTxHash, 'ethereum');
    await expect(page.locator('[data-testid="bridge-complete"]')).toBeVisible();

    // === PHASE 6: PORTFOLIO MANAGEMENT ===

    // Step 13: Review complete portfolio
    await helpers.navigateTo('/wallet/portfolio');

    // Verify multi-chain assets
    await expect(page.locator('[data-testid="wrfcoin-balance"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="ethereum-wrf-balance"]')
    ).toContainText(bridgeAmount);

    // Check NFT portfolio value
    await page.click('[data-testid="nft-portfolio-tab"]');
    await expect(
      page.locator('[data-testid="owned-nfts-count"]')
    ).toContainText('2'); // Created + purchased

    // Check governance participation
    await page.click('[data-testid="governance-tab"]');
    await expect(
      page.locator('[data-testid="proposals-created"]')
    ).toContainText('1');
    await expect(
      page.locator('[data-testid="tokens-delegated"]')
    ).toContainText('200');

    // === PHASE 7: PLATFORM ENGAGEMENT SUMMARY ===

    // Step 14: View comprehensive dashboard
    await helpers.navigateTo('/dashboard');

    // Verify all activities are reflected
    await expect(
      page.locator('[data-testid="weather-stations-count"]')
    ).toContainText('1');
    await expect(page.locator('[data-testid="nfts-owned"]')).toContainText('2');
    await expect(
      page.locator('[data-testid="governance-proposals"]')
    ).toContainText('1');
    await expect(
      page.locator('[data-testid="cross-chain-assets"]')
    ).toBeVisible();

    // Check earnings summary
    const totalEarnings = await page
      .locator('[data-testid="total-platform-earnings"]')
      .textContent();
    expect(parseFloat(totalEarnings.replace(/[^0-9.]/g, ''))).toBeGreaterThan(
      0
    );

    // Step 15: Export activity report
    await page.click('[data-testid="export-activity-button"]');
    await page.selectOption('[data-testid="export-format"]', 'pdf');
    await page.click('[data-testid="generate-report-button"]');

    await expect(
      page.locator('[data-testid="report-generated"]')
    ).toBeVisible();
    const reportLink = await page
      .locator('[data-testid="download-report-link"]')
      .getAttribute('href');
    expect(reportLink).toContain('.pdf');

    // === VERIFICATION: COMPLETE JOURNEY METRICS ===

    // Step 16: Verify journey completion metrics
    await helpers.navigateTo('/profile/achievements');

    const expectedAchievements = [
      'first-weather-station',
      'nft-creator',
      'nft-collector',
      'governance-participant',
      'cross-chain-user',
      'platform-explorer',
    ];

    for (const achievement of expectedAchievements) {
      await expect(
        page.locator(`[data-testid="achievement-${achievement}"]`)
      ).toBeVisible();
    }

    // Verify platform engagement score
    const engagementScore = await page
      .locator('[data-testid="engagement-score"]')
      .textContent();
    expect(parseFloat(engagementScore)).toBeGreaterThan(75); // High engagement

    // Final wallet balance verification
    await helpers.navigateTo('/wallet');
    const finalBalance = await helpers.getWalletBalance();

    // Should have substantial balance from various activities
    expect(finalBalance.WRF).toBeGreaterThan(500);

    // Transaction history should show all activities
    await page.click('[data-testid="transaction-history-tab"]');

    const transactionTypes = [
      'Weather Earnings',
      'NFT Sale',
      'NFT Purchase',
      'Governance Stake',
      'Governance Vote',
      'Delegation',
      'Bridge Out',
    ];

    for (const txType of transactionTypes) {
      await expect(
        page
          .locator('[data-testid="transaction-item"]')
          .filter({ hasText: txType })
      ).toBeVisible();
    }

    console.log(
      '✅ Complete platform journey test passed - user successfully engaged with all major platform features'
    );
  });

  test('Multi-user collaborative workflow: Proposal creation and community voting', async ({
    page,
    context,
  }) => {
    // This test simulates a collaborative governance process with multiple users

    // === PHASE 1: PROPOSAL CREATOR (USER 1) ===

    // User 1: Create comprehensive proposal
    await helpers.loginAs('user1');
    await helpers.navigateTo('/governance/create');

    const collaborativeProposal = {
      title: 'Community Weather Network Expansion',
      description:
        'Establish community-funded weather station program with shared rewards',
      fundingAmount: '50000',
      duration: '14',
    };

    await helpers.fillForm({
      'proposal-title': collaborativeProposal.title,
      'proposal-description': collaborativeProposal.description,
      'treasury-allocation': collaborativeProposal.fundingAmount,
      'voting-duration': collaborativeProposal.duration,
    });

    await page.click('[data-testid="add-treasury-allocation"]');
    await page.fill('[data-testid="stake-amount-input"]', '500');

    await page.click('[data-testid="submit-proposal-button"]');
    await page.click('[data-testid="approve-stake-button"]');
    await page.click('[data-testid="sign-stake-transaction"]');

    const proposalCreationTx = await page
      .locator('[data-testid="stake-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(proposalCreationTx);

    const proposalId = await page
      .locator('[data-testid="proposal-id"]')
      .textContent();

    // === PHASE 2: COMMUNITY DISCUSSION (MULTIPLE USERS) ===

    // User 2: Comment and ask questions
    const page2 = await context.newPage();
    const helpers2 = new TestHelpers(page2);
    await helpers2.loginAs('user2');

    await helpers2.navigateTo(`/governance/proposal/${proposalId}`);

    // Add community comment
    await page2.fill(
      '[data-testid="comment-input"]',
      'Great proposal! What are the selection criteria for weather station locations?'
    );
    await page2.click('[data-testid="submit-comment-button"]');

    await expect(
      page2.locator('[data-testid="comment-submitted"]')
    ).toBeVisible();

    // User 3: Provide technical analysis
    const page3 = await context.newPage();
    const helpers3 = new TestHelpers(page3);
    await helpers3.loginAs('user3');

    await helpers3.navigateTo(`/governance/proposal/${proposalId}`);

    await page3.fill(
      '[data-testid="comment-input"]',
      'Technical analysis: The proposed network topology would increase data redundancy by 35%. I support this initiative.'
    );
    await page3.click('[data-testid="submit-comment-button"]');

    // === PHASE 3: VOTING PHASE ===

    // User 1: Vote in favor (creator)
    await page.click('[data-testid="vote-for-button"]');
    await page.click('[data-testid="confirm-vote-button"]');
    await page.click('[data-testid="sign-vote-transaction"]');

    const vote1TxHash = await page
      .locator('[data-testid="vote-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(vote1TxHash);

    // User 2: Vote in favor
    await page2.click('[data-testid="vote-for-button"]');
    await page2.click('[data-testid="confirm-vote-button"]');
    await page2.click('[data-testid="sign-vote-transaction"]');

    const vote2TxHash = await page2
      .locator('[data-testid="vote-tx-hash"]')
      .textContent();
    await helpers2.waitForTransaction(vote2TxHash);

    // User 3: Vote in favor
    await page3.click('[data-testid="vote-for-button"]');
    await page3.click('[data-testid="confirm-vote-button"]');
    await page3.click('[data-testid="sign-vote-transaction"]');

    const vote3TxHash = await page3
      .locator('[data-testid="vote-tx-hash"]')
      .textContent();
    await helpers3.waitForTransaction(vote3TxHash);

    // Add more voters to reach quorum
    const additionalVoters = ['validator1', 'validator2', 'user4'];
    const voterPages = [];

    for (const voter of additionalVoters) {
      const voterPage = await context.newPage();
      const voterHelpers = new TestHelpers(voterPage);
      await voterHelpers.loginAs(voter);

      await voterHelpers.navigateTo(`/governance/proposal/${proposalId}`);
      await voterPage.click('[data-testid="vote-for-button"]');
      await voterPage.click('[data-testid="confirm-vote-button"]');
      await voterPage.click('[data-testid="sign-vote-transaction"]');

      const voterTxHash = await voterPage
        .locator('[data-testid="vote-tx-hash"]')
        .textContent();
      await voterHelpers.waitForTransaction(voterTxHash);

      voterPages.push(voterPage);
    }

    // === PHASE 4: PROPOSAL EXECUTION ===

    // Check if proposal passed
    await page.reload();
    await expect(page.locator('[data-testid="proposal-status"]')).toContainText(
      'Passed'
    );
    await expect(page.locator('[data-testid="execution-ready"]')).toBeVisible();

    // Execute the proposal
    await page.click('[data-testid="execute-proposal-button"]');
    await page.click('[data-testid="confirm-execution-button"]');
    await page.click('[data-testid="sign-execution-transaction"]');

    const executionTxHash = await page
      .locator('[data-testid="execution-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(executionTxHash);

    // === PHASE 5: COMMUNITY BENEFITS DISTRIBUTION ===

    // Verify treasury funds distributed
    await helpers.navigateTo('/governance/treasury');

    const treasuryTx = page
      .locator('[data-testid="treasury-transaction"]')
      .filter({ hasText: 'Proposal Execution' });
    await expect(treasuryTx).toBeVisible();
    await expect(treasuryTx).toContainText(collaborativeProposal.fundingAmount);

    // Verify all participants received rewards
    for (let i = 0; i < voterPages.length; i++) {
      const voterPage = voterPages[i];
      const voterHelpers = new TestHelpers(voterPage);

      await voterHelpers.navigateTo('/wallet');
      await voterPage.click('[data-testid="transaction-history-tab"]');

      const rewardTx = voterPage
        .locator('[data-testid="transaction-item"]')
        .filter({ hasText: 'Governance Reward' });
      await expect(rewardTx).toBeVisible();
    }

    // === VERIFICATION: COLLABORATIVE SUCCESS ===

    // Verify community engagement metrics
    await helpers.navigateTo(`/governance/proposal/${proposalId}/analytics`);

    await expect(page.locator('[data-testid="total-voters"]')).toContainText(
      '6'
    );
    await expect(
      page.locator('[data-testid="participation-rate"]')
    ).toContainText('%');
    await expect(
      page.locator('[data-testid="community-support"]')
    ).toContainText('100%'); // All voted in favor

    // Close additional pages
    await page2.close();
    await page3.close();
    for (const voterPage of voterPages) {
      await voterPage.close();
    }

    console.log(
      '✅ Multi-user collaborative workflow test passed - community successfully collaborated on governance proposal'
    );
  });
});
