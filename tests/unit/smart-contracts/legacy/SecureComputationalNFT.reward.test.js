const { expect } = require('chai');
const { ethers } = require('hardhat');
const { time } = require('@nomicfoundation/hardhat-network-helpers');

describe('SecureComputationalNFT - Reward Calculation Security Tests', function () {
  let computeNFT, wrfToken;
  let owner, creator, provider1, provider2, attacker;

  // Test constants
  const MIN_STAKE = ethers.parseEther('100'); // 100 WRF
  const COMPUTE_REWARD_RATE = ethers.parseEther('10'); // 10 WRF per hour
  const MAX_EXECUTION_TIME = 86400; // 24 hours
  const MIN_EXECUTION_TIME = 60; // 1 minute
  const CREATOR_PERCENTAGE = 70; // 70%
  const PROVIDER_BASE_PERCENTAGE = 30; // 30%
  const PERCENTAGE_BASE = 100;
  const SECONDS_PER_HOUR = 3600;
  const SLASH_PERCENTAGE = 10; // 10%

  beforeEach(async function () {
    [owner, creator, provider1, provider2, attacker] =
      await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('MockERC20');
    wrfToken = await WRFToken.deploy(
      'WRF Token',
      'WRF',
      ethers.parseEther('10000000')
    );

    // Deploy SecureComputationalNFT
    const SecureComputationalNFT = await ethers.getContractFactory(
      'SecureComputationalNFT'
    );
    computeNFT = await SecureComputationalNFT.deploy(
      await wrfToken.getAddress()
    );

    // Distribute tokens
    await wrfToken.transfer(creator.address, ethers.parseEther('10000'));
    await wrfToken.transfer(provider1.address, ethers.parseEther('10000'));
    await wrfToken.transfer(provider2.address, ethers.parseEther('10000'));
    await wrfToken.transfer(attacker.address, ethers.parseEther('10000'));

    // Approve tokens
    await wrfToken
      .connect(creator)
      .approve(await computeNFT.getAddress(), ethers.parseEther('10000'));
    await wrfToken
      .connect(owner)
      .approve(await computeNFT.getAddress(), ethers.parseEther('10000'));

    // Register providers
    await computeNFT.registerComputeProvider(
      provider1.address,
      '{"cpu": 8, "memory": "16GB", "storage": "1TB"}',
      ethers.parseEther('1000')
    );

    await computeNFT.registerComputeProvider(
      provider2.address,
      '{"cpu": 16, "memory": "32GB", "storage": "2TB"}',
      ethers.parseEther('2000')
    );
  });

  describe('Reward Calculation Security', function () {
    it('Should prevent arithmetic overflow in reward calculations', async function () {
      // Test with maximum safe execution time
      const maxExecutionTime = MAX_EXECUTION_TIME; // 24 hours
      const stakeAmount = ethers.parseEther('1000');

      // Calculate expected values using SafeMath logic
      const expectedComputeReward =
        (BigInt(maxExecutionTime) * COMPUTE_REWARD_RATE) /
        BigInt(SECONDS_PER_HOUR);
      const expectedTotalReward = stakeAmount + expectedComputeReward;
      const expectedCreatorPortion =
        (stakeAmount * BigInt(CREATOR_PERCENTAGE)) / BigInt(PERCENTAGE_BASE);
      const expectedProviderPortion =
        expectedTotalReward - expectedCreatorPortion;

      const [computeReward, totalReward, creatorPortion, providerPortion] =
        await computeNFT.calculateExpectedRewards(
          maxExecutionTime,
          stakeAmount
        );

      expect(computeReward).to.equal(expectedComputeReward);
      expect(totalReward).to.equal(expectedTotalReward);
      expect(creatorPortion).to.equal(expectedCreatorPortion);
      expect(providerPortion).to.equal(expectedProviderPortion);
    });

    it('Should prevent underflow in reward calculations', async function () {
      // Test with minimum values
      const minExecutionTime = MIN_EXECUTION_TIME; // 1 minute
      const minStakeAmount = MIN_STAKE; // 100 WRF

      const [computeReward, totalReward, creatorPortion, providerPortion] =
        await computeNFT.calculateExpectedRewards(
          minExecutionTime,
          minStakeAmount
        );

      // All values should be positive and properly calculated
      expect(computeReward).to.be.greaterThan(0);
      expect(totalReward).to.be.greaterThan(minStakeAmount);
      expect(creatorPortion).to.be.greaterThan(0);
      expect(providerPortion).to.be.greaterThan(0);

      // Verify distribution adds up correctly
      expect(creatorPortion + providerPortion).to.equal(totalReward);
    });

    it('Should validate input parameters before calculations', async function () {
      // Test with invalid execution time (too high)
      await expect(
        computeNFT.calculateExpectedRewards(MAX_EXECUTION_TIME + 1, MIN_STAKE)
      ).to.be.revertedWith('SecureComputationalNFT: Execution time too long');

      // Test with invalid execution time (too low)
      await expect(
        computeNFT.calculateExpectedRewards(MIN_EXECUTION_TIME - 1, MIN_STAKE)
      ).to.be.revertedWith('SecureComputationalNFT: Execution time too short');

      // Test with invalid stake amount (too low)
      await expect(
        computeNFT.calculateExpectedRewards(MIN_EXECUTION_TIME, MIN_STAKE - 1n)
      ).to.be.revertedWith('SecureComputationalNFT: Stake below minimum');
    });

    it('Should handle precision correctly in percentage calculations', async function () {
      const executionTime = 3600; // 1 hour
      const stakeAmount = ethers.parseEther('1000');

      const [computeReward, totalReward, creatorPortion, providerPortion] =
        await computeNFT.calculateExpectedRewards(executionTime, stakeAmount);

      // Verify 70/30 split precision
      const expectedCreatorPortion = (stakeAmount * BigInt(70)) / BigInt(100);
      expect(creatorPortion).to.equal(expectedCreatorPortion);

      // Verify provider gets remainder (30% of stake + full compute reward)
      const expectedProviderPortion = totalReward - expectedCreatorPortion;
      expect(providerPortion).to.equal(expectedProviderPortion);

      // Verify no precision loss
      expect(creatorPortion + providerPortion).to.equal(totalReward);
    });
  });

  describe('Real Execution Flow Security', function () {
    let jobTokenId;

    beforeEach(async function () {
      // Create a compute job
      const tx = await computeNFT.connect(creator).createComputeJob(
        0, // ModelType.WeatherForecast
        'WRF-GFS-Ensemble',
        ethers.toUtf8Bytes('test input data'),
        '{"cpu": 4, "memory": "8GB"}',
        3600, // 1 hour
        ethers.parseEther('1000'),
        'ipfs://test-metadata'
      );

      const receipt = await tx.wait();
      jobTokenId = 1; // First token ID
    });

    it('Should securely calculate and distribute rewards during execution', async function () {
      // Track balances before execution
      const creatorBalanceBefore = await wrfToken.balanceOf(creator.address);
      const providerBalanceBefore = await wrfToken.balanceOf(provider1.address);

      // Execute job
      await expect(
        computeNFT
          .connect(provider1)
          .executeComputeJob(
            jobTokenId,
            ethers.toUtf8Bytes('computed results'),
            'ipfs://result-hash'
          )
      ).to.emit(computeNFT, 'RewardCalculationSecured');

      // Verify reward distribution
      const creatorBalanceAfter = await wrfToken.balanceOf(creator.address);
      const providerBalanceAfter = await wrfToken.balanceOf(provider1.address);

      const creatorReward = creatorBalanceAfter - creatorBalanceBefore;
      const providerReward = providerBalanceAfter - providerBalanceBefore;

      // Calculate expected rewards
      const stakeAmount = ethers.parseEther('1000');
      const executionTime = 3600;
      const expectedComputeReward =
        (BigInt(executionTime) * COMPUTE_REWARD_RATE) /
        BigInt(SECONDS_PER_HOUR);
      const expectedTotalReward = stakeAmount + expectedComputeReward;
      const expectedCreatorPortion =
        (stakeAmount * BigInt(CREATOR_PERCENTAGE)) / BigInt(PERCENTAGE_BASE);
      const expectedProviderPortion =
        expectedTotalReward - expectedCreatorPortion;

      expect(creatorReward).to.equal(expectedCreatorPortion);
      expect(providerReward).to.equal(expectedProviderPortion);
    });

    it('Should emit security events during reward calculations', async function () {
      await expect(
        computeNFT
          .connect(provider1)
          .executeComputeJob(
            jobTokenId,
            ethers.toUtf8Bytes('computed results'),
            'ipfs://result-hash'
          )
      )
        .to.emit(computeNFT, 'ArithmeticSecurityEvent')
        .withArgs(
          'COMPUTE_REWARD_CALCULATION',
          provider1.address,
          3600, // execution time
          COMPUTE_REWARD_RATE,
          COMPUTE_REWARD_RATE, // 1 hour = full rate
          'Compute reward calculated securely'
        );
    });

    it('Should handle failed job slashing securely', async function () {
      // Mock a failing job by providing invalid result data
      const providerStakeBefore = (
        await computeNFT.getProviderStats(provider1.address)
      ).stakedTokens;

      // Execute job with invalid results (empty data causes validation failure)
      await computeNFT.connect(provider1).executeComputeJob(
        jobTokenId,
        '0x', // Empty result data causes failure
        ''
      );

      const providerStakeAfter = (
        await computeNFT.getProviderStats(provider1.address)
      ).stakedTokens;
      const expectedSlash =
        (providerStakeBefore * BigInt(SLASH_PERCENTAGE)) /
        BigInt(PERCENTAGE_BASE);
      const expectedRemainingStake = providerStakeBefore - expectedSlash;

      expect(providerStakeAfter).to.equal(expectedRemainingStake);
    });
  });

  describe('Arithmetic Security Validation', function () {
    it('Should validate arithmetic security for reward calculations', async function () {
      const executionTime = 3600; // 1 hour
      const stakeAmount = ethers.parseEther('1000');

      const [isSecure, securityReport] =
        await computeNFT.validateRewardArithmetic(executionTime, stakeAmount);

      expect(isSecure).to.be.true;
      expect(securityReport).to.equal('All arithmetic operations are secure');
    });

    it('Should detect insecure parameter configurations', async function () {
      // Test with zero execution time
      let [isSecure, securityReport] =
        await computeNFT.validateRewardArithmetic(0, MIN_STAKE);

      expect(isSecure).to.be.false;
      expect(securityReport).to.equal('Zero values detected');

      // Test with excessive execution time
      [isSecure, securityReport] = await computeNFT.validateRewardArithmetic(
        MAX_EXECUTION_TIME + 1,
        MIN_STAKE
      );

      expect(isSecure).to.be.false;
      expect(securityReport).to.equal('Execution time exceeds maximum');
    });

    it('Should detect potential overflow conditions', async function () {
      // Test with extreme stake amount
      const extremeStake = ethers.parseEther('1000000000'); // Very large stake

      const [isSecure, securityReport] =
        await computeNFT.validateRewardArithmetic(3600, extremeStake);

      expect(isSecure).to.be.false;
      expect(securityReport).to.equal('Stake amount exceeds maximum');
    });
  });

  describe('Edge Cases and Attack Scenarios', function () {
    it('Should prevent reward manipulation through execution time', async function () {
      // Attacker tries to create job with manipulated execution time
      await expect(
        computeNFT.connect(attacker).createComputeJob(
          0, // ModelType.WeatherForecast
          'WRF-GFS-Ensemble',
          ethers.toUtf8Bytes('attack input'),
          '{"cpu": 1}',
          MAX_EXECUTION_TIME + 1, // Invalid execution time
          MIN_STAKE,
          'ipfs://attack-metadata'
        )
      ).to.be.revertedWith('SecureComputationalNFT: Execution time too long');
    });

    it('Should prevent stake amount manipulation', async function () {
      // Attacker tries to create job with invalid stake
      await expect(
        computeNFT.connect(attacker).createComputeJob(
          0, // ModelType.WeatherForecast
          'WRF-GFS-Ensemble',
          ethers.toUtf8Bytes('attack input'),
          '{"cpu": 1}',
          3600,
          MIN_STAKE - 1n, // Invalid stake amount
          'ipfs://attack-metadata'
        )
      ).to.be.revertedWith('SecureComputationalNFT: Stake below minimum');
    });

    it('Should handle complex reward distributions without precision loss', async function () {
      // Create job with unusual execution time
      const oddExecutionTime = 1337; // Unusual number
      const oddStakeAmount = ethers.parseEther('777.777');

      await wrfToken
        .connect(creator)
        .approve(await computeNFT.getAddress(), oddStakeAmount);

      const tokenId = await computeNFT
        .connect(creator)
        .createComputeJob.staticCall(
          0,
          'WRF-GFS-Ensemble',
          ethers.toUtf8Bytes('odd input'),
          '{"cpu": 1}',
          oddExecutionTime,
          oddStakeAmount,
          'ipfs://odd-metadata'
        );

      await computeNFT
        .connect(creator)
        .createComputeJob(
          0,
          'WRF-GFS-Ensemble',
          ethers.toUtf8Bytes('odd input'),
          '{"cpu": 1}',
          oddExecutionTime,
          oddStakeAmount,
          'ipfs://odd-metadata'
        );

      // Calculate expected rewards
      const [computeReward, totalReward, creatorPortion, providerPortion] =
        await computeNFT.calculateExpectedRewards(
          oddExecutionTime,
          oddStakeAmount
        );

      // Verify calculations are consistent
      expect(creatorPortion + providerPortion).to.equal(totalReward);
      expect(totalReward).to.equal(oddStakeAmount + computeReward);
    });

    it('Should maintain security under high-frequency operations', async function () {
      const jobs = [];

      // Create multiple jobs rapidly
      for (let i = 0; i < 5; i++) {
        const stakeAmount = ethers.parseEther((100 + i * 100).toString());
        const executionTime = 3600 + i * 1800; // Varying execution times

        await wrfToken
          .connect(creator)
          .approve(await computeNFT.getAddress(), stakeAmount);

        const tx = await computeNFT
          .connect(creator)
          .createComputeJob(
            0,
            'WRF-GFS-Ensemble',
            ethers.toUtf8Bytes(`input-${i}`),
            '{"cpu": 1}',
            executionTime,
            stakeAmount,
            `ipfs://metadata-${i}`
          );

        jobs.push({ tokenId: i + 2, stakeAmount, executionTime }); // Start from tokenId 2
      }

      // Execute all jobs and verify reward calculations
      for (const job of jobs) {
        const [
          expectedComputeReward,
          expectedTotalReward,
          expectedCreatorPortion,
          expectedProviderPortion,
        ] = await computeNFT.calculateExpectedRewards(
          job.executionTime,
          job.stakeAmount
        );

        await expect(
          computeNFT
            .connect(provider1)
            .executeComputeJob(
              job.tokenId,
              ethers.toUtf8Bytes(`results-${job.tokenId}`),
              `ipfs://results-${job.tokenId}`
            )
        )
          .to.emit(computeNFT, 'RewardCalculationSecured')
          .withArgs(
            job.tokenId,
            provider1.address,
            job.stakeAmount,
            expectedComputeReward,
            expectedCreatorPortion,
            expectedProviderPortion
          );
      }
    });

    it('Should handle gas optimization for reward calculations', async function () {
      const executionTime = 3600;
      const stakeAmount = ethers.parseEther('1000');

      // Test gas usage for reward calculation
      const gas = await computeNFT.calculateExpectedRewards.estimateGas(
        executionTime,
        stakeAmount
      );

      console.log(`Reward calculation gas usage: ${gas}`);
      expect(gas).to.be.below(100000); // Should be reasonably efficient
    });
  });

  describe('Provider Statistics Security', function () {
    it('Should securely update provider statistics with SafeMath', async function () {
      const tokenId = 1; // From beforeEach setup

      const statsBefore = await computeNFT.getProviderStats(provider1.address);

      // Execute successful job
      await computeNFT
        .connect(provider1)
        .executeComputeJob(
          tokenId,
          ethers.toUtf8Bytes('results'),
          'ipfs://results'
        );

      const statsAfter = await computeNFT.getProviderStats(provider1.address);

      // Verify statistics updated correctly
      expect(statsAfter.successfulJobs).to.equal(
        statsBefore.successfulJobs + 1n
      );
      expect(statsAfter.totalComputeTime).to.equal(
        statsBefore.totalComputeTime + 3600n
      );
      expect(statsAfter.failedJobs).to.equal(statsBefore.failedJobs);
    });

    it('Should handle failed job statistics securely', async function () {
      const tokenId = 1; // From beforeEach setup

      const statsBefore = await computeNFT.getProviderStats(provider1.address);

      // Execute failed job (empty result data)
      await computeNFT.connect(provider1).executeComputeJob(
        tokenId,
        '0x', // Empty result causes failure
        ''
      );

      const statsAfter = await computeNFT.getProviderStats(provider1.address);

      // Verify statistics updated correctly
      expect(statsAfter.failedJobs).to.equal(statsBefore.failedJobs + 1n);
      expect(statsAfter.successfulJobs).to.equal(statsBefore.successfulJobs);
      expect(statsAfter.totalComputeTime).to.equal(
        statsBefore.totalComputeTime
      ); // No time added for failed job
    });
  });
});
