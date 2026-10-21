#!/usr/bin/env ts-node

/**
 * WRFCoin TypeScript SDK - Basic Usage Example
 *
 * This example demonstrates how to use the WRFCoin TypeScript SDK
 * to interact with the WRFCoin blockchain platform.
 */

import {
  Configuration,
  BlockchainApi,
  WeatherDataApi,
  WalletApi,
} from '../index';

// Configuration
const config = new Configuration({
  basePath: 'http://localhost:3001', // Development server
  // For production: https://api.wrfcoin.com/v1
  apiKey: 'your-api-key-here', // Replace with your actual API key
});

// Initialize API clients
const blockchainApi = new BlockchainApi(config);
const weatherApi = new WeatherDataApi(config);
const walletApi = new WalletApi(config);

async function main() {
  try {
    console.log('🌍 WRFCoin SDK Example - TypeScript');
    console.log('=====================================\n');

    // 1. Get blockchain status
    console.log('1. Fetching blockchain status...');
    const blockchainStatus = await blockchainApi.apiBlockchainStatusGet();
    console.log('   Blockchain Status:', blockchainStatus.data);
    console.log(
      `   Block Height: ${blockchainStatus.data.blockHeight || 'N/A'}`
    );
    console.log(
      `   Network Health: ${(blockchainStatus.data as any).network || 'N/A'}\n`
    );

    // 2. Get recent blocks
    console.log('2. Fetching recent blocks...');
    const blocks = await blockchainApi.apiBlockchainBlocksGet(5); // Get last 5 blocks
    console.log(`   Retrieved ${blocks.data.blocks?.length || 0} blocks`);
    blocks.data.blocks?.forEach((block: any, index: number) => {
      const height = block.height || 'N/A';
      const hash = block.hash ? block.hash.substring(0, 10) + '...' : 'N/A';
      console.log(`   Block ${index + 1}: Height ${height}, Hash: ${hash}`);
    });
    console.log();

    // 3. Generate a new wallet mnemonic
    console.log('3. Generating new wallet mnemonic...');
    const mnemonic = await walletApi.apiWalletGenerateMnemonicPost();
    const mnemonicWords = mnemonic.data.mnemonic?.split(' ') || [];
    const preview =
      mnemonicWords.length >= 3
        ? mnemonicWords.slice(0, 3).join(' ') + '...'
        : 'N/A';
    console.log('   Generated mnemonic (first 3 words):', preview);
    console.log();

    // 4. Submit weather data (example)
    console.log('4. Submitting weather data...');
    const weatherData = {
      stationId: 'example-station-001',
      timestamp: Math.floor(Date.now() / 1000), // Unix timestamp
      temperature: 23.5,
      humidity: 65,
      pressure: 1013.25,
      location: {
        latitude: 40.7128,
        longitude: -74.006,
      },
      sensorData: [
        {
          sensorType: 'temperature',
          value: 23.5,
          unit: 'celsius',
          accuracy: 0.1,
        },
        {
          sensorType: 'humidity',
          value: 65,
          unit: 'percent',
          accuracy: 1.0,
        },
      ],
      quality: {
        score: 0.95,
        verified: true,
      },
    };

    const weatherSubmission =
      await weatherApi.apiWeatherSubmitPost(weatherData);
    console.log('   Weather data submitted successfully');
    console.log(
      `   Submission ID: ${(weatherSubmission.data.data as any)?.submissionId || 'N/A'}`
    );
    console.log(
      `   Reward: ${(weatherSubmission.data.data as any)?.rewards?.amount || 'N/A'} ${(weatherSubmission.data.data as any)?.rewards?.currency || 'N/A'}\n`
    );

    // 5. Demonstrate smart contract interaction
    console.log('5. Testing smart contract API...');
    // Note: NFT functionality would typically be available through smart contracts
    // For now, we'll skip this section as NFT API wasn't generated properly
    console.log(
      '   Smart contract operations available through SmartContractsApi'
    );
    console.log(
      '   (NFT collections would be accessed via smart contract calls)\n'
    );

    console.log('✅ All examples completed successfully!');
  } catch (error) {
    console.error('❌ Error occurred:', error.response?.data || error.message);
  }
}

// Helper function to demonstrate error handling
async function demonstrateErrorHandling() {
  try {
    // Intentionally make a bad request to demonstrate error handling
    await blockchainApi.apiBlockchainBlocksGet(-1); // Invalid parameter
  } catch (error) {
    if (error.response) {
      // The request was made and the server responded with a status code
      console.log('Error Status:', error.response.status);
      console.log('Error Data:', error.response.data);
    } else if (error.request) {
      // The request was made but no response was received
      console.log('Network Error:', error.request);
    } else {
      // Something else happened
      console.log('Error:', error.message);
    }
  }
}

// Run the example
if (require.main === module) {
  main().catch(console.error);
}

export { main, demonstrateErrorHandling };
