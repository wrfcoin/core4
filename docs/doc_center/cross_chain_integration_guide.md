# Wrfcoin Cross-Chain Integration Guide

**Date Created:** May 15, 2025  
**Last Edited:** May 26, 2025

## Introduction

This guide outlines the technical details for integrating Wrfcoin with other blockchain networks.
Wrfcoin's cross-chain capabilities enable weather data, computational resources, and WRF tokens to
seamlessly interact with the broader blockchain ecosystem.

## Architecture Overview

Wrfcoin's cross-chain integration layer is built on a multi-protocol architecture that supports
various interoperability solutions:

1. **Bridge Contracts**: Smart contracts that lock assets on one chain and mint wrapped versions on
   another
2. **Cross-Chain Messaging**: Protocol for transmitting data and instructions between chains
3. **State Verification**: Mechanism for validating the state of external chains
4. **Token Standards**: Compatibility layers for token representation across chains
5. **Message Relayers**: Decentralized network of nodes that transmit and verify cross-chain
   messages

## Supported Cross-Chain Protocols

Wrfcoin now supports **13 comprehensive cross-chain bridge protocols**, making it one of the most
interoperable blockchain networks for weather data and environmental intelligence:

### Primary Protocols

#### 1. LayerZero

Ultra-lightweight cross-chain messaging with omnichain architecture support.

#### 2. Axelar Network

General message passing and asset transfers across 50+ blockchain networks.

#### 3. Wormhole

High-throughput cross-chain messaging with guardian network validation.

### Specialized Integrations

#### 4. Polygon Bridge

Direct integration with Ethereum's leading L2 scaling solution.

#### 5. Arbitrum Bridge

Optimistic rollup connectivity for fast, low-cost transactions.

#### 6. Optimism Bridge

Ethereum L2 integration with fraud proof security.

#### 7. Avalanche Bridge

Multi-subnet connectivity and C-Chain integration.

#### 8. BSC Bridge

Binance Smart Chain integration for DeFi ecosystem access.

#### 9. Fantom Bridge

Opera network connectivity for high-speed transactions.

#### 10. Moonbeam Bridge

Polkadot parachain integration with Ethereum compatibility.

#### 11. Harmony Bridge

Sharding-based blockchain connectivity.

#### 12. Celo Bridge

Mobile-first blockchain integration for global accessibility.

#### 13. Near Rainbow Bridge

NEAR Protocol integration for developer-friendly dApps.

### Enhanced LayerZero Implementation

**Integration Example**:

```solidity
// LayerZero endpoint interface
interface ILayerZeroEndpoint {
    function send(
        uint16 _dstChainId,
        bytes calldata _destination,
        bytes calldata _payload,
        address payable _refundAddress,
        address _zroPaymentAddress,
        bytes calldata _adapterParams
    ) external payable;
}

// Wrfcoin cross-chain contract example
contract WrfcoinCrossChain {
    ILayerZeroEndpoint public endpoint;

    constructor(address _endpoint) {
        endpoint = ILayerZeroEndpoint(_endpoint);
    }

    // Send weather data hash to destination chain
    function sendWeatherData(
        uint16 dstChainId,
        bytes calldata destination,
        bytes32 weatherDataHash
    ) external payable {
        bytes memory payload = abi.encode(weatherDataHash);
        endpoint.send{value: msg.value}(
            dstChainId,
            destination,
            payload,
            payable(msg.sender),
            address(0x0),
            bytes("")
        );
    }
}
```

### 2. Axelar Network

Wrfcoin uses Axelar for general message passing and token transfers with high security guarantees.

**Integration Example**:

```solidity
// Axelar gateway interface
interface IAxelarGateway {
    function sendToken(
        string calldata destinationChain,
        string calldata destinationAddress,
        string calldata symbol,
        uint256 amount
    ) external;
}

// Wrfcoin Axelar integration example
contract WrfcoinAxelarBridge {
    IAxelarGateway public gateway;
    IERC20 public wrfToken;

    constructor(address _gateway, address _wrfToken) {
        gateway = IAxelarGateway(_gateway);
        wrfToken = IERC20(_wrfToken);
    }

    // Bridge WRF tokens to destination chain
    function bridgeTokens(
        string calldata destinationChain,
        string calldata destinationAddress,
        uint256 amount
    ) external {
        require(wrfToken.transferFrom(msg.sender, address(this), amount), "Transfer failed");
        wrfToken.approve(address(gateway), amount);
        gateway.sendToken(destinationChain, destinationAddress, "WRF", amount);
    }
}
```

