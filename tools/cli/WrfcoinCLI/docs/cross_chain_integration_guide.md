# Wrfcoin Cross-Chain Integration Guide

This guide explains how to use Wrfcoin CLI to interact with multiple blockchain networks, enabling
seamless cross-chain operations for weather data, computational tasks, and value transfers.

## Overview

Wrfcoin's cross-chain capabilities allow for:

- Transferring WRF tokens across different blockchain networks
- Bridging NFTs containing weather data and computational models
- Accessing and utilizing computational resources from different chains
- Creating cross-chain oracle feeds for weather data
- Executing weather model computations across multiple networks

## Prerequisites

- Wrfcoin CLI installed
- A funded Wrfcoin wallet
- Wallets on supported target chains (Ethereum, Polkadot, NEAR, etc.)
- Basic understanding of blockchain bridges and cross-chain protocols

## Supported Blockchain Networks

| Network  | Bridge Technology | Supported Assets   | Best For                           |
| -------- | ----------------- | ------------------ | ---------------------------------- |
| Ethereum | Axelar, LayerZero | WRF, NFTs, Data    | DeFi integration, NFT marketplaces |
| Polkadot | Substrate Bridge  | WRF, Data          | Parachain integration, governance  |
| NEAR     | Rainbow Bridge    | WRF, NFTs, Compute | Developer ecosystem, climate dApps |
| Filecoin | FVM Bridge        | Data Storage       | Long-term data archival            |
| Cosmos   | IBC               | WRF, Data          | Inter-blockchain communication     |
| Solana   | Wormhole          | WRF, NFTs          | High throughput applications       |

## Setting Up Cross-Chain Operations

### Initialize Cross-Chain Configuration

Before performing cross-chain operations, set up your configuration:

```bash
wrfcoin crosschain init \
  --default-wallet "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678" \
  --networks "ethereum,polkadot,near" \
  --config-file "crosschain_config.json"
```

### List Available Bridges

To see all available cross-chain bridges:

```bash
wrfcoin crosschain list-bridges
```

## Token Transfers

### Transfer WRF to Ethereum

```bash
wrfcoin crosschain transfer \
  --to-chain ethereum \
  --to-address "0x1234567890abcdef1234567890abcdef12345678" \
  --amount 10.5 \
  --provider axelar \
  --gas-limit 300000 \
  --gas-price 0.01
```

### Transfer WRF from Ethereum to Wrfcoin

```bash
wrfcoin crosschain receive \
  --from-chain ethereum \
  --from-tx "0xabcdef1234567890abcdef1234567890abcdef1234567890" \
  --provider axelar
```

### Check Cross-Chain Transaction Status

```bash
wrfcoin crosschain transaction-info \
  --hash "xc_tx_12345" \
  --provider axelar
```

## NFT Cross-Chain Operations

### Bridge Weather Data NFT to NEAR

```bash
wrfcoin crosschain transfer-nft \
  --nft-id "wrf_nft_12345" \
  --to-chain near \
  --to-account "example.near" \
  --provider rainbow_bridge \
  --gas-limit 500000
```

### Create Cross-Chain Computational NFT

```bash
# First create the NFT on Wrfcoin
NFT_ID=$(wrfcoin nft create \
  --title "Cross-Chain Weather Model" \
  --description "Machine learning model for global temperature prediction" \
  --type computational \
  --file "model_spec.json" \
  --metadata-file "model_metadata.json" \
  --creator-address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678" \
  --cross-chain-compatible \
  --json | jq -r '.id')

# Then make it available on multiple chains
wrfcoin crosschain deploy-nft \
  --nft-id "$NFT_ID" \
  --target-chains "ethereum,near,polkadot" \
  --providers "axelar,rainbow_bridge,substrate_bridge"
```

## Data Storage and Retrieval

### Store Weather Data on Filecoin via Cross-Chain

```bash
wrfcoin crosschain store-data \
  --data-file "weather_dataset_2025.nc" \
  --to-chain filecoin \
  --storage-period 180 \
  --redundancy high \
  --provider filecoin-wrfcoin-bridge \
  --metadata-file "dataset_metadata.json"
```

