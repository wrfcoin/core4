// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BaseERC20Token.sol";

/**
 * @title RewardToken
 * @dev Token with minting rewards for specific activities
 * 
 * Features:
 * - Activity-based minting rewards
 * - Configurable reward rates and activities
 * - Time-based reward decay
 * - Anti-gaming mechanisms
 * - Staking integration for reward boosts
 * 
 * Use Cases:
 * - Weather station operator rewards
 * - Carbon credit verification rewards
 * - Governance participation rewards
 * - Computing resource contribution rewards
 */
contract RewardToken is BaseERC20Token {
    // Reward activities
    enum RewardActivity {
        WEATHER_REPORTING,
        CARBON_VERIFICATION,
        GOVERNANCE_PARTICIPATION,
        COMPUTING_CONTRIBUTION,
        DATA_VALIDATION,
        STAKING
    }
    
    struct RewardConfig {
        uint256 baseReward;      // Base reward amount
        uint256 decayRate;       // Reward decay per period (basis points)
        uint256 maxRewardPerPeriod; // Maximum rewards per period
        uint256 cooldownPeriod;  // Cooldown between rewards
        bool active;             // Whether this reward is active
    }
    
    struct UserRewardState {
        uint256 lastClaimTime;
        uint256 totalClaimed;
        uint256 reputationScore;
        mapping(RewardActivity => uint256) activityCounts;
    }
    
    // Reward configuration
    mapping(RewardActivity => RewardConfig) public rewardConfigs;
    mapping(address => UserRewardState) public userRewards;
    
    // Reward modifiers and boosts
    mapping(address => uint256) public stakingBoosts; // Basis points boost from staking
    uint256 public globalRewardMultiplier = 10000; // 100% in basis points
    
    // Activity verification
    mapping(RewardActivity => mapping(address => bool)) public activityVerifiers;
    mapping(address => mapping(RewardActivity => uint256)) public pendingRewards;
    
    // Anti-gaming
    uint256 public constant MAX_REWARD_PER_TX = 1000 * 10**18; // 1000 tokens max per transaction
    uint256 public constant MIN_VERIFICATION_DELAY = 1 hours;
    
    // Events
    event RewardConfigured(RewardActivity indexed activity, uint256 baseReward, uint256 decayRate);
    event RewardClaimed(address indexed user, RewardActivity indexed activity, uint256 amount);
    event ActivityVerified(address indexed user, RewardActivity indexed activity, address indexed verifier);
    event StakingBoostUpdated(address indexed user, uint256 newBoost);
    
    // Errors
    error RewardNotActive(RewardActivity activity);
    error InsufficientCooldown(uint256 timeLeft);
    error UnauthorizedVerifier(address verifier, RewardActivity activity);
    error ExcessiveRewardAmount(uint256 amount, uint256 limit);
    error NoRewardsPending(address user, RewardActivity activity);
    
    constructor(
        string memory name,
        string memory symbol,
        uint8 decimals,
        uint256 maxSupply_,
        address initialOwner
    ) BaseERC20Token(name, symbol, decimals, maxSupply_, initialOwner) {
        // Initialize default reward configurations
        _setRewardConfig(RewardActivity.WEATHER_REPORTING, 100 * 10**decimals, 100, 1000 * 10**decimals, 1 hours);
        _setRewardConfig(RewardActivity.CARBON_VERIFICATION, 50 * 10**decimals, 50, 500 * 10**decimals, 30 minutes);
        _setRewardConfig(RewardActivity.GOVERNANCE_PARTICIPATION, 25 * 10**decimals, 25, 250 * 10**decimals, 1 days);
        _setRewardConfig(RewardActivity.COMPUTING_CONTRIBUTION, 75 * 10**decimals, 75, 750 * 10**decimals, 2 hours);
        _setRewardConfig(RewardActivity.DATA_VALIDATION, 30 * 10**decimals, 30, 300 * 10**decimals, 45 minutes);
        _setRewardConfig(RewardActivity.STAKING, 10 * 10**decimals, 10, 100 * 10**decimals, 1 days);
    }
    
    /**
     * @dev Configure reward for specific activity
     */
    function setRewardConfig(
        RewardActivity activity,
        uint256 baseReward,
        uint256 decayRate,
        uint256 maxRewardPerPeriod,
        uint256 cooldownPeriod
    ) external onlyRole(ADMIN_ROLE) {
        _setRewardConfig(activity, baseReward, decayRate, maxRewardPerPeriod, cooldownPeriod);
    }
    
    /**
     * @dev Add activity verifier
     */
    function addActivityVerifier(RewardActivity activity, address verifier) external onlyRole(ADMIN_ROLE) {
        activityVerifiers[activity][verifier] = true;
    }
    
    /**
     * @dev Remove activity verifier
     */
    function removeActivityVerifier(RewardActivity activity, address verifier) external onlyRole(ADMIN_ROLE) {
        activityVerifiers[activity][verifier] = false;
    }
    
    /**
     * @dev Verify user activity (called by authorized verifiers)
     */
    function verifyActivity(address user, RewardActivity activity, uint256 amount) external {
        if (!activityVerifiers[activity][msg.sender]) {
            revert UnauthorizedVerifier(msg.sender, activity);
        }
        
        RewardConfig memory config = rewardConfigs[activity];
        if (!config.active) {
            revert RewardNotActive(activity);
        }
        
        // Calculate reward with decay and boosts
        uint256 reward = _calculateReward(user, activity, amount);
        
        // Update pending rewards
        pendingRewards[user][activity] += reward;
        
        emit ActivityVerified(user, activity, msg.sender);
    }
    
    /**
     * @dev Claim pending rewards for specific activity
     */
    function claimReward(RewardActivity activity) external nonReentrant whenNotPaused {
        UserRewardState storage userState = userRewards[msg.sender];
        
        // Check cooldown
        RewardConfig memory config = rewardConfigs[activity];
        if (block.timestamp < userState.lastClaimTime + config.cooldownPeriod) {
            revert InsufficientCooldown(
                userState.lastClaimTime + config.cooldownPeriod - block.timestamp
            );
        }
        
        uint256 reward = pendingRewards[msg.sender][activity];
        if (reward == 0) {
            revert NoRewardsPending(msg.sender, activity);
        }
        
        // Anti-gaming check
        if (reward > MAX_REWARD_PER_TX) {
            revert ExcessiveRewardAmount(reward, MAX_REWARD_PER_TX);
        }
        
        // Clear pending rewards
        pendingRewards[msg.sender][activity] = 0;
        
        // Update user state
        userState.lastClaimTime = block.timestamp;
        userState.totalClaimed += reward;
        userState.activityCounts[activity]++;
        
        // Mint reward tokens
        _mint(msg.sender, reward);
        
        emit RewardClaimed(msg.sender, activity, reward);
    }
    
    /**
     * @dev Claim all pending rewards
     */
    function claimAllRewards() external {
        for (uint i = 0; i < 6; i++) { // 6 reward activities
            RewardActivity activity = RewardActivity(i);
            if (pendingRewards[msg.sender][activity] > 0) {
                try this.claimReward(activity) {
                    // Reward claimed successfully
                } catch {
                    // Skip this reward (likely due to cooldown)
                }
            }
        }
    }
    
    /**
     * @dev Update staking boost for user
     */
    function updateStakingBoost(address user, uint256 boostBasisPoints) external onlyRole(OPERATOR_ROLE) {
        require(boostBasisPoints <= 5000, "RewardToken: Max 50% boost"); // Max 50% boost
        stakingBoosts[user] = boostBasisPoints;
        emit StakingBoostUpdated(user, boostBasisPoints);
    }
    
    /**
     * @dev Set global reward multiplier
     */
    function setGlobalRewardMultiplier(uint256 multiplierBasisPoints) external onlyRole(ADMIN_ROLE) {
        require(multiplierBasisPoints <= 20000, "RewardToken: Max 200% multiplier");
        globalRewardMultiplier = multiplierBasisPoints;
    }
    
    /**
     * @dev Get pending rewards for user and activity
     */
    function getPendingReward(address user, RewardActivity activity) external view returns (uint256) {
        return pendingRewards[user][activity];
    }
    
    /**
     * @dev Get total pending rewards for user
     */
    function getTotalPendingRewards(address user) external view returns (uint256 total) {
        for (uint i = 0; i < 6; i++) {
            total += pendingRewards[user][RewardActivity(i)];
        }
    }
    
    /**
     * @dev Get user reputation score
     */
    function getUserReputation(address user) external view returns (uint256) {
        return userRewards[user].reputationScore;
    }
    
    /**
     * @dev Internal function to set reward configuration
     */
    function _setRewardConfig(
        RewardActivity activity,
        uint256 baseReward,
        uint256 decayRate,
        uint256 maxRewardPerPeriod,
        uint256 cooldownPeriod
    ) internal {
        rewardConfigs[activity] = RewardConfig({
            baseReward: baseReward,
            decayRate: decayRate,
            maxRewardPerPeriod: maxRewardPerPeriod,
            cooldownPeriod: cooldownPeriod,
            active: true
        });
        
        emit RewardConfigured(activity, baseReward, decayRate);
    }
    
    /**
     * @dev Calculate reward with decay and boosts
     */
    function _calculateReward(address user, RewardActivity activity, uint256 baseAmount) internal view returns (uint256) {
        RewardConfig memory config = rewardConfigs[activity];
        UserRewardState storage userState = userRewards[user];
        
        // Start with base reward
        uint256 reward = config.baseReward;
        
        // Apply activity-specific amount if provided
        if (baseAmount > 0) {
            reward = (reward * baseAmount) / 10**decimals();
        }
        
        // Apply time-based decay (rewards decrease over time to encourage early adoption)
        uint256 timeSinceLastClaim = block.timestamp - userState.lastClaimTime;
        if (timeSinceLastClaim > 1 days) {
            uint256 decayPeriods = timeSinceLastClaim / 1 days;
            uint256 decayFactor = 10000 - (config.decayRate * decayPeriods);
            if (decayFactor < 5000) decayFactor = 5000; // Minimum 50% of base reward
            reward = (reward * decayFactor) / 10000;
        }
        
        // Apply reputation boost
        if (userState.reputationScore > 1000) {
            uint256 reputationBoost = (userState.reputationScore - 1000) / 100; // 1% per 100 reputation over 1000
            if (reputationBoost > 2000) reputationBoost = 2000; // Max 20% reputation boost
            reward = (reward * (10000 + reputationBoost)) / 10000;
        }
        
        // Apply staking boost
        uint256 stakingBoost = stakingBoosts[user];
        if (stakingBoost > 0) {
            reward = (reward * (10000 + stakingBoost)) / 10000;
        }
        
        // Apply global multiplier
        reward = (reward * globalRewardMultiplier) / 10000;
        
        // Ensure doesn't exceed maximum per period
        if (reward > config.maxRewardPerPeriod) {
            reward = config.maxRewardPerPeriod;
        }
        
        return reward;
    }
}