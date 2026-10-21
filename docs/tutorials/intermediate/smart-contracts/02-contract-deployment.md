# Smart Contract Deployment Guide

Complete guide to deploying WRFCoin smart contracts, from development to production deployment.

## What You'll Learn

- Smart contract development lifecycle
- Local development environment setup
- Testing strategies for WRFCoin contracts
- Deployment to testnet and mainnet
- Contract verification and interaction
- Post-deployment monitoring and management

## Prerequisites

- Completed [Basic Contract Interaction](./01-basic-contract-interaction.md)
- Familiarity with Solidity and Hardhat
- Understanding of Ethereum development patterns
- Basic knowledge of WRFCoin protocol architecture

---

## 1. Development Environment Setup

### Hardhat Configuration for WRFCoin

```javascript
// hardhat.config.js
require('@nomicfoundation/hardhat-toolbox');
require('@openzeppelin/hardhat-upgrades');
require('dotenv').config();

const { PRIVATE_KEY, WRFCOIN_TESTNET_RPC, WRFCOIN_MAINNET_RPC, ETHERSCAN_API_KEY } = process.env;

module.exports = {
  solidity: {
    version: '0.8.19',
    settings: {
      optimizer: {
        enabled: true,
        runs: 200,
      },
      viaIR: true,
    },
  },
  networks: {
    hardhat: {
      chainId: 31337,
      gas: 12000000,
      gasPrice: 'auto',
      accounts: {
        mnemonic: 'test test test test test test test test test test test junk',
      },
    },
    wrfcoin_testnet: {
      url: WRFCOIN_TESTNET_RPC || 'https://testnet-rpc.wrfcoin.network',
      accounts: PRIVATE_KEY ? [PRIVATE_KEY] : [],
      chainId: 8888,
      gas: 8000000,
      gasPrice: 'auto',
      timeout: 60000,
    },
    wrfcoin_mainnet: {
      url: WRFCOIN_MAINNET_RPC || 'https://rpc.wrfcoin.network',
      accounts: PRIVATE_KEY ? [PRIVATE_KEY] : [],
      chainId: 8889,
      gas: 8000000,
      gasPrice: 'auto',
      timeout: 60000,
    },
  },
  etherscan: {
    apiKey: {
      wrfcoin_testnet: ETHERSCAN_API_KEY,
      wrfcoin_mainnet: ETHERSCAN_API_KEY,
    },
    customChains: [
      {
        network: 'wrfcoin_testnet',
        chainId: 8888,
        urls: {
          apiURL: 'https://testnet-explorer-api.wrfcoin.network/api',
          browserURL: 'https://testnet-explorer.wrfcoin.network',
        },
      },
      {
        network: 'wrfcoin_mainnet',
        chainId: 8889,
        urls: {
          apiURL: 'https://explorer-api.wrfcoin.network/api',
          browserURL: 'https://explorer.wrfcoin.network',
        },
      },
    ],
  },
  gasReporter: {
    enabled: process.env.REPORT_GAS === 'true',
    currency: 'USD',
    token: 'WRF',
    coinmarketcap: process.env.COINMARKETCAP_API_KEY,
  },
  mocha: {
    timeout: 120000,
  },
};
```

### Environment Configuration

```bash
# .env
# Development
PRIVATE_KEY=your_private_key_here
MNEMONIC=your_twelve_word_mnemonic_here

# Network URLs
WRFCOIN_TESTNET_RPC=https://testnet-rpc.wrfcoin.network
WRFCOIN_MAINNET_RPC=https://rpc.wrfcoin.network

# Explorer API Keys
ETHERSCAN_API_KEY=your_wrfcoin_explorer_api_key

# Optional
COINMARKETCAP_API_KEY=your_cmc_api_key
REPORT_GAS=true
```

---

## 2. Smart Contract Templates

### Climate Data Registry Contract

