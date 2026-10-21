const { test, expect } = require('@playwright/test');

/**
 * Integration Settings Testing Suite
 * Tests external service integrations and API configurations
 */
test.describe('Integration Settings Tests', () => {
  let settingsPage;

  test.beforeEach(async ({ page }) => {
    settingsPage = page;
    await page.goto('/settings');
    await page.waitForSelector('[data-testid="settings-dashboard"]', {
      timeout: 10000,
    });
  });

  test('should display integrations settings panel', async () => {
    // Navigate to integrations settings tab
    await settingsPage.click('button:has-text("Integrations")');

    // Verify integrations panel is visible
    await expect(
      settingsPage.locator('h2:has-text("Integrations")')
    ).toBeVisible();

    // Check for integration categories
    const integrationCategories = [
      'Weather Services',
      'IoT Platforms',
      'Cross-Chain Bridges',
      'Storage Networks',
      'AI/ML Services',
    ];

    for (const category of integrationCategories) {
      await expect(settingsPage.locator(`text*=${category}`)).toBeVisible();
    }
  });

  test('should configure weather service integrations', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test weather service integrations
    const weatherServices = [
      'OpenWeatherMap',
      'WeatherAPI',
      'NOAA',
      'AccuWeather',
    ];

    for (const service of weatherServices) {
      const serviceToggle = settingsPage.locator(
        `input[type="checkbox"]:near(text="${service}")`
      );
      if (await serviceToggle.isVisible()) {
        // Enable integration
        if (!(await serviceToggle.isChecked())) {
          await serviceToggle.click();
        }
        expect(await serviceToggle.isChecked()).toBeTruthy();

        // Test API key configuration
        const apiKeyInput = settingsPage.locator(
          `input[placeholder*="API Key"]:near(text="${service}")`
        );
        if (await apiKeyInput.isVisible()) {
          await apiKeyInput.fill('test-api-key-12345');
          await expect(apiKeyInput).toHaveValue('test-api-key-12345');
        }
      }
    }
  });

  test('should configure IoT platform integrations', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test IoT platform integrations
    const iotPlatforms = [
      'Arduino Cloud',
      'Raspberry Pi',
      'IoTeX',
      'The Things Network',
    ];

    for (const platform of iotPlatforms) {
      const platformToggle = settingsPage.locator(
        `input[type="checkbox"]:near(text="${platform}")`
      );
      if (await platformToggle.isVisible()) {
        await platformToggle.click();
        expect(await platformToggle.isChecked()).toBeTruthy();

        // Test device ID configuration
        const deviceIdInput = settingsPage.locator(
          `input[placeholder*="Device ID"]:near(text="${platform}")`
        );
        if (await deviceIdInput.isVisible()) {
          await deviceIdInput.fill('device-001');
          await expect(deviceIdInput).toHaveValue('device-001');
        }
      }
    }
  });

  test('should configure cross-chain bridge integrations', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test cross-chain bridge integrations
    const bridgeServices = [
      'LayerZero',
      'Axelar',
      'Wormhole',
      'Chainlink CCIP',
    ];

    for (const bridge of bridgeServices) {
      const bridgeToggle = settingsPage.locator(
        `input[type="checkbox"]:near(text="${bridge}")`
      );
      if (await bridgeToggle.isVisible()) {
        await bridgeToggle.click();
        expect(await bridgeToggle.isChecked()).toBeTruthy();

        // Test bridge configuration
        const configButton = settingsPage.locator(
          `button:has-text("Configure"):near(text="${bridge}")`
        );
        if (await configButton.isVisible()) {
          await configButton.click();

          // Should open configuration modal/panel
          await expect(
            settingsPage.locator('text*=Configuration')
          ).toBeVisible();
        }
      }
    }
  });

  test('should configure storage network integrations', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test storage network integrations
    const storageNetworks = ['IPFS', 'Filecoin', 'Arweave', 'Storj'];

    for (const network of storageNetworks) {
      const networkToggle = settingsPage.locator(
        `input[type="checkbox"]:near(text="${network}")`
      );
      if (await networkToggle.isVisible()) {
        await networkToggle.click();
        expect(await networkToggle.isChecked()).toBeTruthy();

        // Test network endpoint configuration
        const endpointInput = settingsPage.locator(
          `input[placeholder*="Endpoint"]:near(text="${network}")`
        );
        if (await endpointInput.isVisible()) {
          await endpointInput.fill(
            `https://${network.toLowerCase()}.example.com`
          );
          await expect(endpointInput).toHaveValue(
            `https://${network.toLowerCase()}.example.com`
          );
        }
      }
    }
  });

  test('should configure AI/ML service integrations', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test AI/ML service integrations
    const aiServices = ['OpenAI', 'Anthropic', 'Hugging Face', 'Google AI'];

    for (const service of aiServices) {
      const serviceToggle = settingsPage.locator(
        `input[type="checkbox"]:near(text="${service}")`
      );
      if (await serviceToggle.isVisible()) {
        await serviceToggle.click();
        expect(await serviceToggle.isChecked()).toBeTruthy();

        // Test model configuration
        const modelSelect = settingsPage.locator(
          `select:near(text="${service}")`
        );
        if (await modelSelect.isVisible()) {
          await modelSelect.selectOption('gpt-4');
          expect(await modelSelect.inputValue()).toBe('gpt-4');
        }
      }
    }
  });

  test('should test integration status and health checks', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test integration health check
    const healthCheckButton = settingsPage.locator(
      'button:has-text("Check All Integrations")'
    );
    if (await healthCheckButton.isVisible()) {
      await healthCheckButton.click();

      // Wait for health check results
      await expect(settingsPage.locator('text*=Health Check')).toBeVisible({
        timeout: 15000,
      });

      // Should show status indicators
      const statusIndicators = [
        'Connected',
        'Disconnected',
        'Healthy',
        'Error',
        'Warning',
      ];

      let statusFound = false;
      for (const status of statusIndicators) {
        const statusElement = settingsPage.locator(`text*=${status}`);
        if ((await statusElement.count()) > 0) {
          await expect(statusElement.first()).toBeVisible();
          statusFound = true;
          break;
        }
      }
      expect(statusFound).toBeTruthy();
    }
  });

  test('should manage integration credentials securely', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test secure credential management
    const apiKeyInput = settingsPage.locator(
      'input[type="password"], input[placeholder*="API Key"]'
    );
    if (await apiKeyInput.isVisible()) {
      await apiKeyInput.fill('sensitive-api-key-12345');

      // API key should be masked
      const inputType = await apiKeyInput.getAttribute('type');
      expect(inputType).toBe('password');

      // Test show/hide password functionality
      const showButton = settingsPage.locator(
        'button:near(input[type="password"]):has-text("Show")'
      );
      if (await showButton.isVisible()) {
        await showButton.click();

        const inputTypeAfterShow = await apiKeyInput.getAttribute('type');
        expect(inputTypeAfterShow).toBe('text');
      }
    }
  });

  test('should validate integration configurations', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test invalid API key
    const apiKeyInput = settingsPage.locator('input[placeholder*="API Key"]');
    if (await apiKeyInput.isVisible()) {
      await apiKeyInput.fill('invalid-key');

      const validateButton = settingsPage.locator(
        'button:has-text("Validate")'
      );
      if (await validateButton.isVisible()) {
        await validateButton.click();

        // Should show validation error
        await expect(settingsPage.locator('text*=invalid')).toBeVisible({
          timeout: 10000,
        });
      }
    }

    // Test valid configuration
    if (await apiKeyInput.isVisible()) {
      await apiKeyInput.clear();
      await apiKeyInput.fill('valid-api-key-12345');

      const validateButton = settingsPage.locator(
        'button:has-text("Validate")'
      );
      if (await validateButton.isVisible()) {
        await validateButton.click();

        // Should show validation success
        await expect(settingsPage.locator('text*=valid')).toBeVisible({
          timeout: 10000,
        });
      }
    }
  });

  test('should manage integration rate limits', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test rate limit configuration
    const rateLimitInput = settingsPage.locator(
      'input[placeholder*="Rate Limit"], input[name*="rateLimit"]'
    );
    if (await rateLimitInput.isVisible()) {
      await rateLimitInput.clear();
      await rateLimitInput.fill('100');
      await expect(rateLimitInput).toHaveValue('100');
    }

    // Test rate limit period
    const periodSelect = settingsPage.locator('select:near(text="Rate Limit")');
    if (await periodSelect.isVisible()) {
      await periodSelect.selectOption('minute');
      expect(await periodSelect.inputValue()).toBe('minute');
    }
  });

  test('should export and import integration configurations', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Test export configuration
    const exportButton = settingsPage.locator(
      'button:has-text("Export Config")'
    );
    if (await exportButton.isVisible()) {
      const downloadPromise = settingsPage.waitForEvent('download');
      await exportButton.click();
      const download = await downloadPromise;
      expect(download.suggestedFilename()).toContain('integrations');
    }

    // Test import configuration
    const importButton = settingsPage.locator(
      'button:has-text("Import Config")'
    );
    if (await importButton.isVisible()) {
      await importButton.click();

      // Should show file upload dialog
      await expect(settingsPage.locator('input[type="file"]')).toBeVisible();
    }
  });

  test('should save and persist integration settings', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Configure an integration
    const weatherToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="OpenWeatherMap")'
    );
    if (await weatherToggle.isVisible()) {
      await weatherToggle.click();
      expect(await weatherToggle.isChecked()).toBeTruthy();
    }

    // Save settings
    const saveButton = settingsPage.locator(
      'button:has-text("Save Integrations")'
    );
    if (await saveButton.isVisible()) {
      await saveButton.click();

      // Wait for save confirmation
      await expect(settingsPage.locator('text*=saved')).toBeVisible({
        timeout: 5000,
      });
    }

    // Refresh page and verify persistence
    await settingsPage.reload();
    await settingsPage.waitForSelector('[data-testid="settings-dashboard"]', {
      timeout: 10000,
    });
    await settingsPage.click('button:has-text("Integrations")');

    if (await weatherToggle.isVisible()) {
      expect(await weatherToggle.isChecked()).toBeTruthy();
    }
  });

  test('should display integration usage statistics', async () => {
    await settingsPage.click('button:has-text("Integrations")');

    // Check for usage statistics
    const usageElements = [
      'API Calls',
      'Rate Limit',
      'Last Updated',
      'Data Transferred',
      'Uptime',
    ];

    for (const element of usageElements) {
      const usageElement = settingsPage.locator(`text*=${element}`);
      if ((await usageElement.count()) > 0) {
        await expect(usageElement.first()).toBeVisible();
      }
    }
  });
});
