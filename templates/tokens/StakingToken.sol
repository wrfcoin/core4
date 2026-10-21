// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BaseERC20Token.sol";

/**
 * @title StakingToken
 * @dev Token with built-in staking mechanisms
 * 
 * Features:
 * - Multiple staking pools with different lock periods
 * - Automatic reward distribution
 * - Slashing for misbehavior
 * - Delegation support
 * - Compound staking rewards
 * - Emergency unstaking with penalties
 * 
 * Use Cases:
 * - Validator staking
 * - Liquidity provision staking
 * - Governance staking
 * - Long-term token locking
 */
contract StakingToken is BaseERC20Token {
    struct StakingPool {
        uint256 lockPeriod;          // Minimum lock period in seconds
        uint256 rewardRate;          // Annual reward rate in basis points
        uint256 totalStaked;         // Total tokens staked in this pool
        uint256 minStakeAmount;      // Minimum stake amount
        uint256 maxStakeAmount;      // Maximum stake amount (0 = no limit)
        bool active;                 // Whether pool is accepting new stakes
        uint256 slashingRate;        // Slashing rate for early withdrawal (basis points)
    }
    
    struct Stake {
        uint256 amount;              // Amount staked
        uint256 startTime;           // When stake was created
        uint256 poolId;              // Which pool this stake belongs to
        uint256 rewardDebt;          // Reward debt for accurate reward calculation
        uint256 lastRewardTime;      // Last time rewards were claimed
        address delegatedTo;         // Address stake is delegated to (if any)
        bool active;                 // Whether stake is active
    }
    
    struct DelegationInfo {
        uint256 totalDelegated;      // Total amount delegated to this address
        uint256 delegatorCount;      // Number of delegators
        uint256 commissionRate;      // Commission rate in basis points
        bool acceptingDelegations;   // Whether accepting new delegations
    }
    
    // Staking data
    mapping(uint256 => StakingPool) public stakingPools;
    mapping(address => mapping(uint256 => Stake)) public userStakes; // user => stakeId => stake
    mapping(address => uint256) public userStakeCount;
    mapping(address => DelegationInfo) public delegationInfo;
    mapping(address => uint256) public totalStakedByUser;
    
    uint256 public nextPoolId = 1;
    uint256 public totalStaked;
    uint256 public rewardPool; // Pool of tokens reserved for staking rewards
    
    // Slashing
    address public slashingAuthority;
    mapping(address => bool) public slashingReporters;
    uint256 public constant MAX_SLASHING_RATE = 5000; // Max 50%
    
    // Events
    event Staked(address indexed user, uint256 indexed poolId, uint256 indexed stakeId, uint256 amount);
    event Unstaked(address indexed user, uint256 indexed stakeId, uint256 amount, uint256 penalty);
    event RewardClaimed(address indexed user, uint256 indexed stakeId, uint256 reward);
    event StakingPoolCreated(uint256 indexed poolId, uint256 lockPeriod, uint256 rewardRate);
    event Delegated(address indexed delegator, address indexed validator, uint256 stakeId);
    event Slashed(address indexed user, uint256 indexed stakeId, uint256 amount, string reason);
    
    // Errors
    error InsufficientStakeAmount(uint256 provided, uint256 minimum);
    error ExcessiveStakeAmount(uint256 provided, uint256 maximum);
    error StakeStillLocked(uint256 timeLeft);
    error InactiveStakingPool(uint256 poolId);
    error UnauthorizedSlashing(address caller);
    error InvalidCommissionRate(uint256 rate);
    error StakeNotFound(address user, uint256 stakeId);
    
    constructor(
        string memory name,
        string memory symbol,
        uint8 decimals,
        uint256 maxSupply_,
        address initialOwner
    ) BaseERC20Token(name, symbol, decimals, maxSupply_, initialOwner) {
        slashingAuthority = initialOwner;
        
        // Create default staking pools
        _createStakingPool(30 days, 500, 100 * 10**decimals, 0, 500);    // 30-day, 5% APY
        _createStakingPool(90 days, 800, 100 * 10**decimals, 0, 300);    // 90-day, 8% APY
        _createStakingPool(180 days, 1200, 100 * 10**decimals, 0, 200);  // 180-day, 12% APY
        _createStakingPool(365 days, 1800, 100 * 10**decimals, 0, 100);  // 365-day, 18% APY
    }
    
    /**
     * @dev Create a new staking pool
     */
    function createStakingPool(
        uint256 lockPeriod,
        uint256 rewardRate,
        uint256 minStakeAmount,
        uint256 maxStakeAmount,
        uint256 slashingRate
    ) external onlyRole(ADMIN_ROLE) returns (uint256 poolId) {
        return _createStakingPool(lockPeriod, rewardRate, minStakeAmount, maxStakeAmount, slashingRate);
    }
    
    /**
     * @dev Stake tokens in a specific pool
     */
    function stake(uint256 poolId, uint256 amount) external nonReentrant whenNotPaused returns (uint256 stakeId) {
        StakingPool storage pool = stakingPools[poolId];
        if (!pool.active) {
            revert InactiveStakingPool(poolId);
        }
        
        if (amount < pool.minStakeAmount) {
            revert InsufficientStakeAmount(amount, pool.minStakeAmount);
        }
        
        if (pool.maxStakeAmount > 0 && amount > pool.maxStakeAmount) {
            revert ExcessiveStakeAmount(amount, pool.maxStakeAmount);
        }
        
        // Transfer tokens from user
        _transfer(msg.sender, address(this), amount);
        
        // Create stake
        stakeId = userStakeCount[msg.sender]++;
        userStakes[msg.sender][stakeId] = Stake({
            amount: amount,
            startTime: block.timestamp,
            poolId: poolId,
            rewardDebt: 0,
            lastRewardTime: block.timestamp,
            delegatedTo: address(0),
            active: true
        });
        
        // Update totals
        pool.totalStaked += amount;
        totalStaked += amount;
        totalStakedByUser[msg.sender] += amount;
        
        emit Staked(msg.sender, poolId, stakeId, amount);
        return stakeId;
    }
    
    /**
     * @dev Unstake tokens from a specific stake
     */
    function unstake(uint256 stakeId) external nonReentrant {
        Stake storage userStake = userStakes[msg.sender][stakeId];
        if (!userStake.active) {
            revert StakeNotFound(msg.sender, stakeId);
        }
        
        StakingPool storage pool = stakingPools[userStake.poolId];
        uint256 amount = userStake.amount;
        
        // Check if lock period has passed
        uint256 penalty = 0;
        if (block.timestamp < userStake.startTime + pool.lockPeriod) {
            // Early withdrawal penalty
            penalty = (amount * pool.slashingRate) / 10000;
            amount -= penalty;
            
            if (penalty > 0) {
                // Burn penalty tokens or send to treasury
                _transfer(address(this), owner(), penalty);
            }
        }
        
        // Claim any pending rewards first
        _claimStakeRewards(userStake, msg.sender, stakeId);
        
        // Update state
        userStake.active = false;
        pool.totalStaked -= userStake.amount;
        totalStaked -= userStake.amount;
        totalStakedByUser[msg.sender] -= userStake.amount;
        
        // Transfer tokens back to user
        _transfer(address(this), msg.sender, amount);
        
        emit Unstaked(msg.sender, stakeId, amount, penalty);
    }
    
    /**
     * @dev Claim rewards for a specific stake
     */
    function claimStakeRewards(uint256 stakeId) external nonReentrant {
        Stake storage userStake = userStakes[msg.sender][stakeId];
        if (!userStake.active) {
            revert StakeNotFound(msg.sender, stakeId);
        }
        
        _claimStakeRewards(userStake, msg.sender, stakeId);
    }
    
    /**
     * @dev Delegate stake to a validator
     */
    function delegateStake(uint256 stakeId, address validator) external {
        Stake storage userStake = userStakes[msg.sender][stakeId];
        if (!userStake.active) {
            revert StakeNotFound(msg.sender, stakeId);
        }
        
        DelegationInfo storage validatorInfo = delegationInfo[validator];
        require(validatorInfo.acceptingDelegations, "StakingToken: Validator not accepting delegations");
        
        // Update delegation
        if (userStake.delegatedTo != address(0)) {
            // Remove from previous validator
            delegationInfo[userStake.delegatedTo].totalDelegated -= userStake.amount;
            delegationInfo[userStake.delegatedTo].delegatorCount--;
        }
        
        userStake.delegatedTo = validator;
        validatorInfo.totalDelegated += userStake.amount;
        validatorInfo.delegatorCount++;
        
        emit Delegated(msg.sender, validator, stakeId);
    }
    
    /**
     * @dev Set up validator for delegation
     */
    function setupValidator(uint256 commissionRate, bool acceptDelegations) external {
        if (commissionRate > 2000) { // Max 20% commission
            revert InvalidCommissionRate(commissionRate);
        }
        
        delegationInfo[msg.sender] = DelegationInfo({
            totalDelegated: delegationInfo[msg.sender].totalDelegated,
            delegatorCount: delegationInfo[msg.sender].delegatorCount,
            commissionRate: commissionRate,
            acceptingDelegations: acceptDelegations
        });
    }
    
    /**
     * @dev Slash a stake for misbehavior
     */
    function slash(address user, uint256 stakeId, uint256 slashAmount, string calldata reason) external {
        require(
            msg.sender == slashingAuthority || slashingReporters[msg.sender],
            "StakingToken: Unauthorized slashing"
        );
        
        Stake storage userStake = userStakes[user][stakeId];
        if (!userStake.active) {
            revert StakeNotFound(user, stakeId);
        }
        
        require(slashAmount <= userStake.amount, "StakingToken: Slash amount exceeds stake");
        
        // Update stake amount
        userStake.amount -= slashAmount;
        
        // Update totals
        StakingPool storage pool = stakingPools[userStake.poolId];
        pool.totalStaked -= slashAmount;
        totalStaked -= slashAmount;
        totalStakedByUser[user] -= slashAmount;
        
        // Burn or redistribute slashed tokens
        _transfer(address(this), owner(), slashAmount);
        
        emit Slashed(user, stakeId, slashAmount, reason);
    }
    
    /**
     * @dev Fund the reward pool
     */
    function fundRewardPool(uint256 amount) external onlyRole(ADMIN_ROLE) {
        _transfer(msg.sender, address(this), amount);
        rewardPool += amount;
    }
    
    /**
     * @dev Get calculated rewards for a stake
     */
    function getStakeRewards(address user, uint256 stakeId) external view returns (uint256) {
        Stake storage userStake = userStakes[user][stakeId];
        if (!userStake.active) return 0;
        
        StakingPool storage pool = stakingPools[userStake.poolId];
        
        uint256 stakingDuration = block.timestamp - userStake.lastRewardTime;
        uint256 annualReward = (userStake.amount * pool.rewardRate) / 10000;
        uint256 reward = (annualReward * stakingDuration) / 365 days;
        
        return reward;
    }
    
    /**
     * @dev Get user's total staked amount across all stakes
     */
    function getUserTotalStaked(address user) external view returns (uint256) {
        return totalStakedByUser[user];
    }
    
    /**
     * @dev Get staking pool information
     */
    function getPoolInfo(uint256 poolId) external view returns (StakingPool memory) {
        return stakingPools[poolId];
    }
    
    /**
     * @dev Internal function to create staking pool
     */
    function _createStakingPool(
        uint256 lockPeriod,
        uint256 rewardRate,
        uint256 minStakeAmount,
        uint256 maxStakeAmount,
        uint256 slashingRate
    ) internal returns (uint256 poolId) {
        require(rewardRate <= 5000, "StakingToken: Reward rate too high"); // Max 50% APY
        require(slashingRate <= MAX_SLASHING_RATE, "StakingToken: Slashing rate too high");
        
        poolId = nextPoolId++;
        stakingPools[poolId] = StakingPool({
            lockPeriod: lockPeriod,
            rewardRate: rewardRate,
            totalStaked: 0,
            minStakeAmount: minStakeAmount,
            maxStakeAmount: maxStakeAmount,
            active: true,
            slashingRate: slashingRate
        });
        
        emit StakingPoolCreated(poolId, lockPeriod, rewardRate);
        return poolId;
    }
    
    /**
     * @dev Internal function to claim stake rewards
     */
    function _claimStakeRewards(Stake storage userStake, address user, uint256 stakeId) internal {
        StakingPool storage pool = stakingPools[userStake.poolId];
        
        uint256 stakingDuration = block.timestamp - userStake.lastRewardTime;
        uint256 annualReward = (userStake.amount * pool.rewardRate) / 10000;
        uint256 reward = (annualReward * stakingDuration) / 365 days;
        
        if (reward > 0 && reward <= rewardPool) {
            // Handle delegation commission
            if (userStake.delegatedTo != address(0)) {
                DelegationInfo storage validatorInfo = delegationInfo[userStake.delegatedTo];
                uint256 commission = (reward * validatorInfo.commissionRate) / 10000;
                if (commission > 0) {
                    _transfer(address(this), userStake.delegatedTo, commission);
                    reward -= commission;
                }
            }
            
            // Transfer reward to user
            _transfer(address(this), user, reward);
            rewardPool -= (reward + (userStake.delegatedTo != address(0) ? 
                (reward * delegationInfo[userStake.delegatedTo].commissionRate) / 10000 : 0));
            
            userStake.lastRewardTime = block.timestamp;
            userStake.rewardDebt += reward;
            
            emit RewardClaimed(user, stakeId, reward);
        }
    }
    
    /**
     * @dev Set slashing authority
     */
    function setSlashingAuthority(address newAuthority) external onlyRole(ADMIN_ROLE) {
        slashingAuthority = newAuthority;
    }
    
    /**
     * @dev Add slashing reporter
     */
    function addSlashingReporter(address reporter) external onlyRole(ADMIN_ROLE) {
        slashingReporters[reporter] = true;
    }
    
    /**
     * @dev Remove slashing reporter
     */
    function removeSlashingReporter(address reporter) external onlyRole(ADMIN_ROLE) {
        slashingReporters[reporter] = false;
    }
}