### 3. Wormhole

Wrfcoin utilizes Wormhole for secure cross-chain messaging and token transfers.

**Integration Example**:

```solidity
// Wormhole bridge interface
interface IWormhole {
    function publishMessage(
        uint32 nonce,
        bytes memory payload,
        uint8 consistencyLevel
    ) external payable returns (uint64 sequence);
}

// Wrfcoin Wormhole integration example
contract WrfcoinWormholeBridge {
    IWormhole public wormhole;

    constructor(address _wormhole) {
        wormhole = IWormhole(_wormhole);
    }

    // Send computational task to destination chain
    function sendComputationalTask(
        bytes memory taskData,
        uint8 consistencyLevel
    ) external payable returns (uint64) {
        return wormhole.publishMessage{value: msg.value}(
            0, // nonce
            taskData,
            consistencyLevel
        );
    }
}
```

## Integration Steps

### 1. Setting Up Cross-Chain Communication

#### Initialize the Bridge Contracts

Deploy and configure the appropriate bridge contracts based on your target chains:

```javascript
// Example initialization with Axelar
const initAxelarBridge = async () => {
  // Deploy bridge contract
  const bridge = await deployContract('WrfcoinAxelarBridge', [
    AXELAR_GATEWAY_ADDRESS,
    WRF_TOKEN_ADDRESS,
  ]);

  // Register bridge with Wrfcoin registry
  await registerBridge(bridge.address, 'axelar', [
    'ethereum',
    'polygon',
    'avalanche',
    'fantom',
    'arbitrum',
  ]);

  return bridge;
};
```

#### Configure Message Channels

Set up the communication channels between chains:

```javascript
// Configure cross-chain channels
const setupChannels = async bridge => {
  // Register source and destination endpoints
  await bridge.registerChannel(
    'ethereum', // source chain
    'polygon', // destination chain
    {
      sourceAddress: ETHEREUM_BRIDGE_ADDRESS,
      destinationAddress: POLYGON_BRIDGE_ADDRESS,
      gas: 300000,
      feeToken: ETH_ADDRESS,
    }
  );

  // Add relayers for this channel
  await bridge.addRelayers([RELAYER_1_ADDRESS, RELAYER_2_ADDRESS, RELAYER_3_ADDRESS]);
};
```

### 2. Implementing Token Bridges

#### Token Registration

Register WRF tokens and their wrapped versions on each chain:

```javascript
// Register tokens across chains
const registerTokens = async bridge => {
  // Register native WRF token
  await bridge.registerToken({
    symbol: 'WRF',
    name: 'Wrfcoin Token',
    decimals: 18,
    address: WRF_TOKEN_ADDRESS,
  });

  // Register wrapped tokens on destination chains
  await bridge.registerWrappedToken({
    sourceChain: 'wrfcoin',
    symbol: 'WRF',
    destinationChain: 'ethereum',
    wrappedAddress: ETH_WRAPPED_WRF_ADDRESS,
  });
};
```

#### Token Transfer Implementation

Implement the token transfer functions:

```javascript
// Transfer WRF tokens to destination chain
const transferTokens = async (bridge, destinationChain, destinationAddress, amount) => {
  // Approve tokens for bridge
  const wrfToken = new ethers.Contract(WRF_TOKEN_ADDRESS, ERC20_ABI, signer);
  await wrfToken.approve(bridge.address, amount);

  // Execute bridge transfer
  const tx = await bridge.bridgeTokens(destinationChain, destinationAddress, amount, {
    gasLimit: 500000,
    value: ethers.utils.parseEther('0.01'), // fee for cross-chain transfer
  });

  return tx.wait();
};
```

