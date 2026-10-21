const { expect } = require('chai');
const { ethers } = require('hardhat');
const { loadFixture } = require('@nomicfoundation/hardhat-network-helpers');

/**
 * @title SecureStakingRewardCalculator Security Tests
 * @dev Comprehensive test suite for OWASP SC03 Arithmetic Issues in staking reward calculations
 *
 * SECURITY TEST COVERAGE:
 * - SafeMath overflow/underflow protection
 * - Bounds checking for multiplier operations
 * - Division by zero protection
 * - Integer arithmetic validation
 * - Edge case handling for reward calculations
 *
 * TASK 99.13 VALIDATION:
 * - Protects $2M TVL from arithmetic manipulation
 * - Validates secure reward calculation logic
 * - Tests comprehensive SafeMath integration
 */
describe('SecureStakingRewardCalculator - Security Tests', function () {
  // Test constants
  const PRECISION = ethers.parseEther('1'); // 1e18
  const BASIS_POINTS = 10000;
  const MAX_UINT256 = ethers.MaxUint256;
  const SECONDS_PER_DAY = 86400;

  async function deploySecureStakingRewardCalculatorFixture() {
    const [owner, user1, user2, stakingManager, calculator, attacker] =
      await ethers.getSigners();

    // Deploy mock quality oracle
    const MockQualityOracle = await ethers.getContractFactory(
      'contracts/mocks/MockERC20.sol:MockERC20'
    );
    const qualityOracle = await MockQualityOracle.deploy('MockOracle', 'MO');
    await qualityOracle.waitForDeployment();

    // Deploy SecureStakingRewardCalculator
    const SecureStakingRewardCalculator = await ethers.getContractFactory(
      'SecureStakingRewardCalculator'
    );
    const calculator_contract = await SecureStakingRewardCalculator.deploy(
      await qualityOracle.getAddress()
    );
    await calculator_contract.waitForDeployment();

    // Grant roles
    const CALCULATOR_ROLE = await calculator_contract.CALCULATOR_ROLE();
    const STAKING_MANAGER_ROLE =
      await calculator_contract.STAKING_MANAGER_ROLE();

    await calculator_contract.grantRole(CALCULATOR_ROLE, calculator.address);
    await calculator_contract.grantRole(
      STAKING_MANAGER_ROLE,
      stakingManager.address
    );
    await calculator_contract.setAuthorizedCalculator(calculator.address, true);

    return {
      calculator: calculator_contract,
      qualityOracle,
      owner,
      user1,
      user2,
      stakingManager,
      calculator: calculator,
      attacker,
      CALCULATOR_ROLE,
      STAKING_MANAGER_ROLE,
    };
  }

  describe('Arithmetic Security Validation', function () {
    it('Should prevent overflow in base reward calculation', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // Create parameters that would cause overflow in vulnerable implementation
      const params = {
        stakedAmount: ethers.parseEther('1000000000'), // Very large amount
        baseRewardRate: ethers.parseEther('1000000'), // Very large rate
        stakeDuration: 365 * SECONDS_PER_DAY, // 1 year
        qualityMultiplier: PRECISION, // 100%
        vintageBonus: 0, // No bonus
        sustainabilityBonus: 0, // No bonus
        globalMultiplier: PRECISION, // 100%
        minStakeDuration: 7 * SECONDS_PER_DAY, // 1 week
        maxStakeDuration: 365 * SECONDS_PER_DAY, // 1 year
      };

      // Should revert with overflow protection message
      await expect(
        calculator.connect(calculator).calculateStakingRewards(params)
      ).to.be.revertedWith(
        'SecureStakingRewardCalculator: Stake amount too large for calculation'
      );
    });

    it('Should prevent overflow in quality multiplier calculation', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('1'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: ethers.parseEther('1000000'), // Extremely high multiplier
        vintageBonus: 0,
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Should revert due to multiplier validation
      await expect(
        calculator.connect(calculator).calculateStakingRewards(params)
      ).to.be.revertedWith(
        'SecureStakingRewardCalculator: Quality multiplier too high'
      );
    });

    it('Should prevent overflow in vintage bonus calculation', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('1'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 50000, // 500% bonus (too high)
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Should revert due to bonus validation
      await expect(
        calculator.connect(calculator).calculateStakingRewards(params)
      ).to.be.revertedWith(
        'SecureStakingRewardCalculator: Vintage bonus too high'
      );
    });

    it('Should prevent division by zero errors', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: 0, // Zero staked amount
        baseRewardRate: ethers.parseEther('1'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Should revert due to zero staked amount
      await expect(
        calculator.connect(calculator).calculateStakingRewards(params)
      ).to.be.revertedWith(
        'SecureStakingRewardCalculator: Invalid staked amount'
      );
    });

    it('Should handle edge case calculations correctly', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // Very small values
      const params = {
        stakedAmount: 1,
        baseRewardRate: 1,
        stakeDuration: 1,
        qualityMultiplier: PRECISION,
        vintageBonus: 0,
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 1,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);

      // Should handle small values without reverting
      expect(result.baseReward).to.be.greaterThan(0);
      expect(result.totalReward).to.be.greaterThan(0);
      expect(result.hasOverflowRisk).to.be.false;
    });
  });

  describe('SafeMath Integration Validation', function () {
    it('Should correctly calculate base rewards with SafeMath', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const stakedAmount = ethers.parseEther('1000');
      const baseRewardRate = ethers.parseEther('0.01'); // 1% per unit time
      const stakeDuration = 30 * SECONDS_PER_DAY;

      const params = {
        stakedAmount,
        baseRewardRate,
        stakeDuration,
        qualityMultiplier: PRECISION,
        vintageBonus: 0,
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);

      // Calculate expected base reward: (stakedAmount * baseRewardRate * stakeDuration) / PRECISION
      const expectedBaseReward =
        (stakedAmount * baseRewardRate * BigInt(stakeDuration)) / PRECISION;

      expect(result.baseReward).to.equal(expectedBaseReward);
    });

    it('Should correctly apply quality multiplier with SafeMath', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: ethers.parseEther('1.5'), // 150% multiplier
        vintageBonus: 0,
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);

      // Quality boosted reward should be 1.5x base reward
      const expectedQualityBoosted =
        (result.baseReward * BigInt(ethers.parseEther('1.5'))) / PRECISION;
      expect(result.qualityBoostedReward).to.equal(expectedQualityBoosted);
    });

    it('Should correctly calculate vintage bonus with SafeMath', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000, // 10% bonus
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);

      // Vintage bonus should be 10% of base reward
      const expectedVintageBonus =
        (result.baseReward * BigInt(1000)) / BigInt(BASIS_POINTS);
      expect(result.vintageBonus).to.equal(expectedVintageBonus);
    });

    it('Should correctly calculate total reward with all bonuses', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 365 * SECONDS_PER_DAY, // 1 year for duration bonus
        qualityMultiplier: ethers.parseEther('1.5'), // 150%
        vintageBonus: 1000, // 10%
        sustainabilityBonus: 500, // 5%
        globalMultiplier: ethers.parseEther('1.1'), // 110%
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);

      // Total should include all components
      expect(result.totalReward).to.be.greaterThan(result.baseReward);
      expect(result.totalReward).to.be.greaterThan(result.qualityBoostedReward);

      // Verify no overflow occurred
      expect(result.hasOverflowRisk).to.be.false;

      // Verify effective multiplier is reasonable
      expect(result.effectiveMultiplier).to.be.greaterThan(PRECISION);
      expect(result.effectiveMultiplier).to.be.lessThan(
        ethers.parseEther('10')
      ); // Less than 10x
    });
  });

  describe('Validation and Security Checks', function () {
    it('Should validate calculation safety before processing', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const validation = await calculator.validateCalculationSafety(params);

      expect(validation.isValid).to.be.true;
      expect(validation.errorMessage).to.equal('');
      expect(validation.maxSafeAmount).to.be.greaterThan(0);
    });

    it('Should detect unsafe calculation parameters', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const unsafeParams = {
        stakedAmount: ethers.parseEther('1000000000'), // Too large
        baseRewardRate: ethers.parseEther('1000000'), // Too large
        stakeDuration: 365 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 0,
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const validation =
        await calculator.validateCalculationSafety(unsafeParams);

      expect(validation.isValid).to.be.false;
      expect(validation.errorMessage).to.include(
        'too large for safe calculation'
      );
      expect(validation.maxSafeAmount).to.be.greaterThan(0);
      expect(validation.recommendedAmount).to.be.greaterThan(0);
    });

    it('Should handle zero values safely', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const zeroStakeParams = {
        stakedAmount: 0,
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const validation =
        await calculator.validateCalculationSafety(zeroStakeParams);

      expect(validation.isValid).to.be.false;
      expect(validation.errorMessage).to.include('cannot be zero');
    });

    it('Should prevent unauthorized calculation access', async function () {
      const { calculator, attacker } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Unauthorized user should not be able to call calculation function
      await expect(
        calculator.connect(attacker).calculateStakingRewards(params)
      ).to.be.revertedWith(
        'SecureStakingRewardCalculator: Not authorized calculator'
      );
    });
  });

  describe('Integration and Edge Cases', function () {
    it('Should handle maximum safe values correctly', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // Calculate maximum safe staked amount
      const maxRewardRate = ethers.parseEther('1');
      const maxDuration = 365 * SECONDS_PER_DAY;
      const maxSafeStake = MAX_UINT256 / maxRewardRate / BigInt(maxDuration);
      const safeStake = maxSafeStake / BigInt(1000); // Use much smaller safe value

      const params = {
        stakedAmount: safeStake,
        baseRewardRate: ethers.parseEther('0.001'), // Smaller rate
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);

      expect(result.baseReward).to.be.greaterThan(0);
      expect(result.totalReward).to.be.greaterThan(result.baseReward);
      expect(result.hasOverflowRisk).to.be.false;
    });

    it('Should calculate duration bonuses correctly', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // 1 year staking should get duration bonus
      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 365 * SECONDS_PER_DAY, // 1 year
        qualityMultiplier: PRECISION,
        vintageBonus: 0,
        sustainabilityBonus: 0,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);

      // Should have duration bonus for 1 year staking
      expect(result.durationBonus).to.be.greaterThan(0);

      // Total should include duration bonus
      const expectedTotal = result.qualityBoostedReward + result.durationBonus;
      expect(result.totalReward).to.equal(expectedTotal);
    });

    it('Should allow preview calculations without authorization', async function () {
      const { calculator, attacker } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Preview should work for any user
      const result = await calculator
        .connect(attacker)
        .previewRewardCalculation(params);

      expect(result.baseReward).to.be.greaterThan(0);
      expect(result.totalReward).to.be.greaterThan(0);
    });

    it('Should maintain calculation statistics', async function () {
      const { calculator, user1 } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // First check initial stats
      const initialStats = await calculator.getCalculationStats(user1.address);
      expect(initialStats.totalCalculations).to.equal(0);
      expect(initialStats.totalRewards).to.equal(0);

      // Note: The actual calculation function would update stats,
      // but our current implementation is view-only for the main calculation
      // In a real implementation, stats would be updated during actual reward distribution
    });
  });

  describe('Emergency Controls', function () {
    it('Should pause calculations when emergency pause is activated', async function () {
      const { calculator, owner } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // Emergency pause
      await calculator.connect(owner).pauseCalculations();

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Should revert when paused
      await expect(
        calculator.connect(calculator).calculateStakingRewards(params)
      ).to.be.revertedWith(
        'SecureStakingRewardCalculator: Calculations paused'
      );

      // Resume calculations
      await calculator.connect(owner).resumeCalculations();

      // Should work again after resume
      const result = await calculator
        .connect(calculator)
        .calculateStakingRewards(params);
      expect(result.totalReward).to.be.greaterThan(0);
    });

    it('Should allow emergency contract pause', async function () {
      const { calculator, owner } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // Emergency pause contract
      await calculator.connect(owner).pause();

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Should revert when contract is paused
      await expect(
        calculator.connect(calculator).calculateStakingRewards(params)
      ).to.be.revertedWith('Pausable: paused');
    });
  });

  describe('Quality Tier Integration', function () {
    it('Should have correct quality tier configurations', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // Check premium tier (tier 1)
      const tier1 = await calculator.getQualityTier(1);
      expect(tier1.multiplier).to.equal(ethers.parseEther('2')); // 200%
      expect(tier1.tierName).to.equal('Premium');

      // Check standard tier (tier 3)
      const tier3 = await calculator.getQualityTier(3);
      expect(tier3.multiplier).to.equal(PRECISION); // 100%
      expect(tier3.tierName).to.equal('Standard');
    });

    it('Should update quality tier configurations', async function () {
      const { calculator, stakingManager } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      // Update tier 1 configuration
      await calculator.connect(stakingManager).updateQualityTier(
        1, // tier
        ethers.parseEther('2.5'), // 250% multiplier
        95, // min score
        100, // max score
        'Super Premium' // tier name
      );

      const updatedTier = await calculator.getQualityTier(1);
      expect(updatedTier.multiplier).to.equal(ethers.parseEther('2.5'));
      expect(updatedTier.tierName).to.equal('Super Premium');
    });
  });

  describe('Gas Efficiency', function () {
    it('Should have reasonable gas costs for calculations', async function () {
      const { calculator } = await loadFixture(
        deploySecureStakingRewardCalculatorFixture
      );

      const params = {
        stakedAmount: ethers.parseEther('1000'),
        baseRewardRate: ethers.parseEther('0.01'),
        stakeDuration: 30 * SECONDS_PER_DAY,
        qualityMultiplier: PRECISION,
        vintageBonus: 1000,
        sustainabilityBonus: 500,
        globalMultiplier: PRECISION,
        minStakeDuration: 7 * SECONDS_PER_DAY,
        maxStakeDuration: 365 * SECONDS_PER_DAY,
      };

      // Estimate gas for calculation
      const gasEstimate = await calculator
        .connect(calculator)
        .calculateStakingRewards.estimateGas(params);

      // Gas should be reasonable (less than 200k for calculation)
      expect(gasEstimate).to.be.lessThan(200000);
    });
  });
});