### Retrieve Weather Data from Filecoin

```bash
wrfcoin crosschain retrieve-data \
  --cid "QmX9dsf7e7bfea7d..." \
  --from-chain filecoin \
  --output-file "retrieved_weather_data.nc" \
  --provider filecoin-wrfcoin-bridge
```

## Cross-Chain Computation

### Execute Computation on Near Protocol

```bash
wrfcoin crosschain execute-computation \
  --nft-id "wrf_nft_12345" \
  --execution-chain near \
  --input-data-id "QmX9dsf7e7bfea7d..." \
  --input-chain filecoin \
  --result-chain wrfcoin \
  --compute-budget 50 \
  --timeout 3600
```

### Monitor Cross-Chain Computation

```bash
wrfcoin monitor cross-chain-tasks \
  --interval 10 \
  --task-id "xc_task_12345" \
  --format text
```

## Oracle Feeds for Weather Data

### Create Cross-Chain Oracle Feed

```bash
wrfcoin crosschain create-oracle-feed \
  --title "Global Temperature Index" \
  --description "Daily global temperature average" \
  --update-frequency 3600 \
  --parameters "temperature_2m" \
  --target-chains "ethereum,polkadot" \
  --providers "chainlink,substrate_oracle" \
  --data-source "wrfcoin:global_index"
```

### Update Oracle Feed Data

```bash
wrfcoin crosschain update-oracle-feed \
  --feed-id "oracle_feed_12345" \
  --data-file "temperature_update.json" \
  --timestamp "$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
```

## Integration with Interledger Protocol (ILP)

### Set Up Interledger Payment Pointers

```bash
# Register a payment pointer
PAYMENT_POINTER=$(wrfcoin crosschain ilp-register-pointer \
  --name "WeatherDataProvider" \
  --wallet-address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678" \
  --json | jq -r '.paymentPointer')

echo "Registered payment pointer: $PAYMENT_POINTER"
```

### Stream Payments for Real-time Weather Data

```bash
wrfcoin crosschain ilp-stream \
  --destination "$PAYMENT_POINTER" \
  --total-amount 0.1 \
  --interval-ms 1000 \
  --from-chain wrfcoin \
  --from-address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678"
```

## Advanced Cross-Chain Scenarios

### Setting Up Multi-Chain Weather Data Network

```bash
# Setup data collection nodes on multiple chains
wrfcoin crosschain setup-data-network \
  --name "Global Weather Net" \
  --chains "wrfcoin,near,polkadot" \
  --node-count-per-chain 3 \
  --data-sync-interval 300 \
  --consensus-method "multi-chain-voting" \
  --config-file "multi_chain_config.json"
```

### Cross-Chain Governance Vote

```bash
wrfcoin crosschain governance-vote \
  --proposal-id "xc_prop_12345" \
  --vote "approve" \
  --from-chains "wrfcoin,ethereum,polkadot" \
  --voting-power-calculation "token-weighted" \
  --reason "Improved cross-chain data availability"
```

## Example Cross-Chain Workflows

### Weather Data Publishing Workflow

```bash
# 1. Collect weather data
wrfcoin weather collect-data \
  --region "North America" \
  --parameters "temperature,humidity,pressure,wind" \
  --time-period "24h" \
  --output-file "weather_data_24h.nc"

# 2. Store data on Filecoin
STORAGE_ID=$(wrfcoin crosschain store-data \
  --data-file "weather_data_24h.nc" \
  --to-chain filecoin \
  --storage-period 180 \
  --redundancy high \
  --provider filecoin-wrfcoin-bridge \
  --json | jq -r '.cid')

# 3. Create a cross-chain NFT referencing the data
NFT_ID=$(wrfcoin nft create \
  --title "North America Weather Data" \
  --description "24-hour weather data for North America" \
  --type data \
  --linked-storage "$STORAGE_ID" \
  --cross-chain-compatible \
  --creator-address "Wrf1AbCdEfGhIjKlMnOpQrStUvWxYz12345678" \
  --json | jq -r '.id')

# 4. Make the NFT available on multiple chains
wrfcoin crosschain deploy-nft \
  --nft-id "$NFT_ID" \
  --target-chains "ethereum,near" \
  --providers "axelar,rainbow_bridge"

# 5. Update oracle feeds with the data
wrfcoin crosschain update-oracle-feed \
  --feed-id "oracle_feed_12345" \
  --linked-nft "$NFT_ID" \
  --timestamp "$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
```

