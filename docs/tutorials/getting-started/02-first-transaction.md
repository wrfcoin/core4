# Your First WRFCoin Transaction

Learn how to send your first transaction on the WRFCoin blockchain. This tutorial builds on the
environment setup from the previous guide.

## What You'll Learn

- How to send WRF tokens between wallets
- Understanding transaction fees and confirmation
- Reading transaction receipts
- Best practices for transaction security
- How to track transaction status

## Prerequisites

- Completed [Environment Setup](./01-setup-environment.md)
- Test WRF tokens from the faucet
- Basic understanding of blockchain concepts

## Step 1: Prepare Your Wallets

Let's create a comprehensive transaction example that shows real-world usage patterns.

### Create the Transaction Example

Create `examples/first-transaction.js`:

```javascript
// examples/first-transaction.js
require('dotenv').config();
const { WrfcoinClient } = require('@wrfcoin/sdk');

async function firstTransactionTutorial() {
  console.log('💸 Your First WRFCoin Transaction\n');

  try {
    // Initialize client
    const client = new WrfcoinClient({
      network: process.env.WRFCOIN_NETWORK || 'testnet',
    });

    // Step 1: Create or load sender wallet
    console.log('📋 Step 1: Setting up wallets...');

    // For this tutorial, we'll generate a new wallet
    // In practice, you'd load an existing wallet from a mnemonic
    const senderWallet = await client.wallet.generate();
    console.log(`✅ Sender wallet created: ${senderWallet.address}`);

    // Create a recipient wallet
    const recipientWallet = await client.wallet.generate();
    console.log(`✅ Recipient wallet created: ${recipientWallet.address}\n`);

    // Step 2: Check initial balances
    console.log('💰 Step 2: Checking balances...');
    const senderBalance = await client.wallet.getBalance(senderWallet.address);
    const recipientBalance = await client.wallet.getBalance(recipientWallet.address);

    console.log(`Sender balance: ${senderBalance.WRF} WRF`);
    console.log(`Recipient balance: ${recipientBalance.WRF} WRF\n`);

    // Check if sender has sufficient balance
    if (senderBalance.WRF < 1) {
      console.log('⚠️  Insufficient balance for transaction!');
      console.log(
        `Please visit https://faucet.wrfcoin.com and request test tokens for: ${senderWallet.address}`
      );
      console.log('Then run this script again.\n');
      return;
    }

    // Step 3: Prepare the transaction
    console.log('🔧 Step 3: Preparing transaction...');
    const transferAmount = 0.5; // 0.5 WRF

    // Get current gas price for optimal fee estimation
    const gasPrice = await client.blockchain.getGasPrice();
    console.log(`Current gas price: ${gasPrice} Wei`);

    // Prepare transaction parameters
    const transactionParams = {
      from: senderWallet.address,
      to: recipientWallet.address,
      amount: transferAmount,
      gasLimit: 21000, // Standard transfer gas limit
      gasPrice: gasPrice,
      message: 'My first WRFCoin transaction! 🌍', // Optional message
    };

    // Calculate estimated fees
    const estimatedFee = (transactionParams.gasLimit * gasPrice) / 1e18;
    console.log(`Transfer amount: ${transferAmount} WRF`);
    console.log(`Estimated fee: ${estimatedFee.toFixed(6)} WRF`);
    console.log(`Total cost: ${(transferAmount + estimatedFee).toFixed(6)} WRF\n`);

    // Step 4: Create and sign the transaction
    console.log('✍️  Step 4: Creating and signing transaction...');

    // Create the transaction
    const transaction = await client.transaction.create(transactionParams);
    console.log(`✅ Transaction created with ID: ${transaction.id}`);

    // Sign the transaction with sender's private key
    const signedTransaction = await client.wallet.signTransaction(
      transaction,
      senderWallet.privateKey
    );
    console.log('✅ Transaction signed successfully\n');

    // Step 5: Submit the transaction
    console.log('🚀 Step 5: Submitting transaction to network...');

    const submissionResult = await client.transaction.submit(signedTransaction);
    console.log(`✅ Transaction submitted!`);
    console.log(`   - Transaction Hash: ${submissionResult.hash}`);
    console.log(`   - Status: ${submissionResult.status}`);
    console.log(`   - Block: Pending confirmation...\n`);

    // Step 6: Wait for confirmation
    console.log('⏳ Step 6: Waiting for confirmation...');
    console.log('This may take 30-60 seconds...\n');

    let confirmation;
    let attempts = 0;
    const maxAttempts = 12; // 2 minutes maximum

    while (attempts < maxAttempts) {
      try {
        confirmation = await client.transaction.getReceipt(submissionResult.hash);
        if (confirmation.status === 'confirmed') {
          break;
        }
      } catch (error) {
        // Transaction not yet mined
      }

      attempts++;
      await new Promise(resolve => setTimeout(resolve, 10000)); // Wait 10 seconds
      process.stdout.write('.');
    }

    if (confirmation && confirmation.status === 'confirmed') {
      console.log('\n🎉 Transaction confirmed!');
      console.log(`   - Block Number: ${confirmation.blockNumber}`);
      console.log(`   - Gas Used: ${confirmation.gasUsed}`);
      console.log(`   - Actual Fee: ${((confirmation.gasUsed * gasPrice) / 1e18).toFixed(6)} WRF`);
      console.log(`   - Confirmation Time: ${confirmation.timestamp}\n`);

      // Step 7: Verify final balances
      console.log('🔍 Step 7: Verifying final balances...');
      const newSenderBalance = await client.wallet.getBalance(senderWallet.address);
      const newRecipientBalance = await client.wallet.getBalance(recipientWallet.address);

      console.log(`Sender balance: ${senderBalance.WRF} → ${newSenderBalance.WRF} WRF`);
      console.log(`Recipient balance: ${recipientBalance.WRF} → ${newRecipientBalance.WRF} WRF`);
      console.log(`Amount transferred: ${newRecipientBalance.WRF - recipientBalance.WRF} WRF\n`);

      // Step 8: Transaction analysis
      console.log('📊 Transaction Analysis:');
      console.log(`✅ Transfer successful: ${transferAmount} WRF`);
      console.log(`💳 Total cost: ${(senderBalance.WRF - newSenderBalance.WRF).toFixed(6)} WRF`);
      console.log(
        `⛽ Network fee: ${(senderBalance.WRF - newSenderBalance.WRF - transferAmount).toFixed(6)} WRF`
      );
      console.log(`🌐 View on explorer: https://explorer.wrfcoin.com/tx/${submissionResult.hash}`);
    } else {
      console.log('\n⏰ Transaction taking longer than expected...');
      console.log(
        `Check status manually: https://explorer.wrfcoin.com/tx/${submissionResult.hash}`
      );
    }
  } catch (error) {
    console.error('❌ Transaction failed:', error.message);
    console.log('\nCommon issues:');
    console.log('1. Insufficient balance');
    console.log('2. Network congestion');
    console.log('3. Gas price too low');
    console.log('4. Invalid recipient address');
  }
}

