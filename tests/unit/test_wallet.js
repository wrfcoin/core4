/**
 * Test Script for Account Abstraction Wallet Layer
 *
 * This script tests the account abstraction wallet functionality including:
 * - Creating and retrieving wallets
 * - Delegated signing
 * - Meta-transactions
 * - Sponsor-based gas payments
 */

const crypto = require('crypto');
const http = require('http');

// Generate a test key pair
function generateKeyPair() {
  const privateKey = crypto.randomBytes(32).toString('hex');
  const publicKey =
    '04' + crypto.createHash('sha256').update(privateKey).digest('hex');
  return { publicKey, privateKey };
}

// Sign message helper
function signMessage(message, privateKey) {
  const hmac = crypto.createHmac('sha256', privateKey);
  hmac.update(message);
  return hmac.digest('hex');
}

// Make HTTP request helper function
function makeRequest(options, data = null) {
  return new Promise((resolve, reject) => {
    const req = http.request(options, res => {
      let responseData = '';

      res.on('data', chunk => {
        responseData += chunk;
      });

      res.on('end', () => {
        try {
          const parsedData = JSON.parse(responseData);
          resolve({ statusCode: res.statusCode, data: parsedData });
        } catch (error) {
          reject(new Error(`Failed to parse response: ${error.message}`));
        }
      });
    });

    req.on('error', error => {
      reject(error);
    });

    if (data) {
      req.write(JSON.stringify(data));
    }

    req.end();
  });
}

