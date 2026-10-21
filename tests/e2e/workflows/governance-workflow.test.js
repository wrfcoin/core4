const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('Governance Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('user1');
    await helpers.connectWallet('user1');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Create governance proposal', async ({ page }) => {
    // Navigate to governance section
    await page.click('[data-testid="nav-governance"]');
    await page.waitForURL('**/governance');

    // Verify governance dashboard
    await expect(
      page.locator('[data-testid="governance-dashboard"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="active-proposals"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="voting-power"]')).toBeVisible();

    // Create new proposal
    await page.click('[data-testid="create-proposal-button"]');
    await page.waitForURL('**/governance/propose');

    // Select proposal type
    await page.click('[data-testid="proposal-type-parameter-change"]');

    // Fill proposal details
    const proposalData = {
      title: 'Increase Block Reward',
      description:
        'Proposal to increase block reward from 10 WRF to 12 WRF to incentivize more miners and improve network security.',
      summary: 'Block reward increase for better network security',
      category: 'economic-parameters',
    };

    await helpers.fillForm({
      'proposal-title': proposalData.title,
      'proposal-description': proposalData.description,
      'proposal-summary': proposalData.summary,
    });

    await page.selectOption(
      '[data-testid="proposal-category"]',
      proposalData.category
    );

    // Set proposal parameters
    await page.click('[data-testid="add-parameter-change"]');
    await helpers.fillForm({
      'parameter-name-0': 'BLOCK_REWARD',
      'current-value-0': '10',
      'proposed-value-0': '12',
    });

    // Set voting parameters
    await helpers.fillForm({
      'voting-delay': '1', // 1 day delay
      'voting-period': '7', // 7 days voting period
      'execution-delay': '2', // 2 days execution delay
    });

    // Upload supporting documents
    await helpers.uploadFile(
      '[data-testid="supporting-docs-upload"]',
      './fixtures/governance-proposal.pdf'
    );

    // Review proposal
    await page.click('[data-testid="review-proposal-button"]');

    const modal = await helpers.waitForModal();

    // Verify proposal summary
    await expect(
      page.locator('[data-testid="proposal-review-title"]')
    ).toContainText(proposalData.title);
    await expect(
      page.locator('[data-testid="proposal-review-description"]')
    ).toContainText(proposalData.description);
    await expect(
      page.locator('[data-testid="parameter-changes-list"]')
    ).toBeVisible();

    // Check required voting power
    const requiredPower = await page.textContent(
      '[data-testid="required-voting-power"]'
    );
    expect(requiredPower).toMatch(/\d+(\.\d+)?\s*WRF/);

    // Check proposal creation cost
    await expect(
      page.locator('[data-testid="proposal-creation-fee"]')
    ).toBeVisible();

    // Submit proposal
    await page.click('[data-testid="submit-proposal-button"]');

    // Authenticate transaction
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    // Wait for proposal creation
    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify proposal appears in list
    await page.waitForURL('**/governance/proposal/**');
    await expect(page.locator('[data-testid="proposal-status"]')).toContainText(
      'Pending'
    );
    await expect(page.locator('[data-testid="proposal-id"]')).toBeVisible();
  });

  test('Vote on governance proposal', async ({ page }) => {
    await page.click('[data-testid="nav-governance"]');

    // Find active proposal to vote on
    await page
      .click('[data-testid="proposal-item"][data-status="active"]')
      .first();

    // Verify proposal details page
    await expect(page.locator('[data-testid="proposal-title"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="proposal-description"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="voting-section"]')).toBeVisible();

    // Check voting power
    const votingPower = await page.textContent(
      '[data-testid="user-voting-power"]'
    );
    expect(votingPower).toMatch(/\d+(\.\d+)?\s*WRF/);

    // Check current vote distribution
    await expect(
      page.locator('[data-testid="vote-distribution-chart"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="for-votes-count"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="against-votes-count"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="abstain-votes-count"]')
    ).toBeVisible();

    // Cast vote - vote "For"
    await page.click('[data-testid="vote-for-button"]');

    const modal = await helpers.waitForModal();

    // Add voting reason
    await page.fill(
      '[data-testid="vote-reason-input"]',
      'I support this proposal because it will improve network security and incentivize more participation.'
    );

    // Review vote
    await expect(page.locator('[data-testid="vote-summary"]')).toBeVisible();
    await expect(page.locator('[data-testid="vote-choice"]')).toContainText(
      'For'
    );
    await expect(
      page.locator('[data-testid="voting-power-used"]')
    ).toBeVisible();

    // Submit vote
    await page.click('[data-testid="submit-vote-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify vote was recorded
    await page.reload();
    await expect(
      page.locator('[data-testid="user-vote-status"]')
    ).toContainText('Voted For');
    await expect(
      page.locator('[data-testid="user-vote-reason"]')
    ).toBeVisible();
  });

  test('Delegate voting power', async ({ page }) => {
    await page.click('[data-testid="nav-governance"]');
    await page.click('[data-testid="delegation-tab"]');

    // Verify delegation interface
    await expect(
      page.locator('[data-testid="delegation-interface"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="current-delegate"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="delegated-power"]')).toBeVisible();

    // Delegate to another address
    await page.click('[data-testid="change-delegate-button"]');

    const modal = await helpers.waitForModal();

    // Search for delegate
    await page.fill('[data-testid="delegate-search-input"]', 'expert-delegate');
    await page.click('[data-testid="search-delegates-button"]');

    // Select delegate from search results
    await expect(
      page.locator('[data-testid="delegate-result"]')
    ).toHaveCount.toBeGreaterThan(0);
    await page.click('[data-testid="delegate-result"]').first();

    // Review delegate information
    await expect(
      page.locator('[data-testid="delegate-profile"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="delegate-voting-history"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="delegate-participation-rate"]')
    ).toBeVisible();

    // Set delegation amount
    const delegationAmount = '5000'; // Delegate 5000 WRF voting power
    await page.fill(
      '[data-testid="delegation-amount-input"]',
      delegationAmount
    );

    // Review delegation
    await expect(
      page.locator('[data-testid="delegation-summary"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="remaining-voting-power"]')
    ).toBeVisible();

    // Confirm delegation
    await page.click('[data-testid="confirm-delegation-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify delegation was successful
    await page.reload();
    await expect(
      page.locator('[data-testid="active-delegation"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="delegated-amount"]')
    ).toContainText(delegationAmount);
  });

  test('Execute passed proposal', async ({ page }) => {
    // Switch to user with execution permissions
    await helpers.loginAs('admin');
    await helpers.connectWallet('admin');

    await page.click('[data-testid="nav-governance"]');
    await page.click('[data-testid="executable-proposals-tab"]');

    // Find proposal ready for execution
    await page
      .click('[data-testid="proposal-item"][data-status="passed"]')
      .first();

    // Verify proposal passed voting
    await expect(page.locator('[data-testid="proposal-status"]')).toContainText(
      'Passed'
    );
    await expect(
      page.locator('[data-testid="execution-section"]')
    ).toBeVisible();

    // Check execution requirements
    await expect(
      page.locator('[data-testid="execution-delay-status"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="quorum-met"]')).toContainText(
      'Yes'
    );

    // Execute proposal
    await page.click('[data-testid="execute-proposal-button"]');

    const modal = await helpers.waitForModal();

    // Review execution details
    await expect(
      page.locator('[data-testid="execution-summary"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="parameter-changes-to-apply"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="execution-gas-estimate"]')
    ).toBeVisible();

    // Confirm execution
    await page.click('[data-testid="confirm-execution-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'AdminPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();

    // Verify proposal executed
    await page.reload();
    await expect(page.locator('[data-testid="proposal-status"]')).toContainText(
      'Executed'
    );
    await expect(
      page.locator('[data-testid="execution-timestamp"]')
    ).toBeVisible();
  });

  test('Create and vote on emergency proposal', async ({ page }) => {
    // Switch to user with emergency proposal rights
    await helpers.loginAs('council-member');
    await helpers.connectWallet('council-member');

    await page.click('[data-testid="nav-governance"]');
    await page.click('[data-testid="create-proposal-button"]');

    // Select emergency proposal type
    await page.click('[data-testid="proposal-type-emergency"]');

    // Fill emergency proposal details
    await helpers.fillForm({
      'proposal-title': 'Emergency Security Patch',
      'proposal-description':
        'Critical security vulnerability discovered in smart contracts. Immediate patch required.',
      'emergency-justification':
        'Security vulnerability poses immediate risk to user funds',
    });

    // Set emergency parameters (shorter timeframes)
    await helpers.fillForm({
      'voting-delay': '0', // Immediate voting
      'voting-period': '1', // 1 day voting period
      'execution-delay': '0', // Immediate execution
    });

    // Upload security audit report
    await helpers.uploadFile(
      '[data-testid="emergency-docs-upload"]',
      './fixtures/security-audit.pdf'
    );

    // Submit emergency proposal
    await page.click('[data-testid="submit-emergency-proposal-button"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'CouncilPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');

    // Verify emergency proposal created
    await page.waitForURL('**/governance/proposal/**');
    await expect(
      page.locator('[data-testid="proposal-type-badge"]')
    ).toContainText('Emergency');
    await expect(page.locator('[data-testid="proposal-status"]')).toContainText(
      'Active'
    );

    // Vote on emergency proposal
    await page.click('[data-testid="vote-for-button"]');

    const modal = await helpers.waitForModal();
    await page.fill(
      '[data-testid="vote-reason-input"]',
      'Emergency security fix is critical for platform safety.'
    );
    await page.click('[data-testid="submit-vote-button"]');

    await page.fill('[data-testid="wallet-password-input"]', 'CouncilPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.waitForClose();
  });

  test('Browse governance history and analytics', async ({ page }) => {
    await page.click('[data-testid="nav-governance"]');
    await page.click('[data-testid="history-tab"]');

    // Verify governance history interface
    await expect(
      page.locator('[data-testid="proposal-history"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="governance-stats"]')
    ).toBeVisible();

    // Check governance statistics
    await expect(page.locator('[data-testid="total-proposals"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="passed-proposals"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="voter-participation"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="average-voting-power"]')
    ).toBeVisible();

    // Filter proposals by status
    await page.selectOption('[data-testid="status-filter"]', 'executed');
    const executedProposals = page.locator(
      '[data-testid="proposal-item"][data-status="executed"]'
    );
    await expect(executedProposals).toHaveCount.toBeGreaterThan(0);

    // Filter by category
    await page.selectOption(
      '[data-testid="category-filter"]',
      'economic-parameters'
    );
    await page.waitForTimeout(1000);

    // Test date range filtering
    await page.click('[data-testid="date-filter-button"]');
    await page.fill('[data-testid="start-date"]', '2024-01-01');
    await page.fill('[data-testid="end-date"]', '2024-12-31');
    await page.click('[data-testid="apply-date-filter"]');

    // View proposal details from history
    await page.click('[data-testid="proposal-item"]').first();

    // Verify proposal details page
    await expect(
      page.locator('[data-testid="proposal-timeline"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="voting-results"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="execution-details"]')
    ).toBeVisible();

    // Check governance analytics
    await page.goBack();
    await page.click('[data-testid="analytics-tab"]');

    await expect(
      page.locator('[data-testid="participation-chart"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="proposal-outcomes-chart"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="voting-power-distribution"]')
    ).toBeVisible();
  });

  test('Governance notification and alerts', async ({ page }) => {
    await page.click('[data-testid="nav-governance"]');
    await page.click('[data-testid="notifications-tab"]');

    // Verify notification preferences
    await expect(
      page.locator('[data-testid="notification-settings"]')
    ).toBeVisible();

    // Enable governance notifications
    await page.check('[data-testid="notify-new-proposals"]');
    await page.check('[data-testid="notify-voting-reminders"]');
    await page.check('[data-testid="notify-execution-alerts"]');
    await page.check('[data-testid="notify-delegation-changes"]');

    // Set notification methods
    await page.check('[data-testid="email-notifications"]');
    await page.check('[data-testid="browser-notifications"]');

    // Save settings
    await page.click('[data-testid="save-notification-settings"]');
    await helpers.waitForNotification('success');

    // Test notification for new proposal
    if (
      await page
        .locator('[data-testid="test-notifications-button"]')
        .isVisible()
    ) {
      await page.click('[data-testid="test-notifications-button"]');
      await helpers.waitForNotification('info');
    }

    // Check notification history
    await expect(
      page.locator('[data-testid="notification-history"]')
    ).toBeVisible();
    const notifications = page.locator('[data-testid="notification-item"]');

    if ((await notifications.count()) > 0) {
      await expect(
        notifications.first().locator('[data-testid="notification-type"]')
      ).toBeVisible();
      await expect(
        notifications.first().locator('[data-testid="notification-timestamp"]')
      ).toBeVisible();
    }
  });

  test('Governance accessibility and mobile responsiveness', async ({
    page,
  }) => {
    // Test keyboard navigation
    await page.keyboard.press('Tab');
    await page.keyboard.press('Tab');
    await page.keyboard.press('Enter');

    // Check accessibility violations
    const violations = await helpers.checkAccessibility();
    expect(violations).toHaveLength(0);

    // Test mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });

    await page.click('[data-testid="nav-governance"]');

    // Mobile-specific elements should be visible
    await expect(
      page.locator('[data-testid="mobile-governance-nav"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="mobile-proposal-list"]')
    ).toBeVisible();

    // Test mobile voting interface
    if (
      await page.locator('[data-testid="proposal-item"]').first().isVisible()
    ) {
      await page.click('[data-testid="proposal-item"]').first();
      await expect(
        page.locator('[data-testid="mobile-voting-interface"]')
      ).toBeVisible();
    }
  });

  test('Governance error handling and edge cases', async ({ page }) => {
    await page.click('[data-testid="nav-governance"]');

    // Test insufficient voting power error
    await page.click('[data-testid="create-proposal-button"]');
    await page.click('[data-testid="proposal-type-parameter-change"]');

    // Try to create proposal without enough voting power (mock scenario)
    await helpers.fillForm({
      'proposal-title': 'Test Proposal',
      'proposal-description': 'Test description',
    });

    await page.click('[data-testid="submit-proposal-button"]');

    if (
      await page
        .locator('[data-testid="insufficient-voting-power-error"]')
        .isVisible()
    ) {
      expect(
        await page.textContent(
          '[data-testid="insufficient-voting-power-error"]'
        )
      ).toContain('voting power');
    }

    // Test network connection errors
    await page.route('**/api/governance/**', route => route.abort());
    await page.reload();
    await expect(
      page.locator('[data-testid="governance-connection-error"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retry-connection-button"]')
    ).toBeVisible();
  });
});
