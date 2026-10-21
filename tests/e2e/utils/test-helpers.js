const { expect } = require('@playwright/test');
const config = require('../config/test-config');
const testData = require('../fixtures/test-data.json');

class TestHelpers {
  constructor(page) {
    this.page = page;
    this.config = config;
    this.testData = testData;
  }

  /**
   * Wait for page to load completely
   */
  async waitForPageLoad() {
    await this.page.waitForLoadState('networkidle');
    await this.page.waitForLoadState('domcontentloaded');
  }

  /**
   * Navigate to a specific page
   */
  async navigateTo(path) {
    const url = `${this.config.baseUrls.frontend}${path}`;
    await this.page.goto(url);
    await this.waitForPageLoad();
  }

  /**
   * Login with test user credentials
   */
  async loginAs(userType = 'user1') {
    const user = this.testData.users[userType];
    if (!user) throw new Error(`User type ${userType} not found in test data`);

    await this.navigateTo('/login');
    await this.page.fill('[data-testid="email-input"]', user.email);
    await this.page.fill('[data-testid="password-input"]', user.password);
    await this.page.click('[data-testid="login-button"]');

    // Wait for successful login redirect
    await this.page.waitForURL('**/dashboard');
    await this.waitForPageLoad();
  }

  /**
   * Connect wallet for testing
   */
  async connectWallet(userType = 'user1') {
    const user = this.testData.users[userType];

    // Click connect wallet button
    await this.page.click('[data-testid="connect-wallet-button"]');

    // Select test wallet option
    await this.page.click('[data-testid="test-wallet-option"]');

    // Enter test private key
    await this.page.fill('[data-testid="private-key-input"]', user.privateKey);
    await this.page.click('[data-testid="import-wallet-button"]');

    // Wait for wallet connection
    await this.page.waitForSelector('[data-testid="wallet-connected"]');
  }

  /**
   * Wait for blockchain transaction to complete
   */
  async waitForTransaction(txHash, timeout = config.timeouts.blockchain) {
    const startTime = Date.now();

    while (Date.now() - startTime < timeout) {
      try {
        // Check transaction status via API
        const response = await this.page.request.get(
          `${config.baseUrls.backend}/api/transaction/${txHash}/status`
        );
        const result = await response.json();

        if (result.status === 'confirmed') {
          return result;
        }

        await this.page.waitForTimeout(2000);
      } catch (error) {
        console.warn(`Error checking transaction status: ${error.message}`);
        await this.page.waitForTimeout(2000);
      }
    }

    throw new Error(
      `Transaction ${txHash} did not confirm within ${timeout}ms`
    );
  }

  /**
   * Fill form fields with test data
   */
  async fillForm(formData) {
    for (const [field, value] of Object.entries(formData)) {
      const selector = `[data-testid="${field}-input"]`;
      await this.page.fill(selector, value.toString());
    }
  }

  /**
   * Wait for notification to appear
   */
  async waitForNotification(type = 'success', timeout = 10000) {
    const selector = `[data-testid="notification-${type}"]`;
    await this.page.waitForSelector(selector, { timeout });

    // Get notification text
    const notificationText = await this.page.textContent(selector);
    return notificationText;
  }

  /**
   * Wait for modal to appear and return modal instance
   */
  async waitForModal(timeout = 5000) {
    await this.page.waitForSelector('[data-testid="modal"]', { timeout });
    return new ModalHelper(this.page);
  }

  /**
   * Upload file via file input
   */
  async uploadFile(selector, filePath) {
    const fileInput = this.page.locator(selector);
    await fileInput.setInputFiles(filePath);
  }

  /**
   * Take screenshot with timestamp
   */
  async takeScreenshot(name) {
    const timestamp = Date.now();
    const fileName = `${name}-${timestamp}.png`;
    await this.page.screenshot({
      path: `test/e2e/reports/screenshots/${fileName}`,
      fullPage: true,
    });
    return fileName;
  }

  /**
   * Assert API response
   */
  async assertApiResponse(url, expectedStatus = 200, expectedData = null) {
    const response = await this.page.request.get(url);
    expect(response.status()).toBe(expectedStatus);

    if (expectedData) {
      const responseData = await response.json();
      expect(responseData).toMatchObject(expectedData);
    }

    return response;
  }