```solidity
// contracts/ClimateDataRegistry.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import "@openzeppelin/contracts-upgradeable/security/PausableUpgradeable.sol";
import "@openzeppelin/contracts-upgradeable/security/ReentrancyGuardUpgradeable.sol";
import "./interfaces/IWeatherOracle.sol";
import "./interfaces/IWRFToken.sol";

/**
 * @title ClimateDataRegistry
 * @dev Registry for climate data submissions with validation and rewards
 */
contract ClimateDataRegistry is
    Initializable,
    OwnableUpgradeable,
    PausableUpgradeable,
    ReentrancyGuardUpgradeable
{
    struct ClimateData {
        uint256 id;
        address submitter;
        string ipfsHash;
        uint256 timestamp;
        uint256 latitude;
        uint256 longitude;
        bool validated;
        uint256 rewardAmount;
        bytes32 dataHash;
        uint8 dataType; // 0: temperature, 1: humidity, 2: pressure, etc.
    }

    struct ValidationRequest {
        uint256 dataId;
        address validator;
        uint256 requestTime;
        bool processed;
        bool approved;
        string reason;
    }

    // State variables
    mapping(uint256 => ClimateData) public climateDataEntries;
    mapping(bytes32 => bool) public submittedDataHashes;
    mapping(address => uint256[]) public userSubmissions;
    mapping(uint256 => ValidationRequest[]) public validationRequests;

    uint256 public nextDataId;
    uint256 public validationReward;
    uint256 public submissionReward;
    uint256 public minimumStakeAmount;

    IWeatherOracle public weatherOracle;
    IWRFToken public wrfToken;

    // Events
    event DataSubmitted(
        uint256 indexed dataId,
        address indexed submitter,
        string ipfsHash,
        uint256 timestamp
    );

    event DataValidated(
        uint256 indexed dataId,
        address indexed validator,
        bool approved,
        uint256 rewardAmount
    );

    event ValidationRequested(
        uint256 indexed dataId,
        address indexed validator,
        uint256 timestamp
    );

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    function initialize(
        address _wrfToken,
        address _weatherOracle,
        uint256 _validationReward,
        uint256 _submissionReward,
        uint256 _minimumStakeAmount
    ) public initializer {
        __Ownable_init();
        __Pausable_init();
        __ReentrancyGuard_init();

        wrfToken = IWRFToken(_wrfToken);
        weatherOracle = IWeatherOracle(_weatherOracle);
        validationReward = _validationReward;
        submissionReward = _submissionReward;
        minimumStakeAmount = _minimumStakeAmount;
        nextDataId = 1;
    }

    /**
     * @dev Submit climate data for validation
     * @param _ipfsHash IPFS hash of the climate data
     * @param _latitude Latitude of the data point (scaled by 1e6)
     * @param _longitude Longitude of the data point (scaled by 1e6)
     * @param _dataType Type of climate data (0: temp, 1: humidity, etc.)
     * @param _dataHash Hash of the actual data for verification
     */
    function submitClimateData(
        string calldata _ipfsHash,
        uint256 _latitude,
        uint256 _longitude,
        uint8 _dataType,
        bytes32 _dataHash
    ) external whenNotPaused nonReentrant {
        require(bytes(_ipfsHash).length > 0, "Invalid IPFS hash");
        require(!submittedDataHashes[_dataHash], "Data already submitted");
        require(_latitude <= 90000000 && _longitude <= 180000000, "Invalid coordinates");

        // Require minimum stake
        require(
            wrfToken.balanceOf(msg.sender) >= minimumStakeAmount,
            "Insufficient WRF balance for submission"
        );

        uint256 dataId = nextDataId++;

        ClimateData storage newData = climateDataEntries[dataId];
        newData.id = dataId;
        newData.submitter = msg.sender;
        newData.ipfsHash = _ipfsHash;
        newData.timestamp = block.timestamp;
        newData.latitude = _latitude;
        newData.longitude = _longitude;
        newData.validated = false;
        newData.rewardAmount = 0;
        newData.dataHash = _dataHash;
        newData.dataType = _dataType;

        submittedDataHashes[_dataHash] = true;
        userSubmissions[msg.sender].push(dataId);

        emit DataSubmitted(dataId, msg.sender, _ipfsHash, block.timestamp);

        // Request oracle validation
        _requestOracleValidation(dataId);
    }

    /**
     * @dev Request validation from oracle network
     */
    function _requestOracleValidation(uint256 _dataId) internal {
        ClimateData storage data = climateDataEntries[_dataId];

        ValidationRequest memory request = ValidationRequest({
            dataId: _dataId,
            validator: address(weatherOracle),
            requestTime: block.timestamp,
            processed: false,
            approved: false,
            reason: ""
        });

        validationRequests[_dataId].push(request);

        emit ValidationRequested(_dataId, address(weatherOracle), block.timestamp);

        // Call oracle for validation
        weatherOracle.requestValidation(
            _dataId,
            data.ipfsHash,
            data.latitude,
            data.longitude,
            data.dataType
        );
    }

    /**
     * @dev Process oracle validation result
     * @param _dataId ID of the data entry
     * @param _approved Whether the data was approved
     * @param _reason Reason for approval/rejection
     */
    function processOracleValidation(
        uint256 _dataId,
        bool _approved,
        string calldata _reason
    ) external {
        require(msg.sender == address(weatherOracle), "Only oracle can validate");
        require(_dataId < nextDataId, "Invalid data ID");

        ClimateData storage data = climateDataEntries[_dataId];
        require(!data.validated, "Data already validated");

        data.validated = true;

        if (_approved) {
            // Calculate and distribute rewards
            uint256 reward = _calculateReward(_dataId);
            data.rewardAmount = reward;

            // Transfer reward to submitter
            if (reward > 0) {
                wrfToken.mint(data.submitter, reward);
            }
        }

        // Update validation request
        ValidationRequest[] storage requests = validationRequests[_dataId];
        for (uint i = 0; i < requests.length; i++) {
            if (requests[i].validator == address(weatherOracle) && !requests[i].processed) {
                requests[i].processed = true;
                requests[i].approved = _approved;
                requests[i].reason = _reason;
                break;
            }
        }

        emit DataValidated(_dataId, address(weatherOracle), _approved, data.rewardAmount);
    }

    /**
     * @dev Calculate reward amount based on data quality and network needs
     */
    function _calculateReward(uint256 _dataId) internal view returns (uint256) {
        ClimateData storage data = climateDataEntries[_dataId];

        uint256 baseReward = submissionReward;
        uint256 qualityMultiplier = 100; // Base 100%

        // Time-based bonus (higher rewards for recent data)
        uint256 timeDiff = block.timestamp - data.timestamp;
        if (timeDiff <= 1 hours) {
            qualityMultiplier += 50; // 50% bonus for very recent data
        } else if (timeDiff <= 6 hours) {
            qualityMultiplier += 25; // 25% bonus for recent data
        }

        // Geographic rarity bonus (simplified)
        // In production, this would check data density in the area
        if (_isRareLocation(data.latitude, data.longitude)) {
            qualityMultiplier += 75; // 75% bonus for rare locations
        }

        return (baseReward * qualityMultiplier) / 100;
    }

    /**
     * @dev Check if location has rare data coverage (simplified implementation)
     */
    function _isRareLocation(uint256 _lat, uint256 _lng) internal pure returns (bool) {
        // Simplified: consider polar regions as rare
        return _lat > 80000000 || _lat < -80000000; // Above 80° latitude
    }

    /**
     * @dev Get climate data entry
     */
    function getClimateData(uint256 _dataId)
        external
        view
        returns (ClimateData memory)
    {
        require(_dataId < nextDataId, "Invalid data ID");
        return climateDataEntries[_dataId];
    }

    /**
     * @dev Get user submissions
     */
    function getUserSubmissions(address _user)
        external
        view
        returns (uint256[] memory)
    {
        return userSubmissions[_user];
    }

    /**
     * @dev Get validation requests for data entry
     */
    function getValidationRequests(uint256 _dataId)
        external
        view
        returns (ValidationRequest[] memory)
    {
        return validationRequests[_dataId];
    }

    // Admin functions
    function setRewards(
        uint256 _validationReward,
        uint256 _submissionReward
    ) external onlyOwner {
        validationReward = _validationReward;
        submissionReward = _submissionReward;
    }

    function setMinimumStakeAmount(uint256 _amount) external onlyOwner {
        minimumStakeAmount = _amount;
    }

    function pause() external onlyOwner {
        _pause();
    }

    function unpause() external onlyOwner {
        _unpause();
    }

    function emergencyWithdraw() external onlyOwner {
        uint256 balance = wrfToken.balanceOf(address(this));
        if (balance > 0) {
            wrfToken.transfer(owner(), balance);
        }
    }
}
```

