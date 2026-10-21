// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../tokens/GovernanceToken.sol";

/**
 * @title BasicDAO
 * @dev Simple DAO implementation with proposal and voting mechanisms
 * 
 * Features:
 * - Proposal creation and voting
 * - Time-locked execution
 * - Quorum requirements
 * - Multiple voting types
 * - Treasury management
 * - Member management
 * 
 * Use Cases:
 * - Community governance
 * - Protocol governance
 * - Treasury management
 * - Decentralized decision making
 */
contract BasicDAO is SecurityBase {
    enum ProposalState {
        PENDING,    // Proposal created but not active
        ACTIVE,     // Voting period active
        SUCCEEDED,  // Proposal passed
        DEFEATED,   // Proposal failed
        QUEUED,     // Proposal queued for execution
        EXECUTED,   // Proposal executed
        CANCELLED,  // Proposal cancelled
        EXPIRED     // Proposal expired
    }
    
    enum VoteType {
        FOR,
        AGAINST,
        ABSTAIN
    }
    
    struct Proposal {
        uint256 id;
        address proposer;
        string title;
        string description;
        uint256 startTime;
        uint256 endTime;
        uint256 executionTime;
        uint256 forVotes;
        uint256 againstVotes;
        uint256 abstainVotes;
        bool executed;
        bool cancelled;
        ProposalState state;
        bytes executionData;
        address targetContract;
        uint256 value; // ETH value for execution
    }
    
    struct VoteInfo {
        bool hasVoted;
        VoteType vote;
        uint256 votingPower;
        string reason;
    }
    
    // DAO Configuration
    GovernanceToken public immutable governanceToken;
    uint256 public votingDelay = 1 days;        // Delay before voting starts
    uint256 public votingPeriod = 7 days;       // Voting period duration
    uint256 public executionDelay = 2 days;     // Timelock delay
    uint256 public proposalThreshold = 1000 * 10**18; // Min tokens to propose
    uint256 public quorumNumerator = 4;         // 4% quorum required
    uint256 public quorumDenominator = 100;
    
    // Proposal management
    mapping(uint256 => Proposal) public proposals;
    mapping(uint256 => mapping(address => VoteInfo)) public proposalVotes;
    uint256 public proposalCount;
    uint256 public lastProposalId;
    
    // Treasury management
    uint256 public treasuryBalance;
    mapping(address => uint256) public tokenBalances;
    
    // Member management
    mapping(address => bool) public members;
    mapping(address => uint256) public memberSince;
    uint256 public memberCount;
    
    // Delegation
    mapping(address => address) public delegates;
    mapping(address => uint256) public delegatedVotes;
    
    // Events
    event ProposalCreated(
        uint256 indexed proposalId,
        address indexed proposer,
        string title,
        uint256 startTime,
        uint256 endTime
    );
    event VoteCast(
        uint256 indexed proposalId,
        address indexed voter,
        VoteType vote,
        uint256 votingPower,
        string reason
    );
    event ProposalExecuted(uint256 indexed proposalId, bool success);
    event ProposalCancelled(uint256 indexed proposalId);
    event ProposalQueued(uint256 indexed proposalId, uint256 executionTime);
    event MemberAdded(address indexed member, uint256 timestamp);
    event MemberRemoved(address indexed member, uint256 timestamp);
    event VotingParametersUpdated(
        uint256 votingDelay,
        uint256 votingPeriod,
        uint256 executionDelay,
        uint256 proposalThreshold
    );
    event FundsDeposited(address indexed from, uint256 amount);
    event FundsWithdrawn(address indexed to, uint256 amount);
    
    // Errors
    error InsufficientProposalThreshold();
    error ProposalNotActive();
    error AlreadyVoted();
    error ProposalNotSucceeded();
    error ProposalNotQueued();
    error TimelockNotExpired();
    error ProposalExpired();
    error ExecutionFailed();
    error NotMember();
    error QuorumNotReached();
    error InvalidVotingParameters();
    
    modifier onlyMember() {
        if (!members[msg.sender]) {
            revert NotMember();
        }
        _;
    }
    
    modifier proposalExists(uint256 proposalId) {
        require(proposalId < proposalCount, "BasicDAO: Proposal does not exist");
        _;
    }
    
    constructor(
        address _governanceToken,
        address initialOwner
    ) SecurityBase(initialOwner) {
        governanceToken = GovernanceToken(_governanceToken);
        
        // Add initial owner as first member
        members[initialOwner] = true;
        memberSince[initialOwner] = block.timestamp;
        memberCount = 1;
        
        emit MemberAdded(initialOwner, block.timestamp);
    }
    
    /**
     * @dev Receive ETH deposits to treasury
     */
    receive() external payable {
        treasuryBalance += msg.value;
        emit FundsDeposited(msg.sender, msg.value);
    }
    
    /**
     * @dev Create a new proposal
     */
    function propose(
        string calldata title,
        string calldata description,
        address targetContract,
        uint256 value,
        bytes calldata executionData
    ) external onlyMember returns (uint256) {
        // Check proposal threshold
        uint256 proposerVotes = getVotingPower(msg.sender);
        if (proposerVotes < proposalThreshold) {
            revert InsufficientProposalThreshold();
        }
        
        uint256 proposalId = proposalCount++;
        uint256 startTime = block.timestamp + votingDelay;
        uint256 endTime = startTime + votingPeriod;
        
        proposals[proposalId] = Proposal({
            id: proposalId,
            proposer: msg.sender,
            title: title,
            description: description,
            startTime: startTime,
            endTime: endTime,
            executionTime: 0,
            forVotes: 0,
            againstVotes: 0,
            abstainVotes: 0,
            executed: false,
            cancelled: false,
            state: ProposalState.PENDING,
            executionData: executionData,
            targetContract: targetContract,
            value: value
        });
        
        emit ProposalCreated(proposalId, msg.sender, title, startTime, endTime);
        return proposalId;
    }
    
    /**
     * @dev Cast a vote on a proposal
     */
    function castVote(
        uint256 proposalId,
        VoteType vote,
        string calldata reason
    ) external proposalExists(proposalId) onlyMember {
        _updateProposalState(proposalId);
        
        Proposal storage proposal = proposals[proposalId];
        if (proposal.state != ProposalState.ACTIVE) {
            revert ProposalNotActive();
        }
        
        VoteInfo storage voteInfo = proposalVotes[proposalId][msg.sender];
        if (voteInfo.hasVoted) {
            revert AlreadyVoted();
        }
        
        uint256 votingPower = getVotingPower(msg.sender);
        require(votingPower > 0, "BasicDAO: No voting power");
        
        voteInfo.hasVoted = true;
        voteInfo.vote = vote;
        voteInfo.votingPower = votingPower;
        voteInfo.reason = reason;
        
        if (vote == VoteType.FOR) {
            proposal.forVotes += votingPower;
        } else if (vote == VoteType.AGAINST) {
            proposal.againstVotes += votingPower;
        } else {
            proposal.abstainVotes += votingPower;
        }
        
        emit VoteCast(proposalId, msg.sender, vote, votingPower, reason);
    }
    
    /**
     * @dev Queue a successful proposal for execution
     */
    function queueProposal(uint256 proposalId) 
        external 
        proposalExists(proposalId) 
        onlyRole(ADMIN_ROLE) 
    {
        _updateProposalState(proposalId);
        
        Proposal storage proposal = proposals[proposalId];
        if (proposal.state != ProposalState.SUCCEEDED) {
            revert ProposalNotSucceeded();
        }
        
        proposal.executionTime = block.timestamp + executionDelay;
        proposal.state = ProposalState.QUEUED;
        
        emit ProposalQueued(proposalId, proposal.executionTime);
    }
    
    /**
     * @dev Execute a queued proposal
     */
    function executeProposal(uint256 proposalId) 
        external 
        proposalExists(proposalId) 
        nonReentrant 
    {
        _updateProposalState(proposalId);
        
        Proposal storage proposal = proposals[proposalId];
        if (proposal.state != ProposalState.QUEUED) {
            revert ProposalNotQueued();
        }
        
        if (block.timestamp < proposal.executionTime) {
            revert TimelockNotExpired();
        }
        
        proposal.executed = true;
        proposal.state = ProposalState.EXECUTED;
        
        bool success = false;
        if (proposal.targetContract != address(0)) {
            // External contract call
            (success, ) = proposal.targetContract.call{value: proposal.value}(
                proposal.executionData
            );
        } else if (proposal.value > 0) {
            // ETH transfer
            require(treasuryBalance >= proposal.value, "BasicDAO: Insufficient treasury balance");
            treasuryBalance -= proposal.value;
            (success, ) = payable(proposal.proposer).call{value: proposal.value}("");
        } else {
            success = true; // Governance-only proposal
        }
        
        emit ProposalExecuted(proposalId, success);
        
        if (!success) {
            revert ExecutionFailed();
        }
    }
    
    /**
     * @dev Cancel a proposal
     */
    function cancelProposal(uint256 proposalId) 
        external 
        proposalExists(proposalId) 
    {
        Proposal storage proposal = proposals[proposalId];
        
        // Only proposer or admin can cancel
        require(
            msg.sender == proposal.proposer || hasRole(ADMIN_ROLE, msg.sender),
            "BasicDAO: Not authorized to cancel"
        );
        
        require(
            proposal.state == ProposalState.PENDING || 
            proposal.state == ProposalState.ACTIVE ||
            proposal.state == ProposalState.QUEUED,
            "BasicDAO: Cannot cancel executed proposal"
        );
        
        proposal.cancelled = true;
        proposal.state = ProposalState.CANCELLED;
        
        emit ProposalCancelled(proposalId);
    }
    
    /**
     * @dev Add member to DAO
     */
    function addMember(address account) external onlyRole(ADMIN_ROLE) {
        require(!members[account], "BasicDAO: Already a member");
        
        members[account] = true;
        memberSince[account] = block.timestamp;
        memberCount++;
        
        emit MemberAdded(account, block.timestamp);
    }
    
    /**
     * @dev Remove member from DAO
     */
    function removeMember(address account) external onlyRole(ADMIN_ROLE) {
        require(members[account], "BasicDAO: Not a member");
        
        members[account] = false;
        memberCount--;
        
        emit MemberRemoved(account, block.timestamp);
    }
    
    /**
     * @dev Delegate voting power to another address
     */
    function delegate(address delegatee) external onlyMember {
        address currentDelegate = delegates[msg.sender];
        
        if (currentDelegate != address(0)) {
            delegatedVotes[currentDelegate] -= getTokenVotingPower(msg.sender);
        }
        
        delegates[msg.sender] = delegatee;
        
        if (delegatee != address(0)) {
            delegatedVotes[delegatee] += getTokenVotingPower(msg.sender);
        }
    }
    
    /**
     * @dev Update voting parameters
     */
    function updateVotingParameters(
        uint256 _votingDelay,
        uint256 _votingPeriod,
        uint256 _executionDelay,
        uint256 _proposalThreshold,
        uint256 _quorumNumerator
    ) external onlyRole(ADMIN_ROLE) {
        if (_votingDelay > 30 days || _votingPeriod > 90 days || _executionDelay > 30 days) {
            revert InvalidVotingParameters();
        }
        if (_quorumNumerator > 100) {
            revert InvalidVotingParameters();
        }
        
        votingDelay = _votingDelay;
        votingPeriod = _votingPeriod;
        executionDelay = _executionDelay;
        proposalThreshold = _proposalThreshold;
        quorumNumerator = _quorumNumerator;
        
        emit VotingParametersUpdated(
            _votingDelay,
            _votingPeriod,
            _executionDelay,
            _proposalThreshold
        );
    }
    
    /**
     * @dev Withdraw funds from treasury
     */
    function withdrawTreasury(address to, uint256 amount) 
        external 
        onlyRole(ADMIN_ROLE) 
        nonReentrant 
    {
        require(amount <= treasuryBalance, "BasicDAO: Insufficient balance");
        
        treasuryBalance -= amount;
        payable(to).transfer(amount);
        
        emit FundsWithdrawn(to, amount);
    }
    
    /**
     * @dev Get voting power for an address
     */
    function getVotingPower(address account) public view returns (uint256) {
        if (!members[account]) return 0;
        
        uint256 tokenPower = getTokenVotingPower(account);
        uint256 delegatedPower = delegatedVotes[account];
        
        return tokenPower + delegatedPower;
    }
    
    /**
     * @dev Get token-based voting power
     */
    function getTokenVotingPower(address account) public view returns (uint256) {
        return governanceToken.getVotingPower(account);
    }
    
    /**
     * @dev Check if quorum is reached for a proposal
     */
    function quorumReached(uint256 proposalId) public view returns (bool) {
        Proposal storage proposal = proposals[proposalId];
        uint256 totalVotes = proposal.forVotes + proposal.againstVotes + proposal.abstainVotes;
        uint256 totalSupply = governanceToken.totalSupply();
        uint256 requiredQuorum = (totalSupply * quorumNumerator) / quorumDenominator;
        
        return totalVotes >= requiredQuorum;
    }
    
    /**
     * @dev Update proposal state based on current time and votes
     */
    function _updateProposalState(uint256 proposalId) internal {
        Proposal storage proposal = proposals[proposalId];
        
        if (proposal.cancelled) {
            proposal.state = ProposalState.CANCELLED;
            return;
        }
        
        if (proposal.executed) {
            proposal.state = ProposalState.EXECUTED;
            return;
        }
        
        if (block.timestamp < proposal.startTime) {
            proposal.state = ProposalState.PENDING;
        } else if (block.timestamp <= proposal.endTime) {
            proposal.state = ProposalState.ACTIVE;
        } else {
            // Voting ended, determine result
            if (quorumReached(proposalId) && proposal.forVotes > proposal.againstVotes) {
                proposal.state = ProposalState.SUCCEEDED;
            } else {
                proposal.state = ProposalState.DEFEATED;
            }
        }
        
        // Check for expiration (30 days after end time)
        if (proposal.state == ProposalState.SUCCEEDED && 
            block.timestamp > proposal.endTime + 30 days) {
            proposal.state = ProposalState.EXPIRED;
        }
    }
    
    /**
     * @dev Get proposal information
     */
    function getProposal(uint256 proposalId) external view returns (
        address proposer,
        string memory title,
        string memory description,
        uint256 startTime,
        uint256 endTime,
        uint256 forVotes,
        uint256 againstVotes,
        uint256 abstainVotes,
        ProposalState state,
        bool executed
    ) {
        require(proposalId < proposalCount, "BasicDAO: Proposal does not exist");
        Proposal storage proposal = proposals[proposalId];
        
        return (
            proposal.proposer,
            proposal.title,
            proposal.description,
            proposal.startTime,
            proposal.endTime,
            proposal.forVotes,
            proposal.againstVotes,
            proposal.abstainVotes,
            proposal.state,
            proposal.executed
        );
    }
    
    /**
     * @dev Get user's vote on a proposal
     */
    function getVote(uint256 proposalId, address voter) external view returns (
        bool hasVoted,
        VoteType vote,
        uint256 votingPower,
        string memory reason
    ) {
        VoteInfo storage voteInfo = proposalVotes[proposalId][voter];
        
        return (
            voteInfo.hasVoted,
            voteInfo.vote,
            voteInfo.votingPower,
            voteInfo.reason
        );
    }
    
    /**
     * @dev Get DAO statistics
     */
    function getDAOStats() external view returns (
        uint256 totalMembers,
        uint256 totalProposals,
        uint256 treasuryBalanceAmount,
        uint256 totalTokenSupply,
        uint256 currentQuorumRequirement
    ) {
        uint256 totalSupply = governanceToken.totalSupply();
        uint256 requiredQuorum = (totalSupply * quorumNumerator) / quorumDenominator;
        
        return (
            memberCount,
            proposalCount,
            treasuryBalance,
            totalSupply,
            requiredQuorum
        );
    }
}