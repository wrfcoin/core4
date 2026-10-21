# WRFCoin Smart Contract Templates - Deployment Guide

## Quick Start Deployment

### Prerequisites

- Node.js 16+ and npm/yarn
- Hardhat or Foundry development environment
- Network RPC endpoints (Mainnet, Polygon, etc.)
- Private keys securely managed (use hardware wallets for mainnet)

### Environment Setup

```bash
# Clone and setup
git clone <repository>
cd wrfcoin/templates

# Install dependencies
npm install

# Copy environment template
cp .env.example .env

# Configure .env file
PRIVATE_KEY=your_private_key_here
INFURA_API_KEY=your_infura_key_here
ETHERSCAN_API_KEY=your_etherscan_key_here
POLYGONSCAN_API_KEY=your_polygonscan_key_here
```

### Hardhat Configuration

```javascript
// hardhat.config.js
require('@nomicfoundation/hardhat-toolbox');
require('dotenv').config();

module.exports = {
  solidity: {
    version: '0.8.28',
    settings: {
      optimizer: {
        enabled: true,
        runs: 200,
      },
    },
  },
  networks: {
    hardhat: {
      chainId: 1337,
    },
    sepolia: {
      url: `https://sepolia.infura.io/v3/${process.env.INFURA_API_KEY}`,
      accounts: [process.env.PRIVATE_KEY],
    },
    polygon: {
      url: 'https://polygon-rpc.com/',
      accounts: [process.env.PRIVATE_KEY],
    },
    mainnet: {
      url: `https://mainnet.infura.io/v3/${process.env.INFURA_API_KEY}`,
      accounts: [process.env.PRIVATE_KEY],
    },
  },
  etherscan: {
    apiKey: {
      mainnet: process.env.ETHERSCAN_API_KEY,
      polygon: process.env.POLYGONSCAN_API_KEY,
    },
  },
};
```

## Template-Specific Deployment

### 1. Basic ERC20 Token

```javascript
// scripts/deploy-token.js
const { ethers } = require('hardhat');

