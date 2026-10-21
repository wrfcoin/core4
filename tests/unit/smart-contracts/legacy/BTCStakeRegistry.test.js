const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

/**
 * Comprehensive test suite for BTCStakeRegistry contract
 * Tests WBTC staking, cross-chain integration, and emergency procedures
 */
describe('BTCStakeRegistry - Production Security Tests', function () {
  // Test constants
  const WBTC_DECIMALS = 8;
  const PRECISION = ethers.parseEther('1');
  const MIN_COLLATERAL_RATIO = 150;
  const MAX_COLLATERAL_RATIO = 300;
  const LIQUIDATION_THRESHOLD = 125;
  const MAX_STATION_VALUE = ethers.parseEther('100000'); // $100K
  const BRIDGE_TIMEOUT = 24 * 60 * 60; // 24 hours

  // Test fixture for contract deployment
  async function deployBTCStakeRegistryFixture() {
    const [owner, user1, user2, bridgeOperator, emergencyAdmin, liquidator] =
      await ethers.getSigners();

    // Deploy mock contracts
    const MockERC20 = await ethers.getContractFactory('MockERC20');
    const wbtcToken = await MockERC20.deploy(
      'Wrapped Bitcoin',
      'WBTC',
      WBTC_DECIMALS
    );

    const MockAggregatorV3 =
      await ethers.getContractFactory('MockAggregatorV3');
    const wbtcPriceFeed = await MockAggregatorV3.deploy(
      WBTC_DECIMALS, // decimals
      'WBTC/USD', // description
      1 // version
    );

    const usdPriceFeed = await MockAggregatorV3.deploy(
      8, // decimals
      'USD/USD', // description
      1 // version
    );

    // Set initial WBTC price to $45,000 (8 decimals)
    await wbtcPriceFeed.updateAnswer(4500000000000); // $45,000 with 8 decimals
    await usdPriceFeed.updateAnswer(100000000); // $1.00 with 8 decimals

    // Deploy MultiAssetStaking mock
    const MultiAssetStakingMock = await ethers.getContractFactory(
      'MockMultiAssetStaking'
    );
    const multiAssetStaking = await MultiAssetStakingMock.deploy();

    // Deploy BTCStakeRegistry
    const BTCStakeRegistry =
      await ethers.getContractFactory('BTCStakeRegistry');
    const btcStakeRegistry = await BTCStakeRegistry.deploy(
      await wbtcToken.getAddress(),
      await wbtcPriceFeed.getAddress(),
      await usdPriceFeed.getAddress(),
      await multiAssetStaking.getAddress(),
      owner.address
    );

    // Setup roles
    const BRIDGE_OPERATOR_ROLE = await btcStakeRegistry.BRIDGE_OPERATOR_ROLE();
    const EMERGENCY_ROLE = await btcStakeRegistry.EMERGENCY_ROLE();
    const ORACLE_MANAGER_ROLE = await btcStakeRegistry.ORACLE_MANAGER_ROLE();

    await btcStakeRegistry.grantRole(
      BRIDGE_OPERATOR_ROLE,
      bridgeOperator.address
    );
    await btcStakeRegistry.grantRole(EMERGENCY_ROLE, emergencyAdmin.address);
    await btcStakeRegistry.grantRole(ORACLE_MANAGER_ROLE, owner.address);

    // Mint WBTC to users for testing
    const wbtcAmount = ethers.parseUnits('10', WBTC_DECIMALS); // 10 WBTC
    await wbtcToken.mint(user1.address, wbtcAmount);
    await wbtcToken.mint(user2.address, wbtcAmount);
    await wbtcToken.mint(liquidator.address, wbtcAmount);

    return {
      btcStakeRegistry,
      wbtcToken,
      wbtcPriceFeed,
      usdPriceFeed,
      multiAssetStaking,
      owner,
      user1,
      user2,
      bridgeOperator,
      emergencyAdmin,
      liquidator,
      BRIDGE_OPERATOR_ROLE,
      EMERGENCY_ROLE,
      ORACLE_MANAGER_ROLE,
    };
  }

  // Mock MultiAssetStaking contract for testing
  async function deployMockMultiAssetStaking() {
    const MockMultiAssetStaking = await ethers.getContractFactory(
      'MockMultiAssetStaking'
    );
    return await MockMultiAssetStaking.deploy();
  }

  describe('Deployment and Initialization', function () {
    it('Should deploy with correct initial parameters', async function () {
      const { btcStakeRegistry, wbtcToken, wbtcPriceFeed, usdPriceFeed } =
        await loadFixture(deployBTCStakeRegistryFixture);

      expect(await btcStakeRegistry.wbtcToken()).to.equal(
        await wbtcToken.getAddress()
      );
      expect(await btcStakeRegistry.wbtcPriceFeed()).to.equal(
        await wbtcPriceFeed.getAddress()
      );
      expect(await btcStakeRegistry.usdPriceFeed()).to.equal(
        await usdPriceFeed.getAddress()
      );
      expect(await btcStakeRegistry.emergencyShutdown()).to.be.false;
      expect(await btcStakeRegistry.totalValueLocked()).to.equal(0);
    });

    it('Should set up access control roles correctly', async function () {
      const {
        btcStakeRegistry,
        owner,
        bridgeOperator,
        emergencyAdmin,
        BRIDGE_OPERATOR_ROLE,
        EMERGENCY_ROLE,
      } = await loadFixture(deployBTCStakeRegistryFixture);

      const DEFAULT_ADMIN_ROLE = await btcStakeRegistry.DEFAULT_ADMIN_ROLE();

      expect(await btcStakeRegistry.hasRole(DEFAULT_ADMIN_ROLE, owner.address))
        .to.be.true;
      expect(
        await btcStakeRegistry.hasRole(
          BRIDGE_OPERATOR_ROLE,
          bridgeOperator.address
        )
      ).to.be.true;
      expect(
        await btcStakeRegistry.hasRole(EMERGENCY_ROLE, emergencyAdmin.address)
      ).to.be.true;
    });
  });

  describe('Weather Station Registration', function () {
    it('Should register weather station with valid WBTC collateral', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const stationId = ethers.keccak256(ethers.toUtf8Bytes('test-station-1'));
      const wbtcAmount = ethers.parseUnits('1', WBTC_DECIMALS); // 1 WBTC
      const requiredUsdValue = ethers.parseEther('20000'); // $20K requirement

      // Approve WBTC transfer
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);

      // Register weather station
      await expect(
        btcStakeRegistry
          .connect(user1)
          .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue)
      )
        .to.emit(btcStakeRegistry, 'StationRegistered')
        .withArgs(stationId, user1.address, wbtcAmount, requiredUsdValue);

      // Verify station data
      const station = await btcStakeRegistry.getWeatherStation(stationId);
      expect(station.owner).to.equal(user1.address);
      expect(station.wbtcCollateral).to.equal(wbtcAmount);
      expect(station.requiredCollateral).to.equal(requiredUsdValue);
      expect(station.isActive).to.be.true;
    });

    it('Should reject registration with insufficient collateral ratio', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const stationId = ethers.keccak256(ethers.toUtf8Bytes('test-station-2'));
      const wbtcAmount = ethers.parseUnits('0.5', WBTC_DECIMALS); // 0.5 WBTC ($22.5K)
      const requiredUsdValue = ethers.parseEther('20000'); // $20K requirement (only ~112% ratio)

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue)
      ).to.be.revertedWith('Invalid collateral ratio');
    });

    it('Should prevent duplicate station registration', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const stationId = ethers.keccak256(ethers.toUtf8Bytes('test-station-3'));
      const wbtcAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const requiredUsdValue = ethers.parseEther('20000');

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount * 2n);

      // First registration should succeed
      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue);

      // Second registration with same ID should fail
      await expect(
        btcStakeRegistry
          .connect(user1)
          .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue)
      ).to.be.revertedWith('Station already exists');
    });

    it('Should enforce maximum station value limit', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const stationId = ethers.keccak256(ethers.toUtf8Bytes('test-station-4'));
      const wbtcAmount = ethers.parseUnits('5', WBTC_DECIMALS); // 5 WBTC
      const requiredUsdValue = ethers.parseEther('150000'); // $150K (exceeds MAX_STATION_VALUE)

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue)
      ).to.be.revertedWith('Invalid USD value');
    });
  });

  describe('Collateral Management', function () {
    async function registerStationFixture() {
      const contracts = await deployBTCStakeRegistryFixture();
      const { btcStakeRegistry, wbtcToken, user1 } = contracts;

      const stationId = ethers.keccak256(ethers.toUtf8Bytes('test-station'));
      const wbtcAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const requiredUsdValue = ethers.parseEther('20000');

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);
      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue);

      return { ...contracts, stationId, wbtcAmount, requiredUsdValue };
    }

    it('Should add collateral to existing station', async function () {
      const { btcStakeRegistry, wbtcToken, user1, stationId } =
        await loadFixture(registerStationFixture);

      const additionalWbtc = ethers.parseUnits('0.5', WBTC_DECIMALS);
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), additionalWbtc);

      await expect(
        btcStakeRegistry.connect(user1).addCollateral(stationId, additionalWbtc)
      )
        .to.emit(btcStakeRegistry, 'CollateralAdded')
        .withArgs(
          stationId,
          additionalWbtc,
          ethers.parseUnits('1.5', WBTC_DECIMALS)
        );

      const station = await btcStakeRegistry.getWeatherStation(stationId);
      expect(station.wbtcCollateral).to.equal(
        ethers.parseUnits('1.5', WBTC_DECIMALS)
      );
    });

    it('Should withdraw excess collateral while maintaining minimum ratio', async function () {
      const { btcStakeRegistry, wbtcToken, user1, stationId } =
        await loadFixture(registerStationFixture);

      // Add extra collateral first
      const additionalWbtc = ethers.parseUnits('1', WBTC_DECIMALS);
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), additionalWbtc);
      await btcStakeRegistry
        .connect(user1)
        .addCollateral(stationId, additionalWbtc);

      // Withdraw some collateral
      const withdrawAmount = ethers.parseUnits('0.5', WBTC_DECIMALS);
      const userBalanceBefore = await wbtcToken.balanceOf(user1.address);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .withdrawCollateral(stationId, withdrawAmount)
      )
        .to.emit(btcStakeRegistry, 'CollateralWithdrawn')
        .withArgs(
          stationId,
          withdrawAmount,
          ethers.parseUnits('1.5', WBTC_DECIMALS)
        );

      const userBalanceAfter = await wbtcToken.balanceOf(user1.address);
      expect(userBalanceAfter - userBalanceBefore).to.equal(withdrawAmount);
    });

    it('Should prevent withdrawal that would violate minimum collateral ratio', async function () {
      const { btcStakeRegistry, user1, stationId } = await loadFixture(
        registerStationFixture
      );

      // Try to withdraw too much (would bring ratio below 150%)
      const withdrawAmount = ethers.parseUnits('0.8', WBTC_DECIMALS);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .withdrawCollateral(stationId, withdrawAmount)
      ).to.be.revertedWith('Would violate min collateral ratio');
    });

    it('Should enforce daily withdrawal limits', async function () {
      const { btcStakeRegistry, wbtcToken, user1, stationId } =
        await loadFixture(registerStationFixture);

      // Add substantial collateral
      const largeAmount = ethers.parseUnits('50', WBTC_DECIMALS); // $2.25M worth
      await wbtcToken.mint(user1.address, largeAmount);
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), largeAmount);

      // Register a high-value station
      const highValueStationId = ethers.keccak256(
        ethers.toUtf8Bytes('high-value-station')
      );
      const highRequiredValue = ethers.parseEther('90000'); // $90K

      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(
          highValueStationId,
          largeAmount,
          highRequiredValue
        );

      // Try to withdraw more than daily limit ($1M)
      const excessiveWithdrawAmount = ethers.parseUnits('25', WBTC_DECIMALS); // $1.125M worth

      await expect(
        btcStakeRegistry
          .connect(user1)
          .withdrawCollateral(highValueStationId, excessiveWithdrawAmount)
      ).to.be.revertedWith('Exceeds daily limit');
    });
  });

  describe('Liquidation Mechanism', function () {
    async function setupLiquidatableStationFixture() {
      const contracts = await deployBTCStakeRegistryFixture();
      const { btcStakeRegistry, wbtcToken, wbtcPriceFeed, user1 } = contracts;

      // Register station with minimal collateral
      const stationId = ethers.keccak256(
        ethers.toUtf8Bytes('liquidatable-station')
      );
      const wbtcAmount = ethers.parseUnits('0.7', WBTC_DECIMALS); // 0.7 WBTC
      const requiredUsdValue = ethers.parseEther('20000'); // $20K (157.5% initial ratio)

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);
      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue);

      return { ...contracts, stationId, wbtcAmount, requiredUsdValue };
    }

    it('Should liquidate undercollateralized station', async function () {
      const { btcStakeRegistry, wbtcPriceFeed, liquidator, stationId } =
        await loadFixture(setupLiquidatableStationFixture);

      // Drop WBTC price to trigger liquidation (below 125% threshold)
      await wbtcPriceFeed.updateAnswer(3500000000000); // $35K (brings ratio to ~122.5%)

      const [isLiquidatable] =
        await btcStakeRegistry.checkLiquidationStatus(stationId);
      expect(isLiquidatable).to.be.true;

      // Liquidate the station
      await expect(
        btcStakeRegistry.connect(liquidator).liquidateStation(stationId)
      ).to.emit(btcStakeRegistry, 'StationLiquidated');

      // Verify station is deactivated
      const station = await btcStakeRegistry.getWeatherStation(stationId);
      expect(station.isActive).to.be.false;
      expect(station.isLiquidatable).to.be.true;
    });

    it('Should prevent liquidation of healthy stations', async function () {
      const { btcStakeRegistry, liquidator, stationId } = await loadFixture(
        setupLiquidatableStationFixture
      );

      // Station should not be liquidatable at current price
      const [isLiquidatable] =
        await btcStakeRegistry.checkLiquidationStatus(stationId);
      expect(isLiquidatable).to.be.false;

      await expect(
        btcStakeRegistry.connect(liquidator).liquidateStation(stationId)
      ).to.be.revertedWith('Station not liquidatable');
    });

    it('Should calculate liquidation status correctly', async function () {
      const { btcStakeRegistry, wbtcPriceFeed, stationId } = await loadFixture(
        setupLiquidatableStationFixture
      );

      // Check initial status (healthy)
      let [isLiquidatable, currentRatio] =
        await btcStakeRegistry.checkLiquidationStatus(stationId);
      expect(isLiquidatable).to.be.false;
      expect(currentRatio).to.be.gte(150); // Above minimum ratio

      // Drop price and check again
      await wbtcPriceFeed.updateAnswer(3500000000000); // $35K
      [isLiquidatable, currentRatio] =
        await btcStakeRegistry.checkLiquidationStatus(stationId);
      expect(isLiquidatable).to.be.true;
      expect(currentRatio).to.be.lte(125); // Below liquidation threshold
    });
  });

  describe('Cross-Chain Bridge Operations', function () {
    async function setupBridgeFixture() {
      const contracts = await deployBTCStakeRegistryFixture();
      const { btcStakeRegistry, wbtcToken, user1 } = contracts;

      // Deploy mock bridge
      const MockBridge = await ethers.getContractFactory(
        'MockCrossChainBridge'
      );
      const mockBridge = await MockBridge.deploy();

      // Add bridge as supported
      await btcStakeRegistry.addSupportedBridge(await mockBridge.getAddress());

      return { ...contracts, mockBridge };
    }

    it('Should initiate bridge operation successfully', async function () {
      const { btcStakeRegistry, wbtcToken, user1, mockBridge } =
        await loadFixture(setupBridgeFixture);

      const bridgeAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const destinationChain = 137; // Polygon
      const recipient = user1.address;

      // Approve and initiate bridge
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), bridgeAmount);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .initiateBridgeOperation(
            await mockBridge.getAddress(),
            bridgeAmount,
            destinationChain,
            recipient
          )
      ).to.emit(btcStakeRegistry, 'BridgeOperationInitiated');
    });

    it('Should confirm bridge operation', async function () {
      const { btcStakeRegistry, wbtcToken, user1, mockBridge, bridgeOperator } =
        await loadFixture(setupBridgeFixture);

      const bridgeAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const destinationChain = 137;
      const recipient = user1.address;

      // Initiate bridge
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), bridgeAmount);
      await btcStakeRegistry
        .connect(user1)
        .initiateBridgeOperation(
          await mockBridge.getAddress(),
          bridgeAmount,
          destinationChain,
          recipient
        );

      const nonce = await btcStakeRegistry.bridgeNonce();
      const txHash = ethers.keccak256(
        ethers.toUtf8Bytes('destination-tx-hash')
      );

      // Confirm bridge operation
      await expect(
        btcStakeRegistry
          .connect(bridgeOperator)
          .confirmBridgeOperation(nonce, txHash)
      ).to.emit(btcStakeRegistry, 'BridgeOperationCompleted');

      const operation = await btcStakeRegistry.getBridgeOperation(nonce);
      expect(operation.txHash).to.equal(txHash);
      expect(operation.status).to.equal(1); // BridgeStatus.CONFIRMED
    });

    it('Should process bridge timeouts with automatic refunds', async function () {
      const { btcStakeRegistry, wbtcToken, user1, mockBridge } =
        await loadFixture(setupBridgeFixture);

      const bridgeAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const destinationChain = 137;
      const recipient = user1.address;

      // Initiate bridge
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), bridgeAmount);
      await btcStakeRegistry
        .connect(user1)
        .initiateBridgeOperation(
          await mockBridge.getAddress(),
          bridgeAmount,
          destinationChain,
          recipient
        );

      const nonce = await btcStakeRegistry.bridgeNonce();

      // Fast forward time beyond timeout
      await time.increase(BRIDGE_TIMEOUT + 1);

      const userBalanceBefore = await wbtcToken.balanceOf(user1.address);

      // Process timeout
      await expect(btcStakeRegistry.processBridgeTimeout(nonce)).to.emit(
        btcStakeRegistry,
        'BridgeTimeoutProcessed'
      );

      const userBalanceAfter = await wbtcToken.balanceOf(user1.address);
      expect(userBalanceAfter - userBalanceBefore).to.equal(bridgeAmount);

      const operation = await btcStakeRegistry.getBridgeOperation(nonce);
      expect(operation.status).to.equal(3); // BridgeStatus.TIMEOUT
    });

    it('Should reject operations from unsupported bridges', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const MockBridge = await ethers.getContractFactory(
        'MockCrossChainBridge'
      );
      const unsupportedBridge = await MockBridge.deploy();

      const bridgeAmount = ethers.parseUnits('1', WBTC_DECIMALS);

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), bridgeAmount);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .initiateBridgeOperation(
            await unsupportedBridge.getAddress(),
            bridgeAmount,
            137,
            user1.address
          )
      ).to.be.revertedWith('Unsupported bridge');
    });
  });

  describe('Emergency Procedures', function () {
    it('Should activate emergency shutdown', async function () {
      const { btcStakeRegistry, emergencyAdmin } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      await expect(
        btcStakeRegistry.connect(emergencyAdmin).activateEmergencyShutdown()
      ).to.emit(btcStakeRegistry, 'EmergencyShutdownActivated');

      expect(await btcStakeRegistry.emergencyShutdown()).to.be.true;
      expect(await btcStakeRegistry.paused()).to.be.true;
    });

    it('Should allow emergency withdrawal after shutdown delay', async function () {
      const { btcStakeRegistry, wbtcToken, user1, emergencyAdmin } =
        await loadFixture(deployBTCStakeRegistryFixture);

      // Register a station first
      const stationId = ethers.keccak256(
        ethers.toUtf8Bytes('emergency-station')
      );
      const wbtcAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const requiredUsdValue = ethers.parseEther('20000');

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);
      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue);

      // Activate emergency shutdown
      await btcStakeRegistry
        .connect(emergencyAdmin)
        .activateEmergencyShutdown();

      // Fast forward beyond emergency delay
      const EMERGENCY_DELAY = 48 * 60 * 60; // 48 hours
      await time.increase(EMERGENCY_DELAY + 1);

      const userBalanceBefore = await wbtcToken.balanceOf(user1.address);

      // Emergency withdrawal should succeed
      await expect(
        btcStakeRegistry.connect(user1).emergencyWithdraw(stationId)
      ).to.emit(btcStakeRegistry, 'CollateralWithdrawn');

      const userBalanceAfter = await wbtcToken.balanceOf(user1.address);
      expect(userBalanceAfter - userBalanceBefore).to.equal(wbtcAmount);
    });

    it('Should prevent emergency withdrawal before delay period', async function () {
      const { btcStakeRegistry, wbtcToken, user1, emergencyAdmin } =
        await loadFixture(deployBTCStakeRegistryFixture);

      // Register a station
      const stationId = ethers.keccak256(ethers.toUtf8Bytes('test-station'));
      const wbtcAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const requiredUsdValue = ethers.parseEther('20000');

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);
      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue);

      // Activate emergency shutdown
      await btcStakeRegistry
        .connect(emergencyAdmin)
        .activateEmergencyShutdown();

      // Try immediate emergency withdrawal (should fail)
      await expect(
        btcStakeRegistry.connect(user1).emergencyWithdraw(stationId)
      ).to.be.revertedWith('Emergency delay not met');
    });

    it('Should pause and unpause contract operations', async function () {
      const { btcStakeRegistry, emergencyAdmin, owner } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      // Pause contract
      await btcStakeRegistry.connect(emergencyAdmin).pause();
      expect(await btcStakeRegistry.paused()).to.be.true;

      // Unpause contract
      await btcStakeRegistry.connect(owner).unpause();
      expect(await btcStakeRegistry.paused()).to.be.false;
    });
  });

  describe('Price Oracle Integration', function () {
    it('Should get WBTC price correctly', async function () {
      const { btcStakeRegistry } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const price = await btcStakeRegistry.getWBTCPrice();
      const expectedPrice = ethers.parseEther('45000'); // $45,000 with 18 decimals

      expect(price).to.equal(expectedPrice);
    });

    it('Should update price feeds', async function () {
      const { btcStakeRegistry, owner } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const MockAggregatorV3 =
        await ethers.getContractFactory('MockAggregatorV3');
      const newWbtcFeed = await MockAggregatorV3.deploy(8, 'NEW-WBTC/USD', 1);
      const newUsdFeed = await MockAggregatorV3.deploy(8, 'NEW-USD/USD', 1);

      await expect(
        btcStakeRegistry
          .connect(owner)
          .updatePriceFeeds(
            await newWbtcFeed.getAddress(),
            await newUsdFeed.getAddress()
          )
      ).to.emit(btcStakeRegistry, 'PriceFeedUpdated');

      expect(await btcStakeRegistry.wbtcPriceFeed()).to.equal(
        await newWbtcFeed.getAddress()
      );
      expect(await btcStakeRegistry.usdPriceFeed()).to.equal(
        await newUsdFeed.getAddress()
      );
    });

    it('Should reject stale price data', async function () {
      const { btcStakeRegistry, wbtcPriceFeed } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      // Set a very old timestamp to make price stale
      const oldTimestamp = Math.floor(Date.now() / 1000) - 7200; // 2 hours ago
      await wbtcPriceFeed.updateRoundData(
        1, // roundId
        4500000000000, // answer
        oldTimestamp, // startedAt
        oldTimestamp, // updatedAt
        1 // answeredInRound
      );

      await expect(btcStakeRegistry.getWBTCPrice()).to.be.revertedWith(
        'WBTC price too stale'
      );
    });
  });

  describe('Administrative Functions', function () {
    it('Should add and remove supported bridges', async function () {
      const { btcStakeRegistry, owner } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const MockBridge = await ethers.getContractFactory(
        'MockCrossChainBridge'
      );
      const newBridge = await MockBridge.deploy();

      // Add bridge
      await btcStakeRegistry
        .connect(owner)
        .addSupportedBridge(await newBridge.getAddress());
      expect(
        await btcStakeRegistry.isSupportedBridge(await newBridge.getAddress())
      ).to.be.true;

      // Remove bridge
      await btcStakeRegistry
        .connect(owner)
        .removeSupportedBridge(await newBridge.getAddress());
      expect(
        await btcStakeRegistry.isSupportedBridge(await newBridge.getAddress())
      ).to.be.false;
    });

    it('Should update security parameters', async function () {
      const { btcStakeRegistry, owner } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const newMaxTVL = ethers.parseEther('100000000'); // $100M
      const newDailyLimit = ethers.parseEther('2000000'); // $2M

      await btcStakeRegistry
        .connect(owner)
        .updateSecurityParameters(newMaxTVL, newDailyLimit);

      expect(await btcStakeRegistry.maxTotalValueLocked()).to.equal(newMaxTVL);
      expect(await btcStakeRegistry.dailyWithdrawalLimit()).to.equal(
        newDailyLimit
      );
    });

    it('Should recover stuck tokens during emergency', async function () {
      const { btcStakeRegistry, owner, emergencyAdmin } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      // Deploy a different token (not WBTC)
      const MockERC20 = await ethers.getContractFactory('MockERC20');
      const stuckToken = await MockERC20.deploy('Stuck Token', 'STUCK', 18);

      // Send some tokens to the contract
      const stuckAmount = ethers.parseEther('100');
      await stuckToken.mint(await btcStakeRegistry.getAddress(), stuckAmount);

      // Activate emergency shutdown first
      await btcStakeRegistry
        .connect(emergencyAdmin)
        .activateEmergencyShutdown();

      // Recover stuck tokens
      await btcStakeRegistry
        .connect(owner)
        .recoverStuckTokens(
          await stuckToken.getAddress(),
          stuckAmount,
          owner.address
        );

      expect(await stuckToken.balanceOf(owner.address)).to.equal(stuckAmount);
    });

    it('Should prevent recovery of WBTC collateral', async function () {
      const { btcStakeRegistry, wbtcToken, owner, emergencyAdmin } =
        await loadFixture(deployBTCStakeRegistryFixture);

      // Activate emergency shutdown
      await btcStakeRegistry
        .connect(emergencyAdmin)
        .activateEmergencyShutdown();

      await expect(
        btcStakeRegistry
          .connect(owner)
          .recoverStuckTokens(
            await wbtcToken.getAddress(),
            ethers.parseUnits('1', WBTC_DECIMALS),
            owner.address
          )
      ).to.be.revertedWith('Cannot recover WBTC collateral');
    });
  });

  describe('Health Status and Monitoring', function () {
    it('Should report correct health status', async function () {
      const { btcStakeRegistry } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const [isHealthy, tvlUtilization, failedOperationRate] =
        await btcStakeRegistry.getHealthStatus();

      expect(isHealthy).to.be.true;
      expect(tvlUtilization).to.equal(0); // No TVL initially
      expect(failedOperationRate).to.equal(0); // No operations initially
    });

    it('Should detect unhealthy status during emergency', async function () {
      const { btcStakeRegistry, emergencyAdmin } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      // Activate emergency shutdown
      await btcStakeRegistry
        .connect(emergencyAdmin)
        .activateEmergencyShutdown();

      const [isHealthy] = await btcStakeRegistry.getHealthStatus();
      expect(isHealthy).to.be.false;
    });
  });

  describe('Economic Security Mechanisms', function () {
    it('Should enforce TVL limits', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      // Try to register station that would exceed TVL limit
      const massiveAmount = ethers.parseUnits('2000', WBTC_DECIMALS); // $90M worth at $45K/BTC
      await wbtcToken.mint(user1.address, massiveAmount);
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), massiveAmount);

      const stationId = ethers.keccak256(ethers.toUtf8Bytes('massive-station'));
      const requiredValue = ethers.parseEther('80000'); // $80K requirement

      await expect(
        btcStakeRegistry
          .connect(user1)
          .registerWeatherStation(stationId, massiveAmount, requiredValue)
      ).to.be.revertedWith('Exceeds TVL limit');
    });

    it('Should track daily withdrawal limits correctly', async function () {
      const { btcStakeRegistry, wbtcToken, user1, owner } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      // Set a lower daily limit for testing
      const testDailyLimit = ethers.parseEther('50000'); // $50K
      await btcStakeRegistry.connect(owner).updateSecurityParameters(
        ethers.parseEther('100000000'), // Keep high TVL limit
        testDailyLimit
      );

      // Register station with high collateral
      const highAmount = ethers.parseUnits('2', WBTC_DECIMALS); // $90K worth
      await wbtcToken.mint(user1.address, highAmount);
      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), highAmount);

      const stationId = ethers.keccak256(
        ethers.toUtf8Bytes('high-collateral-station')
      );
      const requiredValue = ethers.parseEther('30000'); // $30K requirement

      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(stationId, highAmount, requiredValue);

      // Try to withdraw amount that exceeds daily limit
      const excessiveWithdraw = ethers.parseUnits('1.5', WBTC_DECIMALS); // $67.5K worth

      await expect(
        btcStakeRegistry
          .connect(user1)
          .withdrawCollateral(stationId, excessiveWithdraw)
      ).to.be.revertedWith('Exceeds daily limit');
    });
  });

  describe('Integration with MultiAssetStaking', function () {
    it('Should integrate with staking contract on registration', async function () {
      const { btcStakeRegistry, wbtcToken, multiAssetStaking, user1 } =
        await loadFixture(deployBTCStakeRegistryFixture);

      const stationId = ethers.keccak256(
        ethers.toUtf8Bytes('integrated-station')
      );
      const wbtcAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const requiredUsdValue = ethers.parseEther('20000');

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);

      // Register station (should trigger staking integration)
      await btcStakeRegistry
        .connect(user1)
        .registerWeatherStation(stationId, wbtcAmount, requiredUsdValue);

      // Verify integration occurred (mock contract would record this)
      expect(await multiAssetStaking.lastStakedAmount()).to.equal(wbtcAmount);
      expect(await multiAssetStaking.lastStaker()).to.equal(user1.address);
    });
  });

  describe('Access Control Security', function () {
    it('Should prevent unauthorized role assignments', async function () {
      const { btcStakeRegistry, user1, BRIDGE_OPERATOR_ROLE } =
        await loadFixture(deployBTCStakeRegistryFixture);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .grantRole(BRIDGE_OPERATOR_ROLE, user1.address)
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent unauthorized emergency actions', async function () {
      const { btcStakeRegistry, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      await expect(
        btcStakeRegistry.connect(user1).activateEmergencyShutdown()
      ).to.be.revertedWith('AccessControl:');

      await expect(btcStakeRegistry.connect(user1).pause()).to.be.revertedWith(
        'AccessControl:'
      );
    });

    it('Should prevent unauthorized bridge confirmations', async function () {
      const { btcStakeRegistry, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      await expect(
        btcStakeRegistry
          .connect(user1)
          .confirmBridgeOperation(
            1,
            ethers.keccak256(ethers.toUtf8Bytes('fake-tx'))
          )
      ).to.be.revertedWith('AccessControl:');
    });
  });

  describe('Input Validation and Edge Cases', function () {
    it('Should validate station ID format', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const wbtcAmount = ethers.parseUnits('1', WBTC_DECIMALS);
      const requiredUsdValue = ethers.parseEther('20000');

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), wbtcAmount);

      await expect(
        btcStakeRegistry.connect(user1).registerWeatherStation(
          ethers.ZeroHash, // Invalid zero hash
          wbtcAmount,
          requiredUsdValue
        )
      ).to.be.revertedWith('Invalid station ID');
    });

    it('Should validate collateral amounts', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const stationId = ethers.keccak256(ethers.toUtf8Bytes('test-station'));
      const requiredUsdValue = ethers.parseEther('20000');

      await wbtcToken
        .connect(user1)
        .approve(
          await btcStakeRegistry.getAddress(),
          ethers.parseUnits('1', WBTC_DECIMALS)
        );

      await expect(
        btcStakeRegistry.connect(user1).registerWeatherStation(
          stationId,
          0, // Invalid zero amount
          requiredUsdValue
        )
      ).to.be.revertedWith('Invalid WBTC amount');
    });

    it('Should handle edge case collateral ratios', async function () {
      const { btcStakeRegistry, wbtcToken, user1 } = await loadFixture(
        deployBTCStakeRegistryFixture
      );

      const stationId = ethers.keccak256(
        ethers.toUtf8Bytes('edge-case-station')
      );

      // Test exact minimum ratio (150%)
      const minWbtcAmount = ethers.parseUnits('0.67', WBTC_DECIMALS); // ~$30,150 worth
      const requiredValue = ethers.parseEther('20000'); // Exactly 150.75% ratio

      await wbtcToken
        .connect(user1)
        .approve(await btcStakeRegistry.getAddress(), minWbtcAmount);

      await expect(
        btcStakeRegistry
          .connect(user1)
          .registerWeatherStation(stationId, minWbtcAmount, requiredValue)
      ).to.not.be.reverted;
    });
  });
});

// Mock contract for testing
async function deployMockContracts() {
  // These would be deployed as part of the test setup
  const mockContracts = {
    mockERC20: 'MockERC20.sol',
    mockAggregator: 'MockAggregatorV3.sol',
    mockMultiAssetStaking: 'MockMultiAssetStaking.sol',
    mockCrossChainBridge: 'MockCrossChainBridge.sol',
  };

  return mockContracts;
}
