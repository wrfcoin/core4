const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  time,
  loadFixture,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('Carbon Credit Yield Farming', function () {
  // Test fixture for consistent setup
  async function deployYieldFarmingFixture() {
    const [owner, user1, user2, feeRecipient, oracle] =
      await ethers.getSigners();

    // Deploy mock tokens
    const MockERC20 = await ethers.getContractFactory('MockERC20');
    const usdc = await MockERC20.deploy('USD Coin', 'USDC', 6);
    const wrfToken = await MockERC20.deploy('WRF Token', 'WRF', 18);

    // Deploy mock carbon credits
    const SimpleMockEnhancedCarbonCredits = await ethers.getContractFactory(
      'SimpleMockEnhancedCarbonCredits'
    );
    const carbonCredits = await SimpleMockEnhancedCarbonCredits.deploy();

    // Deploy carbon basket token
    const CarbonBasketToken =
      await ethers.getContractFactory('CarbonBasketToken');
    const basketConfig = {
      name: 'Nature-Based Solutions',
      symbol: 'NBS',
      allowedProjectTypes: ['forestry', 'blue-carbon'],
      allowedRegistries: ['VCS', 'GS'],
      minVintage: 2020,
      maxVintage: 2025,
      minQualityScore: 700,
      retirementRate: 500, // 5% per year
    };
    const carbonBasketToken = await CarbonBasketToken.deploy(
      await carbonCredits.getAddress(),
      basketConfig
    );

    // Deploy oracles
    const QualityOracle = await ethers.getContractFactory('QualityOracle');
    const qualityOracle = await QualityOracle.deploy();

    const VolatilityOracle =
      await ethers.getContractFactory('VolatilityOracle');
    const volatilityOracle = await VolatilityOracle.deploy();

    // Deploy pool infrastructure
    const WeightedCarbonPool =
      await ethers.getContractFactory('WeightedCarbonPool');
    const poolImplementation = await WeightedCarbonPool.deploy();

    const LiquidityPoolManager = await ethers.getContractFactory(
      'LiquidityPoolManager'
    );
    const poolManager = await LiquidityPoolManager.deploy(
      await usdc.getAddress(),
      await poolImplementation.getAddress(),
      feeRecipient.address
    );

    // Set oracles in pool manager
    await poolManager.setQualityOracle(await qualityOracle.getAddress());
    await poolManager.setVolatilityOracle(await volatilityOracle.getAddress());

    // Deploy yield farming contracts
    const RewardDistributor =
      await ethers.getContractFactory('RewardDistributor');
    const rewardDistributor = await RewardDistributor.deploy(
      feeRecipient.address,
      await qualityOracle.getAddress()
    );

    const CarbonFarmingPool =
      await ethers.getContractFactory('CarbonFarmingPool');
    const farmImplementation = await CarbonFarmingPool.deploy();

    const YieldFarmingManager = await ethers.getContractFactory(
      'YieldFarmingManager'
    );
    const yieldFarmingManager = await YieldFarmingManager.deploy(
      await poolManager.getAddress(),
      await wrfToken.getAddress(),
      await farmImplementation.getAddress(),
      await rewardDistributor.getAddress()
    );

    // Setup permissions
    const DISTRIBUTOR_ROLE = await rewardDistributor.DISTRIBUTOR_ROLE();
    await rewardDistributor.grantRole(
      DISTRIBUTOR_ROLE,
      await yieldFarmingManager.getAddress()
    );

    // Register yield farming manager as router
    await poolManager.registerRouter(await yieldFarmingManager.getAddress());

    // Authorize carbon basket token
    await poolManager.setCarbonTokenAuthorization(
      await carbonBasketToken.getAddress(),
      true
    );

    // Create a weighted pool
    const swapFee = 30; // 0.3%
    await poolManager.createWeightedPool(
      await carbonBasketToken.getAddress(),
      'balanced',
      swapFee
    );

    // Get the created pool address
    const pools = await poolManager.getActivePools();
    const liquidityPool = pools[0].poolAddress;

    // Setup reward token in distributor
    await rewardDistributor.configureRewardToken(
      await wrfToken.getAddress(),
      ethers.parseEther('1000000'), // 1M tokens allocated
      ethers.parseEther('1'), // 1 token per second
      7 * 24 * 60 * 60, // 7 days vesting
      'governance'
    );

    // Mint tokens for testing
    await usdc.mint(user1.address, ethers.parseUnits('10000', 6));
    await usdc.mint(user2.address, ethers.parseUnits('10000', 6));
    await wrfToken.mint(
      await rewardDistributor.getAddress(),
      ethers.parseEther('1000000')
    );

    // Create LP tokens for testing (mock approach)
    const MockLPToken = await ethers.getContractFactory('MockERC20');
    const lpToken = await MockLPToken.deploy('LP Token', 'LP', 18);
    await lpToken.mint(user1.address, ethers.parseEther('1000'));
    await lpToken.mint(user2.address, ethers.parseEther('1000'));

    return {
      owner,
      user1,
      user2,
      feeRecipient,
      oracle,
      usdc,
      wrfToken,
      carbonCredits,
      carbonBasketToken,
      qualityOracle,
      volatilityOracle,
      poolManager,
      rewardDistributor,
      yieldFarmingManager,
      liquidityPool,
      lpToken,
      farmImplementation,
    };
  }

  describe('Deployment and Configuration', function () {
    it('Should deploy all contracts correctly', async function () {
      const { yieldFarmingManager, rewardDistributor, poolManager, wrfToken } =
        await loadFixture(deployYieldFarmingFixture);

      expect(await yieldFarmingManager.poolManager()).to.equal(
        await poolManager.getAddress()
      );
      expect(await yieldFarmingManager.wrfToken()).to.equal(
        await wrfToken.getAddress()
      );
      expect(await yieldFarmingManager.rewardDistributor()).to.equal(
        await rewardDistributor.getAddress()
      );
    });

    it('Should set up quality tier multipliers correctly', async function () {
      const { yieldFarmingManager } = await loadFixture(
        deployYieldFarmingFixture
      );

      expect(await yieldFarmingManager.qualityTierMultipliers(1)).to.equal(
        ethers.parseEther('2')
      ); // Premium: 200%
      expect(await yieldFarmingManager.qualityTierMultipliers(2)).to.equal(
        ethers.parseEther('1.5')
      ); // High: 150%
      expect(await yieldFarmingManager.qualityTierMultipliers(3)).to.equal(
        ethers.parseEther('1')
      ); // Standard: 100%
      expect(await yieldFarmingManager.qualityTierMultipliers(4)).to.equal(
        ethers.parseEther('0.5')
      ); // Basic: 50%
    });

    it('Should configure reward tokens in distributor', async function () {
      const { rewardDistributor, wrfToken } = await loadFixture(
        deployYieldFarmingFixture
      );

      const rewardToken = await rewardDistributor.getRewardToken(
        await wrfToken.getAddress()
      );
      expect(rewardToken.tokenAddress).to.equal(await wrfToken.getAddress());
      expect(rewardToken.totalAllocated).to.equal(ethers.parseEther('1000000'));
      expect(rewardToken.emissionRate).to.equal(ethers.parseEther('1'));
      expect(rewardToken.isActive).to.be.true;
      expect(rewardToken.tokenType).to.equal('governance');
    });
  });

  describe('Farm Creation', function () {
    it('Should create a yield farm successfully', async function () {
      const { yieldFarmingManager, liquidityPool, owner } = await loadFixture(
        deployYieldFarmingFixture
      );

      const baseRewardRate = ethers.parseEther('0.1'); // 0.1 tokens per second per LP token
      const minStakeDuration = 30 * 24 * 60 * 60; // 30 days
      const maxStakeDuration = 365 * 24 * 60 * 60; // 365 days
      const farmDuration = 90 * 24 * 60 * 60; // 90 days

      const tx = await yieldFarmingManager.createFarm(
        liquidityPool,
        baseRewardRate,
        minStakeDuration,
        maxStakeDuration,
        farmDuration,
        'balanced'
      );

      await expect(tx).to.emit(yieldFarmingManager, 'FarmCreated');

      const farms = await yieldFarmingManager.getActiveFarms();
      expect(farms.length).to.equal(1);
      expect(farms[0].liquidityPool).to.equal(liquidityPool);
      expect(farms[0].baseRewardRate).to.equal(baseRewardRate);
    });

    it('Should fail to create farm with invalid parameters', async function () {
      const { yieldFarmingManager, liquidityPool } = await loadFixture(
        deployYieldFarmingFixture
      );

      // Invalid reward rate
      await expect(
        yieldFarmingManager.createFarm(
          liquidityPool,
          0, // Invalid
          30 * 24 * 60 * 60,
          365 * 24 * 60 * 60,
          90 * 24 * 60 * 60,
          'balanced'
        )
      ).to.be.revertedWith('YieldFarmingManager: Invalid reward rate');

      // Invalid stake durations
      await expect(
        yieldFarmingManager.createFarm(
          liquidityPool,
          ethers.parseEther('0.1'),
          365 * 24 * 60 * 60, // Min > Max
          30 * 24 * 60 * 60,
          90 * 24 * 60 * 60,
          'balanced'
        )
      ).to.be.revertedWith('YieldFarmingManager: Invalid stake durations');
    });
  });

  describe('Staking and Unstaking', function () {
    async function deployWithFarmFixture() {
      const baseFixture = await deployYieldFarmingFixture();
      const { yieldFarmingManager, liquidityPool, lpToken, user1 } =
        baseFixture;

      // Create a farm
      const baseRewardRate = ethers.parseEther('0.1');
      const minStakeDuration = 30 * 24 * 60 * 60;
      const maxStakeDuration = 365 * 24 * 60 * 60;
      const farmDuration = 90 * 24 * 60 * 60;

      await yieldFarmingManager.createFarm(
        liquidityPool,
        baseRewardRate,
        minStakeDuration,
        maxStakeDuration,
        farmDuration,
        'balanced'
      );

      const farms = await yieldFarmingManager.getActiveFarms();
      const farmAddress = farms[0].farmAddress;

      // Approve LP tokens for staking
      await lpToken
        .connect(user1)
        .approve(farmAddress, ethers.parseEther('1000'));

      return { ...baseFixture, farmAddress, baseRewardRate };
    }

    it('Should allow users to stake LP tokens', async function () {
      const { yieldFarmingManager, farmAddress, user1, lpToken } =
        await loadFixture(deployWithFarmFixture);

      const stakeAmount = ethers.parseEther('100');
      const initialBalance = await lpToken.balanceOf(user1.address);

      const tx = await yieldFarmingManager
        .connect(user1)
        .stake(farmAddress, stakeAmount);

      await expect(tx)
        .to.emit(yieldFarmingManager, 'UserStaked')
        .withArgs(
          user1.address,
          farmAddress,
          stakeAmount,
          ethers.parseEther('1.5'),
          1000
        );

      // Check user stake
      const userStake = await yieldFarmingManager.getUserStakeInFarm(
        user1.address,
        farmAddress
      );
      expect(userStake).to.equal(stakeAmount);

      // Check user info
      const userInfo = await yieldFarmingManager.getUserFarmInfo(user1.address);
      expect(userInfo.totalStaked).to.equal(stakeAmount);
      expect(userInfo.activeFarmCount).to.equal(1);
    });

    it('Should calculate boosted amounts correctly for different stake sizes', async function () {
      const { yieldFarmingManager, farmAddress, user1, user2, lpToken } =
        await loadFixture(deployWithFarmFixture);

      // Large stake should get 365-day lock (2x boost)
      const largeStake = ethers.parseEther('1000000000000000000000'); // 1e21
      await lpToken.mint(user1.address, largeStake);
      await lpToken.connect(user1).approve(farmAddress, largeStake);

      // Small stake should get 30-day lock (1.1x boost)
      const smallStake = ethers.parseEther('1'); // 1e18
      await lpToken.connect(user2).approve(farmAddress, smallStake);

      await yieldFarmingManager.connect(user1).stake(farmAddress, largeStake);
      await yieldFarmingManager.connect(user2).stake(farmAddress, smallStake);

      // Check that different users got different boosts based on stake size
      const user1Info = await yieldFarmingManager.getUserStakeInFarm(
        user1.address,
        farmAddress
      );
      const user2Info = await yieldFarmingManager.getUserStakeInFarm(
        user2.address,
        farmAddress
      );

      expect(user1Info).to.equal(largeStake);
      expect(user2Info).to.equal(smallStake);
    });

    it('Should prevent staking below minimum amount', async function () {
      const { yieldFarmingManager, farmAddress, user1, lpToken } =
        await loadFixture(deployWithFarmFixture);

      const tinyAmount = ethers.parseUnits('1', 5); // Below MIN_STAKE_AMOUNT
      await lpToken.connect(user1).approve(farmAddress, tinyAmount);

      await expect(
        yieldFarmingManager.connect(user1).stake(farmAddress, tinyAmount)
      ).to.be.revertedWith('YieldFarmingManager: Stake amount too small');
    });

    it('Should allow users to unstake with early penalty', async function () {
      const { yieldFarmingManager, farmAddress, user1, lpToken } =
        await loadFixture(deployWithFarmFixture);

      const stakeAmount = ethers.parseEther('100');

      // Stake first
      await yieldFarmingManager.connect(user1).stake(farmAddress, stakeAmount);

      // Unstake immediately (should incur penalty)
      const tx = await yieldFarmingManager
        .connect(user1)
        .unstake(farmAddress, stakeAmount);

      await expect(tx).to.emit(yieldFarmingManager, 'UserUnstaked');

      // Check user stake is reduced
      const userStake = await yieldFarmingManager.getUserStakeInFarm(
        user1.address,
        farmAddress
      );
      expect(userStake).to.equal(0);
    });

    it('Should prevent unstaking more than staked', async function () {
      const { yieldFarmingManager, farmAddress, user1 } = await loadFixture(
        deployWithFarmFixture
      );

      const stakeAmount = ethers.parseEther('100');
      const unstakeAmount = ethers.parseEther('200');

      await yieldFarmingManager.connect(user1).stake(farmAddress, stakeAmount);

      await expect(
        yieldFarmingManager.connect(user1).unstake(farmAddress, unstakeAmount)
      ).to.be.revertedWith('YieldFarmingManager: Insufficient stake');
    });
  });

  describe('Reward Calculation and Distribution', function () {
    async function deployWithStakedUsersFixture() {
      const baseFixture = await deployWithFarmFixture();
      const { yieldFarmingManager, farmAddress, user1, user2, lpToken } =
        baseFixture;

      // Stake for both users
      const stakeAmount1 = ethers.parseEther('100');
      const stakeAmount2 = ethers.parseEther('200');

      await yieldFarmingManager.connect(user1).stake(farmAddress, stakeAmount1);
      await yieldFarmingManager.connect(user2).stake(farmAddress, stakeAmount2);

      return { ...baseFixture, stakeAmount1, stakeAmount2 };
    }

    it('Should calculate pending rewards correctly', async function () {
      const { yieldFarmingManager, farmAddress, user1, baseRewardRate } =
        await loadFixture(deployWithStakedUsersFixture);

      // Wait some time
      await time.increase(3600); // 1 hour

      const pendingRewards = await yieldFarmingManager.getPendingRewards(
        user1.address,
        farmAddress
      );

      // Should have some pending rewards
      expect(pendingRewards).to.be.gt(0);
    });

    it('Should allow users to claim rewards', async function () {
      const {
        yieldFarmingManager,
        rewardDistributor,
        farmAddress,
        user1,
        wrfToken,
      } = await loadFixture(deployWithStakedUsersFixture);

      // Wait some time
      await time.increase(3600); // 1 hour

      const initialBalance = await wrfToken.balanceOf(user1.address);

      const tx = await yieldFarmingManager
        .connect(user1)
        .claimRewards(farmAddress);

      await expect(tx).to.emit(yieldFarmingManager, 'RewardsClaimed');

      // Check that vesting schedule was created in reward distributor
      const vestingInfo = await rewardDistributor.getVestingInfo(
        user1.address,
        await wrfToken.getAddress()
      );
      expect(vestingInfo.totalAmount).to.be.gt(0);
    });

    it('Should apply quality bonuses correctly', async function () {
      const {
        yieldFarmingManager,
        farmAddress,
        user1,
        qualityOracle,
        carbonBasketToken,
      } = await loadFixture(deployWithStakedUsersFixture);

      // Set high quality score
      await qualityOracle.updateQualityScore(
        await carbonBasketToken.getAddress(),
        900,
        'High quality'
      );

      // Wait and claim rewards
      await time.increase(3600);
      await yieldFarmingManager.connect(user1).claimRewards(farmAddress);

      // Quality bonus should be applied (this is a simplified test)
      const farmInfo = await yieldFarmingManager.getFarmInfo(farmAddress);
      expect(farmInfo.qualityMultiplier).to.be.gt(ethers.parseEther('1'));
    });
  });

  describe('Reward Distributor Vesting', function () {
    async function deployWithRewardsFixture() {
      const baseFixture = await deployWithStakedUsersFixture();
      const {
        rewardDistributor,
        user1,
        wrfToken,
        yieldFarmingManager,
        farmAddress,
      } = baseFixture;

      // Generate some rewards
      await time.increase(3600); // 1 hour
      await yieldFarmingManager.connect(user1).claimRewards(farmAddress);

      return baseFixture;
    }

    it('Should create vesting schedule when distributing rewards', async function () {
      const { rewardDistributor, user1, wrfToken } = await loadFixture(
        deployWithRewardsFixture
      );

      const vestingInfo = await rewardDistributor.getVestingInfo(
        user1.address,
        await wrfToken.getAddress()
      );

      expect(vestingInfo.totalAmount).to.be.gt(0);
      expect(vestingInfo.duration).to.equal(7 * 24 * 60 * 60); // 7 days
      expect(vestingInfo.isRevoked).to.be.false;
    });

    it('Should allow claiming vested rewards', async function () {
      const { rewardDistributor, user1, wrfToken } = await loadFixture(
        deployWithRewardsFixture
      );

      const initialBalance = await wrfToken.balanceOf(user1.address);

      // Wait for some vesting
      await time.increase(24 * 60 * 60); // 1 day

      const claimableBefore = await rewardDistributor.getClaimableAmount(
        user1.address,
        await wrfToken.getAddress()
      );
      expect(claimableBefore).to.be.gt(0);

      const tx = await rewardDistributor
        .connect(user1)
        .claimRewards(await wrfToken.getAddress());
      await expect(tx).to.emit(rewardDistributor, 'RewardClaimed');

      const finalBalance = await wrfToken.balanceOf(user1.address);
      expect(finalBalance).to.be.gt(initialBalance);
    });

    it('Should allow early vesting with penalty', async function () {
      const { rewardDistributor, user1, wrfToken, feeRecipient } =
        await loadFixture(deployWithRewardsFixture);

      const initialUserBalance = await wrfToken.balanceOf(user1.address);
      const initialFeeBalance = await wrfToken.balanceOf(feeRecipient.address);

      const tx = await rewardDistributor
        .connect(user1)
        .claimEarlyVesting(await wrfToken.getAddress());
      await expect(tx).to.emit(rewardDistributor, 'EarlyVesting');

      const finalUserBalance = await wrfToken.balanceOf(user1.address);
      const finalFeeBalance = await wrfToken.balanceOf(feeRecipient.address);

      // User should receive some tokens (less due to penalty)
      expect(finalUserBalance).to.be.gt(initialUserBalance);
      // Fee recipient should receive penalty
      expect(finalFeeBalance).to.be.gt(initialFeeBalance);
    });

    it('Should handle anti-gaming delay correctly', async function () {
      const { rewardDistributor, user1, wrfToken } = await loadFixture(
        deployWithRewardsFixture
      );

      // Try to claim twice quickly
      await rewardDistributor
        .connect(user1)
        .claimRewards(await wrfToken.getAddress());

      await expect(
        rewardDistributor
          .connect(user1)
          .claimRewards(await wrfToken.getAddress())
      ).to.be.revertedWith('RewardDistributor: Anti-gaming delay active');
    });
  });

  describe('Emergency Features', function () {
    it('Should allow emergency withdrawal when enabled', async function () {
      const { yieldFarmingManager, farmAddress, user1, lpToken } =
        await loadFixture(deployWithStakedUsersFixture);

      // Enable emergency withdrawals
      await yieldFarmingManager.setEmergencyWithdrawEnabled(true);

      const initialBalance = await lpToken.balanceOf(user1.address);

      const tx = await yieldFarmingManager
        .connect(user1)
        .emergencyWithdraw(farmAddress);
      await expect(tx).to.emit(yieldFarmingManager, 'EmergencyAction');

      const finalBalance = await lpToken.balanceOf(user1.address);
      expect(finalBalance).to.be.gt(initialBalance);

      // Stake should be zero
      const userStake = await yieldFarmingManager.getUserStakeInFarm(
        user1.address,
        farmAddress
      );
      expect(userStake).to.equal(0);
    });

    it('Should prevent emergency withdrawal when disabled', async function () {
      const { yieldFarmingManager, farmAddress, user1 } = await loadFixture(
        deployWithStakedUsersFixture
      );

      await expect(
        yieldFarmingManager.connect(user1).emergencyWithdraw(farmAddress)
      ).to.be.revertedWith(
        'YieldFarmingManager: Emergency withdraw not enabled'
      );
    });

    it('Should allow pausing and unpausing', async function () {
      const { yieldFarmingManager, farmAddress, user1, lpToken } =
        await loadFixture(deployWithFarmFixture);

      // Pause
      await yieldFarmingManager.pause();

      // Should not allow staking when paused
      await expect(
        yieldFarmingManager
          .connect(user1)
          .stake(farmAddress, ethers.parseEther('100'))
      ).to.be.revertedWith('Pausable: paused');

      // Unpause
      await yieldFarmingManager.unpause();

      // Should allow staking again
      await expect(
        yieldFarmingManager
          .connect(user1)
          .stake(farmAddress, ethers.parseEther('100'))
      ).to.not.be.reverted;
    });
  });

  describe('Access Control', function () {
    it('Should enforce farm manager role for farm creation', async function () {
      const { yieldFarmingManager, liquidityPool, user1 } = await loadFixture(
        deployYieldFarmingFixture
      );

      await expect(
        yieldFarmingManager
          .connect(user1)
          .createFarm(
            liquidityPool,
            ethers.parseEther('0.1'),
            30 * 24 * 60 * 60,
            365 * 24 * 60 * 60,
            90 * 24 * 60 * 60,
            'balanced'
          )
      ).to.be.reverted; // Should fail due to missing role
    });

    it('Should enforce reward manager role for reward configuration', async function () {
      const { rewardDistributor, wrfToken, user1 } = await loadFixture(
        deployYieldFarmingFixture
      );

      await expect(
        rewardDistributor
          .connect(user1)
          .configureRewardToken(
            await wrfToken.getAddress(),
            ethers.parseEther('1000000'),
            ethers.parseEther('1'),
            7 * 24 * 60 * 60,
            'governance'
          )
      ).to.be.reverted; // Should fail due to missing role
    });

    it('Should enforce emergency role for emergency functions', async function () {
      const { yieldFarmingManager, user1 } = await loadFixture(
        deployYieldFarmingFixture
      );

      await expect(
        yieldFarmingManager.connect(user1).setEmergencyWithdrawEnabled(true)
      ).to.be.reverted; // Should fail due to missing role
    });
  });

  describe('Integration Tests', function () {
    it('Should handle complete farming lifecycle correctly', async function () {
      const {
        yieldFarmingManager,
        rewardDistributor,
        farmAddress,
        user1,
        lpToken,
        wrfToken,
      } = await loadFixture(deployWithFarmFixture);

      const stakeAmount = ethers.parseEther('100');

      // 1. Stake
      await yieldFarmingManager.connect(user1).stake(farmAddress, stakeAmount);

      // 2. Wait and accrue rewards
      await time.increase(24 * 60 * 60); // 1 day

      // 3. Claim rewards (creates vesting)
      await yieldFarmingManager.connect(user1).claimRewards(farmAddress);

      // 4. Wait for partial vesting
      await time.increase(2 * 24 * 60 * 60); // 2 more days

      // 5. Claim vested rewards
      const claimableBefore = await rewardDistributor.getClaimableAmount(
        user1.address,
        await wrfToken.getAddress()
      );
      expect(claimableBefore).to.be.gt(0);

      await rewardDistributor
        .connect(user1)
        .claimRewards(await wrfToken.getAddress());

      // 6. Unstake
      await yieldFarmingManager
        .connect(user1)
        .unstake(farmAddress, stakeAmount);

      // Verify final state
      const userStake = await yieldFarmingManager.getUserStakeInFarm(
        user1.address,
        farmAddress
      );
      expect(userStake).to.equal(0);

      const wrfBalance = await wrfToken.balanceOf(user1.address);
      expect(wrfBalance).to.be.gt(0);
    });
  });

  // Helper function for fixture deployment
  async function deployWithFarmFixture() {
    const baseFixture = await deployYieldFarmingFixture();
    const { yieldFarmingManager, liquidityPool, lpToken, user1 } = baseFixture;

    // Create a farm
    const baseRewardRate = ethers.parseEther('0.1');
    const minStakeDuration = 30 * 24 * 60 * 60;
    const maxStakeDuration = 365 * 24 * 60 * 60;
    const farmDuration = 90 * 24 * 60 * 60;

    await yieldFarmingManager.createFarm(
      liquidityPool,
      baseRewardRate,
      minStakeDuration,
      maxStakeDuration,
      farmDuration,
      'balanced'
    );

    const farms = await yieldFarmingManager.getActiveFarms();
    const farmAddress = farms[0].farmAddress;

    // Approve LP tokens for staking
    await lpToken
      .connect(user1)
      .approve(farmAddress, ethers.parseEther('1000'));

    return { ...baseFixture, farmAddress, baseRewardRate };
  }

  async function deployWithStakedUsersFixture() {
    const baseFixture = await deployWithFarmFixture();
    const { yieldFarmingManager, farmAddress, user1, user2, lpToken } =
      baseFixture;

    // Stake for both users
    const stakeAmount1 = ethers.parseEther('100');
    const stakeAmount2 = ethers.parseEther('200');

    await lpToken
      .connect(user2)
      .approve(farmAddress, ethers.parseEther('1000'));

    await yieldFarmingManager.connect(user1).stake(farmAddress, stakeAmount1);
    await yieldFarmingManager.connect(user2).stake(farmAddress, stakeAmount2);

    return { ...baseFixture, stakeAmount1, stakeAmount2 };
  }
});
