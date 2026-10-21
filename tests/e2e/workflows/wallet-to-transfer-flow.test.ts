import { test, expect } from '../fixtures/wallet-fixtures';
import { TEST_CONSTANTS } from '../test-data/test-constants';
import {
  WalletTestUtils,
  WeatherTestUtils,
  WaitUtils,
  AssertionUtils,
} from '../utils/test-helpers';
import { faker } from '@faker-js/faker';

/**
 * WRFCoin E2E Test Suite: Wallet Creation to Token Transfer Flow
 *
 * This test suite covers the complete wallet-to-trading flow including:
 * 1. Wallet Creation & Setup (BIP-39/44 compatibility, multi-sig, hardware integration)
 * 2. Token Operations (balance verification, transfers, approvals, cross-chain)
 * 3. Transaction Verification (history, balance updates, gas handling)
 * 4. Security & Edge Cases (invalid addresses, insufficient balance, network issues)
 */

test.describe('Wallet Creation to Token Transfer Flow', () => {
  let testData: any;

  test.beforeEach(async ({ page, walletPage }) => {
    // Initialize test data for each test
    testData = {
      newWallet: WalletTestUtils.generateTestWalletData(),
      testUser: TEST_CONSTANTS.TEST_USERS.REGULAR_USER,
      transferAmount: '10.5',
      recipients: [
        TEST_CONSTANTS.TEST_WALLETS.PRIMARY,
        TEST_CONSTANTS.TEST_WALLETS.SECONDARY,
        TEST_CONSTANTS.TEST_WALLETS.MULTISIG,
      ],
    };

    await walletPage.gotoWallet();
    await WaitUtils.waitForPageLoad(page);
  });

  test.describe('Wallet Creation & Setup', () => {
    test('should create new wallet with BIP-39 seed phrase generation', async ({
      page,
      walletPage,
    }) => {
      await test.step('Navigate to wallet creation', async () => {
        await page.click('[data-testid="create-new-wallet-button"]');
        await expect(
          page.locator('[data-testid="wallet-creation-modal"]')
        ).toBeVisible();
      });

      await test.step('Generate BIP-39 seed phrase', async () => {
        await page.click('[data-testid="generate-seed-button"]');

        // Verify 12-word seed phrase is generated
        const seedPhraseElements = page.locator('[data-testid="seed-word"]');
        await expect(seedPhraseElements).toHaveCount(12);

        // Collect seed words for verification
        const seedWords: string[] = [];
        for (let i = 0; i < 12; i++) {
          const word = await seedPhraseElements.nth(i).textContent();
          expect(word).toBeTruthy();
          seedWords.push(word!.trim());
        }

        testData.seedPhrase = seedWords.join(' ');
      });

      await test.step('Confirm seed phrase backup', async () => {
        await page.check('[data-testid="seed-backup-confirmed"]');
        await page.click('[data-testid="continue-to-verification"]');

        // Verify seed phrase by selecting words in random order
        const verificationIndices = [2, 5, 8, 11]; // Random positions
        const seedWords = testData.seedPhrase.split(' ');

        for (const index of verificationIndices) {
          const expectedWord = seedWords[index];
          await page.click(`[data-testid="verification-word-${index}"]`);
          await page.click(`[data-testid="word-option-${expectedWord}"]`);
        }

        await page.click('[data-testid="verify-seed-button"]');
        await expect(
          page.locator('[data-testid="seed-verified"]')
        ).toBeVisible();
      });

      await test.step('Set wallet password and create wallet', async () => {
        await page.fill(
          '[data-testid="wallet-password-input"]',
          'SecurePassword123!'
        );
        await page.fill(
          '[data-testid="wallet-password-confirm"]',
          'SecurePassword123!'
        );
        await page.click('[data-testid="create-wallet-button"]');

        // Wait for wallet creation confirmation
        await expect(
          page.locator('[data-testid="wallet-created-success"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="wallet-address"]')
        ).toBeVisible();

        // Verify wallet address format (Ethereum format)
        const walletAddress = await page.textContent(
          '[data-testid="wallet-address"]'
        );
        expect(walletAddress).toMatch(/^0x[a-fA-F0-9]{40}$/);
        testData.newWalletAddress = walletAddress;
      });

      await test.step('Verify wallet appears in dashboard', async () => {
        await page.click('[data-testid="close-creation-modal"]');
        await walletPage.assertWalletConnected();

        const displayedAddress = await walletPage.getWalletAddress();
        expect(displayedAddress).toBe(testData.newWalletAddress);
      });
    });

    test('should import wallet from existing seed phrase', async ({
      page,
      walletPage,
    }) => {
      const existingSeedPhrase =
        'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
      const expectedAddress = '0x9858EfFD232B4033E47d90003D41EC34EcaEda94'; // Known address for test seed

      await test.step('Navigate to wallet import', async () => {
        await page.click('[data-testid="import-wallet-button"]');
        await expect(
          page.locator('[data-testid="import-wallet-modal"]')
        ).toBeVisible();
      });

      await test.step('Select seed phrase import method', async () => {
        await page.click('[data-testid="import-method-seed"]');
        await expect(
          page.locator('[data-testid="seed-phrase-input-area"]')
        ).toBeVisible();
      });

      await test.step('Enter seed phrase', async () => {
        await page.fill(
          '[data-testid="seed-phrase-textarea"]',
          existingSeedPhrase
        );

        // Verify seed phrase validation
        await expect(
          page.locator('[data-testid="seed-validation-success"]')
        ).toBeVisible();

        // Check derived address preview
        const previewAddress = await page.textContent(
          '[data-testid="derived-address-preview"]'
        );
        expect(previewAddress).toBe(expectedAddress);
      });

      await test.step('Complete wallet import', async () => {
        await page.fill(
          '[data-testid="wallet-password-input"]',
          'ImportPassword123!'
        );
        await page.fill(
          '[data-testid="wallet-password-confirm"]',
          'ImportPassword123!'
        );
        await page.fill(
          '[data-testid="wallet-name-input"]',
          'Imported Test Wallet'
        );

        await page.click('[data-testid="import-wallet-button"]');
        await expect(
          page.locator('[data-testid="wallet-imported-success"]')
        ).toBeVisible();
      });

      await test.step('Verify imported wallet functionality', async () => {
        await page.click('[data-testid="close-import-modal"]');
        await walletPage.assertWalletConnected();

        const walletAddress = await walletPage.getWalletAddress();
        expect(walletAddress).toBe(expectedAddress);
      });
    });

    test('should import wallet from private key', async ({
      page,
      walletPage,
    }) => {
      const testPrivateKey =
        '0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318';
      const expectedAddress = '0x2c7536E3605D9C16a7a3D7b1898e529396a65c23'; // Known address for test private key

      await test.step('Navigate to private key import', async () => {
        await page.click('[data-testid="import-wallet-button"]');
        await page.click('[data-testid="import-method-private-key"]');
        await expect(
          page.locator('[data-testid="private-key-input-area"]')
        ).toBeVisible();
      });

      await test.step('Enter private key', async () => {
        await page.fill('[data-testid="private-key-input"]', testPrivateKey);

        // Verify address derivation
        const previewAddress = await page.textContent(
          '[data-testid="derived-address-preview"]'
        );
        expect(previewAddress).toBe(expectedAddress);
      });

      await test.step('Complete import and verify', async () => {
        await page.fill(
          '[data-testid="wallet-password-input"]',
          'PrivateKeyPassword123!'
        );
        await page.fill(
          '[data-testid="wallet-password-confirm"]',
          'PrivateKeyPassword123!'
        );
        await page.fill(
          '[data-testid="wallet-name-input"]',
          'Private Key Wallet'
        );

        await page.click('[data-testid="import-private-key-button"]');
        await expect(
          page.locator('[data-testid="wallet-imported-success"]')
        ).toBeVisible();

        await page.click('[data-testid="close-import-modal"]');
        await walletPage.assertWalletConnected();

        const walletAddress = await walletPage.getWalletAddress();
        expect(walletAddress).toBe(expectedAddress);
      });
    });

    test('should create multi-signature wallet', async ({
      page,
      walletPage,
    }) => {
      const cosignerAddresses = [
        '0x742d35Cc6634C0532925a3b8D1c9e37C2384C24D',
        '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045',
        '0x8ba1f109551bD432803012645Hac136c2ad2b6f5',
      ];

      await test.step('Navigate to multisig creation', async () => {
        await page.click('[data-testid="create-multisig-wallet"]');
        await expect(
          page.locator('[data-testid="multisig-creation-modal"]')
        ).toBeVisible();
      });

      await test.step('Configure multisig parameters', async () => {
        // Set required signatures (2 of 3)
        await page.selectOption('[data-testid="required-signatures"]', '2');
        await page.selectOption('[data-testid="total-signers"]', '3');

        // Add cosigner addresses
        for (let i = 0; i < cosignerAddresses.length; i++) {
          await page.fill(
            `[data-testid="cosigner-address-${i}"]`,
            cosignerAddresses[i]
          );

          // Verify address validation
          await expect(
            page.locator(`[data-testid="address-valid-${i}"]`)
          ).toBeVisible();
        }

        await page.fill(
          '[data-testid="multisig-name"]',
          'Test MultiSig Wallet'
        );
      });

      await test.step('Deploy multisig contract', async () => {
        await page.click('[data-testid="deploy-multisig-button"]');

        // Wait for contract deployment
        await expect(
          page.locator('[data-testid="deployment-pending"]')
        ).toBeVisible();
        await WaitUtils.waitForTransaction(page);
        await expect(
          page.locator('[data-testid="multisig-deployed-success"]')
        ).toBeVisible();

        // Verify contract address is generated
        const contractAddress = await page.textContent(
          '[data-testid="multisig-contract-address"]'
        );
        expect(contractAddress).toMatch(/^0x[a-fA-F0-9]{40}$/);
        testData.multisigAddress = contractAddress;
      });

      await test.step('Verify multisig wallet functionality', async () => {
        await page.click('[data-testid="close-multisig-modal"]');

        // Switch to multisig wallet
        await page.click('[data-testid="wallet-selector"]');
        await page.click(
          `[data-testid="wallet-option-${testData.multisigAddress}"]`
        );

        await walletPage.assertWalletConnected();
        const walletAddress = await walletPage.getWalletAddress();
        expect(walletAddress).toBe(testData.multisigAddress);

        // Verify multisig indicators
        await expect(
          page.locator('[data-testid="multisig-indicator"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="required-signatures-display"]')
        ).toContainText('2 of 3');
      });
    });

    test('should integrate with hardware wallet (simulated)', async ({
      page,
      walletPage,
    }) => {
      await test.step('Navigate to hardware wallet connection', async () => {
        await page.click('[data-testid="connect-hardware-wallet"]');
        await expect(
          page.locator('[data-testid="hardware-wallet-modal"]')
        ).toBeVisible();
      });

      await test.step('Select Ledger device', async () => {
        await page.click('[data-testid="hardware-type-ledger"]');
        await expect(
          page.locator('[data-testid="ledger-connection-screen"]')
        ).toBeVisible();
      });

      await test.step('Simulate hardware wallet connection', async () => {
        // Mock hardware wallet connection for testing
        await page.evaluate(() => {
          window.dispatchEvent(
            new CustomEvent('hardware-wallet-connected', {
              detail: {
                type: 'ledger',
                address: '0x1234567890123456789012345678901234567890',
                deviceId: 'ledger-device-001',
              },
            })
          );
        });

        await expect(
          page.locator('[data-testid="hardware-connected"]')
        ).toBeVisible();

        const hwAddress = await page.textContent(
          '[data-testid="hardware-wallet-address"]'
        );
        expect(hwAddress).toMatch(/^0x[a-fA-F0-9]{40}$/);
      });

      await test.step('Complete hardware wallet setup', async () => {
        await page.fill(
          '[data-testid="hardware-wallet-name"]',
          'My Ledger Wallet'
        );
        await page.click('[data-testid="save-hardware-wallet"]');

        await expect(
          page.locator('[data-testid="hardware-wallet-saved"]')
        ).toBeVisible();
        await page.click('[data-testid="close-hardware-modal"]');

        // Verify hardware wallet appears in wallet list
        await page.click('[data-testid="wallet-selector"]');
        await expect(
          page.locator('[data-testid="wallet-option-ledger"]')
        ).toBeVisible();
      });

      await test.step('Test hardware wallet transaction signing', async () => {
        // Switch to hardware wallet
        await page.click('[data-testid="wallet-option-ledger"]');
        await walletPage.assertWalletConnected();

        // Initiate a transaction to test hardware signing
        await walletPage.sendButton.click();
        await expect(walletPage.sendModal).toBeVisible();

        await walletPage.recipientAddressInput.fill(testData.recipients[0]);
        await walletPage.sendAmountInput.fill('1.0');
        await walletPage.confirmSendButton.click();

        // Hardware wallet signing simulation
        await expect(
          page.locator('[data-testid="hardware-signing-pending"]')
        ).toBeVisible();

        // Mock hardware signature approval
        await page.evaluate(() => {
          window.dispatchEvent(
            new CustomEvent('hardware-signature-approved', {
              detail: { signature: '0xmocksignature...' },
            })
          );
        });

        await expect(
          page.locator('[data-testid="hardware-transaction-signed"]')
        ).toBeVisible();
      });
    });
  });

  test.describe('Token Operations', () => {
    test.beforeEach(async ({ walletPage }) => {
      // Connect wallet for token operations
      await walletPage.connectWallet();
      await walletPage.assertWalletConnected();
    });

    test('should verify and display native WRF token balance', async ({
      page,
      walletPage,
    }) => {
      await test.step('Verify WRF token appears in balance', async () => {
        const balance = await walletPage.getWalletBalance();
        expect(balance).toBeTruthy();

        // Check if balance is displayed in correct format
        expect(balance).toMatch(/^\d+(\.\d+)?\s*(WRF|ETH)$/);
      });

      await test.step('Verify token list contains WRF', async () => {
        await expect(walletPage.tokenList).toBeVisible();
        await expect(page.locator('[data-testid="token-wrf"]')).toBeVisible();

        const wrfBalance = await page.textContent(
          '[data-testid="token-wrf-balance"]'
        );
        expect(wrfBalance).toBeTruthy();
        expect(parseFloat(wrfBalance!.split(' ')[0])).toBeGreaterThanOrEqual(0);
      });

      await test.step('Verify balance updates with refresh', async () => {
        const initialBalance = await walletPage.getWalletBalance();

        await page.click('[data-testid="refresh-balance-button"]');
        await expect(
          page.locator('[data-testid="balance-refreshing"]')
        ).toBeVisible();

        // Wait for balance refresh
        await page.waitForTimeout(2000);

        const updatedBalance = await walletPage.getWalletBalance();
        expect(updatedBalance).toBeTruthy();
      });
    });

    test('should perform token transfer between wallets', async ({
      page,
      walletPage,
    }) => {
      const transferData = {
        recipient: testData.recipients[0],
        amount: testData.transferAmount,
        token: 'WRF',
      };

      await test.step('Initiate token transfer', async () => {
        const initialBalance = await walletPage.getWalletBalance();
        testData.initialBalance = parseFloat(initialBalance.split(' ')[0]);

        await walletPage.sendTransaction(
          transferData.recipient,
          transferData.amount,
          transferData.token
        );
      });

      await test.step('Verify transaction appears in history', async () => {
        const transactions = await walletPage.getTransactionHistory();
        expect(transactions.length).toBeGreaterThan(0);

        const latestTx = transactions[0];
        expect(latestTx.amount).toContain(transferData.amount);
        expect(latestTx.status).toMatch(/pending|confirmed/);

        testData.transactionHash = latestTx.hash;
      });

      await test.step('Wait for transaction confirmation', async () => {
        await walletPage.waitForTransactionConfirmation(
          testData.transactionHash
        );

        // Verify transaction status updated to confirmed
        const transactions = await walletPage.getTransactionHistory();
        const confirmedTx = transactions.find(
          tx => tx.hash === testData.transactionHash
        );
        expect(confirmedTx?.status).toBe('confirmed');
      });

      await test.step('Verify balance updated after transfer', async () => {
        const currentBalance = await walletPage.getWalletBalance();
        const newBalance = parseFloat(currentBalance.split(' ')[0]);

        // Account for transaction amount and gas fees
        expect(newBalance).toBeLessThan(testData.initialBalance);

        // Approximate check accounting for gas fees
        const expectedBalance =
          testData.initialBalance - parseFloat(transferData.amount);
        expect(newBalance).toBeLessThan(expectedBalance + 1); // Allow for gas fees
      });
    });

    test('should handle token approvals for smart contracts', async ({
      page,
      walletPage,
    }) => {
      const spenderAddress = '0xDeFiContractAddress123456789012345678901234';
      const approvalAmount = '100';

      await test.step('Navigate to token approvals', async () => {
        await page.click('[data-testid="token-approvals-button"]');
        await expect(
          page.locator('[data-testid="approvals-modal"]')
        ).toBeVisible();
      });

      await test.step('Create new token approval', async () => {
        await page.click('[data-testid="add-approval-button"]');

        await page.fill(
          '[data-testid="spender-address-input"]',
          spenderAddress
        );
        await page.fill(
          '[data-testid="approval-amount-input"]',
          approvalAmount
        );
        await page.selectOption('[data-testid="approval-token-select"]', 'WRF');

        await page.click('[data-testid="approve-button"]');
        await WaitUtils.waitForTransaction(page);

        await expect(
          page.locator('[data-testid="approval-success"]')
        ).toBeVisible();
      });

      await test.step('Verify approval in allowances list', async () => {
        const allowanceRow = page.locator(
          `[data-testid="allowance-${spenderAddress}"]`
        );
        await expect(allowanceRow).toBeVisible();

        const allowanceAmount = await allowanceRow
          .locator('[data-testid="allowance-amount"]')
          .textContent();
        expect(allowanceAmount).toContain(approvalAmount);
      });

      await test.step('Modify existing approval', async () => {
        const updatedAmount = '200';

        await page.click(`[data-testid="modify-allowance-${spenderAddress}"]`);
        await page.fill('[data-testid="new-allowance-amount"]', updatedAmount);
        await page.click('[data-testid="update-allowance-button"]');

        await WaitUtils.waitForTransaction(page);
        await expect(
          page.locator('[data-testid="allowance-updated"]')
        ).toBeVisible();
      });

      await test.step('Revoke token approval', async () => {
        await page.click(`[data-testid="revoke-allowance-${spenderAddress}"]`);
        await page.click('[data-testid="confirm-revoke-button"]');

        await WaitUtils.waitForTransaction(page);

        // Verify approval is removed from list
        await expect(
          page.locator(`[data-testid="allowance-${spenderAddress}"]`)
        ).not.toBeVisible();
      });
    });

    test('should perform cross-chain token transfers via bridges', async ({
      page,
      walletPage,
      apiHelper,
    }) => {
      const bridgeTransfer = {
        sourceChain: 'ethereum',
        destinationChain: 'polygon',
        amount: '5.0',
        token: 'WRF',
      };

      await test.step('Navigate to cross-chain bridge', async () => {
        await page.click('[data-testid="cross-chain-bridge-button"]');
        await expect(
          page.locator('[data-testid="bridge-interface"]')
        ).toBeVisible();
      });

      await test.step('Configure bridge transfer', async () => {
        await page.selectOption(
          '[data-testid="source-chain-select"]',
          bridgeTransfer.sourceChain
        );
        await page.selectOption(
          '[data-testid="destination-chain-select"]',
          bridgeTransfer.destinationChain
        );
        await page.selectOption(
          '[data-testid="bridge-token-select"]',
          bridgeTransfer.token
        );

        await page.fill(
          '[data-testid="bridge-amount-input"]',
          bridgeTransfer.amount
        );

        // Verify bridge route and fees
        await expect(
          page.locator('[data-testid="bridge-route"]')
        ).toBeVisible();
        await expect(page.locator('[data-testid="bridge-fee"]')).toBeVisible();

        const estimatedFee = await page.textContent(
          '[data-testid="bridge-fee"]'
        );
        expect(estimatedFee).toMatch(/^\d+(\.\d+)?\s*\w+$/);
      });

      await test.step('Execute cross-chain transfer', async () => {
        await page.click('[data-testid="initiate-bridge-button"]');

        // Confirm bridge transaction
        await expect(
          page.locator('[data-testid="bridge-confirmation-modal"]')
        ).toBeVisible();
        await page.click('[data-testid="confirm-bridge-button"]');

        // Wait for source chain transaction
        await WaitUtils.waitForTransaction(page);
        await expect(
          page.locator('[data-testid="source-tx-confirmed"]')
        ).toBeVisible();

        const sourceTxHash = await page.textContent(
          '[data-testid="source-tx-hash"]'
        );
        expect(sourceTxHash).toMatch(/^0x[a-fA-F0-9]{64}$/);
      });

      await test.step('Monitor bridge completion', async () => {
        // Bridge should show progress
        await expect(
          page.locator('[data-testid="bridge-in-progress"]')
        ).toBeVisible();

        // Wait for destination chain confirmation (may take longer in real scenarios)
        await page.waitForTimeout(30000); // Extended timeout for bridge completion

        // Check bridge status via API
        const bridgeStatus = await apiHelper.checkBridgeStatus(sourceTxHash);
        expect(['pending', 'completed']).toContain(bridgeStatus.status);

        if (bridgeStatus.status === 'completed') {
          await expect(
            page.locator('[data-testid="bridge-completed"]')
          ).toBeVisible();
          const destTxHash = await page.textContent(
            '[data-testid="destination-tx-hash"]'
          );
          expect(destTxHash).toMatch(/^0x[a-fA-F0-9]{64}$/);
        }
      });
    });
  });

  test.describe('Transaction Verification', () => {
    test.beforeEach(async ({ walletPage }) => {
      await walletPage.connectWallet();
    });

    test('should display comprehensive transaction history', async ({
      page,
      walletPage,
    }) => {
      await test.step('Verify transaction history interface', async () => {
        await expect(walletPage.transactionHistory).toBeVisible();

        const transactions = await walletPage.getTransactionHistory();
        if (transactions.length > 0) {
          // Check transaction data structure
          const firstTx = transactions[0];
          expect(firstTx.hash).toMatch(/^0x[a-fA-F0-9]{64}$/);
          expect(firstTx.amount).toBeTruthy();
          expect(['pending', 'confirmed', 'failed']).toContain(firstTx.status);
        }
      });

      await test.step('Verify transaction chronological ordering', async () => {
        const transactions = await walletPage.getTransactionHistory();

        if (transactions.length > 1) {
          // Transactions should be ordered by timestamp (newest first)
          for (let i = 0; i < transactions.length - 1; i++) {
            const currentTx = page
              .locator('[data-testid="transaction-row"]')
              .nth(i);
            const nextTx = page
              .locator('[data-testid="transaction-row"]')
              .nth(i + 1);

            const currentTime = await currentTx
              .locator('[data-testid="transaction-timestamp"]')
              .textContent();
            const nextTime = await nextTx
              .locator('[data-testid="transaction-timestamp"]')
              .textContent();

            expect(new Date(currentTime!).getTime()).toBeGreaterThanOrEqual(
              new Date(nextTime!).getTime()
            );
          }
        }
      });

      await test.step('Test transaction detail expansion', async () => {
        const transactions = await walletPage.getTransactionHistory();

        if (transactions.length > 0) {
          const firstTxRow = page
            .locator('[data-testid="transaction-row"]')
            .first();
          await firstTxRow.click();

          // Verify detailed transaction information
          await expect(
            page.locator('[data-testid="transaction-details"]')
          ).toBeVisible();
          await expect(page.locator('[data-testid="gas-used"]')).toBeVisible();
          await expect(page.locator('[data-testid="gas-price"]')).toBeVisible();
          await expect(
            page.locator('[data-testid="block-number"]')
          ).toBeVisible();
          await expect(
            page.locator('[data-testid="transaction-fee"]')
          ).toBeVisible();
        }
      });
    });

    test('should accurately track balance updates after transactions', async ({
      page,
      walletPage,
    }) => {
      await test.step('Record initial balance', async () => {
        const initialBalance = await walletPage.getWalletBalance();
        testData.initialBalance = parseFloat(initialBalance.split(' ')[0]);
      });

      await test.step('Perform test transaction', async () => {
        const transferAmount = '1.0';
        const recipient = testData.recipients[0];

        await walletPage.sendTransaction(recipient, transferAmount, 'WRF');

        // Wait for transaction confirmation
        const transactions = await walletPage.getTransactionHistory();
        const latestTx = transactions[0];
        await walletPage.waitForTransactionConfirmation(latestTx.hash);
      });

      await test.step('Verify balance decrease', async () => {
        const newBalance = await walletPage.getWalletBalance();
        const currentBalance = parseFloat(newBalance.split(' ')[0]);

        // Balance should decrease by at least the transfer amount (plus gas)
        expect(currentBalance).toBeLessThan(testData.initialBalance);
        expect(testData.initialBalance - currentBalance).toBeGreaterThan(0.9); // Account for gas fees
      });

      await test.step('Verify transaction fee calculation', async () => {
        const transactions = await walletPage.getTransactionHistory();
        const latestTx = transactions[0];

        // Click on transaction to see details
        const txRow = page.locator(
          `[data-testid="transaction-row"][data-hash="${latestTx.hash}"]`
        );
        await txRow.click();

        const gasUsed = await page.textContent('[data-testid="gas-used"]');
        const gasPrice = await page.textContent('[data-testid="gas-price"]');
        const txFee = await page.textContent('[data-testid="transaction-fee"]');

        expect(gasUsed).toMatch(/^\d+$/);
        expect(gasPrice).toMatch(/^\d+(\.\d+)?/);
        expect(txFee).toMatch(/^\d+(\.\d+)?\s*\w+$/);
      });
    });

    test('should handle failed transaction scenarios', async ({
      page,
      walletPage,
    }) => {
      await test.step('Simulate failed transaction with insufficient gas', async () => {
        // Mock insufficient gas scenario by setting gas limit too low
        await page.evaluate(() => {
          window.localStorage.setItem('mockInsufficientGas', 'true');
        });

        try {
          await walletPage.sendTransaction(
            testData.recipients[0],
            '1.0',
            'WRF'
          );
        } catch (error) {
          // Expected to fail due to insufficient gas
        }
      });

      await test.step('Verify failed transaction handling', async () => {
        // Check if error is properly displayed
        await expect(
          page.locator('[data-testid="transaction-error"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="error-message"]')
        ).toContainText('insufficient gas');

        // Clear mock
        await page.evaluate(() => {
          window.localStorage.removeItem('mockInsufficientGas');
        });
      });

      await test.step('Test transaction retry mechanism', async () => {
        await page.click('[data-testid="retry-transaction-button"]');

        // Should open send modal with previous values pre-filled
        await expect(walletPage.sendModal).toBeVisible();
        expect(await walletPage.recipientAddressInput.inputValue()).toBe(
          testData.recipients[0]
        );
        expect(await walletPage.sendAmountInput.inputValue()).toBe('1.0');

        await page.click('[data-testid="cancel-send-button"]');
      });
    });
  });

  test.describe('Security & Edge Cases', () => {
    test.beforeEach(async ({ walletPage }) => {
      await walletPage.connectWallet();
    });

    test('should handle invalid wallet addresses', async ({
      page,
      walletPage,
    }) => {
      const invalidAddresses = [
        '0x123', // Too short
        '0xinvalidhexcharacters', // Invalid characters
        '1234567890123456789012345678901234567890', // Missing 0x prefix
        '0x', // Empty after prefix
        '',
      ];

      for (const invalidAddress of invalidAddresses) {
        await test.step(`Test invalid address: ${invalidAddress}`, async () => {
          await walletPage.sendButton.click();
          await walletPage.recipientAddressInput.fill(invalidAddress);

          // Verify validation error
          await expect(
            page.locator('[data-testid="address-validation-error"]')
          ).toBeVisible();

          // Confirm button should be disabled
          await expect(walletPage.confirmSendButton).toBeDisabled();

          await page.click('[data-testid="cancel-send-button"]');
        });
      }
    });

    test('should handle insufficient balance scenarios', async ({
      page,
      walletPage,
    }) => {
      await test.step('Attempt to send more than available balance', async () => {
        const currentBalance = await walletPage.getWalletBalance();
        const balance = parseFloat(currentBalance.split(' ')[0]);
        const excessiveAmount = (balance + 1000).toString();

        await walletPage.sendButton.click();
        await walletPage.recipientAddressInput.fill(testData.recipients[0]);
        await walletPage.sendAmountInput.fill(excessiveAmount);

        await walletPage.confirmSendButton.click();
      });

      await test.step('Verify insufficient balance error handling', async () => {
        await expect(
          page.locator('[data-testid="insufficient-balance-error"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="error-message"]')
        ).toContainText(TEST_CONSTANTS.ERROR_MESSAGES.INSUFFICIENT_BALANCE);
      });

      await test.step('Verify suggested actions for insufficient balance', async () => {
        await expect(
          page.locator('[data-testid="suggested-actions"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="add-funds-suggestion"]')
        ).toBeVisible();
        await expect(
          page.locator('[data-testid="reduce-amount-suggestion"]')
        ).toBeVisible();

        // Test reduce amount suggestion
        await page.click('[data-testid="reduce-amount-suggestion"]');

        const suggestedAmount = await walletPage.sendAmountInput.inputValue();
        expect(parseFloat(suggestedAmount)).toBeLessThan(
          parseFloat(await walletPage.getWalletBalance())
        );
      });
    });

    test('should handle network congestion and retry logic', async ({
      page,
      walletPage,
    }) => {
      await test.step('Simulate network congestion', async () => {
        // Mock slow network responses
        await page.route('**/api/transactions/**', route => {
          setTimeout(() => route.continue(), 5000); // 5 second delay
        });

        await walletPage.sendTransaction(testData.recipients[0], '0.1', 'WRF');
      });

      await test.step('Verify retry mechanism during network issues', async () => {
        // Should show network delay warning
        await expect(
          page.locator('[data-testid="network-delay-warning"]')
        ).toBeVisible();

        // Retry button should appear after timeout
        await page.waitForTimeout(6000);
        await expect(
          page.locator('[data-testid="retry-transaction-button"]')
        ).toBeVisible();

        // Clear route mock
        await page.unroute('**/api/transactions/**');

        // Test retry functionality
        await page.click('[data-testid="retry-transaction-button"]');
        await expect(
          page.locator('[data-testid="transaction-retrying"]')
        ).toBeVisible();
      });
    });

    test('should handle wallet disconnect and reconnect flows', async ({
      page,
      walletPage,
    }) => {
      await test.step('Verify wallet is connected', async () => {
        await walletPage.assertWalletConnected();
      });

      await test.step('Simulate wallet disconnection', async () => {
        // Simulate MetaMask disconnection
        await page.evaluate(() => {
          window.dispatchEvent(
            new CustomEvent('wallet-disconnected', {
              detail: { reason: 'user-initiated' },
            })
          );
        });

        await expect(
          page.locator('[data-testid="wallet-disconnected-warning"]')
        ).toBeVisible();
        await expect(walletPage.connectWalletButton).toBeVisible();
      });

      await test.step('Test automatic reconnection attempt', async () => {
        // App should attempt to reconnect
        await expect(
          page.locator('[data-testid="reconnecting"]')
        ).toBeVisible();

        // After timeout, should show reconnect button
        await page.waitForTimeout(3000);
        await expect(
          page.locator('[data-testid="reconnect-wallet-button"]')
        ).toBeVisible();
      });

      await test.step('Test manual reconnection', async () => {
        await page.click('[data-testid="reconnect-wallet-button"]');
        await walletPage.connectWallet();
        await walletPage.assertWalletConnected();

        // Verify wallet state is restored
        const walletAddress = await walletPage.getWalletAddress();
        expect(walletAddress).toMatch(/^0x[a-fA-F0-9]{40}$/);
      });
    });

    test('should validate transaction parameters and prevent common errors', async ({
      page,
      walletPage,
    }) => {
      await test.step('Test negative amount validation', async () => {
        await walletPage.sendButton.click();
        await walletPage.sendAmountInput.fill('-1.0');

        await expect(
          page.locator('[data-testid="amount-validation-error"]')
        ).toBeVisible();
        await expect(walletPage.confirmSendButton).toBeDisabled();
      });

      await test.step('Test zero amount validation', async () => {
        await walletPage.sendAmountInput.fill('0');

        await expect(
          page.locator('[data-testid="zero-amount-error"]')
        ).toBeVisible();
        await expect(walletPage.confirmSendButton).toBeDisabled();
      });

      await test.step('Test excessive decimal places', async () => {
        await walletPage.sendAmountInput.fill('1.123456789012345678901');

        // Should be truncated to token decimals (18 for WRF)
        const actualValue = await walletPage.sendAmountInput.inputValue();
        const decimalPlaces = actualValue.split('.')[1]?.length || 0;
        expect(decimalPlaces).toBeLessThanOrEqual(18);
      });

      await test.step('Test self-transfer prevention', async () => {
        const ownAddress = await walletPage.getWalletAddress();

        await walletPage.recipientAddressInput.fill(ownAddress);
        await walletPage.sendAmountInput.fill('1.0');

        await expect(
          page.locator('[data-testid="self-transfer-warning"]')
        ).toBeVisible();
        // Should allow but warn about self-transfers
      });

      await test.step('Cancel transaction and verify cleanup', async () => {
        await page.click('[data-testid="cancel-send-button"]');
        await expect(walletPage.sendModal).not.toBeVisible();

        // Verify form is reset for next use
        await walletPage.sendButton.click();
        expect(await walletPage.recipientAddressInput.inputValue()).toBe('');
        expect(await walletPage.sendAmountInput.inputValue()).toBe('');
      });
    });
  });
});
