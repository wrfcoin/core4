# Getting Started with WRFCoin: Environment Setup

Welcome to WRFCoin! This tutorial will guide you through setting up your development environment to
start building on the WRFCoin platform - the world's first climate-focused blockchain.

## What You'll Learn

- How to set up your development environment
- Install and configure WRFCoin SDK
- Create your first wallet
- Connect to the WRFCoin network
- Verify your setup is working

## Prerequisites

- Basic programming knowledge (JavaScript or Python)
- Node.js 16+ or Python 3.8+
- A text editor or IDE
- Internet connection

## Step 1: Choose Your Development Path

WRFCoin supports multiple SDKs. Choose the one that matches your experience:

### JavaScript/TypeScript (Recommended for beginners)

- ✅ Most comprehensive examples
- ✅ Great for web applications
- ✅ Extensive documentation

### Python (Recommended for data scientists)

- ✅ Perfect for data analysis
- ✅ Climate research integration
- ✅ Scientific computing libraries

### CLI (For advanced users)

- ✅ Direct blockchain interaction
- ✅ Automation and scripting
- ✅ Server-side operations

## Step 2: JavaScript SDK Setup

### Installation

```bash
# Create a new project
mkdir my-wrfcoin-app
cd my-wrfcoin-app

# Initialize npm project
npm init -y

# Install WRFCoin JavaScript SDK
npm install @wrfcoin/sdk

# Install additional dependencies for examples
npm install dotenv axios
```

### Project Structure

Create the following files in your project:

```
my-wrfcoin-app/
├── package.json
├── .env
├── examples/
│   ├── basic-setup.js
│   └── wallet-example.js
└── README.md
```

### Environment Configuration

Create a `.env` file with your configuration:

```env
# .env
WRFCOIN_NETWORK=testnet
WRFCOIN_API_URL=https://testnet-api.wrfcoin.com
WRFCOIN_WEBSOCKET_URL=wss://testnet-ws.wrfcoin.com
WRFCOIN_LOG_LEVEL=info
```

### Basic Setup Test

Create `examples/basic-setup.js`:

```javascript
// examples/basic-setup.js
require('dotenv').config();
const { WrfcoinClient } = require('@wrfcoin/sdk');

async function testSetup() {
  console.log('🌍 Welcome to WRFCoin!');
  console.log('Setting up your development environment...\n');

  try {
    // Initialize the WRFCoin client
    const client = new WrfcoinClient({
      network: process.env.WRFCOIN_NETWORK || 'testnet',
      apiUrl: process.env.WRFCOIN_API_URL,
      logLevel: process.env.WRFCOIN_LOG_LEVEL || 'info',
    });

    console.log('✅ WRFCoin client initialized');

    // Test network connection
    const networkInfo = await client.getNetworkInfo();
    console.log('✅ Connected to WRFCoin network');
    console.log(`   - Network: ${networkInfo.name}`);
    console.log(`   - Chain ID: ${networkInfo.chainId}`);
    console.log(`   - Block Height: ${networkInfo.blockHeight}`);
    console.log(`   - Network Health: ${networkInfo.status}`);

    // Test API connectivity
    const apiStatus = await client.getApiStatus();
    console.log('✅ API connection verified');
    console.log(`   - API Version: ${apiStatus.version}`);
    console.log(`   - Response Time: ${apiStatus.responseTime}ms`);

    console.log("\n🎉 Setup complete! You're ready to start building on WRFCoin.");
    console.log('\nNext steps:');
    console.log('1. Run: node examples/wallet-example.js');
    console.log(
      '2. Check out the tutorial: docs/tutorials/getting-started/02-first-transaction.md'
    );
  } catch (error) {
    console.error('❌ Setup failed:', error.message);
    console.log('\nTroubleshooting:');
    console.log('1. Check your internet connection');
    console.log('2. Verify your .env configuration');
    console.log("3. Ensure you're using Node.js 16+");
    process.exit(1);
  }
}

// Run the setup test
testSetup();
```

### Wallet Example

Create `examples/wallet-example.js`:

