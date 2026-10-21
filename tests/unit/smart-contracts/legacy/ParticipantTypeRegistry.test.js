const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('ParticipantTypeRegistry', function () {
  // Test fixtures
  async function deployRegistryFixture() {
    const [
      owner,
      oracle,
      weatherStation,
      validator,
      storage,
      slasher,
      user1,
      user2,
      user3,
    ] = await ethers.getSigners();

    const ParticipantTypeRegistry = await ethers.getContractFactory(
      'ParticipantTypeRegistry'
    );
    const registry = await ParticipantTypeRegistry.deploy();
    await registry.waitForDeployment();

    // Grant roles
    const ORACLE_ROLE = await registry.ORACLE_ROLE();
    const WEATHER_STATION_ROLE = await registry.WEATHER_STATION_ROLE();
    const VALIDATOR_ROLE = await registry.VALIDATOR_ROLE();
    const STORAGE_ROLE = await registry.STORAGE_ROLE();
    const SLASHING_ROLE = await registry.SLASHING_ROLE();
    const GOVERNANCE_ROLE = await registry.GOVERNANCE_ROLE();

    await registry.grantRole(ORACLE_ROLE, oracle.address);
    await registry.grantRole(WEATHER_STATION_ROLE, weatherStation.address);
    await registry.grantRole(VALIDATOR_ROLE, validator.address);
    await registry.grantRole(STORAGE_ROLE, storage.address);
    await registry.grantRole(SLASHING_ROLE, slasher.address);

    return {
      registry,
      owner,
      oracle,
      weatherStation,
      validator,
      storage,
      slasher,
      user1,
      user2,
      user3,
      ORACLE_ROLE,
      WEATHER_STATION_ROLE,
      VALIDATOR_ROLE,
      STORAGE_ROLE,
      SLASHING_ROLE,
      GOVERNANCE_ROLE,
    };
  }

  async function registerParticipantFixture() {
    const fixture = await loadFixture(deployRegistryFixture);
    const { registry, oracle, user1 } = fixture;

    // Sample geographic data
    const location = {
      latitude: 40_750_000, // New York City scaled by 1e6
      longitude: -73_985_000,
      zone: 0, // DEFAULT_AREA
      countryCode: ethers.encodeBytes32String('US'),
      regionCode: ethers.encodeBytes32String('NY'),
    };

    // Register participant as weather station
    await registry.connect(oracle).registerParticipant(
      user1.address,
      0, // WEATHER_STATION
      location,
      1 // EMAIL_VERIFIED
    );

    return { ...fixture, location };
  }

  describe('Deployment', function () {
    it('Should deploy with correct initial state', async function () {
      const { registry, owner } = await loadFixture(deployRegistryFixture);

      expect(await registry.totalParticipants()).to.equal(0);
      expect(await registry.totalActiveParticipants()).to.equal(0);
      expect(await registry.registrationOpen()).to.equal(true);
      expect(
        await registry.hasRole(
          await registry.DEFAULT_ADMIN_ROLE(),
          owner.address
        )
      ).to.equal(true);
    });

    it('Should set up roles correctly', async function () {
      const { registry, owner, oracle } = await loadFixture(
        deployRegistryFixture
      );

      expect(
        await registry.hasRole(await registry.GOVERNANCE_ROLE(), owner.address)
      ).to.equal(true);
      expect(
        await registry.hasRole(await registry.ORACLE_ROLE(), oracle.address)
      ).to.equal(true);
    });
  });

  describe('Participant Registration', function () {
    it('Should register a new participant successfully', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        deployRegistryFixture
      );

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      await expect(
        registry.connect(oracle).registerParticipant(
          user1.address,
          0, // WEATHER_STATION
          location,
          1 // EMAIL_VERIFIED
        )
      )
        .to.emit(registry, 'ParticipantRegistered')
        .withArgs(user1.address, 0, (await time.latest()) + 1);

      expect(await registry.totalParticipants()).to.equal(1);
      expect(await registry.totalActiveParticipants()).to.equal(1);
    });

    it('Should reject registration with invalid parameters', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        deployRegistryFixture
      );

      const location = {
        latitude: 91_000_000, // Invalid latitude
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      await expect(
        registry
          .connect(oracle)
          .registerParticipant(user1.address, 0, location, 1)
      ).to.be.revertedWith('Invalid latitude');
    });

    it('Should reject registration from unauthorized address', async function () {
      const { registry, user1, user2 } = await loadFixture(
        deployRegistryFixture
      );

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      await expect(
        registry
          .connect(user1)
          .registerParticipant(user2.address, 0, location, 1)
      ).to.be.revertedWith('Unauthorized registration');
    });

    it('Should prevent duplicate registration', async function () {
      const { registry, oracle, user1, location } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(oracle).registerParticipant(
          user1.address,
          1, // Different type
          location,
          1
        )
      ).to.be.revertedWith('Participant already registered');
    });
  });

  describe('Participation Type Management', function () {
    it('Should add new participation type successfully', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(oracle).addParticipationType(user1.address, 1) // FORECAST_PRODUCER
      )
        .to.emit(registry, 'ParticipationTypeAdded')
        .withArgs(user1.address, 1, (await time.latest()) + 1);

      const participationTypes = await registry.getParticipationTypes(
        user1.address
      );
      expect(participationTypes).to.include(0n); // WEATHER_STATION
      expect(participationTypes).to.include(1n); // FORECAST_PRODUCER
    });

    it('Should reject duplicate participation type', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(oracle).addParticipationType(user1.address, 0) // Already has WEATHER_STATION
      ).to.be.revertedWith('Participation type already exists');
    });

    it('Should remove participation type successfully', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Add second type first
      await registry.connect(oracle).addParticipationType(user1.address, 1);

      await expect(
        registry.connect(oracle).removeParticipationType(user1.address, 0) // Remove WEATHER_STATION
      )
        .to.emit(registry, 'ParticipationTypeRemoved')
        .withArgs(user1.address, 0, (await time.latest()) + 1);

      const participationTypes = await registry.getParticipationTypes(
        user1.address
      );
      expect(participationTypes).to.not.include(0n);
      expect(participationTypes).to.include(1n);
    });

    it('Should not allow removing last participation type', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(oracle).removeParticipationType(user1.address, 0)
      ).to.be.revertedWith('Cannot remove last participation type');
    });
  });

  describe('Activity Tracking', function () {
    it('Should update activity successfully', async function () {
      const { registry, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(weatherStation).updateActivity(
          user1.address,
          0, // WEATHER_STATION
          8500, // 85% quality score
          5, // 5 contributions
          9500 // 95% uptime
        )
      ).to.emit(registry, 'ActivityUpdated');

      const metrics = await registry.getActivityMetrics(user1.address, 0);
      expect(metrics.qualityScore).to.equal(8500);
      expect(metrics.totalContributions).to.equal(5);
      expect(metrics.uptimePercentage).to.equal(9500);
    });

    it('Should reject invalid quality score', async function () {
      const { registry, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(weatherStation).updateActivity(
          user1.address,
          0,
          11000, // Invalid score > 10000
          1,
          9000
        )
      ).to.be.revertedWith('Invalid quality score');
    });

    it('Should track consecutive days active', async function () {
      const { registry, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Day 1 activity
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 8000, 1, 9000);

      // Move to next day
      await time.increase(24 * 60 * 60);

      // Day 2 activity
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 8000, 1, 9000);

      const metrics = await registry.getActivityMetrics(user1.address, 0);
      expect(metrics.consecutiveDaysActive).to.equal(2);
    });

    it('Should reset consecutive days if gap in activity', async function () {
      const { registry, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Day 1 activity
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 8000, 1, 9000);

      // Skip day 2, go to day 3
      await time.increase(48 * 60 * 60);

      // Day 3 activity
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 8000, 1, 9000);

      const metrics = await registry.getActivityMetrics(user1.address, 0);
      expect(metrics.consecutiveDaysActive).to.equal(1); // Reset to 1
    });
  });

  describe('Slashing Event Tracking', function () {
    it('Should record slashing event successfully', async function () {
      const { registry, slasher, user1 } = await loadFixture(
        registerParticipantFixture
      );

      const slashedAmount = ethers.parseEther('100');
      const penaltyMultiplier = 2000000; // 2x multiplier

      await expect(
        registry
          .connect(slasher)
          .recordSlashingEvent(user1.address, slashedAmount, penaltyMultiplier)
      )
        .to.emit(registry, 'SlashingEventRecorded')
        .withArgs(
          user1.address,
          slashedAmount,
          penaltyMultiplier,
          (await time.latest()) + 1
        );

      // Check that activity score was penalized
      const score = await registry.getParticipationScore(user1.address);
      expect(score).to.be.lt(await registry.MIN_ACTIVITY_SCORE()); // Should be at minimum
    });

    it('Should reject invalid penalty multiplier', async function () {
      const { registry, slasher, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(slasher).recordSlashingEvent(
          user1.address,
          ethers.parseEther('100'),
          6000000 // 6x - exceeds maximum of 5x
        )
      ).to.be.revertedWith('Invalid penalty multiplier');
    });
  });

  describe('Layaway Period Calculation', function () {
    it('Should calculate correct layaway period for multi-method participant', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Add second participation type to make it multi-method
      await registry.connect(oracle).addParticipationType(user1.address, 1); // FORECAST_PRODUCER

      const layawayPeriod = await registry.calculateLayawayPeriod(
        user1.address
      );
      const basePeriod = await registry.BASE_LAYAWAY_MULTI_METHOD();

      // Multi-method should get base 30-day period (with some adjustments)
      expect(layawayPeriod).to.be.lte(basePeriod * 2); // Should be reasonable multiple
      expect(layawayPeriod).to.be.gte(7 * 24 * 60 * 60); // At least 7 days minimum
    });

    it('Should calculate correct layaway period for verified single-method', async function () {
      const { registry, user1 } = await loadFixture(registerParticipantFixture);

      const layawayPeriod = await registry.calculateLayawayPeriod(
        user1.address
      );
      const basePeriod = await registry.BASE_LAYAWAY_VERIFIED_SINGLE();

      // Verified single-method should get 60-day base period
      expect(layawayPeriod).to.be.gte(7 * 24 * 60 * 60); // At least 7 days minimum
      expect(layawayPeriod).to.be.lte(365 * 24 * 60 * 60); // At most 365 days maximum
    });

    it('Should apply slashing penalty to layaway period', async function () {
      const { registry, slasher, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Record slashing event
      const penaltyMultiplier = 2000000; // 2x multiplier
      await registry
        .connect(slasher)
        .recordSlashingEvent(
          user1.address,
          ethers.parseEther('100'),
          penaltyMultiplier
        );

      const layawayPeriod = await registry.calculateLayawayPeriod(
        user1.address
      );

      // Should be significantly longer due to slashing penalty
      expect(layawayPeriod).to.be.gt(60 * 24 * 60 * 60); // Should be > 60 days due to penalty
    });

    it('Should provide quality adjustments to layaway period', async function () {
      const { registry, oracle, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Update with excellent quality score
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 9500, 10, 9800);

      const layawayPeriod = await registry.calculateLayawayPeriod(
        user1.address
      );

      // Should be reduced due to excellent quality
      expect(layawayPeriod).to.be.lt(90 * 24 * 60 * 60); // Should be less than 90 days
      expect(layawayPeriod).to.be.gte(7 * 24 * 60 * 60); // At least minimum
    });
  });

  describe('Multi-Method Participant Detection', function () {
    it('Should correctly identify multi-method participants', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Initially single-method
      expect(await registry.isMultiMethodParticipant(user1.address)).to.equal(
        false
      );

      // Add second type
      await registry.connect(oracle).addParticipationType(user1.address, 1);

      // Now multi-method
      expect(await registry.isMultiMethodParticipant(user1.address)).to.equal(
        true
      );
    });
  });

  describe('Verification Management', function () {
    it('Should update verification status successfully', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(oracle).updateVerificationStatus(user1.address, 3) // GOVERNMENT_ID_VERIFIED
      )
        .to.emit(registry, 'VerificationStatusUpdated')
        .withArgs(user1.address, 1, 3, (await time.latest()) + 1);
    });

    it('Should reject invalid verification status', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await expect(
        registry.connect(oracle).updateVerificationStatus(user1.address, 99) // Invalid status
      ).to.be.revertedWith('Invalid verification status');
    });
  });

  describe('Geographic Location Management', function () {
    it('Should update geographic location successfully', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      const newLocation = {
        latitude: 34_050_000, // Los Angeles
        longitude: -118_250_000,
        zone: 2, // DENSE_URBAN_AREA
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('CA'),
      };

      await expect(
        registry
          .connect(oracle)
          .updateGeographicLocation(user1.address, newLocation)
      )
        .to.emit(registry, 'GeographicLocationUpdated')
        .withArgs(
          user1.address,
          newLocation.countryCode,
          newLocation.regionCode,
          (await time.latest()) + 1
        );
    });
  });

  describe('Query Functions', function () {
    it('Should return correct participation score', async function () {
      const { registry, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      const initialScore = await registry.getParticipationScore(user1.address);
      expect(initialScore).to.be.gte(await registry.MIN_ACTIVITY_SCORE());

      // Update activity to improve score
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 9000, 5, 9500);

      const newScore = await registry.getParticipationScore(user1.address);
      expect(newScore).to.be.gt(initialScore);
    });

    it('Should return correct participation types', async function () {
      const { registry, oracle, user1 } = await loadFixture(
        registerParticipantFixture
      );

      let participationTypes = await registry.getParticipationTypes(
        user1.address
      );
      expect(participationTypes.length).to.equal(1);
      expect(participationTypes[0]).to.equal(0n); // WEATHER_STATION

      // Add second type
      await registry.connect(oracle).addParticipationType(user1.address, 1);

      participationTypes = await registry.getParticipationTypes(user1.address);
      expect(participationTypes.length).to.equal(2);
      expect(participationTypes).to.include(0n);
      expect(participationTypes).to.include(1n);
    });

    it('Should return correct activity metrics', async function () {
      const { registry, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 8750, 3, 9250);

      const metrics = await registry.getActivityMetrics(user1.address, 0);
      expect(metrics.qualityScore).to.equal(8750);
      expect(metrics.totalContributions).to.equal(3);
      expect(metrics.uptimePercentage).to.equal(9250);
      expect(metrics.lastContribution).to.be.gt(0);
    });

    it('Should return correct participant history', async function () {
      const { registry, user1 } = await loadFixture(registerParticipantFixture);

      const history = await registry.getParticipantHistory(user1.address);
      expect(history.registrationTime).to.be.gt(0);
      expect(history.lastActivity).to.be.gt(0);
      expect(history.overallRating).to.equal(1); // FAIR rating initially
    });
  });

  describe('Statistics and Reporting', function () {
    it('Should track participation type counts correctly', async function () {
      const { registry, oracle, user1, user2 } = await loadFixture(
        deployRegistryFixture
      );

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      // Register two weather stations
      await registry
        .connect(oracle)
        .registerParticipant(user1.address, 0, location, 1);
      await registry
        .connect(oracle)
        .registerParticipant(user2.address, 0, location, 1);

      expect(await registry.getParticipationTypeCount(0)).to.equal(2); // WEATHER_STATION
      expect(await registry.getParticipationTypeCount(1)).to.equal(0); // FORECAST_PRODUCER
    });

    it('Should track participants by country', async function () {
      const { registry, oracle, user1, user2 } = await loadFixture(
        deployRegistryFixture
      );

      const usLocation = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      await registry
        .connect(oracle)
        .registerParticipant(user1.address, 0, usLocation, 1);

      const usParticipants = await registry.getParticipantsByCountry(
        ethers.encodeBytes32String('US')
      );
      expect(usParticipants.length).to.equal(1);
      expect(usParticipants[0]).to.equal(user1.address);
    });

    it('Should return comprehensive contract stats', async function () {
      const { registry, oracle, user1, user2 } = await loadFixture(
        deployRegistryFixture
      );

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      await registry
        .connect(oracle)
        .registerParticipant(user1.address, 0, location, 1);
      await registry
        .connect(oracle)
        .registerParticipant(user2.address, 1, location, 1);

      const stats = await registry.getContractStats();
      expect(stats[0]).to.equal(2); // totalParticipants
      expect(stats[1]).to.equal(2); // totalActiveParticipants
      expect(stats[2]).to.equal(1); // WEATHER_STATION count
      expect(stats[3]).to.equal(1); // FORECAST_PRODUCER count
    });
  });

  describe('Administrative Functions', function () {
    it('Should allow governance to pause and unpause', async function () {
      const { registry, owner } = await loadFixture(deployRegistryFixture);

      await registry.connect(owner).pause();
      expect(await registry.paused()).to.equal(true);

      await registry.connect(owner).unpause();
      expect(await registry.paused()).to.equal(false);
    });

    it('Should allow governance to control registration status', async function () {
      const { registry, owner } = await loadFixture(deployRegistryFixture);

      await registry.connect(owner).setRegistrationOpen(false);
      expect(await registry.registrationOpen()).to.equal(false);

      await registry.connect(owner).setRegistrationOpen(true);
      expect(await registry.registrationOpen()).to.equal(true);
    });

    it('Should reject administrative actions from unauthorized users', async function () {
      const { registry, user1 } = await loadFixture(deployRegistryFixture);

      await expect(registry.connect(user1).pause()).to.be.revertedWith(
        'Unauthorized'
      );

      await expect(
        registry.connect(user1).setRegistrationOpen(false)
      ).to.be.revertedWith('Unauthorized');
    });
  });

  describe('Rate Limiting', function () {
    it('Should enforce registration cooldown', async function () {
      const { registry, oracle, user1, user2 } = await loadFixture(
        deployRegistryFixture
      );

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      // First registration
      await registry
        .connect(oracle)
        .registerParticipant(user1.address, 0, location, 1);

      // Immediate second registration should fail due to cooldown
      await expect(
        registry
          .connect(oracle)
          .registerParticipant(user2.address, 0, location, 1)
      ).to.be.revertedWith('Registration cooldown active');
    });

    it('Should allow registration after cooldown period', async function () {
      const { registry, oracle, user1, user2 } = await loadFixture(
        deployRegistryFixture
      );

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      // First registration
      await registry
        .connect(oracle)
        .registerParticipant(user1.address, 0, location, 1);

      // Fast forward past cooldown
      await time.increase(60 * 60 + 1); // 1 hour + 1 second

      // Second registration should now succeed
      await expect(
        registry
          .connect(oracle)
          .registerParticipant(user2.address, 0, location, 1)
      ).to.emit(registry, 'ParticipantRegistered');
    });
  });

  describe('Edge Cases and Security', function () {
    it('Should reject zero address participant', async function () {
      const { registry, oracle } = await loadFixture(deployRegistryFixture);

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      await expect(
        registry
          .connect(oracle)
          .registerParticipant(ethers.ZeroAddress, 0, location, 1)
      ).to.be.revertedWith('Invalid participant address');
    });

    it('Should reject contract address as participant', async function () {
      const { registry, oracle } = await loadFixture(deployRegistryFixture);

      const location = {
        latitude: 40_750_000,
        longitude: -73_985_000,
        zone: 0,
        countryCode: ethers.encodeBytes32String('US'),
        regionCode: ethers.encodeBytes32String('NY'),
      };

      await expect(
        registry
          .connect(oracle)
          .registerParticipant(await registry.getAddress(), 0, location, 1)
      ).to.be.revertedWith('Cannot be contract address');
    });

    it('Should handle activity score decay over time', async function () {
      const { registry, weatherStation, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Set high activity initially
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 9000, 10, 9500);
      const initialScore = await registry.getParticipationScore(user1.address);

      // Move forward in time without activity
      await time.increase(30 * 24 * 60 * 60); // 30 days

      // Update with zero new contributions to trigger decay calculation
      await registry
        .connect(weatherStation)
        .updateActivity(user1.address, 0, 9000, 0, 9500);
      const decayedScore = await registry.getParticipationScore(user1.address);

      // Score should have decayed due to inactivity
      expect(decayedScore).to.be.lt(initialScore);
    });

    it('Should maintain minimum activity score floor', async function () {
      const { registry, slasher, user1 } = await loadFixture(
        registerParticipantFixture
      );

      // Apply severe slashing that would bring score below minimum
      await registry.connect(slasher).recordSlashingEvent(
        user1.address,
        ethers.parseEther('1000'),
        3000000 // 3x penalty
      );

      const score = await registry.getParticipationScore(user1.address);
      const minScore = await registry.MIN_ACTIVITY_SCORE();

      // Score should not go below minimum
      expect(score).to.be.gte(minScore);
    });
  });
});
