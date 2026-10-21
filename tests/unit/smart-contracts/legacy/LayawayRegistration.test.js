const { expect } = require('chai');
const { ethers, upgrades } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('LayawayRegistration', function () {
  // Test constants
  const PERCENTAGE_PRECISION = 10000;
  const REQUIRED_STAKE = ethers.parseEther('1000'); // $1000 equivalent
  const EARNINGS_AMOUNT = ethers.parseEther('100'); // $100 equivalent
  const MULTI_METHOD_PERIOD = 30 * 24 * 60 * 60; // 30 days
  const SINGLE_METHOD_PERIOD = 60 * 24 * 60 * 60; // 60 days
  const UNVERIFIED_PERIOD = 90 * 24 * 60 * 60; // 90 days

  async function deployLayawayFixture() {
    const [
      owner,
      participant1,
      participant2,
      participant3,
      earningsDistributor,
    ] = await ethers.getSigners();

    // Deploy mock contracts
    const MockParticipantTypeRegistry = await ethers.getContractFactory(
      'MockParticipantTypeRegistry'
    );
    const MockDynamicStakeCalculator = await ethers.getContractFactory(
      'MockDynamicStakeCalculator'
    );
    const MockSocialVerification = await ethers.getContractFactory(
      'MockSocialVerification'
    );
    const MockWRFToken = await ethers.getContractFactory('MockWRFToken');
    const MockSlashingMechanism = await ethers.getContractFactory(
      'MockSlashingMechanism'
    );

    const participantTypeRegistry = await MockParticipantTypeRegistry.deploy();
    const stakeCalculator = await MockDynamicStakeCalculator.deploy();
    const socialVerification = await MockSocialVerification.deploy();
    const wrfToken = await MockWRFToken.deploy();
    const slashingMechanism = await MockSlashingMechanism.deploy();

    // Deploy LayawayRegistration with upgrades
    const LayawayRegistration = await ethers.getContractFactory(
      'LayawayRegistration'
    );
    const layawayRegistration = await upgrades.deployProxy(
      LayawayRegistration,
      [
        await participantTypeRegistry.getAddress(),
        await stakeCalculator.getAddress(),
        await socialVerification.getAddress(),
        await wrfToken.getAddress(),
        await slashingMechanism.getAddress(),
      ],
      { initializer: 'initialize' }
    );

    // Grant earnings distributor role
    const EARNINGS_DISTRIBUTOR_ROLE = ethers.keccak256(
      ethers.toUtf8Bytes('EARNINGS_DISTRIBUTOR_ROLE')
    );
    await layawayRegistration.grantRole(
      EARNINGS_DISTRIBUTOR_ROLE,
      earningsDistributor.address
    );

    // Setup mock responses
    await stakeCalculator.setRequiredStake(
      participant1.address,
      REQUIRED_STAKE
    );
    await stakeCalculator.setRequiredStake(
      participant2.address,
      REQUIRED_STAKE
    );
    await stakeCalculator.setRequiredStake(
      participant3.address,
      REQUIRED_STAKE
    );

    // Setup participant types
    await participantTypeRegistry.setParticipantInfo(
      participant1.address,
      true,
      true,
      false
    ); // Multi-method, verified
    await participantTypeRegistry.setParticipantInfo(
      participant2.address,
      false,
      true,
      false
    ); // Single-method, verified
    await participantTypeRegistry.setParticipantInfo(
      participant3.address,
      false,
      false,
      false
    ); // Unverified

    // Fund WRF token contract
    await wrfToken.mint(
      await layawayRegistration.getAddress(),
      ethers.parseEther('1000000')
    );

    return {
      layawayRegistration,
      participantTypeRegistry,
      stakeCalculator,
      socialVerification,
      wrfToken,
      slashingMechanism,
      owner,
      participant1,
      participant2,
      participant3,
      earningsDistributor,
    };
  }

  describe('Deployment', function () {
    it('Should initialize with correct configuration', async function () {
      const { layawayRegistration, participantTypeRegistry, stakeCalculator } =
        await loadFixture(deployLayawayFixture);

      expect(await layawayRegistration.participantTypeRegistry()).to.equal(
        await participantTypeRegistry.getAddress()
      );
      expect(await layawayRegistration.stakeCalculator()).to.equal(
        await stakeCalculator.getAddress()
      );

      // Check default configuration
      const config = await layawayRegistration.config();
      expect(config.multiMethodPeriod).to.equal(MULTI_METHOD_PERIOD);
      expect(config.singleMethodPeriod).to.equal(SINGLE_METHOD_PERIOD);
      expect(config.unverifiedPeriod).to.equal(UNVERIFIED_PERIOD);
      expect(config.slashingMultiplier).to.equal(20000); // 2x
    });
  });

  describe('Registration', function () {
    it('Should allow layaway registration with zero upfront cost', async function () {
      const { layawayRegistration, participant1 } =
        await loadFixture(deployLayawayFixture);

      const tx = await layawayRegistration
        .connect(participant1)
        .registerLayaway();
      const receipt = await tx.wait();

      // Check event emission
      const event = receipt.logs.find(
        log => log.fragment?.name === 'LayawayRegistered'
      );
      expect(event.args.participant).to.equal(participant1.address);
      expect(event.args.requiredStake).to.equal(REQUIRED_STAKE);

      // Check account creation
      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.participant).to.equal(participant1.address);
      expect(account.requiredStake).to.equal(REQUIRED_STAKE);
      expect(account.accumulatedStake).to.equal(0);
      expect(account.status).to.equal(0); // PROVISIONAL
      expect(account.minimumLayawayPeriod).to.equal(MULTI_METHOD_PERIOD); // Multi-method participant
    });

    it('Should calculate correct layaway periods for different participant types', async function () {
      const { layawayRegistration, participant1, participant2, participant3 } =
        await loadFixture(deployLayawayFixture);

      // Register different types
      await layawayRegistration.connect(participant1).registerLayaway(); // Multi-method
      await layawayRegistration.connect(participant2).registerLayaway(); // Single-method
      await layawayRegistration.connect(participant3).registerLayaway(); // Unverified

      const account1 = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      const account2 = await layawayRegistration.getLayawayAccount(
        participant2.address
      );
      const account3 = await layawayRegistration.getLayawayAccount(
        participant3.address
      );

      expect(account1.minimumLayawayPeriod).to.equal(MULTI_METHOD_PERIOD); // 30 days
      expect(account2.minimumLayawayPeriod).to.equal(SINGLE_METHOD_PERIOD); // 60 days
      expect(account3.minimumLayawayPeriod).to.equal(UNVERIFIED_PERIOD); // 90 days
    });

    it('Should prevent duplicate registration', async function () {
      const { layawayRegistration, participant1 } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      await expect(
        layawayRegistration.connect(participant1).registerLayaway()
      ).to.be.revertedWith('Already registered');
    });
  });

  describe('Earnings Processing', function () {
    it('Should allocate 100% of earnings to stake in first 25%', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      const tx = await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);
      const receipt = await tx.wait();

      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.accumulatedStake).to.equal(EARNINGS_AMOUNT); // 100% to stake
      expect(account.totalEarningsReceived).to.equal(EARNINGS_AMOUNT);

      // Check event
      const event = receipt.logs.find(
        log => log.fragment?.name === 'EarningsProcessed'
      );
      expect(event.args.allocatedToStake).to.equal(EARNINGS_AMOUNT);
      expect(event.args.allocatedToParticipant).to.equal(0);
    });

    it('Should allocate 75%/25% split in second 25% (25%-50%)', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Fill first 25% (250 WRF)
      const firstQuarter = REQUIRED_STAKE / 4n;
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, firstQuarter);

      // Process earnings in second quarter
      const tx = await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);
      const receipt = await tx.wait();

      // Should be 75% to stake, 25% to participant
      const expectedToStake = (EARNINGS_AMOUNT * 75n) / 100n;
      const expectedToParticipant = (EARNINGS_AMOUNT * 25n) / 100n;

      const event = receipt.logs.find(
        log => log.fragment?.name === 'EarningsProcessed'
      );
      expect(event.args.allocatedToStake).to.equal(expectedToStake);
      expect(event.args.allocatedToParticipant).to.equal(expectedToParticipant);
    });

    it('Should allocate 50%/50% split in third 25% (50%-75%)', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Fill first 50%
      const firstHalf = REQUIRED_STAKE / 2n;
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, firstHalf);

      // Process earnings in third quarter
      const tx = await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);
      const receipt = await tx.wait();

      // Should be 50% to stake, 50% to participant
      const expectedToStake = EARNINGS_AMOUNT / 2n;
      const expectedToParticipant = EARNINGS_AMOUNT / 2n;

      const event = receipt.logs.find(
        log => log.fragment?.name === 'EarningsProcessed'
      );
      expect(event.args.allocatedToStake).to.equal(expectedToStake);
      expect(event.args.allocatedToParticipant).to.equal(expectedToParticipant);
    });

    it('Should allocate 25%/75% split in final 25% (75%-100%)', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Fill first 75%
      const threeQuarters = (REQUIRED_STAKE * 3n) / 4n;
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, threeQuarters);

      // Process earnings in final quarter
      const tx = await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);
      const receipt = await tx.wait();

      // Should be 25% to stake, 75% to participant
      const expectedToStake = (EARNINGS_AMOUNT * 25n) / 100n;
      const expectedToParticipant = (EARNINGS_AMOUNT * 75n) / 100n;

      const event = receipt.logs.find(
        log => log.fragment?.name === 'EarningsProcessed'
      );
      expect(event.args.allocatedToStake).to.equal(expectedToStake);
      expect(event.args.allocatedToParticipant).to.equal(expectedToParticipant);
    });

    it('Should allocate 100% to participant after stake completion', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Fill required stake completely
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, REQUIRED_STAKE);

      // Advance time to meet minimum period
      await time.increase(MULTI_METHOD_PERIOD + 1);

      // Process additional earnings after completion
      const tx = await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);
      const receipt = await tx.wait();

      // Should be 0% to stake, 100% to participant
      const event = receipt.logs.find(
        log => log.fragment?.name === 'EarningsProcessed'
      );
      expect(event.args.allocatedToStake).to.equal(0);
      expect(event.args.allocatedToParticipant).to.equal(EARNINGS_AMOUNT);
    });

    it('Should enforce rate limiting', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // First processing should succeed
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);

      // Second processing immediately should fail
      await expect(
        layawayRegistration
          .connect(earningsDistributor)
          .processEarnings(participant1.address, EARNINGS_AMOUNT)
      ).to.be.revertedWith('Rate limited');
    });
  });

  describe('Stake Acceleration', function () {
    it('Should allow manual stake acceleration with ETH', async function () {
      const { layawayRegistration, participant1 } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Configure ETH as payment asset
      await layawayRegistration.configurePaymentAsset(
        0,
        ethers.ZeroAddress,
        true
      ); // ETH

      const accelerationAmount = ethers.parseEther('100');

      const tx = await layawayRegistration
        .connect(participant1)
        .accelerateStake(accelerationAmount, 0, { value: accelerationAmount }); // PaymentAsset.ETH = 0
      const receipt = await tx.wait();

      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.accumulatedStake).to.equal(accelerationAmount);

      // Check event
      const event = receipt.logs.find(
        log => log.fragment?.name === 'StakeAccelerated'
      );
      expect(event.args.participant).to.equal(participant1.address);
      expect(event.args.amount).to.equal(accelerationAmount);
    });

    it('Should track acceleration history', async function () {
      const { layawayRegistration, participant1 } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();
      await layawayRegistration.configurePaymentAsset(
        0,
        ethers.ZeroAddress,
        true
      );

      const amount1 = ethers.parseEther('100');
      const amount2 = ethers.parseEther('200');

      await layawayRegistration
        .connect(participant1)
        .accelerateStake(amount1, 0, { value: amount1 });
      await layawayRegistration
        .connect(participant1)
        .accelerateStake(amount2, 0, { value: amount2 });

      const history = await layawayRegistration.getAccelerationHistory(
        participant1.address
      );
      expect(history.length).to.equal(2);
      expect(history[0].amount).to.equal(amount1);
      expect(history[1].amount).to.equal(amount2);
    });

    it('Should not allow acceleration beyond required stake', async function () {
      const { layawayRegistration, participant1 } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();
      await layawayRegistration.configurePaymentAsset(
        0,
        ethers.ZeroAddress,
        true
      );

      // Try to accelerate more than required
      const excessAmount = REQUIRED_STAKE * 2n;

      const tx = await layawayRegistration
        .connect(participant1)
        .accelerateStake(excessAmount, 0, { value: excessAmount });

      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.accumulatedStake).to.equal(REQUIRED_STAKE); // Capped at required
    });
  });

  describe('Activation', function () {
    it('Should activate participant when stake complete and period met', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Complete stake through earnings
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, REQUIRED_STAKE);

      // Advance time to meet minimum period
      await time.increase(MULTI_METHOD_PERIOD + 1);

      // Process earnings to trigger activation
      const tx = await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, ethers.parseEther('1'));

      const receipt = await tx.wait();
      const activationEvent = receipt.logs.find(
        log => log.fragment?.name === 'ParticipantActivated'
      );
      expect(activationEvent).to.not.be.undefined;

      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.status).to.equal(1); // ACTIVE
      expect(account.activationTime).to.be.gt(0);
    });

    it('Should check activation eligibility', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Should not be eligible initially
      expect(
        await layawayRegistration.checkActivationEligibility(
          participant1.address
        )
      ).to.be.false;

      // Complete stake but not time period
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, REQUIRED_STAKE);
      expect(
        await layawayRegistration.checkActivationEligibility(
          participant1.address
        )
      ).to.be.false;

      // Meet time period
      await time.increase(MULTI_METHOD_PERIOD + 1);
      expect(
        await layawayRegistration.checkActivationEligibility(
          participant1.address
        )
      ).to.be.true;
    });
  });

  describe('Emergency Withdrawal', function () {
    it('Should allow emergency withdrawal after minimum period with penalty', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Accumulate some stake
      const partialStake = REQUIRED_STAKE / 2n;
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, partialStake);

      // Advance time to meet minimum period
      await time.increase(MULTI_METHOD_PERIOD + 1);

      const tx = await layawayRegistration
        .connect(participant1)
        .withdrawAccumulated();
      const receipt = await tx.wait();

      // Check penalty applied (10%)
      const expectedPenalty = (partialStake * 1000n) / 10000n; // 10%
      const expectedWithdrawal = partialStake - expectedPenalty;

      const event = receipt.logs.find(
        log => log.fragment?.name === 'EmergencyWithdrawal'
      );
      expect(event.args.amount).to.equal(expectedWithdrawal);
      expect(event.args.penalty).to.equal(expectedPenalty);

      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.emergencyExit).to.be.true;
      expect(account.status).to.equal(2); // SUSPENDED
    });

    it('Should not allow withdrawal before minimum period', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);

      await expect(
        layawayRegistration.connect(participant1).withdrawAccumulated()
      ).to.be.revertedWith('Minimum period not met');
    });
  });

  describe('Administrative Functions', function () {
    it('Should allow admin to update layaway configuration', async function () {
      const { layawayRegistration, owner } =
        await loadFixture(deployLayawayFixture);

      const newMultiMethodPeriod = 45 * 24 * 60 * 60; // 45 days
      const newSingleMethodPeriod = 75 * 24 * 60 * 60; // 75 days
      const newUnverifiedPeriod = 105 * 24 * 60 * 60; // 105 days
      const newSlashingMultiplier = 25000; // 2.5x

      await layawayRegistration.updateLayawayConfiguration(
        newMultiMethodPeriod,
        newSingleMethodPeriod,
        newUnverifiedPeriod,
        newSlashingMultiplier
      );

      const config = await layawayRegistration.config();
      expect(config.multiMethodPeriod).to.equal(newMultiMethodPeriod);
      expect(config.singleMethodPeriod).to.equal(newSingleMethodPeriod);
      expect(config.unverifiedPeriod).to.equal(newUnverifiedPeriod);
      expect(config.slashingMultiplier).to.equal(newSlashingMultiplier);
    });

    it('Should apply quality bonus to reduce layaway period', async function () {
      const { layawayRegistration, participant1, owner } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      const newMultiplier = 11000; // 110% quality multiplier
      await layawayRegistration.applyQualityBonus(
        participant1.address,
        newMultiplier
      );

      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.qualityMultiplier).to.equal(newMultiplier);
      // Period should be reduced
      expect(account.minimumLayawayPeriod).to.be.lt(MULTI_METHOD_PERIOD);
    });

    it('Should handle slashing events', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Accumulate some stake
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, EARNINGS_AMOUNT);

      // Handle slashing
      await layawayRegistration
        .connect(earningsDistributor)
        .handleSlashing(participant1.address);

      const account = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      expect(account.accumulatedStake).to.equal(0); // Reset
      expect(account.status).to.equal(2); // SUSPENDED
      expect(account.minimumLayawayPeriod).to.equal(MULTI_METHOD_PERIOD * 2); // 2x period
    });
  });

  describe('Bulk Operations', function () {
    it('Should process earnings for multiple participants in batch', async function () {
      const {
        layawayRegistration,
        participant1,
        participant2,
        earningsDistributor,
      } = await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();
      await layawayRegistration.connect(participant2).registerLayaway();

      const participants = [participant1.address, participant2.address];
      const earnings = [EARNINGS_AMOUNT, EARNINGS_AMOUNT];

      await layawayRegistration
        .connect(earningsDistributor)
        .bulkProcessEarnings(participants, earnings);

      const account1 = await layawayRegistration.getLayawayAccount(
        participant1.address
      );
      const account2 = await layawayRegistration.getLayawayAccount(
        participant2.address
      );

      expect(account1.accumulatedStake).to.equal(EARNINGS_AMOUNT);
      expect(account2.accumulatedStake).to.equal(EARNINGS_AMOUNT);
    });
  });

  describe('View Functions', function () {
    it('Should return correct stake progress', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // Accumulate 25% of required stake
      const quarterStake = REQUIRED_STAKE / 4n;
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, quarterStake);

      const progress = await layawayRegistration.getStakeProgress(
        participant1.address
      );
      expect(progress).to.equal(2500); // 25% * 100 (percentage precision)
    });

    it('Should preview earnings allocation correctly', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      // At 0% progress, should be 100% to stake
      const [toStake1, toParticipant1] =
        await layawayRegistration.previewEarningsAllocation(
          participant1.address,
          EARNINGS_AMOUNT
        );
      expect(toStake1).to.equal(EARNINGS_AMOUNT);
      expect(toParticipant1).to.equal(0);

      // Accumulate 50% and test again
      const halfStake = REQUIRED_STAKE / 2n;
      await layawayRegistration
        .connect(earningsDistributor)
        .processEarnings(participant1.address, halfStake);

      const [toStake2, toParticipant2] =
        await layawayRegistration.previewEarningsAllocation(
          participant1.address,
          EARNINGS_AMOUNT
        );
      expect(toStake2).to.equal(EARNINGS_AMOUNT / 2n); // 50%
      expect(toParticipant2).to.equal(EARNINGS_AMOUNT / 2n); // 50%
    });

    it('Should return system statistics', async function () {
      const { layawayRegistration, participant1, participant2 } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();
      await layawayRegistration.connect(participant2).registerLayaway();

      const [provisional, active, totalStake, totalEarnings] =
        await layawayRegistration.getSystemStats();

      expect(provisional).to.equal(2); // 2 provisional participants
      expect(active).to.equal(0); // 0 active participants
    });
  });

  describe('Security', function () {
    it('Should only allow earnings distributor to process earnings', async function () {
      const { layawayRegistration, participant1, participant2 } =
        await loadFixture(deployLayawayFixture);

      await layawayRegistration.connect(participant1).registerLayaway();

      await expect(
        layawayRegistration
          .connect(participant2)
          .processEarnings(participant1.address, EARNINGS_AMOUNT)
      ).to.be.revertedWith(/AccessControl: account .* is missing role/);
    });

    it('Should only allow admin to update configuration', async function () {
      const { layawayRegistration, participant1 } =
        await loadFixture(deployLayawayFixture);

      await expect(
        layawayRegistration
          .connect(participant1)
          .updateLayawayConfiguration(
            30 * 24 * 60 * 60,
            60 * 24 * 60 * 60,
            90 * 24 * 60 * 60,
            20000
          )
      ).to.be.revertedWith(/AccessControl: account .* is missing role/);
    });

    it('Should prevent processing for unregistered participants', async function () {
      const { layawayRegistration, participant1, earningsDistributor } =
        await loadFixture(deployLayawayFixture);

      await expect(
        layawayRegistration
          .connect(earningsDistributor)
          .processEarnings(participant1.address, EARNINGS_AMOUNT)
      ).to.be.revertedWith('Not registered');
    });
  });

  describe('Pause Functionality', function () {
    it('Should pause and unpause contract operations', async function () {
      const { layawayRegistration, participant1, owner } =
        await loadFixture(deployLayawayFixture);

      // Pause contract
      const EMERGENCY_ROLE = ethers.keccak256(
        ethers.toUtf8Bytes('EMERGENCY_ROLE')
      );
      await layawayRegistration.emergencyPause();

      // Should not allow registration when paused
      await expect(
        layawayRegistration.connect(participant1).registerLayaway()
      ).to.be.revertedWith('Pausable: paused');

      // Unpause
      await layawayRegistration.emergencyUnpause();

      // Should work after unpause
      await expect(layawayRegistration.connect(participant1).registerLayaway())
        .to.not.be.reverted;
    });
  });
});

// Mock contract implementations for testing
// These would be deployed separately for the tests

// Mock contracts would be implemented here or in separate files
// For brevity, I'm showing the structure but not implementing full mocks