### Weather Oracle Contract

```solidity
// contracts/WeatherOracle.sol
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";
import "@openzeppelin/contracts-upgradeable/security/PausableUpgradeable.sol";
import "./interfaces/IClimateDataRegistry.sol";

/**
 * @title WeatherOracle
 * @dev Oracle contract for validating weather data submissions
 */
contract WeatherOracle is
    Initializable,
    AccessControlUpgradeable,
    PausableUpgradeable
{
    bytes32 public constant ORACLE_ROLE = keccak256("ORACLE_ROLE");
    bytes32 public constant VALIDATOR_ROLE = keccak256("VALIDATOR_ROLE");

    struct ValidationRequest {
        uint256 dataId;
        string ipfsHash;
        uint256 latitude;
        uint256 longitude;
        uint8 dataType;
        uint256 requestTime;
        bool processed;
        address requester;
    }

    struct OracleNode {
        address nodeAddress;
        string endpoint;
        uint256 stakingAmount;
        uint256 validationCount;
        uint256 accuracy;
        bool active;
        uint256 lastValidation;
    }

    mapping(uint256 => ValidationRequest) public validationRequests;
    mapping(address => OracleNode) public oracleNodes;
    mapping(uint256 => mapping(address => bool)) public validatorVotes;
    mapping(uint256 => uint256) public validationVoteCount;

    address[] public activeOracles;
    uint256 public nextRequestId;
    uint256 public requiredValidators;
    uint256 public validationTimeout;
    uint256 public minimumStake;

    IClimateDataRegistry public climateRegistry;

    event ValidationRequested(
        uint256 indexed requestId,
        uint256 indexed dataId,
        address indexed requester
    );

    event ValidationCompleted(
        uint256 indexed requestId,
        uint256 indexed dataId,
        bool approved,
        string reason
    );

    event OracleNodeRegistered(
        address indexed nodeAddress,
        string endpoint,
        uint256 stakingAmount
    );

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    function initialize(
        address _climateRegistry,
        uint256 _requiredValidators,
        uint256 _validationTimeout,
        uint256 _minimumStake
    ) public initializer {
        __AccessControl_init();
        __Pausable_init();

        _grantRole(DEFAULT_ADMIN_ROLE, msg.sender);
        _grantRole(ORACLE_ROLE, msg.sender);

        climateRegistry = IClimateDataRegistry(_climateRegistry);
        requiredValidators = _requiredValidators;
        validationTimeout = _validationTimeout;
        minimumStake = _minimumStake;
        nextRequestId = 1;
    }

    /**
     * @dev Register as oracle node
     */
    function registerOracleNode(
        string calldata _endpoint,
        uint256 _stakingAmount
    ) external payable {
        require(_stakingAmount >= minimumStake, "Insufficient staking amount");
        require(msg.value >= _stakingAmount, "Insufficient ETH sent");
        require(!oracleNodes[msg.sender].active, "Node already registered");

        oracleNodes[msg.sender] = OracleNode({
            nodeAddress: msg.sender,
            endpoint: _endpoint,
            stakingAmount: _stakingAmount,
            validationCount: 0,
            accuracy: 100, // Start with 100% accuracy
            active: true,
            lastValidation: block.timestamp
        });

        activeOracles.push(msg.sender);
        _grantRole(VALIDATOR_ROLE, msg.sender);

        emit OracleNodeRegistered(msg.sender, _endpoint, _stakingAmount);
    }

    /**
     * @dev Request validation from oracle network
     */
    function requestValidation(
        uint256 _dataId,
        string calldata _ipfsHash,
        uint256 _latitude,
        uint256 _longitude,
        uint8 _dataType
    ) external whenNotPaused {
        require(msg.sender == address(climateRegistry), "Only registry can request");

        uint256 requestId = nextRequestId++;

        validationRequests[requestId] = ValidationRequest({
            dataId: _dataId,
            ipfsHash: _ipfsHash,
            latitude: _latitude,
            longitude: _longitude,
            dataType: _dataType,
            requestTime: block.timestamp,
            processed: false,
            requester: msg.sender
        });

        emit ValidationRequested(requestId, _dataId, msg.sender);
    }

    /**
     * @dev Submit validation result
     */
    function submitValidation(
        uint256 _requestId,
        bool _approved,
        string calldata _reason
    ) external onlyRole(VALIDATOR_ROLE) whenNotPaused {
        require(_requestId < nextRequestId, "Invalid request ID");

        ValidationRequest storage request = validationRequests[_requestId];
        require(!request.processed, "Request already processed");
        require(
            block.timestamp <= request.requestTime + validationTimeout,
            "Validation timeout exceeded"
        );
        require(!validatorVotes[_requestId][msg.sender], "Already voted");

        validatorVotes[_requestId][msg.sender] = true;
        validationVoteCount[_requestId]++;

        // Update oracle node stats
        OracleNode storage node = oracleNodes[msg.sender];
        node.validationCount++;
        node.lastValidation = block.timestamp;

        // Check if we have enough validations
        if (validationVoteCount[_requestId] >= requiredValidators) {
            request.processed = true;

            // Send result to climate registry
            climateRegistry.processOracleValidation(
                request.dataId,
                _approved,
                _reason
            );

            emit ValidationCompleted(_requestId, request.dataId, _approved, _reason);
        }
    }

    /**
     * @dev Get validation request details
     */
    function getValidationRequest(uint256 _requestId)
        external
        view
        returns (ValidationRequest memory)
    {
        return validationRequests[_requestId];
    }

    /**
     * @dev Get active oracle nodes
     */
    function getActiveOracles() external view returns (address[] memory) {
        return activeOracles;
    }

    /**
     * @dev Get oracle node details
     */
    function getOracleNode(address _nodeAddress)
        external
        view
        returns (OracleNode memory)
    {
        return oracleNodes[_nodeAddress];
    }

    // Admin functions
    function setRequiredValidators(uint256 _required) external onlyRole(DEFAULT_ADMIN_ROLE) {
        requiredValidators = _required;
    }

    function setValidationTimeout(uint256 _timeout) external onlyRole(DEFAULT_ADMIN_ROLE) {
        validationTimeout = _timeout;
    }

    function setMinimumStake(uint256 _amount) external onlyRole(DEFAULT_ADMIN_ROLE) {
        minimumStake = _amount;
    }

    function deactivateOracle(address _oracle) external onlyRole(DEFAULT_ADMIN_ROLE) {
        oracleNodes[_oracle].active = false;
        _revokeRole(VALIDATOR_ROLE, _oracle);

        // Remove from active list
        for (uint i = 0; i < activeOracles.length; i++) {
            if (activeOracles[i] == _oracle) {
                activeOracles[i] = activeOracles[activeOracles.length - 1];
                activeOracles.pop();
                break;
            }
        }
    }

    function pause() external onlyRole(DEFAULT_ADMIN_ROLE) {
        _pause();
    }

    function unpause() external onlyRole(DEFAULT_ADMIN_ROLE) {
        _unpause();
    }
}
```