  /**
   * Wait for element to be visible and interactable
   */
  async waitForElement(selector, options = {}) {
    await this.page.waitForSelector(selector, {
      state: 'visible',
      timeout: 10000,
      ...options,
    });
    return this.page.locator(selector);
  }

  /**
   * Simulate API response for testing
   */
  async mockApiResponse(url, responseData) {
    await this.page.route(url, route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(responseData),
      });
    });
  }

  /**
   * Generate random test data
   */
  generateRandomData(type) {
    const generators = {
      email: () => `test-${Date.now()}@wrfcoin.test`,
      address: () => `0x${Math.random().toString(16).substr(2, 40)}`,
      amount: () => (Math.random() * 1000).toFixed(2),
      hash: () => `0x${Math.random().toString(16).substr(2, 64)}`,
      timestamp: () => new Date().toISOString(),
      uuid: () => crypto.randomUUID(),
    };

    return generators[type] ? generators[type]() : null;
  }

  /**
   * Measure page performance
   */
  async measurePerformance() {
    return await this.page.evaluate(() => {
      const navigation = performance.getEntriesByType('navigation')[0];
      return {
        domContentLoaded:
          navigation.domContentLoadedEventEnd -
          navigation.domContentLoadedEventStart,
        load: navigation.loadEventEnd - navigation.loadEventStart,
        firstPaint:
          performance.getEntriesByName('first-paint')[0]?.startTime || 0,
        firstContentfulPaint:
          performance.getEntriesByName('first-contentful-paint')[0]
            ?.startTime || 0,
      };
    });
  }

  /**
   * Check accessibility violations
   */
  async checkAccessibility() {
    // Inject axe-core for accessibility testing
    await this.page.addScriptTag({
      url: 'https://unpkg.com/axe-core@4.7.0/axe.min.js',
    });

    const violations = await this.page.evaluate(() => {
      return axe.run().then(results => results.violations);
    });

    return violations;
  }

  /**
   * Clean up test data after test completion
   */
  async cleanup() {
    // Clear localStorage
    await this.page.evaluate(() => localStorage.clear());

    // Clear sessionStorage
    await this.page.evaluate(() => sessionStorage.clear());

    // Clear cookies
    await this.page.context().clearCookies();
  }
}

class ModalHelper {
  constructor(page) {
    this.page = page;
    this.modal = page.locator('[data-testid="modal"]');
  }

  async getTitle() {
    return await this.modal
      .locator('[data-testid="modal-header"]')
      .textContent();
  }

  async getBody() {
    return await this.modal.locator('[data-testid="modal-body"]').textContent();
  }

  async clickButton(buttonType) {
    await this.modal.locator(`[data-testid="btn-${buttonType}"]`).click();
  }

  async close() {
    await this.modal.locator('[data-testid="btn-close"]').click();
    await this.page.waitForSelector('[data-testid="modal"]', {
      state: 'hidden',
    });
  }

  async waitForClose() {
    await this.page.waitForSelector('[data-testid="modal"]', {
      state: 'hidden',
    });
  }
}

class ApiHelper {
  constructor(page) {
    this.page = page;
    this.baseUrl = config.baseUrls.backend;
  }

  async get(endpoint, headers = {}) {
    return await this.page.request.get(`${this.baseUrl}${endpoint}`, {
      headers: { ...config.api.headers, ...headers },
    });
  }

  async post(endpoint, data, headers = {}) {
    return await this.page.request.post(`${this.baseUrl}${endpoint}`, {
      data,
      headers: { ...config.api.headers, ...headers },
    });
  }

  async put(endpoint, data, headers = {}) {
    return await this.page.request.put(`${this.baseUrl}${endpoint}`, {
      data,
      headers: { ...config.api.headers, ...headers },
    });
  }

  async delete(endpoint, headers = {}) {
    return await this.page.request.delete(`${this.baseUrl}${endpoint}`, {
      headers: { ...config.api.headers, ...headers },
    });
  }
}

module.exports = { TestHelpers, ModalHelper, ApiHelper };