### 3. Cross-Chain Data Messaging

#### Weather Data Transmission

Send weather data hashes or IPFS references across chains:

```javascript
// Send weather data reference cross-chain
const sendWeatherData = async (bridge, destinationChain, data) => {
  // Package the data
  const weatherData = {
    timestamp: Math.floor(Date.now() / 1000),
    region: data.region,
    dataType: data.dataType,
    ipfsHash: data.ipfsHash,
    signature: await signData(data),
  };

  // Encode for cross-chain transmission
  const encodedData = ethers.utils.defaultAbiCoder.encode(
    ['uint256', 'string', 'string', 'string', 'bytes'],
    [
      weatherData.timestamp,
      weatherData.region,
      weatherData.dataType,
      weatherData.ipfsHash,
      weatherData.signature,
    ]
  );

  // Send cross-chain message
  const tx = await bridge.sendMessage(
    destinationChain,
    WEATHER_ORACLE_ADDRESS, // destination contract
    encodedData,
    {
      gasLimit: 300000,
      value: ethers.utils.parseEther('0.005'), // message fee
    }
  );

  return tx.wait();
};
```

#### Computational Task Distribution

Distribute computational tasks across chains:

```javascript
// Send computational task to destination chain
const distributeTask = async (bridge, destinationChain, task) => {
  // Package the task
  const taskPackage = {
    taskId: task.id,
    modelType: task.modelType,
    inputParameters: task.parameters,
    requiredResources: {
      cpuCores: task.resources.cpu,
      memoryMB: task.resources.memory,
      gpuRequired: task.resources.gpu,
    },
    reward: task.reward,
    deadline: task.deadline,
  };

  // Encode for cross-chain transmission
  const encodedTask = ethers.utils.defaultAbiCoder.encode(
    ['string', 'string', 'string', 'tuple(uint8,uint32,bool)', 'uint256', 'uint256'],
    [
      taskPackage.taskId,
      taskPackage.modelType,
      JSON.stringify(taskPackage.inputParameters),
      [
        taskPackage.requiredResources.cpuCores,
        taskPackage.requiredResources.memoryMB,
        taskPackage.requiredResources.gpuRequired,
      ],
      taskPackage.reward,
      taskPackage.deadline,
    ]
  );

  // Send cross-chain task
  const tx = await bridge.sendComputationalTask(
    destinationChain,
    COMPUTE_COORDINATOR_ADDRESS, // destination contract
    encodedTask,
    {
      gasLimit: 500000,
      value: ethers.utils.parseEther('0.008'), // message fee
    }
  );

  return tx.wait();
};
```

### 4. Handling Incoming Messages

#### Message Receiver Implementation

Implement receivers for cross-chain messages:

```solidity
// Example receiver contract for weather data
contract WeatherDataReceiver {
    // Event emitted when new weather data is received
    event WeatherDataReceived(
        uint256 timestamp,
        string region,
        string dataType,
        string ipfsHash
    );

    // Mapping to track processed message IDs
    mapping(bytes32 => bool) public processedMessages;

    // Authorized source contracts on other chains
    mapping(string => address) public trustedSources;

    constructor() {
        // Initialize trusted sources
        trustedSources["wrfcoin"] = 0x1234...5678; // Address on WRF chain
    }

    // Process incoming weather data from Axelar
    function processWeatherData(
        string memory sourceChain,
        string memory sourceAddress,
        bytes calldata payload
    ) external {
        // Verify source is trusted
        require(trustedSources[sourceChain] == stringToAddress(sourceAddress),
                "Untrusted source");

        // Decode the payload
        (
            uint256 timestamp,
            string memory region,
            string memory dataType,
            string memory ipfsHash,
            bytes memory signature
        ) = abi.decode(payload, (uint256, string, string, string, bytes));

        // Create and check message ID to prevent duplicates
        bytes32 messageId = keccak256(abi.encodePacked(
            sourceChain,
            sourceAddress,
            timestamp,
            region,
            dataType,
            ipfsHash
        ));
        require(!processedMessages[messageId], "Message already processed");
        processedMessages[messageId] = true;

        // Verify the signature (implementation depends on verification scheme)
        require(verifySignature(payload, signature), "Invalid signature");

        // Process the weather data
        // ...

        // Emit event
        emit WeatherDataReceived(timestamp, region, dataType, ipfsHash);
    }

    // Helper function to convert string address to address type
    function stringToAddress(string memory s) internal pure returns (address) {
        // Implementation omitted for brevity
    }

    // Verify signature from trusted source
    function verifySignature(bytes memory data, bytes memory signature)
        internal view returns (bool) {
        // Implementation omitted for brevity
    }
}
```

