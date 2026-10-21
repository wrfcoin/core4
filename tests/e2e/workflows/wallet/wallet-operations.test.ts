import { test, expect } from '../../fixtures/wallet-fixtures';
import { TEST_CONSTANTS } from '../../test-data/test-constants';
import { WalletTestUtils, WaitUtils } from '../../utils/test-helpers';

test.describe('Wallet Operations E2E Tests', () => {
  test.beforeEach(async ({ walletPage }) => {
    await walletPage.gotoWallet();
  });

  test('should connect wallet successfully', async ({ walletPage }) => {
    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Verify wallet connection', async () => {
      await walletPage.assertWalletConnected();

      const walletAddress = await walletPage.getWalletAddress();
      expect(walletAddress).toMatch(/^0x[a-fA-F0-9]{40}$/);
    });
  });

  test('should display wallet balance correctly', async ({ walletPage }) => {
    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Verify balance display', async () => {
      const balance = await walletPage.getWalletBalance();
      expect(balance).toBeTruthy();
      expect(parseFloat(balance)).toBeGreaterThanOrEqual(0);
    });
  });

  test('should send transaction successfully', async ({
    walletPage,
    authenticatedUser,
  }) => {
    const recipientAddress = TEST_CONSTANTS.TEST_WALLETS.SECONDARY;
    const sendAmount = '1.0';

    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Send transaction', async () => {
      await walletPage.sendTransaction(recipientAddress, sendAmount, 'WRF');
    });

    await test.step('Verify transaction in history', async () => {
      const transactions = await walletPage.getTransactionHistory();
      expect(transactions.length).toBeGreaterThan(0);

      const latestTransaction = transactions[0];
      expect(latestTransaction.amount).toContain(sendAmount);
      expect(latestTransaction.status).toBe('confirmed');
    });
  });

  test('should handle insufficient balance error', async ({ walletPage }) => {
    const recipientAddress = TEST_CONSTANTS.TEST_WALLETS.SECONDARY;
    const largeAmount = '999999999';

    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Attempt to send large amount', async () => {
      // This should trigger an insufficient balance error
      try {
        await walletPage.sendTransaction(recipientAddress, largeAmount, 'WRF');
      } catch (error) {
        // Expected to fail
      }
    });

    await test.step('Verify error message', async () => {
      await walletPage.assertErrorMessage(
        TEST_CONSTANTS.ERROR_MESSAGES.INSUFFICIENT_BALANCE
      );
    });
  });

  test('should add custom token successfully', async ({ walletPage }) => {
    const customToken = {
      address: '0x1234567890123456789012345678901234567890',
      symbol: 'TEST',
      decimals: '18',
    };

    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Add custom token', async () => {
      await walletPage.addToken(
        customToken.address,
        customToken.symbol,
        customToken.decimals
      );
    });

    await test.step('Verify token was added', async () => {
      await walletPage.assertSuccessMessage('Token added successfully');

      // Verify token appears in token list
      const tokenList = walletPage.tokenList;
      await expect(
        tokenList.locator(`text=${customToken.symbol}`)
      ).toBeVisible();
    });
  });

  test('should display and copy wallet address', async ({ walletPage }) => {
    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Copy wallet address', async () => {
      await walletPage.copyWalletAddress();
    });

    await test.step('Verify copy success message', async () => {
      await walletPage.assertSuccessMessage('Address copied');
    });
  });

  test('should disconnect wallet properly', async ({ walletPage }) => {
    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
      await walletPage.assertWalletConnected();
    });

    await test.step('Disconnect wallet', async () => {
      await walletPage.disconnectWallet();
    });

    await test.step('Verify wallet disconnection', async () => {
      await walletPage.assertWalletDisconnected();
    });
  });

  test('should handle network switching', async ({ walletPage, page }) => {
    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Switch network', async () => {
      // Mock network switch request
      await page.evaluate(() => {
        // Simulate MetaMask network switch
        window.dispatchEvent(
          new CustomEvent('ethereum#chainChanged', {
            detail: { chainId: '0x89' }, // Polygon
          })
        );
      });
    });

    await test.step('Verify network switch handling', async () => {
      // Check if UI updates to reflect network change
      const networkIndicator = page.locator(
        '[data-testid="network-indicator"]'
      );
      await expect(networkIndicator).toContainText('Polygon');
    });
  });

  test('should maintain wallet connection across page navigation', async ({
    walletPage,
  }) => {
    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
      await walletPage.assertWalletConnected();
    });

    await test.step('Navigate to different page', async () => {
      await walletPage.navigateTo('defi');
      await expect(walletPage.page).toHaveURL(/.*\/defi/);
    });

    await test.step('Navigate back to wallet', async () => {
      await walletPage.navigateTo('wallet');
      await expect(walletPage.page).toHaveURL(/.*\/wallet/);
    });

    await test.step('Verify wallet still connected', async () => {
      await walletPage.assertWalletConnected();
    });
  });

  test('should handle transaction confirmation states', async ({
    walletPage,
  }) => {
    const recipientAddress = TEST_CONSTANTS.TEST_WALLETS.SECONDARY;
    const sendAmount = '0.1';

    await test.step('Connect wallet', async () => {
      await walletPage.connectWallet();
    });

    await test.step('Start transaction', async () => {
      await walletPage.sendTransaction(recipientAddress, sendAmount, 'WRF');
    });

    await test.step('Wait for transaction confirmation', async () => {
      // Get the transaction hash from the latest transaction
      const transactions = await walletPage.getTransactionHistory();
      const latestTransactionHash = transactions[0].hash;

      await walletPage.waitForTransactionConfirmation(latestTransactionHash);
    });

    await test.step('Verify final transaction state', async () => {
      const transactions = await walletPage.getTransactionHistory();
      const confirmedTransaction = transactions.find(
        tx => tx.status === 'confirmed'
      );
      expect(confirmedTransaction).toBeTruthy();
    });
  });
});
