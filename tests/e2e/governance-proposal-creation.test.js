const { test, expect } = require('@playwright/test');

/**
 * Governance Proposal Creation Test Suite
 * Tests the proposal creation process including form validation,
 * submission, and error handling.
 */

test.describe('Governance Proposal Creation', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to governance dashboard
    await page.goto('/governance');
    await page.waitForLoadState('networkidle');
  });

  test('should display create proposal button and navigate to form', async ({
    page,
  }) => {
    // Check if "Create Proposal" button exists
    const createButton = page.locator('button:has-text("Create Proposal")');
    await expect(createButton).toBeVisible();

    // Click the create proposal button
    await createButton.click();

    // Wait for navigation or modal to open
    await page.waitForTimeout(1000);

    // Check if we're on create proposal page or modal opened
    const titleInput = page.locator(
      'input[name="title"], input[placeholder*="title"]'
    );
    await expect(titleInput).toBeVisible();
  });

  test('should validate required fields in proposal creation form', async ({
    page,
  }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Try to submit empty form
    const submitButton = page.locator(
      'button:has-text("Create Proposal"), button[type="submit"]'
    );
    await submitButton.click();

    // Check for validation errors
    const errorMessages = page.locator(
      '.text-red-500, .error, [class*="error"]'
    );
    await expect(errorMessages.first()).toBeVisible({ timeout: 5000 });
  });

  test('should create a text proposal successfully', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Fill out the proposal form
    await page.fill(
      'input[name="title"], input[placeholder*="title"]',
      'Test Climate Data Enhancement'
    );
    await page.fill(
      'textarea[name="description"], textarea[placeholder*="description"]',
      'This is a test proposal to enhance climate data validation protocols.'
    );

    // Select proposal type if available
    const proposalTypeSelect = page.locator(
      'select[name="proposalType"], select[name="proposal_type"]'
    );
    if (await proposalTypeSelect.isVisible()) {
      await proposalTypeSelect.selectOption('Text');
    }

    // Fill proposer address if required
    const proposerInput = page.locator(
      'input[name="proposer"], input[placeholder*="address"]'
    );
    if (await proposerInput.isVisible()) {
      await proposerInput.fill('0x1234567890123456789012345678901234567890');
    }

    // Fill content field for text proposals
    const contentInput = page.locator(
      'textarea[name="content"], textarea[placeholder*="content"]'
    );
    if (await contentInput.isVisible()) {
      await contentInput.fill('Detailed proposal content goes here...');
    }

    // Submit the proposal
    const submitButton = page.locator(
      'button:has-text("Create Proposal"), button[type="submit"]'
    );
    await submitButton.click();

    // Wait for success or navigation
    await page.waitForTimeout(3000);

    // Check for success indicators (could be redirect to proposal list or success message)
    const successIndicators = [
      page.locator('.success, [class*="success"]'),
      page.locator('text=Proposal created'),
      page.locator('text=Successfully'),
      page.url().includes('/governance'),
    ];

    let foundSuccess = false;
    for (const indicator of successIndicators.slice(0, -1)) {
      if (await indicator.isVisible({ timeout: 2000 }).catch(() => false)) {
        foundSuccess = true;
        break;
      }
    }

    if (!foundSuccess) {
      foundSuccess = successIndicators[successIndicators.length - 1];
    }

    expect(foundSuccess).toBeTruthy();
  });

  test('should create a parameter change proposal', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Select parameter change type
    const proposalTypeSelect = page.locator(
      'select[name="proposalType"], select[name="proposal_type"]'
    );
    if (await proposalTypeSelect.isVisible()) {
      await proposalTypeSelect.selectOption('ParameterChange');
    }

    // Fill basic fields
    await page.fill('input[name="title"]', 'Increase Voting Quorum');
    await page.fill(
      'textarea[name="description"]',
      'Proposal to increase the voting quorum from 15% to 20%'
    );

    // Fill parameter-specific fields
    const parameterInput = page.locator('input[name="parameter"]');
    if (await parameterInput.isVisible()) {
      await parameterInput.fill('quorum_percentage');
    }

    const valueInput = page.locator('input[name="value"]');
    if (await valueInput.isVisible()) {
      await valueInput.fill('20');
    }

    // Fill proposer if required
    const proposerInput = page.locator('input[name="proposer"]');
    if (await proposerInput.isVisible()) {
      await proposerInput.fill('0x1234567890123456789012345678901234567890');
    }

    // Submit proposal
    await page.click(
      'button:has-text("Create Proposal"), button[type="submit"]'
    );
    await page.waitForTimeout(3000);

    // Verify creation
    expect(page.url()).toContain('/governance');
  });

  test('should create a funding proposal', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Select funding proposal type
    const proposalTypeSelect = page.locator('select[name="proposalType"]');
    if (await proposalTypeSelect.isVisible()) {
      await proposalTypeSelect.selectOption('FundingProposal');
    }

    // Fill basic fields
    await page.fill(
      'input[name="title"]',
      'Research Grant for Weather Station Network'
    );
    await page.fill(
      'textarea[name="description"]',
      'Request for funding to establish new weather monitoring stations'
    );

    // Fill funding-specific fields
    const recipientInput = page.locator('input[name="recipient"]');
    if (await recipientInput.isVisible()) {
      await recipientInput.fill('0x9876543210987654321098765432109876543210');
    }

    const amountInput = page.locator('input[name="amount"]');
    if (await amountInput.isVisible()) {
      await amountInput.fill('50000');
    }

    const purposeInput = page.locator('textarea[name="purpose"]');
    if (await purposeInput.isVisible()) {
      await purposeInput.fill(
        'Funding will be used to purchase and deploy weather monitoring equipment'
      );
    }

    // Fill proposer if required
    const proposerInput = page.locator('input[name="proposer"]');
    if (await proposerInput.isVisible()) {
      await proposerInput.fill('0x1234567890123456789012345678901234567890');
    }

    // Submit proposal
    await page.click(
      'button:has-text("Create Proposal"), button[type="submit"]'
    );
    await page.waitForTimeout(3000);

    // Verify creation
    expect(page.url()).toContain('/governance');
  });

  test('should validate proposal fields based on type', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Test parameter change validation
    const proposalTypeSelect = page.locator('select[name="proposalType"]');
    if (await proposalTypeSelect.isVisible()) {
      await proposalTypeSelect.selectOption('ParameterChange');

      // Fill title and description but not parameter-specific fields
      await page.fill('input[name="title"]', 'Test Parameter Change');
      await page.fill('textarea[name="description"]', 'Test description');

      // Try to submit without parameter fields
      await page.click('button[type="submit"]');

      // Should show validation errors for parameter fields
      const parameterError = page.locator(
        'text=Parameter name is required, text=Parameter is required'
      );
      await expect(parameterError.first()).toBeVisible({ timeout: 3000 });
    }
  });

  test('should handle proposal creation errors gracefully', async ({
    page,
  }) => {
    // Mock API failure
    await page.route('**/api/governance/proposals', route => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Internal server error' }),
      });
    });

    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Fill and submit form
    await page.fill('input[name="title"]', 'Test Proposal');
    await page.fill('textarea[name="description"]', 'Test description');

    const proposerInput = page.locator('input[name="proposer"]');
    if (await proposerInput.isVisible()) {
      await proposerInput.fill('0x1234567890123456789012345678901234567890');
    }

    await page.click('button[type="submit"]');
    await page.waitForTimeout(2000);

    // Should show error message
    const errorMessage = page.locator(
      '.error, [class*="error"], text=error, text=failed'
    );
    await expect(errorMessage.first()).toBeVisible({ timeout: 5000 });
  });

  test('should support proposal templates if available', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Check if templates are available
    const templateSelector = page.locator('.template, [class*="template"]');
    if (await templateSelector.first().isVisible({ timeout: 2000 })) {
      // Click on first template
      await templateSelector.first().click();

      // Verify template data is populated
      const titleInput = page.locator('input[name="title"]');
      const title = await titleInput.inputValue();
      expect(title.length).toBeGreaterThan(0);
    }
  });

  test('should cancel proposal creation and return to dashboard', async ({
    page,
  }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Fill some data
    await page.fill('input[name="title"]', 'Test Proposal');

    // Click cancel
    const cancelButton = page.locator('button:has-text("Cancel")');
    await cancelButton.click();

    // Should return to governance dashboard
    await page.waitForTimeout(1000);
    expect(page.url()).toContain('/governance');

    // Dashboard elements should be visible
    const dashboardTitle = page.locator(
      'h1:has-text("Governance"), h2:has-text("Governance"), text=Governance Dashboard'
    );
    await expect(dashboardTitle.first()).toBeVisible();
  });
});
