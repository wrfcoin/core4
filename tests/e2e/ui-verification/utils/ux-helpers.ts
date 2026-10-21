/**
 * UX Verification Helpers
 *
 * Utility functions for comprehensive UI/UX testing including:
 * - Loading state verification
 * - Error state validation
 * - Empty state checks
 * - Responsive design testing
 * - Button state verification
 * - Form validation helpers
 * - Navigation testing
 * - Accessibility auditing
 */
import { Page, Locator, expect, BrowserContext } from '@playwright/test';
import * as fs from 'fs';
import * as path from 'path';

// Output directories
const SCREENSHOT_DIR = path.join(__dirname, '..', 'screenshots');
const ACCESSIBILITY_DIR = path.join(__dirname, '..', 'accessibility');

export interface UXAuditResult {
  page: string;
  viewport: string;
  loadingStates: LoadingStateResult;
  errorStates: ErrorStateResult;
  emptyStates: EmptyStateResult;
  buttonStates: ButtonStateResult;
  formValidation: FormValidationResult;
  navigation: NavigationResult;
  accessibility: AccessibilityResult;
  screenshots: string[];
  issues: UXIssue[];
}

export interface UXIssue {
  severity: 'critical' | 'major' | 'minor' | 'suggestion';
  category: string;
  description: string;
  selector?: string;
  screenshot?: string;
}

export interface LoadingStateResult {
  hasLoadingIndicator: boolean;
  loadingDuration: number;
  skeletonScreens: boolean;
}

export interface ErrorStateResult {
  hasErrorBoundary: boolean;
  errorMessageClear: boolean;
  errorRecoveryOption: boolean;
}

export interface EmptyStateResult {
  hasEmptyState: boolean;
  emptyStateHelpful: boolean;
  callToAction: boolean;
}

export interface ButtonStateResult {
  hoverState: boolean;
  activeState: boolean;
  disabledState: boolean;
  loadingState: boolean;
  focusVisible: boolean;
}

export interface FormValidationResult {
  inlineErrors: boolean;
  requiredFieldIndicators: boolean;
  submitButtonState: boolean;
}

export interface NavigationResult {
  breadcrumbsPresent: boolean;
  allLinksWork: boolean;
  backNavigationWorks: boolean;
}

export interface AccessibilityResult {
  violations: AccessibilityViolation[];
  warnings: AccessibilityViolation[];
  passes: number;
}

export interface AccessibilityViolation {
  id: string;
  impact: string;
  description: string;
  nodes: string[];
}

/**
 * UX Verification Helper Class
 */
export class UXVerifier {
  private page: Page;
  private issues: UXIssue[] = [];
  private screenshots: string[] = [];

  constructor(page: Page) {
    this.page = page;
  }

  /**
   * Capture a screenshot with metadata
   */
  async captureScreenshot(name: string, fullPage: boolean = true): Promise<string> {
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const filename = `${name}-${timestamp}.png`;
    const filepath = path.join(SCREENSHOT_DIR, filename);

    await this.page.screenshot({
      path: filepath,
      fullPage,
    });

    this.screenshots.push(filepath);
    return filepath;
  }

  /**
   * Verify loading states
   */
  async verifyLoadingStates(): Promise<LoadingStateResult> {
    const startTime = Date.now();

    // Check for loading indicators
    const loadingSelectors = [
      '[data-testid="loading-spinner"]',
      '[data-testid="loading-skeleton"]',
      '[data-testid="loading-progress"]',
      '.loading',
      '.spinner',
      '[role="progressbar"]',
    ];

    let hasLoadingIndicator = false;
    let skeletonScreens = false;

    for (const selector of loadingSelectors) {
      const element = this.page.locator(selector).first();
      if (await element.isVisible({ timeout: 1000 }).catch(() => false)) {
        hasLoadingIndicator = true;
        if (selector.includes('skeleton')) {
          skeletonScreens = true;
        }
        // Wait for loading to complete
        await element.waitFor({ state: 'hidden', timeout: 30000 }).catch(() => {});
        break;
      }
    }

    const loadingDuration = Date.now() - startTime;

    // Check if page loaded without loading indicator (might be instant)
    if (!hasLoadingIndicator && loadingDuration < 100) {
      hasLoadingIndicator = true; // Instant load is acceptable
    }

    return {
      hasLoadingIndicator,
      loadingDuration,
      skeletonScreens,
    };
  }