// Educational helper functions
async function explainTransactionFields() {
  console.log('\n📚 Understanding Transaction Fields:\n');

  console.log('🔹 FROM: Your wallet address (transaction sender)');
  console.log('🔹 TO: Recipient wallet address');
  console.log('🔹 AMOUNT: Number of WRF tokens to transfer');
  console.log('🔹 GAS LIMIT: Maximum computational work allowed');
  console.log('🔹 GAS PRICE: Fee per unit of computational work');
  console.log('🔹 MESSAGE: Optional text attached to transaction');
  console.log('🔹 NONCE: Transaction sequence number (auto-handled)\n');

  console.log('💡 Pro Tips:');
  console.log('• Higher gas price = faster confirmation');
  console.log('• Gas limit too low = transaction fails');
  console.log('• Always verify recipient address twice');
  console.log('• Save transaction hashes for record keeping\n');
}

async function showAdvancedFeatures() {
  console.log('🚀 Advanced Transaction Features:\n');

  console.log('🌟 Climate Data Transactions:');
  console.log('   Submit weather data and earn WRF rewards');
  console.log('   Example: client.weather.submitData(weatherData)');

  console.log('\n🌟 Multi-token Transfers:');
  console.log('   Send multiple tokens in one transaction');
  console.log('   Example: client.transaction.multiTransfer([...tokens])');

  console.log('\n🌟 Scheduled Transactions:');
  console.log('   Set up recurring payments or automated transfers');
  console.log('   Example: client.transaction.schedule(params, schedule)');

  console.log('\n🌟 Cross-chain Transfers:');
  console.log('   Bridge WRF tokens to other blockchains');
  console.log('   Example: client.bridge.transfer(amount, targetChain)');
}

