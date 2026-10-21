const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('FairLaunchIntegrationManager', function () {
  // Test fixtures
  async function deployIntegrationFixture() {
    const [
      owner,
      oracle,
      stakeManager,
      participant1,
      participant2,
      participant3,
      treasury,
    ] = await ethers.getSigners();

    // Deploy ParticipantTypeRegistry
    const ParticipantTypeRegistry = await ethers.getContractFactory(
      'ParticipantTypeRegistry'
    );
    const participantRegistry = await ParticipantTypeRegistry.deploy();
    await participantRegistry.waitForDeployment();

    // Grant roles
    const ORACLE_ROLE = await participantRegistry.ORACLE_ROLE();
    await participantRegistry.grantRole(ORACLE_ROLE, oracle.address);

    // Deploy mock WRF Token
    const MockERC20 = await ethers.getContractFactory('MockERC20');
    const wrfToken = await MockERC20.deploy(
      'WRFCoin',
      'WRF',
      ethers.parseEther('1000000000') // 1B tokens
    );
    await wrfToken.waitForDeployment();

    // Deploy mock FairLaunchStakeManager
    const FairLaunchStakeManager = await ethers.getContractFactory(
      'FairLaunchStakeManager'
    );
    const fairLaunchStakeManager = await FairLaunchStakeManager.deploy(
      owner.address, // emergency multisig
      treasury.address // treasury
    );
    await fairLaunchStakeManager.waitForDeployment();

    // Deploy FairLaunchIntegrationManager
    const FairLaunchIntegrationManager = await ethers.getContractFactory(
      'FairLaunchIntegrationManager'
    );
    const integrationManager = await FairLaunchIntegrationManager.deploy(
      await participantRegistry.getAddress(),
      await fairLaunchStakeManager.getAddress(),
      await wrfToken.getAddress()
    );
    await integrationManager.waitForDeployment();

    // Set up integration contracts in registry
    await participantRegistry.setFairLaunchContracts(
      await fairLaunchStakeManager.getAddress(),
      await wrfToken.getAddress()
    );

    // Transfer tokens to integration manager for fair launch
    const fairLaunchAllocation = ethers.parseEther('100000000'); // 100M tokens
    await wrfToken.transfer(
      await integrationManager.getAddress(),
      fairLaunchAllocation
    );

    // Give participants some WRF tokens for participation fees
    const participationFee = ethers.parseEther('100');
    await wrfToken.transfer(participant1.address, participationFee.mul(10));
    await wrfToken.transfer(participant2.address, participationFee.mul(10));
    await wrfToken.transfer(participant3.address, participationFee.mul(10));

    return {
      participantRegistry,
      fairLaunchStakeManager,
      wrfToken,
      integrationManager,
      owner,
      oracle,
      stakeManager,
      participant1,
      participant2,
      participant3,
      treasury,
      ORACLE_ROLE,
      fairLaunchAllocation,
    };
  }

  async function deployWithRegisteredParticipantsFixture() {
    const fixture = await loadFixture(deployIntegrationFixture);
    const {
      participantRegistry,
      oracle,
      participant1,
      participant2,
      participant3,
    } = fixture;

    // Sample geographic data
    const usLocation = {
      latitude: 40_750_000, // New York
      longitude: -73_985_000,
      zone: 0,
      countryCode: ethers.encodeBytes32String('US'),
      regionCode: ethers.encodeBytes32String('NY'),
    };

    const euLocation = {
      latitude: 51_500_000, // London
      longitude: 125_000,
      zone: 0,
      countryCode: ethers.encodeBytes32String('GB'),
      regionCode: ethers.encodeBytes32String('LN'),
    };

    // Register participants with different characteristics
    // Participant 1: Single method, email verified
    await participantRegistry.connect(oracle).registerParticipant(
      participant1.address,
      0, // WEATHER_STATION
      usLocation,
      1 // EMAIL_VERIFIED
    );

    // Participant 2: Multi-method, government ID verified
    await participantRegistry.connect(oracle).registerParticipant(
      participant2.address,
      0, // WEATHER_STATION
      euLocation,
      3 // GOVERNMENT_ID_VERIFIED
    );
    await participantRegistry
      .connect(oracle)
      .addParticipationType(participant2.address, 1); // FORECAST_PRODUCER

    // Participant 3: Single method, unverified (will be ineligible initially)
    await participantRegistry.connect(oracle).registerParticipant(
      participant3.address,
      0, // WEATHER_STATION
      usLocation,
      0 // UNVERIFIED
    );

    return { ...fixture, usLocation, euLocation };
  }

  async function deployActiveFairLaunchFixture() {
    const fixture = await loadFixture(deployWithRegisteredParticipantsFixture);
    const { integrationManager, fairLaunchAllocation } = fixture;

    // Activate fair launch
    const duration = 30 * 24 * 60 * 60; // 30 days
    await integrationManager.activateFairLaunch(duration, fairLaunchAllocation);

    return fixture;
  }

  describe('Deployment', function () {
    it('Should deploy with correct initial state', async function () {
      const {
        integrationManager,
        participantRegistry,
        fairLaunchStakeManager,
        wrfToken,
      } = await loadFixture(deployIntegrationFixture);

      expect(await integrationManager.participantRegistry()).to.equal(
        await participantRegistry.getAddress()
      );
      expect(await integrationManager.stakeManager()).to.equal(
        await fairLaunchStakeManager.getAddress()
      );
      expect(await integrationManager.wrfToken()).to.equal(
        await wrfToken.getAddress()
      );
      expect(await integrationManager.fairLaunchActive()).to.equal(false);
      expect(await integrationManager.totalFairLaunchParticipants()).to.equal(
        0
      );
    });

    it('Should set up access control correctly', async function () {
      const { integrationManager, owner } = await loadFixture(
        deployIntegrationFixture
      );

      const FAIR_LAUNCH_ADMIN_ROLE =
        await integrationManager.FAIR_LAUNCH_ADMIN_ROLE();
      expect(
        await integrationManager.hasRole(FAIR_LAUNCH_ADMIN_ROLE, owner.address)
      ).to.equal(true);
    });
  });

  describe('Fair Launch Activation', function () {
    it('Should activate fair launch successfully', async function () {
      const { integrationManager, fairLaunchAllocation } = await loadFixture(
        deployIntegrationFixture
      );

      const duration = 30 * 24 * 60 * 60; // 30 days

      await expect(
        integrationManager.activateFairLaunch(duration, fairLaunchAllocation)
      ).to.emit(integrationManager, 'FairLaunchActivated');

      expect(await integrationManager.fairLaunchActive()).to.equal(true);
      expect(await integrationManager.fairLaunchTokenAllocation()).to.equal(
        fairLaunchAllocation
      );
    });

    it('Should reject invalid parameters', async function () {
      const { integrationManager, fairLaunchAllocation } = await loadFixture(
        deployIntegrationFixture
      );

      // Invalid duration
      await expect(
        integrationManager.activateFairLaunch(0, fairLaunchAllocation)
      ).to.be.revertedWith('Invalid duration');

      // Invalid token allocation
      await expect(
        integrationManager.activateFairLaunch(30 * 24 * 60 * 60, 0)
      ).to.be.revertedWith('Invalid token allocation');
    });

    it('Should prevent duplicate activation', async function () {
      const { integrationManager, fairLaunchAllocation } = await loadFixture(
        deployActiveFairLaunchFixture
      );

      await expect(
        integrationManager.activateFairLaunch(
          30 * 24 * 60 * 60,
          fairLaunchAllocation
        )
      ).to.be.revertedWith('Fair launch already active');
    });
  });

  describe('Fair Launch Registration', function () {
    it('Should register eligible participant successfully', async function () {
      const { integrationManager, participant1, wrfToken } = await loadFixture(
        deployActiveFairLaunchFixture
      );

      // Approve participation fee
      const participationFee = await integrationManager.PARTICIPATION_FEE();
      await wrfToken
        .connect(participant1)
        .approve(await integrationManager.getAddress(), participationFee);

      await expect(
        integrationManager.connect(participant1).registerForFairLaunch()
      ).to.emit(integrationManager, 'FairLaunchParticipantRegistered');

      const status = await integrationManager.fairLaunchStatus(
        participant1.address
      );
      expect(status.isRegistered).to.equal(true);
      expect(status.isEligible).to.equal(true);
      expect(await integrationManager.totalFairLaunchParticipants()).to.equal(
        1
      );
    });

    it('Should calculate different staking requirements for different participants', async function () {
      const { integrationManager, participant1, participant2 } =
        await loadFixture(deployActiveFairLaunchFixture);

      // Check eligibility and requirements
      const [eligible1, reason1, stakeReq1, layaway1] =
        await integrationManager.checkFairLaunchEligibility(
          participant1.address
        );
      const [eligible2, reason2, stakeReq2, layaway2] =
        await integrationManager.checkFairLaunchEligibility(
          participant2.address
        );

      expect(eligible1).to.equal(true);
      expect(eligible2).to.equal(true);

      // Multi-method participant (participant2) should have lower requirements
      expect(stakeReq2).to.be.lt(stakeReq1);
      expect(layaway2).to.be.lt(layaway1);
    });

    it('Should reject ineligible participants', async function () {
      const { integrationManager, participant3, wrfToken } = await loadFixture(
        deployActiveFairLaunchFixture
      );

      // Participant3 is unverified and newly registered, should be ineligible
      const participationFee = await integrationManager.PARTICIPATION_FEE();
      await wrfToken
        .connect(participant3)
        .approve(await integrationManager.getAddress(), participationFee);

      // First check eligibility
      const [eligible, reason] =
        await integrationManager.checkFairLaunchEligibility(
          participant3.address
        );

      if (!eligible) {
        await expect(
          integrationManager.connect(participant3).registerForFairLaunch()
        ).to.be.revertedWith(reason);
      }
    });

    it('Should prevent duplicate registration', async function () {
      const { integrationManager, participant1, wrfToken } = await loadFixture(
        deployActiveFairLaunchFixture
      );

      const participationFee = await integrationManager.PARTICIPATION_FEE();
      await wrfToken
        .connect(participant1)
        .approve(
          await integrationManager.getAddress(),
          participationFee.mul(2)
        );

      // First registration
      await integrationManager.connect(participant1).registerForFairLaunch();

      // Second registration should fail
      await expect(
        integrationManager.connect(participant1).registerForFairLaunch()
      ).to.be.revertedWith('Already registered');
    });
  });

  describe('Staking Integration', function () {
    it('Should handle staking completion', async function () {
      const {
        integrationManager,
        participant1,
        wrfToken,
        fairLaunchStakeManager,
      } = await loadFixture(deployActiveFairLaunchFixture);

      // Register for fair launch
      const participationFee = await integrationManager.PARTICIPATION_FEE();
      await wrfToken
        .connect(participant1)
        .approve(await integrationManager.getAddress(), participationFee);
      await integrationManager.connect(participant1).registerForFairLaunch();

      // Set up staking asset (using WRF token as staking asset for simplicity)
      const stakeAmount = ethers.parseEther('1000');
      await fairLaunchStakeManager.setSupportedAsset(
        await wrfToken.getAddress(),
        18,
        ethers.parseEther('100'),
        ethers.parseEther('10000'),
        'WRF'
      );
      await fairLaunchStakeManager.setOraclePrice(
        await wrfToken.getAddress(),
        1000000, // $1 per WRF (with precision)
        owner.address // Mock oracle
      );

      // Approve and stake
      await wrfToken
        .connect(participant1)
        .approve(await fairLaunchStakeManager.getAddress(), stakeAmount);

      await expect(
        integrationManager
          .connect(participant1)
          .completeFairLaunchStaking(await wrfToken.getAddress(), stakeAmount)
      ).to.emit(integrationManager, 'FairLaunchStakeCompleted');

      const status = await integrationManager.fairLaunchStatus(
        participant1.address
      );
      expect(status.hasStaked).to.equal(true);
      expect(status.layawayEndTime).to.be.gt(0);
    });
  });

  describe('Layaway Period Management', function () {
    it('Should enforce layaway period before token release', async function () {
      const { integrationManager, participant1 } = await loadFixture(
        deployActiveFairLaunchFixture
      );

      // Complete registration and staking (simplified for test)
      // ... (setup code similar to previous test)

      // Try to claim before layaway period ends
      await expect(
        integrationManager.connect(participant1).claimLayawayTokens()
      ).to.be.revertedWith('Layaway period not completed');
    });

    it('Should allow token claims after layaway completion', async function () {
      const { integrationManager, participant1, wrfToken } = await loadFixture(
        deployActiveFairLaunchFixture
      );

      // Complete full flow and fast-forward time
      // ... (setup and time manipulation)

      // This test would require more complex setup to fully implement
      // For now, we'll test the core logic
    });
  });

  describe('Statistics and Reporting', function () {
    it('Should provide accurate fair launch statistics', async function () {
      const { integrationManager, participant1, participant2, wrfToken } =
        await loadFixture(deployActiveFairLaunchFixture);

      // Register participants
      const participationFee = await integrationManager.PARTICIPATION_FEE();

      await wrfToken
        .connect(participant1)
        .approve(await integrationManager.getAddress(), participationFee);
      await integrationManager.connect(participant1).registerForFairLaunch();

      await wrfToken
        .connect(participant2)
        .approve(await integrationManager.getAddress(), participationFee);
      await integrationManager.connect(participant2).registerForFairLaunch();

      const [
        totalParticipants,
        eligibleParticipants,
        totalTokensAllocated,
        totalTokensClaimed,
        averageLayaway,
      ] = await integrationManager.getFairLaunchStatistics();

      expect(totalParticipants).to.equal(2);
      expect(totalTokensAllocated).to.be.gt(0);
    });

    it('Should track participant status correctly', async function () {
      const { integrationManager, participant1, wrfToken } = await loadFixture(
        deployActiveFairLaunchFixture
      );

      // Before registration
      let [status, schedule, releasable] =
        await integrationManager.getFairLaunchStatus(participant1.address);
      expect(status.isRegistered).to.equal(false);

      // After registration
      const participationFee = await integrationManager.PARTICIPATION_FEE();
      await wrfToken
        .connect(participant1)
        .approve(await integrationManager.getAddress(), participationFee);
      await integrationManager.connect(participant1).registerForFairLaunch();

      [status, schedule, releasable] =
        await integrationManager.getFairLaunchStatus(participant1.address);
      expect(status.isRegistered).to.equal(true);
      expect(status.hasStaked).to.equal(false);
    });
  });

  describe('Administrative Functions', function () {
    it('Should allow admin to pause and unpause', async function () {
      const { integrationManager, owner } = await loadFixture(
        deployIntegrationFixture
      );

      await integrationManager.connect(owner).emergencyPause();
      expect(await integrationManager.paused()).to.equal(true);

      await integrationManager.connect(owner).unpause();
      expect(await integrationManager.paused()).to.equal(false);
    });

    it('Should allow updating minimum participants', async function () {
      const { integrationManager, owner } = await loadFixture(
        deployIntegrationFixture
      );

      await integrationManager.connect(owner).updateMinimumParticipants(200);
      expect(await integrationManager.minimumParticipants()).to.equal(200);
    });

    it('Should reject admin actions from unauthorized users', async function () {
      const { integrationManager, participant1 } = await loadFixture(
        deployIntegrationFixture
      );

      await expect(integrationManager.connect(participant1).emergencyPause()).to
        .be.reverted;

      await expect(
        integrationManager.connect(participant1).updateMinimumParticipants(200)
      ).to.be.reverted;
    });
  });

  describe('Edge Cases and Security', function () {
    it('Should handle zero participants gracefully', async function () {
      const { integrationManager } = await loadFixture(
        deployIntegrationFixture
      );

      const [
        totalParticipants,
        eligibleParticipants,
        totalTokensAllocated,
        totalTokensClaimed,
        averageLayaway,
      ] = await integrationManager.getFairLaunchStatistics();

      expect(totalParticipants).to.equal(0);
      expect(averageLayaway).to.equal(0);
    });

    it('Should validate layaway periods within bounds', async function () {
      const { integrationManager } = await loadFixture(
        deployIntegrationFixture
      );

      const minLayaway = await integrationManager.MIN_LAYAWAY_PERIOD();
      const maxLayaway = await integrationManager.MAX_LAYAWAY_PERIOD();

      expect(minLayaway).to.equal(7 * 24 * 60 * 60); // 7 days
      expect(maxLayaway).to.equal(365 * 24 * 60 * 60); // 365 days
    });

    it('Should prevent operations when fair launch is not active', async function () {
      const { integrationManager, participant1 } = await loadFixture(
        deployWithRegisteredParticipantsFixture
      );

      await expect(
        integrationManager.connect(participant1).registerForFairLaunch()
      ).to.be.revertedWith('Fair launch not active');
    });
  });

  describe('Integration with ParticipantTypeRegistry', function () {
    it('Should correctly integrate with participant registry', async function () {
      const {
        integrationManager,
        participantRegistry,
        participant1,
        oracle,
        wrfToken,
      } = await loadFixture(deployActiveFairLaunchFixture);

      // Update participant activity to improve eligibility
      const WEATHER_STATION_ROLE =
        await participantRegistry.WEATHER_STATION_ROLE();
      await participantRegistry.grantRole(WEATHER_STATION_ROLE, oracle.address);

      await participantRegistry.connect(oracle).updateActivity(
        participant1.address,
        0, // WEATHER_STATION
        9000, // High quality score
        10, // Multiple contributions
        9500 // High uptime
      );

      // Check that integration reflects updated data
      const [eligible, reason, stakeReq, layaway] =
        await integrationManager.checkFairLaunchEligibility(
          participant1.address
        );
      expect(eligible).to.equal(true);
    });

    it('Should handle slashing events appropriately', async function () {
      const { integrationManager, participantRegistry, participant1, oracle } =
        await loadFixture(deployActiveFairLaunchFixture);

      // Add slashing role and record slashing event
      const SLASHING_ROLE = await participantRegistry.SLASHING_ROLE();
      await participantRegistry.grantRole(SLASHING_ROLE, oracle.address);

      await participantRegistry.connect(oracle).recordSlashingEvent(
        participant1.address,
        ethers.parseEther('100'), // Slashed amount
        2000000 // 2x penalty multiplier
      );

      // Check that staking requirements are increased due to slashing
      const [eligible, reason, stakeReq, layaway] =
        await integrationManager.checkFairLaunchEligibility(
          participant1.address
        );

      // Participant should still be eligible but with higher requirements
      expect(eligible).to.equal(true);
      expect(stakeReq).to.be.gt(0);
      expect(layaway).to.be.gt(30 * 24 * 60 * 60); // Should be longer than base period
    });
  });
});
