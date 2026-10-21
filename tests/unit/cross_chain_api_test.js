/**
 * Cross-Chain Transfer API Test Script
 *
 * This script tests the cross-chain transfer API endpoints:
 * - GET /api/cross-chain/transfers - Get all transfers
 * - GET /api/cross-chain/transfers/:id - Get transfer by ID
 * - GET /api/cross-chain/transfers/sender/:address - Get transfers by sender address
 * - GET /api/cross-chain/transfers/recipient/:address - Get transfers by recipient address
 * - GET /api/cross-chain/transfers/address/:address - Get transfers by address (sender or recipient)
 * - GET /api/cross-chain/transfers/chains/:fromChain/:toChain - Get transfers by chain pair
 * - POST /api/cross-chain/transfers - Create a new cross-chain transfer
 * - PUT /api/cross-chain/transfers/:id - Update a cross-chain transfer
 */

const API_BASE_URL = 'http://localhost:3000/api/cross-chain';

// Test data for creating a new cross-chain transfer
const newTransfer = {
  fromChain: 'Wrfcoin',
  toChain: 'Ethereum',
  senderAddress: 'Wrf1TestSenderAddress000000000000000001',
  recipientAddress: '0xTestEthereumRecipientAddress00001',
  amount: 1000000000, // 10 WRF
  fee: 5000000, // 0.05 WRF
  sourceTxHash: 'WrfcoinTxHash00000000000000000000000001',
  status: 'PENDING',
  tokenType: 'NATIVE',
};

// Test data for updating a cross-chain transfer
const updateData = {
  status: 'COMPLETED',
  destinationTxHash: '0xEthereumTxHash0000000000000000000001',
  completedAt: new Date().toISOString(),
};

// Test function to get all cross-chain transfers
async function getAllTransfers() {
  try {
    const response = await fetch(`${API_BASE_URL}/transfers`);
    const data = await response.json();
    console.log('GET All Transfers Response:', response.status);
    console.log('Transfers:', data);
    return data;
  } catch (error) {
    console.error('Error getting all transfers:', error.message);
  }
}

// Test function to create a new cross-chain transfer
async function createTransfer() {
  try {
    const response = await fetch(`${API_BASE_URL}/transfers`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(newTransfer),
    });
    const data = await response.json();
    console.log('Create Transfer Response:', response.status);
    console.log('Created Transfer:', data);
    return data;
  } catch (error) {
    console.error('Error creating transfer:', error.message);
  }
}

// Test function to get a transfer by ID
async function getTransferById(id) {
  try {
    const response = await fetch(`${API_BASE_URL}/transfers/${id}`);
    const data = await response.json();
    console.log(`GET Transfer By ID ${id} Response:`, response.status);
    console.log('Transfer:', data);
    return data;
  } catch (error) {
    console.error(`Error getting transfer by ID ${id}:`, error.message);
  }
}

// Test function to update a transfer
async function updateTransfer(id) {
  try {
    const response = await fetch(`${API_BASE_URL}/transfers/${id}`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(updateData),
    });
    const data = await response.json();
    console.log(`UPDATE Transfer ${id} Response:`, response.status);
    console.log('Updated Transfer:', data);
    return data;
  } catch (error) {
    console.error(`Error updating transfer ${id}:`, error.message);
  }
}

// Test function to get transfers by sender address
async function getTransfersBySender(address) {
  try {
    const response = await fetch(`${API_BASE_URL}/transfers/sender/${address}`);
    const data = await response.json();
    console.log(
      `GET Transfers By Sender ${address} Response:`,
      response.status
    );
    console.log('Transfers:', data);
    return data;
  } catch (error) {
    console.error(
      `Error getting transfers by sender ${address}:`,
      error.message
    );
  }
}

// Test function to get transfers by chain pair
async function getTransfersByChainPair(fromChain, toChain) {
  try {
    const response = await fetch(
      `${API_BASE_URL}/transfers/chains/${fromChain}/${toChain}`
    );
    const data = await response.json();
    console.log(
      `GET Transfers By Chain Pair ${fromChain}->${toChain} Response:`,
      response.status
    );
    console.log('Transfers:', data);
    return data;
  } catch (error) {
    console.error(
      `Error getting transfers by chain pair ${fromChain}->${toChain}:`,
      error.message
    );
  }
}

// Main test function
async function runTests() {
  console.log('=== Starting Cross-Chain API Tests ===');

  // First, get all transfers (should be empty or contain existing data)
  const initialTransfers = await getAllTransfers();

  // Create a new transfer
  const createdTransfer = await createTransfer();
  if (!createdTransfer) {
    console.error('Failed to create transfer, aborting tests');
    return;
  }

  // Get the transfer by ID
  await getTransferById(createdTransfer.id);

  // Get transfers by sender address
  await getTransfersBySender(newTransfer.senderAddress);

  // Get transfers by chain pair
  await getTransfersByChainPair(newTransfer.fromChain, newTransfer.toChain);

  // Update the transfer
  await updateTransfer(createdTransfer.id);

  // Get the updated transfer
  await getTransferById(createdTransfer.id);

  // Get all transfers again to see the updated list
  await getAllTransfers();

  console.log('=== Cross-Chain API Tests Completed ===');
}

// Run the tests
runTests().catch(console.error);