### Cross-Chain Computational Workflow

```bash
# 1. Identify a computational model NFT on Ethereum
MODEL_NFT=$(wrfcoin crosschain find-nft \
  --from-chain ethereum \
  --type computational \
  --keywords "climate,neural-network" \
  --json | jq -r '.id')

# 2. Bring the model to Wrfcoin
WRFCOIN_NFT=$(wrfcoin crosschain transfer-nft \
  --nft-id "$MODEL_NFT" \
  --from-chain ethereum \
  --to-chain wrfcoin \
  --provider axelar \
  --json | jq -r '.id')

# 3. Get input data from Filecoin
wrfcoin crosschain retrieve-data \
  --cid "QmX9dsf7e7bfea7d..." \
  --from-chain filecoin \
  --output-file "input_data.nc" \
  --provider filecoin-wrfcoin-bridge

# 4. Execute computation on NEAR
TASK_ID=$(wrfcoin crosschain execute-computation \
  --nft-id "$WRFCOIN_NFT" \
  --execution-chain near \
  --input-file "input_data.nc" \
  --result-chain wrfcoin \
  --compute-budget 50 \
  --timeout 3600 \
  --json | jq -r '.id')

# 5. Wait for computation to complete
wrfcoin compute wait-for-completion --id "$TASK_ID"

# 6. Get computational results
wrfcoin compute get-results --id "$TASK_ID" --output-file "computation_results.nc"

# 7. Store results on multiple chains
wrfcoin crosschain store-results \
  --task-id "$TASK_ID" \
  --target-chains "wrfcoin,ethereum,near" \
  --providers "native,axelar,rainbow_bridge"
```

## Best Practices

1. **Gas Estimation**: Always check estimated gas costs before cross-chain operations
2. **Transaction Verification**: Verify transactions on both source and destination chains
3. **Bridge Selection**: Choose bridges based on security, speed, and cost requirements
4. **Error Handling**: Implement proper error handling for cross-chain operations
5. **Chain Compatibility**: Verify asset and data format compatibility across chains
6. **Test Transactions**: Start with small test transactions before large transfers
7. **Timeout Settings**: Set appropriate timeouts for cross-chain operations

## Troubleshooting

### Common Issues

- **Transaction Pending**: Cross-chain transactions may take longer than regular transactions
- **Bridge Liquidity**: Ensure the bridge has sufficient liquidity for your transfer
- **Format Incompatibility**: Data formats may differ between chains
- **Gas Limits**: Insufficient gas can cause cross-chain transactions to fail

### Debugging Commands

```bash
# Check bridge status
wrfcoin crosschain bridge-status --provider axelar

# Verify transaction inclusion
wrfcoin crosschain verify-transaction \
  --hash "xc_tx_12345" \
  --source-chain wrfcoin \
  --target-chain ethereum

# Check cross-chain logs
wrfcoin crosschain get-logs \
  --tx-hash "xc_tx_12345" \
  --provider axelar
```

## Security Considerations

1. **Bridge Security**: Research bridge security before using it for valuable transfers
2. **Transaction Finality**: Wait for sufficient confirmations on both chains
3. **Custody Models**: Understand if the bridge uses custodial or non-custodial models
4. **Smart Contract Risks**: Be aware of smart contract risks in cross-chain operations
5. **Validator Sets**: Check the security of validator sets for cross-chain bridges

## Further Reading

- [Wrfcoin Cross-Chain White Paper](https://docs.wrfcoin.network/crosschain.pdf)
- [Axelar Integration Guide](https://docs.wrfcoin.network/integration/axelar)
- [LayerZero Integration Guide](https://docs.wrfcoin.network/integration/layerzero)
- [Interledger Protocol Integration](https://docs.wrfcoin.network/integration/ilp)
