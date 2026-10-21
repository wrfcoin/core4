const { test, expect } = require('@playwright/test');
const { TestHelpers } = require('../utils/test-helpers');

test.describe('User Onboarding Workflow', () => {
  let helpers;

  test.beforeEach(async ({ page }) => {
    helpers = new TestHelpers(page);
    await helpers.navigateTo('/');
  });

  test.afterEach(async () => {
    await helpers.cleanup();
  });

  test('Complete user registration and account setup', async ({ page }) => {
    // Step 1: Navigate to landing page
    await expect(page.locator('[data-testid="landing-hero"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="get-started-button"]')
    ).toBeVisible();

    // Step 2: Click get started
    await page.click('[data-testid="get-started-button"]');
    await page.waitForURL('**/register');

    // Step 3: Fill registration form
    const userData = {
      firstName: 'John',
      lastName: 'Doe',
      email: helpers.generateRandomData('email'),
      password: 'TestPass123!',
      confirmPassword: 'TestPass123!',
      agreeToTerms: true,
    };

    await helpers.fillForm({
      'first-name': userData.firstName,
      'last-name': userData.lastName,
      email: userData.email,
      password: userData.password,
      'confirm-password': userData.confirmPassword,
    });

    await page.check('[data-testid="agree-terms-checkbox"]');
    await page.click('[data-testid="register-button"]');

    // Step 4: Verify email verification prompt
    await page.waitForURL('**/verify-email');
    await expect(
      page.locator('[data-testid="verification-message"]')
    ).toContainText(userData.email);

    // Step 5: Simulate email verification (in real app this would be via email link)
    const verificationCode = '123456'; // Mock verification code
    await page.fill(
      '[data-testid="verification-code-input"]',
      verificationCode
    );
    await page.click('[data-testid="verify-email-button"]');

    // Step 6: Should redirect to wallet setup
    await page.waitForURL('**/wallet-setup');
    await expect(
      page.locator('[data-testid="wallet-setup-title"]')
    ).toBeVisible();

    // Step 7: Create new wallet
    await page.click('[data-testid="create-new-wallet-button"]');
    await page.waitForSelector('[data-testid="seed-phrase-display"]');

    // Step 8: Backup seed phrase
    const seedPhrase = await page.textContent(
      '[data-testid="seed-phrase-display"]'
    );
    expect(seedPhrase).toBeTruthy();
    expect(seedPhrase.split(' ')).toHaveLength(12);

    await page.check('[data-testid="seed-phrase-backed-up-checkbox"]');
    await page.click('[data-testid="continue-button"]');

    // Step 9: Confirm seed phrase
    const seedWords = seedPhrase.split(' ');
    const randomIndices = [2, 5, 8]; // Mock random word verification

    for (const index of randomIndices) {
      await page.fill(
        `[data-testid="seed-word-${index + 1}"]`,
        seedWords[index]
      );
    }

    await page.click('[data-testid="confirm-seed-phrase-button"]');

    // Step 10: Set wallet password
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.fill(
      '[data-testid="confirm-wallet-password-input"]',
      'WalletPass123!'
    );
    await page.click('[data-testid="set-wallet-password-button"]');

    // Step 11: Complete KYC (basic verification)
    await page.waitForURL('**/kyc-verification');
    await expect(page.locator('[data-testid="kyc-title"]')).toBeVisible();

    await helpers.fillForm({
      'date-of-birth': '1990-01-01',
      country: 'United States',
      address: '123 Main St',
      city: 'New York',
      'postal-code': '10001',
    });

    // Upload mock ID document
    await helpers.uploadFile(
      '[data-testid="id-document-upload"]',
      './fixtures/mock-id.jpg'
    );
    await page.click('[data-testid="submit-kyc-button"]');

    // Step 12: KYC submitted confirmation
    await expect(
      page.locator('[data-testid="kyc-submitted-message"]')
    ).toBeVisible();
    await page.click('[data-testid="continue-to-dashboard-button"]');

    // Step 13: First-time dashboard experience
    await page.waitForURL('**/dashboard');
    await expect(
      page.locator('[data-testid="welcome-tour-modal"]')
    ).toBeVisible();

    // Step 14: Complete welcome tour
    const tourSteps = [
      'weather-data',
      'nft-marketplace',
      'governance',
      'cross-chain',
    ];

    for (const step of tourSteps) {
      await expect(page.locator(`[data-testid="tour-${step}"]`)).toBeVisible();
      await page.click('[data-testid="tour-next-button"]');
    }

    await page.click('[data-testid="tour-finish-button"]');

    // Step 15: Verify dashboard is fully loaded
    await expect(page.locator('[data-testid="wallet-balance"]')).toBeVisible();
    await expect(
      page.locator('[data-testid="weather-stations-widget"]')
    ).toBeVisible();
    await expect(
      page.locator('[data-testid="recent-activities"]')
    ).toBeVisible();

    // Step 16: Connect weather station (optional onboarding step)
    await page.click('[data-testid="connect-weather-station-button"]');

    const modal = await helpers.waitForModal();
    await expect(
      page.locator('[data-testid="station-types-list"]')
    ).toBeVisible();

    await page.click('[data-testid="station-type-ambient-weather"]');
    await helpers.fillForm({
      'station-name': 'My Weather Station',
      'api-key': 'test-api-key-12345',
      'application-key': 'test-app-key-67890',
    });

    await page.click('[data-testid="connect-station-button"]');

    // Verify connection success
    await helpers.waitForNotification('success');
    await modal.close();

    // Final verification: User onboarding complete
    await expect(
      page.locator('[data-testid="onboarding-complete-badge"]')
    ).toBeVisible();

    // Verify user can access all main features
    const mainNavItems = ['wallet', 'nft', 'governance', 'bridge'];
    for (const item of mainNavItems) {
      await expect(page.locator(`[data-testid="nav-${item}"]`)).toBeVisible();
    }
  });

  test('Handle registration with existing email', async ({ page }) => {
    // Try to register with email that already exists
    await page.click('[data-testid="get-started-button"]');
    await page.waitForURL('**/register');

    await helpers.fillForm({
      'first-name': 'Jane',
      'last-name': 'Smith',
      email: 'existing@wrfcoin.test', // Email that already exists
      password: 'TestPass123!',
      'confirm-password': 'TestPass123!',
    });

    await page.check('[data-testid="agree-terms-checkbox"]');
    await page.click('[data-testid="register-button"]');

    // Should show error message
    await helpers.waitForNotification('error');
    const errorMessage = await page.textContent(
      '[data-testid="registration-error"]'
    );
    expect(errorMessage).toContain('email already exists');
  });

  test('Validate password strength requirements', async ({ page }) => {
    await page.click('[data-testid="get-started-button"]');
    await page.waitForURL('**/register');

    // Test weak password
    await page.fill('[data-testid="password-input"]', 'weak');
    await expect(
      page.locator('[data-testid="password-strength-weak"]')
    ).toBeVisible();

    // Test medium password
    await page.fill('[data-testid="password-input"]', 'MediumPass1');
    await expect(
      page.locator('[data-testid="password-strength-medium"]')
    ).toBeVisible();

    // Test strong password
    await page.fill('[data-testid="password-input"]', 'StrongPass123!');
    await expect(
      page.locator('[data-testid="password-strength-strong"]')
    ).toBeVisible();
  });

  test('Import existing wallet during onboarding', async ({ page }) => {
    // Complete registration steps (abbreviated)
    await page.click('[data-testid="get-started-button"]');
    await helpers.fillForm({
      'first-name': 'Import',
      'last-name': 'User',
      email: helpers.generateRandomData('email'),
      password: 'TestPass123!',
      'confirm-password': 'TestPass123!',
    });
    await page.check('[data-testid="agree-terms-checkbox"]');
    await page.click('[data-testid="register-button"]');

    // Skip email verification for test
    await page.goto('/wallet-setup');

    // Choose import wallet option
    await page.click('[data-testid="import-existing-wallet-button"]');

    // Test import with seed phrase
    const testSeedPhrase =
      'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
    await page.fill('[data-testid="seed-phrase-input"]', testSeedPhrase);
    await page.fill('[data-testid="wallet-password-input"]', 'WalletPass123!');
    await page.fill(
      '[data-testid="confirm-wallet-password-input"]',
      'WalletPass123!'
    );

    await page.click('[data-testid="import-wallet-button"]');

    // Should show success and continue to dashboard
    await helpers.waitForNotification('success');
    await page.waitForURL('**/dashboard');
  });

  test('Onboarding accessibility compliance', async ({ page }) => {
    // Check accessibility throughout onboarding flow
    await page.click('[data-testid="get-started-button"]');

    // Check registration page accessibility
    const registrationViolations = await helpers.checkAccessibility();
    expect(registrationViolations).toHaveLength(0);

    // Complete registration and check wallet setup accessibility
    await helpers.fillForm({
      'first-name': 'Access',
      'last-name': 'Test',
      email: helpers.generateRandomData('email'),
      password: 'TestPass123!',
      'confirm-password': 'TestPass123!',
    });
    await page.check('[data-testid="agree-terms-checkbox"]');
    await page.click('[data-testid="register-button"]');

    await page.goto('/wallet-setup'); // Skip email verification for test

    const walletSetupViolations = await helpers.checkAccessibility();
    expect(walletSetupViolations).toHaveLength(0);
  });

  test('Onboarding performance benchmarks', async ({ page }) => {
    // Measure performance throughout onboarding
    await page.click('[data-testid="get-started-button"]');

    const registrationPerf = await helpers.measurePerformance();
    expect(registrationPerf.load).toBeLessThan(3000);
    expect(registrationPerf.firstContentfulPaint).toBeLessThan(1500);

    // Complete quick registration for dashboard performance test
    await helpers.fillForm({
      'first-name': 'Perf',
      'last-name': 'Test',
      email: helpers.generateRandomData('email'),
      password: 'TestPass123!',
      'confirm-password': 'TestPass123!',
    });
    await page.check('[data-testid="agree-terms-checkbox"]');
    await page.click('[data-testid="register-button"]');

    await page.goto('/dashboard'); // Skip intermediate steps for performance test

    const dashboardPerf = await helpers.measurePerformance();
    expect(dashboardPerf.load).toBeLessThan(4000);
    expect(dashboardPerf.firstContentfulPaint).toBeLessThan(2000);
  });
});
