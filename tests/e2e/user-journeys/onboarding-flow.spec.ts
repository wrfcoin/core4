import { test, expect } from '@playwright/test';

test.describe('Unified Onboarding Flow', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.removeItem('wrfcoin_onboarding');
      localStorage.removeItem('wrfcoin_wallet');
    });
  });

  test('happy path: navigate to onboarding and see stepper', async ({
    page,
  }) => {
    await test.step('Navigate to onboarding', async () => {
      await page.goto('/onboarding');
      await expect(page.locator('text=Wallet Setup')).toBeVisible();
      await expect(page.locator('text=Fund Wallet')).toBeVisible();
      await expect(page.locator('text=Hardware Wallet')).toBeVisible();
      await expect(page.locator('text=Complete')).toBeVisible();
    });

    await test.step('Step 1: See wallet creation options', async () => {
      await expect(page.locator('text=Create New Wallet')).toBeVisible();
      await expect(
        page.locator('text=Import Existing Wallet')
      ).toBeVisible();
      await expect(
        page.locator('text=Connect Browser Wallet')
      ).toBeVisible();
    });
  });
});
