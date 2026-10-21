/**
 * Integration Tests: Governance ↔ Wallet
 * Tests interactions between governance system and wallet components
 */

const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../e2e/utils/test-helpers');
const { TestDatabase } = require('../utils/test-database');
const { MockServices } = require('../utils/mock-services');

test.describe('Governance and Wallet Integration', () => {
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

  test('Create proposal with wallet token stake', async ({ page }) => {
    // Step 1: Check wallet balance and staking requirements
    await helpers.navigateTo('/wallet');
    const balance = await helpers.getWalletBalance();
    expect(balance.WRF).toBeGreaterThan(1000); // Min stake requirement

    // Step 2: Navigate to governance
    await helpers.navigateTo('/governance');
    await page.click('[data-testid="create-proposal-button"]');

    // Step 3: Fill proposal form
    const proposalData = {
      title: 'Increase Weather Station Rewards',
      description:
        'Proposal to increase rewards for weather station operators by 20%',
      category: 'economic',
      duration: '7', // days
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

    // Step 4: Stake tokens for proposal
    const stakeAmount = '100';
    await page.fill('[data-testid="stake-amount-input"]', stakeAmount);

    // Verify stake requirements
    await expect(
      page.locator('[data-testid="stake-requirement-met"]')
    ).toBeVisible();

    // Step 5: Submit proposal with wallet signature
    await page.click('[data-testid="submit-proposal-button"]');

    // Wallet approval modal
    await expect(
      page.locator('[data-testid="wallet-stake-approval"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="stake-amount-confirm"]')
    ).toContainText(stakeAmount);

    await page.click('[data-testid="approve-stake-button"]');

    // Sign transaction
    await page.click('[data-testid="sign-stake-transaction"]');

    const stakeTxHash = await page
      .locator('[data-testid="stake-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(stakeTxHash);

    // Step 6: Verify proposal created and tokens staked
    await page.waitForURL('**/governance/proposal/*');
    const proposalId = await page
      .locator('[data-testid="proposal-id"]')
      .textContent();

    // Verify wallet balance reduced
    await helpers.navigateTo('/wallet');
    const newBalance = await helpers.getWalletBalance();
    expect(newBalance.WRF).toBe(balance.WRF - parseFloat(stakeAmount));

    // Verify staked tokens shown
    await expect(page.locator('[data-testid="staked-tokens"]')).toContainText(
      stakeAmount
    );
  });

  test('Vote on proposal with delegated voting power', async ({
    page,
    context,
  }) => {
    // Step 1: Setup delegation (user1 delegates to user2)
    await helpers.navigateTo('/governance/delegation');

    const delegateAddress = helpers.testData.users.user2.walletAddress;
    await page.fill('[data-testid="delegate-address-input"]', delegateAddress);
    await page.fill('[data-testid="delegation-amount-input"]', '500');

    await page.click('[data-testid="delegate-tokens-button"]');
    await page.click('[data-testid="confirm-delegation-button"]');

    const delegationTxHash = await page
      .locator('[data-testid="delegation-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(delegationTxHash);

    // Step 2: Login as user2 (delegate)
    const page2 = await context.newPage();
    const helpers2 = new TestHelpers(page2);
    await helpers2.loginAs('user2');

    // Step 3: Navigate to active proposals
    await helpers2.navigateTo('/governance');
    const proposal = page2.locator('[data-testid="proposal-card"]').first();
    await proposal.click();

    // Step 4: Verify delegated voting power
    const votingPower = await page2
      .locator('[data-testid="total-voting-power"]')
      .textContent();
    expect(parseFloat(votingPower)).toBeGreaterThan(500); // Own tokens + delegated

    // Show breakdown
    await page2.click('[data-testid="voting-power-details"]');
    await expect(page2.locator('[data-testid="own-tokens"]')).toBeVisible();
    await expect(
      page2.locator('[data-testid="delegated-tokens"]')
    ).toContainText('500');

    // Step 5: Cast vote with delegated power
    await page2.click('[data-testid="vote-for-button"]');

    // Vote confirmation modal
    await expect(
      page2.locator('[data-testid="vote-confirmation-modal"]')
    ).toBeVisible();
    await expect(page2.locator('[data-testid="vote-weight"]')).toContainText(
      votingPower
    );

    await page2.click('[data-testid="confirm-vote-button"]');
    await page2.click('[data-testid="sign-vote-transaction"]');

    const voteTxHash = await page2
      .locator('[data-testid="vote-tx-hash"]')
      .textContent();
    await helpers2.waitForTransaction(voteTxHash);

    // Step 6: Verify vote recorded
    await expect(
      page2.locator('[data-testid="vote-cast-success"]')
    ).toBeVisible();
    await expect(
      page2.locator('[data-testid="user-vote-status"]')
    ).toContainText('Voted: For');

    // Step 7: Verify proposal vote count updated
    const forVotes = await page2
      .locator('[data-testid="for-votes-count"]')
      .textContent();
    expect(parseFloat(forVotes)).toBeGreaterThanOrEqual(
      parseFloat(votingPower)
    );

    await page2.close();
  });

  test('Execute proposal and distribute treasury funds', async ({ page }) => {
    // Step 1: Navigate to passed proposal ready for execution
    await helpers.navigateTo('/governance');

    // Find executable proposal
    const executableProposal = page
      .locator('[data-testid="proposal-card"]')
      .filter({
        hasText: 'Ready to Execute',
      })
      .first();

    await executableProposal.click();

    // Step 2: Verify execution requirements
    await expect(page.locator('[data-testid="proposal-status"]')).toContainText(
      'Passed'
    );
    await expect(page.locator('[data-testid="execution-ready"]')).toBeVisible();

    // Check treasury allocation
    const treasuryAmount = await page
      .locator('[data-testid="treasury-allocation"]')
      .textContent();
    const allocationValue = parseFloat(treasuryAmount.replace(/[^0-9.]/g, ''));

    // Step 3: Execute proposal
    await page.click('[data-testid="execute-proposal-button"]');

    // Execution confirmation
    await expect(page.locator('[data-testid="execution-modal"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="treasury-transfer-amount"]')
    ).toContainText(treasuryAmount);

    // Step 4: Sign execution transaction
    await page.click('[data-testid="confirm-execution-button"]');
    await page.click('[data-testid="sign-execution-transaction"]');

    const executionTxHash = await page
      .locator('[data-testid="execution-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(executionTxHash);

    // Step 5: Verify execution completed
    await expect(page.locator('[data-testid="proposal-status"]')).toContainText(
      'Executed'
    );
    await expect(
      page.locator('[data-testid="execution-timestamp"]')
    ).toBeVisible();

    // Step 6: Verify treasury funds distributed
    if (allocationValue > 0) {
      // Check beneficiary wallet received funds
      const beneficiary = await page
        .locator('[data-testid="proposal-beneficiary"]')
        .textContent();

      // Navigate to treasury page
      await helpers.navigateTo('/governance/treasury');

      // Find distribution transaction
      const distributionTx = page
        .locator('[data-testid="treasury-transaction"]')
        .filter({
          hasText: 'Distribution',
        })
        .first();

      await expect(distributionTx).toBeVisible();
      await expect(distributionTx).toContainText(treasuryAmount);
      await expect(distributionTx).toContainText(beneficiary);
    }

    // Step 7: Return staked tokens to proposer
    const proposerAddress = await page
      .locator('[data-testid="proposal-creator"]')
      .textContent();

    if (proposerAddress === helpers.testData.users.user1.walletAddress) {
      await helpers.navigateTo('/wallet');

      // Check for returned stake
      await page.click('[data-testid="transaction-history-tab"]');
      const stakeReturn = page
        .locator('[data-testid="transaction-item"]')
        .filter({
          hasText: 'Stake Return',
        })
        .first();

      await expect(stakeReturn).toBeVisible();
      await expect(stakeReturn).toContainText('100 WRF'); // Original stake amount
    }
  });

  test('Governance token vesting and wallet integration', async ({ page }) => {
    // Step 1: Check vesting schedule
    await helpers.navigateTo('/wallet/vesting');

    await expect(
      page.locator('[data-testid="vesting-schedule"]')
    ).toBeVisible();
    const totalVested = await page
      .locator('[data-testid="total-vested-amount"]')
      .textContent();
    const availableToClaim = await page
      .locator('[data-testid="available-to-claim"]')
      .textContent();

    const claimableAmount = parseFloat(
      availableToClaim.replace(/[^0-9.]/g, '')
    );
    expect(claimableAmount).toBeGreaterThan(0);

    // Step 2: Claim vested tokens
    await page.click('[data-testid="claim-vested-tokens-button"]');

    // Claim confirmation modal
    await expect(
      page.locator('[data-testid="claim-confirmation-modal"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="claim-amount"]')).toContainText(
      availableToClaim
    );

    // Get wallet balance before claim
    const balanceBefore = await helpers.getWalletBalance();

    // Step 3: Execute claim
    await page.click('[data-testid="confirm-claim-button"]');
    await page.click('[data-testid="sign-claim-transaction"]');

    const claimTxHash = await page
      .locator('[data-testid="claim-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(claimTxHash);

    // Step 4: Verify tokens added to wallet
    await helpers.navigateTo('/wallet');
    const balanceAfter = await helpers.getWalletBalance();

    expect(balanceAfter.WRF).toBe(balanceBefore.WRF + claimableAmount);

    // Step 5: Use vested tokens for governance
    await helpers.navigateTo('/governance');

    // Verify increased voting power
    const votingPower = await page
      .locator('[data-testid="user-voting-power"]')
      .textContent();
    expect(parseFloat(votingPower)).toBeGreaterThanOrEqual(claimableAmount);

    // Step 6: Verify vesting schedule updated
    await helpers.navigateTo('/wallet/vesting');
    const newAvailable = await page
      .locator('[data-testid="available-to-claim"]')
      .textContent();
    expect(parseFloat(newAvailable.replace(/[^0-9.]/g, ''))).toBe(0);

    // Check next vesting date
    await expect(
      page.locator('[data-testid="next-vesting-date"]')
    ).toBeVisible();
  });

  test('Multi-sig governance proposal with wallet signatures', async ({
    page,
    context,
  }) => {
    // Step 1: Create multi-sig proposal requiring multiple approvals
    await helpers.navigateTo('/governance/create');

    await page.click('[data-testid="multi-sig-proposal-type"]');

    const proposalData = {
      title: 'Emergency Protocol Update',
      description: 'Critical security update requiring multi-sig approval',
      requiredSignatures: '3',
      signers: [
        helpers.testData.users.user1.walletAddress,
        helpers.testData.users.user2.walletAddress,
        helpers.testData.users.user3.walletAddress,
      ],
    };

    await helpers.fillForm({
      'proposal-title': proposalData.title,
      'proposal-description': proposalData.description,
      'required-signatures': proposalData.requiredSignatures,
    });

    // Add signers
    for (const signer of proposalData.signers) {
      await page.fill('[data-testid="add-signer-input"]', signer);
      await page.click('[data-testid="add-signer-button"]');
    }

    // Submit proposal
    await page.click('[data-testid="submit-proposal-button"]');
    await page.click('[data-testid="sign-proposal-creation"]');

    const proposalTxHash = await page
      .locator('[data-testid="proposal-tx-hash"]')
      .textContent();
    await helpers.waitForTransaction(proposalTxHash);

    const proposalId = await page
      .locator('[data-testid="proposal-id"]')
      .textContent();

    // Step 2: First signer approves (user1 already logged in)
    await page.click('[data-testid="approve-multi-sig-button"]');
    await page.click('[data-testid="sign-approval-transaction"]');

    await helpers.waitForTransaction(
      await page.locator('[data-testid="approval-tx-hash"]').textContent()
    );

    await expect(
      page.locator('[data-testid="signatures-count"]')
    ).toContainText('1/3');

    // Step 3: Second signer approves (user2)
    const page2 = await context.newPage();
    const helpers2 = new TestHelpers(page2);
    await helpers2.loginAs('user2');

    await helpers2.navigateTo(`/governance/proposal/${proposalId}`);
    await page2.click('[data-testid="approve-multi-sig-button"]');
    await page2.click('[data-testid="sign-approval-transaction"]');

    await helpers2.waitForTransaction(
      await page2.locator('[data-testid="approval-tx-hash"]').textContent()
    );

    await expect(
      page2.locator('[data-testid="signatures-count"]')
    ).toContainText('2/3');

    // Step 4: Third signer approves (user3)
    const page3 = await context.newPage();
    const helpers3 = new TestHelpers(page3);
    await helpers3.loginAs('user3');

    await helpers3.navigateTo(`/governance/proposal/${proposalId}`);
    await page3.click('[data-testid="approve-multi-sig-button"]');
    await page3.click('[data-testid="sign-approval-transaction"]');

    await helpers3.waitForTransaction(
      await page3.locator('[data-testid="approval-tx-hash"]').textContent()
    );

    // Step 5: Verify proposal auto-executed after threshold
    await expect(
      page3.locator('[data-testid="proposal-status"]')
    ).toContainText('Executed');
    await expect(
      page3.locator('[data-testid="multi-sig-complete"]')
    ).toBeVisible();

    // Verify all signatures recorded
    await page3.click('[data-testid="view-signatures"]');
    for (const signer of proposalData.signers) {
      await expect(
        page3.locator(`[data-testid="signature-${signer}"]`)
      ).toBeVisible();
    }

    await page2.close();
    await page3.close();
  });
});
