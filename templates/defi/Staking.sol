// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";
import "../tokens/BaseERC20Token.sol";

/**
 * @title Staking
 * @dev General purpose staking contract with rewards distribution
 * 
 * Features:
 * - Multi-tier staking with different lock periods
 * - Flexible reward distribution mechanisms
 * - Compound staking support
 * - Penalty system for early withdrawal
 * - Delegation and voting power tracking
 * - Automated reward calculation
 * 
 * Use Cases:
 * - Protocol governance staking
 * - Validator staking for consensus
 * - Yield farming and liquidity mining
 * - Long-term token locking incentives
 */
contract Staking is SecurityBase {
    struct StakingTier {
        uint256 minAmount;           // Minimum stake amount
        uint256 maxAmount;           // Maximum stake amount (0 = no limit)
        uint256 lockPeriod;          // Lock period in seconds
        uint256 rewardMultiplier;    // Reward multiplier in basis points
        uint256 earlyWithdrawPenalty; // Penalty for early withdrawal (basis points)
        bool active;                 // Whether this tier is active
        string name;                 // Tier name (e.g., "Bronze", "Silver", "Gold")
    }
    
    struct Stake {
        uint256 amount;              // Amount staked
        uint256 tierId;              // Staking tier ID
        uint256 startTime;           // When stake was created
        uint256 lastRewardTime;      // Last time rewards were calculated
        uint256 accumulatedRewards;  // Total rewards earned but not claimed
        uint256 rewardDebt;          // Reward debt for accurate calculation
        bool compound;               // Whether to compound rewards
        address delegatedTo;         // Address stake is delegated to
    }
    
    struct UserStaking {
        Stake[] stakes;              // User's stakes
        uint256 totalStaked;         // Total amount staked across all stakes
        uint256 totalRewardsClaimed; // Total rewards claimed
        uint256 votingPower;         // Current voting power
        uint256 delegatedVotingPower; // Voting power delegated to this user
    }
    
    struct RewardPool {
        BaseERC20Token rewardToken;  // Token used for rewards
        uint256 rewardRate;          // Rewards per second
        uint256 totalRewardsDistributed; // Total rewards distributed
        uint256 lastUpdateTime;      // Last time pool was updated
        bool active;                 // Whether pool is active
    }
    
    // Core staking data
    BaseERC20Token public stakingToken;
    mapping(address => UserStaking) public userStaking;
    mapping(uint256 => StakingTier) public stakingTiers;
    mapping(uint256 => RewardPool) public rewardPools;
    
    uint256 public nextTierId = 1;
    uint256 public nextPoolId = 1;
    uint256 public totalStaked;
    uint256 public totalStakers;
    
    // Delegation tracking
    mapping(address => address[]) public delegators; // delegatee => delegator addresses
    mapping(address => uint256) public delegatedPower; // total voting power delegated to address
    
    // Reward calculation
    uint256 public accRewardPerShare; // Accumulated rewards per share
    uint256 public constant PRECISION = 1e18;
    
    // Slashing
    mapping(address => bool) public slashingAuthorities;
    mapping(address => uint256) public slashedAmounts;
    uint256 public totalSlashed;
    
    // Events
    event Staked(address indexed user, uint256 indexed stakeId, uint256 amount, uint256 tierId);
    event Unstaked(address indexed user, uint256 indexed stakeId, uint256 amount, uint256 penalty);
    event RewardsClaimed(address indexed user, uint256 amount);
    event RewardsCompounded(address indexed user, uint256 amount);
    event StakingTierAdded(uint256 indexed tierId, string name, uint256 minAmount, uint256 lockPeriod);
    event Delegated(address indexed delegator, address indexed delegatee, uint256 votingPower);
    event Slashed(address indexed user, uint256 amount, string reason);
    event RewardPoolAdded(uint256 indexed poolId, address rewardToken, uint256 rewardRate);
    
    // Errors
    error InvalidTier(uint256 tierId);
    error InsufficientAmount(uint256 provided, uint256 required);
    error ExcessiveAmount(uint256 provided, uint256 maximum);
    error StakeStillLocked(uint256 timeLeft);
    error InvalidStakeId(uint256 stakeId);
    error NoRewardsToClaim();
    error UnauthorizedSlashing(address caller);
    error TierNotActive(uint256 tierId);
    
    constructor(
        BaseERC20Token _stakingToken,
        address initialOwner
    ) SecurityBase(initialOwner) {
        stakingToken = _stakingToken;
        
        // Add default staking tiers
        _addStakingTier("Flexible", 100 * 10**18, 0, 0, 10000, 500); // No lock, base rewards, 5% penalty
        _addStakingTier("Bronze", 1000 * 10**18, 0, 30 days, 12000, 1000); // 30 days, 20% bonus, 10% penalty
        _addStakingTier("Silver", 5000 * 10**18, 0, 90 days, 15000, 1500); // 90 days, 50% bonus, 15% penalty
        _addStakingTier("Gold", 10000 * 10**18, 0, 180 days, 20000, 2000); // 180 days, 100% bonus, 20% penalty
        _addStakingTier("Platinum", 50000 * 10**18, 0, 365 days, 30000, 2500); // 365 days, 200% bonus, 25% penalty
    }
    
    /**
     * @dev Stake tokens in a specific tier
     */
    function stake(uint256 amount, uint256 tierId, bool compound) external nonReentrant whenNotPaused returns (uint256 stakeId) {
        StakingTier storage tier = stakingTiers[tierId];
        if (!tier.active) {
            revert TierNotActive(tierId);
        }
        
        if (amount < tier.minAmount) {
            revert InsufficientAmount(amount, tier.minAmount);
        }
        
        if (tier.maxAmount > 0 && amount > tier.maxAmount) {
            revert ExcessiveAmount(amount, tier.maxAmount);
        }
        
        // Transfer tokens from user
        stakingToken.transferFrom(msg.sender, address(this), amount);
        
        // Update global state
        _updateRewardPool(0); // Update default reward pool
        
        UserStaking storage userStakingData = userStaking[msg.sender];
        
        // Create new stake
        stakeId = userStakingData.stakes.length;
        userStakingData.stakes.push(Stake({
            amount: amount,
            tierId: tierId,
            startTime: block.timestamp,
            lastRewardTime: block.timestamp,
            accumulatedRewards: 0,
            rewardDebt: (amount * accRewardPerShare) / PRECISION,
            compound: compound,
            delegatedTo: address(0)
        }));
        
        // Update user totals
        if (userStakingData.totalStaked == 0) {
            totalStakers++;
        }
        userStakingData.totalStaked += amount;
        totalStaked += amount;
        
        // Update voting power
        _updateVotingPower(msg.sender);
        
        emit Staked(msg.sender, stakeId, amount, tierId);
        return stakeId;
    }
    
    /**
     * @dev Unstake tokens from a specific stake
     */
    function unstake(uint256 stakeId) external nonReentrant returns (uint256 amount, uint256 penalty) {
        UserStaking storage userStakingData = userStaking[msg.sender];
        
        if (stakeId >= userStakingData.stakes.length) {
            revert InvalidStakeId(stakeId);
        }
        
        Stake storage userStake = userStakingData.stakes[stakeId];
        StakingTier storage tier = stakingTiers[userStake.tierId];
        
        amount = userStake.amount;
        require(amount > 0, "Staking: Stake already withdrawn");
        
        // Check if lock period has passed
        penalty = 0;
        if (block.timestamp < userStake.startTime + tier.lockPeriod) {
            penalty = (amount * tier.earlyWithdrawPenalty) / 10000;
            
            if (penalty > 0) {
                // Transfer penalty to fee recipient or burn
                stakingToken.transfer(owner(), penalty);
            }
        }
        
        // Update rewards before unstaking
        _updateStakeRewards(msg.sender, stakeId);
        
        // If compounding is enabled, add accumulated rewards to stake
        if (userStake.compound && userStake.accumulatedRewards > 0) {
            _compoundRewards(msg.sender, stakeId);
        }
        
        // Remove stake
        userStake.amount = 0;
        
        // Update user totals
        userStakingData.totalStaked -= amount;
        totalStaked -= amount;
        
        if (userStakingData.totalStaked == 0) {
            totalStakers--;
        }
        
        // Update voting power
        _updateVotingPower(msg.sender);
        
        // Transfer tokens back to user
        uint256 withdrawAmount = amount - penalty;
        stakingToken.transfer(msg.sender, withdrawAmount);
        
        emit Unstaked(msg.sender, stakeId, withdrawAmount, penalty);
        return (withdrawAmount, penalty);
    }
    
    /**
     * @dev Claim rewards from a specific stake
     */
    function claimRewards(uint256 stakeId) external nonReentrant returns (uint256 rewards) {
        UserStaking storage userStakingData = userStaking[msg.sender];
        
        if (stakeId >= userStakingData.stakes.length) {
            revert InvalidStakeId(stakeId);
        }
        
        // Update stake rewards
        _updateStakeRewards(msg.sender, stakeId);
        
        Stake storage userStake = userStakingData.stakes[stakeId];
        rewards = userStake.accumulatedRewards;
        
        if (rewards == 0) {
            revert NoRewardsToClaim();
        }
        
        // Reset accumulated rewards
        userStake.accumulatedRewards = 0;
        userStakingData.totalRewardsClaimed += rewards;
        
        // Transfer reward tokens (for now, mint staking tokens as rewards)
        stakingToken.transfer(msg.sender, rewards);
        
        emit RewardsClaimed(msg.sender, rewards);
        return rewards;
    }
    
    /**
     * @dev Claim rewards from all stakes
     */
    function claimAllRewards() external returns (uint256 totalRewards) {
        UserStaking storage userStakingData = userStaking[msg.sender];
        
        for (uint256 i = 0; i < userStakingData.stakes.length; i++) {
            if (userStakingData.stakes[i].amount > 0) {
                try this.claimRewards(i) returns (uint256 rewards) {
                    totalRewards += rewards;
                } catch {
                    // Skip stakes with no rewards
                }
            }
        }
        
        return totalRewards;
    }
    
    /**
     * @dev Compound rewards back into stake
     */
    function compoundRewards(uint256 stakeId) external nonReentrant {
        _updateStakeRewards(msg.sender, stakeId);
        _compoundRewards(msg.sender, stakeId);
    }
    
    /**
     * @dev Delegate voting power to another address
     */
    function delegate(address delegatee, uint256 stakeId) external {
        UserStaking storage userStakingData = userStaking[msg.sender];
        
        if (stakeId >= userStakingData.stakes.length) {
            revert InvalidStakeId(stakeId);
        }
        
        Stake storage userStake = userStakingData.stakes[stakeId];
        
        // Remove from previous delegatee
        if (userStake.delegatedTo != address(0)) {
            _removeDelegation(msg.sender, userStake.delegatedTo, userStake.amount);
        }
        
        // Add to new delegatee
        userStake.delegatedTo = delegatee;
        if (delegatee != address(0)) {
            _addDelegation(msg.sender, delegatee, userStake.amount);
        }
        
        emit Delegated(msg.sender, delegatee, userStake.amount);
    }
    
    /**
     * @dev Slash a user's stake (admin only)
     */
    function slash(address user, uint256 amount, string calldata reason) external {
        require(slashingAuthorities[msg.sender] || hasRole(ADMIN_ROLE, msg.sender), "Staking: Unauthorized slashing");
        
        UserStaking storage userStakingData = userStaking[user];
        require(userStakingData.totalStaked >= amount, "Staking: Insufficient stake to slash");
        
        // Slash from user's stakes (FIFO)
        uint256 remainingSlash = amount;
        for (uint256 i = 0; i < userStakingData.stakes.length && remainingSlash > 0; i++) {
            Stake storage userStake = userStakingData.stakes[i];
            if (userStake.amount > 0) {
                uint256 slashAmount = remainingSlash > userStake.amount ? userStake.amount : remainingSlash;
                userStake.amount -= slashAmount;
                remainingSlash -= slashAmount;
            }
        }
        
        // Update totals
        userStakingData.totalStaked -= amount;
        totalStaked -= amount;
        slashedAmounts[user] += amount;
        totalSlashed += amount;
        
        // Update voting power
        _updateVotingPower(user);
        
        emit Slashed(user, amount, reason);
    }
    
    /**
     * @dev Get user's total pending rewards
     */
    function getPendingRewards(address user) external view returns (uint256 totalPending) {
        UserStaking storage userStakingData = userStaking[user];
        
        for (uint256 i = 0; i < userStakingData.stakes.length; i++) {
            if (userStakingData.stakes[i].amount > 0) {
                totalPending += _calculatePendingRewards(user, i);
            }
        }
        
        return totalPending;
    }
    
    /**
     * @dev Get user's stake information
     */
    function getUserStake(address user, uint256 stakeId) external view returns (
        uint256 amount,
        uint256 tierId,
        uint256 startTime,
        uint256 pendingRewards,
        bool compound,
        address delegatedTo
    ) {
        UserStaking storage userStakingData = userStaking[user];
        require(stakeId < userStakingData.stakes.length, "Staking: Invalid stake ID");
        
        Stake storage userStake = userStakingData.stakes[stakeId];
        
        return (
            userStake.amount,
            userStake.tierId,
            userStake.startTime,
            _calculatePendingRewards(user, stakeId),
            userStake.compound,
            userStake.delegatedTo
        );
    }
    
    /**
     * @dev Get user's staking summary
     */
    function getUserStakingSummary(address user) external view returns (
        uint256 totalStaked,
        uint256 totalRewardsClaimed,
        uint256 votingPower,
        uint256 totalPendingRewards,
        uint256 stakesCount
    ) {
        UserStaking storage userStakingData = userStaking[user];
        
        return (
            userStakingData.totalStaked,
            userStakingData.totalRewardsClaimed,
            userStakingData.votingPower,
            this.getPendingRewards(user),
            userStakingData.stakes.length
        );
    }
    
    /**
     * @dev Internal function to update stake rewards
     */
    function _updateStakeRewards(address user, uint256 stakeId) internal {
        UserStaking storage userStakingData = userStaking[user];
        Stake storage userStake = userStakingData.stakes[stakeId];
        
        if (userStake.amount == 0) return;
        
        uint256 pending = _calculatePendingRewards(user, stakeId);
        userStake.accumulatedRewards += pending;
        userStake.lastRewardTime = block.timestamp;
        userStake.rewardDebt = (userStake.amount * accRewardPerShare) / PRECISION;
    }
    
    /**
     * @dev Calculate pending rewards for a stake
     */
    function _calculatePendingRewards(address user, uint256 stakeId) internal view returns (uint256) {
        UserStaking storage userStakingData = userStaking[user];
        Stake storage userStake = userStakingData.stakes[stakeId];
        StakingTier storage tier = stakingTiers[userStake.tierId];
        
        if (userStake.amount == 0) return 0;
        
        uint256 timeDiff = block.timestamp - userStake.lastRewardTime;
        uint256 baseReward = (userStake.amount * timeDiff * 1e18) / (365 days * 100); // 1% base APR
        
        // Apply tier multiplier
        uint256 reward = (baseReward * tier.rewardMultiplier) / 10000;
        
        return reward;
    }
    
    /**
     * @dev Compound rewards into stake
     */
    function _compoundRewards(address user, uint256 stakeId) internal {
        UserStaking storage userStakingData = userStaking[user];
        Stake storage userStake = userStakingData.stakes[stakeId];
        
        uint256 rewards = userStake.accumulatedRewards;
        if (rewards > 0) {
            userStake.amount += rewards;
            userStake.accumulatedRewards = 0;
            
            // Update totals
            userStakingData.totalStaked += rewards;
            totalStaked += rewards;
            
            // Update voting power
            _updateVotingPower(user);
            
            emit RewardsCompounded(user, rewards);
        }
    }
    
    /**
     * @dev Update reward pool
     */
    function _updateRewardPool(uint256 poolId) internal {
        RewardPool storage pool = rewardPools[poolId];
        if (!pool.active || totalStaked == 0) return;
        
        uint256 timeDiff = block.timestamp - pool.lastUpdateTime;
        uint256 reward = timeDiff * pool.rewardRate;
        
        accRewardPerShare += (reward * PRECISION) / totalStaked;
        pool.lastUpdateTime = block.timestamp;
        pool.totalRewardsDistributed += reward;
    }
    
    /**
     * @dev Update user's voting power
     */
    function _updateVotingPower(address user) internal {
        UserStaking storage userStakingData = userStaking[user];
        
        uint256 newVotingPower = 0;
        for (uint256 i = 0; i < userStakingData.stakes.length; i++) {
            Stake storage userStake = userStakingData.stakes[i];
            if (userStake.amount > 0 && userStake.delegatedTo == address(0)) {
                StakingTier storage tier = stakingTiers[userStake.tierId];
                
                // Voting power increases with stake amount and time
                uint256 timeBonus = (block.timestamp - userStake.startTime) / 30 days; // 1% per month
                uint256 power = userStake.amount * (100 + timeBonus) / 100;
                power = (power * tier.rewardMultiplier) / 10000; // Apply tier multiplier
                
                newVotingPower += power;
            }
        }
        
        userStakingData.votingPower = newVotingPower;
    }
    
    /**
     * @dev Add delegation
     */
    function _addDelegation(address delegator, address delegatee, uint256 amount) internal {
        delegators[delegatee].push(delegator);
        delegatedPower[delegatee] += amount;
        userStaking[delegatee].delegatedVotingPower += amount;
    }
    
    /**
     * @dev Remove delegation
     */
    function _removeDelegation(address delegator, address delegatee, uint256 amount) internal {
        // Remove from delegators array
        address[] storage dels = delegators[delegatee];
        for (uint256 i = 0; i < dels.length; i++) {
            if (dels[i] == delegator) {
                dels[i] = dels[dels.length - 1];
                dels.pop();
                break;
            }
        }
        
        delegatedPower[delegatee] -= amount;
        userStaking[delegatee].delegatedVotingPower -= amount;
    }
    
    /**
     * @dev Add staking tier
     */
    function addStakingTier(
        string calldata name,
        uint256 minAmount,
        uint256 maxAmount,
        uint256 lockPeriod,
        uint256 rewardMultiplier,
        uint256 earlyWithdrawPenalty
    ) external onlyRole(ADMIN_ROLE) returns (uint256 tierId) {
        return _addStakingTier(name, minAmount, maxAmount, lockPeriod, rewardMultiplier, earlyWithdrawPenalty);
    }
    
    /**
     * @dev Internal function to add staking tier
     */
    function _addStakingTier(
        string memory name,
        uint256 minAmount,
        uint256 maxAmount,
        uint256 lockPeriod,
        uint256 rewardMultiplier,
        uint256 earlyWithdrawPenalty
    ) internal returns (uint256 tierId) {
        tierId = nextTierId++;
        
        stakingTiers[tierId] = StakingTier({
            minAmount: minAmount,
            maxAmount: maxAmount,
            lockPeriod: lockPeriod,
            rewardMultiplier: rewardMultiplier,
            earlyWithdrawPenalty: earlyWithdrawPenalty,
            active: true,
            name: name
        });
        
        emit StakingTierAdded(tierId, name, minAmount, lockPeriod);
        return tierId;
    }
    
    /**
     * @dev Add slashing authority
     */
    function addSlashingAuthority(address authority) external onlyRole(ADMIN_ROLE) {
        slashingAuthorities[authority] = true;
    }
    
    /**
     * @dev Remove slashing authority
     */
    function removeSlashingAuthority(address authority) external onlyRole(ADMIN_ROLE) {
        slashingAuthorities[authority] = false;
    }
    
    /**
     * @dev Set tier active status
     */
    function setTierActive(uint256 tierId, bool active) external onlyRole(ADMIN_ROLE) {
        stakingTiers[tierId].active = active;
    }
}