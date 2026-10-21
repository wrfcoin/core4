const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('Carbon Credits Trading Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.loginAs('carbon-provider');
    await helpers.connectWallet('carbon-provider');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Register carbon credit project', async ({ page }) => {
    // Navigate to carbon credits section
    await page.click('[data-testid="nav-carbon"]');
    await page.waitForURL('**/carbon');

    // Verify carbon credits dashboard
    await expect(
      page.locator('[data-testid="carbon-dashboard"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="project-registry"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="marketplace"]')).toBeVisible();

    // Register new carbon project
    await page.click('[data-testid="register-project-button"]');
    await page.waitForURL('**/carbon/projects/register');

    // Fill project registration form
    const projectData = {
      name: 'Amazon Rainforest Conservation',
      type: 'Forest Conservation',
      location: 'Amazon Basin, Brazil',
      description:
        'Large-scale rainforest conservation project preventing deforestation and protecting biodiversity',
      methodology: 'REDD+',
    };

    await helpers.fillForm({
      'project-name': projectData.name,
      'project-type': projectData.type,
      'project-location': projectData.location,
      'project-description': projectData.description,
      methodology: projectData.methodology,
    });

    // Set project coordinates and area
    await helpers.fillForm({
      latitude: '-3.4653',
      longitude: '-62.2159',
      'project-area-hectares': '50000',
      'baseline-year': '2020',
    });

    // Upload verification documents
    await helpers.uploadFile(
      '[data-testid="project-design-document"]',
      './fixtures/project-design.pdf'
    );
    await helpers.uploadFile(
      '[data-testid="monitoring-plan"]',
      './fixtures/monitoring-plan.pdf'
    );
    await helpers.uploadFile(
      '[data-testid="environmental-impact"]',
      './fixtures/environmental-impact.pdf'
    );

    // Set credit parameters
    await helpers.fillForm({
      'estimated-annual-credits': '100000',
      'credit-price': '15.50',
      'project-duration-years': '10',
      'verification-standard': 'VCS',
    });

    // Add project milestones
    await page.click('[data-testid="add-milestone"]');
    await helpers.fillForm({
      'milestone-name-0': 'Baseline Assessment Complete',
      'milestone-date-0': '2024-12-31',
      'milestone-credits-0': '25000',
    });

    await page.click('[data-testid="add-milestone"]');
    await helpers.fillForm({
      'milestone-name-1': 'First Verification Audit',
      'milestone-date-1': '2025-06-30',
      'milestone-credits-1': '50000',
    });

    // Review and submit registration
    await page.click('[data-testid="review-registration-button"]');

    const modal = await helpers.waitForModal();

    // Verify registration summary
    await expect(page.locator('[data-testid="project-summary"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="credit-projections"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="verification-docs"]')
    ).toBeVisible();

    // Submit for verification
    await page.click('[data-testid="submit-for-verification"]');

    // Authenticate
    await page.fill('[data-testid="wallet-password-input"]', 'CarbonPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify project status
    await page.waitForURL('**/carbon/projects/**');
    await expect(page.locator('[data-testid="project-status"]')).toContainText(
      'Under Review'
    );
    await expect(page.locator('[data-testid="project-id"]')).toBeVisible();
  });

  test('Mint verified carbon credits', async ({ page }) => {
    // Switch to verifier role
    await helpers.loginAs('carbon-verifier');
    await helpers.connectWallet('carbon-verifier');

    await page.click('[data-testid="nav-carbon"]');
    await page.click('[data-testid="verification-queue-tab"]');

    // Select project for verification
    await page
      .click('[data-testid="project-item"][data-status="pending-verification"]')
      .first();

    // Complete verification process
    await expect(
      page.locator('[data-testid="verification-checklist"]')
    ).toBeVisible();

    // Verify documentation
    await page.check('[data-testid="verify-project-design"]');
    await page.check('[data-testid="verify-monitoring-plan"]');
    await page.check('[data-testid="verify-environmental-impact"]');
    await page.check('[data-testid="verify-baseline-methodology"]');

    // Approve milestone achievement
    await page.click('[data-testid="milestone-verification-tab"]');
    await page.check('[data-testid="milestone-verified-0"]');
    await page.fill('[data-testid="verified-credits-0"]', '25000');

    // Submit verification
    await page.click('[data-testid="approve-verification-button"]');

    const modal = await helpers.waitForModal();

    await page.fill(
      '[data-testid="verification-notes"]',
      'Project meets all VCS standards. Baseline methodology is sound and monitoring plan is comprehensive.'
    );

    await page.click('[data-testid="confirm-verification"]');

    // Authenticate verification
    await page.fill(
      '[data-testid="wallet-password-input"]',
      'VerifierPass123!'
    );
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Switch back to project owner to mint credits
    await helpers.loginAs('carbon-provider');
    await helpers.connectWallet('carbon-provider');

    await page.click('[data-testid="nav-carbon"]');
    await page.click('[data-testid="my-projects-tab"]');

    // Select verified project
    await page
      .click('[data-testid="project-item"][data-status="verified"]')
      .first();

    // Mint carbon credits
    await page.click('[data-testid="mint-credits-button"]');

    const mintModal = await helpers.waitForModal();

    // Review minting details
    await expect(
      page.locator('[data-testid="verified-credits-amount"]')
    ).toContainText('25000');
    await expect(page.locator('[data-testid="minting-cost"]')).toBeVisible();

    // Confirm minting
    await page.click('[data-testid="confirm-mint-credits"]');

    // Authenticate minting transaction
    await page.fill('[data-testid="wallet-password-input"]', 'CarbonPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await mintModal.close();

    // Verify credits minted
    await page.reload();
    await expect(page.locator('[data-testid="minted-credits"]')).toContainText(
      '25000'
    );
    await expect(
      page.locator('[data-testid="available-credits"]')
    ).toContainText('25000');
  });

  test('Trade carbon credits on marketplace', async ({ page }) => {
    // Switch to carbon credit buyer
    await helpers.loginAs('corporation');
    await helpers.connectWallet('corporation');

    await page.click('[data-testid="nav-carbon"]');
    await page.click('[data-testid="marketplace-tab"]');

    // Browse carbon credits marketplace
    await expect(
      page.locator('[data-testid="carbon-marketplace"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="credit-listings"]')).toBeVisible();

    // Filter by project type
    await page.selectOption(
      '[data-testid="filter-project-type"]',
      'Forest Conservation'
    );
    await page.selectOption(
      '[data-testid="filter-verification-standard"]',
      'VCS'
    );

    // Set price range
    await page.fill('[data-testid="min-price-filter"]', '10.00');
    await page.fill('[data-testid="max-price-filter"]', '20.00');
    await page.click('[data-testid="apply-filters"]');

    // Select carbon credits to purchase
    await page.click('[data-testid="credit-listing"]').first();

    // Review credit details
    await expect(page.locator('[data-testid="credit-details"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="project-information"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="verification-status"]')
    ).toBeVisible();

    // Purchase carbon credits
    await page.click('[data-testid="purchase-credits-button"]');

    const modal = await helpers.waitForModal();

    // Set purchase amount
    const purchaseAmount = '5000';
    await page.fill('[data-testid="credit-quantity"]', purchaseAmount);

    // Review purchase details
    await expect(page.locator('[data-testid="total-cost"]')).toBeVisible();
    await expect(page.locator('[data-testid="unit-price"]')).toBeVisible();
    await expect(page.locator('[data-testid="marketplace-fee"]')).toBeVisible();

    // Confirm purchase
    await page.click('[data-testid="confirm-purchase-button"]');

    // Authenticate payment
    await page.fill('[data-testid="wallet-password-input"]', 'CorpPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify purchase in portfolio
    await page.click('[data-testid="my-portfolio-tab"]');
    await expect(page.locator('[data-testid="owned-credits"]')).toContainText(
      purchaseAmount
    );
    await expect(page.locator('[data-testid="portfolio-value"]')).toBeVisible();
  });

  test('Retire carbon credits for offset', async ({ page }) => {
    await page.click('[data-testid="nav-carbon"]');
    await page.click('[data-testid="my-portfolio-tab"]');

    // Select credits to retire
    await page.click('[data-testid="credit-holding"]').first();
    await page.click('[data-testid="retire-credits-button"]');

    const modal = await helpers.waitForModal();

    // Set retirement details
    const retirementAmount = '1000';
    await page.fill('[data-testid="retirement-quantity"]', retirementAmount);
    await page.fill(
      '[data-testid="retirement-reason"]',
      'Corporate sustainability initiative - offsetting 2024 carbon footprint'
    );
    await page.fill(
      '[data-testid="beneficiary-name"]',
      'Global Climate Action Initiative'
    );

    // Review retirement certificate
    await expect(
      page.locator('[data-testid="retirement-certificate-preview"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retirement-serial-number"]')
    ).toBeVisible();

    // Confirm retirement
    await page.click('[data-testid="confirm-retirement-button"]');

    // Authenticate retirement transaction
    await page.fill('[data-testid="wallet-password-input"]', 'CorpPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify retirement in records
    await page.click('[data-testid="retirement-records-tab"]');
    await expect(
      page.locator('[data-testid="retirement-record"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="retired-amount"]')).toContainText(
      retirementAmount
    );
    await expect(
      page.locator('[data-testid="retirement-certificate"]')
    ).toBeVisible();
  });

  test('Carbon credit AMM liquidity provision', async ({ page }) => {
    // Switch to liquidity provider
    await helpers.loginAs('carbon-provider');
    await helpers.connectWallet('carbon-provider');

    await page.click('[data-testid="nav-carbon"]');
    await page.click('[data-testid="amm-pools-tab"]');

    // Create new carbon credit liquidity pool
    await page.click('[data-testid="create-pool-button"]');

    const modal = await helpers.waitForModal();

    // Select token pair: Carbon Credits / USDC
    await page.selectOption('[data-testid="token-a-select"]', 'CARBON-VCS-001');
    await page.selectOption('[data-testid="token-b-select"]', 'USDC');

    // Set initial liquidity
    await page.fill('[data-testid="token-a-amount"]', '10000'); // 10,000 carbon credits
    await page.fill('[data-testid="token-b-amount"]', '150000'); // $150,000 USDC

    // Review pool creation
    await expect(page.locator('[data-testid="initial-price"]')).toContainText(
      '15.00'
    ); // $15 per credit
    await expect(page.locator('[data-testid="pool-share"]')).toContainText(
      '100%'
    );

    // Create pool
    await page.click('[data-testid="create-pool-confirm"]');

    // Authenticate pool creation
    await page.fill('[data-testid="wallet-password-input"]', 'CarbonPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Verify pool appears in list
    await expect(page.locator('[data-testid="amm-pool"]')).toBeVisible();
    await expect(page.locator('[data-testid="pool-tvl"]')).toBeVisible();
    await expect(page.locator('[data-testid="pool-volume"]')).toBeVisible();
  });

  test('Automated carbon credit trading', async ({ page }) => {
    await page.click('[data-testid="nav-carbon"]');
    await page.click('[data-testid="trading-bot-tab"]');

    // Configure automated trading strategy
    await page.click('[data-testid="create-trading-strategy"]');

    const modal = await helpers.waitForModal();

    // Set strategy parameters
    await helpers.fillForm({
      'strategy-name': 'Carbon Price Arbitrage',
      'strategy-type': 'arbitrage',
      'min-price-threshold': '12.00',
      'max-price-threshold': '18.00',
      'trade-size-percentage': '10',
      'stop-loss-percentage': '5',
    });

    // Configure trading pairs
    await page.click('[data-testid="add-trading-pair"]');
    await page.selectOption('[data-testid="pair-token-a-0"]', 'CARBON-VCS-001');
    await page.selectOption('[data-testid="pair-token-b-0"]', 'USDC');

    // Set risk management
    await page.fill('[data-testid="max-daily-trades"]', '50');
    await page.fill('[data-testid="max-position-size"]', '100000');
    await page.check('[data-testid="enable-slippage-protection"]');
    await page.fill('[data-testid="max-slippage"]', '2');

    // Enable strategy
    await page.check('[data-testid="enable-strategy"]');

    // Save strategy
    await page.click('[data-testid="save-strategy"]');

    // Authenticate strategy creation
    await page.fill('[data-testid="wallet-password-input"]', 'CarbonPass123!');
    await page.click('[data-testid="authenticate-button"]');

    await helpers.waitForNotification('success');
    await modal.close();

    // Monitor strategy performance
    await expect(page.locator('[data-testid="active-strategy"]')).toBeVisible();
    await expect(page.locator('[data-testid="strategy-status"]')).toContainText(
      'Active'
    );
    await expect(page.locator('[data-testid="strategy-pnl"]')).toBeVisible();
  });

  test('Carbon credit verification and audit trail', async ({ page }) => {
    await page.click('[data-testid="nav-carbon"]');
    await page.click('[data-testid="verification-tab"]');

    // View credit verification history
    await page.click('[data-testid="credit-batch"]').first();

    // Verify audit trail components
    await expect(
      page.locator('[data-testid="verification-timeline"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="project-documents"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="monitoring-reports"]')
    ).toBeVisible();

    // Check verification events
    const verificationEvents = page.locator(
      '[data-testid="verification-event"]'
    );
    await expect(verificationEvents).toHaveCount.toBeGreaterThan(0);

    // Review individual verification event
    await verificationEvents.first().click();

    await expect(page.locator('[data-testid="verifier-info"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="verification-date"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="verification-standard"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="verification-documents"]')
    ).toBeVisible();

    // Download verification certificate
    await page.click('[data-testid="download-certificate"]');
    await helpers.waitForNotification('success');

    // View blockchain attestation
    await page.click('[data-testid="blockchain-attestation-tab"]');
    await expect(
      page.locator('[data-testid="transaction-hash"]')
    ).toBeVisible();
    await expect(page.locator('[data-testid="block-number"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="attestation-timestamp"]')
    ).toBeVisible();
  });

  test('Carbon credits error handling', async ({ page }) => {
    await page.click('[data-testid="nav-carbon"]');

    // Test insufficient credits error
    await page.click('[data-testid="my-portfolio-tab"]');

    if (await page.locator('[data-testid="credit-holding"]').isVisible()) {
      await page.click('[data-testid="credit-holding"]').first();
      await page.click('[data-testid="retire-credits-button"]');

      // Try to retire more credits than available
      await page.fill('[data-testid="retirement-quantity"]', '999999');
      await expect(
        page.locator('[data-testid="insufficient-credits-error"]')
      ).toBeVisible();
    }

    // Test invalid project data
    await page.click('[data-testid="register-project-button"]');

    // Submit without required fields
    await page.click('[data-testid="submit-for-verification"]');
    await expect(
      page.locator('[data-testid="validation-errors"]')
    ).toBeVisible();

    // Test network connection issues
    await page.route('**/api/carbon/**', route => route.abort());
    await page.reload();
    await expect(
      page.locator('[data-testid="carbon-connection-error"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="retry-connection-button"]')
    ).toBeVisible();
  });

  test('Carbon credits accessibility and mobile responsiveness', async ({
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

    await page.click('[data-testid="nav-carbon"]');

    // Mobile-specific elements should be visible
    await expect(
      page.locator('[data-testid="mobile-carbon-nav"]')
    ).toBeVisible();

    // Test mobile marketplace interface
    await page.click('[data-testid="marketplace-tab"]');
    await expect(
      page.locator('[data-testid="mobile-marketplace-grid"]')
    ).toBeVisible();
  });
});