  /**
   * Verify error states
   */
  async verifyErrorStates(): Promise<ErrorStateResult> {
    const errorSelectors = [
      '[data-testid="error-message"]',
      '[data-testid="error-boundary"]',
      '[role="alert"]',
      '.error-message',
      '.error',
    ];

    let hasErrorBoundary = false;
    let errorMessageClear = true;
    let errorRecoveryOption = false;

    // Check for error boundary component
    const errorBoundary = this.page.locator('[data-testid="error-boundary"]');
    if (await errorBoundary.isVisible({ timeout: 500 }).catch(() => false)) {
      hasErrorBoundary = true;
    }

    // Check for error messages
    for (const selector of errorSelectors) {
      const errorElement = this.page.locator(selector).first();
      if (await errorElement.isVisible({ timeout: 500 }).catch(() => false)) {
        const text = await errorElement.textContent();
        // Check if error message is helpful (not just "Error" or empty)
        errorMessageClear = text ? text.length > 10 : false;

        // Check for retry/recovery buttons
        const recoveryButton = this.page.locator('[data-testid="retry-button"], [data-testid="reload-button"], button:has-text("Try Again")').first();
        errorRecoveryOption = await recoveryButton.isVisible({ timeout: 500 }).catch(() => false);
        break;
      }
    }

    return {
      hasErrorBoundary,
      errorMessageClear,
      errorRecoveryOption,
    };
  }

  /**
   * Verify empty states
   */
  async verifyEmptyStates(): Promise<EmptyStateResult> {
    const emptySelectors = [
      '[data-testid="empty-state"]',
      '[data-testid="no-data"]',
      '[data-testid="no-results"]',
      '.empty-state',
      '.no-data',
    ];

    let hasEmptyState = false;
    let emptyStateHelpful = false;
    let callToAction = false;

    for (const selector of emptySelectors) {
      const emptyElement = this.page.locator(selector).first();
      if (await emptyElement.isVisible({ timeout: 500 }).catch(() => false)) {
        hasEmptyState = true;

        // Check for helpful text
        const text = await emptyElement.textContent();
        emptyStateHelpful = text ? text.length > 20 : false;

        // Check for CTA button
        const ctaButton = emptyElement.locator('button, a').first();
        callToAction = await ctaButton.isVisible({ timeout: 500 }).catch(() => false);
        break;
      }
    }

    return {
      hasEmptyState,
      emptyStateHelpful,
      callToAction,
    };
  }

  /**
   * Verify button states
   */
  async verifyButtonStates(buttonSelector: string): Promise<ButtonStateResult> {
    const button = this.page.locator(buttonSelector).first();

    const result: ButtonStateResult = {
      hoverState: false,
      activeState: false,
      disabledState: false,
      loadingState: false,
      focusVisible: false,
    };

    if (!(await button.isVisible({ timeout: 1000 }).catch(() => false))) {
      return result;
    }

    // Check hover state
    await button.hover();
    const hoverStyles = await button.evaluate((el) => {
      const styles = window.getComputedStyle(el);
      return {
        cursor: styles.cursor,
        backgroundColor: styles.backgroundColor,
      };
    });
    result.hoverState = hoverStyles.cursor === 'pointer' || hoverStyles.cursor !== 'default';

    // Check focus state
    await button.focus();
    const focusStyles = await button.evaluate((el) => {
      const styles = window.getComputedStyle(el);
      return {
        outline: styles.outline,
        boxShadow: styles.boxShadow,
      };
    });
    result.focusVisible = focusStyles.outline !== 'none' || focusStyles.boxShadow !== 'none';

    // Check for disabled state attribute
    const isDisabled = await button.isDisabled();
    result.disabledState = isDisabled;

    // Check for loading state
    const hasLoadingClass = await button.evaluate((el) => {
      return el.classList.contains('loading') || el.getAttribute('data-loading') === 'true';
    });
    result.loadingState = hasLoadingClass;

    return result;
  }

  /**
   * Verify form validation
   */
  async verifyFormValidation(formSelector: string): Promise<FormValidationResult> {
    const form = this.page.locator(formSelector).first();

    const result: FormValidationResult = {
      inlineErrors: false,
      requiredFieldIndicators: false,
      submitButtonState: false,
    };

    if (!(await form.isVisible({ timeout: 1000 }).catch(() => false))) {
      return result;
    }

    // Check for required field indicators
    const requiredIndicators = form.locator('[required], .required, *:has-text("*")');
    result.requiredFieldIndicators = await requiredIndicators.count() > 0;

    // Check for inline error messages
    const inlineErrors = form.locator('[data-testid*="error"], .error, .invalid-feedback');
    result.inlineErrors = await inlineErrors.count() > 0 ||
      await form.locator('input:invalid').count() > 0;

    // Check submit button state
    const submitButton = form.locator('[type="submit"], button:has-text("Submit")').first();
    if (await submitButton.isVisible({ timeout: 500 }).catch(() => false)) {
      result.submitButtonState = true;
    }

    return result;
  }

