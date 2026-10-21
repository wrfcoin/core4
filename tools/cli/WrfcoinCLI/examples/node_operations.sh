#!/bin/bash

# This example script demonstrates various node operations
# using the Wrfcoin CLI.

# Path to the Wrfcoin CLI executable
# In a real environment, you would use the full path to the executable
WRFCOIN_CLI="cargo run --"

# Display node information
echo "Getting node information..."
$WRFCOIN_CLI node info

# Start a node with API and mining
echo -e "\nStarting Wrfcoin node with API server and mining..."
# Note: In a real script, we would need to handle this as a background process
# For demonstration purposes, we're just showing the command
echo "$WRFCOIN_CLI start --api --mine"

# Instead of actually running the node, let's explore blockchain data
echo -e "\nGetting blockchain information..."
$WRFCOIN_CLI chain info

# List the latest blocks
echo -e "\nListing the latest 5 blocks..."
$WRFCOIN_CLI chain list-blocks --limit 5

# Show pending transactions
echo -e "\nShowing pending transactions..."
$WRFCOIN_CLI chain pending-transactions

echo -e "\nNode operations demo complete!"