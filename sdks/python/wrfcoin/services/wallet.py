"""
Wrfcoin Python SDK - Wallet Service

Wallet operations, transactions, and account management.
"""

from typing import Dict, Any, List, Optional
from ..exceptions import (
    ValidationError, 
    InvalidAddressError, 
    InvalidAmountError,
    InsufficientFundsError,
    TransactionError
)


class WalletService:
    """Service for wallet operations and transaction management"""
    
    def __init__(self, graphql_client):
        self.graphql = graphql_client
    
    async def get_balance(self, address: str) -> Dict[str, Any]:
        """Get wallet balance and token holdings"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        query = """
        query GetWalletBalance($address: String!) {
            wallet(address: $address) {
                address
                balance
                tokens {
                    address
                    symbol
                    name
                    balance
                    decimals
                    priceUSD
                    valueUSD
                }
                totalValueUSD
                transactionCount
            }
        }
        """
        
        result = await self.graphql.query(query, {"address": address})
        return result.get('wallet', {})
    
    async def send_transaction(self, transaction: Dict[str, Any]) -> Dict[str, Any]:
        """Send a transaction"""
        # Validate required fields
        required_fields = ['from', 'to', 'value']
        for field in required_fields:
            if field not in transaction:
                raise ValidationError(f"Transaction field '{field}' is required")
        
        # Validate addresses
        if not self._is_valid_address(transaction['from']):
            raise InvalidAddressError(f"Invalid from address: {transaction['from']}")
        
        if not self._is_valid_address(transaction['to']):
            raise InvalidAddressError(f"Invalid to address: {transaction['to']}")
        
        # Validate amount
        try:
            value = float(transaction['value'])
            if value <= 0:
                raise InvalidAmountError("Transaction value must be positive")
        except (ValueError, TypeError):
            raise InvalidAmountError(f"Invalid transaction value: {transaction['value']}")
        
        mutation = """
        mutation SendTransaction($transaction: TransactionInput!) {
            sendTransaction(transaction: $transaction) {
                hash
                status
                gasUsed
                gasPrice
                blockNumber
                blockHash
                transactionIndex
                from
                to
                value
                confirmations
            }
        }
        """
        
        try:
            result = await self.graphql.mutate(mutation, {"transaction": transaction})
            return result.get('sendTransaction', {})
        except Exception as error:
            raise TransactionError(f"Failed to send transaction: {error}")
    
    async def transfer_tokens(
        self, 
        from_address: str, 
        to_address: str, 
        amount: str, 
        token_address: Optional[str] = None
    ) -> Dict[str, Any]:
        """Transfer WRF tokens or other tokens"""
        if not self._is_valid_address(from_address):
            raise InvalidAddressError(f"Invalid from address: {from_address}")
        
        if not self._is_valid_address(to_address):
            raise InvalidAddressError(f"Invalid to address: {to_address}")
        
        try:
            amount_float = float(amount)
            if amount_float <= 0:
                raise InvalidAmountError("Transfer amount must be positive")
        except (ValueError, TypeError):
            raise InvalidAmountError(f"Invalid amount: {amount}")
        
        mutation = """
        mutation TransferTokens($fromAddress: String!, $toAddress: String!, $amount: String!, $tokenAddress: String) {
            transferTokens(
                fromAddress: $fromAddress,
                toAddress: $toAddress,
                amount: $amount,
                tokenAddress: $tokenAddress
            ) {
                hash
                status
                from
                to
                value
                gasUsed
                confirmations
            }
        }
        """
        
        variables = {
            "fromAddress": from_address,
            "toAddress": to_address,
            "amount": amount,
            "tokenAddress": token_address
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('transferTokens', {})
        except Exception as error:
            if "insufficient" in str(error).lower():
                raise InsufficientFundsError(f"Insufficient balance for transfer: {error}")
            raise TransactionError(f"Failed to transfer tokens: {error}")
    
    async def get_transaction_history(
        self, 
        address: str, 
        limit: int = 20, 
        offset: int = 0,
        transaction_type: Optional[str] = None
    ) -> List[Dict[str, Any]]:
        """Get transaction history for an address"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        if limit <= 0 or limit > 100:
            raise ValidationError("Limit must be between 1 and 100")
        
        query = """
        query GetTransactionHistory($address: String!, $limit: Int!, $offset: Int!, $type: String) {
            transactionHistory(address: $address, limit: $limit, offset: $offset, type: $type) {
                transactions {
                    hash
                    blockNumber
                    timestamp
                    from
                    to
                    value
                    gasPrice
                    gasUsed
                    status
                    type
                    tokenTransfers {
                        token
                        from
                        to
                        value
                        symbol
                    }
                }
                totalCount
                hasMore
            }
        }
        """
        
        variables = {
            "address": address,
            "limit": limit,
            "offset": offset,
            "type": transaction_type
        }
        
        result = await self.graphql.query(query, variables)
        history = result.get('transactionHistory', {})
        return history.get('transactions', [])
    
    async def get_pending_transactions(self, address: str) -> List[Dict[str, Any]]:
        """Get pending transactions for an address"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        query = """
        query GetPendingTransactions($address: String!) {
            pendingTransactions(address: $address) {
                hash
                from
                to
                value
                gasPrice
                gasLimit
                nonce
                timestamp
            }
        }
        """
        
        result = await self.graphql.query(query, {"address": address})
        return result.get('pendingTransactions', [])
    
    async def approve_token_spending(
        self, 
        owner_address: str, 
        spender_address: str, 
        token_address: str, 
        amount: str
    ) -> Dict[str, Any]:
        """Approve token spending allowance"""
        if not self._is_valid_address(owner_address):
            raise InvalidAddressError(f"Invalid owner address: {owner_address}")
        
        if not self._is_valid_address(spender_address):
            raise InvalidAddressError(f"Invalid spender address: {spender_address}")
        
        if not self._is_valid_address(token_address):
            raise InvalidAddressError(f"Invalid token address: {token_address}")
        
        try:
            amount_float = float(amount)
            if amount_float < 0:
                raise InvalidAmountError("Approval amount cannot be negative")
        except (ValueError, TypeError):
            raise InvalidAmountError(f"Invalid amount: {amount}")
        
        mutation = """
        mutation ApproveTokenSpending(
            $ownerAddress: String!,
            $spenderAddress: String!,
            $tokenAddress: String!,
            $amount: String!
        ) {
            approveTokenSpending(
                ownerAddress: $ownerAddress,
                spenderAddress: $spenderAddress,
                tokenAddress: $tokenAddress,
                amount: $amount
            ) {
                hash
                status
                allowance
                gasUsed
            }
        }
        """
        
        variables = {
            "ownerAddress": owner_address,
            "spenderAddress": spender_address,
            "tokenAddress": token_address,
            "amount": amount
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('approveTokenSpending', {})
        except Exception as error:
            raise TransactionError(f"Failed to approve token spending: {error}")
    
    async def get_token_allowance(
        self, 
        owner_address: str, 
        spender_address: str, 
        token_address: str
    ) -> Dict[str, Any]:
        """Get token spending allowance"""
        if not self._is_valid_address(owner_address):
            raise InvalidAddressError(f"Invalid owner address: {owner_address}")
        
        if not self._is_valid_address(spender_address):
            raise InvalidAddressError(f"Invalid spender address: {spender_address}")
        
        if not self._is_valid_address(token_address):
            raise InvalidAddressError(f"Invalid token address: {token_address}")
        
        query = """
        query GetTokenAllowance($ownerAddress: String!, $spenderAddress: String!, $tokenAddress: String!) {
            tokenAllowance(
                ownerAddress: $ownerAddress,
                spenderAddress: $spenderAddress,
                tokenAddress: $tokenAddress
            ) {
                allowance
                symbol
                decimals
                spenderAddress
                ownerAddress
            }
        }
        """
        
        variables = {
            "ownerAddress": owner_address,
            "spenderAddress": spender_address,
            "tokenAddress": token_address
        }
        
        result = await self.graphql.query(query, variables)
        return result.get('tokenAllowance', {})
    
    async def estimate_transaction_fee(self, transaction: Dict[str, Any]) -> Dict[str, Any]:
        """Estimate transaction fees"""
        required_fields = ['from', 'to']
        for field in required_fields:
            if field not in transaction:
                raise ValidationError(f"Transaction field '{field}' is required")
        
        query = """
        query EstimateTransactionFee($transaction: TransactionInput!) {
            estimateTransactionFee(transaction: $transaction) {
                gasLimit
                gasPrice
                gasFee
                priorityFee
                totalFee
                feeInUSD
            }
        }
        """
        
        result = await self.graphql.query(query, {"transaction": transaction})
        return result.get('estimateTransactionFee', {})
    
    async def get_portfolio(self, address: str) -> Dict[str, Any]:
        """Get comprehensive portfolio information"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        query = """
        query GetPortfolio($address: String!) {
            portfolio(address: $address) {
                totalValueUSD
                wrfBalance
                wrfValueUSD
                tokens {
                    address
                    symbol
                    name
                    balance
                    valueUSD
                    priceUSD
                    change24h
                }
                nfts {
                    tokenId
                    collection
                    name
                    imageUrl
                    valueUSD
                }
                stakingPositions {
                    validator
                    stakedAmount
                    rewards
                    apy
                }
                governanceVotes {
                    proposalId
                    choice
                    votingPower
                    timestamp
                }
            }
        }
        """
        
        result = await self.graphql.query(query, {"address": address})
        return result.get('portfolio', {})
    
    async def subscribe_to_balance_changes(self, address: str, callback) -> callable:
        """Subscribe to balance changes for an address"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        subscription = """
        subscription BalanceChanges($address: String!) {
            balanceChanges(address: $address) {
                address
                balance
                previousBalance
                change
                timestamp
                transactionHash
            }
        }
        """
        
        return await self.graphql.subscribe(
            subscription,
            variables={"address": address},
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
    
    def format_token_amount(self, amount: str, decimals: int) -> float:
        """Format token amount from smallest unit to human readable"""
        try:
            amount_int = int(amount)
            return amount_int / (10 ** decimals)
        except (ValueError, TypeError):
            raise ValidationError(f"Invalid amount format: {amount}")
    
    def parse_token_amount(self, amount: float, decimals: int) -> str:
        """Parse human readable amount to smallest unit"""
        try:
            amount_int = int(amount * (10 ** decimals))
            return str(amount_int)
        except (ValueError, TypeError):
            raise ValidationError(f"Invalid amount: {amount}")