// Run tests
async function runTests() {
  console.log('Starting Account Abstraction Wallet tests...');

  // Test health endpoint
  try {
    const healthOptions = {
      hostname: 'localhost',
      port: 5002,
      path: '/health',
      method: 'GET',
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const healthResponse = await makeRequest(healthOptions);
    console.log(
      'Health endpoint response:',
      JSON.stringify(healthResponse, null, 2)
    );
  } catch (error) {
    console.error('Health endpoint test failed:', error.message);
  }

  let walletAddress, privateKey;

  // Test creating a wallet
  try {
    const keyPair = generateKeyPair();
    privateKey = keyPair.privateKey;

    const createWalletOptions = {
      hostname: 'localhost',
      port: 5002,
      path: '/api/wallet',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const createWalletData = {
      userId: 1,
      publicKey: keyPair.publicKey,
    };

    console.log('Creating wallet with public key:', keyPair.publicKey);
    const createWalletResponse = await makeRequest(
      createWalletOptions,
      createWalletData
    );
    console.log(
      'Create wallet response:',
      JSON.stringify(createWalletResponse, null, 2)
    );

    if (createWalletResponse.data.success) {
      walletAddress = createWalletResponse.data.wallet.address;

      // Test getting wallet
      const getWalletOptions = {
        hostname: 'localhost',
        port: 5002,
        path: `/api/wallet/${walletAddress}`,
        method: 'GET',
        headers: {
          'Content-Type': 'application/json',
        },
      };

      console.log(`Getting wallet with address: ${walletAddress}`);
      const getWalletResponse = await makeRequest(getWalletOptions);
      console.log(
        'Get wallet response:',
        JSON.stringify(getWalletResponse, null, 2)
      );
    }
  } catch (error) {
    console.error('Wallet creation/retrieval tests failed:', error.message);
  }

  // Test meta-transaction
  try {
    if (walletAddress) {
      console.log('\nTesting Meta-Transaction...');

      const operation = {
        type: 'TRANSFER',
        from: walletAddress,
        to: 'wrf1receiver000000000000000000000000000000',
        amount: '10.5',
        nonce: 0,
      };

      // Create operation hash for signing
      const operationString = JSON.stringify(operation);
      const operationHash = crypto
        .createHash('sha256')
        .update(operationString)
        .digest('hex');

      // Sign the operation
      const signature = signMessage(operationHash, privateKey);

      const metaTxOptions = {
        hostname: 'localhost',
        port: 5002,
        path: '/api/meta-transaction',
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
      };

      const metaTxData = {
        operation,
        signature,
      };

      console.log(
        'Submitting meta-transaction:',
        JSON.stringify(metaTxData, null, 2)
      );
      try {
        const metaTxResponse = await makeRequest(metaTxOptions, metaTxData);
        console.log(
          'Meta-transaction response:',
          JSON.stringify(metaTxResponse, null, 2)
        );
      } catch (error) {
        console.log(
          'Meta-transaction endpoint not fully implemented yet:',
          error.message
        );
      }
    }
  } catch (error) {
    console.error('Meta-transaction tests failed:', error.message);
  }

  // Test Delegated Signing
  try {
    console.log('\nTesting Delegated Signing...');

    // Create a delegation request between two wallets
    const delegator = walletAddress;
    const delegateeKeyPair = generateKeyPair();

    const createDelegateWalletOptions = {
      hostname: 'localhost',
      port: 5002,
      path: '/api/wallet',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const createDelegateWalletData = {
      userId: 2,
      publicKey: delegateeKeyPair.publicKey,
    };

    console.log('Creating delegatee wallet...');
    let delegatee;
    try {
      const delegateWalletResponse = await makeRequest(
        createDelegateWalletOptions,
        createDelegateWalletData
      );
      if (delegateWalletResponse.data.success) {
        delegatee = delegateWalletResponse.data.wallet.address;

        // Create delegation request
        const delegation = {
          delegator,
          delegatee,
          permissions: ['TRANSFER', 'SIGN'],
          expiresAt: new Date(Date.now() + 86400000).toISOString(), // 24 hours
        };

        // Hash and sign the delegation
        const delegationString = JSON.stringify(delegation);
        const delegationHash = crypto
          .createHash('sha256')
          .update(delegationString)
          .digest('hex');
        const signature = signMessage(delegationHash, privateKey);

        const delegationOptions = {
          hostname: 'localhost',
          port: 5002,
          path: '/api/delegation',
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
        };

        const delegationData = {
          delegation,
          signature,
        };

        console.log(
          'Creating delegation:',
          JSON.stringify(delegationData, null, 2)
        );
        try {
          const delegationResponse = await makeRequest(
            delegationOptions,
            delegationData
          );
          console.log(
            'Delegation response:',
            JSON.stringify(delegationResponse, null, 2)
          );
        } catch (error) {
          console.log(
            'Delegation endpoint not fully implemented yet:',
            error.message
          );
        }
      }
    } catch (error) {
      console.log('Delegatee wallet creation failed:', error.message);
    }
  } catch (error) {
    console.error('Delegated signing tests failed:', error.message);
  }

  // Test sponsored transaction
  try {
    console.log('\nTesting Sponsored Transaction...');

    // Create sponsored transaction request
    const operation = {
      type: 'TRANSFER',
      from: walletAddress,
      to: 'wrf1receiver000000000000000000000000000000',
      amount: '5.25',
      nonce: 1,
    };

    // Create operation hash for signing
    const operationString = JSON.stringify(operation);
    const operationHash = crypto
      .createHash('sha256')
      .update(operationString)
      .digest('hex');

    // Sign the operation
    const signature = signMessage(operationHash, privateKey);

    const sponsorTxOptions = {
      hostname: 'localhost',
      port: 5002,
      path: '/api/sponsored-transaction',
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const sponsorTxData = {
      operation,
      signature,
      sponsorId: 'sponsor-123', // The ID of the sponsor who will pay gas
      reason: 'New user onboarding',
    };

    console.log(
      'Submitting sponsored transaction:',
      JSON.stringify(sponsorTxData, null, 2)
    );
    try {
      const sponsorTxResponse = await makeRequest(
        sponsorTxOptions,
        sponsorTxData
      );
      console.log(
        'Sponsored transaction response:',
        JSON.stringify(sponsorTxResponse, null, 2)
      );
    } catch (error) {
      console.log(
        'Sponsored transaction endpoint not fully implemented yet:',
        error.message
      );
    }
  } catch (error) {
    console.error('Sponsored transaction tests failed:', error.message);
  }

  console.log('\nAccount Abstraction Wallet tests completed.');
}

runTests().catch(console.error);