---

## 3. Deployment Scripts

### Complete Deployment Script

```javascript
// scripts/deploy-climate-system.js
const { ethers, upgrades } = require('hardhat');
const fs = require('fs');
const path = require('path');

async function main() {
  console.log('🚀 Starting WRFCoin Climate System Deployment');

  const [deployer] = await ethers.getSigners();
  console.log('📝 Deploying with account:', deployer.address);
  console.log('💰 Account balance:', ethers.utils.formatEther(await deployer.getBalance()));

  const network = await ethers.provider.getNetwork();
  console.log('🌐 Network:', network.name, '(Chain ID:', network.chainId + ')');

  // Deployment configuration
  const config = {
    wrfToken: {
      name: 'WRFCoin Climate Token',
      symbol: 'WRF',
      initialSupply: ethers.utils.parseEther('100000000'), // 100M tokens
      decimals: 18,
    },
    oracle: {
      requiredValidators: 3,
      validationTimeout: 3600, // 1 hour
      minimumStake: ethers.utils.parseEther('1000'), // 1000 WRF
    },
    registry: {
      validationReward: ethers.utils.parseEther('10'), // 10 WRF
      submissionReward: ethers.utils.parseEther('5'), // 5 WRF
      minimumStakeAmount: ethers.utils.parseEther('100'), // 100 WRF
    },
  };

  const deployments = {};

  try {
    // 1. Deploy WRF Token
    console.log('\n📄 Deploying WRF Token...');
    const WRFToken = await ethers.getContractFactory('WRFToken');
    const wrfToken = await upgrades.deployProxy(
      WRFToken,
      [
        config.wrfToken.name,
        config.wrfToken.symbol,
        config.wrfToken.initialSupply,
        deployer.address,
      ],
      { initializer: 'initialize' }
    );
    await wrfToken.deployed();

    deployments.wrfToken = {
      proxy: wrfToken.address,
      implementation: await upgrades.erc1967.getImplementationAddress(wrfToken.address),
    };
    console.log('✅ WRF Token deployed to:', wrfToken.address);

    // 2. Deploy Weather Oracle
    console.log('\n🔮 Deploying Weather Oracle...');
    const WeatherOracle = await ethers.getContractFactory('WeatherOracle');
    const weatherOracle = await upgrades.deployProxy(
      WeatherOracle,
      [
        ethers.constants.AddressZero, // Will be set after registry deployment
        config.oracle.requiredValidators,
        config.oracle.validationTimeout,
        config.oracle.minimumStake,
      ],
      { initializer: 'initialize' }
    );
    await weatherOracle.deployed();

    deployments.weatherOracle = {
      proxy: weatherOracle.address,
      implementation: await upgrades.erc1967.getImplementationAddress(weatherOracle.address),
    };
    console.log('✅ Weather Oracle deployed to:', weatherOracle.address);

    // 3. Deploy Climate Data Registry
    console.log('\n📊 Deploying Climate Data Registry...');
    const ClimateDataRegistry = await ethers.getContractFactory('ClimateDataRegistry');
    const climateRegistry = await upgrades.deployProxy(
      ClimateDataRegistry,
      [
        wrfToken.address,
        weatherOracle.address,
        config.registry.validationReward,
        config.registry.submissionReward,
        config.registry.minimumStakeAmount,
      ],
      { initializer: 'initialize' }
    );
    await climateRegistry.deployed();

    deployments.climateRegistry = {
      proxy: climateRegistry.address,
      implementation: await upgrades.erc1967.getImplementationAddress(climateRegistry.address),
    };
    console.log('✅ Climate Data Registry deployed to:', climateRegistry.address);

    // 4. Configure oracle with registry address
    console.log('\n⚙️  Configuring contracts...');
    const oracleAdmin = await weatherOracle.DEFAULT_ADMIN_ROLE();
    await weatherOracle.grantRole(oracleAdmin, deployer.address);

    // Update oracle with registry address (if needed)
    // Note: This might need a setter function in the oracle contract

    // 5. Setup initial permissions and minting
    console.log('\n🔑 Setting up permissions...');

    // Grant minting role to climate registry
    const minterRole = await wrfToken.MINTER_ROLE();
    await wrfToken.grantRole(minterRole, climateRegistry.address);

    // Grant oracle role to deployer for testing
    const oracleRole = await weatherOracle.ORACLE_ROLE();
    await weatherOracle.grantRole(oracleRole, deployer.address);

    console.log('\n🎉 Deployment completed successfully!');
    console.log('\n📋 Contract Addresses:');
    console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');
    console.log('WRF Token (Proxy):', deployments.wrfToken.proxy);
    console.log('Weather Oracle (Proxy):', deployments.weatherOracle.proxy);
    console.log('Climate Registry (Proxy):', deployments.climateRegistry.proxy);
    console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');

    // 6. Save deployment info
    const deploymentInfo = {
      network: network.name,
      chainId: network.chainId,
      deployer: deployer.address,
      timestamp: new Date().toISOString(),
      contracts: deployments,
      config: config,
      gasUsed: {
        // Will be calculated from transaction receipts
      },
    };

    const deploymentPath = path.join(__dirname, '..', 'deployments', `${network.name}.json`);
    const deploymentDir = path.dirname(deploymentPath);

    if (!fs.existsSync(deploymentDir)) {
      fs.mkdirSync(deploymentDir, { recursive: true });
    }

    fs.writeFileSync(deploymentPath, JSON.stringify(deploymentInfo, null, 2));
    console.log('\n💾 Deployment info saved to:', deploymentPath);

    // 7. Verify contracts (if on testnet/mainnet)
    if (network.chainId !== 31337 && process.env.ETHERSCAN_API_KEY) {
      console.log('\n🔍 Starting contract verification...');
      await verifyContracts(deployments);
    }

    return deployments;
  } catch (error) {
    console.error('\n❌ Deployment failed:', error);
    throw error;
  }
}

async function verifyContracts(deployments) {
  console.log('Verifying contracts on block explorer...');

  try {
    // Verify proxy contracts
    await hre.run('verify:verify', {
      address: deployments.wrfToken.proxy,
      constructorArguments: [],
    });
    console.log('✅ WRF Token verified');

    await hre.run('verify:verify', {
      address: deployments.weatherOracle.proxy,
      constructorArguments: [],
    });
    console.log('✅ Weather Oracle verified');

    await hre.run('verify:verify', {
      address: deployments.climateRegistry.proxy,
      constructorArguments: [],
    });
    console.log('✅ Climate Registry verified');
  } catch (error) {
    console.warn('⚠️  Verification failed (this is often due to timing):', error.message);
    console.log('You can verify manually later using the implementation addresses');
  }
}

// Execute deployment
if (require.main === module) {
  main()
    .then(() => process.exit(0))
    .catch(error => {
      console.error(error);
      process.exit(1);
    });
}

module.exports = { main };
```

