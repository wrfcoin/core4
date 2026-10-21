const { test, expect } = require('@playwright/test');

/**
 * Module Management Testing Suite
 * Tests module installation, configuration, and lifecycle management
 */
test.describe('Module Management Tests', () => {
  let settingsPage;

  test.beforeEach(async ({ page }) => {
    settingsPage = page;
    await page.goto('/settings');
    await page.waitForSelector('[data-testid="settings-dashboard"]', {
      timeout: 10000,
    });
  });

  test('should display module management panel', async () => {
    // Navigate to module management tab
    await settingsPage.click('button:has-text("Module Management")');

    // Verify module management panel is visible
    await expect(
      settingsPage.locator('h2:has-text("Module Management")')
    ).toBeVisible();

    // Check for module categories
    const moduleCategories = [
      'Installed Modules',
      'Available Modules',
      'Core Modules',
      'Third-party Modules',
    ];

    for (const category of moduleCategories) {
      await expect(settingsPage.locator(`text*=${category}`)).toBeVisible();
    }
  });

  test('should display installed modules list', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Check for common installed modules
    const coreModules = [
      'Wallet Manager',
      'Transaction Handler',
      'Network Monitor',
      'Security Manager',
      'Storage Handler',
    ];

    for (const module of coreModules) {
      const moduleElement = settingsPage.locator(`text*=${module}`);
      if ((await moduleElement.count()) > 0) {
        await expect(moduleElement.first()).toBeVisible();

        // Check for module status indicator
        const statusElement = settingsPage.locator(
          `text*=Enabled:near(text="${module}")`
        );
        if ((await statusElement.count()) > 0) {
          await expect(statusElement.first()).toBeVisible();
        }
      }
    }
  });

  test('should enable and disable modules', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Find a module toggle
    const moduleToggle = settingsPage
      .locator('input[type="checkbox"]:near(text="Module")')
      .first();
    if (await moduleToggle.isVisible()) {
      const initialState = await moduleToggle.isChecked();

      // Toggle module state
      await moduleToggle.click();
      const newState = await moduleToggle.isChecked();
      expect(newState).toBe(!initialState);

      // Should show confirmation dialog for critical modules
      const confirmDialog = settingsPage.locator('text*=Are you sure');
      if ((await confirmDialog.count()) > 0) {
        await settingsPage.click('button:has-text("Confirm")');
      }

      // Toggle back
      await moduleToggle.click();
      const finalState = await moduleToggle.isChecked();
      expect(finalState).toBe(initialState);
    }
  });

  test('should install new modules', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Navigate to available modules
    const availableModulesTab = settingsPage.locator(
      'button:has-text("Available Modules")'
    );
    if (await availableModulesTab.isVisible()) {
      await availableModulesTab.click();

      // Find an installable module
      const installButton = settingsPage
        .locator('button:has-text("Install")')
        .first();
      if (await installButton.isVisible()) {
        await installButton.click();

        // Should show installation progress
        await expect(settingsPage.locator('text*=Installing')).toBeVisible({
          timeout: 5000,
        });

        // Wait for installation completion
        await expect(settingsPage.locator('text*=Installed')).toBeVisible({
          timeout: 30000,
        });
      }
    }
  });

  test('should uninstall modules', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Find a non-core module to uninstall
    const uninstallButton = settingsPage
      .locator('button:has-text("Uninstall"):not(:near(text="Core"))')
      .first();
    if (await uninstallButton.isVisible()) {
      await uninstallButton.click();

      // Should show confirmation dialog
      await expect(settingsPage.locator('text*=Are you sure')).toBeVisible();

      // Confirm uninstallation
      await settingsPage.click('button:has-text("Confirm")');

      // Should show uninstallation progress
      await expect(settingsPage.locator('text*=Uninstalling')).toBeVisible({
        timeout: 5000,
      });

      // Wait for uninstallation completion
      await expect(settingsPage.locator('text*=Uninstalled')).toBeVisible({
        timeout: 30000,
      });
    }
  });

  test('should configure module settings', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Find a module with configuration options
    const configureButton = settingsPage
      .locator('button:has-text("Configure")')
      .first();
    if (await configureButton.isVisible()) {
      await configureButton.click();

      // Should open module configuration panel
      await expect(settingsPage.locator('text*=Configuration')).toBeVisible();

      // Test common configuration options
      const configOptions = [
        'input[type="checkbox"]',
        'input[type="text"]',
        'select',
        'textarea',
      ];

      for (const option of configOptions) {
        const configElement = settingsPage.locator(option).first();
        if (await configElement.isVisible()) {
          // Test interaction based on element type
          const elementType =
            (await configElement.getAttribute('type')) ||
            (await configElement.evaluate(el => el.tagName.toLowerCase()));

          if (elementType === 'checkbox') {
            await configElement.click();
          } else if (elementType === 'text' || elementType === 'textarea') {
            await configElement.fill('test-configuration-value');
          } else if (elementType === 'select') {
            const options = await configElement.locator('option').count();
            if (options > 1) {
              await configElement.selectOption({ index: 1 });
            }
          }
        }
      }

      // Save configuration
      const saveConfigButton = settingsPage.locator(
        'button:has-text("Save Configuration")'
      );
      if (await saveConfigButton.isVisible()) {
        await saveConfigButton.click();
        await expect(settingsPage.locator('text*=saved')).toBeVisible({
          timeout: 5000,
        });
      }
    }
  });

  test('should handle module dependencies', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Try to disable a module that other modules depend on
    const coreModuleToggle = settingsPage
      .locator('input[type="checkbox"]:near(text="Core")')
      .first();
    if (
      (await coreModuleToggle.isVisible()) &&
      (await coreModuleToggle.isChecked())
    ) {
      await coreModuleToggle.click();

      // Should show dependency warning
      await expect(settingsPage.locator('text*=depend')).toBeVisible({
        timeout: 5000,
      });

      // Should list dependent modules
      await expect(settingsPage.locator('text*=modules depend')).toBeVisible();

      // Cancel the operation
      const cancelButton = settingsPage.locator('button:has-text("Cancel")');
      if (await cancelButton.isVisible()) {
        await cancelButton.click();
      }
    }
  });

  test('should validate module compatibility', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Navigate to available modules
    const availableModulesTab = settingsPage.locator(
      'button:has-text("Available Modules")'
    );
    if (await availableModulesTab.isVisible()) {
      await availableModulesTab.click();

      // Check for compatibility indicators
      const compatibilityIndicators = [
        'Compatible',
        'Incompatible',
        'Requires Update',
        'Version Conflict',
      ];

      for (const indicator of compatibilityIndicators) {
        const indicatorElement = settingsPage.locator(`text*=${indicator}`);
        if ((await indicatorElement.count()) > 0) {
          await expect(indicatorElement.first()).toBeVisible();
        }
      }
    }
  });

  test('should update modules', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Check for modules with available updates
    const updateButton = settingsPage
      .locator('button:has-text("Update")')
      .first();
    if (await updateButton.isVisible()) {
      await updateButton.click();

      // Should show update progress
      await expect(settingsPage.locator('text*=Updating')).toBeVisible({
        timeout: 5000,
      });

      // Wait for update completion
      await expect(settingsPage.locator('text*=Updated')).toBeVisible({
        timeout: 30000,
      });
    }

    // Test bulk update
    const updateAllButton = settingsPage.locator(
      'button:has-text("Update All")'
    );
    if (await updateAllButton.isVisible()) {
      await updateAllButton.click();

      // Should show bulk update progress
      await expect(settingsPage.locator('text*=Updating all')).toBeVisible({
        timeout: 5000,
      });
    }
  });

  test('should display module information', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Click on module info button
    const infoButton = settingsPage
      .locator('button:has-text("Info"), button[title*="Information"]')
      .first();
    if (await infoButton.isVisible()) {
      await infoButton.click();

      // Should display module details
      const moduleDetails = [
        'Version',
        'Author',
        'Description',
        'Dependencies',
        'License',
      ];

      for (const detail of moduleDetails) {
        const detailElement = settingsPage.locator(`text*=${detail}`);
        if ((await detailElement.count()) > 0) {
          await expect(detailElement.first()).toBeVisible();
        }
      }
    }
  });

  test('should search and filter modules', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Test module search
    const searchInput = settingsPage.locator(
      'input[placeholder*="Search"], input[name*="search"]'
    );
    if (await searchInput.isVisible()) {
      await searchInput.fill('wallet');

      // Should filter modules based on search term
      await expect(settingsPage.locator('text*=wallet')).toBeVisible();

      // Clear search
      await searchInput.clear();
    }

    // Test category filter
    const categoryFilter = settingsPage.locator(
      'select:near(text="Category"), select[name*="category"]'
    );
    if (await categoryFilter.isVisible()) {
      await categoryFilter.selectOption('Security');

      // Should filter modules by category
      await expect(settingsPage.locator('text*=Security')).toBeVisible();
    }
  });

  test('should handle module errors and recovery', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Look for modules with errors
    const errorIndicator = settingsPage
      .locator('text*=Error, text*=Failed')
      .first();
    if (await errorIndicator.isVisible()) {
      // Should show error details
      await errorIndicator.click();
      await expect(settingsPage.locator('text*=Error Details')).toBeVisible();

      // Test recovery options
      const recoverButton = settingsPage.locator(
        'button:has-text("Recover"), button:has-text("Repair")'
      );
      if (await recoverButton.isVisible()) {
        await recoverButton.click();

        // Should show recovery process
        await expect(settingsPage.locator('text*=Recovering')).toBeVisible({
          timeout: 5000,
        });
      }
    }
  });

  test('should backup and restore module configurations', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Test backup functionality
    const backupButton = settingsPage.locator(
      'button:has-text("Backup Modules")'
    );
    if (await backupButton.isVisible()) {
      const downloadPromise = settingsPage.waitForEvent('download');
      await backupButton.click();
      const download = await downloadPromise;
      expect(download.suggestedFilename()).toContain('modules');
    }

    // Test restore functionality
    const restoreButton = settingsPage.locator(
      'button:has-text("Restore Modules")'
    );
    if (await restoreButton.isVisible()) {
      await restoreButton.click();

      // Should show file upload dialog
      await expect(settingsPage.locator('input[type="file"]')).toBeVisible();
    }
  });

  test('should manage module permissions', async () => {
    await settingsPage.click('button:has-text("Module Management")');

    // Find a module with permission settings
    const permissionsButton = settingsPage
      .locator('button:has-text("Permissions")')
      .first();
    if (await permissionsButton.isVisible()) {
      await permissionsButton.click();

      // Should show permission settings
      await expect(settingsPage.locator('text*=Permissions')).toBeVisible();

      // Test permission toggles
      const permissionTypes = [
        'Network Access',
        'File System Access',
        'Cryptographic Operations',
        'User Data Access',
      ];

      for (const permission of permissionTypes) {
        const permissionToggle = settingsPage.locator(
          `input[type="checkbox"]:near(text="${permission}")`
        );
        if (await permissionToggle.isVisible()) {
          const initialState = await permissionToggle.isChecked();
          await permissionToggle.click();
          const newState = await permissionToggle.isChecked();
          expect(newState).toBe(!initialState);
        }
      }

      // Save permission changes
      const savePermissionsButton = settingsPage.locator(
        'button:has-text("Save Permissions")'
      );
      if (await savePermissionsButton.isVisible()) {
        await savePermissionsButton.click();
        await expect(settingsPage.locator('text*=saved')).toBeVisible({
          timeout: 5000,
        });
      }
    }
  });
});
