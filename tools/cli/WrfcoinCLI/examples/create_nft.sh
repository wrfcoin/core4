#!/bin/bash

# This example script demonstrates how to create a computational NFT
# using the Wrfcoin CLI.

# Path to the Wrfcoin CLI executable
# In a real environment, you would use the full path to the executable
WRFCOIN_CLI="cargo run --"

# First, let's create a wallet address if we don't have one
echo "Checking for wallet address..."
WALLET_OUTPUT=$($WRFCOIN_CLI wallet list-addresses)

if [[ $WALLET_OUTPUT == *"No addresses in wallet"* ]]; then
  echo "No addresses found. Creating a new wallet address..."
  $WRFCOIN_CLI wallet new-address
fi

# Show available NFT categories
echo -e "\nAvailable NFT computation categories:"
$WRFCOIN_CLI nft categories

# Create a computational NFT for weather forecasting
echo -e "\nCreating a weather forecast computational NFT..."
$WRFCOIN_CLI nft create \
  --title "Temperature Forecast for New York" \
  --description "24-hour temperature forecast for New York City using historical data and machine learning" \
  --computation-type "Weather" \
  --input-data "nyc_weather_data.csv" \
  --reward 100

# List all NFTs
echo -e "\nListing all computational NFTs:"
$WRFCOIN_CLI nft list

echo -e "\nNFT creation complete!"