// Run the tutorial
async function runTutorial() {
  await firstTransactionTutorial();
  await explainTransactionFields();
  await showAdvancedFeatures();

  console.log('\n🎓 Tutorial Complete!');
  console.log('\nNext Steps:');
  console.log('1. Try different transaction amounts');
  console.log('2. Experiment with gas prices');
  console.log('3. Add transaction messages');
  console.log('4. Learn about [WRFCoin Concepts](./03-understanding-concepts.md)');
  console.log('5. Explore [Weather Data Submission](../intermediate/weather-data-tutorial.md)');
}

runTutorial();
```

## Step 2: Python Transaction Example

Create a Python version for data scientists:

Create `examples/first_transaction.py`:

```python
# examples/first_transaction.py
import asyncio
import os
from dotenv import load_dotenv
from wrfcoin_sdk import WrfcoinClient

load_dotenv()

async def first_transaction_tutorial():
    """
    Complete tutorial for sending your first WRFCoin transaction in Python
    """
    print("🐍 Your First WRFCoin Transaction (Python)\n")

    try:
        # Initialize client
        client = WrfcoinClient(
            network=os.getenv('WRFCOIN_NETWORK', 'testnet')
        )

        # Step 1: Setup wallets
        print("📋 Step 1: Setting up wallets...")
        sender = await client.wallet.generate()
        recipient = await client.wallet.generate()

        print(f"✅ Sender: {sender['address']}")
        print(f"✅ Recipient: {recipient['address']}\n")

        # Step 2: Check balances
        print("💰 Step 2: Checking balances...")
        sender_balance = await client.wallet.get_balance(sender['address'])
        recipient_balance = await client.wallet.get_balance(recipient['address'])

        print(f"Sender balance: {sender_balance['WRF']} WRF")
        print(f"Recipient balance: {recipient_balance['WRF']} WRF\n")

        if sender_balance['WRF'] < 1:
            print("⚠️  Insufficient balance!")
            print(f"Get test tokens: https://faucet.wrfcoin.com")
            print(f"Address: {sender['address']}")
            return

        # Step 3: Prepare transaction
        print("🔧 Step 3: Preparing transaction...")
        transfer_amount = 0.5

        transaction_params = {
            'from': sender['address'],
            'to': recipient['address'],
            'amount': transfer_amount,
            'message': 'Python transaction example! 🐍'
        }

        # Step 4: Create and send transaction
        print("🚀 Step 4: Sending transaction...")

        result = await client.transaction.send(
            transaction_params,
            sender['private_key']
        )

        print(f"✅ Transaction sent!")
        print(f"   Hash: {result['hash']}")
        print(f"   Status: {result['status']}")

        # Step 5: Wait for confirmation
        print("\n⏳ Waiting for confirmation...")
        confirmation = await client.transaction.wait_for_confirmation(
            result['hash'],
            timeout=120
        )

        if confirmation['status'] == 'confirmed':
            print("🎉 Transaction confirmed!")
            print(f"   Block: {confirmation['block_number']}")
            print(f"   Gas used: {confirmation['gas_used']}")

            # Verify balances
            new_sender = await client.wallet.get_balance(sender['address'])
            new_recipient = await client.wallet.get_balance(recipient['address'])

            print(f"\n📊 Final balances:")
            print(f"Sender: {sender_balance['WRF']} → {new_sender['WRF']} WRF")
            print(f"Recipient: {recipient_balance['WRF']} → {new_recipient['WRF']} WRF")

        print(f"\n🌐 View transaction: https://explorer.wrfcoin.com/tx/{result['hash']}")

    except Exception as e:
        print(f"❌ Transaction failed: {e}")

