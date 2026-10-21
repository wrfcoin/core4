// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../tokens/BaseERC20Token.sol";

/**
 * @title LiquidityPool
 * @dev Liquidity provision and rewards system
 * 
 * Features:
 * - Single and dual token liquidity provision
 * - Time-based rewards calculation
 * - Impermanent loss protection
 * - Flexible reward token support
 * - Staking multipliers and bonuses
 * - Emergency withdrawal mechanisms
 * 
 * Use Cases:
 * - DEX liquidity mining
 * - Yield farming protocols
 * - Token incentive programs
 * - Climate project funding pools
 */
contract LiquidityPool is SecurityBase {
    struct PoolInfo {
        BaseERC20Token stakingToken;     // Token to be staked
        BaseERC20Token rewardToken;      // Token given as reward
        uint256 rewardPerSecond;         // Reward tokens per second
        uint256 lastRewardTime;          // Last time rewards were calculated
        uint256 accRewardPerShare;       // Accumulated rewards per share
        uint256 totalStaked;             // Total tokens staked in pool
        uint256 minStakingAmount;        // Minimum staking amount
        uint256 maxStakingAmount;        // Maximum staking amount (0 = no limit)
        uint256 lockupPeriod;            // Minimum lockup period in seconds
        bool active;                     // Whether pool is active
    }
    
    struct UserInfo {
        uint256 amount;                  // Amount of tokens staked
        uint256 rewardDebt;              // Reward debt for accurate calculation
        uint256 pendingRewards;          // Pending reward tokens
        uint256 stakingTime;             // When user started staking
        uint256 lastHarvestTime;         // Last time rewards were harvested
        uint256 totalHarvested;          // Total rewards harvested
        uint256 multiplier;              // Staking multiplier (in basis points)
    }
    
    struct RewardPeriod {
        uint256 startTime;
        uint256 endTime;
        uint256 rewardRate;
        uint256 totalRewards;
        bool active;
    }
    
    // Pool management
    PoolInfo[] public poolInfo;
    mapping(uint256 => mapping(address => UserInfo)) public userInfo;
    mapping(uint256 => RewardPeriod[]) public rewardPeriods;
    
    // Multipliers and bonuses
    mapping(address => uint256) public userMultipliers; // User-specific multipliers
    mapping(uint256 => uint256) public timeMultipliers; // Time-based multipliers
    uint256[] public timeThresholds; // Time thresholds for multipliers
    
    // Impermanent loss protection
    mapping(uint256 => mapping(address => uint256)) public entryPrices;
    mapping(uint256 => uint256) public protectionRate; // Basis points of protection
    bool public impermanentLossProtectionEnabled;
    
    // Emergency features
    mapping(uint256 => bool) public emergencyWithdrawEnabled;
    mapping(uint256 => uint256) public emergencyWithdrawPenalty; // Basis points
    
    // Fee structure
    mapping(uint256 => uint256) public depositFees; // Basis points
    mapping(uint256 => uint256) public withdrawFees; // Basis points
    address public feeRecipient;
    
    // Events
    event PoolAdded(uint256 indexed poolId, address stakingToken, address rewardToken, uint256 rewardPerSecond);
    event Deposit(address indexed user, uint256 indexed poolId, uint256 amount);
    event Withdraw(address indexed user, uint256 indexed poolId, uint256 amount);
    event RewardHarvested(address indexed user, uint256 indexed poolId, uint256 amount);
    event MultiplierUpdated(address indexed user, uint256 oldMultiplier, uint256 newMultiplier);
    event RewardPeriodAdded(uint256 indexed poolId, uint256 startTime, uint256 endTime, uint256 rewardRate);
    event EmergencyWithdraw(address indexed user, uint256 indexed poolId, uint256 amount, uint256 penalty);
    
    // Errors
    error PoolNotActive(uint256 poolId);
    error InsufficientStakeAmount(uint256 provided, uint256 minimum);
    error ExcessiveStakeAmount(uint256 provided, uint256 maximum);
    error StillInLockupPeriod(uint256 timeLeft);
    error InsufficientStakedAmount(uint256 available, uint256 requested);
    error InvalidPoolId(uint256 poolId);
    error NoRewardsToClaim(uint256 poolId);
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        feeRecipient = initialOwner;
        
        // Initialize default time multipliers
        timeThresholds.push(7 days);    // 1 week
        timeThresholds.push(30 days);   // 1 month
        timeThresholds.push(90 days);   // 3 months
        timeThresholds.push(365 days);  // 1 year
        
        timeMultipliers[7 days] = 11000;    // 110% (10% bonus)
        timeMultipliers[30 days] = 12500;   // 125% (25% bonus)
        timeMultipliers[90 days] = 15000;   // 150% (50% bonus)
        timeMultipliers[365 days] = 20000;  // 200% (100% bonus)
    }
    
    /**
     * @dev Add a new liquidity pool
     */
    function addPool(
        BaseERC20Token stakingToken,
        BaseERC20Token rewardToken,
        uint256 rewardPerSecond,
        uint256 minStakingAmount,
        uint256 maxStakingAmount,
        uint256 lockupPeriod
    ) external onlyRole(ADMIN_ROLE) returns (uint256 poolId) {
        poolId = poolInfo.length;
        
        poolInfo.push(PoolInfo({
            stakingToken: stakingToken,
            rewardToken: rewardToken,
            rewardPerSecond: rewardPerSecond,
            lastRewardTime: block.timestamp,
            accRewardPerShare: 0,
            totalStaked: 0,
            minStakingAmount: minStakingAmount,
            maxStakingAmount: maxStakingAmount,
            lockupPeriod: lockupPeriod,
            active: true
        }));
        
        emit PoolAdded(poolId, address(stakingToken), address(rewardToken), rewardPerSecond);
        return poolId;
    }
    
    /**
     * @dev Deposit tokens into liquidity pool
     */
    function deposit(uint256 poolId, uint256 amount) external nonReentrant whenNotPaused {
        if (poolId >= poolInfo.length) {
            revert InvalidPoolId(poolId);
        }
        
        PoolInfo storage pool = poolInfo[poolId];
        if (!pool.active) {
            revert PoolNotActive(poolId);
        }
        
        if (amount < pool.minStakingAmount) {
            revert InsufficientStakeAmount(amount, pool.minStakingAmount);
        }
        
        if (pool.maxStakingAmount > 0 && amount > pool.maxStakingAmount) {
            revert ExcessiveStakeAmount(amount, pool.maxStakingAmount);
        }
        
        UserInfo storage user = userInfo[poolId][msg.sender];
        
        // Update pool rewards
        _updatePool(poolId);
        
        // If user already has stake, harvest pending rewards
        if (user.amount > 0) {
            _harvestRewards(poolId, msg.sender);
        }
        
        // Calculate deposit fee
        uint256 depositFee = (amount * depositFees[poolId]) / 10000;
        uint256 actualAmount = amount - depositFee;
        
        // Transfer tokens
        pool.stakingToken.transferFrom(msg.sender, address(this), amount);
        if (depositFee > 0) {
            pool.stakingToken.transfer(feeRecipient, depositFee);
        }
        
        // Update user info
        user.amount += actualAmount;
        user.stakingTime = block.timestamp;
        user.rewardDebt = (user.amount * pool.accRewardPerShare) / 1e18;
        
        // Update pool total
        pool.totalStaked += actualAmount;
        
        // Record entry price for impermanent loss protection
        if (impermanentLossProtectionEnabled) {
            entryPrices[poolId][msg.sender] = _getCurrentPrice(poolId);
        }
        
        emit Deposit(msg.sender, poolId, actualAmount);
    }
    
    /**
     * @dev Withdraw tokens from liquidity pool
     */
    function withdraw(uint256 poolId, uint256 amount) external nonReentrant {
        if (poolId >= poolInfo.length) {
            revert InvalidPoolId(poolId);
        }
        
        PoolInfo storage pool = poolInfo[poolId];
        UserInfo storage user = userInfo[poolId][msg.sender];
        
        if (amount > user.amount) {
            revert InsufficientStakedAmount(user.amount, amount);
        }
        
        // Check lockup period
        if (block.timestamp < user.stakingTime + pool.lockupPeriod) {
            revert StillInLockupPeriod(
                user.stakingTime + pool.lockupPeriod - block.timestamp
            );
        }
        
        // Update pool rewards
        _updatePool(poolId);
        
        // Harvest pending rewards
        _harvestRewards(poolId, msg.sender);
        
        // Calculate withdraw fee
        uint256 withdrawFee = (amount * withdrawFees[poolId]) / 10000;
        uint256 actualAmount = amount - withdrawFee;
        
        // Update user info
        user.amount -= amount;
        user.rewardDebt = (user.amount * pool.accRewardPerShare) / 1e18;
        
        // Update pool total
        pool.totalStaked -= amount;
        
        // Transfer tokens
        pool.stakingToken.transfer(msg.sender, actualAmount);
        if (withdrawFee > 0) {
            pool.stakingToken.transfer(feeRecipient, withdrawFee);
        }
        
        // Handle impermanent loss protection
        if (impermanentLossProtectionEnabled && protectionRate[poolId] > 0) {
            _processImpermanentLossProtection(poolId, msg.sender, amount);
        }
        
        emit Withdraw(msg.sender, poolId, amount);
    }
    
    /**
     * @dev Emergency withdraw without rewards
     */
    function emergencyWithdraw(uint256 poolId) external nonReentrant {
        require(emergencyWithdrawEnabled[poolId], "LiquidityPool: Emergency withdraw not enabled");
        
        PoolInfo storage pool = poolInfo[poolId];
        UserInfo storage user = userInfo[poolId][msg.sender];
        
        uint256 amount = user.amount;
        require(amount > 0, "LiquidityPool: No stake to withdraw");
        
        // Calculate penalty
        uint256 penalty = (amount * emergencyWithdrawPenalty[poolId]) / 10000;
        uint256 withdrawAmount = amount - penalty;
        
        // Reset user info
        user.amount = 0;
        user.rewardDebt = 0;
        user.pendingRewards = 0;
        
        // Update pool total
        pool.totalStaked -= amount;
        
        // Transfer tokens
        pool.stakingToken.transfer(msg.sender, withdrawAmount);
        if (penalty > 0) {
            pool.stakingToken.transfer(feeRecipient, penalty);
        }
        
        emit EmergencyWithdraw(msg.sender, poolId, withdrawAmount, penalty);
    }
    
    /**
     * @dev Harvest rewards from pool
     */
    function harvest(uint256 poolId) external nonReentrant {
        _updatePool(poolId);
        _harvestRewards(poolId, msg.sender);
    }
    
    /**
     * @dev Harvest rewards from multiple pools
     */
    function massHarvest(uint256[] calldata poolIds) external {
        for (uint256 i = 0; i < poolIds.length; i++) {
            _updatePool(poolIds[i]);
            _harvestRewards(poolIds[i], msg.sender);
        }
    }
    
    /**
     * @dev Update pool rewards
     */
    function _updatePool(uint256 poolId) internal {
        PoolInfo storage pool = poolInfo[poolId];
        
        if (block.timestamp <= pool.lastRewardTime) {
            return;
        }
        
        if (pool.totalStaked == 0) {
            pool.lastRewardTime = block.timestamp;
            return;
        }
        
        uint256 timeElapsed = block.timestamp - pool.lastRewardTime;
        uint256 reward = timeElapsed * pool.rewardPerSecond;
        
        pool.accRewardPerShare += (reward * 1e18) / pool.totalStaked;
        pool.lastRewardTime = block.timestamp;
    }
    
    /**
     * @dev Harvest rewards for user
     */
    function _harvestRewards(uint256 poolId, address userAddress) internal {
        PoolInfo storage pool = poolInfo[poolId];
        UserInfo storage user = userInfo[poolId][userAddress];
        
        if (user.amount == 0) {
            return;
        }
        
        uint256 pending = ((user.amount * pool.accRewardPerShare) / 1e18) - user.rewardDebt;
        
        if (pending > 0) {
            // Apply time-based multiplier
            uint256 stakingDuration = block.timestamp - user.stakingTime;
            uint256 multiplier = _getTimeMultiplier(stakingDuration);
            
            // Apply user-specific multiplier
            if (userMultipliers[userAddress] > 0) {
                multiplier = (multiplier * userMultipliers[userAddress]) / 10000;
            }
            
            pending = (pending * multiplier) / 10000;
            
            // Update user info
            user.pendingRewards += pending;
            user.totalHarvested += pending;
            user.lastHarvestTime = block.timestamp;
            
            // Transfer reward tokens
            pool.rewardToken.transfer(userAddress, pending);
            
            emit RewardHarvested(userAddress, poolId, pending);
        }
        
        user.rewardDebt = (user.amount * pool.accRewardPerShare) / 1e18;
    }
    
    /**
     * @dev Get time-based multiplier
     */
    function _getTimeMultiplier(uint256 stakingDuration) internal view returns (uint256) {
        uint256 multiplier = 10000; // 100% base
        
        for (uint256 i = timeThresholds.length; i > 0; i--) {
            if (stakingDuration >= timeThresholds[i - 1]) {
                return timeMultipliers[timeThresholds[i - 1]];
            }
        }
        
        return multiplier;
    }
    
    /**
     * @dev Process impermanent loss protection
     */
    function _processImpermanentLossProtection(uint256 poolId, address user, uint256 amount) internal {
        uint256 entryPrice = entryPrices[poolId][user];
        uint256 currentPrice = _getCurrentPrice(poolId);
        
        if (currentPrice < entryPrice) {
            uint256 priceDiff = entryPrice - currentPrice;
            uint256 lossPercentage = (priceDiff * 10000) / entryPrice;
            uint256 protectionAmount = (amount * lossPercentage * protectionRate[poolId]) / (10000 * 10000);
            
            if (protectionAmount > 0) {
                // Mint protection tokens or transfer from insurance fund
                poolInfo[poolId].rewardToken.transfer(user, protectionAmount);
            }
        }
    }
    
    /**
     * @dev Get current price for impermanent loss calculation
     */
    function _getCurrentPrice(uint256 poolId) internal view returns (uint256) {
        // Simple implementation - should integrate with price oracle
        return 1e18; // Placeholder
    }
    
    /**
     * @dev Get pending rewards for user
     */
    function pendingReward(uint256 poolId, address userAddress) external view returns (uint256) {
        PoolInfo storage pool = poolInfo[poolId];
        UserInfo storage user = userInfo[poolId][userAddress];
        
        uint256 accRewardPerShare = pool.accRewardPerShare;
        
        if (block.timestamp > pool.lastRewardTime && pool.totalStaked != 0) {
            uint256 timeElapsed = block.timestamp - pool.lastRewardTime;
            uint256 reward = timeElapsed * pool.rewardPerSecond;
            accRewardPerShare += (reward * 1e18) / pool.totalStaked;
        }
        
        uint256 pending = ((user.amount * accRewardPerShare) / 1e18) - user.rewardDebt;
        
        // Apply multipliers
        uint256 stakingDuration = block.timestamp - user.stakingTime;
        uint256 multiplier = _getTimeMultiplier(stakingDuration);
        
        if (userMultipliers[userAddress] > 0) {
            multiplier = (multiplier * userMultipliers[userAddress]) / 10000;
        }
        
        return (pending * multiplier) / 10000;
    }
    
    /**
     * @dev Get user info for pool
     */
    function getUserInfo(uint256 poolId, address userAddress) external view returns (
        uint256 amount,
        uint256 rewardDebt,
        uint256 pendingRewards,
        uint256 stakingTime,
        uint256 totalHarvested
    ) {
        UserInfo storage user = userInfo[poolId][userAddress];
        return (
            user.amount,
            user.rewardDebt,
            user.pendingRewards,
            user.stakingTime,
            user.totalHarvested
        );
    }
    
    /**
     * @dev Set user multiplier
     */
    function setUserMultiplier(address user, uint256 multiplier) external onlyRole(ADMIN_ROLE) {
        require(multiplier <= 30000, "LiquidityPool: Multiplier too high"); // Max 300%
        uint256 oldMultiplier = userMultipliers[user];
        userMultipliers[user] = multiplier;
        emit MultiplierUpdated(user, oldMultiplier, multiplier);
    }
    
    /**
     * @dev Set pool reward rate
     */
    function setRewardRate(uint256 poolId, uint256 rewardPerSecond) external onlyRole(ADMIN_ROLE) {
        _updatePool(poolId);
        poolInfo[poolId].rewardPerSecond = rewardPerSecond;
    }
    
    /**
     * @dev Set pool active status
     */
    function setPoolActive(uint256 poolId, bool active) external onlyRole(ADMIN_ROLE) {
        poolInfo[poolId].active = active;
    }
    
    /**
     * @dev Set deposit fee
     */
    function setDepositFee(uint256 poolId, uint256 feeRate) external onlyRole(ADMIN_ROLE) {
        require(feeRate <= 1000, "LiquidityPool: Fee too high"); // Max 10%
        depositFees[poolId] = feeRate;
    }
    
    /**
     * @dev Set withdraw fee
     */
    function setWithdrawFee(uint256 poolId, uint256 feeRate) external onlyRole(ADMIN_ROLE) {
        require(feeRate <= 1000, "LiquidityPool: Fee too high"); // Max 10%
        withdrawFees[poolId] = feeRate;
    }
    
    /**
     * @dev Enable/disable emergency withdraw
     */
    function setEmergencyWithdrawEnabled(uint256 poolId, bool enabled, uint256 penaltyRate) external onlyRole(ADMIN_ROLE) {
        require(penaltyRate <= 2000, "LiquidityPool: Penalty too high"); // Max 20%
        emergencyWithdrawEnabled[poolId] = enabled;
        emergencyWithdrawPenalty[poolId] = penaltyRate;
    }
    
    /**
     * @dev Set impermanent loss protection
     */
    function setImpermanentLossProtection(bool enabled, uint256 poolId, uint256 rate) external onlyRole(ADMIN_ROLE) {
        require(rate <= 10000, "LiquidityPool: Invalid protection rate");
        impermanentLossProtectionEnabled = enabled;
        protectionRate[poolId] = rate;
    }
    
    /**
     * @dev Get pool count
     */
    function poolLength() external view returns (uint256) {
        return poolInfo.length;
    }
}