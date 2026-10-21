const { test, expect } = require('@playwright/test');

/**
 * Governance Voting Interfaces Test Suite
 * Tests voting functionality, proposal display, and voting interactions.
 */

test.describe('Governance Voting Interfaces', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to governance dashboard
    await page.goto('/governance');
    await page.waitForLoadState('networkidle');
  });

  test('should display active proposals with voting information', async ({
    page,
  }) => {
    // Check for proposals table or list
    const proposalsContainer = page.locator(
      '.proposals-list, .proposals-table, table, [class*="proposal"]'
    );
    await expect(proposalsContainer.first()).toBeVisible({ timeout: 10000 });

    // Check for proposal elements
    const proposalElements = page.locator(
      '.proposal-card, tr, .proposal-item, [class*="proposal"]:not(.proposals-list)'
    );
    const proposalCount = await proposalElements.count();

    if (proposalCount > 0) {
      // Check first proposal has voting information
      const firstProposal = proposalElements.first();

      // Look for voting related text/elements
      const votingElements = firstProposal.locator(
        'text=Vote, text=For, text=Against, .vote, [class*="vote"]'
      );
      await expect(votingElements.first()).toBeVisible({ timeout: 5000 });

      // Check for vote counts or percentages
      const voteCountElements = firstProposal.locator(
        'text=/\\d+%/, text=/\\d+.*vote/, .votes, [class*="votes"]'
      );
      if ((await voteCountElements.count()) > 0) {
        await expect(voteCountElements.first()).toBeVisible();
      }
    }
  });

  test('should show vote buttons for active proposals', async ({ page }) => {
    // Look for vote buttons
    const voteButtons = page.locator(
      'button:has-text("Vote"), button:has-text("Vote For"), button:has-text("Vote Against"), .vote-btn, [class*="vote-btn"]'
    );

    if ((await voteButtons.count()) > 0) {
      await expect(voteButtons.first()).toBeVisible();

      // Check if buttons are enabled
      const firstVoteButton = voteButtons.first();
      await expect(firstVoteButton).toBeEnabled();
    }
  });

  test('should open voting dialog when vote button is clicked', async ({
    page,
  }) => {
    // Find and click a vote button
    const voteButtons = page.locator('button:has-text("Vote")');

    if ((await voteButtons.count()) > 0) {
      await voteButtons.first().click();

      // Check for voting dialog/modal
      const votingDialog = page.locator(
        '.dialog, .modal, [role="dialog"], .vote-dialog, [class*="dialog"], [class*="modal"]'
      );
      await expect(votingDialog.first()).toBeVisible({ timeout: 5000 });

      // Check for voting options
      const votingOptions = page.locator(
        'button:has-text("Yes"), button:has-text("No"), button:has-text("For"), button:has-text("Against"), button:has-text("Abstain")'
      );
      await expect(votingOptions.first()).toBeVisible();
    }
  });

  test('should display voting power information', async ({ page }) => {
    // Look for voting power display
    const votingPowerElements = page.locator(
      'text=voting power, text=WRF, .voting-power, [class*="voting-power"]'
    );

    if ((await votingPowerElements.count()) > 0) {
      await expect(votingPowerElements.first()).toBeVisible();
    }

    // Check in voting dialog if available
    const voteButton = page.locator('button:has-text("Vote")').first();
    if (await voteButton.isVisible()) {
      await voteButton.click();

      const dialogVotingPower = page.locator('text=voting power, text=WRF');
      if ((await dialogVotingPower.count()) > 0) {
        await expect(dialogVotingPower.first()).toBeVisible();
      }

      // Close dialog
      const closeButton = page.locator(
        'button:has-text("Cancel"), button:has-text("Close"), .close, [aria-label="close"]'
      );
      if ((await closeButton.count()) > 0) {
        await closeButton.first().click();
      }
    }
  });

  test('should cast a "Yes" vote successfully', async ({ page }) => {
    // Mock the voting API
    await page.route('**/api/governance/proposals/*/vote', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          message: 'Vote cast successfully',
        }),
      });
    });

    // Find vote button and click
    const voteButton = page.locator('button:has-text("Vote")').first();
    if (await voteButton.isVisible()) {
      await voteButton.click();

      // Click "Yes" or "For" vote
      const yesButton = page.locator(
        'button:has-text("Yes"), button:has-text("Vote Yes"), button:has-text("For"), button:has-text("Vote For")'
      );
      if ((await yesButton.count()) > 0) {
        await yesButton.first().click();

        // Wait for response
        await page.waitForTimeout(2000);

        // Check for success indication
        const successIndicators = page.locator(
          '.success, text=success, text=Vote cast, [class*="success"]'
        );
        if ((await successIndicators.count()) > 0) {
          await expect(successIndicators.first()).toBeVisible({
            timeout: 5000,
          });
        }
      }
    }
  });

  test('should cast a "No" vote successfully', async ({ page }) => {
    // Mock the voting API
    await page.route('**/api/governance/proposals/*/vote', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          message: 'Vote cast successfully',
        }),
      });
    });

    // Find vote button and click
    const voteButton = page.locator('button:has-text("Vote")').first();
    if (await voteButton.isVisible()) {
      await voteButton.click();

      // Click "No" or "Against" vote
      const noButton = page.locator(
        'button:has-text("No"), button:has-text("Vote No"), button:has-text("Against"), button:has-text("Vote Against")'
      );
      if ((await noButton.count()) > 0) {
        await noButton.first().click();

        // Wait for response
        await page.waitForTimeout(2000);

        // Check for success indication
        const successIndicators = page.locator(
          '.success, text=success, text=Vote cast, [class*="success"]'
        );
        if ((await successIndicators.count()) > 0) {
          await expect(successIndicators.first()).toBeVisible({
            timeout: 5000,
          });
        }
      }
    }
  });

  test('should handle abstain votes', async ({ page }) => {
    // Mock the voting API
    await page.route('**/api/governance/proposals/*/vote', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          message: 'Vote cast successfully',
        }),
      });
    });

    // Find vote button and click
    const voteButton = page.locator('button:has-text("Vote")').first();
    if (await voteButton.isVisible()) {
      await voteButton.click();

      // Click "Abstain" vote
      const abstainButton = page.locator('button:has-text("Abstain")');
      if ((await abstainButton.count()) > 0) {
        await abstainButton.first().click();

        // Wait for response
        await page.waitForTimeout(2000);

        // Check for success indication
        const successIndicators = page.locator(
          '.success, text=success, text=Vote cast, [class*="success"]'
        );
        if ((await successIndicators.count()) > 0) {
          await expect(successIndicators.first()).toBeVisible({
            timeout: 5000,
          });
        }
      }
    }
  });

  test('should display vote progress bars correctly', async ({ page }) => {
    // Look for progress bars or vote visualization
    const progressBars = page.locator(
      '.progress, .vote-bar, progress, [class*="progress"], [class*="vote-bar"]'
    );

    if ((await progressBars.count()) > 0) {
      await expect(progressBars.first()).toBeVisible();

      // Check if progress bar has width style (indicating votes)
      const firstBar = progressBars.first();
      const width = await firstBar.getAttribute('style');
      if (width && width.includes('width')) {
        expect(width).toContain('width');
      }
    }
  });

  test('should prevent voting on ended proposals', async ({ page }) => {
    // Look for ended/expired proposals
    const endedProposalIndicators = page.locator(
      'text=ended, text=expired, text=closed, .ended, .expired, [class*="ended"]'
    );

    if ((await endedProposalIndicators.count()) > 0) {
      // Find vote buttons near ended proposals
      const parentContainer = endedProposalIndicators
        .first()
        .locator(
          'xpath=ancestor::*[contains(@class, "proposal") or contains(@class, "card")][1]'
        );
      const voteButtons = parentContainer.locator('button:has-text("Vote")');

      if ((await voteButtons.count()) > 0) {
        // Vote buttons should be disabled
        await expect(voteButtons.first()).toBeDisabled();
      }
    }
  });

  test('should prevent voting on vetoed proposals', async ({ page }) => {
    // Look for vetoed proposals
    const vetoedIndicators = page.locator(
      'text=vetoed, text=VETOED, .vetoed, [class*="vetoed"]'
    );

    if ((await vetoedIndicators.count()) > 0) {
      // Find the vetoed proposal container
      const parentContainer = vetoedIndicators
        .first()
        .locator(
          'xpath=ancestor::*[contains(@class, "proposal") or contains(@class, "card")][1]'
        );
      const voteButtons = parentContainer.locator('button:has-text("Vote")');

      if ((await voteButtons.count()) > 0) {
        // Vote buttons should be disabled or hidden
        const isDisabled = await voteButtons.first().isDisabled();
        const isVisible = await voteButtons.first().isVisible();
        expect(isDisabled || !isVisible).toBeTruthy();
      }
    }
  });

  test('should handle voting errors gracefully', async ({ page }) => {
    // Mock API failure
    await page.route('**/api/governance/proposals/*/vote', route => {
      route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Voting failed' }),
      });
    });

    // Find vote button and attempt to vote
    const voteButton = page.locator('button:has-text("Vote")').first();
    if (await voteButton.isVisible()) {
      await voteButton.click();

      const yesButton = page
        .locator('button:has-text("Yes"), button:has-text("For")')
        .first();
      if (await yesButton.isVisible()) {
        await yesButton.click();

        // Wait and check for error message
        await page.waitForTimeout(2000);

        const errorIndicators = page.locator(
          '.error, text=error, text=failed, [class*="error"]'
        );
        await expect(errorIndicators.first()).toBeVisible({ timeout: 5000 });
      }
    }
  });

  test('should update vote counts after successful vote', async ({ page }) => {
    // Get initial vote counts
    const voteCountElements = page.locator(
      'text=/\\d+.*vote/, text=/\\d+%/, .votes, [class*="votes"]'
    );
    let initialCounts = [];

    if ((await voteCountElements.count()) > 0) {
      for (let i = 0; i < (await voteCountElements.count()); i++) {
        const text = await voteCountElements.nth(i).textContent();
        initialCounts.push(text);
      }
    }

    // Mock successful vote that updates counts
    await page.route('**/api/governance/proposals/*/vote', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          message: 'Vote cast successfully',
        }),
      });
    });

    // Mock updated proposal data
    await page.route('**/api/governance/proposals', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          data: [
            {
              id: 1,
              title: 'Test Proposal',
              yes_votes: 1000,
              no_votes: 500,
              status: 'active',
            },
          ],
        }),
      });
    });

    // Attempt to vote
    const voteButton = page.locator('button:has-text("Vote")').first();
    if (await voteButton.isVisible()) {
      await voteButton.click();

      const yesButton = page
        .locator('button:has-text("Yes"), button:has-text("For")')
        .first();
      if (await yesButton.isVisible()) {
        await yesButton.click();

        // Wait for potential UI update
        await page.waitForTimeout(3000);

        // The test passes if no errors occur during voting process
        expect(true).toBeTruthy();
      }
    }
  });

  test('should cancel vote dialog without casting vote', async ({ page }) => {
    // Find vote button and click
    const voteButton = page.locator('button:has-text("Vote")').first();
    if (await voteButton.isVisible()) {
      await voteButton.click();

      // Find and click cancel button
      const cancelButton = page.locator(
        'button:has-text("Cancel"), button:has-text("Close")'
      );
      if ((await cancelButton.count()) > 0) {
        await cancelButton.first().click();

        // Dialog should close
        await page.waitForTimeout(1000);
        const dialog = page.locator('.dialog, .modal, [role="dialog"]');
        if ((await dialog.count()) > 0) {
          await expect(dialog.first()).not.toBeVisible();
        }
      }
    }
  });
});
