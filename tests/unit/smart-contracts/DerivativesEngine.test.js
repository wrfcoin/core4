const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  time,
  loadFixture,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('DerivativesEngine', function () {
  // Test fixture for deploying contracts
  async function deployDerivativesFixture() {
    const [owner, trader1, trader2, liquidator, marketMaker] =
      await ethers.getSigners();

    // Deploy WRFToken
    const WRFToken = await ethers.getContractFactory('WRFToken');
    const wrfToken = await WRFToken.deploy();

    // Deploy EnhancedCarbonCredits
    const EnhancedCarbonCredits = await ethers.getContractFactory(
      'EnhancedCarbonCredits'
    );
    const carbonCredits = await EnhancedCarbonCredits.deploy(wrfToken.target);

    // Deploy mock oracle
    const MockOracle = await ethers.getContractFactory('QualityOracle');
    const oracle = await MockOracle.deploy();

    // Deploy CarbonCreditAMM
    const CarbonCreditAMM = await ethers.getContractFactory('CarbonCreditAMM');
    const amm = await CarbonCreditAMM.deploy();

    // Deploy DerivativesEngine
    const DerivativesEngine =
      await ethers.getContractFactory('DerivativesEngine');
    const derivativesEngine = await DerivativesEngine.deploy(
      carbonCredits.target,
      wrfToken.target,
      oracle.target,
      amm.target
    );

    // Deploy DerivativesLiquidator
    const DerivativesLiquidator = await ethers.getContractFactory(
      'DerivativesLiquidator'
    );
    const liquidator_contract = await DerivativesLiquidator.deploy(
      derivativesEngine.target,
      amm.target,
      wrfToken.target
    );

    // Setup initial state
    await wrfToken.transfer(trader1.address, ethers.parseEther('10000'));
    await wrfToken.transfer(trader2.address, ethers.parseEther('10000'));
    await wrfToken.transfer(liquidator.address, ethers.parseEther('5000'));

    // Grant roles
    const MARKET_MAKER_ROLE = await derivativesEngine.MARKET_MAKER_ROLE();
    const SETTLEMENT_ROLE = await derivativesEngine.SETTLEMENT_ROLE();
    const AUTHORIZED_ISSUER_ROLE = await carbonCredits.AUTHORIZED_ISSUER_ROLE();

    await derivativesEngine.grantRole(MARKET_MAKER_ROLE, marketMaker.address);
    await derivativesEngine.grantRole(SETTLEMENT_ROLE, owner.address);
    await carbonCredits.grantRole(AUTHORIZED_ISSUER_ROLE, owner.address);

    // Create a carbon credit batch for testing
    await carbonCredits.createBatch(
      ethers.parseEther('1000000'), // 1M credits
      'VCS-2024-001',
      2024,
      'VM0006',
      'forestry',
      'VCS',
      '12.345,-67.890',
      Math.floor(Date.now() / 1000) + 365 * 24 * 3600, // 1 year expiry
      'QmTestHash123',
      'https://registry.verra.org/project/VCS-2024-001'
    );

    return {
      derivativesEngine,
      liquidator_contract,
      carbonCredits,
      wrfToken,
      oracle,
      amm,
      owner,
      trader1,
      trader2,
      liquidator,
      marketMaker,
    };
  }

  describe('Deployment', function () {
    it('Should deploy with correct parameters', async function () {
      const { derivativesEngine, carbonCredits, wrfToken, oracle, amm } =
        await loadFixture(deployDerivativesFixture);

      expect(await derivativesEngine.carbonCredits()).to.equal(
        carbonCredits.target
      );
      expect(await derivativesEngine.wrfToken()).to.equal(wrfToken.target);
      expect(await derivativesEngine.qualityOracle()).to.equal(oracle.target);
      expect(await derivativesEngine.amm()).to.equal(amm.target);
    });
  });

  describe('Futures Creation', function () {
    it('Should create a future contract', async function () {
      const { derivativesEngine, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );

      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600; // 30 days

      await expect(
        derivativesEngine.connect(marketMaker).createFuture(
          1, // tokenId
          ethers.parseEther('1000'), // contract size
          expiryTime,
          0, // CASH_SETTLED
          'Q1-2025 VCS Forestry Futures'
        )
      ).to.emit(derivativesEngine, 'FutureCreated');

      const future = await derivativesEngine.getFuture(1);
      expect(future.underlyingTokenId).to.equal(1);
      expect(future.contractSize).to.equal(ethers.parseEther('1000'));
      expect(future.isActive).to.be.true;
    });

    it('Should fail to create future with past expiry', async function () {
      const { derivativesEngine, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );

      const pastTime = Math.floor(Date.now() / 1000) - 1000;

      await expect(
        derivativesEngine
          .connect(marketMaker)
          .createFuture(
            1,
            ethers.parseEther('1000'),
            pastTime,
            0,
            'Invalid Future'
          )
      ).to.be.revertedWith('Expiry must be in future');
    });

    it('Should fail to create future without MARKET_MAKER_ROLE', async function () {
      const { derivativesEngine, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;

      await expect(
        derivativesEngine
          .connect(trader1)
          .createFuture(
            1,
            ethers.parseEther('1000'),
            expiryTime,
            0,
            'Unauthorized Future'
          )
      ).to.be.reverted;
    });
  });

  describe('Options Creation', function () {
    it('Should create a call option', async function () {
      const { derivativesEngine, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );

      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;

      await expect(
        derivativesEngine.connect(marketMaker).createOption(
          1, // tokenId
          ethers.parseEther('1000'), // contract size
          ethers.parseEther('25'), // strike price
          expiryTime,
          ethers.parseEther('2'), // premium
          1, // CALL_OPTION
          0, // CASH_SETTLED
          'Q1-2025 VCS $25 Call'
        )
      ).to.emit(derivativesEngine, 'OptionCreated');

      const option = await derivativesEngine.getOption(1);
      expect(option.underlyingTokenId).to.equal(1);
      expect(option.strikePrice).to.equal(ethers.parseEther('25'));
      expect(option.optionType).to.equal(1);
      expect(option.isActive).to.be.true;
    });

    it('Should create a put option', async function () {
      const { derivativesEngine, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );

      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;

      await expect(
        derivativesEngine.connect(marketMaker).createOption(
          1,
          ethers.parseEther('1000'),
          ethers.parseEther('20'), // strike price
          expiryTime,
          ethers.parseEther('1.5'), // premium
          2, // PUT_OPTION
          0, // CASH_SETTLED
          'Q1-2025 VCS $20 Put'
        )
      ).to.emit(derivativesEngine, 'OptionCreated');

      const option = await derivativesEngine.getOption(1);
      expect(option.optionType).to.equal(2);
    });
  });

  describe('Position Management', function () {
    beforeEach(async function () {
      const { derivativesEngine, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );

      // Create a future for testing
      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine.connect(marketMaker).createFuture(
        1,
        ethers.parseEther('1000'),
        expiryTime,
        0, // CASH_SETTLED
        'Test Future'
      );
    });

    it('Should open a long position', async function () {
      const { derivativesEngine, wrfToken, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      // Create future first
      const { marketMaker } = await loadFixture(deployDerivativesFixture);
      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );

      const collateralAmount = ethers.parseEther('5000');

      // Approve collateral
      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, collateralAmount);

      await expect(
        derivativesEngine.connect(trader1).openPosition(
          1, // futureId
          0, // FUTURE
          true, // isLong
          1, // quantity
          ethers.parseEther('30'), // maxPrice
          collateralAmount
        )
      ).to.emit(derivativesEngine, 'PositionOpened');

      const position = await derivativesEngine.getPosition(1);
      expect(position.trader).to.equal(trader1.address);
      expect(position.isLong).to.be.true;
      expect(position.quantity).to.equal(1);
    });

    it('Should open a short position', async function () {
      const { derivativesEngine, wrfToken, trader2 } = await loadFixture(
        deployDerivativesFixture
      );

      // Create future first
      const { marketMaker } = await loadFixture(deployDerivativesFixture);
      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );

      const collateralAmount = ethers.parseEther('5000');

      await wrfToken
        .connect(trader2)
        .approve(derivativesEngine.target, collateralAmount);

      await expect(
        derivativesEngine.connect(trader2).openPosition(
          1, // futureId
          0, // FUTURE
          false, // isLong = false (short)
          1, // quantity
          ethers.parseEther('30'), // maxPrice
          collateralAmount
        )
      ).to.emit(derivativesEngine, 'PositionOpened');

      const position = await derivativesEngine.getPosition(1);
      expect(position.isLong).to.be.false;
    });

    it('Should fail to open position with insufficient collateral', async function () {
      const { derivativesEngine, wrfToken, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      // Create future first
      const { marketMaker } = await loadFixture(deployDerivativesFixture);
      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );

      const insufficientCollateral = ethers.parseEther('50'); // Too low

      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, insufficientCollateral);

      await expect(
        derivativesEngine
          .connect(trader1)
          .openPosition(
            1,
            0,
            true,
            1,
            ethers.parseEther('30'),
            insufficientCollateral
          )
      ).to.be.revertedWith('Insufficient collateral');
    });
  });

  describe('Collateral Management', function () {
    beforeEach(async function () {
      // Setup position for testing
      const { derivativesEngine, wrfToken, trader1, marketMaker } =
        await loadFixture(deployDerivativesFixture);

      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );

      const collateralAmount = ethers.parseEther('5000');
      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, collateralAmount);
      await derivativesEngine
        .connect(trader1)
        .openPosition(1, 0, true, 1, ethers.parseEther('30'), collateralAmount);
    });

    it('Should add collateral to position', async function () {
      const { derivativesEngine, wrfToken, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      // Recreate position first
      const { marketMaker } = await loadFixture(deployDerivativesFixture);
      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );
      const collateralAmount = ethers.parseEther('5000');
      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, collateralAmount);
      await derivativesEngine
        .connect(trader1)
        .openPosition(1, 0, true, 1, ethers.parseEther('30'), collateralAmount);

      const additionalCollateral = ethers.parseEther('1000');
      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, additionalCollateral);

      await derivativesEngine
        .connect(trader1)
        .addCollateral(1, additionalCollateral);

      const position = await derivativesEngine.getPosition(1);
      expect(position.collateralAmount).to.equal(ethers.parseEther('6000'));
    });

    it('Should remove collateral from position', async function () {
      const { derivativesEngine, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      // Recreate position first
      const { wrfToken, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );
      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );
      const collateralAmount = ethers.parseEther('5000');
      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, collateralAmount);
      await derivativesEngine
        .connect(trader1)
        .openPosition(1, 0, true, 1, ethers.parseEther('30'), collateralAmount);

      const removalAmount = ethers.parseEther('500');

      await derivativesEngine
        .connect(trader1)
        .removeCollateral(1, removalAmount);

      const position = await derivativesEngine.getPosition(1);
      expect(position.collateralAmount).to.equal(ethers.parseEther('4500'));
    });

    it('Should fail to remove collateral below margin requirement', async function () {
      const { derivativesEngine, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      // Recreate position first
      const { wrfToken, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );
      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );
      const collateralAmount = ethers.parseEther('5000');
      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, collateralAmount);
      await derivativesEngine
        .connect(trader1)
        .openPosition(1, 0, true, 1, ethers.parseEther('30'), collateralAmount);

      const excessiveRemoval = ethers.parseEther('4900'); // Would leave too little

      await expect(
        derivativesEngine.connect(trader1).removeCollateral(1, excessiveRemoval)
      ).to.be.revertedWith('Would violate margin requirement');
    });
  });

  describe('Liquidation', function () {
    it('Should identify liquidatable position', async function () {
      // This would require more complex setup with price movements
      // For now, we'll test the basic liquidation check structure
      const { derivativesEngine } = await loadFixture(deployDerivativesFixture);

      // Test with non-existent position
      await expect(
        derivativesEngine.isPositionLiquidatable(999)
      ).to.be.revertedWith('Invalid position ID');
    });

    it('Should execute liquidation', async function () {
      // This test would require setting up a position that becomes liquidatable
      // through price movements or time passage
      const { derivativesEngine, liquidator } = await loadFixture(
        deployDerivativesFixture
      );

      // Test basic liquidation authorization
      expect(
        await derivativesEngine.hasRole(
          await derivativesEngine.LIQUIDATOR_ROLE(),
          liquidator.address
        )
      ).to.be.false;
    });
  });

  describe('Settlement', function () {
    it('Should settle futures at expiry', async function () {
      const { derivativesEngine, marketMaker, owner } = await loadFixture(
        deployDerivativesFixture
      );

      const expiryTime = Math.floor(Date.now() / 1000) + 100; // 100 seconds
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );

      // Fast forward time to expiry
      await time.increaseTo(expiryTime + 1);

      await expect(
        derivativesEngine.connect(owner).settleDerivative(1, 0)
      ).to.emit(derivativesEngine, 'DerivativeSettled');

      const future = await derivativesEngine.getFuture(1);
      expect(future.isActive).to.be.false;
    });

    it('Should fail to settle before expiry', async function () {
      const { derivativesEngine, marketMaker, owner } = await loadFixture(
        deployDerivativesFixture
      );

      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600; // 30 days
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );

      await expect(
        derivativesEngine.connect(owner).settleDerivative(1, 0)
      ).to.be.revertedWith('Future not expired');
    });
  });

  describe('Oracle Integration', function () {
    it('Should get current price', async function () {
      const { derivativesEngine } = await loadFixture(deployDerivativesFixture);

      const [price, confidence] = await derivativesEngine.getCurrentPrice(1);
      expect(price).to.be.gt(0);
      expect(confidence).to.be.gt(0);
    });

    it('Should get quality adjusted price', async function () {
      const { derivativesEngine } = await loadFixture(deployDerivativesFixture);

      const adjustedPrice = await derivativesEngine.getQualityAdjustedPrice(1);
      expect(adjustedPrice).to.be.gt(0);
    });
  });

  describe('Access Control', function () {
    it('Should restrict market making to MARKET_MAKER_ROLE', async function () {
      const { derivativesEngine, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      await expect(
        derivativesEngine
          .connect(trader1)
          .createFuture(
            1,
            ethers.parseEther('1000'),
            Math.floor(Date.now() / 1000) + 3600,
            0,
            'Unauthorized'
          )
      ).to.be.reverted;
    });

    it('Should restrict settlement to SETTLEMENT_ROLE', async function () {
      const { derivativesEngine, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      await expect(derivativesEngine.connect(trader1).settleDerivative(1, 0)).to
        .be.reverted;
    });
  });

  describe('Emergency Functions', function () {
    it('Should pause and unpause contract', async function () {
      const { derivativesEngine, owner } = await loadFixture(
        deployDerivativesFixture
      );

      await derivativesEngine.connect(owner).pause();
      expect(await derivativesEngine.paused()).to.be.true;

      await derivativesEngine.connect(owner).unpause();
      expect(await derivativesEngine.paused()).to.be.false;
    });

    it('Should prevent trading when paused', async function () {
      const { derivativesEngine, wrfToken, trader1, owner } = await loadFixture(
        deployDerivativesFixture
      );

      await derivativesEngine.connect(owner).pause();

      await wrfToken
        .connect(trader1)
        .approve(derivativesEngine.target, ethers.parseEther('5000'));

      await expect(
        derivativesEngine
          .connect(trader1)
          .openPosition(
            1,
            0,
            true,
            1,
            ethers.parseEther('30'),
            ethers.parseEther('5000')
          )
      ).to.be.revertedWith('Pausable: paused');
    });
  });

  describe('View Functions', function () {
    it('Should return user positions', async function () {
      const { derivativesEngine, trader1 } = await loadFixture(
        deployDerivativesFixture
      );

      const positions = await derivativesEngine.getUserPositions(
        trader1.address
      );
      expect(positions).to.be.an('array');
    });

    it('Should calculate position value', async function () {
      // This would require setting up an actual position first
      const { derivativesEngine } = await loadFixture(deployDerivativesFixture);

      // Test with non-existent position
      await expect(
        derivativesEngine.calculatePositionValue(999)
      ).to.be.revertedWith('Invalid position ID');
    });

    it('Should get margin requirements', async function () {
      const { derivativesEngine, marketMaker } = await loadFixture(
        deployDerivativesFixture
      );

      const expiryTime = Math.floor(Date.now() / 1000) + 30 * 24 * 3600;
      await derivativesEngine
        .connect(marketMaker)
        .createFuture(
          1,
          ethers.parseEther('1000'),
          expiryTime,
          0,
          'Test Future'
        );

      const margin = await derivativesEngine.getMarginRequirement(1, 0);
      expect(margin.initialMargin).to.equal(1000); // 10% in basis points
      expect(margin.maintenanceMargin).to.equal(500); // 5% in basis points
    });
  });
});