### 5. Security Considerations

#### Message Verification

Implement robust verification for cross-chain messages:

```solidity
// Message verification for Wormhole integration
contract WormholeMessageVerifier {
    IWormhole public wormhole;

    // Mapping of trusted emitters by chain ID
    mapping(uint16 => bytes32) public trustedEmitters;

    constructor(address _wormhole) {
        wormhole = IWormhole(_wormhole);

        // Set trusted emitters
        trustedEmitters[1] = 0x1234...5678; // Ethereum emitter
        trustedEmitters[2] = 0xabcd...ef01; // Polygon emitter
        // ... other chains
    }

    // Verify incoming Wormhole message
    function verifyMessage(bytes memory encodedMessage)
        public view returns (bool valid, bytes memory payload) {

        // Parse and verify Wormhole message
        (IWormhole.VM memory vm, bool valid, string memory reason) = wormhole.parseAndVerifyVM(encodedMessage);

        if (!valid) {
            return (false, bytes(reason));
        }

        // Verify the emitter is trusted for the source chain
        if (vm.emitterAddress != trustedEmitters[vm.emitterChainId]) {
            return (false, bytes("Untrusted emitter"));
        }

        return (true, vm.payload);
    }
}
```

#### Replay Protection

Implement controls to prevent replay attacks:

```solidity
// Replay protection for cross-chain messages
contract ReplayProtection {
    // Mapping of processed message IDs
    mapping(bytes32 => bool) public processedMessages;

    // Check and mark message as processed
    function checkAndMarkAsProcessed(bytes32 messageId) internal returns (bool) {
        if (processedMessages[messageId]) {
            return false;
        }

        processedMessages[messageId] = true;
        return true;
    }

    // Generate message ID from components
    function generateMessageId(
        string memory sourceChain,
        address sourceAddress,
        uint256 nonce,
        bytes memory payload
    ) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked(
            sourceChain,
            sourceAddress,
            nonce,
            payload
        ));
    }
}
```

## Chain-Specific Integrations

### 1. Ethereum Integration

```javascript
// Example Ethereum integration
const ethereumConfig = {
  chainId: 1,
  wrfTokenAddress: '0x1234...5678', // Wrapped WRF on Ethereum
  bridgeAddress: '0xabcd...ef01',
  axelarGateway: '0x2345...6789',
  gasService: '0x3456...789a',
  gasLimit: 500000,
  estimateGasPrice: async () => {
    // Implementation to estimate gas price
  },
};
```

### 2. Avalanche Integration

```javascript
// Example Avalanche integration
const avalancheConfig = {
  chainId: 43114,
  wrfTokenAddress: '0x2345...6789', // Wrapped WRF on Avalanche
  bridgeAddress: '0xbcde...f012',
  axelarGateway: '0x3456...789a',
  gasService: '0x4567...89ab',
  gasLimit: 2000000,
  estimateGasPrice: async () => {
    // Implementation to estimate gas price
  },
};
```

### 3. Polkadot/Parity Integration

For Polkadot/Substrate-based chains:

```javascript
// Example Polkadot parachain integration
const polkadotConfig = {
  parachainId: 2000,
  wrfAssetId: 42, // WRF token asset ID
  endpoints: ['wss://rpc.polkadot.io', 'wss://backup.polkadot.io'],
  xcmDestinations: {
    acala: { paraId: 2000, format: 'V2' },
    moonbeam: { paraId: 2004, format: 'V2' },
  },
  connectToChain: async () => {
    // Implementation to connect to Polkadot
    const { ApiPromise, WsProvider } = require('@polkadot/api');
    const provider = new WsProvider(polkadotConfig.endpoints[0]);
    const api = await ApiPromise.create({ provider });
    return api;
  },
};
```