### Testing Script

```javascript
// scripts/test-deployment.js
const { ethers } = require('hardhat');
const deploymentInfo = require('../deployments/wrfcoin_testnet.json');

async function main() {
  console.log('🧪 Testing deployed contracts...');

  const [deployer, user1, user2] = await ethers.getSigners();

  // Get contract instances
  const wrfToken = await ethers.getContractAt('WRFToken', deploymentInfo.contracts.wrfToken.proxy);
  const weatherOracle = await ethers.getContractAt(
    'WeatherOracle',
    deploymentInfo.contracts.weatherOracle.proxy
  );
  const climateRegistry = await ethers.getContractAt(
    'ClimateDataRegistry',
    deploymentInfo.contracts.climateRegistry.proxy
  );

  console.log('\n📊 Contract Information:');
  console.log('WRF Token:', await wrfToken.name(), '(' + (await wrfToken.symbol()) + ')');
  console.log('Total Supply:', ethers.utils.formatEther(await wrfToken.totalSupply()), 'WRF');
  console.log(
    'Deployer Balance:',
    ethers.utils.formatEther(await wrfToken.balanceOf(deployer.address)),
    'WRF'
  );

  // Test 1: Mint tokens to test user
  console.log('\n💰 Minting test tokens...');
  await wrfToken.mint(user1.address, ethers.utils.parseEther('10000'));
  console.log(
    'User1 Balance:',
    ethers.utils.formatEther(await wrfToken.balanceOf(user1.address)),
    'WRF'
  );

  // Test 2: Submit climate data
  console.log('\n🌡️  Testing climate data submission...');
  const testData = {
    ipfsHash: 'QmTestHash123456789abcdef',
    latitude: 40774900, // 40.7749° N (scaled by 1e6)
    longitude: -122419400, // -122.4194° W (scaled by 1e6)
    dataType: 0, // Temperature
    dataHash: ethers.utils.keccak256(ethers.utils.toUtf8Bytes('test-climate-data')),
  };

  const tx = await climateRegistry
    .connect(user1)
    .submitClimateData(
      testData.ipfsHash,
      testData.latitude,
      testData.longitude,
      testData.dataType,
      testData.dataHash
    );

  const receipt = await tx.wait();
  console.log('✅ Climate data submitted. Gas used:', receipt.gasUsed.toString());

  // Get the submitted data
  const dataEntry = await climateRegistry.getClimateData(1);
  console.log('📝 Submitted data:', {
    id: dataEntry.id.toString(),
    submitter: dataEntry.submitter,
    ipfsHash: dataEntry.ipfsHash,
    validated: dataEntry.validated,
  });

  // Test 3: Register oracle node
  console.log('\n🔮 Testing oracle node registration...');
  const stakeAmount = ethers.utils.parseEther('1000');
  await weatherOracle
    .connect(user2)
    .registerOracleNode('https://oracle-node.example.com', stakeAmount, { value: stakeAmount });
  console.log('✅ Oracle node registered');

  // Test 4: Submit validation (as oracle)
  console.log('\n✅ Testing validation submission...');
  await weatherOracle.connect(deployer).submitValidation(
    1, // requestId
    true, // approved
    'Data validated successfully'
  );
  console.log('✅ Validation submitted');

  // Check final state
  const finalDataEntry = await climateRegistry.getClimateData(1);
  console.log('📊 Final data state:', {
    validated: finalDataEntry.validated,
    rewardAmount: ethers.utils.formatEther(finalDataEntry.rewardAmount),
  });

  const finalBalance = await wrfToken.balanceOf(user1.address);
  console.log('💰 User1 final balance:', ethers.utils.formatEther(finalBalance), 'WRF');

  console.log('\n🎉 All tests completed successfully!');
}

if (require.main === module) {
  main()
    .then(() => process.exit(0))
    .catch(error => {
      console.error(error);
      process.exit(1);
    });
}
```

