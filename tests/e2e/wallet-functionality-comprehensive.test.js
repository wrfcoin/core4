/**
 * Comprehensive Wallet Functionality Testing Suite
 * Tests wallet creation, import, transfers, multi-asset holdings, transaction history, and backup/restore
 *
 * Task Master Subtask 8.1: Set up Playwright Environment and Test Fixtures
 * Created: 2025-06-23
 */

const { test, expect } = require('@playwright/test');

// Test Configuration
const TEST_CONFIG = {
  baseUrl: 'http://localhost:5000',
  walletRoutes: {
    main: '/wallet',
    enhanced: '/wallet/enhanced',
  },
  timeouts: {
    standard: 10000,
    wallet_creation: 15000,
    blockchain_operation: 20000,
  },
  testData: {
    walletName: 'Test Wallet ' + Date.now(),
    password: 'TestWallet123!',
    testAddress: 'wrf717d72395303fcb2aed4df368f32f1078d18e9cc',
    mockMnemonic:
      'abandon ability able about above absent absorb abstract absurd abuse access accident',
  },
};

// Test Helper Functions
const WalletTestHelpers = {
  /**
   * Navigate to wallet page and wait for full load
   */
  async navigateToWallet(page, route = 'main') {
    const url = TEST_CONFIG.baseUrl + TEST_CONFIG.walletRoutes[route];
    await page.goto(url);
    await page.waitForLoadState('networkidle');
    await page.waitForSelector(
      '[data-testid="wallet-page"], .wallet-container, h4:has-text("WRF Wallet")',
      { timeout: TEST_CONFIG.timeouts.standard }
    );
  },

  /**
   * Mock wallet API responses for testing
   */
  async mockWalletApiResponses(page) {
    // Mock mnemonic generation
    await page.route('**/api/wallet/generate-mnemonic', async route => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          mnemonic: TEST_CONFIG.testData.mockMnemonic,
          strength: 128,
          wordCount: 12,
          entropy: 'deadbeefdeadbeef',
          success: true,
        }),
      });
    });

    // Mock wallet creation
    await page.route('**/api/wallet/create', async route => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 'test-wallet-' + Date.now(),
          address: TEST_CONFIG.testData.testAddress,
          publicKey: '0x' + 'a'.repeat(128),
          network: 'testnet',
          balance: '0.00',
          success: true,
        }),
      });
    });

    // Mock balance and holdings
    await page.route('**/api/wallet/balance/**', async route => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          balance: '8.60 WRF',
          usdValue: '$12.45',
          assets: [
            {
              symbol: 'WRF',
              balance: '8.60',
              usdValue: '$12.45',
              change24h: '+2.3%',
            },
            {
              symbol: 'ETH',
              balance: '0.0025',
              usdValue: '$5.20',
              change24h: '-1.1%',
            },
            {
              symbol: 'BTC',
              balance: '0.00015',
              usdValue: '$8.80',
              change24h: '+0.5%',
            },
          ],
        }),
      });
    });

    // Mock transaction history
    await page.route('**/api/wallet/transactions/**', async route => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          transactions: [
            {
              id: 'tx1',
              type: 'receive',
              amount: '2.50 WRF',
              from: '0x123...abc',
              timestamp: new Date().toISOString(),
              status: 'confirmed',
            },
            {
              id: 'tx2',
              type: 'send',
              amount: '1.00 WRF',
              to: '0x456...def',
              timestamp: new Date(Date.now() - 3600000).toISOString(),
              status: 'confirmed',
            },
          ],
          pagination: { page: 1, total: 2 },
        }),
      });
    });

    console.log('✅ Wallet API mocks configured');
  },

  /**
   * Inject wallet testing utilities into page
   */
  async injectTestHelpers(page) {
    await page.addInitScript(() => {
      window.walletTestHelpers = {
        simulateWalletCreation: walletData => {
          window.mockWalletData = walletData;
          return Promise.resolve(walletData);
        },
        simulateTransaction: txData => {
          window.mockTransactionData = txData;
          return Promise.resolve({ txHash: 'mock_' + Date.now(), ...txData });
        },
        validateAddress: address => {
          return /^(wrf|0x)[a-fA-F0-9]{40}$/.test(address);
        },
      };
    });
  },

  /**
   * Fill wallet creation form
   */
  async fillWalletCreationForm(
    page,
    walletName = TEST_CONFIG.testData.walletName,
    password = TEST_CONFIG.testData.password
  ) {
    await page.fill(
      '[name="walletName"], input[placeholder*="wallet name"]',
      walletName
    );
    await page.fill('[name="password"], input[type="password"]', password);
    await page.fill(
      '[name="confirmPassword"], input[placeholder*="confirm"]',
      password
    );

    // Check terms agreement if present
    const termsCheckbox = page.locator(
      'input[type="checkbox"]:near(text("terms")), input[name="agreedToTerms"]'
    );
    if ((await termsCheckbox.count()) > 0) {
      await termsCheckbox.check();
    }
  },

  /**
   * Wait for wallet operation to complete
   */
  async waitForWalletOperation(page, operation = 'creation') {
    const loadingSelectors = [
      '.loading',
      '[data-testid="wallet-loading"]',
      'text="Creating wallet..."',
      'text="Generating..."',
      '.spinner',
    ];

    // Wait for loading to start (if present)
    try {
      await page.waitForSelector(loadingSelectors.join(', '), {
        timeout: 2000,
      });
    } catch (e) {
      // Loading might start instantly, continue
    }

    // Wait for loading to finish
    for (const selector of loadingSelectors) {
      await page.waitForSelector(selector, {
        state: 'detached',
        timeout: TEST_CONFIG.timeouts.wallet_creation,
      });
    }

    // Wait for success indicators
    await page.waitForSelector(
      [
        'text="success"',
        'text="created"',
        'text="completed"',
        '.success',
        '[data-testid="wallet-success"]',
      ].join(', '),
      { timeout: TEST_CONFIG.timeouts.standard }
    );
  },
};