async def explain_python_features():
    """
    Show Python-specific features for climate data work
    """
    print("\n🐍 Python-Specific Features for Climate Research:\n")

    print("📊 Data Analysis Integration:")
    print("   import pandas as pd")
    print("   import numpy as np")
    print("   from wrfcoin_sdk import WrfcoinClient")
    print("")
    print("   # Load climate data")
    print("   data = pd.read_csv('weather_data.csv')")
    print("   ")
    print("   # Submit to blockchain")
    print("   client = WrfcoinClient()")
    print("   result = await client.weather.submit_dataframe(data)")

    print("\n🔬 Scientific Computing:")
    print("   # Statistical analysis with rewards")
    print("   correlation = np.corrcoef(temperature, co2_levels)")
    print("   await client.research.submit_analysis(correlation)")

    print("\n📈 Real-time Data Streaming:")
    print("   # Stream sensor data")
    print("   async for data_point in sensor_stream:")
    print("       await client.weather.submit_realtime(data_point)")

if __name__ == "__main__":
    asyncio.run(first_transaction_tutorial())
    asyncio.run(explain_python_features())
```

## Step 3: Understanding Transaction Lifecycle

### Transaction States

Every WRFCoin transaction goes through these states:

1. **Created** - Transaction object created in memory
2. **Signed** - Transaction cryptographically signed
3. **Submitted** - Sent to the network mempool
4. **Pending** - Waiting to be included in a block
5. **Confirmed** - Included in a block and verified
6. **Finalized** - Multiple confirmations received

### Visual Transaction Flow

```
[Wallet A] ---> [Create Tx] ---> [Sign Tx] ---> [Submit] ---> [Mempool]
                                                                  |
[Blockchain] <--- [Block] <--- [Mine] <--- [Validate] <--- [Select Tx]
                    |
[Wallet B] <--- [Receive] <--- [Confirm] <--- [Update Balance]
```

## Step 4: Transaction Security Best Practices

### Private Key Security

```javascript
// ❌ NEVER do this in production
const privateKey = 'your-private-key-here';

// ✅ Use environment variables
const privateKey = process.env.WALLET_PRIVATE_KEY;

// ✅ Use hardware wallets for large amounts
const ledgerWallet = await client.wallet.connectLedger();