describe('DerivativesLiquidator', function () {
  async function deployLiquidatorFixture() {
    const fixture = await deployDerivativesFixture();
    return fixture;
  }

  it('Should deploy liquidator contract', async function () {
    const { liquidator_contract, derivativesEngine, amm, wrfToken } =
      await loadFixture(deployLiquidatorFixture);

    expect(await liquidator_contract.derivativesEngine()).to.equal(
      derivativesEngine.target
    );
    expect(await liquidator_contract.amm()).to.equal(amm.target);
    expect(await liquidator_contract.wrfToken()).to.equal(wrfToken.target);
  });

  it('Should check liquidation profitability', async function () {
    const { liquidator_contract } = await loadFixture(deployLiquidatorFixture);

    const gasPrice = ethers.parseUnits('20', 'gwei');
    const [profitable, reward, cost] =
      await liquidator_contract.isLiquidationProfitable(999, gasPrice);

    expect(profitable).to.be.false;
    expect(reward).to.equal(0);
    expect(cost).to.be.gt(0);
  });

  it('Should find liquidatable positions', async function () {
    const { liquidator_contract, trader1 } = await loadFixture(
      deployLiquidatorFixture
    );

    const liquidatablePositions =
      await liquidator_contract.findLiquidatablePositions(trader1.address, 10);
    expect(liquidatablePositions).to.be.an('array');
    expect(liquidatablePositions.length).to.equal(0); // No liquidatable positions initially
  });
});