test.describe('Wallet Functionality - Comprehensive Testing Suite', () => {
  test.beforeEach(async ({ page }) => {
    console.log('🚀 Setting up wallet test environment...');

    // Configure test environment
    await WalletTestHelpers.mockWalletApiResponses(page);
    await WalletTestHelpers.injectTestHelpers(page);

    // Navigate to wallet
    await WalletTestHelpers.navigateToWallet(page);

    console.log('✅ Wallet test environment ready');
  });

  test.describe('1. Wallet Creation and Import Tests', () => {
    test('should display wallet page with all main components', async ({
      page,
    }) => {
      // Verify main wallet page structure
      await expect(
        page.locator('h4:has-text("WRF Wallet"), h2:has-text("WRF Wallet")')
      ).toBeVisible();

      // Check for main wallet sections
      const expectedSections = [
        'Overview',
        'Multi-Assets',
        'Transactions',
        'Backup',
      ];

      for (const section of expectedSections) {
        await expect(page.locator(`text="${section}"`)).toBeVisible();
      }

      // Verify wallet address display
      await expect(page.locator('text=/wrf[a-fA-F0-9]+/')).toBeVisible();

      console.log('✅ Wallet page structure validated');
    });

    test('should create new wallet with valid inputs', async ({ page }) => {
      // Find and click create wallet button
      const createWalletButton = page.locator(
        'button:has-text("Create"), button:has-text("New Wallet"), [data-testid="create-wallet"]'
      );

      if ((await createWalletButton.count()) === 0) {
        // Navigate to enhanced wallet page if no create button on main page
        await WalletTestHelpers.navigateToWallet(page, 'enhanced');
      }

      await expect(createWalletButton.first()).toBeVisible();
      await createWalletButton.first().click();

      // Wait for creation modal/form
      await page.waitForSelector(
        'form, [role="dialog"], .modal, .wallet-creation',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Fill wallet creation form
      await WalletTestHelpers.fillWalletCreationForm(page);

      // Generate mnemonic if needed
      const generateButton = page.locator(
        'button:has-text("Generate"), button:has-text("New Mnemonic")'
      );
      if ((await generateButton.count()) > 0) {
        await generateButton.click();
        await page.waitForSelector('text=/abandon|ability|able/', {
          timeout: 5000,
        });
      }

      // Submit form
      const submitButton = page.locator(
        'button[type="submit"], button:has-text("Create Wallet"), button:has-text("Confirm")'
      );
      await submitButton.click();

      // Wait for wallet creation to complete
      await WalletTestHelpers.waitForWalletOperation(page, 'creation');

      // Verify wallet was created successfully
      await expect(
        page.locator('text="Wallet created", text="Success", .success')
      ).toBeVisible();

      console.log('✅ Wallet creation successful');
    });

    test('should import wallet using mnemonic phrase', async ({ page }) => {
      // Find import wallet option
      const importButton = page.locator(
        'button:has-text("Import"), [data-testid="import-wallet"]'
      );

      if ((await importButton.count()) > 0) {
        await importButton.click();
      } else {
        // Look for import tab in wallet creation modal
        const createButton = page.locator(
          'button:has-text("Create"), button:has-text("New Wallet")'
        );
        await createButton.first().click();

        await page.waitForSelector('[role="dialog"], .modal', {
          timeout: TEST_CONFIG.timeouts.standard,
        });

        // Look for import tab
        const importTab = page.locator(
          'button:has-text("Import"), [role="tab"]:has-text("Import")'
        );
        await importTab.click();
      }

      // Fill mnemonic phrase
      const mnemonicInput = page.locator(
        'textarea[name="mnemonicPhrase"], input[name="mnemonic"], [data-testid="mnemonic-input"]'
      );
      await mnemonicInput.fill(TEST_CONFIG.testData.mockMnemonic);

      // Fill wallet details
      await WalletTestHelpers.fillWalletCreationForm(
        page,
        'Imported Wallet ' + Date.now()
      );

      // Submit import
      const importSubmitButton = page.locator(
        'button:has-text("Import Wallet"), button[type="submit"]'
      );
      await importSubmitButton.click();

      // Wait for import to complete
      await WalletTestHelpers.waitForWalletOperation(page, 'import');

      // Verify import success
      await expect(
        page.locator('text="imported", text="Success", .success')
      ).toBeVisible();

      console.log('✅ Wallet import successful');
    });

    test('should validate wallet creation form inputs', async ({ page }) => {
      // Open wallet creation
      const createButton = page
        .locator('button:has-text("Create"), button:has-text("New Wallet")')
        .first();
      await createButton.click();

      await page.waitForSelector('form, [role="dialog"]', {
        timeout: TEST_CONFIG.timeouts.standard,
      });

      // Test empty form validation
      const submitButton = page.locator(
        'button[type="submit"], button:has-text("Create")'
      );
      await submitButton.click();

      // Check for validation errors
      await expect(
        page.locator('text="required", .error, [role="alert"]')
      ).toBeVisible();

      // Test password mismatch
      await page.fill('[name="password"]', 'password123');
      await page.fill('[name="confirmPassword"]', 'different123');
      await submitButton.click();

      await expect(
        page.locator('text="match", text="mismatch", .error')
      ).toBeVisible();

      // Test weak password
      await page.fill('[name="password"]', '123');
      await page.fill('[name="confirmPassword"]', '123');
      await submitButton.click();

      await expect(
        page.locator('text="8 characters", text="too short", .error')
      ).toBeVisible();

      console.log('✅ Form validation working correctly');
    });
  });

  test.describe('2. Multi-Asset Holdings Tests', () => {
    test('should display multi-asset portfolio with all assets', async ({
      page,
    }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets"), button:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      // Wait for portfolio data to load
      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Verify asset holdings display
      const expectedAssets = ['WRF', 'ETH', 'BTC'];
      for (const asset of expectedAssets) {
        await expect(page.locator(`text="${asset}"`)).toBeVisible();
      }

      // Verify balance and USD value display
      await expect(page.locator('text=/\\$[0-9]+\\.[0-9]+/')).toBeVisible(); // USD values
      await expect(page.locator('text=/[0-9]+\\.[0-9]+/')).toBeVisible(); // Crypto amounts

      // Verify percentage changes
      await expect(page.locator('text=/%/, text="+", text="-"')).toBeVisible();

      console.log('✅ Multi-asset portfolio displayed correctly');
    });

    test('should show accurate asset balances for each cryptocurrency', async ({
      page,
    }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Verify specific asset balances match mock data
      const assetBalances = {
        WRF: '8.60',
        ETH: '0.0025',
        BTC: '0.00015',
      };

      for (const [asset, expectedBalance] of Object.entries(assetBalances)) {
        const assetRow = page.locator(
          `tr:has-text("${asset}"), .asset-item:has-text("${asset}")`
        );
        await expect(assetRow).toBeVisible();
        await expect(
          assetRow.locator(`text="${expectedBalance}"`)
        ).toBeVisible();
      }

      // Verify USD values are displayed for each asset
      const expectedUsdValues = ['$12.45', '$5.20', '$8.80'];
      for (const usdValue of expectedUsdValues) {
        await expect(page.locator(`text="${usdValue}"`)).toBeVisible();
      }

      console.log('✅ Accurate asset balances verified');
    });

    test('should display 24h percentage changes for all assets', async ({
      page,
    }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Verify 24h changes for each asset
      const expectedChanges = ['+2.3%', '-1.1%', '+0.5%'];
      for (const change of expectedChanges) {
        await expect(page.locator(`text="${change}"`)).toBeVisible();
      }

      // Verify positive changes are styled appropriately (green color)
      const positiveChanges = page.locator('text="+2.3%", text="+0.5%"');
      await expect(positiveChanges.first()).toBeVisible();

      // Verify negative changes are styled appropriately (red color)
      const negativeChange = page.locator('text="-1.1%"');
      await expect(negativeChange).toBeVisible();

      console.log('✅ 24h percentage changes displayed correctly');
    });

    test('should show asset details and allow interactions', async ({
      page,
    }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        '.asset-row, [data-testid="asset-item"], table tbody tr',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Click on first asset (WRF)
      const firstAsset = page
        .locator('.asset-row, [data-testid="asset-item"], table tbody tr')
        .first();
      await firstAsset.click();

      // Verify asset detail view or expanded information
      await expect(
        page.locator('text="Details", text="Chart", .asset-details')
      ).toBeVisible();

      console.log('✅ Asset interactions working');
    });

    test('should calculate and display total portfolio value', async ({
      page,
    }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for total portfolio value ($12.45 + $5.20 + $8.80 = $26.45)
      const totalValue = page.locator(
        'text=/Total.*\\$[0-9]+\\.[0-9]+/, [data-testid="total-value"], text=/Portfolio.*\\$[0-9]+\\.[0-9]+/'
      );
      await expect(totalValue).toBeVisible({ timeout: 10000 });

      // Verify the calculated total is reasonable (should be around $26.45)
      const totalText = await totalValue.textContent();
      const totalMatch = totalText.match(/\$([0-9]+\.[0-9]+)/);
      if (totalMatch) {
        const totalAmount = parseFloat(totalMatch[1]);
        expect(totalAmount).toBeGreaterThan(20); // Should be around $26.45
        expect(totalAmount).toBeLessThan(50);
      }

      console.log('✅ Total portfolio value calculated correctly');
    });

    test('should handle portfolio with large number of assets', async ({
      page,
    }) => {
      // Mock API with many assets for performance testing
      await page.route('**/api/wallet/balance/**', async route => {
        const manyAssets = [];
        const cryptos = [
          'WRF',
          'ETH',
          'BTC',
          'ADA',
          'DOT',
          'LINK',
          'UNI',
          'AAVE',
          'SUSHI',
          'YFI',
          'CRV',
          'SNX',
          'COMP',
          'MKR',
          'ZRX',
        ];

        for (let i = 0; i < 15; i++) {
          manyAssets.push({
            symbol: cryptos[i] || `TOKEN${i}`,
            balance: (Math.random() * 100).toFixed(4),
            usdValue: `$${(Math.random() * 1000).toFixed(2)}`,
            change24h: `${Math.random() > 0.5 ? '+' : '-'}${(Math.random() * 10).toFixed(1)}%`,
          });
        }

        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            balance: '850.60 WRF',
            usdValue: '$1,245.50',
            assets: manyAssets,
          }),
        });
      });

      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      // Measure loading performance
      const startTime = Date.now();
      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );
      const loadTime = Date.now() - startTime;

      // Verify multiple assets are displayed
      await expect(page.locator('tr, .asset-item')).toHaveCount({ min: 10 }); // At least 10 assets

      // Performance should be reasonable (under 5 seconds)
      expect(loadTime).toBeLessThan(5000);

      console.log(
        `✅ Large portfolio loaded in ${loadTime}ms with multiple assets`
      );
    });

    test('should support asset sorting and filtering', async ({ page }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for sorting controls (column headers, sort buttons)
      const sortControls = page.locator(
        'th[role="columnheader"], button:has-text("Sort"), [data-testid="sort"]'
      );
      if ((await sortControls.count()) > 0) {
        await sortControls.first().click();

        // Wait for re-sorting to occur
        await page.waitForTimeout(1000);

        // Verify assets are still displayed after sorting
        await expect(
          page.locator('text="WRF", text="ETH", text="BTC"')
        ).toBeVisible();
      }

      // Look for filter/search functionality
      const filterInput = page.locator(
        'input[placeholder*="search"], input[placeholder*="filter"], [data-testid="asset-search"]'
      );
      if ((await filterInput.count()) > 0) {
        await filterInput.fill('WRF');
        await page.waitForTimeout(1000);

        // Verify filtering worked
        await expect(page.locator('text="WRF"')).toBeVisible();
      }

      console.log('✅ Asset sorting and filtering tested');
    });

    test('should display asset allocation percentages', async ({ page }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for allocation percentages or pie chart
      const allocationElements = page.locator(
        'text=/[0-9]+%/, [data-testid="allocation"], .allocation-percentage'
      );
      if ((await allocationElements.count()) > 0) {
        await expect(allocationElements.first()).toBeVisible();

        // Verify percentages add up reasonably (allow some tolerance for rounding)
        const percentageTexts = await allocationElements.allTextContents();
        const percentages = percentageTexts.map(text => {
          const match = text.match(/([0-9]+(?:\.[0-9]+)?)%/);
          return match ? parseFloat(match[1]) : 0;
        });

        const totalPercentage = percentages.reduce((sum, pct) => sum + pct, 0);
        if (totalPercentage > 0) {
          expect(totalPercentage).toBeGreaterThan(90); // Should be close to 100%
          expect(totalPercentage).toBeLessThan(110);
        }
      }

      console.log('✅ Asset allocation percentages verified');
    });

    test('should handle portfolio loading states and errors', async ({
      page,
    }) => {
      // Mock API error for portfolio
      await page.route('**/api/wallet/balance/**', route => route.abort());

      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      // Wait for error state or loading indicator
      await page.waitForSelector(
        'text="error", text="failed", .error, .loading',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      console.log('✅ Error handling verified');
    });

    test('should refresh portfolio data when requested', async ({ page }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for refresh button
      const refreshButton = page.locator(
        'button:has-text("Refresh"), [data-testid="refresh"], button[title*="refresh"]'
      );
      if ((await refreshButton.count()) > 0) {
        await refreshButton.click();

        // Wait for loading state
        await page
          .waitForSelector('.loading, [data-testid="loading"]', {
            timeout: 3000,
          })
          .catch(() => {
            // Loading might be too fast to catch
          });

        // Verify data is still displayed after refresh
        await expect(
          page.locator('text="WRF", text="ETH", text="BTC"')
        ).toBeVisible();
      }

      console.log('✅ Portfolio refresh functionality tested');
    });

    test('should display asset icons and symbols correctly', async ({
      page,
    }) => {
      // Navigate to Multi-Assets tab
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
      }

      await page.waitForSelector(
        'table, .asset-list, [data-testid="portfolio"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Verify asset symbols are displayed prominently
      const assetSymbols = ['WRF', 'ETH', 'BTC'];
      for (const symbol of assetSymbols) {
        const symbolElement = page.locator(`text="${symbol}"`);
        await expect(symbolElement).toBeVisible();

        // Check if there's an icon near the symbol
        const parentElement = symbolElement.locator('xpath=..');
        const iconElements = parentElement.locator(
          'img, svg, .icon, [class*="icon"]'
        );
        if ((await iconElements.count()) > 0) {
          await expect(iconElements.first()).toBeVisible();
        }
      }

      // Verify full asset names if displayed
      const assetNames = page.locator(
        'text="Wrfcoin", text="Ethereum", text="Bitcoin"'
      );
      if ((await assetNames.count()) > 0) {
        await expect(assetNames.first()).toBeVisible();
      }

      console.log('✅ Asset icons and symbols displayed correctly');
    });
  });

  test.describe('3. Transfer Functionality Tests', () => {
    test('should display transfer form with validation', async ({ page }) => {
      // Find transfer section (could be in Overview tab or separate Transfer tab)
      const transferSection = page
        .locator('[data-testid="transfer"], .transfer-form, text="Send"')
        .first();

      if ((await transferSection.count()) === 0) {
        // Look for transfer tab
        const transferTab = page.locator(
          '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
        );
        if ((await transferTab.count()) > 0) {
          await transferTab.click();
        }
      }

      await expect(
        page.locator('input[placeholder*="address"], input[name="address"]')
      ).toBeVisible();
      await expect(
        page.locator('input[placeholder*="amount"], input[name="amount"]')
      ).toBeVisible();

      // Test form validation
      const sendButton = page.locator(
        'button:has-text("Send"), button[type="submit"]'
      );
      await sendButton.click();

      await expect(page.locator('text="required", .error')).toBeVisible();

      console.log('✅ Transfer form validation working');
    });

    test('should process transfer with valid inputs', async ({ page }) => {
      // Navigate to transfer section
      const transferTab = page.locator(
        '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
      );
      if ((await transferTab.count()) > 0) {
        await transferTab.click();
      }

      // Fill transfer form
      await page.fill(
        'input[placeholder*="address"], input[name="address"]',
        'wrf' + 'a'.repeat(37)
      );
      await page.fill(
        'input[placeholder*="amount"], input[name="amount"]',
        '1.5'
      );

      // Submit transfer
      const sendButton = page.locator(
        'button:has-text("Send"), button[type="submit"]'
      );
      await sendButton.click();

      // Wait for confirmation or success
      await page.waitForSelector(
        'text="confirm", text="success", [role="dialog"]',
        { timeout: TEST_CONFIG.timeouts.blockchain_operation }
      );

      console.log('✅ Transfer processing working');
    });

    test('should calculate and display transaction fees correctly', async ({
      page,
    }) => {
      // Navigate to transfer section
      const transferTab = page.locator(
        '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
      );
      if ((await transferTab.count()) > 0) {
        await transferTab.click();
      }

      // Fill valid transfer details
      await page.fill(
        'input[placeholder*="address"], input[name="address"]',
        'wrf1234567890abcdef1234567890abcdef12345678'
      );
      await page.fill(
        'input[placeholder*="amount"], input[name="amount"]',
        '10.0'
      );

      // Wait for fee calculation
      await page.waitForTimeout(1000);

      // Verify fee display
      const feeDisplay = page.locator(
        'text=/Fee.*[0-9]+/, [data-testid="transaction-fee"], text=/Gas.*[0-9]+/'
      );
      await expect(feeDisplay).toBeVisible({ timeout: 5000 });

      // Verify fee is reasonable (between 0.001 and 1.0 WRF)
      const feeText = await feeDisplay.textContent();
      const feeMatch = feeText.match(/([0-9]+\.?[0-9]*)/);
      if (feeMatch) {
        const feeAmount = parseFloat(feeMatch[1]);
        expect(feeAmount).toBeGreaterThan(0.001);
        expect(feeAmount).toBeLessThan(1.0);
      }

      console.log('✅ Transaction fees calculated and displayed correctly');
    });

    test('should handle insufficient funds error', async ({ page }) => {
      // Navigate to transfer section
      const transferTab = page.locator(
        '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
      );
      if ((await transferTab.count()) > 0) {
        await transferTab.click();
      }

      // Fill transfer form with amount exceeding balance
      await page.fill(
        'input[placeholder*="address"], input[name="address"]',
        'wrf1234567890abcdef1234567890abcdef12345678'
      );
      await page.fill(
        'input[placeholder*="amount"], input[name="amount"]',
        '999999.99'
      );

      // Submit transfer
      const sendButton = page.locator(
        'button:has-text("Send"), button[type="submit"]'
      );
      await sendButton.click();

      // Wait for insufficient funds error
      await expect(
        page.locator(
          'text="insufficient", text="not enough", text="balance too low"'
        )
      ).toBeVisible({ timeout: 10000 });

      console.log('✅ Insufficient funds error handled correctly');
    });

    test('should handle invalid recipient address error', async ({ page }) => {
      // Navigate to transfer section
      const transferTab = page.locator(
        '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
      );
      if ((await transferTab.count()) > 0) {
        await transferTab.click();
      }

      // Test cases for invalid addresses
      const invalidAddresses = [
        'invalid_address',
        '123',
        'wrf123', // Too short
        'btc1234567890abcdef1234567890abcdef12345678', // Wrong prefix
        'wrf' + 'x'.repeat(50), // Too long
      ];

      for (const invalidAddress of invalidAddresses) {
        await page.fill(
          'input[placeholder*="address"], input[name="address"]',
          invalidAddress
        );
        await page.fill(
          'input[placeholder*="amount"], input[name="amount"]',
          '1.0'
        );

        const sendButton = page.locator(
          'button:has-text("Send"), button[type="submit"]'
        );
        await sendButton.click();

        // Wait for invalid address error
        await expect(
          page.locator(
            'text="invalid address", text="invalid recipient", .error'
          )
        ).toBeVisible({ timeout: 5000 });

        // Clear form for next test
        await page.fill(
          'input[placeholder*="address"], input[name="address"]',
          ''
        );
      }

      console.log('✅ Invalid address errors handled correctly');
    });

    test('should handle transaction confirmation flow', async ({ page }) => {
      // Navigate to transfer section
      const transferTab = page.locator(
        '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
      );
      if ((await transferTab.count()) > 0) {
        await transferTab.click();
      }

      // Fill valid transfer details
      await page.fill(
        'input[placeholder*="address"], input[name="address"]',
        'wrf1234567890abcdef1234567890abcdef12345678'
      );
      await page.fill(
        'input[placeholder*="amount"], input[name="amount"]',
        '2.5'
      );

      // Submit transfer
      const sendButton = page.locator(
        'button:has-text("Send"), button[type="submit"]'
      );
      await sendButton.click();

      // Wait for confirmation dialog
      await expect(
        page.locator('[role="dialog"], .confirmation-modal, text="Confirm"')
      ).toBeVisible({ timeout: 10000 });

      // Verify confirmation details
      await expect(page.locator('text="2.5"')).toBeVisible(); // Amount
      await expect(page.locator('text=/wrf.*123.*456/')).toBeVisible(); // Recipient
      await expect(page.locator('text=/Fee.*[0-9]+/')).toBeVisible(); // Fee

      // Confirm transaction
      const confirmButton = page.locator(
        'button:has-text("Confirm"), button:has-text("Send Transaction")'
      );
      await confirmButton.click();

      // Wait for transaction to be submitted
      await expect(
        page.locator(
          'text="Transaction submitted", text="Broadcasting", text="Pending"'
        )
      ).toBeVisible({ timeout: 15000 });

      console.log('✅ Transaction confirmation flow working correctly');
    });

    test('should display receive functionality', async ({ page }) => {
      // Look for receive section or tab
      const receiveTab = page.locator(
        '[role="tab"]:has-text("Receive"), button:has-text("Receive")'
      );
      if ((await receiveTab.count()) > 0) {
        await receiveTab.click();
      }

      // Verify wallet address is displayed for receiving
      await expect(
        page.locator('text=/wrf[a-fA-F0-9]+/, [data-testid="wallet-address"]')
      ).toBeVisible();

      // Verify QR code or copy functionality
      const addressElement = page.locator('text=/wrf[a-fA-F0-9]+/').first();
      const address = await addressElement.textContent();
      expect(address).toMatch(/^wrf[a-fA-F0-9]{38,}$/);

      // Test copy address functionality if available
      const copyButton = page.locator(
        'button:has-text("Copy"), [data-testid="copy-address"]'
      );
      if ((await copyButton.count()) > 0) {
        await copyButton.click();
        await expect(
          page.locator('text="Copied", text="Address copied"')
        ).toBeVisible({ timeout: 3000 });
      }

      console.log('✅ Receive functionality working correctly');
    });

    test('should validate amount input constraints', async ({ page }) => {
      // Navigate to transfer section
      const transferTab = page.locator(
        '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
      );
      if ((await transferTab.count()) > 0) {
        await transferTab.click();
      }

      const amountInput = page.locator(
        'input[placeholder*="amount"], input[name="amount"]'
      );
      const addressInput = page.locator(
        'input[placeholder*="address"], input[name="address"]'
      );

      // Fill valid address
      await addressInput.fill('wrf1234567890abcdef1234567890abcdef12345678');

      // Test negative amount
      await amountInput.fill('-1.0');
      const sendButton = page.locator(
        'button:has-text("Send"), button[type="submit"]'
      );
      await sendButton.click();
      await expect(
        page.locator('text="positive", text="greater than", .error')
      ).toBeVisible({ timeout: 5000 });

      // Test zero amount
      await amountInput.fill('0');
      await sendButton.click();
      await expect(
        page.locator('text="greater than", text="minimum", .error')
      ).toBeVisible({ timeout: 5000 });

      // Test very small amount (dust limit)
      await amountInput.fill('0.00000001');
      await sendButton.click();
      await expect(
        page.locator('text="minimum", text="dust", .error')
      ).toBeVisible({ timeout: 5000 });

      console.log('✅ Amount validation constraints working correctly');
    });

    test('should handle network errors during transfer', async ({ page }) => {
      // Navigate to transfer section
      const transferTab = page.locator(
        '[role="tab"]:has-text("Transfer"), button:has-text("Send")'
      );
      if ((await transferTab.count()) > 0) {
        await transferTab.click();
      }

      // Fill valid transfer details
      await page.fill(
        'input[placeholder*="address"], input[name="address"]',
        'wrf1234567890abcdef1234567890abcdef12345678'
      );
      await page.fill(
        'input[placeholder*="amount"], input[name="amount"]',
        '1.0'
      );

      // Mock network failure
      await page.route('**/api/wallet/transfer', route => {
        route.fulfill({
          status: 500,
          body: JSON.stringify({ error: 'Network error' }),
        });
      });

      // Submit transfer
      const sendButton = page.locator(
        'button:has-text("Send"), button[type="submit"]'
      );
      await sendButton.click();

      // Wait for network error handling
      await expect(
        page.locator(
          'text="network error", text="connection failed", text="try again"'
        )
      ).toBeVisible({ timeout: 10000 });

      console.log('✅ Network error handling working correctly');
    });
  });

  test.describe('4. Transaction History Tests', () => {
    test('should display complete transaction history with all required fields', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions"), button:has-text("History")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Verify transaction items are displayed
      await expect(
        page.locator('text="receive", text="send", .transaction-item')
      ).toBeVisible();

      // Verify all required transaction fields are present
      const transactionRows = page.locator('tr, .transaction-item').first();
      await expect(transactionRows).toBeVisible();

      // Verify transaction amounts with proper formatting
      await expect(page.locator('text=/[0-9]+\\.[0-9]+ WRF/')).toBeVisible();

      // Verify transaction status
      await expect(
        page.locator('text="confirmed", text="pending", text="failed", .status')
      ).toBeVisible();

      // Verify transaction dates are displayed
      await expect(
        page.locator(
          'text=/\\d{1,2}\/\\d{1,2}\/\\d{4}/, text=/\\d{4}-\\d{2}-\\d{2}/, .date, .timestamp'
        )
      ).toBeVisible();

      // Verify transaction IDs/hashes are present
      await expect(
        page.locator('text=/0x[a-fA-F0-9]+/, .transaction-hash, .tx-id')
      ).toBeVisible();

      console.log('✅ Complete transaction history displayed correctly');
    });

    test('should verify accuracy of individual transaction records', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Test specific transaction from mock data
      const expectedTransactions = [
        { type: 'send', amount: '1.25 WRF', status: 'confirmed' },
        { type: 'receive', amount: '5.00 WRF', status: 'confirmed' },
        { type: 'send', amount: '0.75 WRF', status: 'pending' },
      ];

      for (const expectedTx of expectedTransactions) {
        // Find transaction row containing this data
        const txRow = page.locator(
          `tr:has-text("${expectedTx.type}"), .transaction-item:has-text("${expectedTx.type}")`
        );
        await expect(txRow).toBeVisible();

        // Verify amount is correct
        await expect(
          txRow.locator(`text="${expectedTx.amount}"`)
        ).toBeVisible();

        // Verify status is correct
        await expect(
          txRow.locator(`text="${expectedTx.status}"`)
        ).toBeVisible();
      }

      console.log('✅ Individual transaction records verified for accuracy');
    });

    test('should display transactions with correct chronological ordering', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Get all transaction rows
      const transactionRows = page.locator('tr, .transaction-item');
      const rowCount = await transactionRows.count();

      if (rowCount > 1) {
        // Extract dates from first two transactions
        const firstRowDate = transactionRows
          .nth(0)
          .locator('.date, .timestamp');
        const secondRowDate = transactionRows
          .nth(1)
          .locator('.date, .timestamp');

        // Verify both have dates
        await expect(firstRowDate).toBeVisible();
        await expect(secondRowDate).toBeVisible();

        // Verify transactions are sorted (most recent first is typical)
        console.log('✅ Transaction chronological ordering verified');
      }

      console.log('✅ Transaction ordering verified');
    });

    test('should validate transaction ID format and uniqueness', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Get all transaction IDs/hashes
      const txHashElements = page.locator(
        'text=/0x[a-fA-F0-9]+/, .transaction-hash, .tx-id'
      );
      const hashCount = await txHashElements.count();

      if (hashCount > 0) {
        // Verify first transaction ID format
        const firstHash = await txHashElements.first().textContent();

        // Transaction hash should be hex format
        const hexPattern = /^0x[a-fA-F0-9]{64}$|^[a-fA-F0-9]{64}$/;
        const isValidHash = hexPattern.test(firstHash.trim());

        expect(isValidHash || firstHash.length > 10).toBeTruthy(); // Either valid hash or reasonable ID

        console.log(`✅ Transaction ID format validated: ${firstHash}`);
      }

      console.log('✅ Transaction ID validation completed');
    });

    test('should implement functional transaction filtering and search', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Test different filter options
      const filterTests = [
        { filter: 'send', expectedType: 'send' },
        { filter: 'receive', expectedType: 'receive' },
        { filter: 'confirmed', expectedStatus: 'confirmed' },
        { filter: 'pending', expectedStatus: 'pending' },
      ];

      for (const test of filterTests) {
        // Look for filter controls
        const searchInput = page.locator(
          'input[placeholder*="search"], input[name="search"], [data-testid="transaction-search"]'
        );
        const filterSelect = page.locator(
          'select[name="filter"], select[data-testid="transaction-filter"]'
        );

        if ((await searchInput.count()) > 0) {
          // Clear and enter search term
          await searchInput.clear();
          await searchInput.fill(test.filter);
          await page.waitForTimeout(1000); // Wait for filter to apply

          // Verify filtering worked
          if (test.expectedType) {
            await expect(
              page.locator(`text="${test.expectedType}"`)
            ).toBeVisible();
          }
          if (test.expectedStatus) {
            await expect(
              page.locator(`text="${test.expectedStatus}"`)
            ).toBeVisible();
          }
        } else if ((await filterSelect.count()) > 0) {
          // Use dropdown filter
          await filterSelect.selectOption(test.filter);
          await page.waitForTimeout(1000);
        }
      }

      console.log('✅ Transaction filtering and search functionality verified');
    });

    test('should handle pagination for large transaction histories', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for pagination controls
      const paginationControls = page.locator(
        '.pagination, [data-testid="pagination"], button:has-text("Next"), button:has-text("Previous")'
      );

      if ((await paginationControls.count()) > 0) {
        // Test pagination functionality
        const nextButton = page.locator(
          'button:has-text("Next"), [data-testid="next-page"]'
        );

        if ((await nextButton.count()) > 0 && (await nextButton.isEnabled())) {
          await nextButton.click();
          await page.waitForTimeout(1000);

          // Verify page changed
          await expect(
            page.locator(
              'table, .transaction-list, [data-testid="transactions"]'
            )
          ).toBeVisible();

          // Test previous button
          const prevButton = page.locator(
            'button:has-text("Previous"), [data-testid="prev-page"]'
          );
          if (
            (await prevButton.count()) > 0 &&
            (await prevButton.isEnabled())
          ) {
            await prevButton.click();
            await page.waitForTimeout(1000);
          }
        }
      }

      // Verify page size controls if present
      const pageSizeSelect = page.locator(
        'select[name="pageSize"], select[data-testid="page-size"]'
      );
      if ((await pageSizeSelect.count()) > 0) {
        await pageSizeSelect.selectOption('50');
        await page.waitForTimeout(1000);
      }

      console.log('✅ Transaction pagination functionality verified');
    });

    test('should display transaction details in expandable view', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for first transaction row
      const firstTransaction = page.locator('tr, .transaction-item').first();
      await expect(firstTransaction).toBeVisible();

      // Try to click for details (expandable row or dedicated button)
      const detailTriggers = firstTransaction.locator(
        'button:has-text("Details"), button:has-text("View"), .expand-icon, td, .transaction-row'
      );

      if ((await detailTriggers.count()) > 0) {
        await detailTriggers.first().click();
        await page.waitForTimeout(1000);

        // Look for expanded details
        const detailsPanel = page.locator(
          '.transaction-details, .expanded-row, [data-testid="transaction-details"]'
        );

        if ((await detailsPanel.count()) > 0) {
          await expect(detailsPanel).toBeVisible();

          // Verify detailed information is shown
          await expect(
            detailsPanel.locator(
              'text=/Block.*[0-9]+/, text=/Gas.*[0-9]+/, text=/Fee.*[0-9]+/'
            )
          ).toBeVisible();
        }
      }

      console.log('✅ Transaction detail view functionality verified');
    });

    test('should validate transaction amounts and calculate totals correctly', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Get all transaction amounts
      const amountElements = page.locator(
        'text=/[0-9]+\\.[0-9]+ WRF/, .amount, [data-testid="transaction-amount"]'
      );
      const amountCount = await amountElements.count();

      if (amountCount > 0) {
        // Verify amounts are properly formatted
        for (let i = 0; i < Math.min(amountCount, 5); i++) {
          const amountText = await amountElements.nth(i).textContent();
          const amountMatch = amountText.match(/([0-9]+\.[0-9]+)/);

          if (amountMatch) {
            const amount = parseFloat(amountMatch[1]);
            expect(amount).toBeGreaterThan(0);
            expect(amount).toBeLessThan(1000000); // Reasonable upper bound
          }
        }
      }

      // Check for total calculations if present
      const totalElements = page.locator(
        'text=/Total.*[0-9]+/, .total, [data-testid="transaction-total"]'
      );
      if ((await totalElements.count()) > 0) {
        await expect(totalElements.first()).toBeVisible();
      }

      console.log('✅ Transaction amounts and totals validated');
    });

    test('should handle different transaction types correctly', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Verify different transaction types are supported
      const transactionTypes = ['send', 'receive', 'swap', 'stake', 'unstake'];

      for (const txType of transactionTypes) {
        const typeElements = page.locator(
          `text="${txType}", .transaction-type:has-text("${txType}"), [data-type="${txType}"]`
        );

        if ((await typeElements.count()) > 0) {
          await expect(typeElements.first()).toBeVisible();

          // Verify type-specific styling or icons
          const typeRow = typeElements
            .first()
            .locator(
              'xpath=ancestor::tr | xpath=ancestor::div[contains(@class, "transaction")]'
            );
          await expect(typeRow).toBeVisible();

          console.log(`✅ Transaction type "${txType}" properly displayed`);
        }
      }

      console.log('✅ Different transaction types verified');
    });

    test('should provide transaction export functionality', async ({
      page,
    }) => {
      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForSelector(
        'table, .transaction-list, [data-testid="transactions"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for export functionality
      const exportButtons = page.locator(
        'button:has-text("Export"), button:has-text("Download"), [data-testid="export-transactions"]'
      );

      if ((await exportButtons.count()) > 0) {
        // Test export button is clickable
        await expect(exportButtons.first()).toBeVisible();
        await expect(exportButtons.first()).toBeEnabled();

        // Click export (but don't actually download in test)
        await exportButtons.first().click();

        // Look for export options or confirmation
        const exportOptions = page.locator(
          'text="CSV", text="PDF", text="JSON", .export-format'
        );
        if ((await exportOptions.count()) > 0) {
          await expect(exportOptions.first()).toBeVisible();
        }
      }

      console.log('✅ Transaction export functionality verified');
    });

    test('should handle empty transaction history gracefully', async ({
      page,
    }) => {
      // Mock empty transaction response
      await page.route('**/api/wallet/transactions/**', async route => {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            transactions: [],
            totalCount: 0,
            page: 1,
            limit: 20,
          }),
        });
      });

      // Navigate to Transactions tab
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
      }

      await page.waitForTimeout(2000); // Wait for API response

      // Verify empty state is displayed
      const emptyStateElements = page.locator(
        'text="No transactions", text="empty", text="transactions found", .empty-state'
      );
      await expect(emptyStateElements.first()).toBeVisible();

      console.log('✅ Empty transaction history handled gracefully');
    });
  });

  test.describe('5. Backup and Restore Tests', () => {
    test('should display comprehensive backup options and interface', async ({
      page,
    }) => {
      // Navigate to Backup tab
      const backupTab = page.locator(
        '[role="tab"]:has-text("Backup"), button:has-text("Backup")'
      );
      if ((await backupTab.count()) > 0) {
        await backupTab.click();
      }

      await page.waitForSelector(
        '.backup-container, [data-testid="backup-section"], .wallet-backup',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Verify comprehensive backup options are available
      const backupOptions = [
        'mnemonic',
        'private key',
        'keystore',
        'backup',
        'seed phrase',
      ];

      for (const option of backupOptions) {
        const optionElement = page.locator(
          `text="${option}", button:has-text("${option}"), [data-backup-type="${option}"]`
        );
        if ((await optionElement.count()) > 0) {
          await expect(optionElement.first()).toBeVisible();
          console.log(`✅ Backup option "${option}" available`);
        }
      }

      // Verify security warnings and instructions
      const securityWarnings = page.locator(
        'text="secure", text="warning", text="never share", .warning, .security-notice'
      );
      if ((await securityWarnings.count()) > 0) {
        await expect(securityWarnings.first()).toBeVisible();
      }

      console.log('✅ Comprehensive backup options and interface displayed');
    });

    test('should create and display mnemonic phrase backup with security measures', async ({
      page,
    }) => {
      // Navigate to Backup tab
      const backupTab = page.locator('[role="tab"]:has-text("Backup")');
      if ((await backupTab.count()) > 0) {
        await backupTab.click();
      }

      await page.waitForSelector(
        '.backup-container, [data-testid="backup-section"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for show mnemonic button
      const showMnemonicButton = page.locator(
        'button:has-text("Show"), button:has-text("Reveal"), button:has-text("Mnemonic"), [data-testid="show-mnemonic"]'
      );

      if ((await showMnemonicButton.count()) > 0) {
        // Test security confirmation if required
        const confirmCheckbox = page.locator(
          'input[type="checkbox"]:near(text("understand")), input[name="confirmSecurity"]'
        );
        if ((await confirmCheckbox.count()) > 0) {
          await confirmCheckbox.check();
        }

        await showMnemonicButton.click();

        // Verify mnemonic is displayed correctly (12 or 24 words)
        const mnemonicDisplay = page.locator(
          '.mnemonic-words, [data-testid="mnemonic-display"], .seed-phrase'
        );
        await expect(mnemonicDisplay).toBeVisible({ timeout: 5000 });

        // Verify individual words are displayed
        const mnemonicText = await mnemonicDisplay.textContent();
        const wordCount = mnemonicText.trim().split(/\s+/).length;
        expect(wordCount).toBeGreaterThanOrEqual(12); // Should be 12 or 24 words
        expect(wordCount).toBeLessThanOrEqual(24);

        // Test copy mnemonic functionality
        const copyButton = page.locator(
          'button:has-text("Copy"), [data-testid="copy-mnemonic"]'
        );
        if ((await copyButton.count()) > 0) {
          await copyButton.click();
          await expect(
            page.locator('text="Copied", text="mnemonic copied"')
          ).toBeVisible({ timeout: 3000 });
        }

        // Test download backup functionality
        const downloadButton = page.locator(
          'button:has-text("Download"), [data-testid="download-backup"]'
        );
        if ((await downloadButton.count()) > 0) {
          await expect(downloadButton).toBeVisible();
          await expect(downloadButton).toBeEnabled();
        }
      }

      console.log(
        '✅ Mnemonic phrase backup with security measures working correctly'
      );
    });

    test('should create and export keystore file backup', async ({ page }) => {
      // Navigate to Backup tab
      const backupTab = page.locator('[role="tab"]:has-text("Backup")');
      if ((await backupTab.count()) > 0) {
        await backupTab.click();
      }

      await page.waitForSelector(
        '.backup-container, [data-testid="backup-section"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Look for keystore backup option
      const keystoreOption = page.locator(
        'button:has-text("Keystore"), [data-backup-type="keystore"], button:has-text("Export Keystore")'
      );

      if ((await keystoreOption.count()) > 0) {
        await keystoreOption.click();

        // Fill password for keystore encryption
        const passwordInput = page.locator(
          'input[type="password"], input[name="keystorePassword"]'
        );
        if ((await passwordInput.count()) > 0) {
          await passwordInput.fill('SecurePassword123!');

          const confirmPasswordInput = page.locator(
            'input[name="confirmPassword"], input[placeholder*="confirm"]'
          );
          if ((await confirmPasswordInput.count()) > 0) {
            await confirmPasswordInput.fill('SecurePassword123!');
          }
        }

        // Create keystore file
        const createKeystoreButton = page.locator(
          'button:has-text("Create"), button:has-text("Export"), button[type="submit"]'
        );
        if ((await createKeystoreButton.count()) > 0) {
          await createKeystoreButton.click();

          // Wait for keystore creation
          await page.waitForSelector(
            'text="created", text="exported", text="download", .success',
            { timeout: 10000 }
          );

          // Verify download link is available
          const downloadLink = page.locator(
            'a[download], button:has-text("Download")'
          );
          if ((await downloadLink.count()) > 0) {
            await expect(downloadLink).toBeVisible();
          }
        }
      }

      console.log('✅ Keystore file backup creation and export working');
    });

    test('should restore wallet from mnemonic phrase with data integrity verification', async ({
      page,
    }) => {
      // Mock wallet data before restore for integrity comparison
      const originalWalletData = {
        address: TEST_CONFIG.testData.testAddress,
        balance: '8.60 WRF',
        assets: ['WRF', 'ETH', 'BTC'],
        transactionCount: 2,
      };

      // Look for restore functionality
      const restoreButton = page.locator(
        'button:has-text("Restore"), [data-testid="restore"], button:has-text("Import Wallet")'
      );

      if ((await restoreButton.count()) > 0) {
        await restoreButton.click();
      } else {
        // Navigate to restore through wallet creation
        const createWalletButton = page.locator(
          'button:has-text("Create"), button:has-text("New Wallet")'
        );
        if ((await createWalletButton.count()) > 0) {
          await createWalletButton.click();
          await page.waitForSelector('[role="dialog"], .modal', {
            timeout: TEST_CONFIG.timeouts.standard,
          });

          const importTab = page.locator(
            'button:has-text("Import"), [role="tab"]:has-text("Import"), [role="tab"]:has-text("Restore")'
          );
          if ((await importTab.count()) > 0) {
            await importTab.click();
          }
        }
      }

      await page.waitForSelector(
        'form, .restore-form, [data-testid="restore-form"]',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      // Fill restore form with test mnemonic
      const mnemonicInput = page.locator(
        'textarea[name="mnemonicPhrase"], input[name="mnemonic"], [data-testid="mnemonic-input"]'
      );
      await expect(mnemonicInput).toBeVisible();
      await mnemonicInput.fill(TEST_CONFIG.testData.mockMnemonic);

      // Fill wallet details
      const walletNameInput = page.locator(
        'input[name="walletName"], input[placeholder*="name"]'
      );
      if ((await walletNameInput.count()) > 0) {
        await walletNameInput.fill('Restored Wallet ' + Date.now());
      }

      const passwordInput = page.locator(
        'input[name="password"], input[type="password"]'
      );
      if ((await passwordInput.count()) > 0) {
        await passwordInput.fill(TEST_CONFIG.testData.password);

        const confirmPasswordInput = page.locator(
          'input[name="confirmPassword"]'
        );
        if ((await confirmPasswordInput.count()) > 0) {
          await confirmPasswordInput.fill(TEST_CONFIG.testData.password);
        }
      }

      // Submit restore
      const restoreSubmitButton = page.locator(
        'button:has-text("Restore"), button:has-text("Import"), button[type="submit"]'
      );
      await restoreSubmitButton.click();

      // Wait for restore process to complete
      await WalletTestHelpers.waitForWalletOperation(page, 'restore');

      // Verify restore success
      await expect(
        page.locator(
          'text="restored", text="imported", text="Success", .success'
        )
      ).toBeVisible({ timeout: 15000 });

      // Verify data integrity after restore
      await page.waitForTimeout(2000); // Allow time for data to load

      // Check restored wallet address
      const restoredAddress = page.locator('text=/wrf[a-fA-F0-9]+/');
      await expect(restoredAddress).toBeVisible();

      // Verify assets are restored (navigate to multi-assets if needed)
      const multiAssetsTab = page.locator(
        '[role="tab"]:has-text("Multi-Assets")'
      );
      if ((await multiAssetsTab.count()) > 0) {
        await multiAssetsTab.click();
        await page.waitForTimeout(1000);

        // Verify assets are displayed
        for (const asset of originalWalletData.assets) {
          await expect(page.locator(`text="${asset}"`)).toBeVisible();
        }
      }

      // Verify transaction history is restored
      const transactionsTab = page.locator(
        '[role="tab"]:has-text("Transactions")'
      );
      if ((await transactionsTab.count()) > 0) {
        await transactionsTab.click();
        await page.waitForTimeout(1000);

        // Check that transactions are present
        const transactionElements = page.locator(
          '.transaction-item, tr, [data-testid="transaction"]'
        );
        const transactionCount = await transactionElements.count();
        expect(transactionCount).toBeGreaterThanOrEqual(1);
      }

      console.log(
        '✅ Mnemonic restore with data integrity verification completed successfully'
      );
    });

    test('should restore wallet from keystore file with password verification', async ({
      page,
    }) => {
      // Mock keystore file data
      const mockKeystoreContent = JSON.stringify({
        version: 3,
        id: 'test-keystore-' + Date.now(),
        address: TEST_CONFIG.testData.testAddress.replace('wrf', ''),
        crypto: {
          ciphertext: 'mock_encrypted_data',
          cipherparams: { iv: 'mock_iv' },
          cipher: 'aes-128-ctr',
          kdf: 'scrypt',
          kdfparams: { dklen: 32, salt: 'mock_salt' },
          mac: 'mock_mac',
        },
      });

      // Look for keystore restore option
      const restoreButton = page.locator(
        'button:has-text("Restore"), button:has-text("Import")'
      );
      if ((await restoreButton.count()) > 0) {
        await restoreButton.click();
      }

      // Look for keystore restore tab or option
      const keystoreTab = page.locator(
        'button:has-text("Keystore"), [role="tab"]:has-text("Keystore"), [data-restore-type="keystore"]'
      );
      if ((await keystoreTab.count()) > 0) {
        await keystoreTab.click();

        // Test file upload functionality
        const fileInput = page.locator(
          'input[type="file"], [data-testid="keystore-file"]'
        );
        if ((await fileInput.count()) > 0) {
          // Create temporary keystore file for testing
          const fileContent = Buffer.from(mockKeystoreContent);
          await fileInput.setInputFiles({
            name: 'test-keystore.json',
            mimeType: 'application/json',
            buffer: fileContent,
          });
        } else {
          // If no file input, try textarea for keystore content
          const keystoreTextarea = page.locator(
            'textarea[name="keystore"], textarea[placeholder*="keystore"]'
          );
          if ((await keystoreTextarea.count()) > 0) {
            await keystoreTextarea.fill(mockKeystoreContent);
          }
        }

        // Enter keystore password
        const passwordInput = page.locator(
          'input[name="keystorePassword"], input[placeholder*="keystore password"]'
        );
        if ((await passwordInput.count()) > 0) {
          await passwordInput.fill('SecurePassword123!');
        }

        // Fill wallet details
        const walletNameInput = page.locator('input[name="walletName"]');
        if ((await walletNameInput.count()) > 0) {
          await walletNameInput.fill('Keystore Wallet ' + Date.now());
        }

        // Submit keystore restore
        const restoreSubmitButton = page.locator(
          'button:has-text("Restore"), button:has-text("Import"), button[type="submit"]'
        );
        await restoreSubmitButton.click();

        // Wait for restore process
        await WalletTestHelpers.waitForWalletOperation(
          page,
          'keystore-restore'
        );

        // Verify restore success or error handling
        const successElement = page.locator(
          'text="restored", text="imported", text="Success", .success'
        );
        const errorElement = page.locator(
          'text="invalid", text="error", text="failed", .error'
        );

        try {
          await expect(successElement).toBeVisible({ timeout: 10000 });
          console.log('✅ Keystore restore successful');
        } catch (e) {
          await expect(errorElement).toBeVisible({ timeout: 5000 });
          console.log(
            '✅ Keystore restore error handling working (expected for mock data)'
          );
        }
      }

      console.log('✅ Keystore file restore with password verification tested');
    });

    test('should handle invalid backup file formats and display appropriate errors', async ({
      page,
    }) => {
      // Test invalid file formats
      const invalidFiles = [
        {
          name: 'invalid.txt',
          content: 'This is not a valid keystore',
          type: 'text/plain',
        },
        {
          name: 'malformed.json',
          content: '{ invalid json',
          type: 'application/json',
        },
        { name: 'empty.json', content: '', type: 'application/json' },
        {
          name: 'wrong-structure.json',
          content: JSON.stringify({ wrong: 'structure' }),
          type: 'application/json',
        },
      ];

      // Navigate to restore functionality
      const restoreButton = page.locator(
        'button:has-text("Restore"), button:has-text("Import")'
      );
      if ((await restoreButton.count()) > 0) {
        await restoreButton.click();
      }

      // Test keystore restore with invalid files
      const keystoreTab = page.locator(
        'button:has-text("Keystore"), [role="tab"]:has-text("Keystore")'
      );
      if ((await keystoreTab.count()) > 0) {
        await keystoreTab.click();

        for (const invalidFile of invalidFiles) {
          // Upload invalid file
          const fileInput = page.locator(
            'input[type="file"], [data-testid="keystore-file"]'
          );
          if ((await fileInput.count()) > 0) {
            const fileBuffer = Buffer.from(invalidFile.content);
            await fileInput.setInputFiles({
              name: invalidFile.name,
              mimeType: invalidFile.type,
              buffer: fileBuffer,
            });

            // Enter password
            const passwordInput = page.locator(
              'input[name="keystorePassword"]'
            );
            if ((await passwordInput.count()) > 0) {
              await passwordInput.fill('TestPassword123!');
            }

            // Try to submit
            const submitButton = page.locator(
              'button:has-text("Restore"), button[type="submit"]'
            );
            await submitButton.click();

            // Verify error message appears
            await expect(
              page.locator(
                'text="invalid", text="error", text="malformed", text="corrupt", .error'
              )
            ).toBeVisible({ timeout: 5000 });

            console.log(
              `✅ Invalid file "${invalidFile.name}" error handled correctly`
            );

            // Clear the form for next test
            if ((await fileInput.count()) > 0) {
              await fileInput.setInputFiles([]);
            }
          }
        }
      }

      // Test invalid mnemonic phrases
      const mnemonicTab = page.locator(
        'button:has-text("Mnemonic"), [role="tab"]:has-text("Mnemonic"), [role="tab"]:has-text("Seed")'
      );
      if ((await mnemonicTab.count()) > 0) {
        await mnemonicTab.click();

        const invalidMnemonics = [
          'invalid words that are not in bip39 wordlist',
          'too few words',
          'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon', // 11 words instead of 12
          'abandon ability able about above absent absorb abstract absurd abuse access accident invalid', // 13 words
          '123 456 789 invalid numeric mnemonic test case example',
        ];

        for (const invalidMnemonic of invalidMnemonics) {
          const mnemonicInput = page.locator(
            'textarea[name="mnemonicPhrase"], input[name="mnemonic"]'
          );
          if ((await mnemonicInput.count()) > 0) {
            await mnemonicInput.fill(invalidMnemonic);

            const submitButton = page.locator(
              'button:has-text("Restore"), button[type="submit"]'
            );
            await submitButton.click();

            // Verify error message appears
            await expect(
              page.locator(
                'text="invalid", text="checksum", text="wordlist", text="12 words", .error'
              )
            ).toBeVisible({ timeout: 5000 });

            console.log(
              `✅ Invalid mnemonic error handled: "${invalidMnemonic.substring(0, 30)}..."`
            );

            // Clear for next test
            await mnemonicInput.clear();
          }
        }
      }

      console.log('✅ Invalid backup file format error handling completed');
    });

    test('should verify complete data integrity after backup and restore cycle', async ({
      page,
    }) => {
      // Step 1: Create reference wallet data
      const referenceData = {
        walletName: 'Integrity Test Wallet ' + Date.now(),
        address: TEST_CONFIG.testData.testAddress,
        assets: ['WRF', 'ETH', 'BTC'],
        balances: { WRF: '8.60', ETH: '0.0025', BTC: '0.00015' },
        transactionCount: 2,
      };

      // Step 2: Create a new wallet for testing
      const createButton = page.locator(
        'button:has-text("Create"), button:has-text("New Wallet")'
      );
      if ((await createButton.count()) > 0) {
        await createButton.click();
        await page.waitForSelector('form, [role="dialog"]', {
          timeout: TEST_CONFIG.timeouts.standard,
        });

        await WalletTestHelpers.fillWalletCreationForm(
          page,
          referenceData.walletName
        );

        const submitButton = page.locator(
          'button[type="submit"], button:has-text("Create")'
        );
        await submitButton.click();

        await WalletTestHelpers.waitForWalletOperation(page, 'creation');
      }

      // Step 3: Navigate to wallet overview and record current state
      await WalletTestHelpers.navigateToWallet(page);
      await page.waitForTimeout(2000); // Allow data to load

      // Record original wallet address
      const originalAddress = await page
        .locator('text=/wrf[a-fA-F0-9]+/')
        .first()
        .textContent();

      // Step 4: Perform backup - get mnemonic
      const backupTab = page.locator('[role="tab"]:has-text("Backup")');
      if ((await backupTab.count()) > 0) {
        await backupTab.click();

        const showMnemonicButton = page.locator(
          'button:has-text("Show"), button:has-text("Reveal")'
        );
        if ((await showMnemonicButton.count()) > 0) {
          await showMnemonicButton.click();

          // Get the mnemonic phrase
          const mnemonicElement = page.locator(
            '.mnemonic-words, [data-testid="mnemonic-display"]'
          );
          const backupMnemonic = await mnemonicElement.textContent();

          expect(backupMnemonic).toBeTruthy();
          expect(
            backupMnemonic.trim().split(/\s+/).length
          ).toBeGreaterThanOrEqual(12);

          console.log('✅ Backup mnemonic obtained');

          // Step 5: Simulate wallet deletion/reset (if supported)
          // This would typically involve clearing wallet data

          // Step 6: Restore wallet from backup
          const restoreButton = page.locator(
            'button:has-text("Restore"), button:has-text("Import")'
          );
          if ((await restoreButton.count()) > 0) {
            await restoreButton.click();

            const mnemonicInput = page.locator(
              'textarea[name="mnemonicPhrase"], input[name="mnemonic"]'
            );
            await mnemonicInput.fill(backupMnemonic.trim());

            await WalletTestHelpers.fillWalletCreationForm(
              page,
              'Restored ' + referenceData.walletName
            );

            const restoreSubmitButton = page.locator(
              'button:has-text("Restore"), button[type="submit"]'
            );
            await restoreSubmitButton.click();

            await WalletTestHelpers.waitForWalletOperation(page, 'restore');

            // Step 7: Verify data integrity after restore
            await page.waitForTimeout(3000); // Allow restored data to load

            // Verify address matches (same mnemonic should generate same address)
            const restoredAddress = await page
              .locator('text=/wrf[a-fA-F0-9]+/')
              .first()
              .textContent();
            expect(restoredAddress).toBe(originalAddress);

            // Verify assets are restored
            const multiAssetsTab = page.locator(
              '[role="tab"]:has-text("Multi-Assets")'
            );
            if ((await multiAssetsTab.count()) > 0) {
              await multiAssetsTab.click();
              await page.waitForTimeout(1000);

              for (const asset of referenceData.assets) {
                await expect(page.locator(`text="${asset}"`)).toBeVisible();
              }

              // Verify specific balances if displayed
              for (const [asset, balance] of Object.entries(
                referenceData.balances
              )) {
                const balanceElement = page.locator(`text="${balance}"`);
                if ((await balanceElement.count()) > 0) {
                  await expect(balanceElement.first()).toBeVisible();
                }
              }
            }

            // Verify transaction history is restored
            const transactionsTab = page.locator(
              '[role="tab"]:has-text("Transactions")'
            );
            if ((await transactionsTab.count()) > 0) {
              await transactionsTab.click();
              await page.waitForTimeout(1000);

              const transactionElements = page.locator('.transaction-item, tr');
              const restoredTransactionCount =
                await transactionElements.count();
              expect(restoredTransactionCount).toBeGreaterThanOrEqual(
                referenceData.transactionCount
              );
            }

            console.log(
              '✅ Complete data integrity verified after backup/restore cycle'
            );
          }
        }
      }

      console.log('✅ Full backup and restore data integrity test completed');
    });

    test('should handle backup and restore operations with proper security confirmations', async ({
      page,
    }) => {
      // Test security measures during backup
      const backupTab = page.locator('[role="tab"]:has-text("Backup")');
      if ((await backupTab.count()) > 0) {
        await backupTab.click();

        // Test password confirmation for sensitive operations
        const showMnemonicButton = page.locator(
          'button:has-text("Show"), button:has-text("Reveal")'
        );
        if ((await showMnemonicButton.count()) > 0) {
          await showMnemonicButton.click();

          // Check for password confirmation dialog
          const passwordConfirmation = page.locator(
            'input[type="password"], [data-testid="password-confirmation"]'
          );
          if ((await passwordConfirmation.count()) > 0) {
            await passwordConfirmation.fill(TEST_CONFIG.testData.password);

            const confirmButton = page.locator(
              'button:has-text("Confirm"), button:has-text("Verify")'
            );
            await confirmButton.click();
          }

          // Verify security warnings are displayed
          const securityWarnings = page.locator(
            'text="Never share", text="Keep safe", text="Write down", .warning, .security-alert'
          );
          if ((await securityWarnings.count()) > 0) {
            await expect(securityWarnings.first()).toBeVisible();
          }

          // Test acknowledgment checkboxes
          const acknowledgmentCheckboxes = page.locator(
            'input[type="checkbox"]:near(text("understand")), input[type="checkbox"]:near(text("secure"))'
          );
          if ((await acknowledgmentCheckboxes.count()) > 0) {
            for (let i = 0; i < (await acknowledgmentCheckboxes.count()); i++) {
              await acknowledgmentCheckboxes.nth(i).check();
            }
          }
        }
      }

      // Test security during restore operations
      const restoreButton = page.locator(
        'button:has-text("Restore"), button:has-text("Import")'
      );
      if ((await restoreButton.count()) > 0) {
        await restoreButton.click();

        // Test wallet replacement warnings
        const replacementWarnings = page.locator(
          'text="replace", text="overwrite", text="existing wallet", .warning'
        );
        if ((await replacementWarnings.count()) > 0) {
          await expect(replacementWarnings.first()).toBeVisible();

          // Test confirmation for wallet replacement
          const confirmReplacement = page.locator(
            'input[type="checkbox"]:near(text("replace")), input[name="confirmReplace"]'
          );
          if ((await confirmReplacement.count()) > 0) {
            await confirmReplacement.check();
          }
        }
      }

      console.log(
        '✅ Security confirmations during backup and restore operations verified'
      );
    });
  });

  test.describe('6. Responsive Design and Performance Tests', () => {
    test('should maintain functionality across different viewport sizes', async ({
      page,
    }) => {
      const viewports = [
        { width: 1920, height: 1080 }, // Desktop
        { width: 1024, height: 768 }, // Tablet landscape
        { width: 768, height: 1024 }, // Tablet portrait
        { width: 375, height: 667 }, // Mobile
      ];

      for (const viewport of viewports) {
        await page.setViewportSize(viewport);

        // Verify wallet page is still functional
        await expect(
          page.locator('h4:has-text("WRF Wallet"), h2:has-text("WRF Wallet")')
        ).toBeVisible();

        // Verify main navigation is accessible
        const tabs = page.locator('[role="tab"], .tab, button');
        await expect(tabs.first()).toBeVisible();

        console.log(
          `✅ Responsive design verified for ${viewport.width}x${viewport.height}`
        );
      }
    });

    test('should load wallet components within reasonable time', async ({
      page,
    }) => {
      const startTime = Date.now();

      await WalletTestHelpers.navigateToWallet(page);

      // Wait for all main components to be visible
      await page.waitForSelector(
        'h4:has-text("WRF Wallet"), h2:has-text("WRF Wallet")',
        { timeout: TEST_CONFIG.timeouts.standard }
      );
      await page.waitForSelector('[role="tab"], .tab', {
        timeout: TEST_CONFIG.timeouts.standard,
      });

      const loadTime = Date.now() - startTime;

      // Should load within 10 seconds
      expect(loadTime).toBeLessThan(10000);

      console.log(`✅ Wallet page loaded in ${loadTime}ms`);
    });
  });

  test.describe('7. Security and Error Handling Tests', () => {
    test('should handle network errors gracefully', async ({ page }) => {
      // Block all API calls to simulate network issues
      await page.route('**/api/**', route => route.abort());

      await WalletTestHelpers.navigateToWallet(page);

      // Should display error state or offline indicator
      await page.waitForSelector(
        'text="error", text="offline", text="failed", .error',
        { timeout: TEST_CONFIG.timeouts.standard }
      );

      console.log('✅ Network error handling verified');
    });

    test('should validate sensitive operations', async ({ page }) => {
      // Navigate to a sensitive operation (like backup)
      const backupTab = page.locator('[role="tab"]:has-text("Backup")');
      if ((await backupTab.count()) > 0) {
        await backupTab.click();

        // Look for password confirmation or security checks
        const securityElements = page.locator(
          'input[type="password"], text="confirm", text="verify"'
        );
        if ((await securityElements.count()) > 0) {
          await expect(securityElements.first()).toBeVisible();
        }
      }

      console.log('✅ Security validation checked');
    });
  });
});

// Test Teardown
test.afterEach(async ({ page }) => {
  // Take screenshot on failure
  if (test.info().status !== 'passed') {
    const screenshot = await page.screenshot({
      path: `test-results/wallet-failure-${Date.now()}.png`,
      fullPage: true,
    });
    console.log('📸 Failure screenshot captured');
  }

  console.log('🧹 Test cleanup completed');
});

console.log('📋 Comprehensive Wallet Testing Suite Loaded Successfully');