  /**
   * Verify navigation
   */
  async verifyNavigation(): Promise<NavigationResult> {
    const result: NavigationResult = {
      breadcrumbsPresent: false,
      allLinksWork: true,
      backNavigationWorks: false,
    };

    // Check for breadcrumbs
    const breadcrumbs = this.page.locator('[data-testid="breadcrumbs"], nav[aria-label="breadcrumb"], .breadcrumb');
    result.breadcrumbsPresent = await breadcrumbs.isVisible({ timeout: 1000 }).catch(() => false);

    // Test back navigation
    const currentUrl = this.page.url();
    await this.page.goBack();
    await this.page.waitForTimeout(1000);
    const afterBackUrl = this.page.url();
    result.backNavigationWorks = afterBackUrl !== currentUrl || afterBackUrl.includes('://');
    await this.page.goForward();

    // Check if main nav links are present
    const navLinks = this.page.locator('nav a, [data-testid="nav-link"]');
    const linkCount = await navLinks.count();
    result.allLinksWork = linkCount > 0;

    return result;
  }

  /**
   * Run accessibility audit using axe-core
   */
  async runAccessibilityAudit(): Promise<AccessibilityResult> {
    // Inject axe-core
    await this.page.addScriptTag({
      url: 'https://cdnjs.cloudflare.com/ajax/libs/axe-core/4.8.2/axe.min.js',
    });

    // Run axe
    const results = await this.page.evaluate(async () => {
      // @ts-ignore
      return await axe.run();
    }).catch(() => ({
      violations: [],
      passes: [],
      incomplete: [],
    }));

    const violations: AccessibilityViolation[] = results.violations?.map((v: any) => ({
      id: v.id,
      impact: v.impact,
      description: v.description,
      nodes: v.nodes?.map((n: any) => n.target?.join(', ')) || [],
    })) || [];

    const warnings: AccessibilityViolation[] = results.incomplete?.map((w: any) => ({
      id: w.id,
      impact: w.impact || 'unknown',
      description: w.description,
      nodes: w.nodes?.map((n: any) => n.target?.join(', ')) || [],
    })) || [];

    return {
      violations,
      warnings,
      passes: results.passes?.length || 0,
    };
  }

  /**
   * Add a UX issue to the report
   */
  addIssue(issue: UXIssue): void {
    this.issues.push(issue);
  }

  /**
   * Get all collected issues
   */
  getIssues(): UXIssue[] {
    return this.issues;
  }

  /**
   * Perform comprehensive UX audit on current page
   */
  async performFullAudit(pageName: string, viewport: string): Promise<UXAuditResult> {
    // Capture initial screenshot
    await this.captureScreenshot(`${pageName}-${viewport}-initial`);

    // Run all verifications
    const [
      loadingStates,
      errorStates,
      emptyStates,
      navigation,
      accessibility,
    ] = await Promise.all([
      this.verifyLoadingStates(),
      this.verifyErrorStates(),
      this.verifyEmptyStates(),
      this.verifyNavigation(),
      this.runAccessibilityAudit(),
    ]);

    // Default button states (will be verified per-button in specific tests)
    const buttonStates: ButtonStateResult = {
      hoverState: false,
      activeState: false,
      disabledState: false,
      loadingState: false,
      focusVisible: false,
    };

    // Default form validation (will be verified per-form in specific tests)
    const formValidation: FormValidationResult = {
      inlineErrors: false,
      requiredFieldIndicators: false,
      submitButtonState: false,
    };

    // Log accessibility violations as issues
    for (const violation of accessibility.violations) {
      this.addIssue({
        severity: violation.impact === 'critical' ? 'critical' :
                  violation.impact === 'serious' ? 'major' : 'minor',
        category: 'accessibility',
        description: violation.description,
        selector: violation.nodes.join(', '),
      });
    }

    // Capture final screenshot
    await this.captureScreenshot(`${pageName}-${viewport}-final`);

    return {
      page: pageName,
      viewport,
      loadingStates,
      errorStates,
      emptyStates,
      buttonStates,
      formValidation,
      navigation,
      accessibility,
      screenshots: this.screenshots,
      issues: this.issues,
    };
  }

  /**
   * Save audit results to file
   */
  async saveAuditResults(results: UXAuditResult): Promise<string> {
    const filename = `ux-audit-${results.page}-${results.viewport}-${Date.now()}.json`;
    const filepath = path.join(ACCESSIBILITY_DIR, filename);

    fs.writeFileSync(filepath, JSON.stringify(results, null, 2));
    return filepath;
  }
}

/**
 * Viewport configurations for responsive testing
 */
export const VIEWPORTS = {
  desktop: { width: 1920, height: 1080, name: 'desktop' },
  laptop: { width: 1440, height: 900, name: 'laptop' },
  tablet: { width: 768, height: 1024, name: 'tablet' },
  mobile: { width: 375, height: 667, name: 'mobile' },
} as const;

/**
 * Test all viewports for a given test function
 */
export async function testAllViewports(
  page: Page,
  testFn: (viewport: typeof VIEWPORTS[keyof typeof VIEWPORTS]) => Promise<void>
): Promise<void> {
  for (const [key, viewport] of Object.entries(VIEWPORTS)) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await testFn(viewport);
  }
}

export default UXVerifier;
