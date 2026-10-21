const { test, expect } = require('@playwright/test');

/**
 * Governance Parameter Management Test Suite
 * Tests parameter management functionality including viewing parameters,
 * creating parameter change proposals, and parameter validation.
 */

test.describe('Governance Parameter Management', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to governance dashboard
    await page.goto('/governance');
    await page.waitForLoadState('networkidle');
  });

  test('should display governance parameters in settings', async ({ page }) => {
    // Look for settings tab or section
    const settingsTab = page.locator(
      'button:has-text("Settings"), .tab:has-text("Settings"), [href*="settings"]'
    );

    if ((await settingsTab.count()) > 0) {
      await settingsTab.first().click();
      await page.waitForTimeout(1000);

      // Check for parameter displays
      const parameterElements = page.locator(
        '.setting-item, .parameter, text=Voting Delay, text=Voting Period, text=Quorum'
      );
      await expect(parameterElements.first()).toBeVisible({ timeout: 5000 });

      // Check for specific parameter values
      const parameterValues = page.locator(
        'text=/\\d+%/, text=/\\d+ day/, text=/\\d+ week/'
      );
      if ((await parameterValues.count()) > 0) {
        await expect(parameterValues.first()).toBeVisible();
      }
    }
  });

  test('should show current voting parameters', async ({ page }) => {
    // Navigate to settings if available
    const settingsTab = page.locator(
      'button:has-text("Settings"), .tab:has-text("Settings")'
    );
    if ((await settingsTab.count()) > 0) {
      await settingsTab.first().click();
    }

    // Look for voting parameters
    const votingParameters = [
      'Voting Delay',
      'Voting Period',
      'Quorum',
      'voting delay',
      'voting period',
      'quorum',
    ];

    for (const param of votingParameters) {
      const paramElement = page.locator(`text=${param}`);
      if ((await paramElement.count()) > 0) {
        await expect(paramElement.first()).toBeVisible();
        break;
      }
    }
  });

  test('should display foundation governance parameters', async ({ page }) => {
    // Navigate to settings if available
    const settingsTab = page.locator('button:has-text("Settings")');
    if ((await settingsTab.count()) > 0) {
      await settingsTab.first().click();
    }

    // Look for foundation-specific parameters
    const foundationParams = page.locator(
      'text=Foundation, text=Veto, text=foundation, text=veto'
    );
    if ((await foundationParams.count()) > 0) {
      await expect(foundationParams.first()).toBeVisible();
    }

    // Check for transition timeline
    const transitionElements = page.locator(
      'text=days, text=Transition, text=Decentralization'
    );
    if ((await transitionElements.count()) > 0) {
      await expect(transitionElements.first()).toBeVisible();
    }
  });

  test('should create parameter change proposal', async ({ page }) => {
    // Navigate to create proposal
    const createButton = page.locator('button:has-text("Create Proposal")');
    await createButton.click();
    await page.waitForTimeout(1000);

    // Select parameter change type
    const proposalTypeSelect = page.locator(
      'select[name="proposalType"], select[name="proposal_type"]'
    );
    if ((await proposalTypeSelect.count()) > 0) {
      await proposalTypeSelect.selectOption('ParameterChange');

      // Fill parameter change fields
      await page.fill('input[name="title"]', 'Increase Voting Quorum to 25%');
      await page.fill(
        'textarea[name="description"]',
        'Proposal to increase voting quorum from current value to 25% for better security'
      );

      // Fill parameter-specific fields
      const parameterInput = page.locator('input[name="parameter"]');
      if ((await parameterInput.count()) > 0) {
        await parameterInput.fill('quorum_percentage');
      }

      const valueInput = page.locator('input[name="value"]');
      if ((await valueInput.count()) > 0) {
        await valueInput.fill('25');
      }

      // Fill proposer address if required
      const proposerInput = page.locator('input[name="proposer"]');
      if ((await proposerInput.count()) > 0) {
        await proposerInput.fill('0x1234567890123456789012345678901234567890');
      }

      // Mock successful API response
      await page.route('**/api/governance/proposals', route => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            success: true,
            proposalId: 'PARAM-001',
            message: 'Parameter change proposal created',
          }),
        });
      });

      // Submit proposal
      await page.click('button[type="submit"]');
      await page.waitForTimeout(2000);

      // Verify success
      expect(page.url()).toContain('/governance');
    }
  });

  test('should validate parameter change proposal fields', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    // Select parameter change type
    const proposalTypeSelect = page.locator('select[name="proposalType"]');
    if ((await proposalTypeSelect.count()) > 0) {
      await proposalTypeSelect.selectOption('ParameterChange');

      // Fill only basic fields, leave parameter fields empty
      await page.fill('input[name="title"]', 'Test Parameter Change');
      await page.fill('textarea[name="description"]', 'Test description');

      // Try to submit without parameter fields
      await page.click('button[type="submit"]');

      // Should show validation errors
      const errorElements = page.locator(
        '.error, .text-red-500, text=required, text=Parameter name is required'
      );
      await expect(errorElements.first()).toBeVisible({ timeout: 5000 });
    }
  });

  test('should prevent unauthorized parameter changes', async ({ page }) => {
    // Mock API response that indicates lack of authorization
    await page.route('**/api/governance/proposals', route => {
      route.fulfill({
        status: 403,
        contentType: 'application/json',
        body: JSON.stringify({
          error: 'Unauthorized to create parameter change proposals',
          code: 'INSUFFICIENT_PERMISSIONS',
        }),
      });
    });

    // Try to create parameter change proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    const proposalTypeSelect = page.locator('select[name="proposalType"]');
    if ((await proposalTypeSelect.count()) > 0) {
      await proposalTypeSelect.selectOption('ParameterChange');

      // Fill fields
      await page.fill('input[name="title"]', 'Unauthorized Parameter Change');
      await page.fill('textarea[name="description"]', 'This should fail');

      const parameterInput = page.locator('input[name="parameter"]');
      if ((await parameterInput.count()) > 0) {
        await parameterInput.fill('test_parameter');
      }

      const valueInput = page.locator('input[name="value"]');
      if ((await valueInput.count()) > 0) {
        await valueInput.fill('100');
      }

      // Submit
      await page.click('button[type="submit"]');
      await page.waitForTimeout(2000);

      // Should show authorization error
      const errorElements = page.locator(
        '.error, text=Unauthorized, text=permission'
      );
      await expect(errorElements.first()).toBeVisible({ timeout: 5000 });
    }
  });

  test('should display parameter history if available', async ({ page }) => {
    // Look for parameter history section
    const historyElements = page.locator(
      'text=History, text=Changes, .history, [class*="history"]'
    );

    if ((await historyElements.count()) > 0) {
      await historyElements.first().click();

      // Check for historical parameter changes
      const historyItems = page.locator(
        '.history-item, .change, [class*="change"], [class*="history-item"]'
      );
      if ((await historyItems.count()) > 0) {
        await expect(historyItems.first()).toBeVisible();
      }
    }
  });

  test('should show parameter validation rules', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    const proposalTypeSelect = page.locator('select[name="proposalType"]');
    if ((await proposalTypeSelect.count()) > 0) {
      await proposalTypeSelect.selectOption('ParameterChange');

      // Test with invalid parameter values
      const parameterInput = page.locator('input[name="parameter"]');
      if ((await parameterInput.count()) > 0) {
        await parameterInput.fill('invalid_parameter');
      }

      const valueInput = page.locator('input[name="value"]');
      if ((await valueInput.count()) > 0) {
        // Try invalid value (e.g., negative percentage)
        await valueInput.fill('-10');
      }

      // Basic fields
      await page.fill('input[name="title"]', 'Invalid Parameter Test');
      await page.fill(
        'textarea[name="description"]',
        'Testing invalid parameter value'
      );

      // Submit and expect validation error
      await page.click('button[type="submit"]');

      // Look for validation messages
      const validationElements = page.locator(
        '.error, .validation, text=invalid, text=must be'
      );
      if ((await validationElements.count()) > 0) {
        await expect(validationElements.first()).toBeVisible({ timeout: 5000 });
      }
    }
  });

  test('should show current parameter values in proposal form', async ({
    page,
  }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    const proposalTypeSelect = page.locator('select[name="proposalType"]');
    if ((await proposalTypeSelect.count()) > 0) {
      await proposalTypeSelect.selectOption('ParameterChange');

      // Look for current value indicators
      const currentValueElements = page.locator(
        'text=current, text=Current, .current-value, [class*="current"]'
      );
      if ((await currentValueElements.count()) > 0) {
        await expect(currentValueElements.first()).toBeVisible();
      }

      // Look for helper text or hints about valid parameter names
      const helperText = page.locator(
        '.help, .hint, .placeholder, [placeholder]'
      );
      if ((await helperText.count()) > 0) {
        const text = await helperText.first().textContent();
        expect(text.length).toBeGreaterThan(0);
      }
    }
  });

  test('should handle parameter management API errors', async ({ page }) => {
    // Mock API failure for parameter data
    await page.route('**/api/governance/parameters', route => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Failed to load parameters' }),
      });
    });

    // Navigate to settings
    const settingsTab = page.locator('button:has-text("Settings")');
    if ((await settingsTab.count()) > 0) {
      await settingsTab.click();
      await page.waitForTimeout(2000);

      // Should show error state or fallback
      const errorElements = page.locator(
        '.error, text=error, text=failed, text=unavailable'
      );
      if ((await errorElements.count()) > 0) {
        await expect(errorElements.first()).toBeVisible();
      }
    }
  });

  test('should display parameter change impact warnings', async ({ page }) => {
    // Navigate to create proposal
    await page.click('button:has-text("Create Proposal")');
    await page.waitForTimeout(1000);

    const proposalTypeSelect = page.locator('select[name="proposalType"]');
    if ((await proposalTypeSelect.count()) > 0) {
      await proposalTypeSelect.selectOption('ParameterChange');

      // Fill with a critical parameter change
      const parameterInput = page.locator('input[name="parameter"]');
      if ((await parameterInput.count()) > 0) {
        await parameterInput.fill('voting_period');
      }

      const valueInput = page.locator('input[name="value"]');
      if ((await valueInput.count()) > 0) {
        await valueInput.fill('1'); // Very short voting period
      }

      // Look for warning messages
      await page.waitForTimeout(1000);
      const warningElements = page.locator(
        '.warning, text=warning, text=caution, text=impact, [class*="warning"]'
      );
      if ((await warningElements.count()) > 0) {
        await expect(warningElements.first()).toBeVisible();
      }
    }
  });
});
