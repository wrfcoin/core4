// Test file to verify WRFToken enhanced compilation
const { expect } = require('chai');
const { ethers } = require('hardhat');
const { loadFixture } = require('@nomicfoundation/hardhat-network-helpers');

describe('WRFToken Enhanced Fair Launch Integration', function () {
  async function deployWRFTokenFixture() {
    const [owner, station1, station2, advocate, recruit] =
      await ethers.getSigners();

    // Deploy WRFToken
    const WRFToken = await ethers.getContractFactory('WRFToken');
    const wrfToken = await WRFToken.deploy();
    await wrfToken.waitForDeployment();

    return {
      wrfToken,
      owner,
      station1,
      station2,
      advocate,
      recruit,
    };
  }

  describe('Enhanced Registration System', function () {
    it('Should deploy with correct enhanced features', async function () {
      const { wrfToken } = await loadFixture(deployWRFTokenFixture);

      // Check basic token properties
      expect(await wrfToken.name()).to.equal('WRFCoin');
      expect(await wrfToken.symbol()).to.equal('WRF');
      expect(await wrfToken.totalSupply()).to.equal(0); // Fair launch starts with zero

      // Check enhanced staking flags
      expect(await wrfToken.permissionlessRegistrationEnabled()).to.equal(
        false
      );
      expect(await wrfToken.advancedStakingEnabled()).to.equal(false);
    });

    it('Should allow owner to enable advanced staking', async function () {
      const { wrfToken, owner } = await loadFixture(deployWRFTokenFixture);

      await expect(wrfToken.connect(owner).setAdvancedStaking(true))
        .to.emit(wrfToken, 'AdvancedStakingToggled')
        .withArgs(true);

      expect(await wrfToken.advancedStakingEnabled()).to.equal(true);
    });

    it('Should provide registration options for participants', async function () {
      const { wrfToken, station1 } = await loadFixture(deployWRFTokenFixture);

      const options = await wrfToken.getRegistrationOptions(station1.address);

      // Initially only legacy methods disabled
      expect(options.canRegisterLegacy).to.equal(false);
      expect(options.canRegisterMultiAsset).to.equal(false);
      expect(options.canRegisterLayaway).to.equal(false);
      expect(options.canRegisterDelegation).to.equal(false);
      expect(options.eligibilityReason).to.equal('Eligible for registration');
    });

    it('Should show comprehensive station info', async function () {
      const { wrfToken, owner, station1 } = await loadFixture(
        deployWRFTokenFixture
      );

      // Register station via owner (legacy method)
      await wrfToken
        .connect(owner)
        .registerWeatherStation(station1.address, 'STATION_001');

      const info = await wrfToken.getComprehensiveStationInfo(station1.address);

      expect(info.isRegistered).to.equal(true);
      expect(info.stationId).to.equal('STATION_001');
      expect(info.registrationMethod).to.equal(0); // LEGACY_OWNER
      expect(info.totalStakeValueUSD).to.equal(0);
      expect(info.meetsStakeRequirements).to.equal(true);
      expect(info.isEligible).to.equal(true);
    });
  });

  describe('Enhanced Stake Information', function () {
    it('Should provide enhanced stake info', async function () {
      const { wrfToken, owner, station1 } = await loadFixture(
        deployWRFTokenFixture
      );

      const stakeInfo = await wrfToken.getEnhancedStakeInfo(station1.address);

      expect(stakeInfo.hasStake).to.equal(false);
      expect(stakeInfo.legacyStakeAmount).to.equal(0);
      expect(stakeInfo.canRecoverStake).to.equal(false);
      expect(stakeInfo.totalStakeValueUSD).to.equal(0);
      expect(stakeInfo.requiredStakeUSD).to.equal(0);
      expect(stakeInfo.registrationMethod).to.equal(0); // LEGACY_OWNER
      expect(stakeInfo.isDelegated).to.equal(false);
    });

    it('Should show advanced staking system status', async function () {
      const { wrfToken } = await loadFixture(deployWRFTokenFixture);

      const status = await wrfToken.getAdvancedStakingStatus();

      expect(status.permissionlessEnabled).to.equal(false);
      expect(status.advancedStakingEnabled).to.equal(false);
      expect(status.multiAssetStakingAddress).to.equal(ethers.ZeroAddress);
      expect(status.dynamicCalculatorAddress).to.equal(ethers.ZeroAddress);
      expect(status.layawayRegistrationAddress).to.equal(ethers.ZeroAddress);
      expect(status.stakeDelegationAddress).to.equal(ethers.ZeroAddress);
      expect(status.slashingMechanismAddress).to.equal(ethers.ZeroAddress);
    });
  });

  describe('Legacy Registration Compatibility', function () {
    it('Should support legacy ETH staking registration', async function () {
      const { wrfToken, owner, station1 } = await loadFixture(
        deployWRFTokenFixture
      );

      // Enable permissionless registration
      await wrfToken.connect(owner).setPermissionlessRegistration(true);

      // Register with ETH stake
      await expect(
        wrfToken
          .connect(station1)
          .registerWeatherStationWithStake('STATION_ETH', {
            value: ethers.parseEther('0.1'),
          })
      )
        .to.emit(wrfToken, 'WeatherStationStakeRegistered')
        .withArgs(station1.address, 'STATION_ETH', ethers.parseEther('0.1'))
        .and.to.emit(wrfToken, 'WeatherStationRegistered')
        .withArgs(station1.address, 'STATION_ETH');

      // Check registration
      expect(await wrfToken.weatherStations(station1.address)).to.equal(true);
      expect(await wrfToken.stationIds(station1.address)).to.equal(
        'STATION_ETH'
      );
      expect(await wrfToken.stationStakes(station1.address)).to.equal(
        ethers.parseEther('0.1')
      );
      expect(
        await wrfToken.stationRegistrationMethod(station1.address)
      ).to.equal(1); // LEGACY_STAKE
    });

    it('Should allow stake recovery', async function () {
      const { wrfToken, owner, station1 } = await loadFixture(
        deployWRFTokenFixture
      );

      // Enable permissionless registration and register with stake
      await wrfToken.connect(owner).setPermissionlessRegistration(true);
      await wrfToken
        .connect(station1)
        .registerWeatherStationWithStake('STATION_ETH', {
          value: ethers.parseEther('0.1'),
        });

      const initialBalance = await ethers.provider.getBalance(station1.address);

      // Deregister and recover stake
      await expect(wrfToken.connect(station1).deregisterAndRecoverStake())
        .to.emit(wrfToken, 'StakeRecovered')
        .withArgs(station1.address, ethers.parseEther('0.1'))
        .and.to.emit(wrfToken, 'WeatherStationDeregistered')
        .withArgs(station1.address);

      // Check deregistration
      expect(await wrfToken.weatherStations(station1.address)).to.equal(false);
      expect(await wrfToken.stationStakes(station1.address)).to.equal(0);
    });
  });

  describe('Fair Launch Principles', function () {
    it('Should maintain zero pre-mine principle', async function () {
      const { wrfToken } = await loadFixture(deployWRFTokenFixture);

      // Total supply should start at zero
      expect(await wrfToken.totalSupply()).to.equal(0);

      // Max supply should be 1 billion tokens
      expect(await wrfToken.MAX_SUPPLY()).to.equal(
        ethers.parseEther('1000000000')
      );
    });

    it('Should maintain fair launch status', async function () {
      const { wrfToken } = await loadFixture(deployWRFTokenFixture);

      const status = await wrfToken.getFairLaunchStatus();

      expect(status.isZeroPremine).to.equal(true);
      expect(status.isPermissionlessActive).to.equal(false);
      expect(status.totalSupplyNow).to.equal(0);
      expect(status.percentageFromRewards).to.equal(100);
    });
  });

  describe('Enhanced Security Features', function () {
    it('Should enforce stake requirements for reward claims', async function () {
      const { wrfToken, owner, station1 } = await loadFixture(
        deployWRFTokenFixture
      );

      // Register station
      await wrfToken
        .connect(owner)
        .registerWeatherStation(station1.address, 'STATION_001');

      // Should be able to claim rewards initially (meets requirements by default for legacy)
      await expect(wrfToken.connect(station1).claimWeatherReward()).to.emit(
        wrfToken,
        'WeatherRewardClaimed'
      );

      expect(await wrfToken.totalSupply()).to.equal(ethers.parseEther('17.5')); // WEATHER_REWARD_RATE
    });

    it('Should prevent claims from unregistered stations', async function () {
      const { wrfToken, station1 } = await loadFixture(deployWRFTokenFixture);

      await expect(
        wrfToken.connect(station1).claimWeatherReward()
      ).to.be.revertedWith('WRFToken: Not a registered weather station');
    });

    it('Should respect cooldown periods', async function () {
      const { wrfToken, owner, station1 } = await loadFixture(
        deployWRFTokenFixture
      );

      // Register and claim once
      await wrfToken
        .connect(owner)
        .registerWeatherStation(station1.address, 'STATION_001');
      await wrfToken.connect(station1).claimWeatherReward();

      // Should fail to claim again immediately
      await expect(
        wrfToken.connect(station1).claimWeatherReward()
      ).to.be.revertedWith('WRFToken: Reward claimed too recently');
    });
  });
});
