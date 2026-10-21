"""
Wrfcoin Python SDK - Governance Service

DAO governance, voting, proposals, and community participation.
"""

from typing import Dict, Any, List, Optional
from datetime import datetime
from ..exceptions import (
    ValidationError,
    InvalidAddressError,
    GovernanceError,
    TransactionError,
    InsufficientFundsError
)


class GovernanceService:
    """Service for DAO governance operations"""
    
    def __init__(self, graphql_client):
        self.graphql = graphql_client
    
    async def get_proposal(self, proposal_id: str) -> Dict[str, Any]:
        """Get detailed proposal information"""
        if not proposal_id:
            raise ValidationError("Proposal ID is required")
        
        query = """
        query GetProposal($proposalId: String!) {
            proposal(id: $proposalId) {
                id
                title
                description
                proposer
                status
                category
                votingStartTime
                votingEndTime
                executionTime
                quorum
                threshold
                voting {
                    forVotes
                    againstVotes
                    abstainVotes
                    totalVotes
                    participationRate
                    currentQuorum
                    passingThreshold
                }
                execution {
                    isExecutable
                    executionDelay
                    executedAt
                    executionHash
                    failed
                    failureReason
                }
                metadata {
                    ipfsHash
                    discussionUrl
                    documentUrl
                    summary
                }
                timeline {
                    event
                    timestamp
                    transactionHash
                }
                votes {
                    voter
                    choice
                    votingPower
                    timestamp
                    reason
                }
            }
        }
        """
        
        result = await self.graphql.query(query, {"proposalId": proposal_id})
        proposal = result.get('proposal')
        
        if not proposal:
            raise GovernanceError(f"Proposal not found: {proposal_id}")
        
        return proposal
    
    async def get_proposals(
        self,
        status: Optional[str] = None,
        category: Optional[str] = None,
        proposer: Optional[str] = None,
        limit: int = 20,
        offset: int = 0
    ) -> List[Dict[str, Any]]:
        """Get list of proposals with filtering"""
        if proposer and not self._is_valid_address(proposer):
            raise InvalidAddressError(f"Invalid proposer address: {proposer}")
        
        if limit <= 0 or limit > 100:
            raise ValidationError("Limit must be between 1 and 100")
        
        query = """
        query GetProposals($status: String, $category: String, $proposer: String, $limit: Int!, $offset: Int!) {
            proposals(status: $status, category: $category, proposer: $proposer, limit: $limit, offset: $offset) {
                proposals {
                    id
                    title
                    description
                    proposer
                    status
                    category
                    votingStartTime
                    votingEndTime
                    voting {
                        forVotes
                        againstVotes
                        totalVotes
                        participationRate
                    }
                    metadata {
                        summary
                    }
                }
                totalCount
                hasMore
            }
        }
        """
        
        variables = {
            "limit": limit,
            "offset": offset
        }
        
        if status:
            variables["status"] = status
        if category:
            variables["category"] = category
        if proposer:
            variables["proposer"] = proposer
        
        result = await self.graphql.query(query, variables)
        proposals_data = result.get('proposals', {})
        return proposals_data.get('proposals', [])
    
    async def create_proposal(
        self,
        proposer_address: str,
        title: str,
        description: str,
        category: str,
        actions: List[Dict[str, Any]],
        metadata: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """Create a new governance proposal"""
        if not self._is_valid_address(proposer_address):
            raise InvalidAddressError(f"Invalid proposer address: {proposer_address}")
        
        if not title or len(title.strip()) < 5:
            raise ValidationError("Title must be at least 5 characters long")
        
        if not description or len(description.strip()) < 20:
            raise ValidationError("Description must be at least 20 characters long")
        
        if not category:
            raise ValidationError("Category is required")
        
        if not actions:
            raise ValidationError("At least one action is required")
        
        # Validate actions
        for i, action in enumerate(actions):
            if 'target' not in action or 'data' not in action:
                raise ValidationError(f"Action {i} must include 'target' and 'data' fields")
            
            if not self._is_valid_address(action['target']):
                raise InvalidAddressError(f"Invalid target address in action {i}: {action['target']}")
        
        mutation = """
        mutation CreateProposal(
            $proposerAddress: String!,
            $title: String!,
            $description: String!,
            $category: String!,
            $actions: [ProposalActionInput!]!,
            $metadata: ProposalMetadataInput
        ) {
            createProposal(
                proposerAddress: $proposerAddress,
                title: $title,
                description: $description,
                category: $category,
                actions: $actions,
                metadata: $metadata
            ) {
                proposalId
                transactionHash
                status
                votingStartTime
                votingEndTime
                quorum
                threshold
                proposalFee
            }
        }
        """
        
        variables = {
            "proposerAddress": proposer_address,
            "title": title,
            "description": description,
            "category": category,
            "actions": actions,
            "metadata": metadata
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('createProposal', {})
        except Exception as error:
            if "insufficient" in str(error).lower():
                raise InsufficientFundsError(f"Insufficient balance for proposal fee: {error}")
            raise GovernanceError(f"Failed to create proposal: {error}")
    
    async def vote_on_proposal(
        self,
        voter_address: str,
        proposal_id: str,
        choice: int,
        reason: Optional[str] = None
    ) -> Dict[str, Any]:
        """Vote on a proposal"""
        if not self._is_valid_address(voter_address):
            raise InvalidAddressError(f"Invalid voter address: {voter_address}")
        
        if not proposal_id:
            raise ValidationError("Proposal ID is required")
        
        if choice not in [0, 1, 2]:  # 0=against, 1=for, 2=abstain
            raise ValidationError("Choice must be 0 (against), 1 (for), or 2 (abstain)")
        
        mutation = """
        mutation VoteOnProposal(
            $voterAddress: String!,
            $proposalId: String!,
            $choice: Int!,
            $reason: String
        ) {
            voteOnProposal(
                voterAddress: $voterAddress,
                proposalId: $proposalId,
                choice: $choice,
                reason: $reason
            ) {
                voteId
                transactionHash
                status
                votingPower
                choice
                timestamp
                gasUsed
            }
        }
        """
        
        variables = {
            "voterAddress": voter_address,
            "proposalId": proposal_id,
            "choice": choice,
            "reason": reason
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('voteOnProposal', {})
        except Exception as error:
            raise GovernanceError(f"Failed to vote on proposal: {error}")
    
    async def delegate_voting_power(
        self,
        delegator_address: str,
        delegate_address: str,
        amount: Optional[str] = None
    ) -> Dict[str, Any]:
        """Delegate voting power to another address"""
        if not self._is_valid_address(delegator_address):
            raise InvalidAddressError(f"Invalid delegator address: {delegator_address}")
        
        if not self._is_valid_address(delegate_address):
            raise InvalidAddressError(f"Invalid delegate address: {delegate_address}")
        
        if amount:
            try:
                amount_float = float(amount)
                if amount_float <= 0:
                    raise ValidationError("Delegation amount must be positive")
            except (ValueError, TypeError):
                raise ValidationError(f"Invalid amount: {amount}")
        
        mutation = """
        mutation DelegateVotingPower(
            $delegatorAddress: String!,
            $delegateAddress: String!,
            $amount: String
        ) {
            delegateVotingPower(
                delegatorAddress: $delegatorAddress,
                delegateAddress: $delegateAddress,
                amount: $amount
            ) {
                delegationId
                transactionHash
                status
                delegatedAmount
                totalDelegated
                delegateVotingPower
                gasUsed
            }
        }
        """
        
        variables = {
            "delegatorAddress": delegator_address,
            "delegateAddress": delegate_address,
            "amount": amount
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('delegateVotingPower', {})
        except Exception as error:
            raise GovernanceError(f"Failed to delegate voting power: {error}")
    
    async def execute_proposal(
        self,
        executor_address: str,
        proposal_id: str
    ) -> Dict[str, Any]:
        """Execute a passed proposal"""
        if not self._is_valid_address(executor_address):
            raise InvalidAddressError(f"Invalid executor address: {executor_address}")
        
        if not proposal_id:
            raise ValidationError("Proposal ID is required")
        
        mutation = """
        mutation ExecuteProposal($executorAddress: String!, $proposalId: String!) {
            executeProposal(executorAddress: $executorAddress, proposalId: $proposalId) {
                executionId
                transactionHash
                status
                executedAt
                gasUsed
                success
                results {
                    actionIndex
                    success
                    returnData
                    gasUsed
                }
            }
        }
        """
        
        variables = {
            "executorAddress": executor_address,
            "proposalId": proposal_id
        }
        
        try:
            result = await self.graphql.mutate(mutation, variables)
            return result.get('executeProposal', {})
        except Exception as error:
            raise GovernanceError(f"Failed to execute proposal: {error}")
    
    async def get_voting_power(self, address: str) -> Dict[str, Any]:
        """Get voting power for an address"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        query = """
        query GetVotingPower($address: String!) {
            votingPower(address: $address) {
                address
                totalVotingPower
                tokenBalance
                delegatedToSelf
                delegatedToOthers
                delegatedFromOthers
                delegations {
                    delegate
                    amount
                    timestamp
                }
                receivedDelegations {
                    delegator
                    amount
                    timestamp
                }
                stakingMultiplier
                governanceRewards
                participationRate
            }
        }
        """
        
        result = await self.graphql.query(query, {"address": address})
        return result.get('votingPower', {})
    
    async def get_governance_stats() -> Dict[str, Any]:
        """Get overall governance statistics"""
        query = """
        query GetGovernanceStats {
            governanceStats {
                totalProposals
                activeProposals
                totalVoters
                totalVotingPower
                treasuryBalance
                proposalCategories {
                    category
                    count
                    successRate
                }
                recentActivity {
                    proposalsCreated24h
                    votescast24h
                    participationRate24h
                }
                topDelegates {
                    address
                    delegatedPower
                    delegatorCount
                    participationRate
                }
            }
        }
        """
        
        result = await self.graphql.query(query)
        return result.get('governanceStats', {})
    
    async def get_user_governance_activity(
        self,
        address: str,
        limit: int = 20,
        offset: int = 0
    ) -> Dict[str, Any]:
        """Get user's governance activity history"""
        if not self._is_valid_address(address):
            raise InvalidAddressError(f"Invalid address format: {address}")
        
        if limit <= 0 or limit > 100:
            raise ValidationError("Limit must be between 1 and 100")
        
        query = """
        query GetUserGovernanceActivity($address: String!, $limit: Int!, $offset: Int!) {
            userGovernanceActivity(address: $address, limit: $limit, offset: $offset) {
                votes {
                    proposalId
                    proposalTitle
                    choice
                    votingPower
                    timestamp
                    reason
                }
                proposalsCreated {
                    proposalId
                    title
                    status
                    votingStartTime
                    createdAt
                }
                delegations {
                    type
                    delegate
                    delegator
                    amount
                    timestamp
                }
                rewards {
                    type
                    amount
                    timestamp
                    source
                }
                stats {
                    totalVotes
                    totalProposals
                    participationRate
                    totalRewards
                }
            }
        }
        """
        
        variables = {
            "address": address,
            "limit": limit,
            "offset": offset
        }
        
        result = await self.graphql.query(query, variables)
        return result.get('userGovernanceActivity', {})
    
    async def subscribe_to_governance_events(self, callback) -> callable:
        """Subscribe to governance events"""
        subscription = """
        subscription GovernanceEvents {
            governanceEvents {
                type
                proposalId
                address
                timestamp
                data {
                    ... on ProposalCreated {
                        title
                        proposer
                        category
                    }
                    ... on VoteCast {
                        voter
                        choice
                        votingPower
                    }
                    ... on ProposalExecuted {
                        executor
                        success
                    }
                }
            }
        }
        """
        
        return await self.graphql.subscribe(
            subscription,
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
    
    def get_vote_choice_text(self, choice: int) -> str:
        """Convert vote choice number to human readable text"""
        choices = {
            0: "Against",
            1: "For", 
            2: "Abstain"
        }
        return choices.get(choice, "Unknown")
    
    def calculate_quorum_progress(self, total_votes: int, quorum: int, total_supply: int) -> float:
        """Calculate quorum progress percentage"""
        if total_supply == 0:
            return 0.0
        
        required_votes = (quorum * total_supply) / 100
        if required_votes == 0:
            return 100.0
        
        return min((total_votes / required_votes) * 100, 100.0)
    
    def is_proposal_passing(self, for_votes: int, against_votes: int, threshold: int) -> bool:
        """Check if proposal is currently passing based on threshold"""
        total_votes = for_votes + against_votes
        if total_votes == 0:
            return False
        
        approval_rate = (for_votes / total_votes) * 100
        return approval_rate >= threshold