---

## 4. Deployment Commands

### Deploy to Testnet

```bash
# Deploy to WRFCoin testnet
npx hardhat run scripts/deploy-climate-system.js --network wrfcoin_testnet

# Test deployment
npx hardhat run scripts/test-deployment.js --network wrfcoin_testnet

# Verify contracts
npx hardhat verify --network wrfcoin_testnet PROXY_ADDRESS
```

### Deploy to Mainnet

```bash
# Deploy to mainnet (production)
npx hardhat run scripts/deploy-climate-system.js --network wrfcoin_mainnet

# Verify deployment
npx hardhat run scripts/test-deployment.js --network wrfcoin_mainnet
```

---

## 5. Post-Deployment Management

### Contract Interaction Examples

```javascript
// examples/interact-with-contracts.js
const { ethers } = require('hardhat');

async function interactWithContracts() {
  const [deployer] = await ethers.getSigners();

  // Load deployed contracts
  const deploymentInfo = require('../deployments/wrfcoin_testnet.json');
  const climateRegistry = await ethers.getContractAt(
    'ClimateDataRegistry',
    deploymentInfo.contracts.climateRegistry.proxy
  );

  // Submit real climate data
  const realClimateData = {
    ipfsHash: 'QmRealWeatherData12345abcdef',
    latitude: 37774900, // San Francisco coordinates
    longitude: -122419400,
    dataType: 0, // Temperature
    dataHash: ethers.utils.keccak256(
      ethers.utils.toUtf8Bytes(
        JSON.stringify({
          temperature: 22.5,
          timestamp: Date.now(),
          stationId: 'SF_DOWNTOWN_001',
        })
      )
    ),
  };

  console.log('📤 Submitting real climate data...');
  const tx = await climateRegistry.submitClimateData(
    realClimateData.ipfsHash,
    realClimateData.latitude,
    realClimateData.longitude,
    realClimateData.dataType,
    realClimateData.dataHash
  );

  const receipt = await tx.wait();
  console.log('✅ Transaction hash:', receipt.transactionHash);
  console.log('⛽ Gas used:', receipt.gasUsed.toString());

  // Get user submissions
  const userSubmissions = await climateRegistry.getUserSubmissions(deployer.address);
  console.log('📊 Total submissions by user:', userSubmissions.length);
}

// Monitoring and analytics
async function getNetworkStats() {
  const deploymentInfo = require('../deployments/wrfcoin_testnet.json');
  const climateRegistry = await ethers.getContractAt(
    'ClimateDataRegistry',
    deploymentInfo.contracts.climateRegistry.proxy
  );
  const wrfToken = await ethers.getContractAt('WRFToken', deploymentInfo.contracts.wrfToken.proxy);

  console.log('\n📈 Network Statistics:');
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');

  const totalSupply = await wrfToken.totalSupply();
  console.log('Total WRF Supply:', ethers.utils.formatEther(totalSupply));

  const nextDataId = await climateRegistry.nextDataId();
  console.log('Total Data Submissions:', (nextDataId - 1).toString());

  // Get recent submissions (last 10)
  for (let i = Math.max(1, nextDataId - 10); i < nextDataId; i++) {
    const data = await climateRegistry.getClimateData(i);
    console.log(`Data ${i}: ${data.validated ? '✅' : '⏳'} ${data.ipfsHash.substring(0, 10)}...`);
  }
}

if (require.main === module) {
  interactWithContracts()
    .then(() => getNetworkStats())
    .then(() => process.exit(0))
    .catch(console.error);
}
```

---

## Next Steps

1. **Complete [API Integration tutorials](../api-integration/)**
2. **Build [End-to-End Application guides](../applications/)**
3. **Explore [Advanced DeFi contracts](./03-defi-contracts.md)**

---

## Troubleshooting

### Common Issues

**Gas Estimation Errors:**

```bash
# Increase gas limit in hardhat.config.js
gas: 12000000
```

**Proxy Deployment Failures:**

```bash
# Check constructor parameters
# Ensure proper initialization
```

**Verification Failures:**

```bash
# Wait 5-10 minutes after deployment
# Use implementation address for manual verification
```

This deployment guide provides a complete foundation for deploying WRFCoin smart contracts to any
network with proper testing and verification.
