"""
Wrfcoin Python SDK - NFT Service

NFT operations, minting, trading, and metadata management.
"""

from typing import Dict, Any, List, Optional
from ..exceptions import (
    ValidationError, 
    InvalidAddressError,
    NFTError,
    TransactionError
)


class NFTService:
    """Service for NFT operations and computational NFT management"""
    
    def __init__(self, graphql_client):
        self.graphql = graphql_client
    
    async def get_nft(self, token_id: str, collection_address: Optional[str] = None) -> Dict[str, Any]:
        """Get NFT details by token ID"""
        if not token_id:
            raise ValidationError("Token ID is required")
        
        query = """
        query GetNFT($tokenId: String!, $collectionAddress: String) {
            nft(tokenId: $tokenId, collectionAddress: $collectionAddress) {
                tokenId
                collection
                owner
                name
                description
                imageUrl
                attributes {
                    traitType
                    value
                    displayType
                }
                metadata
                createdAt
                lastTransfer
                computational {
                    isComputational
                    computationResults {
                        modelId
                        inputData
                        outputData
                        executionTime
                        gasUsed
                        status
                    }
                    performance {
                        accuracy
                        inferenceTime
                        powerConsumption
                    }
                }
                carbonCredits {
                    creditTokens
                    offsetAmount
                    certificationDetails
                }
                trading {
                    isForSale
                    price
                    currency
                    marketplace
                    bids {
                        bidder
                        amount
                        timestamp
                    }
                }
            }
        }
        """
        
        variables = {"tokenId": token_id}
        if collection_address:
            if not self._is_valid_address(collection_address):
                raise InvalidAddressError(f"Invalid collection address: {collection_address}")
            variables["collectionAddress"] = collection_address
        
        result = await self.graphql.query(query, variables)
        nft_data = result.get('nft')
        
        if not nft_data:
            raise NFTError(f"NFT not found: {token_id}", token_id=token_id)
        
        return nft_data
    
    async def get_collection(self, collection_address: str) -> Dict[str, Any]:
        """Get NFT collection information"""
        if not self._is_valid_address(collection_address):
            raise InvalidAddressError(f"Invalid collection address: {collection_address}")
        
        query = """
        query GetCollection($address: String!) {
            nftCollection(address: $address) {
                address
                name
                symbol
                description
                totalSupply
                owner
                royalty
                verified
                floorPrice
                totalVolume
                stats {
                    holders
                    sales24h
                    volume24h
                    averagePrice
                }
                metadata {
                    bannerImageUrl
                    featuredImageUrl
                    description
                    externalLink
                    discordUrl
                    twitterUrl
                }
            }
        }
        """
        
        result = await self.graphql.query(query, {"address": collection_address})
        collection = result.get('nftCollection')
        
        if not collection:
            raise NFTError(f"Collection not found: {collection_address}")
        
        return collection
    
    async def get_user_nfts(
        self, 
        owner_address: str, 
        collection_address: Optional[str] = None,
        limit: int = 20,
        offset: int = 0
    ) -> List[Dict[str, Any]]:
        """Get NFTs owned by a user"""
        if not self._is_valid_address(owner_address):
            raise InvalidAddressError(f"Invalid owner address: {owner_address}")
        
        if collection_address and not self._is_valid_address(collection_address):
            raise InvalidAddressError(f"Invalid collection address: {collection_address}")
        
        if limit <= 0 or limit > 100:
            raise ValidationError("Limit must be between 1 and 100")
        
        query = """
        query GetUserNFTs($ownerAddress: String!, $collectionAddress: String, $limit: Int!, $offset: Int!) {
            userNFTs(ownerAddress: $ownerAddress, collectionAddress: $collectionAddress, limit: $limit, offset: $offset) {
                nfts {
                    tokenId
                    collection
                    name
                    imageUrl
                    lastPrice
                    estimatedValue
                    computational {
                        isComputational
                        performance {
                            accuracy
                            inferenceTime
                        }
                    }
                }
                totalCount
                hasMore
            }
        }
        """
        
        variables = {
            "ownerAddress": owner_address,
            "limit": limit,
            "offset": offset
        }
        
        if collection_address:
            variables["collectionAddress"] = collection_address
        
        result = await self.graphql.query(query, variables)
        user_nfts = result.get('userNFTs', {})
        return user_nfts.get('nfts', [])
    
    async def mint_computational_nft(
        self, 
        to_address: str,
        model_id: str,
        input_data: Dict[str, Any],
        metadata: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Mint a computational NFT with AI model results"""
        if not self._is_valid_address(to_address):
            raise InvalidAddressError(f"Invalid recipient address: {to_address}")
        
        if not model_id:
            raise ValidationError("Model ID is required")
        
        if not input_data:
            raise ValidationError("Input data is required")
        
        mutation = """
        mutation MintComputationalNFT(
            $toAddress: String!,
            $modelId: String!,
            $inputData: JSON!,
            $metadata: NFTMetadataInput!
        ) {
            mintComputationalNFT(
                toAddress: $toAddress,
                modelId: $modelId,
                inputData: $inputData,
                metadata: $metadata
            ) {
                tokenId
                transactionHash
                computational {
                    modelId
                    inputData
                    outputData
                    executionTime
                    gasUsed
                    performance {
                        accuracy
                        inferenceTime
                        powerConsumption
                    }
                }
                carbonCredits {
                    creditTokens
                    offsetAmount
                }
            }
        }
        """
        
        variables = {
            "toAddress": to_address,
            "modelId": model_id,
            "inputData": input_data,
            "metadata": metadata
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('mintComputationalNFT', {})
        except Exception as error:
            raise NFTError(f"Failed to mint computational NFT: {error}")
    
    async def mint_carbon_credit_nft(
        self,
        to_address: str,
        credit_amount: str,
        project_details: Dict[str, Any],
        verification_data: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Mint a carbon credit NFT"""
        if not self._is_valid_address(to_address):
            raise InvalidAddressError(f"Invalid recipient address: {to_address}")
        
        try:
            amount = float(credit_amount)
            if amount <= 0:
                raise ValidationError("Credit amount must be positive")
        except (ValueError, TypeError):
            raise ValidationError(f"Invalid credit amount: {credit_amount}")
        
        mutation = """
        mutation MintCarbonCreditNFT(
            $toAddress: String!,
            $creditAmount: String!,
            $projectDetails: CarbonProjectInput!,
            $verificationData: VerificationDataInput!
        ) {
            mintCarbonCreditNFT(
                toAddress: $toAddress,
                creditAmount: $creditAmount,
                projectDetails: $projectDetails,
                verificationData: $verificationData
            ) {
                tokenId
                transactionHash
                carbonCredits {
                    creditTokens
                    offsetAmount
                    projectId
                    certificationDetails {
                        standard
                        verifier
                        issuanceDate
                        expiryDate
                    }
                    vintageYear
                    geography
                    methodology
                }
            }
        }
        """
        
        variables = {
            "toAddress": to_address,
            "creditAmount": credit_amount,
            "projectDetails": project_details,
            "verificationData": verification_data
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('mintCarbonCreditNFT', {})
        except Exception as error:
            raise NFTError(f"Failed to mint carbon credit NFT: {error}")
    
    async def transfer_nft(
        self,
        from_address: str,
        to_address: str,
        token_id: str,
        collection_address: Optional[str] = None
    ) -> Dict[str, Any]:
        """Transfer an NFT"""
        if not self._is_valid_address(from_address):
            raise InvalidAddressError(f"Invalid from address: {from_address}")
        
        if not self._is_valid_address(to_address):
            raise InvalidAddressError(f"Invalid to address: {to_address}")
        
        if not token_id:
            raise ValidationError("Token ID is required")
        
        mutation = """
        mutation TransferNFT(
            $fromAddress: String!,
            $toAddress: String!,
            $tokenId: String!,
            $collectionAddress: String
        ) {
            transferNFT(
                fromAddress: $fromAddress,
                toAddress: $toAddress,
                tokenId: $tokenId,
                collectionAddress: $collectionAddress
            ) {
                transactionHash
                status
                gasUsed
                from
                to
                tokenId
            }
        }
        """
        
        variables = {
            "fromAddress": from_address,
            "toAddress": to_address,
            "tokenId": token_id
        }
        
        if collection_address:
            if not self._is_valid_address(collection_address):
                raise InvalidAddressError(f"Invalid collection address: {collection_address}")
            variables["collectionAddress"] = collection_address
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('transferNFT', {})
        except Exception as error:
            raise TransactionError(f"Failed to transfer NFT: {error}")
    
    async def list_nft_for_sale(
        self,
        owner_address: str,
        token_id: str,
        price: str,
        currency: str = "WRF",
        marketplace: str = "wrfcoin",
        collection_address: Optional[str] = None
    ) -> Dict[str, Any]:
        """List an NFT for sale"""
        if not self._is_valid_address(owner_address):
            raise InvalidAddressError(f"Invalid owner address: {owner_address}")
        
        if not token_id:
            raise ValidationError("Token ID is required")
        
        try:
            price_float = float(price)
            if price_float <= 0:
                raise ValidationError("Price must be positive")
        except (ValueError, TypeError):
            raise ValidationError(f"Invalid price: {price}")
        
        mutation = """
        mutation ListNFTForSale(
            $ownerAddress: String!,
            $tokenId: String!,
            $price: String!,
            $currency: String!,
            $marketplace: String!,
            $collectionAddress: String
        ) {
            listNFTForSale(
                ownerAddress: $ownerAddress,
                tokenId: $tokenId,
                price: $price,
                currency: $currency,
                marketplace: $marketplace,
                collectionAddress: $collectionAddress
            ) {
                listingId
                transactionHash
                status
                price
                currency
                marketplace
                expiryDate
            }
        }
        """
        
        variables = {
            "ownerAddress": owner_address,
            "tokenId": token_id,
            "price": price,
            "currency": currency,
            "marketplace": marketplace
        }
        
        if collection_address:
            if not self._is_valid_address(collection_address):
                raise InvalidAddressError(f"Invalid collection address: {collection_address}")
            variables["collectionAddress"] = collection_address
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('listNFTForSale', {})
        except Exception as error:
            raise NFTError(f"Failed to list NFT for sale: {error}")
    
    async def buy_nft(
        self,
        buyer_address: str,
        listing_id: str,
        payment_token: Optional[str] = None
    ) -> Dict[str, Any]:
        """Buy an NFT from marketplace"""
        if not self._is_valid_address(buyer_address):
            raise InvalidAddressError(f"Invalid buyer address: {buyer_address}")
        
        if not listing_id:
            raise ValidationError("Listing ID is required")
        
        mutation = """
        mutation BuyNFT($buyerAddress: String!, $listingId: String!, $paymentToken: String) {
            buyNFT(buyerAddress: $buyerAddress, listingId: $listingId, paymentToken: $paymentToken) {
                transactionHash
                status
                price
                currency
                tokenId
                seller
                buyer
                marketplace
                gasUsed
            }
        }
        """
        
        variables = {
            "buyerAddress": buyer_address,
            "listingId": listing_id
        }
        
        if payment_token:
            if not self._is_valid_address(payment_token):
                raise InvalidAddressError(f"Invalid payment token address: {payment_token}")
            variables["paymentToken"] = payment_token
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('buyNFT', {})
        except Exception as error:
            raise NFTError(f"Failed to buy NFT: {error}")
    
    async def get_marketplace_listings(
        self,
        collection_address: Optional[str] = None,
        price_min: Optional[str] = None,
        price_max: Optional[str] = None,
        sort_by: str = "price_asc",
        limit: int = 20,
        offset: int = 0
    ) -> List[Dict[str, Any]]:
        """Get marketplace listings with filtering"""
        if collection_address and not self._is_valid_address(collection_address):
            raise InvalidAddressError(f"Invalid collection address: {collection_address}")
        
        if limit <= 0 or limit > 100:
            raise ValidationError("Limit must be between 1 and 100")
        
        query = """
        query GetMarketplaceListings(
            $collectionAddress: String,
            $priceMin: String,
            $priceMax: String,
            $sortBy: String!,
            $limit: Int!,
            $offset: Int!
        ) {
            marketplaceListings(
                collectionAddress: $collectionAddress,
                priceMin: $priceMin,
                priceMax: $priceMax,
                sortBy: $sortBy,
                limit: $limit,
                offset: $offset
            ) {
                listings {
                    listingId
                    tokenId
                    collection
                    name
                    imageUrl
                    price
                    currency
                    seller
                    marketplace
                    listedAt
                    expiryDate
                    computational {
                        isComputational
                        performance {
                            accuracy
                            inferenceTime
                        }
                    }
                }
                totalCount
                hasMore
            }
        }
        """
        
        variables = {
            "sortBy": sort_by,
            "limit": limit,
            "offset": offset
        }
        
        if collection_address:
            variables["collectionAddress"] = collection_address
        if price_min:
            variables["priceMin"] = price_min
        if price_max:
            variables["priceMax"] = price_max
        
        result = await self.graphql.query(query, variables)
        marketplace = result.get('marketplaceListings', {})
        return marketplace.get('listings', [])
    
    async def subscribe_to_nft_transfers(self, collection_address: Optional[str] = None, callback = None) -> callable:
        """Subscribe to NFT transfer events"""
        if collection_address and not self._is_valid_address(collection_address):
            raise InvalidAddressError(f"Invalid collection address: {collection_address}")
        
        subscription = """
        subscription NFTTransfers($collectionAddress: String) {
            nftTransfers(collectionAddress: $collectionAddress) {
                tokenId
                collection
                from
                to
                transactionHash
                timestamp
                price
                marketplace
            }
        }
        """
        
        variables = {}
        if collection_address:
            variables["collectionAddress"] = collection_address
        
        return await self.graphql.subscribe(
            subscription,
            variables=variables,
            on_data=callback
        )
    
    def _is_valid_address(self, address: str) -> bool:
        """Validate Ethereum-style address format"""
        if not isinstance(address, str):
            return False
        
        # Remove 0x prefix if present
        addr = address.lower()
        if addr.startswith('0x'):
            addr = addr[2:]
        
        # Check if it's 40 hex characters
        if len(addr) != 40:
            return False
        
        try:
            int(addr, 16)
            return True
        except ValueError:
            return False