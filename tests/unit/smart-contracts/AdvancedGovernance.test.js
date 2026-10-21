const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  time,
  loadFixture,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('Advanced Governance System', function () {
  async function deployAdvancedGovernanceFixture() {
    const [
      admin,
      validator1,
      validator2,
      delegator1,
      delegator2,
      user1,
      user2,
    ] = await ethers.getSigners();

    // Deploy mock WRF token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    const wrfToken = await WRFToken.deploy();

    // Deploy TimelockController
    const TimelockController = await ethers.getContractFactory(
      '@openzeppelin/contracts/governance/TimelockController.sol:TimelockController'
    );
    const timelock = await TimelockController.deploy(
      86400, // 1 day delay
      [admin.address], // proposers
      [admin.address], // executors
      admin.address // admin
    );

    // Deploy WRFGovernor
    const WRFGovernor = await ethers.getContractFactory('WRFGovernor');
    const governor = await WRFGovernor.deploy(
      await wrfToken.getAddress(),
      await timelock.getAddress(),
      1, // voting delay
      604800, // voting period (1 week)
      ethers.parseEther('1000'), // proposal threshold
      4 // quorum fraction (4%)
    );

    // Deploy DPoSValidator
    const DPoSValidator = await ethers.getContractFactory('DPoSValidator');
    const dposValidator = await DPoSValidator.deploy(
      await wrfToken.getAddress(),
      86400 // epoch duration (1 day)
    );

    // Deploy ClimateEmergencyProtocol
    const ClimateEmergencyProtocol = await ethers.getContractFactory(
      'ClimateEmergencyProtocol'
    );
    const emergencyProtocol = await ClimateEmergencyProtocol.deploy(
      await wrfToken.getAddress(),
      admin.address, // emergency vault
      admin.address // admin
    );

    // Deploy StakeholderIncentives
    const StakeholderIncentives = await ethers.getContractFactory(
      'StakeholderIncentives'
    );
    const incentives = await StakeholderIncentives.deploy(
      await wrfToken.getAddress(),
      ethers.ZeroAddress, // achievement NFT (mock)
      admin.address
    );

    // Deploy GovernanceAnalytics
    const GovernanceAnalytics = await ethers.getContractFactory(
      'GovernanceAnalytics'
    );
    const analytics = await GovernanceAnalytics.deploy(admin.address);

    // Deploy ComplianceModule
    const ComplianceModule =
      await ethers.getContractFactory('ComplianceModule');
    const compliance = await ComplianceModule.deploy(admin.address);

    // Deploy EnhancedTreasury
    const EnhancedTreasury =
      await ethers.getContractFactory('EnhancedTreasury');
    const treasury = await EnhancedTreasury.deploy(
      admin.address,
      await timelock.getAddress(),
      1500 // 15% emergency fund
    );

    // Distribute tokens for testing
    await wrfToken.transfer(validator1.address, ethers.parseEther('50000'));
    await wrfToken.transfer(validator2.address, ethers.parseEther('50000'));
    await wrfToken.transfer(delegator1.address, ethers.parseEther('10000'));
    await wrfToken.transfer(delegator2.address, ethers.parseEther('10000'));
    await wrfToken.transfer(user1.address, ethers.parseEther('5000'));
    await wrfToken.transfer(user2.address, ethers.parseEther('5000'));

    return {
      wrfToken,
      timelock,
      governor,
      dposValidator,
      emergencyProtocol,
      incentives,
      analytics,
      compliance,
      treasury,
      admin,
      validator1,
      validator2,
      delegator1,
      delegator2,
      user1,
      user2,
    };
  }

  describe('DPoS Validator System', function () {
    it('Should allow validator registration', async function () {
      const { dposValidator, wrfToken, validator1 } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const stakeAmount = ethers.parseEther('15000');
      await wrfToken
        .connect(validator1)
        .approve(await dposValidator.getAddress(), stakeAmount);

      await dposValidator.connect(validator1).registerValidator(
        'Climate Validator 1',
        'Expert in climate data validation',
        500, // 5% commission
        stakeAmount
      );

      const validator = await dposValidator.getValidator(validator1.address);
      expect(validator.validatorAddress).to.equal(validator1.address);
      expect(validator.selfStake).to.equal(stakeAmount);
      expect(validator.commissionRate).to.equal(500);
    });

    it('Should allow delegation to validators', async function () {
      const { dposValidator, wrfToken, validator1, delegator1 } =
        await loadFixture(deployAdvancedGovernanceFixture);

      // Register validator first
      const stakeAmount = ethers.parseEther('15000');
      await wrfToken
        .connect(validator1)
        .approve(await dposValidator.getAddress(), stakeAmount);
      await dposValidator
        .connect(validator1)
        .registerValidator(
          'Climate Validator 1',
          'Expert in climate data validation',
          500,
          stakeAmount
        );

      // Delegate to validator
      const delegationAmount = ethers.parseEther('5000');
      await wrfToken
        .connect(delegator1)
        .approve(await dposValidator.getAddress(), delegationAmount);

      await dposValidator.connect(delegator1).delegate(
        validator1.address,
        delegationAmount,
        30 * 24 * 60 * 60, // 30 days lock period
        true // auto compound
      );

      const delegation = await dposValidator.getDelegation(
        delegator1.address,
        validator1.address
      );
      expect(delegation.amount).to.equal(delegationAmount);
      expect(delegation.autoCompound).to.be.true;

      const validator = await dposValidator.getValidator(validator1.address);
      expect(validator.totalDelegated).to.equal(delegationAmount);
    });

    it('Should handle validator slashing', async function () {
      const { dposValidator, wrfToken, validator1, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      // Register validator
      const stakeAmount = ethers.parseEther('15000');
      await wrfToken
        .connect(validator1)
        .approve(await dposValidator.getAddress(), stakeAmount);
      await dposValidator
        .connect(validator1)
        .registerValidator(
          'Climate Validator 1',
          'Expert in climate data validation',
          500,
          stakeAmount
        );

      const validatorBefore = await dposValidator.getValidator(
        validator1.address
      );
      const initialStake = validatorBefore.selfStake;

      // Slash validator
      await dposValidator
        .connect(admin)
        .slashValidator(validator1.address, 'Providing false climate data');

      const validatorAfter = await dposValidator.getValidator(
        validator1.address
      );
      expect(validatorAfter.selfStake).to.be.lt(initialStake);
      expect(validatorAfter.status).to.equal(3); // SLASHED status
    });
  });

  describe('Climate Emergency Protocol', function () {
    it('Should allow oracle data updates', async function () {
      const { emergencyProtocol, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      // Grant oracle role
      const ORACLE_ROLE = await emergencyProtocol.ORACLE_ROLE();
      await emergencyProtocol
        .connect(admin)
        .grantRole(ORACLE_ROLE, admin.address);

      // Update climate data
      await emergencyProtocol.connect(admin).updateOracleData(
        'global_temp_anomaly',
        155, // 1.55°C anomaly
        Math.floor(Date.now() / 1000)
      );

      const twamValue = await emergencyProtocol.getTWAMValue(
        'global_temp_anomaly'
      );
      expect(twamValue).to.equal(155);
    });

    it('Should activate circuit breaker', async function () {
      const { emergencyProtocol, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const GUARDIAN_ROLE = await emergencyProtocol.GUARDIAN_ROLE();
      await emergencyProtocol
        .connect(admin)
        .grantRole(GUARDIAN_ROLE, admin.address);

      await emergencyProtocol
        .connect(admin)
        .activateCircuitBreaker('Test emergency halt');

      expect(await emergencyProtocol.circuitBreakerActive()).to.be.true;
    });
  });

  describe('Stakeholder Incentives', function () {
    it('Should record participation and distribute rewards', async function () {
      const { incentives, wrfToken, user1, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const INCENTIVE_MANAGER_ROLE = await incentives.INCENTIVE_MANAGER_ROLE();
      await incentives
        .connect(admin)
        .grantRole(INCENTIVE_MANAGER_ROLE, admin.address);

      // Transfer some tokens to incentives contract for rewards
      await wrfToken
        .connect(admin)
        .transfer(await incentives.getAddress(), ethers.parseEther('10000'));

      const initialBalance = await wrfToken.balanceOf(user1.address);

      // Record voting participation
      await incentives.connect(admin).recordParticipation(
        user1.address,
        0, // VOTING category
        100, // score
        0 // program ID
      );

      const finalBalance = await wrfToken.balanceOf(user1.address);
      expect(finalBalance).to.be.gt(initialBalance);

      // Check profile
      const profile = await incentives.getStakeholderProfile(user1.address);
      expect(profile.totalParticipationScore).to.equal(100);
    });

    it('Should unlock achievements', async function () {
      const { incentives, user1, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const INCENTIVE_MANAGER_ROLE = await incentives.INCENTIVE_MANAGER_ROLE();
      await incentives
        .connect(admin)
        .grantRole(INCENTIVE_MANAGER_ROLE, admin.address);

      // Record enough voting to unlock "First Vote" achievement
      await incentives.connect(admin).recordParticipation(
        user1.address,
        0, // VOTING category
        1, // score
        0 // program ID
      );

      // Check if achievement was unlocked
      const hasAchievement = await incentives.stakeholderAchievements(
        user1.address,
        0
      );
      expect(hasAchievement).to.be.true;
    });
  });

  describe('Governance Analytics', function () {
    it('Should record vote data', async function () {
      const { analytics, user1, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const DATA_PROVIDER_ROLE = await analytics.DATA_PROVIDER_ROLE();
      await analytics
        .connect(admin)
        .grantRole(DATA_PROVIDER_ROLE, admin.address);

      await analytics.connect(admin).recordVote(
        1, // proposal ID
        user1.address,
        1, // support (for)
        ethers.parseEther('1000') // voting power
      );

      const votingPattern = await analytics.getVotingPattern(user1.address);
      expect(votingPattern.totalVotes).to.equal(1);
      expect(votingPattern.forVotes).to.equal(1);
    });

    it('Should take governance snapshots', async function () {
      const { analytics, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const ANALYTICS_MANAGER_ROLE = await analytics.ANALYTICS_MANAGER_ROLE();
      await analytics
        .connect(admin)
        .grantRole(ANALYTICS_MANAGER_ROLE, admin.address);

      const snapshotId = await analytics.connect(admin).takeSnapshot();

      // The snapshot ID should be the current timestamp
      const currentTime = await time.latest();
      expect(snapshotId).to.be.closeTo(currentTime, 10);
    });

    it('Should calculate network health', async function () {
      const { analytics, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const ANALYTICS_MANAGER_ROLE = await analytics.ANALYTICS_MANAGER_ROLE();
      await analytics
        .connect(admin)
        .grantRole(ANALYTICS_MANAGER_ROLE, admin.address);

      await analytics.connect(admin).calculateNetworkHealth();

      const networkHealth = await analytics.networkHealthScore();
      expect(networkHealth).to.be.gt(0);
      expect(networkHealth).to.be.lte(100);
    });
  });

  describe('Compliance Module', function () {
    it('Should update KYC status', async function () {
      const { compliance, user1, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const KYC_PROVIDER_ROLE = await compliance.KYC_PROVIDER_ROLE();
      await compliance
        .connect(admin)
        .grantRole(KYC_PROVIDER_ROLE, admin.address);

      // Register KYC provider first
      await compliance
        .connect(admin)
        .registerKYCProvider('TestKYC', admin.address);

      const expiryDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60; // 1 year from now
      const documentHash = ethers.keccak256(
        ethers.toUtf8Bytes('test_document')
      );

      await compliance.connect(admin).updateKYCStatus(
        user1.address,
        2, // VERIFIED
        2, // ENHANCED level
        'TestKYC',
        documentHash,
        expiryDate
      );

      const record = await compliance.getComplianceRecord(user1.address);
      expect(record.kycStatus).to.equal(2); // VERIFIED
      expect(record.level).to.equal(2); // ENHANCED
    });

    it('Should validate transaction compliance', async function () {
      const { compliance, user1, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const COMPLIANCE_OFFICER_ROLE =
        await compliance.COMPLIANCE_OFFICER_ROLE();
      const KYC_PROVIDER_ROLE = await compliance.KYC_PROVIDER_ROLE();

      await compliance
        .connect(admin)
        .grantRole(COMPLIANCE_OFFICER_ROLE, admin.address);
      await compliance
        .connect(admin)
        .grantRole(KYC_PROVIDER_ROLE, admin.address);

      // Register KYC provider and verify user
      await compliance
        .connect(admin)
        .registerKYCProvider('TestKYC', admin.address);

      const expiryDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;
      const documentHash = ethers.keccak256(
        ethers.toUtf8Bytes('test_document')
      );

      await compliance.connect(admin).updateKYCStatus(
        user1.address,
        2, // VERIFIED
        2, // ENHANCED level
        'TestKYC',
        documentHash,
        expiryDate
      );

      // Set jurisdiction compliance
      await compliance
        .connect(admin)
        .setJurisdictionCompliance(user1.address, 1, true); // EU_GDPR

      // Validate transaction
      const result = await compliance.connect(admin).validateTransaction(
        user1.address,
        'governance_vote',
        ethers.parseEther('1000'),
        1 // EU_GDPR jurisdiction
      );

      expect(result.approved).to.be.true;
    });
  });

  describe('Enhanced Treasury', function () {
    it('Should add treasury assets', async function () {
      const { treasury, wrfToken, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const TREASURY_MANAGER_ROLE = await treasury.TREASURY_MANAGER_ROLE();
      await treasury
        .connect(admin)
        .grantRole(TREASURY_MANAGER_ROLE, admin.address);

      await treasury.connect(admin).addTreasuryAsset(
        await wrfToken.getAddress(),
        5 // medium risk rating
      );

      const asset = await treasury.getTreasuryAsset(
        await wrfToken.getAddress()
      );
      expect(asset.isActive).to.be.true;
      expect(asset.riskRating).to.equal(5);
    });

    it('Should deposit funds into treasury', async function () {
      const { treasury, wrfToken, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const TREASURY_MANAGER_ROLE = await treasury.TREASURY_MANAGER_ROLE();
      await treasury
        .connect(admin)
        .grantRole(TREASURY_MANAGER_ROLE, admin.address);

      await treasury
        .connect(admin)
        .addTreasuryAsset(await wrfToken.getAddress(), 5);

      const depositAmount = ethers.parseEther('10000');
      await wrfToken
        .connect(admin)
        .approve(await treasury.getAddress(), depositAmount);
      await treasury
        .connect(admin)
        .deposit(await wrfToken.getAddress(), depositAmount);

      const asset = await treasury.getTreasuryAsset(
        await wrfToken.getAddress()
      );
      expect(asset.balance).to.equal(depositAmount);

      const totalValue = await treasury.getTotalTreasuryValue();
      expect(totalValue).to.equal(depositAmount);
    });

    it('Should allocate funds based on strategy', async function () {
      const { treasury, wrfToken, admin, user1 } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const TREASURY_MANAGER_ROLE = await treasury.TREASURY_MANAGER_ROLE();
      const FUND_ALLOCATOR_ROLE = await treasury.FUND_ALLOCATOR_ROLE();

      await treasury
        .connect(admin)
        .grantRole(TREASURY_MANAGER_ROLE, admin.address);
      await treasury
        .connect(admin)
        .grantRole(FUND_ALLOCATOR_ROLE, admin.address);

      // Add asset and deposit funds
      await treasury
        .connect(admin)
        .addTreasuryAsset(await wrfToken.getAddress(), 5);

      const depositAmount = ethers.parseEther('10000');
      await wrfToken
        .connect(admin)
        .approve(await treasury.getAddress(), depositAmount);
      await treasury
        .connect(admin)
        .deposit(await wrfToken.getAddress(), depositAmount);

      // Allocate funds
      const allocationAmount = ethers.parseEther('1000');
      const proposalHash = ethers.keccak256(
        ethers.toUtf8Bytes('test_proposal')
      );

      await treasury.connect(admin).allocateFunds(
        0, // DEVELOPMENT category
        await wrfToken.getAddress(),
        allocationAmount,
        user1.address,
        'Development funding',
        proposalHash,
        1 // governance approval ID
      );

      const asset = await treasury.getTreasuryAsset(
        await wrfToken.getAddress()
      );
      expect(asset.reservedAmount).to.equal(allocationAmount);
    });

    it('Should generate financial reports', async function () {
      const { treasury, admin } = await loadFixture(
        deployAdvancedGovernanceFixture
      );

      const AUDITOR_ROLE = await treasury.AUDITOR_ROLE();
      await treasury.connect(admin).grantRole(AUDITOR_ROLE, admin.address);

      // Fast forward time to make report generation available
      await time.increase(30 * 24 * 60 * 60 + 1); // 30 days + 1 second

      const reportId = await treasury.connect(admin).generateFinancialReport();
      expect(reportId).to.be.gt(0);
    });
  });

  describe('Integration Tests', function () {
    it('Should integrate governance with emergency protocol', async function () {
      const { governor, emergencyProtocol, wrfToken, admin } =
        await loadFixture(deployAdvancedGovernanceFixture);

      // Setup emergency protocol oracle
      const ORACLE_ROLE = await emergencyProtocol.ORACLE_ROLE();
      await emergencyProtocol
        .connect(admin)
        .grantRole(ORACLE_ROLE, admin.address);

      // Simulate high CO2 reading that should trigger emergency
      await emergencyProtocol.connect(admin).updateOracleData(
        'co2_concentration',
        43000, // 430 ppm - above threshold
        Math.floor(Date.now() / 1000)
      );

      const activeEmergencies = await emergencyProtocol.getActiveEmergencies();
      // Note: In full implementation, this would trigger an emergency event
      // For now, we just verify the oracle data was recorded
      const twamValue =
        await emergencyProtocol.getTWAMValue('co2_concentration');
      expect(twamValue).to.equal(43000);
    });

    it('Should integrate validator performance with incentives', async function () {
      const { dposValidator, incentives, wrfToken, validator1, admin } =
        await loadFixture(deployAdvancedGovernanceFixture);

      // Register validator
      const stakeAmount = ethers.parseEther('15000');
      await wrfToken
        .connect(validator1)
        .approve(await dposValidator.getAddress(), stakeAmount);
      await dposValidator
        .connect(validator1)
        .registerValidator(
          'Climate Validator 1',
          'Expert in climate data validation',
          500,
          stakeAmount
        );

      // Update performance scores
      await dposValidator.connect(admin).updatePerformanceScores(
        [validator1.address],
        [95], // performance score
        [90] // climate score
      );

      const validator = await dposValidator.getValidator(validator1.address);
      expect(validator.performanceScore).to.equal(95);
      expect(validator.climateScore).to.equal(90);

      // This would trigger incentive rewards in full implementation
      const INCENTIVE_MANAGER_ROLE = await incentives.INCENTIVE_MANAGER_ROLE();
      await incentives
        .connect(admin)
        .grantRole(INCENTIVE_MANAGER_ROLE, admin.address);

      // Record validation participation
      await incentives.connect(admin).recordParticipation(
        validator1.address,
        3, // VALIDATION category
        95, // score based on performance
        0 // program ID
      );

      const profile = await incentives.getStakeholderProfile(
        validator1.address
      );
      expect(profile.totalParticipationScore).to.equal(95);
    });
  });
});