// ✅ Use multi-signature for enterprise
const multiSigWallet = await client.wallet.createMultiSig({
  signers: [address1, address2, address3],
  threshold: 2,
});
```

### Transaction Validation

```javascript
// Always validate before sending
function validateTransaction(params) {
  // Check recipient address format
  if (!client.utils.isValidAddress(params.to)) {
    throw new Error('Invalid recipient address');
  }

  // Check amount is positive
  if (params.amount <= 0) {
    throw new Error('Amount must be positive');
  }

  // Check sufficient balance
  if (senderBalance < params.amount + estimatedFee) {
    throw new Error('Insufficient balance for transaction + fees');
  }

  return true;
}
```

## Step 5: Error Handling and Troubleshooting

### Common Errors and Solutions

```javascript
async function robustTransactionSending(params) {
  try {
    return await client.transaction.send(params);
  } catch (error) {
    switch (error.code) {
      case 'INSUFFICIENT_BALANCE':
        console.log('💰 Get more tokens from faucet or reduce amount');
        break;

      case 'INVALID_ADDRESS':
        console.log('🎯 Check recipient address format');
        break;

      case 'GAS_TOO_LOW':
        console.log('⛽ Increase gas price for faster confirmation');
        // Retry with higher gas
        params.gasPrice *= 1.5;
        return await client.transaction.send(params);

      case 'NONCE_TOO_LOW':
        console.log('🔄 Getting latest nonce and retrying...');
        params.nonce = await client.wallet.getNonce(params.from);
        return await client.transaction.send(params);

      case 'NETWORK_ERROR':
        console.log('🌐 Network issue, retrying in 10 seconds...');
        await new Promise(resolve => setTimeout(resolve, 10000));
        return await robustTransactionSending(params);

      default:
        console.error('❌ Unknown error:', error.message);
        throw error;
    }
  }
}
```

## Step 6: Advanced Transaction Features

### Batch Transactions

```javascript
// Send multiple transactions efficiently
const batchTransactions = [
  { to: address1, amount: 1.0 },
  { to: address2, amount: 2.0 },
  { to: address3, amount: 0.5 },
];

const batchResult = await client.transaction.sendBatch(batchTransactions);
console.log(`Sent ${batchResult.length} transactions in one batch`);
```

### Transaction Messages and Metadata

```javascript
// Add rich metadata to transactions
const txWithMetadata = {
  from: senderWallet.address,
  to: recipientWallet.address,
  amount: 1.0,
  message: 'Payment for weather station data',
  metadata: {
    category: 'research_payment',
    stationId: 'WS-001',
    dataType: 'temperature',
    coordinates: { lat: 40.7128, lng: -74.006 },
  },
};
```

## Step 7: Run the Tutorial

Execute your first transaction:

```bash
# JavaScript
node examples/first-transaction.js

# Python
python examples/first_transaction.py
```

## What You've Learned

✅ **Transaction Creation** - How to prepare and structure transactions ✅ **Digital Signatures** -
Securing transactions with cryptographic signatures  
✅ **Network Submission** - Sending transactions to the blockchain ✅ **Confirmation Tracking** -
Monitoring transaction status ✅ **Error Handling** - Dealing with common transaction issues ✅
**Security Best Practices** - Protecting your assets and private keys

## Next Steps

### Immediate Practice

1. **Try different amounts** - Send various amounts to test
2. **Experiment with fees** - Test different gas prices
3. **Add messages** - Include meaningful transaction messages
4. **Test error scenarios** - Try sending with insufficient balance

### Advanced Learning

1. **[Understanding WRFCoin Concepts](./03-understanding-concepts.md)** - Deep dive into blockchain
   concepts
2. **[Weather Data Submission](../intermediate/weather-data-tutorial.md)** - Learn to submit climate
   data
3. **[Smart Contract Interaction](../intermediate/smart-contracts/basic-contract-interaction.md)** -
   Interact with smart contracts
4. **[Cross-chain Transfers](../intermediate/cross-chain-tutorial.md)** - Bridge tokens across
   blockchains

### Real-World Applications

- **Research Payments** - Pay for climate research data
- **Carbon Credit Trading** - Buy and sell carbon credits
- **IoT Device Payments** - Automated payments for sensor data
- **Governance Participation** - Vote on protocol changes

---

**🎉 Congratulations!** You've successfully sent your first WRFCoin transaction. You're now ready to
explore the exciting world of climate-focused blockchain applications!

**Need Help?** Join our community:

- 💬 [Discord](https://discord.gg/wrfcoin)
- 📖 [Documentation](https://docs.wrfcoin.com)
- 🐛 [GitHub Issues](https://github.com/wrfcoin/core/issues)
