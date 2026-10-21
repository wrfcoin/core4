import { test as base, expect } from '@playwright/test';
import { WalletPage } from '../page-objects/wallet/wallet-page';
import { APIHelper } from '../utils/test-helpers';

// Define custom fixtures
type WalletFixtures = {
  walletPage: WalletPage;
  apiHelper: APIHelper;
  authenticatedUser: { token: string; address: string };
};

export const test = base.extend<WalletFixtures>({
  walletPage: async ({ page }, use) => {
    const walletPage = new WalletPage(page);
    await use(walletPage);
  },

  apiHelper: async ({}, use) => {
    const apiHelper = new APIHelper();
    await use(apiHelper);
  },

  authenticatedUser: async ({ apiHelper }, use) => {
    // Create a test user and authenticate
    const testCredentials = {
      username: `test_${Date.now()}@example.com`,
      password: 'TestPassword123!',
    };

    try {
      const token = await apiHelper.authenticate(testCredentials);
      await use({
        token,
        address: '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D', // Mock wallet address
      });
    } catch (error) {
      console.warn('Authentication failed, using mock user');
      await use({
        token: 'mock_token',
        address: '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D',
      });
    }
  },
});

export { expect } from '@playwright/test';