async function main() {
  const [deployer] = await ethers.getSigners();
  console.log('Deploying with account:', deployer.address);

  // Deploy token
  const Token = await ethers.getContractFactory('BaseERC20Token');
  const token = await Token.deploy(
    'Climate Token', // name
    'CLIMATE', // symbol
    18, // decimals
    ethers.parseEther('1000000'), // initial supply
    deployer.address // owner
  );

  await token.waitForDeployment();
  console.log('Token deployed to:', await token.getAddress());

  // Verify on block explorer
  if (network.name !== 'hardhat') {
    console.log('Waiting for block confirmations...');
    await token.deploymentTransaction().wait(6);

    await hre.run('verify:verify', {
      address: await token.getAddress(),
      constructorArguments: [
        'Climate Token',
        'CLIMATE',
        18,
        ethers.parseEther('1000000'),
        deployer.address,
      ],
    });
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
```

**Deploy:**

```bash
npx hardhat run scripts/deploy-token.js --network sepolia
```

### 2. Governance System Deployment

```javascript
// scripts/deploy-governance.js
const { ethers } = require('hardhat');

async function main() {
  const [deployer] = await ethers.getSigners();

  // 1. Deploy Governance Token
  console.log('Deploying Governance Token...');
  const GovernanceToken = await ethers.getContractFactory('GovernanceToken');
  const governanceToken = await GovernanceToken.deploy(
    'DAO Token',
    'DAO',
    18,
    ethers.parseEther('10000000'),
    deployer.address
  );
  await governanceToken.waitForDeployment();
  console.log('Governance Token:', await governanceToken.getAddress());

  // 2. Deploy Timelock
  console.log('Deploying Timelock...');
  const TimelockController = await ethers.getContractFactory('TimelockController');
  const timelock = await TimelockController.deploy(
    2 * 24 * 60 * 60, // 2 days delay
    [], // proposers (will be set to governor)
    [], // executors (will be set to governor)
    deployer.address // admin
  );
  await timelock.waitForDeployment();
  console.log('Timelock:', await timelock.getAddress());

  // 3. Deploy Governor
  console.log('Deploying Governor...');
  const AdvancedGovernor = await ethers.getContractFactory('AdvancedGovernor');
  const governor = await AdvancedGovernor.deploy(
    await governanceToken.getAddress(),
    await timelock.getAddress(),
    'Climate DAO',
    7 * 24 * 60 * 60, // 7 days voting period
    1 * 24 * 60 * 60, // 1 day voting delay
    ethers.parseEther('100000'), // proposal threshold
    500 // 5% quorum
  );
  await governor.waitForDeployment();
  console.log('Governor:', await governor.getAddress());

  // 4. Deploy Treasury
  console.log('Deploying Treasury...');
  const TreasuryManager = await ethers.getContractFactory('TreasuryManager');
  const treasury = await TreasuryManager.deploy(deployer.address, await timelock.getAddress());
  await treasury.waitForDeployment();
  console.log('Treasury:', await treasury.getAddress());

  // 5. Set up permissions
  console.log('Setting up permissions...');

  // Grant roles to governor
  const PROPOSER_ROLE = await timelock.PROPOSER_ROLE();
  const EXECUTOR_ROLE = await timelock.EXECUTOR_ROLE();
  const TIMELOCK_ADMIN_ROLE = await timelock.TIMELOCK_ADMIN_ROLE();

  await timelock.grantRole(PROPOSER_ROLE, await governor.getAddress());
  await timelock.grantRole(EXECUTOR_ROLE, await governor.getAddress());

  // Revoke admin role from deployer (optional, be careful!)
  // await timelock.revokeRole(TIMELOCK_ADMIN_ROLE, deployer.address);

  console.log('Governance system deployed successfully!');

  // Save deployment addresses
  const deployment = {
    governanceToken: await governanceToken.getAddress(),
    timelock: await timelock.getAddress(),
    governor: await governor.getAddress(),
    treasury: await treasury.getAddress(),
    network: network.name,
    deployer: deployer.address,
  };

  require('fs').writeFileSync(
    `deployments/${network.name}-governance.json`,
    JSON.stringify(deployment, null, 2)
  );
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
```

### 3. Carbon Marketplace Deployment

```javascript
// scripts/deploy-marketplace.js
const { ethers } = require('hardhat');

async function main() {
  const [deployer] = await ethers.getSigners();

  // Deploy marketplace
  console.log('Deploying Carbon Marketplace...');
  const CarbonMarketplace = await ethers.getContractFactory('CarbonMarketplace');
  const marketplace = await CarbonMarketplace.deploy(deployer.address);

  await marketplace.waitForDeployment();
  console.log('Marketplace deployed to:', await marketplace.getAddress());

  // Set up verified issuers
  console.log('Setting up verified issuers...');
  await marketplace.verifyIssuer(deployer.address, true);

  // Set up certified verifiers
  console.log('Setting up certified verifiers...');
  await marketplace.certifyVerifier(deployer.address, true);

  console.log('Marketplace setup complete!');
}
```

### 4. Weather Oracle Deployment

```javascript
// scripts/deploy-oracle.js
const { ethers } = require('hardhat');

async function main() {
  const [deployer] = await ethers.getSigners();

  // Deploy weather oracle
  console.log('Deploying Weather Oracle...');
  const WeatherDataOracle = await ethers.getContractFactory('WeatherDataOracle');
  const oracle = await WeatherDataOracle.deploy(deployer.address);

  await oracle.waitForDeployment();
  console.log('Oracle deployed to:', await oracle.getAddress());

  // Set up operator role for data providers
  const OPERATOR_ROLE = await oracle.OPERATOR_ROLE();
  await oracle.grantRole(OPERATOR_ROLE, deployer.address);

  console.log('Oracle setup complete!');
}
```

## Multi-Template Deployment

### Complete Ecosystem Deployment

```javascript
// scripts/deploy-ecosystem.js
const { ethers } = require('hardhat');

async function deployEcosystem() {
  const [deployer] = await ethers.getSigners();
  console.log('Deploying complete climate ecosystem...');

  const deployments = {};

  // 1. Deploy Base Infrastructure
  console.log('\n=== Deploying Base Infrastructure ===');

  // Governance Token
  const GovernanceToken = await ethers.getContractFactory('GovernanceToken');
  const governanceToken = await GovernanceToken.deploy(
    'Climate DAO',
    'CDAO',
    18,
    ethers.parseEther('100000000'),
    deployer.address
  );
  await governanceToken.waitForDeployment();
  deployments.governanceToken = await governanceToken.getAddress();
  console.log('✓ Governance Token:', deployments.governanceToken);

  // 2. Deploy Oracle Infrastructure
  console.log('\n=== Deploying Oracle Infrastructure ===');

  const WeatherDataOracle = await ethers.getContractFactory('WeatherDataOracle');
  const oracle = await WeatherDataOracle.deploy(deployer.address);
  await oracle.waitForDeployment();
  deployments.oracle = await oracle.getAddress();
  console.log('✓ Weather Oracle:', deployments.oracle);

  // 3. Deploy Carbon Credit System
  console.log('\n=== Deploying Carbon Credit System ===');

  const CarbonMarketplace = await ethers.getContractFactory('CarbonMarketplace');
  const marketplace = await CarbonMarketplace.deploy(deployer.address);
  await marketplace.waitForDeployment();
  deployments.marketplace = await marketplace.getAddress();
  console.log('✓ Carbon Marketplace:', deployments.marketplace);

  const CarbonFootprintTracker = await ethers.getContractFactory('CarbonFootprintTracker');
  const tracker = await CarbonFootprintTracker.deploy(deployer.address);
  await tracker.waitForDeployment();
  deployments.tracker = await tracker.getAddress();
  console.log('✓ Footprint Tracker:', deployments.tracker);

  // 4. Deploy Environmental Impact Tracking
  const EnvironmentalImpact = await ethers.getContractFactory('EnvironmentalImpact');
  const impact = await EnvironmentalImpact.deploy(deployer.address);
  await impact.waitForDeployment();
  deployments.impact = await impact.getAddress();
  console.log('✓ Environmental Impact:', deployments.impact);

  // 5. Set up cross-contract permissions
  console.log('\n=== Setting up Permissions ===');

  // Grant oracle operator role to marketplace for verification
  const OPERATOR_ROLE = await oracle.OPERATOR_ROLE();
  await oracle.grantRole(OPERATOR_ROLE, deployments.marketplace);
  console.log('✓ Oracle permissions set');

  // Verify marketplace issuer for oracle data
  await marketplace.verifyIssuer(deployments.oracle, true);
  console.log('✓ Marketplace permissions set');

  // Save deployment information
  const deploymentInfo = {
    ...deployments,
    network: network.name,
    deployer: deployer.address,
    timestamp: new Date().toISOString(),
    blockNumber: await ethers.provider.getBlockNumber(),
  };

  require('fs').writeFileSync(
    `deployments/${network.name}-ecosystem.json`,
    JSON.stringify(deploymentInfo, null, 2)
  );

  console.log('\n=== Deployment Complete ===');
  console.log('Deployment info saved to:', `deployments/${network.name}-ecosystem.json`);

  return deploymentInfo;
}

deployEcosystem().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
```

## Network-Specific Configurations

### Ethereum Mainnet

```javascript
// Mainnet configuration
const mainnetConfig = {
  gasPrice: ethers.parseUnits('20', 'gwei'), // Adjust based on network conditions
  gasLimit: 8000000,
  confirmations: 5, // Wait for more confirmations
  timeouts: {
    deployment: 300000, // 5 minutes
    verification: 600000, // 10 minutes
  },
};
```

### Polygon

```javascript
// Polygon configuration
const polygonConfig = {
  gasPrice: ethers.parseUnits('30', 'gwei'),
  gasLimit: 6000000,
  confirmations: 3,
  timeouts: {
    deployment: 120000, // 2 minutes
    verification: 300000, // 5 minutes
  },
};
```

### Layer 2 Solutions (Arbitrum, Optimism)

```javascript
// L2 configuration
const l2Config = {
  gasPrice: ethers.parseUnits('0.1', 'gwei'), // Much lower gas prices
  gasLimit: 10000000,
  confirmations: 1,
  timeouts: {
    deployment: 60000, // 1 minute
    verification: 180000, // 3 minutes
  },
};
```

## Verification and Testing

### Contract Verification

```javascript
// scripts/verify-contracts.js
async function verifyContract(contractAddress, constructorArgs, contractName) {
  try {
    await hre.run('verify:verify', {
      address: contractAddress,
      constructorArguments: constructorArgs,
      contract: contractName,
    });
    console.log(`✓ Verified ${contractName} at ${contractAddress}`);
  } catch (error) {
    console.log(`✗ Verification failed for ${contractName}:`, error.message);
  }
}

async function verifyAll() {
  const deployment = require(`../deployments/${network.name}-ecosystem.json`);

  // Verify each contract
  await verifyContract(
    deployment.governanceToken,
    ['Climate DAO', 'CDAO', 18, ethers.parseEther('100000000'), deployment.deployer],
    'contracts/tokens/GovernanceToken.sol:GovernanceToken'
  );

  await verifyContract(
    deployment.oracle,
    [deployment.deployer],
    'contracts/oracles/WeatherDataOracle.sol:WeatherDataOracle'
  );

  // Add more verifications...
}
```

### Post-Deployment Testing

```javascript
// scripts/test-deployment.js
async function testDeployment() {
  const deployment = require(`../deployments/${network.name}-ecosystem.json`);

  // Connect to deployed contracts
  const governanceToken = await ethers.getContractAt('GovernanceToken', deployment.governanceToken);
  const oracle = await ethers.getContractAt('WeatherDataOracle', deployment.oracle);
  const marketplace = await ethers.getContractAt('CarbonMarketplace', deployment.marketplace);

  console.log('Testing deployed contracts...');

  // Test governance token
  const totalSupply = await governanceToken.totalSupply();
  console.log('✓ Governance token total supply:', ethers.formatEther(totalSupply));

  // Test oracle
  const stationCount = await oracle.stationCount();
  console.log('✓ Oracle station count:', stationCount.toString());

  // Test marketplace
  const creditCount = await marketplace.creditCount();
  console.log('✓ Marketplace credit count:', creditCount.toString());

  console.log('All tests passed!');
}
```

## Monitoring and Maintenance

### Event Monitoring Setup

```javascript
// scripts/monitor-events.js
async function setupEventMonitoring() {
  const deployment = require(`../deployments/${network.name}-ecosystem.json`);

  const marketplace = await ethers.getContractAt('CarbonMarketplace', deployment.marketplace);

  // Monitor credit issuance
  marketplace.on('CreditIssued', (creditId, creditType, issuer, carbonOffsetTons) => {
    console.log(`New credit issued: ${creditId} by ${issuer} for ${carbonOffsetTons} tons`);
    // Send to monitoring system
  });

  // Monitor credit sales
  marketplace.on('CreditSold', (listingId, buyer, seller, totalPrice) => {
    console.log(`Credit sold: Listing ${listingId} for ${ethers.formatEther(totalPrice)} ETH`);
    // Send to monitoring system
  });
}
```

### Health Checks

```javascript
// scripts/health-check.js
async function healthCheck() {
  const deployment = require(`../deployments/${network.name}-ecosystem.json`);

  try {
    // Check contract accessibility
    const governanceToken = await ethers.getContractAt(
      'GovernanceToken',
      deployment.governanceToken
    );
    await governanceToken.totalSupply();
    console.log('✓ Governance token accessible');

    // Check oracle data freshness
    const oracle = await ethers.getContractAt('WeatherDataOracle', deployment.oracle);
    const dataCount = await oracle.dataCount();
    console.log(`✓ Oracle has ${dataCount} data points`);

    // Check marketplace activity
    const marketplace = await ethers.getContractAt('CarbonMarketplace', deployment.marketplace);
    const volume = await marketplace.totalVolumeTraded();
    console.log(`✓ Marketplace volume: ${volume} tons traded`);

    console.log('All health checks passed!');
    return true;
  } catch (error) {
    console.error('Health check failed:', error);
    return false;
  }
}
```

## Upgrade Procedures

### Proxy Pattern Upgrades (if using upgradeable contracts)

```javascript
// scripts/upgrade-contract.js
const { ethers, upgrades } = require('hardhat');

async function upgradeContract() {
  const deployment = require(`../deployments/${network.name}-ecosystem.json`);

  // Deploy new implementation
  const NewContract = await ethers.getContractFactory('ImprovedCarbonMarketplace');

  console.log('Upgrading marketplace...');
  const upgraded = await upgrades.upgradeProxy(deployment.marketplace, NewContract);

  console.log('Marketplace upgraded to:', await upgraded.getAddress());

  // Update deployment file
  deployment.marketplace = await upgraded.getAddress();
  deployment.lastUpgrade = new Date().toISOString();

  require('fs').writeFileSync(
    `deployments/${network.name}-ecosystem.json`,
    JSON.stringify(deployment, null, 2)
  );
}
```

## Troubleshooting

### Common Issues

1. **Gas Estimation Fails**

   ```javascript
   // Manually set gas limit
   const tx = await contract.someFunction({ gasLimit: 500000 });
   ```

2. **Verification Fails**

   ```bash
   # Try with specific compiler version
   npx hardhat verify --contract contracts/Token.sol:Token 0x... "Constructor" "Args"
   ```

3. **Network Connection Issues**

   ```javascript
   // Add retry logic
   async function deployWithRetry(factory, args, maxRetries = 3) {
     for (let i = 0; i < maxRetries; i++) {
       try {
         return await factory.deploy(...args);
       } catch (error) {
         if (i === maxRetries - 1) throw error;
         console.log(`Retry ${i + 1}/${maxRetries}`);
         await new Promise(resolve => setTimeout(resolve, 5000));
       }
     }
   }
   ```

4. **Role Setup Issues**
   ```javascript
   // Check role before granting
   const hasRole = await contract.hasRole(ROLE, address);
   if (!hasRole) {
     await contract.grantRole(ROLE, address);
   }
   ```

### Emergency Procedures

```javascript
// scripts/emergency-pause.js
async function emergencyPause() {
  const deployment = require(`../deployments/${network.name}-ecosystem.json`);

  const contracts = [
    { name: 'marketplace', address: deployment.marketplace },
    { name: 'tracker', address: deployment.tracker },
    { name: 'impact', address: deployment.impact },
  ];

  for (const contract of contracts) {
    try {
      const instance = await ethers.getContractAt('SecurityBase', contract.address);
      await instance.pause();
      console.log(`✓ Paused ${contract.name}`);
    } catch (error) {
      console.error(`✗ Failed to pause ${contract.name}:`, error.message);
    }
  }
}
```

Remember to always test deployments on testnets first, keep deployment records, and have emergency
procedures ready!