### 4. Near Protocol Integration

```javascript
// Example Near Protocol integration
const nearConfig = {
  networkId: 'mainnet',
  nodeUrl: 'https://rpc.mainnet.near.org',
  wrfTokenAccount: 'wrf.near',
  bridgeAccount: 'bridge.near',
  connectToNear: async () => {
    // Implementation to connect to Near Protocol
    const nearAPI = require('near-api-js');
    const { connect } = nearAPI;
    return connect({
      networkId: nearConfig.networkId,
      nodeUrl: nearConfig.nodeUrl,
      walletUrl: 'https://wallet.near.org',
      helperUrl: 'https://helper.mainnet.near.org',
    });
  },
};
```

## Event Monitoring

### Setting Up Cross-Chain Event Listeners

```javascript
// Initialize cross-chain event listeners
const setupEventListeners = async bridge => {
  // Listen for incoming token transfers
  bridge.on(
    'TokenReceived',
    (sourceChain, sourceAddress, destinationAddress, symbol, amount, event) => {
      console.log(`Received ${amount} ${symbol} from ${sourceChain}`);
      // Process token receipt
      processTokenReceipt(sourceChain, sourceAddress, destinationAddress, symbol, amount);
    }
  );

  // Listen for computational task results
  bridge.on('ComputationalResultReceived', (sourceChain, taskId, resultHash, event) => {
    console.log(`Received result for task ${taskId} from ${sourceChain}`);
    // Process computational result
    processComputationalResult(sourceChain, taskId, resultHash);
  });
};
```

## Error Handling and Recovery

### Implementing Message Recovery

```javascript
// Recover failed cross-chain messages
const recoverFailedMessages = async (bridge, messageIds) => {
  for (const messageId of messageIds) {
    // Get message status
    const status = await bridge.getMessageStatus(messageId);

    if (status === 'failed') {
      // Get message details
      const messageDetails = await bridge.getMessageDetails(messageId);

      // Retry sending the message
      const tx = await bridge.retryMessage(messageId, {
        gasLimit: 500000,
        value: ethers.utils.parseEther('0.02'), // increased fee for retry
      });

      console.log(`Retried message ${messageId}, tx hash: ${tx.hash}`);
    }
  }
};
```

## Advanced Features

### Cross-Chain Governance

Implement cross-chain governance for Wrfcoin protocol parameters:

```javascript
// Register cross-chain governance proposal
const submitCrossChainProposal = async (bridge, proposal) => {
  // Package the proposal
  const governanceProposal = {
    proposalId: proposal.id,
    title: proposal.title,
    description: proposal.description,
    targetChains: proposal.targetChains,
    parameterChanges: proposal.parameters,
    votingPeriod: proposal.votingPeriod,
  };

  // Encode for cross-chain transmission
  const encodedProposal = ethers.utils.defaultAbiCoder.encode(
    ['string', 'string', 'string', 'string[]', 'bytes', 'uint256'],
    [
      governanceProposal.proposalId,
      governanceProposal.title,
      governanceProposal.description,
      governanceProposal.targetChains,
      ethers.utils.toUtf8Bytes(JSON.stringify(governanceProposal.parameterChanges)),
      governanceProposal.votingPeriod,
    ]
  );

  // Send cross-chain proposal
  const tx = await bridge.submitGovernanceProposal(encodedProposal, {
    gasLimit: 500000,
    value: ethers.utils.parseEther('0.01'),
  });

  return tx.wait();
};
```

### Cross-Chain Data Verification

Verify data authenticity across chains:

