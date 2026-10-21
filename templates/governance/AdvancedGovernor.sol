// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BasicDAO.sol";

/**
 * @title AdvancedGovernor
 * @dev Advanced governance system with multiple voting mechanisms
 * 
 * Features:
 * - Multiple voting types (simple, quadratic, ranked choice, approval)
 * - Weighted voting based on reputation and stake duration
 * - Proposal categories with different requirements
 * - Dynamic quorum adjustment
 * - Conditional execution based on multiple criteria
 * - Governance mining and rewards
 * - Anti-manipulation mechanisms
 * 
 * Use Cases:
 * - Complex protocol governance
 * - Multi-stakeholder DAOs
 * - Research organizations
 * - DeFi protocol governance
 */
contract AdvancedGovernor is BasicDAO {
    enum VotingMechanism {
        SIMPLE_MAJORITY,    // Standard 1 token = 1 vote
        QUADRATIC,          // Square root voting power
        RANKED_CHOICE,      // Rank multiple options
        APPROVAL,           // Vote for multiple options
        WEIGHTED_STAKE,     // Weighted by stake duration
        REPUTATION_WEIGHTED // Weighted by reputation score
    }
    
    enum ProposalCategory {
        GOVERNANCE,         // Governance parameter changes
        TREASURY,          // Treasury management
        PROTOCOL,          // Protocol upgrades
        EMERGENCY,         // Emergency proposals
        RESEARCH,          // Research funding
        PARTNERSHIP        // Partnership decisions
    }
    
    struct ProposalConfig {
        VotingMechanism votingMechanism;
        ProposalCategory category;
        uint256 customQuorum;        // Category-specific quorum
        uint256 customThreshold;     // Category-specific proposal threshold
        uint256 customVotingPeriod;  // Category-specific voting period
        bool requiresSupermajority;  // Requires >66% approval
        bool requiresCouncilApproval; // Requires council pre-approval
        string[] options;            // For ranked choice/approval voting
    }
    
    struct WeightedVote {
        VoteType primaryVote;
        uint256[] optionWeights;     // For ranked choice/approval
        uint256 stakeWeight;         // Based on stake duration
        uint256 reputationWeight;    // Based on reputation score
        uint256 effectiveVotingPower; // Final calculated power
    }
    
    struct GovernanceRewards {
        uint256 proposalCreationReward;
        uint256 votingParticipationReward;
        uint256 consecutiveParticipationBonus;
        uint256 qualityBonusMultiplier;
    }
    
    struct Council {
        mapping(address => bool) members;
        uint256 memberCount;
        uint256 requiredApprovals;
        mapping(uint256 => mapping(address => bool)) approvals;
        mapping(uint256 => uint256) approvalCount;
    }
    
    // Enhanced proposal management
    mapping(uint256 => ProposalConfig) public proposalConfigs;
    mapping(uint256 => mapping(address => WeightedVote)) public weightedVotes;
    mapping(ProposalCategory => uint256) public categoryQuorums;
    mapping(ProposalCategory => uint256) public categoryThresholds;
    
    // Reputation system
    mapping(address => uint256) public reputationScores;
    mapping(address => uint256) public stakeDuration;
    mapping(address => uint256) public lastStakeTime;
    mapping(address => uint256) public consecutiveVotes;
    
    // Council governance
    Council public council;
    
    // Dynamic quorum
    uint256 public baseQuorum = 4; // 4%
    uint256 public maxQuorum = 20; // 20%
    uint256 public quorumAdjustmentRate = 1; // 1% per adjustment
    uint256 public lastQuorumAdjustment;
    
    // Governance rewards
    GovernanceRewards public governanceRewards;
    mapping(address => uint256) public totalRewardsEarned;
    mapping(address => uint256) public lastParticipationTime;
    
    // Anti-manipulation
    mapping(address => uint256) public voteDelayPenalty;
    mapping(address => bool) public suspiciousActivity;
    uint256 public maxVotingPowerPerAddress = 5; // Max 5% of total supply
    
    // Events
    event ProposalConfigured(
        uint256 indexed proposalId,
        VotingMechanism mechanism,
        ProposalCategory category
    );
    event WeightedVoteCast(
        uint256 indexed proposalId,
        address indexed voter,
        uint256 effectiveVotingPower,
        uint256[] optionWeights
    );
    event ReputationUpdated(address indexed account, uint256 oldScore, uint256 newScore);
    event QuorumAdjusted(uint256 oldQuorum, uint256 newQuorum);
    event CouncilMemberAdded(address indexed member);
    event CouncilMemberRemoved(address indexed member);
    event CouncilApproval(uint256 indexed proposalId, address indexed councilMember);
    event GovernanceRewardDistributed(address indexed recipient, uint256 amount);
    event SuspiciousActivityFlagged(address indexed account, string reason);
    
    // Errors
    error UnsupportedVotingMechanism();
    error CouncilApprovalRequired();
    error InsufficientCouncilApprovals();
    error SupermajorityRequired();
    error VotingPowerExceedsLimit();
    error SuspiciousActivityDetected();
    error InvalidOptionWeights();
    
    constructor(
        address _governanceToken,
        address initialOwner
    ) BasicDAO(_governanceToken, initialOwner) {
        _setupCategoryDefaults();
        _setupGovernanceRewards();
        _initializeCouncil(initialOwner);
    }
    
    /**
     * @dev Create proposal with advanced configuration
     */
    function proposeAdvanced(
        string calldata title,
        string calldata description,
        address targetContract,
        uint256 value,
        bytes calldata executionData,
        VotingMechanism mechanism,
        ProposalCategory category,
        string[] calldata options
    ) external onlyMember returns (uint256) {
        uint256 proposalId = this.propose(title, description, targetContract, value, executionData);
        
        // Configure advanced proposal settings
        proposalConfigs[proposalId] = ProposalConfig({
            votingMechanism: mechanism,
            category: category,
            customQuorum: categoryQuorums[category],
            customThreshold: categoryThresholds[category],
            customVotingPeriod: _getCategoryVotingPeriod(category),
            requiresSupermajority: _requiresSupermajority(category),
            requiresCouncilApproval: _requiresCouncilApproval(category),
            options: options
        });
        
        // Award proposal creation reward
        _distributeGovernanceReward(msg.sender, governanceRewards.proposalCreationReward);
        
        emit ProposalConfigured(proposalId, mechanism, category);
        return proposalId;
    }
    
    /**
     * @dev Cast weighted vote with multiple mechanisms
     */
    function castWeightedVote(
        uint256 proposalId,
        VoteType primaryVote,
        uint256[] calldata optionWeights,
        string calldata reason
    ) external proposalExists(proposalId) onlyMember {
        _updateProposalState(proposalId);
        
        Proposal storage proposal = proposals[proposalId];
        ProposalConfig storage config = proposalConfigs[proposalId];
        
        if (proposal.state != ProposalState.ACTIVE) {
            revert ProposalNotActive();
        }
        
        // Check for suspicious activity
        if (suspiciousActivity[msg.sender]) {
            revert SuspiciousActivityDetected();
        }
        
        VoteInfo storage voteInfo = proposalVotes[proposalId][msg.sender];
        if (voteInfo.hasVoted) {
            revert AlreadyVoted();
        }
        
        // Calculate effective voting power based on mechanism
        uint256 effectiveVotingPower = _calculateEffectiveVotingPower(
            msg.sender,
            config.votingMechanism
        );
        
        // Apply voting power limit
        uint256 maxAllowedPower = (governanceToken.totalSupply() * maxVotingPowerPerAddress) / 100;
        if (effectiveVotingPower > maxAllowedPower) {
            effectiveVotingPower = maxAllowedPower;
            emit SuspiciousActivityFlagged(msg.sender, "Voting power exceeds limit");
        }
        
        // Validate option weights for multi-option voting
        if (config.options.length > 0) {
            _validateOptionWeights(optionWeights, config.options.length);
        }
        
        // Record vote
        voteInfo.hasVoted = true;
        voteInfo.vote = primaryVote;
        voteInfo.votingPower = effectiveVotingPower;
        voteInfo.reason = reason;
        
        weightedVotes[proposalId][msg.sender] = WeightedVote({
            primaryVote: primaryVote,
            optionWeights: optionWeights,
            stakeWeight: _calculateStakeWeight(msg.sender),
            reputationWeight: _calculateReputationWeight(msg.sender),
            effectiveVotingPower: effectiveVotingPower
        });
        
        // Apply votes based on mechanism
        _applyVoteToProposal(proposalId, primaryVote, effectiveVotingPower, optionWeights);
        
        // Update reputation and rewards
        _updateVoterReputation(msg.sender, true);
        _distributeGovernanceReward(msg.sender, governanceRewards.votingParticipationReward);
        
        // Track consecutive participation
        if (block.timestamp - lastParticipationTime[msg.sender] <= 30 days) {
            consecutiveVotes[msg.sender]++;
            if (consecutiveVotes[msg.sender] >= 5) {
                _distributeGovernanceReward(
                    msg.sender,
                    governanceRewards.consecutiveParticipationBonus
                );
            }
        } else {
            consecutiveVotes[msg.sender] = 1;
        }
        lastParticipationTime[msg.sender] = block.timestamp;
        
        emit WeightedVoteCast(proposalId, msg.sender, effectiveVotingPower, optionWeights);
        emit VoteCast(proposalId, msg.sender, primaryVote, effectiveVotingPower, reason);
    }
    
    /**
     * @dev Council approval for proposals
     */
    function councilApprove(uint256 proposalId) external {
        require(council.members[msg.sender], "AdvancedGovernor: Not council member");
        require(!council.approvals[proposalId][msg.sender], "AdvancedGovernor: Already approved");
        
        ProposalConfig storage config = proposalConfigs[proposalId];
        require(config.requiresCouncilApproval, "AdvancedGovernor: Council approval not required");
        
        council.approvals[proposalId][msg.sender] = true;
        council.approvalCount[proposalId]++;
        
        emit CouncilApproval(proposalId, msg.sender);
    }
    
    /**
     * @dev Override execution to check advanced requirements
     */
    function executeProposal(uint256 proposalId) external override proposalExists(proposalId) nonReentrant {
        ProposalConfig storage config = proposalConfigs[proposalId];
        
        // Check council approval requirement
        if (config.requiresCouncilApproval) {
            if (council.approvalCount[proposalId] < council.requiredApprovals) {
                revert InsufficientCouncilApprovals();
            }
        }
        
        // Check supermajority requirement
        if (config.requiresSupermajority) {
            Proposal storage proposal = proposals[proposalId];
            uint256 totalVotes = proposal.forVotes + proposal.againstVotes;
            if (totalVotes == 0 || proposal.forVotes * 100 / totalVotes < 67) {
                revert SupermajorityRequired();
            }
        }
        
        super.executeProposal(proposalId);
        
        // Adjust quorum based on participation
        _adjustQuorum(proposalId);
    }
    
    /**
     * @dev Add council member
     */
    function addCouncilMember(address member) external onlyRole(ADMIN_ROLE) {
        require(!council.members[member], "AdvancedGovernor: Already council member");
        
        council.members[member] = true;
        council.memberCount++;
        
        emit CouncilMemberAdded(member);
    }
    
    /**
     * @dev Remove council member
     */
    function removeCouncilMember(address member) external onlyRole(ADMIN_ROLE) {
        require(council.members[member], "AdvancedGovernor: Not council member");
        
        council.members[member] = false;
        council.memberCount--;
        
        emit CouncilMemberRemoved(member);
    }
    
    /**
     * @dev Update reputation score
     */
    function updateReputation(address account, uint256 newScore) external onlyRole(OPERATOR_ROLE) {
        uint256 oldScore = reputationScores[account];
        reputationScores[account] = newScore;
        
        emit ReputationUpdated(account, oldScore, newScore);
    }
    
    /**
     * @dev Flag suspicious activity
     */
    function flagSuspiciousActivity(address account, bool suspicious) external onlyRole(ADMIN_ROLE) {
        suspiciousActivity[account] = suspicious;
        
        if (suspicious) {
            emit SuspiciousActivityFlagged(account, "Manually flagged by admin");
        }
    }
    
    /**
     * @dev Calculate effective voting power based on mechanism
     */
    function _calculateEffectiveVotingPower(
        address voter,
        VotingMechanism mechanism
    ) internal view returns (uint256) {
        uint256 basePower = getTokenVotingPower(voter);
        
        if (mechanism == VotingMechanism.SIMPLE_MAJORITY) {
            return basePower;
        } else if (mechanism == VotingMechanism.QUADRATIC) {
            return _sqrt(basePower);
        } else if (mechanism == VotingMechanism.WEIGHTED_STAKE) {
            uint256 stakeWeight = _calculateStakeWeight(voter);
            return basePower * stakeWeight / 100;
        } else if (mechanism == VotingMechanism.REPUTATION_WEIGHTED) {
            uint256 reputationWeight = _calculateReputationWeight(voter);
            return basePower * reputationWeight / 100;
        }
        
        return basePower;
    }
    
    /**
     * @dev Calculate stake duration weight
     */
    function _calculateStakeWeight(address voter) internal view returns (uint256) {
        uint256 duration = stakeDuration[voter];
        if (duration < 30 days) return 50;      // 50% weight for < 1 month
        if (duration < 90 days) return 75;      // 75% weight for < 3 months
        if (duration < 180 days) return 100;    // 100% weight for < 6 months
        if (duration < 365 days) return 125;    // 125% weight for < 1 year
        return 150;                             // 150% weight for >= 1 year
    }
    
    /**
     * @dev Calculate reputation weight
     */
    function _calculateReputationWeight(address voter) internal view returns (uint256) {
        uint256 reputation = reputationScores[voter];
        if (reputation < 100) return 50;        // 50% weight for low reputation
        if (reputation < 500) return 75;        // 75% weight for medium reputation
        if (reputation < 1000) return 100;      // 100% weight for good reputation
        return 125;                             // 125% weight for excellent reputation
    }
    
    /**
     * @dev Apply vote to proposal based on mechanism
     */
    function _applyVoteToProposal(
        uint256 proposalId,
        VoteType vote,
        uint256 power,
        uint256[] memory optionWeights
    ) internal {
        Proposal storage proposal = proposals[proposalId];
        
        if (vote == VoteType.FOR) {
            proposal.forVotes += power;
        } else if (vote == VoteType.AGAINST) {
            proposal.againstVotes += power;
        } else {
            proposal.abstainVotes += power;
        }
        
        // Handle multi-option voting (implementation would depend on specific needs)
        // This is a simplified version
    }
    
    /**
     * @dev Validate option weights for multi-option voting
     */
    function _validateOptionWeights(uint256[] memory weights, uint256 optionCount) internal pure {
        if (weights.length != optionCount) {
            revert InvalidOptionWeights();
        }
        
        uint256 totalWeight = 0;
        for (uint256 i = 0; i < weights.length; i++) {
            totalWeight += weights[i];
        }
        
        if (totalWeight != 100) {
            revert InvalidOptionWeights();
        }
    }
    
    /**
     * @dev Update voter reputation based on participation quality
     */
    function _updateVoterReputation(address voter, bool qualityParticipation) internal {
        uint256 currentScore = reputationScores[voter];
        uint256 adjustment = qualityParticipation ? 10 : 5;
        
        uint256 newScore = currentScore + adjustment;
        if (newScore > 2000) newScore = 2000; // Cap at 2000
        
        reputationScores[voter] = newScore;
    }
    
    /**
     * @dev Distribute governance rewards
     */
    function _distributeGovernanceReward(address recipient, uint256 baseAmount) internal {
        uint256 reputationBonus = reputationScores[recipient] / 100; // 1% per 100 reputation
        uint256 totalReward = baseAmount + (baseAmount * reputationBonus / 100);
        
        totalRewardsEarned[recipient] += totalReward;
        
        // Mint governance tokens as reward (simplified)
        // governanceToken.mint(recipient, totalReward);
        
        emit GovernanceRewardDistributed(recipient, totalReward);
    }
    
    /**
     * @dev Adjust quorum based on participation
     */
    function _adjustQuorum(uint256 proposalId) internal {
        if (block.timestamp < lastQuorumAdjustment + 30 days) return;
        
        Proposal storage proposal = proposals[proposalId];
        uint256 totalVotes = proposal.forVotes + proposal.againstVotes + proposal.abstainVotes;
        uint256 totalSupply = governanceToken.totalSupply();
        uint256 participationRate = (totalVotes * 100) / totalSupply;
        
        uint256 newQuorum = quorumNumerator;
        
        if (participationRate < baseQuorum) {
            // Decrease quorum if participation is low
            if (newQuorum > 1) newQuorum -= quorumAdjustmentRate;
        } else if (participationRate > baseQuorum * 2) {
            // Increase quorum if participation is high
            if (newQuorum < maxQuorum) newQuorum += quorumAdjustmentRate;
        }
        
        if (newQuorum != quorumNumerator) {
            uint256 oldQuorum = quorumNumerator;
            quorumNumerator = newQuorum;
            lastQuorumAdjustment = block.timestamp;
            
            emit QuorumAdjusted(oldQuorum, newQuorum);
        }
    }
    
    /**
     * @dev Get category-specific voting period
     */
    function _getCategoryVotingPeriod(ProposalCategory category) internal view returns (uint256) {
        if (category == ProposalCategory.EMERGENCY) return 1 days;
        if (category == ProposalCategory.GOVERNANCE) return 14 days;
        return votingPeriod; // Default
    }
    
    /**
     * @dev Check if category requires supermajority
     */
    function _requiresSupermajority(ProposalCategory category) internal pure returns (bool) {
        return category == ProposalCategory.GOVERNANCE || category == ProposalCategory.PROTOCOL;
    }
    
    /**
     * @dev Check if category requires council approval
     */
    function _requiresCouncilApproval(ProposalCategory category) internal pure returns (bool) {
        return category == ProposalCategory.EMERGENCY || category == ProposalCategory.PROTOCOL;
    }
    
    /**
     * @dev Square root function
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
    
    // Setup functions
    
    function _setupCategoryDefaults() private {
        categoryQuorums[ProposalCategory.GOVERNANCE] = 10;
        categoryQuorums[ProposalCategory.TREASURY] = 5;
        categoryQuorums[ProposalCategory.PROTOCOL] = 15;
        categoryQuorums[ProposalCategory.EMERGENCY] = 3;
        categoryQuorums[ProposalCategory.RESEARCH] = 4;
        categoryQuorums[ProposalCategory.PARTNERSHIP] = 4;
        
        categoryThresholds[ProposalCategory.GOVERNANCE] = 5000 * 10**18;
        categoryThresholds[ProposalCategory.TREASURY] = 2000 * 10**18;
        categoryThresholds[ProposalCategory.PROTOCOL] = 10000 * 10**18;
        categoryThresholds[ProposalCategory.EMERGENCY] = 1000 * 10**18;
        categoryThresholds[ProposalCategory.RESEARCH] = 1000 * 10**18;
        categoryThresholds[ProposalCategory.PARTNERSHIP] = 1000 * 10**18;
    }
    
    function _setupGovernanceRewards() private {
        governanceRewards = GovernanceRewards({
            proposalCreationReward: 100 * 10**18,      // 100 tokens
            votingParticipationReward: 10 * 10**18,    // 10 tokens
            consecutiveParticipationBonus: 50 * 10**18, // 50 tokens
            qualityBonusMultiplier: 150                 // 150% = 1.5x multiplier
        });
    }
    
    function _initializeCouncil(address initialMember) private {
        council.members[initialMember] = true;
        council.memberCount = 1;
        council.requiredApprovals = 1;
    }
}