```javascript
// examples/wallet-example.js
require('dotenv').config();
const { WrfcoinClient } = require('@wrfcoin/sdk');

async function walletExample() {
  console.log('🔐 WRFCoin Wallet Example\n');

  try {
    const client = new WrfcoinClient({
      network: process.env.WRFCOIN_NETWORK || 'testnet',
    });

    // Generate a new wallet
    console.log('Creating a new wallet...');
    const wallet = await client.wallet.generate();

    console.log('✅ Wallet created successfully!');
    console.log(`   - Address: ${wallet.address}`);
    console.log(`   - Mnemonic: ${wallet.mnemonic}`);
    console.log('⚠️  IMPORTANT: Save your mnemonic phrase securely!\n');

    // Check wallet balance
    const balance = await client.wallet.getBalance(wallet.address);
    console.log(`💰 Wallet Balance: ${balance.WRF} WRF`);

    if (balance.WRF === 0) {
      console.log('\n🚰 To get test tokens:');
      console.log('1. Visit: https://faucet.wrfcoin.com');
      console.log('2. Enter your address:', wallet.address);
      console.log('3. Request test WRF tokens');
    }

    // Show wallet capabilities
    console.log('\n🛠️  What you can do with this wallet:');
    console.log('• Send and receive WRF tokens');
    console.log('• Submit weather data and earn rewards');
    console.log('• Participate in governance voting');
    console.log('• Mint and trade climate NFTs');
    console.log('• Bridge tokens across chains');
  } catch (error) {
    console.error('❌ Wallet example failed:', error.message);
  }
}

walletExample();
```

## Step 3: Python SDK Setup

### Installation

```bash
# Create virtual environment
python -m venv wrfcoin-env
source wrfcoin-env/bin/activate  # On Windows: wrfcoin-env\Scripts\activate

# Install WRFCoin Python SDK
pip install wrfcoin-sdk requests python-dotenv
```

### Python Example

Create `setup_test.py`:

```python
# setup_test.py
import os
import asyncio
from dotenv import load_dotenv
from wrfcoin_sdk import WrfcoinClient

load_dotenv()

async def test_python_setup():
    print("🐍 WRFCoin Python SDK Setup Test\n")

    try:
        # Initialize client
        client = WrfcoinClient(
            network=os.getenv('WRFCOIN_NETWORK', 'testnet'),
            api_url=os.getenv('WRFCOIN_API_URL')
        )

        print("✅ Python SDK initialized")

        # Test connection
        network_info = await client.get_network_info()
        print(f"✅ Connected to {network_info['name']}")
        print(f"   - Block Height: {network_info['block_height']}")

        # Generate wallet
        wallet = await client.wallet.generate()
        print(f"✅ Wallet created: {wallet['address']}")

        print("\n🎉 Python setup complete!")
        print("\nNext: Check out docs/tutorials/getting-started/02-first-transaction.md")

    except Exception as e:
        print(f"❌ Setup failed: {e}")

if __name__ == "__main__":
    asyncio.run(test_python_setup())
```

## Step 4: Verify Your Setup

Run your setup test:

```bash
# JavaScript
node examples/basic-setup.js

# Python
python setup_test.py
```

### Expected Output

You should see something like:

```
🌍 Welcome to WRFCoin!
Setting up your development environment...

✅ WRFCoin client initialized
✅ Connected to WRFCoin network
   - Network: WRFCoin Testnet
   - Chain ID: 31337
   - Block Height: 12345
   - Network Health: healthy
✅ API connection verified
   - API Version: v2.1.0
   - Response Time: 45ms

🎉 Setup complete! You're ready to start building on WRFCoin.
```

## Step 5: Get Testnet Tokens

To interact with the blockchain, you'll need test tokens:

1. **Visit the Faucet**: https://faucet.wrfcoin.com
2. **Enter your wallet address** from the wallet example
3. **Request test WRF tokens** (you'll receive 100 test WRF)
4. **Wait 1-2 minutes** for the transaction to confirm

## Common Issues & Solutions

### Issue: "Network connection failed"

**Solution**: Check your internet connection and firewall settings.

### Issue: "API endpoint not responding"

**Solution**: Verify the API URL in your `.env` file.

### Issue: "Module not found"

**Solution**: Ensure you've installed all dependencies with `npm install` or `pip install`.

### Issue: "Wallet generation failed"

**Solution**: Check that you have sufficient entropy (move your mouse/type on keyboard).

## What's Next?

🎯 **Congratulations!** You've successfully set up your WRFCoin development environment.

### Next Tutorials:

1. **[Your First Transaction](./02-first-transaction.md)** - Send your first WRF transaction
2. **[Understanding WRFCoin Concepts](./03-understanding-concepts.md)** - Learn blockchain
   fundamentals
3. **[Weather Data Submission](../intermediate/weather-data-tutorial.md)** - Submit your first
   climate data

### Quick Reference:

- **Documentation**: https://docs.wrfcoin.com
- **API Reference**: https://api-docs.wrfcoin.com
- **Discord Community**: https://discord.gg/wrfcoin
- **GitHub**: https://github.com/wrfcoin/core

---

**Need Help?** Join our Discord community or open an issue on GitHub. The WRFCoin developer
community is here to help! 🌱
