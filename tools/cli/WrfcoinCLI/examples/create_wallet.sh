#!/bin/bash

# This example script demonstrates how to create a wallet and generate addresses
# using the Wrfcoin CLI.

# Path to the Wrfcoin CLI executable
# In a real environment, you would use the full path to the executable
WRFCOIN_CLI="cargo run --"

# Generate a new wallet address
echo "Generating a new wallet address..."
$WRFCOIN_CLI wallet new-address

# List all addresses in the wallet
echo -e "\nListing all wallet addresses..."
$WRFCOIN_CLI wallet list-addresses

# Get the balance of the wallet
echo -e "\nChecking wallet balance..."
$WRFCOIN_CLI wallet balance

echo -e "\nWallet setup complete!"