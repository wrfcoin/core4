/**
 * Puppeteer Script for Lighthouse CI
 * Handles authentication and complex user flows for performance testing
 */

/**
 * Main setup function for Lighthouse CI
 * @param {import('puppeteer').Browser} browser
 * @param {{url: string, flow: string}} context
 */
async function setup(browser, context) {
  const page = await browser.newPage();

  // Set viewport for consistent testing
  await page.setViewport({
    width: 1920,
    height: 1080,
    deviceScaleFactor: 1,
  });

  // Set user agent
  await page.setUserAgent(
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36'
  );

  // Enable request interception for mocking
  await page.setRequestInterception(true);

  // Handle resource optimization
  page.on('request', request => {
    const resourceType = request.resourceType();

    // Block unnecessary resources in CI
    if (['font', 'media'].includes(resourceType)) {
      request.abort();
    } else {
      request.continue();
    }
  });

  // Handle authentication if needed
  if (context.url.includes('/wallet') || context.url.includes('/governance')) {
    await handleAuthentication(page);
  }

  // Handle specific page setups
  if (context.url.includes('/nft')) {
    await setupNFTPage(page);
  } else if (context.url.includes('/defi')) {
    await setupDeFiPage(page);
  } else if (context.url.includes('/carbon')) {
    await setupCarbonPage(page);
  }

  return page;
}

/**
 * Handle user authentication for protected pages
 */
async function handleAuthentication(page) {
  try {
    // Set mock authentication token
    await page.evaluateOnNewDocument(() => {
      localStorage.setItem('auth_token', 'mock_test_token');
      localStorage.setItem('user_id', 'test_user_123');
      localStorage.setItem('wallet_connected', 'true');
    });

    // Set mock wallet connection
    await page.evaluateOnNewDocument(() => {
      window.ethereum = {
        isMetaMask: true,
        selectedAddress: '0x742d35Cc6634C0532925a3b8D4B9A4fc8E5d0D8a',
        request: async () => ['0x742d35Cc6634C0532925a3b8D4B9A4fc8E5d0D8a'],
        on: () => {},
        removeListener: () => {},
      };
    });
  } catch (error) {
    console.warn('Authentication setup failed:', error.message);
  }
}

/**
 * Setup NFT marketplace page for testing
 */
async function setupNFTPage(page) {
  try {
    // Mock NFT data to prevent loading delays
    await page.evaluateOnNewDocument(() => {
      window.__INITIAL_NFT_DATA__ = {
        featured: [
          {
            id: '1',
            name: 'Weather Station NFT #001',
            image:
              'data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjAwIiBoZWlnaHQ9IjIwMCI+PC9zdmc+',
            price: '0.1',
            owner: '0x742d35Cc6634C0532925a3b8D4B9A4fc8E5d0D8a',
          },
        ],
        recent: [],
      };
    });
  } catch (error) {
    console.warn('NFT page setup failed:', error.message);
  }
}

/**
 * Setup DeFi page for testing
 */
async function setupDeFiPage(page) {
  try {
    // Mock DeFi data
    await page.evaluateOnNewDocument(() => {
      window.__INITIAL_DEFI_DATA__ = {
        pools: [
          {
            id: 'WRF-USDC',
            totalLiquidity: '1000000',
            apr: '12.5',
            userLiquidity: '0',
          },
        ],
        userBalance: {
          WRF: '1000',
          USDC: '500',
        },
      };
    });
  } catch (error) {
    console.warn('DeFi page setup failed:', error.message);
  }
}

/**
 * Setup Carbon Credits page for testing
 */
async function setupCarbonPage(page) {
  try {
    // Mock carbon credit data
    await page.evaluateOnNewDocument(() => {
      window.__INITIAL_CARBON_DATA__ = {
        credits: [
          {
            id: 'CC001',
            project: 'Reforestation Project',
            amount: '100',
            price: '25',
            verified: true,
          },
        ],
        userCredits: [],
      };
    });
  } catch (error) {
    console.warn('Carbon page setup failed:', error.message);
  }
}

/**
 * Cleanup function (optional)
 */
async function cleanup(browser, context) {
  // Any cleanup needed after lighthouse run
  console.log('Lighthouse test completed for:', context.url);
}

module.exports = {
  setup,
  cleanup,
};