```javascript
// Cross-chain data verification
contract CrossChainVerifier {
    // Mapping of verified data hashes
    mapping(bytes32 => bool) public verifiedData;

    // Event for verified data
    event DataVerified(bytes32 dataHash, string sourceChain, uint256 timestamp);

    // Verify data from multiple chains
    function verifyDataFromMultipleChains(
        bytes32 dataHash,
        DataAttestation[] calldata attestations
    ) external returns (bool) {
        // Require at least 3 attestations
        require(attestations.length >= 3, "Insufficient attestations");

        // Verify each attestation
        uint validAttestations = 0;
        for (uint i = 0; i < attestations.length; i++) {
            if (verifyAttestation(dataHash, attestations[i])) {
                validAttestations++;
            }
        }

        // Require majority of attestations to be valid
        bool isVerified = validAttestations >= (attestations.length / 2 + 1);

        if (isVerified) {
            verifiedData[dataHash] = true;
            emit DataVerified(dataHash, "multiple", block.timestamp);
        }

        return isVerified;
    }

    // Verify individual attestation
    function verifyAttestation(
        bytes32 dataHash,
        DataAttestation calldata attestation
    ) internal pure returns (bool) {
        // Validation logic omitted for brevity
        return true;
    }

    // Data attestation struct
    struct DataAttestation {
        string sourceChain;
        address validator;
        bytes signature;
        uint256 timestamp;
    }
}
```

## Testing and Integration

### Test Environment Setup

```javascript
// Set up cross-chain test environment
const setupTestEnvironment = async () => {
  // Initialize local blockchain instances
  const wrfChain = await startLocalChain('wrfcoin', 9545);
  const ethChain = await startLocalChain('ethereum', 8545);
  const avaxChain = await startLocalChain('avalanche', 7545);

  // Deploy test tokens
  const wrfToken = await deployToken(wrfChain, 'WRF', 'Wrfcoin Token', 18);
  const wrappedWrfEth = await deployToken(ethChain, 'WWRF', 'Wrapped WRF', 18);
  const wrappedWrfAvax = await deployToken(avaxChain, 'WWRF', 'Wrapped WRF', 18);

  // Deploy bridge contracts
  const wrfBridge = await deployBridge(wrfChain, wrfToken.address);
  const ethBridge = await deployBridge(ethChain, wrappedWrfEth.address);
  const avaxBridge = await deployBridge(avaxChain, wrappedWrfAvax.address);

  // Configure relayers
  const relayers = await setupRelayers([wrfChain, ethChain, avaxChain]);

  // Connect bridges
  await connectBridges(wrfBridge, ethBridge, avaxBridge, relayers);

  return {
    chains: { wrfChain, ethChain, avaxChain },
    tokens: { wrfToken, wrappedWrfEth, wrappedWrfAvax },
    bridges: { wrfBridge, ethBridge, avaxBridge },
    relayers,
  };
};
```

### Integration Tests

```javascript
// Run cross-chain integration tests
const runIntegrationTests = async testEnv => {
  // Test token transfers
  await testTokenTransfer(
    testEnv.chains.wrfChain,
    testEnv.chains.ethChain,
    testEnv.bridges.wrfBridge,
    testEnv.bridges.ethBridge,
    testEnv.tokens.wrfToken,
    testEnv.tokens.wrappedWrfEth,
    ethers.utils.parseEther('100')
  );

  // Test data messaging
  await testDataMessaging(
    testEnv.chains.wrfChain,
    testEnv.chains.avaxChain,
    testEnv.bridges.wrfBridge,
    testEnv.bridges.avaxBridge
  );

  // Test computational task distribution
  await testComputationalTaskDistribution(
    testEnv.chains.wrfChain,
    testEnv.chains.ethChain,
    testEnv.bridges.wrfBridge,
    testEnv.bridges.ethBridge
  );
};
```

## Support Resources

For additional support on cross-chain integrations:

- Documentation: [docs.wrfcoin.network/cross-chain](https://docs.wrfcoin.network/cross-chain)
- Developer forum:
  [forum.wrfcoin.network/category/cross-chain](https://forum.wrfcoin.network/category/cross-chain)
- GitHub: [github.com/wrfcoin/cross-chain-bridges](https://github.com/wrfcoin/cross-chain-bridges)
- Discord: [discord.wrfcoin.network](https://discord.wrfcoin.network) (Channel: #cross-chain-dev)
