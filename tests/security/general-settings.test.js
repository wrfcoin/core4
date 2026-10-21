const { test, expect } = require('@playwright/test');

/**
 * General Settings Testing Suite
 * Tests display settings, regional settings, accessibility, and other general configurations
 */
test.describe('General Settings Tests', () => {
  let settingsPage;

  test.beforeEach(async ({ page }) => {
    settingsPage = page;
    await page.goto('/settings');
    await page.waitForSelector('[data-testid="settings-dashboard"]', {
      timeout: 10000,
    });
  });

  test('should display general settings panel', async () => {
    // Navigate to general settings tab
    await settingsPage.click('button:has-text("General Settings")');

    // Verify general settings panel is visible
    await expect(
      settingsPage.locator('h2:has-text("General Settings")')
    ).toBeVisible();

    // Check for settings categories
    const settingsCategories = [
      'Display Settings',
      'Regional Settings',
      'Accessibility',
      'Notifications',
      'Performance',
    ];

    for (const category of settingsCategories) {
      await expect(settingsPage.locator(`text*=${category}`)).toBeVisible();
    }
  });

  test('should configure display settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test theme selection
    const themeOptions = ['Light', 'Dark', 'Auto'];
    for (const theme of themeOptions) {
      const themeOption = settingsPage.locator(`input[value="${theme}"]`);
      if (await themeOption.isVisible()) {
        await themeOption.click();
        expect(await themeOption.isChecked()).toBeTruthy();

        // Verify theme is applied
        const bodyClass = await settingsPage
          .locator('body')
          .getAttribute('class');
        expect(bodyClass).toContain(theme.toLowerCase());
      }
    }

    // Test font size adjustment
    const fontSizeSlider = settingsPage.locator(
      'input[type="range"]:near(text="Font Size")'
    );
    if (await fontSizeSlider.isVisible()) {
      await fontSizeSlider.fill('16');
      expect(await fontSizeSlider.inputValue()).toBe('16');
    }

    // Test UI density
    const densityOptions = ['Compact', 'Normal', 'Comfortable'];
    for (const density of densityOptions) {
      const densityOption = settingsPage.locator(`input[value="${density}"]`);
      if (await densityOption.isVisible()) {
        await densityOption.click();
        expect(await densityOption.isChecked()).toBeTruthy();
      }
    }
  });

  test('should configure regional settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test language selection
    const languageSelect = settingsPage.locator('select:near(text="Language")');
    if (await languageSelect.isVisible()) {
      await languageSelect.selectOption('en-US');
      expect(await languageSelect.inputValue()).toBe('en-US');

      // Test other languages
      const languages = ['es-ES', 'fr-FR', 'de-DE', 'ja-JP'];
      for (const lang of languages) {
        const option = languageSelect.locator(`option[value="${lang}"]`);
        if ((await option.count()) > 0) {
          await languageSelect.selectOption(lang);
          expect(await languageSelect.inputValue()).toBe(lang);
        }
      }
    }

    // Test timezone selection
    const timezoneSelect = settingsPage.locator('select:near(text="Timezone")');
    if (await timezoneSelect.isVisible()) {
      await timezoneSelect.selectOption('America/New_York');
      expect(await timezoneSelect.inputValue()).toBe('America/New_York');
    }

    // Test date format
    const dateFormatSelect = settingsPage.locator(
      'select:near(text="Date Format")'
    );
    if (await dateFormatSelect.isVisible()) {
      const dateFormats = ['MM/DD/YYYY', 'DD/MM/YYYY', 'YYYY-MM-DD'];
      for (const format of dateFormats) {
        const option = dateFormatSelect.locator(`option[value="${format}"]`);
        if ((await option.count()) > 0) {
          await dateFormatSelect.selectOption(format);
          expect(await dateFormatSelect.inputValue()).toBe(format);
        }
      }
    }

    // Test currency settings
    const currencySelect = settingsPage.locator('select:near(text="Currency")');
    if (await currencySelect.isVisible()) {
      await currencySelect.selectOption('USD');
      expect(await currencySelect.inputValue()).toBe('USD');
    }
  });

  test('should configure accessibility settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test high contrast mode
    const highContrastToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="High Contrast")'
    );
    if (await highContrastToggle.isVisible()) {
      await highContrastToggle.click();
      expect(await highContrastToggle.isChecked()).toBeTruthy();

      // Verify high contrast is applied
      const bodyClass = await settingsPage
        .locator('body')
        .getAttribute('class');
      expect(bodyClass).toContain('high-contrast');
    }

    // Test reduced motion
    const reducedMotionToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Reduced Motion")'
    );
    if (await reducedMotionToggle.isVisible()) {
      await reducedMotionToggle.click();
      expect(await reducedMotionToggle.isChecked()).toBeTruthy();
    }

    // Test screen reader support
    const screenReaderToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Screen Reader")'
    );
    if (await screenReaderToggle.isVisible()) {
      await screenReaderToggle.click();
      expect(await screenReaderToggle.isChecked()).toBeTruthy();
    }

    // Test keyboard navigation
    const keyboardNavToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Keyboard Navigation")'
    );
    if (await keyboardNavToggle.isVisible()) {
      await keyboardNavToggle.click();
      expect(await keyboardNavToggle.isChecked()).toBeTruthy();
    }
  });

  test('should configure notification settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test desktop notifications
    const desktopNotificationsToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Desktop Notifications")'
    );
    if (await desktopNotificationsToggle.isVisible()) {
      await desktopNotificationsToggle.click();
      expect(await desktopNotificationsToggle.isChecked()).toBeTruthy();
    }

    // Test email notifications
    const emailNotificationsToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Email Notifications")'
    );
    if (await emailNotificationsToggle.isVisible()) {
      await emailNotificationsToggle.click();
      expect(await emailNotificationsToggle.isChecked()).toBeTruthy();
    }

    // Test notification categories
    const notificationCategories = [
      'Transaction Alerts',
      'Security Alerts',
      'System Updates',
      'Price Alerts',
    ];

    for (const category of notificationCategories) {
      const categoryToggle = settingsPage.locator(
        `input[type="checkbox"]:near(text="${category}")`
      );
      if (await categoryToggle.isVisible()) {
        await categoryToggle.click();
        expect(await categoryToggle.isChecked()).toBeTruthy();
      }
    }

    // Test notification sound
    const soundSelect = settingsPage.locator(
      'select:near(text="Notification Sound")'
    );
    if (await soundSelect.isVisible()) {
      const sounds = ['Default', 'Chime', 'Bell', 'None'];
      for (const sound of sounds) {
        const option = soundSelect.locator(`option[value="${sound}"]`);
        if ((await option.count()) > 0) {
          await soundSelect.selectOption(sound);
          expect(await soundSelect.inputValue()).toBe(sound);
        }
      }
    }
  });

  test('should configure performance settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test animation settings
    const animationsToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Animations")'
    );
    if (await animationsToggle.isVisible()) {
      await animationsToggle.click();
      expect(await animationsToggle.isChecked()).toBeTruthy();
    }

    // Test auto-refresh interval
    const refreshIntervalSelect = settingsPage.locator(
      'select:near(text="Auto Refresh")'
    );
    if (await refreshIntervalSelect.isVisible()) {
      const intervals = ['5', '10', '30', '60'];
      for (const interval of intervals) {
        const option = refreshIntervalSelect.locator(
          `option[value="${interval}"]`
        );
        if ((await option.count()) > 0) {
          await refreshIntervalSelect.selectOption(interval);
          expect(await refreshIntervalSelect.inputValue()).toBe(interval);
        }
      }
    }

    // Test data preloading
    const preloadingToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Preload Data")'
    );
    if (await preloadingToggle.isVisible()) {
      await preloadingToggle.click();
      expect(await preloadingToggle.isChecked()).toBeTruthy();
    }

    // Test cache settings
    const cacheSize = settingsPage.locator(
      'input[type="number"]:near(text="Cache Size")'
    );
    if (await cacheSize.isVisible()) {
      await cacheSize.fill('100');
      expect(await cacheSize.inputValue()).toBe('100');
    }
  });

  test('should configure backup and sync settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test automatic backup
    const autoBackupToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Automatic Backup")'
    );
    if (await autoBackupToggle.isVisible()) {
      await autoBackupToggle.click();
      expect(await autoBackupToggle.isChecked()).toBeTruthy();

      // Test backup frequency
      const backupFrequency = settingsPage.locator(
        'select:near(text="Backup Frequency")'
      );
      if (await backupFrequency.isVisible()) {
        await backupFrequency.selectOption('daily');
        expect(await backupFrequency.inputValue()).toBe('daily');
      }
    }

    // Test cloud sync
    const cloudSyncToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="Cloud Sync")'
    );
    if (await cloudSyncToggle.isVisible()) {
      await cloudSyncToggle.click();
      expect(await cloudSyncToggle.isChecked()).toBeTruthy();
    }
  });

  test('should manage user preferences', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test default page on startup
    const startupPageSelect = settingsPage.locator(
      'select:near(text="Startup Page")'
    );
    if (await startupPageSelect.isVisible()) {
      const pages = ['Dashboard', 'Wallet', 'Trading', 'Settings'];
      for (const page of pages) {
        const option = startupPageSelect.locator(`option[value="${page}"]`);
        if ((await option.count()) > 0) {
          await startupPageSelect.selectOption(page);
          expect(await startupPageSelect.inputValue()).toBe(page);
        }
      }
    }

    // Test confirmation prompts
    const confirmationCategories = [
      'Delete Confirmations',
      'Transaction Confirmations',
      'Logout Confirmations',
    ];

    for (const category of confirmationCategories) {
      const categoryToggle = settingsPage.locator(
        `input[type="checkbox"]:near(text="${category}")`
      );
      if (await categoryToggle.isVisible()) {
        await categoryToggle.click();
        expect(await categoryToggle.isChecked()).toBeTruthy();
      }
    }
  });

  test('should import and export settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test export settings
    const exportButton = settingsPage.locator(
      'button:has-text("Export Settings")'
    );
    if (await exportButton.isVisible()) {
      const downloadPromise = settingsPage.waitForEvent('download');
      await exportButton.click();
      const download = await downloadPromise;
      expect(download.suggestedFilename()).toContain('settings');
    }

    // Test import settings
    const importButton = settingsPage.locator(
      'button:has-text("Import Settings")'
    );
    if (await importButton.isVisible()) {
      await importButton.click();

      // Should show file upload dialog
      await expect(settingsPage.locator('input[type="file"]')).toBeVisible();
    }
  });

  test('should reset settings to defaults', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Change a setting first
    const themeOption = settingsPage.locator('input[value="Dark"]');
    if (await themeOption.isVisible()) {
      await themeOption.click();
    }

    // Test reset to defaults
    const resetButton = settingsPage.locator(
      'button:has-text("Reset to Defaults")'
    );
    if (await resetButton.isVisible()) {
      await resetButton.click();

      // Should show confirmation dialog
      await expect(settingsPage.locator('text*=Are you sure')).toBeVisible();

      // Confirm reset
      await settingsPage.click('button:has-text("Confirm")');

      // Should show reset confirmation
      await expect(settingsPage.locator('text*=reset')).toBeVisible({
        timeout: 5000,
      });

      // Verify theme is back to default (Light)
      const lightThemeOption = settingsPage.locator('input[value="Light"]');
      if (await lightThemeOption.isVisible()) {
        expect(await lightThemeOption.isChecked()).toBeTruthy();
      }
    }
  });

  test('should save and persist general settings', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Configure multiple settings
    const darkThemeOption = settingsPage.locator('input[value="Dark"]');
    if (await darkThemeOption.isVisible()) {
      await darkThemeOption.click();
    }

    const languageSelect = settingsPage.locator('select:near(text="Language")');
    if (await languageSelect.isVisible()) {
      await languageSelect.selectOption('es-ES');
    }

    // Save settings
    const saveButton = settingsPage.locator('button:has-text("Save Settings")');
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
    await settingsPage.click('button:has-text("General Settings")');

    // Verify settings persisted
    if (await darkThemeOption.isVisible()) {
      expect(await darkThemeOption.isChecked()).toBeTruthy();
    }

    if (await languageSelect.isVisible()) {
      expect(await languageSelect.inputValue()).toBe('es-ES');
    }
  });

  test('should validate setting constraints', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Test invalid cache size
    const cacheSize = settingsPage.locator(
      'input[type="number"]:near(text="Cache Size")'
    );
    if (await cacheSize.isVisible()) {
      await cacheSize.fill('-10');

      // Should show validation error
      await expect(settingsPage.locator('text*=invalid')).toBeVisible();

      // Test maximum limit
      await cacheSize.fill('99999');

      // Should show validation warning or auto-correct
      const saveButton = settingsPage.locator('button:has-text("Save")');
      if (await saveButton.isVisible()) {
        await saveButton.click();

        // Should either show error or auto-correct to max value
        const hasError =
          (await settingsPage.locator('text*=too large').count()) > 0;
        const correctedValue = await cacheSize.inputValue();

        expect(hasError || parseInt(correctedValue) <= 1000).toBeTruthy();
      }
    }
  });

  test('should handle settings conflicts', async () => {
    await settingsPage.click('button:has-text("General Settings")');

    // Enable high contrast and dark theme (potential conflict)
    const highContrastToggle = settingsPage.locator(
      'input[type="checkbox"]:near(text="High Contrast")'
    );
    const darkThemeOption = settingsPage.locator('input[value="Dark"]');

    if (
      (await highContrastToggle.isVisible()) &&
      (await darkThemeOption.isVisible())
    ) {
      await highContrastToggle.click();
      await darkThemeOption.click();

      // Should handle the conflict gracefully
      const conflictWarning = settingsPage.locator('text*=conflict');
      if ((await conflictWarning.count()) > 0) {
        await expect(conflictWarning.first()).toBeVisible();
      }
    }
  });
});
