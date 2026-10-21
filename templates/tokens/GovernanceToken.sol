// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BaseERC20Token.sol";

/**
 * @title GovernanceToken
 * @dev Advanced governance token with delegation, checkpointing, and voting power mechanics
 * 
 * Features:
 * - Full governance capabilities with vote delegation
 * - Historical balance checkpointing
 * - Quadratic voting support
 * - Reputation-based voting power
 * - Anti-sybil protections
 * - Governance reward mechanisms
 * 
 * Use Cases:
 * - DAO governance tokens
 * - Protocol governance
 * - Stakeholder voting rights
 * - Decentralized decision making
 */
contract GovernanceToken is BaseERC20Token {
    // Governance parameters
    uint256 public constant MIN_PROPOSAL_THRESHOLD = 1000 * 10**18; // 1000 tokens
    uint256 public constant MIN_VOTING_PERIOD = 3 days;
    uint256 public constant MAX_VOTING_PERIOD = 14 days;
    
    // Voting power mechanics
    mapping(address => uint256) public reputationScore;
    mapping(address => bool) public verifiedVoters;
    mapping(address => uint256) public lastVoteTimestamp;
    mapping(address => uint256) public totalVotesCast;
    
    // Governance rewards
    uint256 public governanceRewardRate = 10 * 10**18; // 10 tokens per participation
    mapping(address => uint256) public governanceRewardsEarned;
    mapping(address => uint256) public lastRewardClaim;
    
    // Quadratic voting credits
    mapping(address => mapping(uint256 => uint256)) public votingCredits; // voter => proposalId => credits
    
    // Delegation enhancements
    mapping(address => address[]) public delegationHistory;
    mapping(address => uint256) public delegationTimestamp;
    
    // Events
    event VoterVerified(address indexed voter, uint256 reputationScore);
    event GovernanceRewardClaimed(address indexed voter, uint256 amount);
    event VotingCreditsAllocated(address indexed voter, uint256 proposalId, uint256 credits);
    event ReputationUpdated(address indexed voter, uint256 oldScore, uint256 newScore);
    
    // Errors
    error InsufficientVotingPower();
    error VoterNotVerified();
    error InsufficientReputation();
    error GovernanceRewardsCooldown();
    
    constructor(
        string memory name,
        string memory symbol,
        uint256 maxSupply,
        uint256 initialSupply,
        address initialOwner
    ) BaseERC20Token(
        name,
        symbol,
        18,
        maxSupply,
        initialSupply,
        initialOwner
    ) {
        // Set initial reputation for owner
        reputationScore[initialOwner] = 1000;
        verifiedVoters[initialOwner] = true;
    }
    
    /**
     * @dev Verify a voter and set their reputation
     */
    function verifyVoter(address voter, uint256 reputation) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        require(reputation >= 100, "GovernanceToken: Minimum reputation required");
        
        verifiedVoters[voter] = true;
        uint256 oldScore = reputationScore[voter];
        reputationScore[voter] = reputation;
        
        emit VoterVerified(voter, reputation);
        emit ReputationUpdated(voter, oldScore, reputation);
    }
    
    /**
     * @dev Get enhanced voting power including reputation
     */
    function getVotingPower(address account) public view returns (uint256) {
        uint256 baseBalance = getVotes(account);
        
        if (!verifiedVoters[account]) {
            return baseBalance;
        }
        
        // Apply reputation multiplier (1x to 2x based on reputation)
        uint256 reputation = reputationScore[account];
        uint256 multiplier = 100 + (reputation * 100 / 1000); // Max 2x multiplier
        
        return (baseBalance * multiplier) / 100;
    }
    
    /**
     * @dev Calculate quadratic voting credits
     */
    function calculateVotingCredits(address voter) public view returns (uint256) {
        uint256 votingPower = getVotingPower(voter);
        
        // Quadratic voting: credits = sqrt(voting power)
        return _sqrt(votingPower / 10**18); // Normalize to token units
    }
    
    /**
     * @dev Allocate voting credits for a proposal
     */
    function allocateVotingCredits(address voter, uint256 proposalId) 
        external 
        onlyRole(OPERATOR_ROLE) 
    {
        if (!verifiedVoters[voter]) {
            revert VoterNotVerified();
        }
        
        uint256 credits = calculateVotingCredits(voter);
        votingCredits[voter][proposalId] = credits;
        
        emit VotingCreditsAllocated(voter, proposalId, credits);
    }
    
    /**
     * @dev Update reputation based on voting participation
     */
    function updateReputationFromVoting(address voter, bool participationQuality) 
        external 
        onlyRole(OPERATOR_ROLE) 
    {
        if (!verifiedVoters[voter]) {
            return;
        }
        
        uint256 oldScore = reputationScore[voter];
        uint256 newScore = oldScore;
        
        if (participationQuality) {
            // Reward good participation
            newScore = oldScore + 10;
            if (newScore > 1000) newScore = 1000; // Cap at 1000
        } else {
            // Penalize poor participation
            if (oldScore > 10) {
                newScore = oldScore - 5;
            }
        }
        
        reputationScore[voter] = newScore;
        lastVoteTimestamp[voter] = block.timestamp;
        totalVotesCast[voter]++;
        
        emit ReputationUpdated(voter, oldScore, newScore);
    }
    
    /**
     * @dev Claim governance participation rewards
     */
    function claimGovernanceRewards() external whenNotPaused {
        require(verifiedVoters[msg.sender], "GovernanceToken: Not a verified voter");
        require(
            block.timestamp >= lastRewardClaim[msg.sender] + 7 days,
            "GovernanceToken: Reward cooldown active"
        );
        
        // Calculate rewards based on participation
        uint256 recentVotes = _getRecentVotingActivity(msg.sender);
        if (recentVotes == 0) {
            revert InsufficientVotingPower();
        }
        
        uint256 rewardAmount = governanceRewardRate * recentVotes;
        
        // Apply reputation multiplier
        uint256 reputation = reputationScore[msg.sender];
        uint256 multiplier = 50 + (reputation * 50 / 1000); // 0.5x to 1.5x multiplier
        rewardAmount = (rewardAmount * multiplier) / 100;
        
        // Check max supply
        if (totalSupply() + rewardAmount > maxSupply) {
            revert ExceedsMaxSupply();
        }
        
        lastRewardClaim[msg.sender] = block.timestamp;
        governanceRewardsEarned[msg.sender] += rewardAmount;
        
        _mint(msg.sender, rewardAmount);
        emit GovernanceRewardClaimed(msg.sender, rewardAmount);
    }
    
    /**
     * @dev Set governance reward rate
     */
    function setGovernanceRewardRate(uint256 newRate) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        require(newRate <= 100 * 10**18, "GovernanceToken: Reward rate too high");
        governanceRewardRate = newRate;
    }
    
    /**
     * @dev Delegate with enhanced tracking
     */
    function delegate(address delegatee) public override {
        address currentDelegatee = delegates(msg.sender);
        
        // Track delegation history
        if (currentDelegatee != delegatee) {
            delegationHistory[msg.sender].push(delegatee);
            delegationTimestamp[msg.sender] = block.timestamp;
        }
        
        super.delegate(delegatee);
    }
    
    /**
     * @dev Get delegation history for an address
     */
    function getDelegationHistory(address account) 
        external 
        view 
        returns (address[] memory) 
    {
        return delegationHistory[account];
    }
    
    /**
     * @dev Check if address meets proposal threshold
     */
    function meetsProposalThreshold(address account) external view returns (bool) {
        return getVotingPower(account) >= MIN_PROPOSAL_THRESHOLD;
    }
    
    /**
     * @dev Get comprehensive voter information
     */
    function getVoterInfo(address voter) external view returns (
        bool isVerified,
        uint256 reputation,
        uint256 votingPower,
        uint256 totalVotes,
        uint256 lastVote,
        uint256 rewardsEarned,
        uint256 availableCredits
    ) {
        return (
            verifiedVoters[voter],
            reputationScore[voter],
            getVotingPower(voter),
            totalVotesCast[voter],
            lastVoteTimestamp[voter],
            governanceRewardsEarned[voter],
            calculateVotingCredits(voter)
        );
    }
    
    /**
     * @dev Get governance statistics
     */
    function getGovernanceStats() external view returns (
        uint256 totalVerifiedVoters,
        uint256 totalGovernanceRewards,
        uint256 averageReputation,
        uint256 proposalThreshold
    ) {
        // Note: These would need counters in a production implementation
        return (
            0, // Placeholder - implement counter
            0, // Placeholder - implement counter
            0, // Placeholder - implement calculation
            MIN_PROPOSAL_THRESHOLD
        );
    }
    
    /**
     * @dev Emergency reputation reset for governance attacks
     */
    function emergencyReputationReset(address[] calldata voters) 
        external 
        onlyRole(EMERGENCY_ROLE) 
    {
        for (uint256 i = 0; i < voters.length; i++) {
            reputationScore[voters[i]] = 100; // Reset to minimum
            verifiedVoters[voters[i]] = false;
        }
    }
    
    // Internal functions
    
    /**
     * @dev Calculate recent voting activity
     */
    function _getRecentVotingActivity(address voter) internal view returns (uint256) {
        // Simplified implementation - in production, track specific timeframes
        if (block.timestamp <= lastVoteTimestamp[voter] + 30 days) {
            return 1; // Participated in last 30 days
        }
        return 0;
    }
    
    /**
     * @dev Square root function for quadratic voting
     */
    function _sqrt(uint256 x) internal pure returns (uint256) {
        if (x == 0) return 0;
        uint256 z = (x + 1) / 2;
        uint256 y = x;
        while (z < y) {
            y = z;
            z = (x / z + z) / 2;
        }
        return y;
    }
    
    /**
     * @dev Override _update to track governance activity
     */
    function _update(address from, address to, uint256 value) internal override {
        super._update(from, to, value);
        
        // Update governance activity on transfers
        if (from != address(0) && to != address(0)) {
            // Decay reputation over time for inactive users
            _decayReputation(from);
            _decayReputation(to);
        }
    }
    
    /**
     * @dev Decay reputation over time for inactive users
     */
    function _decayReputation(address voter) internal {
        if (!verifiedVoters[voter]) return;
        
        uint256 daysSinceLastVote = (block.timestamp - lastVoteTimestamp[voter]) / 1 days;
        if (daysSinceLastVote > 90) { // 3 months of inactivity
            uint256 decay = daysSinceLastVote / 30; // Decay per month
            if (reputationScore[voter] > decay) {
                reputationScore[voter] -= decay;
            } else {
                reputationScore[voter] = 100; // Minimum reputation
            }
        }
    }
}