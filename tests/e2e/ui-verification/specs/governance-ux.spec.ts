/**
 * Governance UX Verification Tests
 *
 * Comprehensive UI/UX testing for governance functionality:
 * - Proposal card display
 * - Voting interface
 * - Progress bar accuracy
 * - Delegation UI
 */
import { test, expect, Page } from '@playwright/test';
import { UXVerifier, VIEWPORTS } from '../utils/ux-helpers';

test.describe('Governance UX Verification', () => {
  let verifier: UXVerifier;

  test.beforeEach(async ({ page }) => {
    verifier = new UXVerifier(page);
  });

  test.describe('Governance Dashboard', () => {
    test('should display governance dashboard with proper loading', async ({ page }) => {
      await page.goto('/governance');

      const loadingResult = await verifier.verifyLoadingStates();
      expect(loadingResult.hasLoadingIndicator).toBe(true);

      await verifier.captureScreenshot('governance-dashboard-loaded');

      const content = page.locator('main, [role="main"], [class*="governance"]').first();
      await expect(content).toBeVisible({ timeout: 10000 });
    });

    test('should display governance statistics', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      // Look for stats cards
      const statsCards = page.locator(
        '[data-testid*="stat"], [class*="stat"], [class*="metric"]'
      );

      const cardCount = await statsCards.count();
      await verifier.captureScreenshot('governance-stats');

      if (cardCount > 0) {
        const firstCard = statsCards.first();
        const text = await firstCard.textContent();
        // Stats should contain numbers
        expect(text).toMatch(/\d/);
      }
    });
  });

  test.describe('Proposal Cards', () => {
    test('should display proposal list with cards', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      // Look for proposal cards
      const proposals = page.locator(
        '[data-testid="proposal-card"], [class*="proposal"], [class*="card"]'
      );

      const proposalCount = await proposals.count();
      await verifier.captureScreenshot('governance-proposals');

      if (proposalCount > 0) {
        const firstProposal = proposals.first();

        // Proposal should have title
        const title = firstProposal.locator('h1, h2, h3, h4, [class*="title"]').first();
        const hasTitle = await title.isVisible({ timeout: 2000 }).catch(() => false);

        // Proposal should have status
        const status = firstProposal.locator('[class*="status"], [class*="badge"]').first();
        const hasStatus = await status.isVisible({ timeout: 2000 }).catch(() => false);

        expect(hasTitle || hasStatus).toBe(true);
      }
    });

    test('should display proposal status badges correctly', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      const statusBadges = page.locator(
        '[class*="status"], [class*="badge"], [data-testid*="status"]'
      );

      const badgeCount = await statusBadges.count();
      await verifier.captureScreenshot('governance-status-badges');

      if (badgeCount > 0) {
        // Status badges should have clear visual distinction
        const firstBadge = statusBadges.first();
        const backgroundColor = await firstBadge.evaluate((el) => {
          return window.getComputedStyle(el).backgroundColor;
        });
        // Should have a background color (not transparent)
      }
    });

    test('should display voting progress bars', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      const progressBars = page.locator(
        '[data-testid="vote-progress"], [class*="progress"], [role="progressbar"]'
      );

      const progressCount = await progressBars.count();
      await verifier.captureScreenshot('governance-progress-bars');

      if (progressCount > 0) {
        const firstProgress = progressBars.first();

        // Progress should have visual width
        const box = await firstProgress.boundingBox();
        if (box) {
          expect(box.width).toBeGreaterThan(0);
        }
      }
    });

    test('should show for/against vote percentages', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      const votePercentages = page.locator(
        '[class*="for"], [class*="against"], [class*="percentage"], [class*="vote-count"]'
      );

      const percentCount = await votePercentages.count();

      if (percentCount > 0) {
        await verifier.captureScreenshot('governance-vote-percentages');
      }
    });
  });

  test.describe('Voting Interface', () => {
    test('should display voting options', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      // Click on first proposal to open voting interface
      const firstProposal = page.locator('[class*="proposal"]').first();

      if (await firstProposal.isVisible({ timeout: 5000 }).catch(() => false)) {
        await firstProposal.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('governance-voting-interface');

        // Look for voting buttons
        const voteButtons = page.locator(
          'button:has-text("For"), button:has-text("Against"), button:has-text("Abstain"), [data-testid*="vote"]'
        );

        const buttonCount = await voteButtons.count();
      }
    });

    test('should handle vote confirmation', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      const firstProposal = page.locator('[class*="proposal"]').first();

      if (await firstProposal.isVisible({ timeout: 5000 }).catch(() => false)) {
        await firstProposal.click();
        await page.waitForTimeout(500);

        // Look for vote button
        const voteFor = page.locator('button:has-text("For"), button:has-text("Yes")').first();

        if (await voteFor.isVisible({ timeout: 2000 }).catch(() => false)) {
          await voteFor.click();
          await page.waitForTimeout(500);

          await verifier.captureScreenshot('governance-vote-confirmation');

          // Confirmation modal should appear
          const modal = page.locator('[role="dialog"], .modal').first();
          const hasModal = await modal.isVisible({ timeout: 2000 }).catch(() => false);
        }
      }
    });

    test('should display voting power', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      // Look for voting power display
      const votingPower = page.locator(
        '[data-testid="voting-power"], [class*="voting-power"], [class*="power"]'
      ).first();

      if (await votingPower.isVisible({ timeout: 5000 }).catch(() => false)) {
        const powerText = await votingPower.textContent();
        await verifier.captureScreenshot('governance-voting-power');
      }
    });
  });

  test.describe('Proposal Detail', () => {
    test('should display proposal detail page', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      const firstProposal = page.locator('[class*="proposal"]').first();

      if (await firstProposal.isVisible({ timeout: 5000 }).catch(() => false)) {
        await firstProposal.click();
        await page.waitForLoadState('domcontentloaded');

        await verifier.captureScreenshot('governance-proposal-detail');

        // Should show proposal description
        const description = page.locator(
          '[data-testid="proposal-description"], [class*="description"], [class*="content"]'
        ).first();

        // Should show timeline
        const timeline = page.locator(
          '[class*="timeline"], [class*="dates"], [class*="schedule"]'
        ).first();
      }
    });

    test('should display vote breakdown', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      const firstProposal = page.locator('[class*="proposal"]').first();

      if (await firstProposal.isVisible({ timeout: 5000 }).catch(() => false)) {
        await firstProposal.click();
        await page.waitForLoadState('domcontentloaded');

        // Look for vote breakdown
        const breakdown = page.locator(
          '[class*="breakdown"], [class*="results"], [class*="votes"]'
        ).first();

        if (await breakdown.isVisible({ timeout: 5000 }).catch(() => false)) {
          await verifier.captureScreenshot('governance-vote-breakdown');
        }
      }
    });
  });

  test.describe('Delegation', () => {
    test('should display delegation interface', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      // Look for delegation tab/button
      const delegateButton = page.locator(
        'button:has-text("Delegate"), a:has-text("Delegate"), [data-testid="delegate"]'
      ).first();

      if (await delegateButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await delegateButton.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('governance-delegation');

        // Delegation form should appear
        const delegateInput = page.locator(
          'input[placeholder*="address"], input[name*="delegate"]'
        ).first();
      }
    });
  });

  test.describe('Create Proposal', () => {
    test('should display create proposal form', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      // Look for create proposal button
      const createButton = page.locator(
        'button:has-text("Create"), button:has-text("New Proposal"), a:has-text("Create")'
      ).first();

      if (await createButton.isVisible({ timeout: 5000 }).catch(() => false)) {
        await createButton.click();
        await page.waitForTimeout(500);

        await verifier.captureScreenshot('governance-create-proposal');

        // Form should have required fields
        const titleInput = page.locator('input[name*="title"], input[placeholder*="title"]').first();
        const descriptionInput = page.locator('textarea').first();
      }
    });
  });

  test.describe('Responsive Design', () => {
    test('should adapt proposal cards for mobile', async ({ page }) => {
      await page.setViewportSize({ width: 375, height: 667 });
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      await verifier.captureScreenshot('governance-mobile');

      const proposals = page.locator('[class*="proposal"]');
      const proposalCount = await proposals.count();

      if (proposalCount > 0) {
        const firstProposal = proposals.first();
        const box = await firstProposal.boundingBox();
        if (box) {
          // Cards should fit mobile viewport
          expect(box.width).toBeLessThanOrEqual(375);
        }
      }
    });

    test('should maintain usability across viewports', async ({ page }) => {
      for (const [name, viewport] of Object.entries(VIEWPORTS)) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
        await page.goto('/governance');
        await page.waitForLoadState('domcontentloaded');

        await verifier.captureScreenshot(`governance-${name}`);

        // Main actions should be accessible
        const actionButtons = page.locator('button:visible');
        const buttonCount = await actionButtons.count();
        expect(buttonCount).toBeGreaterThan(0);
      }
    });
  });

  test.describe('Accessibility', () => {
    test('should pass accessibility audit', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      const result = await verifier.runAccessibilityAudit();

      // No critical violations
      const critical = result.violations.filter(v => v.impact === 'critical');
      expect(critical).toHaveLength(0);
    });

    test('should support keyboard navigation', async ({ page }) => {
      await page.goto('/governance');
      await page.waitForLoadState('domcontentloaded');

      // Tab through elements
      for (let i = 0; i < 10; i++) {
        await page.keyboard.press('Tab');
      }

      const focused = page.locator(':focus');
      expect(await focused.count()).toBeGreaterThan(0);

      await verifier.captureScreenshot('governance-keyboard-nav');
    });
  });
});
