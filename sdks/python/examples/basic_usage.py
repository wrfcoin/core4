#!/usr/bin/env python3
"""
Basic usage example for Wrfcoin Python SDK

This example demonstrates core functionality including:
- SDK initialization and configuration
- Network information retrieval
- Account balance checking
- Basic transaction operations
- Error handling
"""

import asyncio
import os
from wrfcoin import WrfcoinSDK, WrfcoinError


async def main():
    """Main example function"""
    print("🚀 Wrfcoin Python SDK - Basic Usage Example")
    print("=" * 50)
    
    # Initialize SDK for testnet with debug mode
    sdk = WrfcoinSDK.testnet(
        debug=True,
        timeout=30
    )
    
    try:
        # Test connection
        print("\n📡 Testing connection...")
        is_ready = await sdk.is_ready()
        if is_ready:
            print("✅ Successfully connected to Wrfcoin testnet")
        else:
            print("❌ Failed to connect to network")
            return
        
        # Get network information
        print("\n🌐 Network Information:")
        network_info = await sdk.blockchain.get_network_info()
        print(f"   Chain ID: {network_info.get('chainId', 'N/A')}")
        print(f"   Block Number: {network_info.get('blockNumber', 'N/A')}")
        print(f"   Gas Price: {network_info.get('gasPrice', 'N/A')}")
        print(f"   Total Supply: {network_info.get('totalSupply', 'N/A')} WRF")
        
        # Example wallet address (replace with actual address for real usage)
        example_address = "0x742d35Cc6634C0532925a3b8D3B9E0C0Da8E4Baf"
        
        # Get account balance
        print(f"\n💰 Account Balance for {example_address}:")
        try:
            balance = await sdk.wallet.get_balance(example_address)
            balance_wrf = sdk.wallet.format_token_amount(balance.get('balance', '0'), 18)
            print(f"   WRF Balance: {balance_wrf:.4f} WRF")
            print(f"   Transaction Count: {balance.get('transactionCount', 0)}")
            
            # Get portfolio information
            portfolio = await sdk.wallet.get_portfolio(example_address)
            print(f"   Portfolio Value: ${portfolio.get('totalValueUSD', 0):.2f}")
            print(f"   Token Count: {len(portfolio.get('tokens', []))}")
            print(f"   NFT Count: {len(portfolio.get('nfts', []))}")
            
        except WrfcoinError as e:
            print(f"   ⚠️  Could not fetch balance: {e.message}")
        
        # Get latest block information
        print("\n📦 Latest Block:")
        try:
            latest_block = await sdk.blockchain.get_block()
            print(f"   Block Number: {latest_block.get('number', 'N/A')}")
            print(f"   Block Hash: {latest_block.get('hash', 'N/A')[:20]}...")
            print(f"   Timestamp: {latest_block.get('timestamp', 'N/A')}")
            print(f"   Transaction Count: {latest_block.get('transactionCount', 0)}")
            print(f"   Gas Used: {latest_block.get('gasUsed', 'N/A')}")
        except WrfcoinError as e:
            print(f"   ⚠️  Could not fetch block: {e.message}")
        
        # Get gas price information
        print("\n⛽ Gas Price Information:")
        try:
            gas_price = await sdk.blockchain.get_gas_price()
            print(f"   Slow: {gas_price.get('slow', 'N/A')} gwei")
            print(f"   Standard: {gas_price.get('standard', 'N/A')} gwei")
            print(f"   Fast: {gas_price.get('fast', 'N/A')} gwei")
            print(f"   Instant: {gas_price.get('instant', 'N/A')} gwei")
        except WrfcoinError as e:
            print(f"   ⚠️  Could not fetch gas prices: {e.message}")
        
        # Demonstrate transaction estimation
        print("\n📊 Transaction Estimation:")
        try:
            example_tx = {
                "from": example_address,
                "to": "0x8ba1f109551bD432803012645Hac136c45cb2274",
                "value": "1000000000000000000"  # 1 WRF in wei
            }
            
            gas_estimate = await sdk.blockchain.estimate_gas(example_tx)
            print(f"   Estimated Gas: {gas_estimate:,}")
            
            fee_estimate = await sdk.wallet.estimate_transaction_fee(example_tx)
            print(f"   Gas Fee: {fee_estimate.get('gasFee', 'N/A')}")
            print(f"   Total Fee: {fee_estimate.get('totalFee', 'N/A')}")
            
        except WrfcoinError as e:
            print(f"   ⚠️  Could not estimate transaction: {e.message}")
        
        # Check SDK status
        print("\n📊 SDK Status:")
        status = await sdk.get_status()
        print(f"   Connected: {status.connected}")
        print(f"   Authenticated: {status.authenticated}")
        print(f"   Network: {status.network}")
        print(f"   Block Height: {status.block_height or 'N/A'}")
        
        # Demonstrate error handling with invalid address
        print("\n🚨 Error Handling Demo:")
        try:
            await sdk.wallet.get_balance("invalid_address")
        except WrfcoinError as e:
            print(f"   ✅ Caught expected error: {type(e).__name__}: {e.message}")
        
        print("\n✨ Example completed successfully!")
        
    except Exception as e:
        print(f"\n❌ Unexpected error: {e}")
    
    finally:
        # Always disconnect when done
        await sdk.disconnect()
        print("\n👋 Disconnected from network")


if __name__ == "__main__":
    # Run the example
    asyncio.run(main())