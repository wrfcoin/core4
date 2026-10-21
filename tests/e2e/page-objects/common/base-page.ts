import { Page, Locator, expect } from '@playwright/test';

export class BasePage {
  protected page: Page;

  // Common locators
  readonly loadingSpinner: Locator;
  readonly errorMessage: Locator;
  readonly successMessage: Locator;
  readonly navigationMenu: Locator;
  readonly userProfile: Locator;

  constructor(page: Page) {
    this.page = page;

    // Initialize common locators
    this.loadingSpinner = page.locator('[data-testid="loading-spinner"]');
    this.errorMessage = page.locator('[data-testid="error-message"]');
    this.successMessage = page.locator('[data-testid="success-message"]');
    this.navigationMenu = page.locator('[data-testid="navigation-menu"]');
    this.userProfile = page.locator('[data-testid="user-profile"]');
  }

  // Common actions
  async goto(url: string) {
    await this.page.goto(url);
    await this.waitForPageLoad();
  }

  async waitForPageLoad() {
    await this.page.waitForLoadState('domcontentloaded');
    await this.page.waitForSelector('[data-testid="page-loaded"]', {
      timeout: 30000,
    });
  }

  async waitForLoadingToComplete() {
    if (await this.loadingSpinner.isVisible()) {
      await this.loadingSpinner.waitFor({ state: 'hidden', timeout: 30000 });
    }
  }

  async assertPageTitle(expectedTitle: string) {
    await expect(this.page).toHaveTitle(expectedTitle);
  }

  async assertErrorMessage(expectedMessage: string) {
    await expect(this.errorMessage).toBeVisible();
    await expect(this.errorMessage).toContainText(expectedMessage);
  }

  async assertSuccessMessage(expectedMessage: string) {
    await expect(this.successMessage).toBeVisible();
    await expect(this.successMessage).toContainText(expectedMessage);
  }

  async navigateTo(section: string) {
    await this.navigationMenu.locator(`[data-testid="nav-${section}"]`).click();
    await this.waitForPageLoad();
  }

  async takeScreenshot(name: string) {
    return await this.page.screenshot({
      path: `tests/e2e/test-results/screenshots/${name}.png`,
      fullPage: true,
    });
  }

  // Network utilities
  async waitForAPIResponse(endpoint: string, timeout: number = 30000) {
    return await this.page.waitForResponse(
      response => response.url().includes(endpoint) && response.ok(),
      { timeout }
    );
  }

