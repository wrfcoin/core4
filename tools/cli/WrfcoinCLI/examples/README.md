# Wrfcoin CLI Examples

This directory contains example scripts that demonstrate how to use the Wrfcoin CLI for various
operations.

## Available Examples

1. **create_wallet.sh** - Demonstrates how to create a wallet, generate addresses, and check
   balances.
2. **create_nft.sh** - Shows how to create a computational NFT for weather forecasting.
3. **node_operations.sh** - Illustrates node management operations like starting a node, checking
   status, and viewing blockchain data.

## Running the Examples

Make sure you have the Wrfcoin CLI built and that the dependencies are installed:

```bash
cd ../
cargo build
```

Then, to run an example:

```bash
# Make the script executable
chmod +x examples/create_wallet.sh

# Run the script
./examples/create_wallet.sh
```

## Usage Notes

- These examples are designed to work with the CLI in development mode.
- In a production environment, you would use the full path to the executable.
- Some commands like `start` won't fully execute in these examples since they would block the
  script. In real usage, you would run them in the background or in a separate terminal.

## Command Reference

Here's a summary of the main CLI commands:

### Wallet Commands

- `wallet new-address` - Generate a new wallet address
- `wallet list-addresses` - List all addresses in the wallet
- `wallet balance` - Show wallet balance
- `wallet set-primary ADDRESS` - Set primary address
- `wallet send --to ADDRESS --amount AMOUNT` - Send tokens
- `wallet history` - Show transaction history

### Blockchain Commands

- `chain info` - Show blockchain information
- `chain block --hash HASH` - Show a specific block by hash
- `chain block --height HEIGHT` - Show a specific block by height
- `chain list-blocks --limit N` - List the latest N blocks
- `chain pending-transactions` - Show pending transactions
- `chain export --output FILE` - Export blockchain data
- `chain import --input FILE` - Import blockchain data

### NFT Commands

- `nft list` - List all computational NFTs
- `nft show ID` - Show details for a specific NFT
- `nft create` - Create a new computational NFT
- `nft submit` - Submit a computation result
- `nft categories` - List available computation categories
- `nft add-category NAME` - Add a new computation category

### Node Commands

- `node info` - Show node information
- `node list-peers` - List known peers
- `node list-connected-peers` - List connected peers
