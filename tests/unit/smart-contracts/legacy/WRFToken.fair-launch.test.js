const { expect } = require('chai');
const { ethers } = require('hardhat');
const { loadFixture } = require('@nomicfoundation/hardhat-network-helpers');

describe('WRFToken Fair Launch Tests', function () {
  async function deployTokenFixture() {
    const [owner, weatherStation, staker] = await ethers.getSigners();

    const WRFToken = await ethers.getContractFactory('WRFToken');
    const token = await WRFToken.deploy();
    await token.waitForDeployment();

    return { token, owner, weatherStation, staker };
  }

  describe('Fair Launch Verification', function () {
    it('Should start with 0 total supply (no pre-mine)', async function () {
      const { token } = await loadFixture(deployTokenFixture);

      const totalSupply = await token.totalSupply();
      expect(totalSupply).to.equal(0);
    });

    it('Should have 0 balance for deployer (no pre-mine)', async function () {
      const { token, owner } = await loadFixture(deployTokenFixture);

      const ownerBalance = await token.balanceOf(owner.address);
      expect(ownerBalance).to.equal(0);
    });

    it('Should preserve MAX_SUPPLY constant at 1B tokens', async function () {
      const { token } = await loadFixture(deployTokenFixture);

      const maxSupply = await token.MAX_SUPPLY();
      expect(maxSupply).to.equal(ethers.parseEther('1000000000')); // 1B tokens
    });

    it('Should have full mintable supply available initially', async function () {
      const { token } = await loadFixture(deployTokenFixture);

      const remainingSupply = await token.remainingMintableSupply();
      const maxSupply = await token.MAX_SUPPLY();
      expect(remainingSupply).to.equal(maxSupply);
    });
  });

  describe('Token Minting Through Rewards Only', function () {
    it('Should allow weather station reward as first tokens minted', async function () {
      const { token, owner, weatherStation } =
        await loadFixture(deployTokenFixture);

      // Register weather station
      await token
        .connect(owner)
        .registerWeatherStation(weatherStation.address, 'STATION_001');

      // Claim first reward (should be first tokens in existence)
      await token.connect(weatherStation).claimWeatherReward();

      const totalSupply = await token.totalSupply();
      const weatherRewardRate = await token.WEATHER_REWARD_RATE();
      const stationBalance = await token.balanceOf(weatherStation.address);

      expect(totalSupply).to.equal(weatherRewardRate);
      expect(stationBalance).to.equal(weatherRewardRate);
      expect(totalSupply).to.equal(ethers.parseEther('17.5')); // 17.5 WRF
    });

    it('Should allow staking rewards as first tokens minted', async function () {
      const { token, owner, staker } = await loadFixture(deployTokenFixture);

      // Set owner as reward distributor (owner is automatically authorized)
      const rewardAmount = ethers.parseEther('100');

      // Mint staking reward (should be first tokens in existence)
      await token
        .connect(owner)
        .mintStakingReward(
          staker.address,
          rewardAmount,
          'Computational NFT Staking'
        );

      const totalSupply = await token.totalSupply();
      const stakerBalance = await token.balanceOf(staker.address);

      expect(totalSupply).to.equal(rewardAmount);
      expect(stakerBalance).to.equal(rewardAmount);
    });

    it('Should enforce max supply limit with fair launch', async function () {
      const { token, owner, staker } = await loadFixture(deployTokenFixture);

      const maxSupply = await token.MAX_SUPPLY();

      // Try to mint more than max supply
      await expect(
        token
          .connect(owner)
          .mintStakingReward(staker.address, maxSupply + 1n, 'Overflow test')
      ).to.be.revertedWith('WRFToken: Max supply exceeded');
    });
  });

  describe('Contract State Verification', function () {
    it('Should maintain all security features with fair launch', async function () {
      const { token, owner } = await loadFixture(deployTokenFixture);

      // Verify pause functionality
      await token.connect(owner).pause();
      expect(await token.paused()).to.be.true;

      await token.connect(owner).unpause();
      expect(await token.paused()).to.be.false;

      // Verify weather reward rate
      const rewardRate = await token.WEATHER_REWARD_RATE();
      expect(rewardRate).to.equal(ethers.parseEther('17.5'));

      // Verify max daily reward
      const maxDailyReward = await token.MAX_DAILY_REWARD();
      expect(maxDailyReward).to.equal(ethers.parseEther('420'));
    });

    it('Should preserve all contract functionality', async function () {
      const { token, owner, weatherStation } =
        await loadFixture(deployTokenFixture);

      // Register weather station
      await token
        .connect(owner)
        .registerWeatherStation(weatherStation.address, 'TEST_STATION');

      // Check station info
      const stationInfo = await token.getWeatherStationInfo(
        weatherStation.address
      );
      expect(stationInfo.isRegistered).to.be.true;
      expect(stationInfo.stationId).to.equal('TEST_STATION');

      // Check reward availability
      const rewardInfo = await token.canClaimReward(weatherStation.address);
      expect(rewardInfo.canClaim).to.be.true;
    });
  });
});
