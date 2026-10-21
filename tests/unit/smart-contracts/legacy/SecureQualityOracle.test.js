const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  time,
  loadFixture,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('SecureQualityOracle - Comprehensive Security Tests', function () {
  let oracle;
  let owner, manager, updater, attacker, user;
  let mockToken, mockFeed1, mockFeed2, mockFeed3;

  const ORACLE_MANAGER_ROLE = ethers.utils.keccak256(
    ethers.utils.toUtf8Bytes('ORACLE_MANAGER_ROLE')
  );
  const QUALITY_UPDATER_ROLE = ethers.utils.keccak256(
    ethers.utils.toUtf8Bytes('QUALITY_UPDATER_ROLE')
  );
  const EMERGENCY_ROLE = ethers.utils.keccak256(
    ethers.utils.toUtf8Bytes('EMERGENCY_ROLE')
  );

  async function deployOracleFixture() {
    [owner, manager, updater, attacker, user] = await ethers.getSigners();

    // Deploy mock contracts
    const MockToken = await ethers.getContractFactory('MockERC20');
    mockToken = await MockToken.deploy('Test Token', 'TEST');

    const MockAggregator = await ethers.getContractFactory('MockAggregatorV3');
    mockFeed1 = await MockAggregator.deploy(8, 'Feed 1');
    mockFeed2 = await MockAggregator.deploy(8, 'Feed 2');
    mockFeed3 = await MockAggregator.deploy(8, 'Feed 3');

    // Deploy oracle
    const SecureQualityOracle = await ethers.getContractFactory(
      'SecureQualityOracle'
    );
    oracle = await SecureQualityOracle.deploy();

    // Grant roles
    await oracle.grantRole(ORACLE_MANAGER_ROLE, manager.address);
    await oracle.grantRole(QUALITY_UPDATER_ROLE, updater.address);
    await oracle.grantRole(EMERGENCY_ROLE, owner.address);

    return {
      oracle,
      mockToken,
      mockFeed1,
      mockFeed2,
      mockFeed3,
      owner,
      manager,
      updater,
      attacker,
      user,
    };
  }

  describe('Deployment and Initialization', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);
    });

    it('Should deploy with correct initial state', async function () {
      expect(
        await oracle.hasRole(await oracle.DEFAULT_ADMIN_ROLE(), owner.address)
      ).to.be.true;
      expect(await oracle.hasRole(ORACLE_MANAGER_ROLE, manager.address)).to.be
        .true;
      expect(await oracle.hasRole(QUALITY_UPDATER_ROLE, updater.address)).to.be
        .true;
      expect(await oracle.MAX_DEVIATION()).to.equal(500); // 5%
      expect(await oracle.STALENESS_THRESHOLD()).to.equal(3600); // 1 hour
      expect(await oracle.MIN_VALID_SOURCES()).to.equal(3);
    });

    it('Should set correct tier thresholds', async function () {
      expect(await oracle.getTierMinimumScore(1)).to.equal(20);
      expect(await oracle.getTierMinimumScore(2)).to.equal(40);
      expect(await oracle.getTierMinimumScore(3)).to.equal(60);
      expect(await oracle.getTierMinimumScore(4)).to.equal(80);
      expect(await oracle.getTierMinimumScore(5)).to.equal(95);
    });
  });

  describe('Oracle Source Management', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);
    });

    it('Should allow oracle manager to add sources', async function () {
      await expect(
        oracle.connect(manager).addOracleSource(
          mockToken.address,
          mockFeed1.address,
          3000, // 30% weight
          'Primary Feed'
        )
      )
        .to.emit(oracle, 'OracleSourceAdded')
        .withArgs(mockToken.address, mockFeed1.address, 3000, 'Primary Feed');
    });

    it('Should reject non-manager attempts to add sources', async function () {
      await expect(
        oracle
          .connect(attacker)
          .addOracleSource(
            mockToken.address,
            mockFeed1.address,
            3000,
            'Malicious Feed'
          )
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should reject invalid parameters', async function () {
      // Invalid token address
      await expect(
        oracle
          .connect(manager)
          .addOracleSource(
            ethers.constants.AddressZero,
            mockFeed1.address,
            3000,
            'Invalid Token'
          )
      ).to.be.revertedWith('Invalid token address');

      // Invalid feed address
      await expect(
        oracle
          .connect(manager)
          .addOracleSource(
            mockToken.address,
            ethers.constants.AddressZero,
            3000,
            'Invalid Feed'
          )
      ).to.be.revertedWith('Invalid feed address');

      // Invalid weight
      await expect(
        oracle
          .connect(manager)
          .addOracleSource(
            mockToken.address,
            mockFeed1.address,
            0,
            'Zero Weight'
          )
      ).to.be.revertedWith('Invalid weight');

      await expect(
        oracle.connect(manager).addOracleSource(
          mockToken.address,
          mockFeed1.address,
          20000, // > QUALITY_PRECISION
          'Excessive Weight'
        )
      ).to.be.revertedWith('Invalid weight');
    });

    it('Should limit maximum number of sources', async function () {
      // Add 10 sources (maximum)
      for (let i = 0; i < 10; i++) {
        const MockAggregator =
          await ethers.getContractFactory('MockAggregatorV3');
        const feed = await MockAggregator.deploy(8, `Feed ${i}`);
        await oracle
          .connect(manager)
          .addOracleSource(mockToken.address, feed.address, 1000, `Feed ${i}`);
      }

      // Attempt to add 11th source should fail
      const MockAggregator =
        await ethers.getContractFactory('MockAggregatorV3');
      const extraFeed = await MockAggregator.deploy(8, 'Extra Feed');
      await expect(
        oracle
          .connect(manager)
          .addOracleSource(
            mockToken.address,
            extraFeed.address,
            1000,
            'Extra Feed'
          )
      ).to.be.revertedWith('Too many sources');
    });
  });

  describe('Multi-Oracle Consensus', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);

      // Set up mock feeds with quality scores
      await mockFeed1.setLatestAnswer(85); // 85% quality
      await mockFeed2.setLatestAnswer(80); // 80% quality
      await mockFeed3.setLatestAnswer(90); // 90% quality

      // Add oracle sources
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed1.address, 3000, 'Feed 1');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed2.address, 3000, 'Feed 2');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed3.address, 4000, 'Feed 3');
    });

    it('Should calculate weighted median consensus', async function () {
      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 85)
      ).to.emit(oracle, 'ConsensusAchieved');

      const score = await oracle.getQualityScore(mockToken.address);
      expect(score).to.be.closeTo(85, 5); // Should be close to median
    });

    it('Should reject updates with insufficient sources', async function () {
      // Remove two sources to have only one
      await oracle.connect(manager).removeOracleSource(mockToken.address, 1);
      await oracle.connect(manager).removeOracleSource(mockToken.address, 1);

      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 85)
      ).to.be.revertedWith('Insufficient valid sources');
    });

    it('Should handle oracle failures gracefully', async function () {
      // Make one feed return invalid data
      await mockFeed1.setLatestAnswer(0); // Invalid score

      // Should still work with remaining valid sources
      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 85)
      ).to.emit(oracle, 'ConsensusAchieved');
    });
  });

  describe('Price Manipulation Protection', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);

      // Set up oracle sources
      await mockFeed1.setLatestAnswer(80);
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(80);

      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed1.address, 3333, 'Feed 1');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed2.address, 3333, 'Feed 2');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed3.address, 3334, 'Feed 3');

      // Set initial quality score
      await oracle.connect(updater).updateQualityScore(mockToken.address, 80);
    });

    it('Should trigger circuit breaker on large deviation', async function () {
      // Update feeds to show 10% increase (above 5% limit)
      await mockFeed1.setLatestAnswer(88);
      await mockFeed2.setLatestAnswer(89);
      await mockFeed3.setLatestAnswer(87);

      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 88)
      )
        .to.emit(oracle, 'CircuitBreakerTriggered')
        .and.to.be.revertedWith('Price deviation exceeds maximum allowed');
    });

    it('Should allow updates within deviation threshold', async function () {
      // Update feeds to show 3% increase (within 5% limit)
      await mockFeed1.setLatestAnswer(82);
      await mockFeed2.setLatestAnswer(83);
      await mockFeed3.setLatestAnswer(82);

      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 82)
      )
        .to.emit(oracle, 'QualityUpdated')
        .withArgs(mockToken.address, 82, await time.latest());
    });

    it('Should block access when circuit breaker is triggered', async function () {
      // Trigger circuit breaker
      await mockFeed1.setLatestAnswer(90);
      await mockFeed2.setLatestAnswer(91);
      await mockFeed3.setLatestAnswer(89);

      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 90)
      ).to.be.revertedWith('Price deviation exceeds maximum allowed');

      // Should reject quality score requests
      await expect(
        oracle.getQualityScore(mockToken.address)
      ).to.be.revertedWith('Circuit breaker active');
    });
  });

  describe('TWAP Protection', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);

      // Set up oracle sources
      await mockFeed1.setLatestAnswer(80);
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(80);

      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed1.address, 3333, 'Feed 1');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed2.address, 3333, 'Feed 2');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed3.address, 3334, 'Feed 3');
    });

    it('Should calculate TWAP correctly', async function () {
      // Set initial score
      await oracle.connect(updater).updateQualityScore(mockToken.address, 80);

      // Wait and update with different score
      await time.increase(300); // 5 minutes
      await mockFeed1.setLatestAnswer(82);
      await mockFeed2.setLatestAnswer(82);
      await mockFeed3.setLatestAnswer(82);
      await oracle.connect(updater).updateQualityScore(mockToken.address, 82);

      // Check TWAP value
      const twapValue = await oracle.getTWAQuality(mockToken.address, 600); // 10 minutes
      expect(twapValue).to.be.greaterThan(80);
      expect(twapValue).to.be.lessThan(82);
    });

    it('Should use TWAP in pricing factor calculation', async function () {
      await oracle.connect(updater).updateQualityScore(mockToken.address, 80);

      const pricingFactor = await oracle.getQualityPricingFactor(
        mockToken.address
      );
      expect(pricingFactor).to.equal(110); // 80+ score should give 110%
    });
  });

  describe('Staleness Protection', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);

      // Set up oracle sources
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed1.address, 3333, 'Feed 1');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed2.address, 3333, 'Feed 2');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed3.address, 3334, 'Feed 3');

      // Set fresh data
      await mockFeed1.setLatestAnswer(80);
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(80);
    });

    it('Should accept fresh data', async function () {
      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 80)
      ).to.emit(oracle, 'QualityUpdated');
    });

    it('Should reject stale data', async function () {
      // Make data stale (older than 1 hour)
      await time.increase(3700); // 1 hour + 100 seconds

      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 80)
      ).to.be.revertedWith('Insufficient valid sources');
    });

    it('Should check data freshness correctly', async function () {
      await oracle.connect(updater).updateQualityScore(mockToken.address, 80);

      // Should be fresh initially
      expect(await oracle.isQualityDataFresh(mockToken.address, 3600)).to.be
        .true;

      // Should become stale after time passes
      await time.increase(3700);
      expect(await oracle.isQualityDataFresh(mockToken.address, 3600)).to.be
        .false;
    });
  });

  describe('Emergency Controls', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);

      // Set up oracle sources and initial data
      await mockFeed1.setLatestAnswer(80);
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(80);

      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed1.address, 3333, 'Feed 1');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed2.address, 3333, 'Feed 2');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed3.address, 3334, 'Feed 3');

      await oracle.connect(updater).updateQualityScore(mockToken.address, 80);
    });

    it('Should allow emergency pause', async function () {
      await expect(oracle.connect(owner).emergencyPause())
        .to.emit(oracle, 'EmergencyModeActivated')
        .withArgs(ethers.constants.AddressZero, 'Global emergency pause');

      expect(await oracle.paused()).to.be.true;
      expect(await oracle.globalEmergencyMode()).to.be.true;
    });

    it('Should allow token-specific emergency pause', async function () {
      await expect(
        oracle
          .connect(owner)
          .emergencyPauseToken(mockToken.address, 'Security threat detected')
      )
        .to.emit(oracle, 'EmergencyModeActivated')
        .withArgs(mockToken.address, 'Security threat detected');
    });

    it('Should use emergency quality scores during emergency', async function () {
      await oracle
        .connect(owner)
        .setEmergencyQualityScore(mockToken.address, 60);
      await oracle
        .connect(owner)
        .emergencyPauseToken(mockToken.address, 'Emergency test');

      const score = await oracle.getQualityScore(mockToken.address);
      expect(score).to.equal(60);
    });

    it('Should reject non-emergency role attempts', async function () {
      await expect(
        oracle.connect(attacker).emergencyPause()
      ).to.be.revertedWith('AccessControl:');

      await expect(
        oracle
          .connect(attacker)
          .emergencyPauseToken(mockToken.address, 'Malicious pause')
      ).to.be.revertedWith('AccessControl:');
    });
  });

  describe('Access Control', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);
    });

    it('Should enforce oracle manager role', async function () {
      await expect(
        oracle
          .connect(attacker)
          .addOracleSource(
            mockToken.address,
            mockFeed1.address,
            3000,
            'Unauthorized'
          )
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should enforce quality updater role', async function () {
      await expect(
        oracle.connect(attacker).updateQualityScore(mockToken.address, 80)
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should enforce emergency role', async function () {
      await expect(
        oracle.connect(attacker).emergencyPause()
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should allow role admin to grant roles', async function () {
      await oracle.connect(owner).grantRole(ORACLE_MANAGER_ROLE, user.address);
      expect(await oracle.hasRole(ORACLE_MANAGER_ROLE, user.address)).to.be
        .true;
    });
  });

  describe('Gas Optimization', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);

      // Set up multiple oracle sources
      await mockFeed1.setLatestAnswer(80);
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(80);

      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed1.address, 2500, 'Feed 1');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed2.address, 2500, 'Feed 2');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed3.address, 2500, 'Feed 3');

      // Add more sources for testing
      const MockAggregator =
        await ethers.getContractFactory('MockAggregatorV3');
      const mockFeed4 = await MockAggregator.deploy(8, 'Feed 4');
      const mockFeed5 = await MockAggregator.deploy(8, 'Feed 5');

      await mockFeed4.setLatestAnswer(80);
      await mockFeed5.setLatestAnswer(80);

      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed4.address, 1250, 'Feed 4');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed5.address, 1250, 'Feed 5');
    });

    it('Should handle multiple oracle calls efficiently', async function () {
      const tx = await oracle
        .connect(updater)
        .updateQualityScore(mockToken.address, 80);
      const receipt = await tx.wait();

      // Gas usage should be reasonable even with 5 oracle sources
      expect(receipt.gasUsed).to.be.lessThan(500000); // 500k gas limit
    });

    it('Should batch quality updates efficiently', async function () {
      const tokens = [mockToken.address];
      const scores = [80];

      const tx = await oracle
        .connect(updater)
        .batchUpdateQuality(tokens, scores);
      const receipt = await tx.wait();

      expect(receipt.gasUsed).to.be.lessThan(300000); // Efficient batching
    });
  });

  describe('Attack Simulation', function () {
    beforeEach(async function () {
      await loadFixture(deployOracleFixture);

      // Set up oracle sources
      await mockFeed1.setLatestAnswer(80);
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(80);

      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed1.address, 3333, 'Feed 1');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed2.address, 3333, 'Feed 2');
      await oracle
        .connect(manager)
        .addOracleSource(mockToken.address, mockFeed3.address, 3334, 'Feed 3');

      await oracle.connect(updater).updateQualityScore(mockToken.address, 80);
    });

    it('Should resist single oracle manipulation', async function () {
      // Attacker tries to manipulate one oracle
      await mockFeed1.setLatestAnswer(100); // Extreme value

      // Other oracles remain normal
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(81);

      // Update should use median, ignoring the extreme outlier
      await oracle.connect(updater).updateQualityScore(mockToken.address, 81);
      const score = await oracle.getQualityScore(mockToken.address);

      // Score should be close to median (80-81), not the manipulated 100
      expect(score).to.be.closeTo(80, 3);
    });

    it('Should resist coordinated manipulation attempts', async function () {
      // Simulate coordinated attack on 2/3 oracles
      await mockFeed1.setLatestAnswer(95);
      await mockFeed2.setLatestAnswer(94);
      await mockFeed3.setLatestAnswer(80); // Only one honest oracle

      // Should trigger circuit breaker due to large deviation
      await expect(
        oracle.connect(updater).updateQualityScore(mockToken.address, 93)
      ).to.emit(oracle, 'CircuitBreakerTriggered');
    });

    it('Should detect and prevent flash loan attacks', async function () {
      // Simulate rapid price changes (flash loan pattern)
      await mockFeed1.setLatestAnswer(85);
      await mockFeed2.setLatestAnswer(85);
      await mockFeed3.setLatestAnswer(85);

      await oracle.connect(updater).updateQualityScore(mockToken.address, 85);

      // Immediate attempt to manipulate back (same block)
      await mockFeed1.setLatestAnswer(80);
      await mockFeed2.setLatestAnswer(80);
      await mockFeed3.setLatestAnswer(80);

      // TWAP should provide resistance to rapid changes
      const twapValue = await oracle.getTWAQuality(mockToken.address, 300);
      expect(twapValue).to.be.greaterThan(80); // Should reflect both values
    });
  });
});

// Mock contracts for testing
contract('MockERC20', () => {
  // Basic ERC20 mock implementation would go here
});

contract('MockAggregatorV3', () => {
  // Mock Chainlink aggregator implementation would go here
});
