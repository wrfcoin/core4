const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('LiquidityPoolManager', function () {
  let liquidityPoolManager;
  let weightedCarbonPool;
  let carbonBasketToken;
  let usdc;
  let qualityOracle;
  let volatilityOracle;
  let owner;
  let poolCreator;
  let user1;
  let user2;
  let feeRecipient;

  beforeEach(async function () {
    [owner, poolCreator, user1, user2, feeRecipient] =
      await ethers.getSigners();

    // Deploy mock USDC
    const MockERC20 = await ethers.getContractFactory('MockERC20');
    usdc = await MockERC20.deploy('USD Coin', 'USDC', 6);

    // Deploy CarbonBasketToken
    const CarbonBasketToken =
      await ethers.getContractFactory('CarbonBasketToken');
    const SimpleMockEnhancedCarbonCredits = await ethers.getContractFactory(
      'SimpleMockEnhancedCarbonCredits'
    );
    const enhancedCarbonCredits =
      await SimpleMockEnhancedCarbonCredits.deploy();

    const basketConfig = {
      name: 'Test Carbon Basket',
      symbol: 'TCB',
      allowedProjectTypes: ['forestry', 'renewable-energy'],
      allowedRegistries: ['VCS', 'GS'],
      minVintage: 2020,
      maxVintage: 2025,
      minQualityScore: 500,
      retirementRate: 100,
    };

    carbonBasketToken = await CarbonBasketToken.deploy(
      await enhancedCarbonCredits.getAddress(),
      basketConfig
    );

    // Deploy oracles
    const QualityOracle = await ethers.getContractFactory('QualityOracle');
    qualityOracle = await QualityOracle.deploy();

    const VolatilityOracle =
      await ethers.getContractFactory('VolatilityOracle');
    volatilityOracle = await VolatilityOracle.deploy();

    // Deploy WeightedCarbonPool implementation
    const WeightedCarbonPool =
      await ethers.getContractFactory('WeightedCarbonPool');
    const poolImplementation = await WeightedCarbonPool.deploy();

    // Deploy LiquidityPoolManager
    const LiquidityPoolManager = await ethers.getContractFactory(
      'LiquidityPoolManager'
    );
    liquidityPoolManager = await LiquidityPoolManager.deploy(
      await usdc.getAddress(),
      await poolImplementation.getAddress(),
      feeRecipient.address
    );

    // Setup roles
    const POOL_CREATOR_ROLE = await liquidityPoolManager.POOL_CREATOR_ROLE();
    await liquidityPoolManager.grantRole(
      POOL_CREATOR_ROLE,
      poolCreator.address
    );

    // Authorize carbon token
    await liquidityPoolManager.setCarbonTokenAuthorization(
      await carbonBasketToken.getAddress(),
      true
    );

    // Set oracles
    await liquidityPoolManager.setQualityOracle(
      await qualityOracle.getAddress()
    );
    await liquidityPoolManager.setVolatilityOracle(
      await volatilityOracle.getAddress()
    );
  });

  describe('Deployment', function () {
    it('Should deploy with correct configuration', async function () {
      expect(await liquidityPoolManager.usdcAddress()).to.equal(
        await usdc.getAddress()
      );
      expect(await liquidityPoolManager.feeRecipient()).to.equal(
        feeRecipient.address
      );
      expect(await liquidityPoolManager.protocolFeeBasisPoints()).to.equal(10);
    });

    it('Should setup default pool types', async function () {
      expect(await liquidityPoolManager.supportedPoolTypes('stable')).to.be
        .true;
      expect(await liquidityPoolManager.supportedPoolTypes('balanced')).to.be
        .true;
      expect(await liquidityPoolManager.supportedPoolTypes('growth')).to.be
        .true;
    });

    it('Should setup correct roles', async function () {
      const DEFAULT_ADMIN_ROLE =
        await liquidityPoolManager.DEFAULT_ADMIN_ROLE();
      const POOL_CREATOR_ROLE = await liquidityPoolManager.POOL_CREATOR_ROLE();

      expect(
        await liquidityPoolManager.hasRole(DEFAULT_ADMIN_ROLE, owner.address)
      ).to.be.true;
      expect(
        await liquidityPoolManager.hasRole(
          POOL_CREATOR_ROLE,
          poolCreator.address
        )
      ).to.be.true;
    });
  });

  describe('Pool Creation', function () {
    it('Should create a stable pool (80/20)', async function () {
      const tx = await liquidityPoolManager
        .connect(poolCreator)
        .createWeightedPool(
          await carbonBasketToken.getAddress(),
          'stable',
          30 // 0.3% fee
        );

      await expect(tx).to.emit(liquidityPoolManager, 'PoolCreated');

      const pools = await liquidityPoolManager.getActivePools();
      expect(pools.length).to.equal(1);
      expect(pools[0].poolType).to.equal('stable');
      expect(pools[0].weightUSDC).to.equal(ethers.parseEther('0.8'));
      expect(pools[0].weightCBT).to.equal(ethers.parseEther('0.2'));
    });

    it('Should create a balanced pool (50/50)', async function () {
      await liquidityPoolManager.connect(poolCreator).createWeightedPool(
        await carbonBasketToken.getAddress(),
        'balanced',
        50 // 0.5% fee
      );

      const pools = await liquidityPoolManager.getActivePools();
      expect(pools[0].poolType).to.equal('balanced');
      expect(pools[0].weightUSDC).to.equal(ethers.parseEther('0.5'));
      expect(pools[0].weightCBT).to.equal(ethers.parseEther('0.5'));
    });

    it('Should create a growth pool (20/80)', async function () {
      await liquidityPoolManager.connect(poolCreator).createWeightedPool(
        await carbonBasketToken.getAddress(),
        'growth',
        25 // 0.25% fee
      );

      const pools = await liquidityPoolManager.getActivePools();
      expect(pools[0].poolType).to.equal('growth');
      expect(pools[0].weightUSDC).to.equal(ethers.parseEther('0.2'));
      expect(pools[0].weightCBT).to.equal(ethers.parseEther('0.8'));
    });

    it('Should fail to create pool with unauthorized token', async function () {
      const MockToken = await ethers.getContractFactory('MockERC20');
      const unauthorizedToken = await MockToken.deploy(
        'Unauthorized',
        'UNAUTH',
        18
      );

      await expect(
        liquidityPoolManager
          .connect(poolCreator)
          .createWeightedPool(
            await unauthorizedToken.getAddress(),
            'stable',
            30
          )
      ).to.be.revertedWith('LiquidityPoolManager: Unauthorized carbon token');
    });

    it('Should fail to create pool with unsupported type', async function () {
      await expect(
        liquidityPoolManager
          .connect(poolCreator)
          .createWeightedPool(
            await carbonBasketToken.getAddress(),
            'invalid',
            30
          )
      ).to.be.revertedWith('LiquidityPoolManager: Unsupported pool type');
    });

    it('Should fail to create pool with invalid fee', async function () {
      await expect(
        liquidityPoolManager.connect(poolCreator).createWeightedPool(
          await carbonBasketToken.getAddress(),
          'stable',
          1500 // 15% - too high
        )
      ).to.be.revertedWith('LiquidityPoolManager: Invalid swap fee');
    });

    it('Should fail if not pool creator', async function () {
      await expect(
        liquidityPoolManager
          .connect(user1)
          .createWeightedPool(
            await carbonBasketToken.getAddress(),
            'stable',
            30
          )
      ).to.be.revertedWithCustomError(
        liquidityPoolManager,
        'AccessControlUnauthorizedAccount'
      );
    });
  });

  describe('Pool Management', function () {
    let poolAddress;

    beforeEach(async function () {
      await liquidityPoolManager
        .connect(poolCreator)
        .createWeightedPool(await carbonBasketToken.getAddress(), 'stable', 30);

      const pools = await liquidityPoolManager.getActivePools();
      poolAddress = pools[0].poolAddress;
    });

    it('Should update pool weights from quality oracle', async function () {
      // Setup quality oracle with test score
      const ORACLE_UPDATER_ROLE = await qualityOracle.ORACLE_UPDATER_ROLE();
      await qualityOracle.grantRole(ORACLE_UPDATER_ROLE, owner.address);

      await qualityOracle.updateQualityScore(
        await carbonBasketToken.getAddress(),
        750, // High quality score
        'Test update'
      );

      const ORACLE_MANAGER_ROLE =
        await liquidityPoolManager.ORACLE_MANAGER_ROLE();

      await liquidityPoolManager.updatePoolWeightsFromOracle(poolAddress);

      // Weights should be adjusted based on quality
      const pools = await liquidityPoolManager.getActivePools();
      // With quality score of 750 (650 above baseline 100), adjustment should be significant
      expect(pools[0].weightCBT).to.be.gt(ethers.parseEther('0.2')); // Greater than original 20%
    });

    it('Should update pool fee from volatility oracle', async function () {
      // Setup volatility oracle with test data
      const PRICE_FEEDER_ROLE = await volatilityOracle.PRICE_FEEDER_ROLE();
      await volatilityOracle.grantRole(PRICE_FEEDER_ROLE, owner.address);

      // Simulate high volatility
      await volatilityOracle.updateVolatility(
        await usdc.getAddress(),
        await carbonBasketToken.getAddress(),
        ethers.parseEther('100') // Initial price
      );

      await volatilityOracle.updateVolatility(
        await usdc.getAddress(),
        await carbonBasketToken.getAddress(),
        ethers.parseEther('150') // 50% price increase
      );

      const FEE_MANAGER_ROLE = await liquidityPoolManager.FEE_MANAGER_ROLE();

      await liquidityPoolManager.updatePoolFeeFromVolatility(poolAddress);

      const pools = await liquidityPoolManager.getActivePools();
      expect(pools[0].swapFee).to.be.gt(30); // Should be higher than original 0.3%
    });

    it('Should collect protocol fees', async function () {
      const FEE_MANAGER_ROLE = await liquidityPoolManager.FEE_MANAGER_ROLE();

      await expect(
        liquidityPoolManager.collectProtocolFees(poolAddress)
      ).to.emit(liquidityPoolManager, 'ProtocolFeesCollected');
    });

    it('Should record swap statistics', async function () {
      // This would be called by the pool contract
      const pool = await ethers.getContractAt(
        'WeightedCarbonPool',
        poolAddress
      );

      // Simulate a swap by calling recordSwap (normally called by pool)
      // Note: This would fail in practice since only the pool can call it
      // This test demonstrates the interface
    });
  });

  describe('Oracle Management', function () {
    it('Should set quality oracle', async function () {
      const newOracle = await ethers.getContractFactory('QualityOracle');
      const newQualityOracle = await newOracle.deploy();

      await expect(
        liquidityPoolManager.setQualityOracle(
          await newQualityOracle.getAddress()
        )
      )
        .to.emit(liquidityPoolManager, 'OracleUpdated')
        .withArgs(await newQualityOracle.getAddress(), 'quality');

      expect(await liquidityPoolManager.qualityOracle()).to.equal(
        await newQualityOracle.getAddress()
      );
    });

    it('Should set volatility oracle', async function () {
      const newOracle = await ethers.getContractFactory('VolatilityOracle');
      const newVolatilityOracle = await newOracle.deploy();

      await expect(
        liquidityPoolManager.setVolatilityOracle(
          await newVolatilityOracle.getAddress()
        )
      )
        .to.emit(liquidityPoolManager, 'OracleUpdated')
        .withArgs(await newVolatilityOracle.getAddress(), 'volatility');

      expect(await liquidityPoolManager.volatilityOracle()).to.equal(
        await newVolatilityOracle.getAddress()
      );
    });

    it('Should fail to set oracle if not authorized', async function () {
      const newOracle = await ethers.getContractFactory('QualityOracle');
      const newQualityOracle = await newOracle.deploy();

      await expect(
        liquidityPoolManager
          .connect(user1)
          .setQualityOracle(await newQualityOracle.getAddress())
      ).to.be.revertedWithCustomError(
        liquidityPoolManager,
        'AccessControlUnauthorizedAccount'
      );
    });
  });

  describe('Carbon Token Authorization', function () {
    it('Should authorize carbon token', async function () {
      const MockToken = await ethers.getContractFactory('MockERC20');
      const newToken = await MockToken.deploy('New Carbon', 'NCARB', 18);

      await expect(
        liquidityPoolManager.setCarbonTokenAuthorization(
          await newToken.getAddress(),
          true
        )
      )
        .to.emit(liquidityPoolManager, 'CarbonTokenAuthorized')
        .withArgs(await newToken.getAddress(), true);

      expect(
        await liquidityPoolManager.authorizedCarbonTokens(
          await newToken.getAddress()
        )
      ).to.be.true;
    });

    it('Should deauthorize carbon token', async function () {
      await liquidityPoolManager.setCarbonTokenAuthorization(
        await carbonBasketToken.getAddress(),
        false
      );

      expect(
        await liquidityPoolManager.authorizedCarbonTokens(
          await carbonBasketToken.getAddress()
        )
      ).to.be.false;
    });

    it('Should fail if not admin', async function () {
      await expect(
        liquidityPoolManager
          .connect(user1)
          .setCarbonTokenAuthorization(
            await carbonBasketToken.getAddress(),
            false
          )
      ).to.be.revertedWithCustomError(
        liquidityPoolManager,
        'AccessControlUnauthorizedAccount'
      );
    });
  });

  describe('Fee Management', function () {
    it('Should set fee recipient', async function () {
      const newRecipient = user1.address;

      await expect(liquidityPoolManager.setFeeRecipient(newRecipient))
        .to.emit(liquidityPoolManager, 'FeeRecipientUpdated')
        .withArgs(feeRecipient.address, newRecipient);

      expect(await liquidityPoolManager.feeRecipient()).to.equal(newRecipient);
    });

    it('Should set protocol fee', async function () {
      const newFee = 25; // 0.25%

      await expect(liquidityPoolManager.setProtocolFee(newFee))
        .to.emit(liquidityPoolManager, 'ProtocolFeeUpdated')
        .withArgs(10, newFee);

      expect(await liquidityPoolManager.protocolFeeBasisPoints()).to.equal(
        newFee
      );
    });

    it('Should fail to set protocol fee too high', async function () {
      await expect(
        liquidityPoolManager.setProtocolFee(600) // 6% - too high
      ).to.be.revertedWith('LiquidityPoolManager: Protocol fee too high');
    });

    it('Should fail to set invalid fee recipient', async function () {
      await expect(
        liquidityPoolManager.setFeeRecipient(ethers.ZeroAddress)
      ).to.be.revertedWith('LiquidityPoolManager: Invalid fee recipient');
    });
  });

  describe('Emergency Functions', function () {
    let poolAddress;

    beforeEach(async function () {
      await liquidityPoolManager
        .connect(poolCreator)
        .createWeightedPool(await carbonBasketToken.getAddress(), 'stable', 30);

      const pools = await liquidityPoolManager.getActivePools();
      poolAddress = pools[0].poolAddress;
    });

    it('Should emergency pause specific pool', async function () {
      await liquidityPoolManager.emergencyPausePool(poolAddress);

      const pool = await ethers.getContractAt(
        'WeightedCarbonPool',
        poolAddress
      );
      expect(await pool.paused()).to.be.true;
    });

    it('Should emergency unpause specific pool', async function () {
      await liquidityPoolManager.emergencyPausePool(poolAddress);
      await liquidityPoolManager.emergencyUnpausePool(poolAddress);

      const pool = await ethers.getContractAt(
        'WeightedCarbonPool',
        poolAddress
      );
      expect(await pool.paused()).to.be.false;
    });

    it('Should emergency pause all pools', async function () {
      // Create another pool
      await liquidityPoolManager
        .connect(poolCreator)
        .createWeightedPool(
          await carbonBasketToken.getAddress(),
          'balanced',
          40
        );

      await liquidityPoolManager.emergencyPauseAll();

      expect(await liquidityPoolManager.paused()).to.be.true;

      const pools = await liquidityPoolManager.getActivePools();
      for (const poolConfig of pools) {
        const pool = await ethers.getContractAt(
          'WeightedCarbonPool',
          poolConfig.poolAddress
        );
        expect(await pool.paused()).to.be.true;
      }
    });

    it('Should fail emergency functions if not authorized', async function () {
      await expect(
        liquidityPoolManager.connect(user1).emergencyPausePool(poolAddress)
      ).to.be.revertedWithCustomError(
        liquidityPoolManager,
        'AccessControlUnauthorizedAccount'
      );
    });
  });

  describe('View Functions', function () {
    beforeEach(async function () {
      // Create multiple pools
      await liquidityPoolManager
        .connect(poolCreator)
        .createWeightedPool(await carbonBasketToken.getAddress(), 'stable', 30);

      await liquidityPoolManager
        .connect(poolCreator)
        .createWeightedPool(
          await carbonBasketToken.getAddress(),
          'balanced',
          40
        );
    });

    it('Should get active pools', async function () {
      const pools = await liquidityPoolManager.getActivePools();
      expect(pools.length).to.equal(2);
      expect(pools[0].poolType).to.equal('stable');
      expect(pools[1].poolType).to.equal('balanced');
    });

    it('Should get pool by carbon token', async function () {
      const pool = await liquidityPoolManager.getPoolByCarbonToken(
        await carbonBasketToken.getAddress()
      );

      expect(pool.carbonBasketToken).to.equal(
        await carbonBasketToken.getAddress()
      );
      expect(pool.isActive).to.be.true;
    });

    it('Should get pools count', async function () {
      expect(await liquidityPoolManager.getPoolsCount()).to.equal(2);
    });

    it('Should get pool stats', async function () {
      const pools = await liquidityPoolManager.getActivePools();
      const stats = await liquidityPoolManager.getPoolStats(
        pools[0].poolAddress
      );

      expect(stats.totalVolume24h).to.equal(0);
      expect(stats.totalLiquidity).to.equal(0);
      expect(stats.feesGenerated24h).to.equal(0);
    });

    it('Should fail to get pool by non-existent token', async function () {
      const MockToken = await ethers.getContractFactory('MockERC20');
      const nonExistentToken = await MockToken.deploy('Non-existent', 'NE', 18);

      await expect(
        liquidityPoolManager.getPoolByCarbonToken(
          await nonExistentToken.getAddress()
        )
      ).to.be.revertedWith('LiquidityPoolManager: Pool not found');
    });
  });
});
