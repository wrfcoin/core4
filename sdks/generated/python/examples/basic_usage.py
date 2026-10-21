#!/usr/bin/env python3
"""
WRFCoin Python SDK - Basic Usage Example

This example demonstrates how to use the WRFCoin Python SDK
to interact with the WRFCoin blockchain platform.
"""

import os
import asyncio
from datetime import datetime
from typing import Dict, Any

import wrfcoin_sdk
from wrfcoin_sdk.rest import ApiException
from wrfcoin_sdk.api import (
    blockchain_api,
    weather_data_api,
    wallet_api,
    nft_api
)
from wrfcoin_sdk.models import WeatherDataSubmission


class WRFCoinExample:
    def __init__(self, api_key: str = None, base_url: str = "http://localhost:3001"):
        """Initialize the WRFCoin SDK client."""
        # Configure API client
        configuration = wrfcoin_sdk.Configuration(
            host=base_url,
            api_key={'ApiKeyAuth': api_key or 'your-api-key-here'}
        )
        
        # Create API client
        self.api_client = wrfcoin_sdk.ApiClient(configuration)
        
        # Initialize API instances
        self.blockchain_api = blockchain_api.BlockchainApi(self.api_client)
        self.weather_api = weather_data_api.WeatherDataApi(self.api_client)
        self.wallet_api = wallet_api.WalletApi(self.api_client)
        self.nft_api = nft_api.NFTApi(self.api_client)

    async def run_examples(self):
        """Run all example operations."""
        print("🌍 WRFCoin SDK Example - Python")
        print("=====================================\n")

        try:
            # 1. Get blockchain status
            await self.get_blockchain_status()
            
            # 2. Get recent blocks
            await self.get_recent_blocks()
            
            # 3. Generate wallet mnemonic
            await self.generate_wallet_mnemonic()
            
            # 4. Submit weather data
            await self.submit_weather_data()
            
            # 5. Get NFT collections
            await self.get_nft_collections()
            
            print("✅ All examples completed successfully!")
            
        except ApiException as e:
            print(f"❌ API Error occurred: {e}")
        except Exception as e:
            print(f"❌ Unexpected error: {e}")
        finally:
            self.api_client.close()

    async def get_blockchain_status(self):
        """Get current blockchain status."""
        print("1. Fetching blockchain status...")
        try:
            response = self.blockchain_api.api_blockchain_status_get()
            status = response
            print(f"   Blockchain Status: {status}")
            print(f"   Block Height: {getattr(status, 'block_height', 'N/A')}")
            print(f"   Network Health: {getattr(status, 'network_health', 'N/A')}\n")
        except ApiException as e:
            print(f"   Error getting blockchain status: {e}")

    async def get_recent_blocks(self):
        """Get recent blockchain blocks."""
        print("2. Fetching recent blocks...")
        try:
            response = self.blockchain_api.api_blockchain_blocks_get(limit=5)
            blocks = response.get('blocks', [])
            print(f"   Retrieved {len(blocks)} blocks")
            for i, block in enumerate(blocks[:5]):
                block_height = block.get('height', 'N/A')
                block_hash = block.get('hash', 'N/A')[:10] + '...' if block.get('hash') else 'N/A'
                print(f"   Block {i + 1}: Height {block_height}, Hash: {block_hash}")
            print()
        except ApiException as e:
            print(f"   Error getting blocks: {e}")

    async def generate_wallet_mnemonic(self):
        """Generate a new wallet mnemonic."""
        print("3. Generating new wallet mnemonic...")
        try:
            response = self.wallet_api.api_wallet_generate_mnemonic_post()
            mnemonic = response.get('mnemonic', '')
            # Only show first 3 words for security
            mnemonic_preview = ' '.join(mnemonic.split()[:3]) + '...' if mnemonic else 'N/A'
            print(f"   Generated mnemonic (first 3 words): {mnemonic_preview}")
            print()
        except ApiException as e:
            print(f"   Error generating mnemonic: {e}")

    async def submit_weather_data(self):
        """Submit weather data to the platform."""
        print("4. Submitting weather data...")
        try:
            # Create weather data submission
            weather_data = WeatherDataSubmission(
                station_id="example-station-001",
                timestamp=datetime.now().isoformat(),
                temperature=23.5,
                humidity=65,
                pressure=1013.25,
                location={
                    "latitude": 40.7128,
                    "longitude": -74.0060
                },
                sensor_data=[
                    {
                        "sensor_type": "temperature",
                        "value": 23.5,
                        "unit": "celsius",
                        "accuracy": 0.1
                    },
                    {
                        "sensor_type": "humidity", 
                        "value": 65,
                        "unit": "percent",
                        "accuracy": 1.0
                    }
                ],
                quality={
                    "score": 0.95,
                    "verified": True
                }
            )
            
            response = self.weather_api.api_weather_data_submit_post(weather_data)
            data = response.get('data', {})
            rewards = data.get('rewards', {})
            
            print("   Weather data submitted successfully")
            print(f"   Submission ID: {data.get('submissionId', 'N/A')}")
            print(f"   Reward: {rewards.get('amount', 'N/A')} {rewards.get('currency', 'N/A')}\n")
            
        except ApiException as e:
            print(f"   Error submitting weather data: {e}")

    async def get_nft_collections(self):
        """Get NFT collections."""
        print("5. Fetching NFT collections...")
        try:
            response = self.nft_api.api_nft_collections_get(limit=10)
            collections = response.get('collections', [])
            print(f"   Found {len(collections)} NFT collections")
            for i, collection in enumerate(collections[:10]):
                name = collection.get('name', 'Unknown')
                token_count = collection.get('tokenCount', 0)
                print(f"   Collection {i + 1}: {name} ({token_count} tokens)")
            print()
        except ApiException as e:
            print(f"   Error getting NFT collections: {e}")

    def demonstrate_error_handling(self):
        """Demonstrate proper error handling."""
        print("6. Demonstrating error handling...")
        try:
            # Intentionally make a bad request
            self.blockchain_api.api_blockchain_blocks_get(limit=-1)  # Invalid parameter
        except ApiException as e:
            print(f"   Caught API Exception: Status {e.status}")
            print(f"   Error message: {e.reason}")
        except Exception as e:
            print(f"   Caught unexpected error: {e}")


def main():
    """Main function to run the examples."""
    # You can set your API key as an environment variable
    api_key = os.getenv('WRFCOIN_API_KEY', 'your-api-key-here')
    base_url = os.getenv('WRFCOIN_API_URL', 'http://localhost:3001')
    
    # Create and run examples
    example = WRFCoinExample(api_key=api_key, base_url=base_url)
    
    # Run async examples
    asyncio.run(example.run_examples())
    
    # Demonstrate error handling
    example.demonstrate_error_handling()


if __name__ == "__main__":
    main()