  async interceptAPICall(endpoint: string, mockResponse: any) {
    await this.page.route(`**/${endpoint}`, async route => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockResponse),
      });
    });
  }

  // ==========================================
  // UX Verification Methods (Task 123)
  // ==========================================

  /**
   * Verify loading states are properly displayed
   */
  async verifyLoadingState(): Promise<boolean> {
    const loadingSelectors = [
      '[data-testid="loading-spinner"]',
      '[data-testid="loading-skeleton"]',
      '[role="progressbar"]',
      '.loading',
      '.spinner',
    ];

    for (const selector of loadingSelectors) {
      const element = this.page.locator(selector).first();
      if (await element.isVisible({ timeout: 500 }).catch(() => false)) {
        return true;
      }
    }
    return false;
  }

  /**
   * Verify error states are displayed with clear messaging
   */
  async verifyErrorState(): Promise<{ hasError: boolean; message: string | null }> {
    const errorSelectors = [
      '[data-testid="error-message"]',
      '[data-testid="error-boundary"]',
      '[role="alert"]',
      '.error-message',
      '.error',
    ];

    for (const selector of errorSelectors) {
      const element = this.page.locator(selector).first();
      if (await element.isVisible({ timeout: 500 }).catch(() => false)) {
        const message = await element.textContent();
        return { hasError: true, message };
      }
    }
    return { hasError: false, message: null };
  }

  /**
   * Verify empty states have helpful content
   */
  async verifyEmptyState(): Promise<{ isEmpty: boolean; hasHelpText: boolean }> {
    const emptySelectors = [
      '[data-testid="empty-state"]',
      '[data-testid="no-data"]',
      '.empty-state',
      '.no-data',
    ];

    for (const selector of emptySelectors) {
      const element = this.page.locator(selector).first();
      if (await element.isVisible({ timeout: 500 }).catch(() => false)) {
        const text = await element.textContent();
        return { isEmpty: true, hasHelpText: text ? text.length > 20 : false };
      }
    }
    return { isEmpty: false, hasHelpText: false };
  }

  /**
   * Test responsive design at a specific viewport
   */
  async testResponsiveAt(width: number, height: number): Promise<void> {
    await this.page.setViewportSize({ width, height });
    await this.page.waitForTimeout(300);
  }

  /**
   * Check for horizontal overflow (responsive issues)
   */
  async checkHorizontalOverflow(): Promise<boolean> {
    const bodyWidth = await this.page.evaluate(() => document.body.scrollWidth);
    const viewportWidth = await this.page.evaluate(() => window.innerWidth);
    return bodyWidth > viewportWidth + 10;
  }

  /**
   * Verify button states (hover, focus, disabled)
   */
  async verifyButtonStates(selector: string): Promise<{
    hasHover: boolean;
    hasFocus: boolean;
    isClickable: boolean;
  }> {
    const button = this.page.locator(selector).first();

    if (!(await button.isVisible({ timeout: 1000 }).catch(() => false))) {
      return { hasHover: false, hasFocus: false, isClickable: false };
    }

    // Test hover
    await button.hover();
    const cursor = await button.evaluate(el =>
      window.getComputedStyle(el).cursor
    );
    const hasHover = cursor === 'pointer';

    // Test focus
    await button.focus();
    const outline = await button.evaluate(el =>
      window.getComputedStyle(el).outline
    );
    const hasFocus = outline !== 'none';

    // Check if clickable
    const isDisabled = await button.isDisabled();
    const isClickable = !isDisabled;

    return { hasHover, hasFocus, isClickable };
  }

  /**
   * Verify form validation states
   */
  async verifyFormValidation(formSelector: string): Promise<{
    hasRequiredIndicators: boolean;
    hasInlineErrors: boolean;
  }> {
    const form = this.page.locator(formSelector).first();

    if (!(await form.isVisible({ timeout: 1000 }).catch(() => false))) {
      return { hasRequiredIndicators: false, hasInlineErrors: false };
    }

    const required = await form.locator('[required], .required').count();
    const errors = await form.locator('[class*="error"], .invalid-feedback').count();

    return {
      hasRequiredIndicators: required > 0,
      hasInlineErrors: errors > 0,
    };
  }

  /**
   * Take a UX verification screenshot with metadata
   */
  async takeUXScreenshot(name: string, viewport?: string): Promise<string> {
    const viewportSuffix = viewport ? `-${viewport}` : '';
    const path = `tests/e2e/ui-verification/screenshots/${name}${viewportSuffix}.png`;

    await this.page.screenshot({
      path,
      fullPage: true,
    });

    return path;
  }

  /**
   * Test keyboard navigation through focusable elements
   */
  async testKeyboardNavigation(maxTabs: number = 10): Promise<number> {
    let tabCount = 0;
    let lastFocused = '';

    for (let i = 0; i < maxTabs; i++) {
      await this.page.keyboard.press('Tab');
      const focused = this.page.locator(':focus');

      if (await focused.count() === 0) break;

      const currentFocused = await focused.first()
        .evaluate(el => el.tagName + (el as HTMLElement).className);

      if (currentFocused === lastFocused) break;

      lastFocused = currentFocused;
      tabCount++;
    }

    return tabCount;
  }

  /**
   * Perform a full UX audit on the current page
   */
  async performUXAudit(pageName: string): Promise<{
    loading: boolean;
    error: { hasError: boolean; message: string | null };
    empty: { isEmpty: boolean; hasHelpText: boolean };
    overflow: boolean;
    keyboardNavCount: number;
  }> {
    const loading = await this.verifyLoadingState();
    const error = await this.verifyErrorState();
    const empty = await this.verifyEmptyState();
    const overflow = await this.checkHorizontalOverflow();
    const keyboardNavCount = await this.testKeyboardNavigation();

    return {
      loading,
      error,
      empty,
      overflow,
      keyboardNavCount,
    };
  }
}
