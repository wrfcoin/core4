"""
Wrfcoin Python SDK - Blockchain Service

Core blockchain operations and network information.
"""

from typing import Dict, Any, List, Optional
from ..exceptions import NetworkError, ValidationError, InvalidAddressError


class BlockchainService:
    """Service for blockchain operations and network information"""
    
    def __init__(self, graphql_client):
        self.graphql = graphql_client
    
    async def get_network_info(self) -> Dict[str, Any]:
        """Get current network information"""
        query = """
        query GetNetworkInfo {
            networkInfo {
                chainId
                blockNumber
                blockTime
                gasPrice
                totalSupply
                circulatingSupply
                stakingRewards
                validators {
                    address
                    stake
                    active
                }
            }
        }
        """
        
        result = await self.graphql.query(query)
        return result.get('networkInfo', {})
    
    async def get_block(self, block_number: Optional[int] = None) -> Dict[str, Any]:
        """Get block information by number (latest if None)"""
        query = """
        query GetBlock($blockNumber: Int) {
            block(number: $blockNumber) {
                number
                hash
                timestamp
                parentHash
                gasUsed
                gasLimit
                transactionCount
                transactions {
                    hash
                    from
                    to
                    value
                    gasPrice
                    gasUsed
                    status
                }
            }
        }
        """
        
        variables = {"blockNumber": block_number} if block_number is not None else {}
        result = await self.graphql.query(query, variables)
        return result.get('block', {})
    
    async def get_transaction(self, tx_hash: str) -> Dict[str, Any]:
        """Get transaction details by hash"""
        if not tx_hash or not isinstance(tx_hash, str):
            raise ValidationError("Transaction hash is required and must be a string")
        
        query = """
        query GetTransaction($hash: String!) {
            transaction(hash: $hash) {
                hash
                blockNumber
                transactionIndex
                from
                to
                value
                gasPrice
                gasLimit
                gasUsed
                status
                logs {
                    address
                    topics
                    data
                }
                receipt {
                    status
                    gasUsed
                    logs {
                        address
                        topics
                        data
                    }
                }
            }
        }
        """
        
        result = await self.graphql.query(query, {"hash": tx_hash})
        return result.get('transaction', {})
    
    async def get_balance(self, address: str, token_address: Optional[str] = None) -> Dict[str, Any]:
        """Get account balance (WRF or specific token)"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        if token_address:
            query = """
            query GetTokenBalance($address: String!, $tokenAddress: String!) {
                tokenBalance(address: $address, tokenAddress: $tokenAddress) {
                    balance
                    symbol
                    decimals
                    name
                }
            }
            """
            variables = {"address": address, "tokenAddress": token_address}
        else:
            query = """
            query GetBalance($address: String!) {
                account(address: $address) {
                    balance
                    nonce
                    transactionCount
                }
            }
            """
            variables = {"address": address}
        
        result = await self.graphql.query(query, variables)
        
        if token_address:
            return result.get('tokenBalance', {})
        else:
            return result.get('account', {})
    
    async def get_gas_price(self) -> Dict[str, Any]:
        """Get current gas price recommendations"""
        query = """
        query GetGasPrice {
            gasPrice {
                slow
                standard  
                fast
                instant
                estimatedWaitTimes {
                    slow
                    standard
                    fast
                    instant
                }
            }
        }
        """
        
        result = await self.graphql.query(query)
        return result.get('gasPrice', {})
    
    async def estimate_gas(self, transaction: Dict[str, Any]) -> int:
        """Estimate gas for a transaction"""
        required_fields = ['from', 'to']
        for field in required_fields:
            if field not in transaction:
                raise ValidationError(f"Transaction field '{field}' is required")
        
        if not self._is_valid_address(transaction['from']):
            raise InvalidAddressError(f"Invalid from address: {transaction['from']}")
        
        if not self._is_valid_address(transaction['to']):
            raise InvalidAddressError(f"Invalid to address: {transaction['to']}")
        
        query = """
        query EstimateGas($transaction: TransactionInput!) {
            estimateGas(transaction: $transaction) {
                gasLimit
                gasPrice
                totalCost
            }
        }
        """
        
        result = await self.graphql.query(query, {"transaction": transaction})
        gas_estimate = result.get('estimateGas', {})
        return gas_estimate.get('gasLimit', 0)
    
    async def get_transaction_count(self, address: str) -> int:
        """Get transaction count (nonce) for address"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        query = """
        query GetTransactionCount($address: String!) {
            account(address: $address) {
                nonce
                transactionCount
            }
        }
        """
        
        result = await self.graphql.query(query, {"address": address})
        account = result.get('account', {})
        return account.get('nonce', 0)
    
    async def get_accounts(self, limit: int = 10, offset: int = 0) -> List[Dict[str, Any]]:
        """Get list of accounts"""
        if limit <= 0 or limit > 100:
            raise ValidationError("Limit must be between 1 and 100")
        
        query = """
        query GetAccounts($limit: Int!, $offset: Int!) {
            accounts(limit: $limit, offset: $offset) {
                address
                balance
                transactionCount
                isContract
                createdAt
            }
        }
        """
        
        result = await self.graphql.query(query, {"limit": limit, "offset": offset})
        return result.get('accounts', [])
    
    async def get_validators(self) -> List[Dict[str, Any]]:
        """Get list of network validators"""
        query = """
        query GetValidators {
            validators {
                address
                stake
                delegatedStake
                commission
                active
                jailed
                uptime
                missedBlocks
                rewards
            }
        }
        """
        
        result = await self.graphql.query(query)
        return result.get('validators', [])
    
    async def get_staking_info(self, address: Optional[str] = None) -> Dict[str, Any]:
        """Get staking information"""
        if address:
            if not self._is_valid_address(address):
                raise InvalidAddressError(f"Invalid address format: {address}")
            
            query = """
            query GetStakingInfo($address: String!) {
                stakingInfo(address: $address) {
                    stakedAmount
                    rewards
                    delegations {
                        validator
                        amount
                        rewards
                    }
                    unbonding {
                        validator
                        amount
                        completionTime
                    }
                }
            }
            """
            variables = {"address": address}
        else:
            query = """
            query GetGlobalStakingInfo {
                globalStakingInfo {
                    totalStaked
                    totalRewards
                    stakingRatio
                    averageAPY
                    validatorCount
                    activeValidators
                }
            }
            """
            variables = {}
        
        result = await self.graphql.query(query, variables)
        return result.get('stakingInfo' if address else 'globalStakingInfo', {})
    
    async def subscribe_to_blocks(self, callback) -> callable:
        """Subscribe to new blocks"""
        subscription = """
        subscription NewBlocks {
            newBlock {
                number
                hash
                timestamp
                transactionCount
                gasUsed
            }
        }
        """
        
        return await self.graphql.subscribe(
            subscription,
            on_data=callback
        )
    
    async def subscribe_to_transactions(self, address: Optional[str] = None, callback = None) -> callable:
        """Subscribe to transactions (optionally filtered by address)"""
        if address and not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        if address:
            subscription = """
            subscription TransactionsByAddress($address: String!) {
                transactionsByAddress(address: $address) {
                    hash
                    from
                    to
                    value
                    status
                    blockNumber
                }
            }
            """
            variables = {"address": address}
        else:
            subscription = """
            subscription NewTransactions {
                newTransaction {
                    hash
                    from
                    to
                    value
                    status
                    blockNumber
                }
            }
            """
            variables = {}
        
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
    
    def format_balance(self, balance: str, decimals: int = 18) -> float:
        """Format balance from wei to human readable"""
        try:
            balance_int = int(balance)
            return balance_int / (10 ** decimals)
        except (ValueError, TypeError):
            raise ValidationError(f"Invalid balance format: {balance}")
    
    def format_gas_price(self, gas_price: str) -> float:
        """Format gas price from wei to gwei"""
        try:
            gas_price_int = int(gas_price)
            return gas_price_int / (10 ** 9)  # Convert wei to gwei
        except (ValueError, TypeError):
            raise ValidationError(f"Invalid gas price format: {gas_price}")