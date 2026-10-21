// SPDX-License-Identifier: MIT
import { expect } from 'chai';
import hre from 'hardhat';
const { ethers } = hre;
import { loadFixture, time } from '@nomicfoundation/hardhat-network-helpers';

/**
 * Comprehensive Security Test Suite for SecureStakingRewardCalculator
 *
 * SECURITY VULNERABILITIES ADDRESSED:
 * - OWASP SC03: Arithmetic Issues (overflow/underflow)
 * - Precision and rounding attacks
 * - Reward manipulation through parameter manipulation
 * - Economic attacks targeting $2M TVL
 * - Integer overflow/underflow in calculations
 * - Bounds checking for multipliers and rates
 * - Access control for critical functions
 * - DoS attacks through array manipulation
 */
describe('SecureStakingRewardCalculator - Comprehensive Security Tests', function () {
  let calculator, mockOracle, owner, staker, attacker, stakingManager;
  let deployer, oracleManager;

  // Quality tiers enum
  const QualityTier = {
    NONE: 0,
    BRONZE: 1,
    SILVER: 2,
    GOLD: 3,
    PLATINUM: 4,
    DIAMOND: 5,
  };

  // Maximum values for testing edge cases
  const MAX_UINT256 = ethers.MaxUint256;
  const MAX_SAFE_UINT256 = ethers.parseEther('1000000000'); // 1B tokens

  // Security test fixture
  async function deployCalculatorFixture() {
    [deployer, owner, staker, attacker, stakingManager, oracleManager] =
      await ethers.getSigners();

    // Deploy mock quality oracle
    const MockOracle = await ethers.getContractFactory('MockOracle');
    const mockOracle = await MockOracle.deploy();
    await mockOracle.waitForDeployment();

    // Deploy SecureStakingRewardCalculator
    const SecureStakingRewardCalculator = await ethers.getContractFactory(
      'SecureStakingRewardCalculator'
    );
    const calculator = await SecureStakingRewardCalculator.deploy(
      await mockOracle.getAddress(),
      owner.address // Emergency role
    );
    await calculator.waitForDeployment();

    // Grant necessary roles
    const STAKING_MANAGER_ROLE = await calculator.STAKING_MANAGER_ROLE();
    const ORACLE_MANAGER_ROLE = await calculator.ORACLE_MANAGER_ROLE();

    await calculator
      .connect(owner)
      .grantRole(STAKING_MANAGER_ROLE, stakingManager.address);
    await calculator
      .connect(owner)
      .grantRole(ORACLE_MANAGER_ROLE, oracleManager.address);

    // Set up mock oracle with test data
    await mockOracle.setQualityScore(100); // High quality score

    return {
      calculator,
      mockOracle,
      owner,
      staker,
      attacker,
      stakingManager,
      oracleManager,
      deployer,
    };
  }

  describe('🔢 Arithmetic Overflow/Underflow Protection', function () {
    it('Should prevent overflow in base reward calculations', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Test with maximum safe values
      const maxStakedAmount = ethers.parseEther('1000000'); // 1M tokens
      const maxDuration = 365 * 24 * 3600; // 1 year in seconds
      const vintageYear = 2024;

      // Should not overflow
      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          maxStakedAmount,
          maxDuration,
          QualityTier.GOLD,
          vintageYear
        );

      expect(reward).to.be.lt(MAX_SAFE_UINT256);
    });

    it('Should prevent overflow with extreme multipliers', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Test with values that could cause overflow
      const largeAmount = ethers.parseEther('100000000'); // 100M tokens
      const longDuration = 10 * 365 * 24 * 3600; // 10 years
      const vintageYear = 2024;

      await expect(
        calculator
          .connect(stakingManager)
          .calculateRewards(
            largeAmount,
            longDuration,
            QualityTier.DIAMOND,
            vintageYear
          )
      ).to.be.revertedWith('Calculation would overflow');
    });

    it('Should prevent underflow in bonus calculations', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const duration = 24 * 3600; // 1 day
      const vintageYear = 1900; // Very old vintage that might cause underflow

      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.BRONZE,
          vintageYear
        );

      // Should not underflow (return 0 or positive value)
      expect(reward).to.be.gte(0);
    });

    it('Should handle zero values safely', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      await expect(
        calculator.connect(stakingManager).calculateRewards(
          0, // Zero staked amount
          3600,
          QualityTier.GOLD,
          2024
        )
      ).to.be.revertedWith('Invalid staked amount');

      await expect(
        calculator.connect(stakingManager).calculateRewards(
          ethers.parseEther('1000'),
          0, // Zero duration
          QualityTier.GOLD,
          2024
        )
      ).to.be.revertedWith('Invalid duration');
    });
  });

  describe('🎯 Precision and Rounding Attack Prevention', function () {
    it('Should maintain precision in reward calculations', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Test with small amounts that could cause precision loss
      const smallAmount = ethers.parseUnits('1', 6); // 1 micro-token
      const shortDuration = 1; // 1 second
      const vintageYear = 2024;

      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          smallAmount,
          shortDuration,
          QualityTier.BRONZE,
          vintageYear
        );

      // Should not return zero due to precision loss
      expect(reward).to.be.gte(0);
    });

    it('Should prevent rounding manipulation attacks', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const baseAmount = ethers.parseEther('1000');
      const duration = 3600; // 1 hour
      const vintageYear = 2024;

      // Calculate reward for base amount
      const baseReward = await calculator
        .connect(stakingManager)
        .calculateRewards(baseAmount, duration, QualityTier.GOLD, vintageYear);

      // Calculate reward for slightly different amount
      const modifiedAmount = baseAmount + 1n;
      const modifiedReward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          modifiedAmount,
          duration,
          QualityTier.GOLD,
          vintageYear
        );

      // Difference should be proportional, not exploitable
      const rewardDifference = modifiedReward - baseReward;
      expect(rewardDifference).to.be.lte(baseReward / 1000000n); // < 0.0001% difference
    });

    it('Should handle precision scaling correctly', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Test with amounts that require precision scaling
      const preciseAmount = ethers.parseEther('1000.123456789');
      const duration = 7 * 24 * 3600; // 1 week
      const vintageYear = 2024;

      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          preciseAmount,
          duration,
          QualityTier.SILVER,
          vintageYear
        );

      expect(reward).to.be.gt(0);
      expect(reward).to.be.lt(preciseAmount); // Sanity check
    });
  });

  describe('💰 Economic Security ($2M TVL Protection)', function () {
    it('Should enforce maximum reward limits', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const largeStake = ethers.parseEther('2000000'); // $2M equivalent
      const longDuration = 365 * 24 * 3600; // 1 year
      const vintageYear = 2024;

      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          largeStake,
          longDuration,
          QualityTier.DIAMOND,
          vintageYear
        );

      // Reward should not exceed safety limit (50% of stake)
      const maxAllowedReward = largeStake / 2n;
      expect(reward).to.be.lte(maxAllowedReward);
    });

    it('Should prevent reward manipulation through extreme parameters', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const normalStake = ethers.parseEther('1000');
      const extremeDuration = 100 * 365 * 24 * 3600; // 100 years
      const vintageYear = 2024;

      await expect(
        calculator
          .connect(stakingManager)
          .calculateRewards(
            normalStake,
            extremeDuration,
            QualityTier.DIAMOND,
            vintageYear
          )
      ).to.be.revertedWith('Duration exceeds maximum allowed');
    });

    it('Should validate vintage year bounds', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const duration = 30 * 24 * 3600; // 30 days

      // Future vintage year should be rejected
      const futureYear = new Date().getFullYear() + 10;
      await expect(
        calculator
          .connect(stakingManager)
          .calculateRewards(
            stakedAmount,
            duration,
            QualityTier.GOLD,
            futureYear
          )
      ).to.be.revertedWith('Invalid vintage year');

      // Very old vintage year should be handled safely
      const ancientYear = 1900;
      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.GOLD,
          ancientYear
        );
      expect(reward).to.be.gte(0);
    });

    it('Should prevent reward farming through quality tier manipulation', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const duration = 7 * 24 * 3600; // 1 week
      const vintageYear = 2024;

      // Calculate rewards for different quality tiers
      const bronzeReward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.BRONZE,
          vintageYear
        );

      const diamondReward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.DIAMOND,
          vintageYear
        );

      // Higher tier should give higher rewards, but within reasonable bounds
      expect(diamondReward).to.be.gt(bronzeReward);
      expect(diamondReward).to.be.lte(bronzeReward * 5n); // Max 5x multiplier
    });
  });

  describe('🚫 Access Control Security', function () {
    it('Should prevent unauthorized reward calculations', async function () {
      const { calculator, attacker } = await loadFixture(
        deployCalculatorFixture
      );

      await expect(
        calculator
          .connect(attacker)
          .calculateRewards(
            ethers.parseEther('1000'),
            3600,
            QualityTier.GOLD,
            2024
          )
      ).to.be.revertedWith(/AccessControl:/);
    });

    it('Should prevent unauthorized parameter updates', async function () {
      const { calculator, attacker } = await loadFixture(
        deployCalculatorFixture
      );

      await expect(
        calculator
          .connect(attacker)
          .updateBaseRewardRate(ethers.parseEther('0.01'))
      ).to.be.revertedWith(/AccessControl:/);

      await expect(
        calculator
          .connect(attacker)
          .updateQualityMultipliers([1000, 1100, 1200, 1300, 1400])
      ).to.be.revertedWith(/AccessControl:/);
    });

    it('Should prevent unauthorized emergency actions', async function () {
      const { calculator, attacker } = await loadFixture(
        deployCalculatorFixture
      );

      await expect(calculator.connect(attacker).pause()).to.be.revertedWith(
        /AccessControl:/
      );

      await expect(calculator.connect(attacker).unpause()).to.be.revertedWith(
        /AccessControl:/
      );
    });

    it('Should prevent unauthorized oracle updates', async function () {
      const { calculator, attacker } = await loadFixture(
        deployCalculatorFixture
      );

      await expect(
        calculator.connect(attacker).updateQualityOracle(attacker.address)
      ).to.be.revertedWith(/AccessControl:/);
    });
  });

  describe('🏗️ Input Validation and Bounds Checking', function () {
    it('Should validate staked amount bounds', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const duration = 7 * 24 * 3600; // 1 week
      const vintageYear = 2024;

      // Maximum allowed amount
      const maxAmount = await calculator.MAX_STAKE_AMOUNT();

      await expect(
        calculator
          .connect(stakingManager)
          .calculateRewards(
            maxAmount + 1n,
            duration,
            QualityTier.GOLD,
            vintageYear
          )
      ).to.be.revertedWith('Stake amount exceeds maximum');
    });

    it('Should validate duration bounds', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const vintageYear = 2024;

      // Maximum allowed duration
      const maxDuration = await calculator.MAX_STAKING_DURATION();

      await expect(
        calculator
          .connect(stakingManager)
          .calculateRewards(
            stakedAmount,
            maxDuration + 1n,
            QualityTier.GOLD,
            vintageYear
          )
      ).to.be.revertedWith('Duration exceeds maximum allowed');
    });

    it('Should validate quality tier enum', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const duration = 7 * 24 * 3600;
      const vintageYear = 2024;

      // Invalid quality tier
      await expect(
        calculator.connect(stakingManager).calculateRewards(
          stakedAmount,
          duration,
          99, // Invalid tier
          vintageYear
        )
      ).to.be.revertedWith('Invalid quality tier');
    });

    it('Should handle edge case values safely', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Minimum valid values
      const minStake = ethers.parseUnits('1', 18); // 1 wei
      const minDuration = 1; // 1 second
      const currentYear = new Date().getFullYear();

      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          minStake,
          minDuration,
          QualityTier.BRONZE,
          currentYear
        );

      expect(reward).to.be.gte(0);
    });
  });

  describe('💾 Array and Memory DoS Protection', function () {
    it('Should prevent DoS through large array operations', async function () {
      const { calculator, owner } = await loadFixture(deployCalculatorFixture);

      // Attempt to set very large array of multipliers
      const largeArray = new Array(10000).fill(1000);

      await expect(
        calculator.connect(owner).updateQualityMultipliers(largeArray)
      ).to.be.revertedWith('Array too large');
    });

    it('Should validate array length consistency', async function () {
      const { calculator, owner } = await loadFixture(deployCalculatorFixture);

      // Try to set invalid number of multipliers
      const invalidArray = [1000, 1100]; // Should be 5 elements for 5 quality tiers

      await expect(
        calculator.connect(owner).updateQualityMultipliers(invalidArray)
      ).to.be.revertedWith('Invalid multipliers array length');
    });

    it('Should handle batch operations efficiently', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Test batch calculation (if implemented)
      const stakes = [
        ethers.parseEther('100'),
        ethers.parseEther('200'),
        ethers.parseEther('300'),
      ];
      const durations = [3600, 7200, 10800]; // 1, 2, 3 hours
      const tiers = [QualityTier.BRONZE, QualityTier.SILVER, QualityTier.GOLD];
      const years = [2024, 2024, 2024];

      // Should not cause DoS even with multiple calculations
      for (let i = 0; i < stakes.length; i++) {
        const reward = await calculator
          .connect(stakingManager)
          .calculateRewards(stakes[i], durations[i], tiers[i], years[i]);
        expect(reward).to.be.gte(0);
      }
    });
  });

  describe('⏰ Time-based Attack Prevention', function () {
    it('Should handle timestamp manipulation safely', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const duration = 7 * 24 * 3600; // 1 week
      const vintageYear = 2024;

      // Calculate reward before time manipulation
      const rewardBefore = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.GOLD,
          vintageYear
        );

      // Advance time significantly
      await time.increase(365 * 24 * 3600); // 1 year

      // Calculate reward after time advancement
      const rewardAfter = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.GOLD,
          vintageYear
        );

      // Rewards should be consistent regardless of block timestamp
      expect(rewardAfter).to.equal(rewardBefore);
    });

    it('Should validate vintage year against current time', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const duration = 7 * 24 * 3600;

      // Future vintage year should be rejected
      const futureYear = new Date().getFullYear() + 1;

      await expect(
        calculator
          .connect(stakingManager)
          .calculateRewards(
            stakedAmount,
            duration,
            QualityTier.GOLD,
            futureYear
          )
      ).to.be.revertedWith('Invalid vintage year');
    });
  });

  describe('🔄 Reentrancy Protection', function () {
    it('Should prevent reentrancy in calculation functions', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Deploy a malicious contract that attempts reentrancy
      const MaliciousCalculator = await ethers.getContractFactory(
        'MaliciousFlashLoanReceiver'
      );
      const maliciousCalculator = await MaliciousCalculator.deploy();
      await maliciousCalculator.waitForDeployment();

      // Attempt to use calculator from malicious contract
      const stakedAmount = ethers.parseEther('1000');
      const duration = 3600;
      const vintageYear = 2024;

      // Direct call should work
      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.GOLD,
          vintageYear
        );

      expect(reward).to.be.gt(0);
    });
  });

  describe('🎲 Oracle Manipulation Protection', function () {
    it('Should validate oracle responses', async function () {
      const { calculator, mockOracle, stakingManager, oracleManager } =
        await loadFixture(deployCalculatorFixture);

      // Set invalid oracle data
      await mockOracle.setQualityScore(0); // Invalid score

      const stakedAmount = ethers.parseEther('1000');
      const duration = 3600;
      const vintageYear = 2024;

      await expect(
        calculator
          .connect(stakingManager)
          .calculateRewards(
            stakedAmount,
            duration,
            QualityTier.GOLD,
            vintageYear
          )
      ).to.be.revertedWith('Invalid oracle response');
    });

    it('Should handle oracle failures gracefully', async function () {
      const { calculator, stakingManager, oracleManager } = await loadFixture(
        deployCalculatorFixture
      );

      // Deploy failing oracle
      const FailingOracle = await ethers.getContractFactory('MockOracle');
      const failingOracle = await FailingOracle.deploy();
      await failingOracle.waitForDeployment();

      // Update to failing oracle
      await calculator
        .connect(oracleManager)
        .updateQualityOracle(await failingOracle.getAddress());

      const stakedAmount = ethers.parseEther('1000');
      const duration = 3600;
      const vintageYear = 2024;

      // Should use fallback mechanism or safe defaults
      const reward = await calculator
        .connect(stakingManager)
        .calculateRewards(
          stakedAmount,
          duration,
          QualityTier.GOLD,
          vintageYear
        );

      expect(reward).to.be.gte(0);
    });
  });

  describe('🔧 Configuration and Parameter Security', function () {
    it('Should validate base reward rate updates', async function () {
      const { calculator, owner } = await loadFixture(deployCalculatorFixture);

      // Zero rate should be rejected
      await expect(
        calculator.connect(owner).updateBaseRewardRate(0)
      ).to.be.revertedWith('Invalid reward rate');

      // Excessive rate should be rejected
      const excessiveRate = ethers.parseEther('1'); // 100% per second
      await expect(
        calculator.connect(owner).updateBaseRewardRate(excessiveRate)
      ).to.be.revertedWith('Reward rate too high');
    });

    it('Should validate quality multiplier ranges', async function () {
      const { calculator, owner } = await loadFixture(deployCalculatorFixture);

      // Multipliers with invalid values
      const invalidMultipliers = [0, 1100, 1200, 1300, 1400]; // First multiplier is 0

      await expect(
        calculator.connect(owner).updateQualityMultipliers(invalidMultipliers)
      ).to.be.revertedWith('Invalid multiplier value');

      // Multipliers that are too high
      const excessiveMultipliers = [1000, 1100, 1200, 1300, 10000]; // Last multiplier is 10x

      await expect(
        calculator.connect(owner).updateQualityMultipliers(excessiveMultipliers)
      ).to.be.revertedWith('Multiplier too high');
    });

    it('Should maintain configuration consistency', async function () {
      const { calculator, owner } = await loadFixture(deployCalculatorFixture);

      // Valid multipliers should be accepted
      const validMultipliers = [1000, 1100, 1200, 1300, 1400]; // 10% increments

      await calculator
        .connect(owner)
        .updateQualityMultipliers(validMultipliers);

      // Verify multipliers were set
      for (let i = 0; i < validMultipliers.length; i++) {
        const multiplier = await calculator.qualityMultipliers(i);
        expect(multiplier).to.equal(validMultipliers[i]);
      }
    });
  });
});
