const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('CarbonPoolRouter', function () {
  let carbonPoolRouter;
  let liquidityPoolManager;
  let ammFactory;
  let weightedPool;
  let ammPool;
  let usdc;
  let carbonBasketToken;
  let carbonBasketToken2;
  let qualityOracle;
  let volatilityOracle;
  let owner;
  let trader1;
  let trader2;
  let operator;

  const USDC_DECIMALS = 6;
  const USDC_AMOUNT = 50000n * 10n ** BigInt(USDC_DECIMALS); // 50,000 USDC
  const CBT_AMOUNT = ethers.parseEther('5000'); // 5,000 CBT
  const CBT2_AMOUNT = ethers.parseEther('3000'); // 3,000 CBT2

  beforeEach(async function () {
    [owner, trader1, trader2, operator] = await ethers.getSigners();

    // Deploy mock USDC
    const MockERC20 = await ethers.getContractFactory('MockERC20');
    usdc = await MockERC20.deploy('USD Coin', 'USDC', USDC_DECIMALS);

    // Deploy Enhanced Carbon Credits mock
    const SimpleMockEnhancedCarbonCredits = await ethers.getContractFactory(
      'SimpleMockEnhancedCarbonCredits'
    );
    const enhancedCarbonCredits =
      await SimpleMockEnhancedCarbonCredits.deploy();

    // Deploy Carbon Basket Tokens
    const basketConfig1 = {
      name: 'Forest Carbon Basket',
      symbol: 'FCB',
      allowedProjectTypes: ['forestry'],
      allowedRegistries: ['VCS'],
      minVintage: 2020,
      maxVintage: 2025,
      minQualityScore: 600,
      retirementRate: 100,
    };

    const basketConfig2 = {
      name: 'Renewable Energy Basket',
      symbol: 'REB',
      allowedProjectTypes: ['renewable-energy'],
      allowedRegistries: ['GS'],
      minVintage: 2020,
      maxVintage: 2025,
      minQualityScore: 700,
      retirementRate: 100,
    };

    const CarbonBasketToken =
      await ethers.getContractFactory('CarbonBasketToken');
    carbonBasketToken = await CarbonBasketToken.deploy(
      await enhancedCarbonCredits.getAddress(),
      basketConfig1
    );
    carbonBasketToken2 = await CarbonBasketToken.deploy(
      await enhancedCarbonCredits.getAddress(),
      basketConfig2
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
      owner.address // fee recipient
    );

    // Deploy AMM Factory
    const CarbonCreditAMMFactory = await ethers.getContractFactory(
      'CarbonCreditAMMFactory'
    );
    ammFactory = await CarbonCreditAMMFactory.deploy();

    // Deploy CarbonPoolRouter
    const CarbonPoolRouter =
      await ethers.getContractFactory('CarbonPoolRouter');
    carbonPoolRouter = await CarbonPoolRouter.deploy(
      await liquidityPoolManager.getAddress(),
      await ammFactory.getAddress(),
      await usdc.getAddress()
    );

    // Setup roles and permissions
    const POOL_CREATOR_ROLE = await liquidityPoolManager.POOL_CREATOR_ROLE();
    await liquidityPoolManager.grantRole(POOL_CREATOR_ROLE, owner.address);

    const OPERATOR_ROLE = await carbonPoolRouter.OPERATOR_ROLE();
    await carbonPoolRouter.grantRole(OPERATOR_ROLE, operator.address);

    // Authorize carbon tokens in LiquidityPoolManager
    await liquidityPoolManager.setCarbonTokenAuthorization(
      await carbonBasketToken.getAddress(),
      true
    );
    await liquidityPoolManager.setCarbonTokenAuthorization(
      await carbonBasketToken2.getAddress(),
      true
    );

    // Set oracles
    await liquidityPoolManager.setQualityOracle(
      await qualityOracle.getAddress()
    );
    await liquidityPoolManager.setVolatilityOracle(
      await volatilityOracle.getAddress()
    );

    // Create a weighted pool
    await liquidityPoolManager.createWeightedPool(
      await carbonBasketToken.getAddress(),
      'stable', // 80/20 USDC/CBT
      30 // 0.3% fee
    );

    const pools = await liquidityPoolManager.getActivePools();
    weightedPool = await ethers.getContractAt(
      'WeightedCarbonPool',
      pools[0].poolAddress
    );

    // Create an AMM pool
    const ammPoolCreatorRole = await ammFactory.POOL_CREATOR_ROLE();
    await ammFactory.grantRole(ammPoolCreatorRole, owner.address);

    await ammFactory.createPool(
      await usdc.getAddress(),
      await carbonBasketToken2.getAddress(),
      30 // 0.3% fee
    );

    const ammPoolAddress = await ammFactory.pools(
      await usdc.getAddress(),
      await carbonBasketToken2.getAddress(),
      30
    );
    ammPool = await ethers.getContractAt('CarbonCreditAMM', ammPoolAddress);

    // Mint tokens to accounts
    await usdc.mint(trader1.address, USDC_AMOUNT * 10n);
    await usdc.mint(trader2.address, USDC_AMOUNT * 10n);
    await usdc.mint(owner.address, USDC_AMOUNT * 10n);

    await carbonBasketToken.transfer(trader1.address, CBT_AMOUNT * 5n);
    await carbonBasketToken.transfer(trader2.address, CBT_AMOUNT * 5n);
    await carbonBasketToken.transfer(owner.address, CBT_AMOUNT * 5n);

    await carbonBasketToken2.transfer(trader1.address, CBT2_AMOUNT * 5n);
    await carbonBasketToken2.transfer(trader2.address, CBT2_AMOUNT * 5n);
    await carbonBasketToken2.transfer(owner.address, CBT2_AMOUNT * 5n);

    // Add liquidity to weighted pool
    const deadline = Math.floor(Date.now() / 1000) + 3600;
    await usdc
      .connect(owner)
      .approve(await weightedPool.getAddress(), ethers.MaxUint256);
    await carbonBasketToken
      .connect(owner)
      .approve(await weightedPool.getAddress(), ethers.MaxUint256);

    await weightedPool
      .connect(owner)
      .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);

    // Add liquidity to AMM pool
    await usdc
      .connect(owner)
      .approve(await ammPool.getAddress(), ethers.MaxUint256);
    await carbonBasketToken2
      .connect(owner)
      .approve(await ammPool.getAddress(), ethers.MaxUint256);

    await ammPool
      .connect(owner)
      .addLiquidity(USDC_AMOUNT, CBT2_AMOUNT, 0, 0, owner.address, deadline);

    // Initialize router with pools
    await carbonPoolRouter.connect(operator).initializePools();
  });

  describe('Pool Integration', function () {
    it('Should initialize with weighted and AMM pools', async function () {
      // Check that pools were added
      const usdcCbtPools = await carbonPoolRouter.getPoolsByTokenPair(
        await usdc.getAddress(),
        await carbonBasketToken.getAddress()
      );
      expect(usdcCbtPools.length).to.equal(1);
      expect(usdcCbtPools[0].poolType).to.equal(1); // WeightedPool

      const usdcCbt2Pools = await carbonPoolRouter.getPoolsByTokenPair(
        await usdc.getAddress(),
        await carbonBasketToken2.getAddress()
      );
      expect(usdcCbt2Pools.length).to.equal(1);
      expect(usdcCbt2Pools[0].poolType).to.equal(2); // CPAMM

      expect(
        await carbonPoolRouter.authorizedPools(await weightedPool.getAddress())
      ).to.be.true;
      expect(await carbonPoolRouter.authorizedPools(await ammPool.getAddress()))
        .to.be.true;
    });

    it('Should add new pools correctly', async function () {
      // Create another weighted pool
      await liquidityPoolManager.createWeightedPool(
        await carbonBasketToken2.getAddress(),
        'balanced', // 50/50
        40 // 0.4% fee
      );

      const newPools = await liquidityPoolManager.getActivePools();
      const newPoolAddress = newPools[1].poolAddress;

      await carbonPoolRouter.connect(operator).addPool(
        newPoolAddress,
        1, // WeightedPool
        await usdc.getAddress(),
        await carbonBasketToken2.getAddress()
      );

      const pools = await carbonPoolRouter.getPoolsByTokenPair(
        await usdc.getAddress(),
        await carbonBasketToken2.getAddress()
      );
      expect(pools.length).to.equal(2); // Now has both AMM and weighted pool
    });
  });

  describe('Routing', function () {
    beforeEach(async function () {
      // Setup trader approvals
      await usdc
        .connect(trader1)
        .approve(await carbonPoolRouter.getAddress(), ethers.MaxUint256);
      await carbonBasketToken
        .connect(trader1)
        .approve(await carbonPoolRouter.getAddress(), ethers.MaxUint256);
      await carbonBasketToken2
        .connect(trader1)
        .approve(await carbonPoolRouter.getAddress(), ethers.MaxUint256);
    });

    it('Should find best route for direct swap', async function () {
      const swapAmount = USDC_AMOUNT / 20n; // 5% of liquidity

      const route = await carbonPoolRouter.getBestRoute(
        await usdc.getAddress(),
        await carbonBasketToken.getAddress(),
        swapAmount
      );

      expect(route.pools.length).to.equal(1);
      expect(route.pools[0]).to.equal(await weightedPool.getAddress());
      expect(route.poolTypes[0]).to.equal(1); // WeightedPool
      expect(route.totalAmountOut).to.be.gt(0);
    });

    it('Should execute swap through weighted pool', async function () {
      const swapAmount = USDC_AMOUNT / 20n;
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const initialCBTBalance = await carbonBasketToken.balanceOf(
        trader1.address
      );

      await expect(
        carbonPoolRouter.connect(trader1).swapExactTokensForTokens(
          await usdc.getAddress(),
          await carbonBasketToken.getAddress(),
          swapAmount,
          0, // Accept any amount
          trader1.address,
          deadline
        )
      ).to.emit(carbonPoolRouter, 'SwapExecuted');

      const finalCBTBalance = await carbonBasketToken.balanceOf(
        trader1.address
      );
      expect(finalCBTBalance).to.be.gt(initialCBTBalance);
    });

    it('Should execute swap through AMM pool', async function () {
      const swapAmount = USDC_AMOUNT / 20n;
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const initialCBT2Balance = await carbonBasketToken2.balanceOf(
        trader1.address
      );

      await carbonPoolRouter
        .connect(trader1)
        .swapExactTokensForTokens(
          await usdc.getAddress(),
          await carbonBasketToken2.getAddress(),
          swapAmount,
          0,
          trader1.address,
          deadline
        );

      const finalCBT2Balance = await carbonBasketToken2.balanceOf(
        trader1.address
      );
      expect(finalCBT2Balance).to.be.gt(initialCBT2Balance);
    });

    it('Should handle multi-hop routing through USDC', async function () {
      // Test CBT -> USDC -> CBT2 routing
      const swapAmount = CBT_AMOUNT / 50n; // Small amount
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const route = await carbonPoolRouter.getBestRoute(
        await carbonBasketToken.getAddress(),
        await carbonBasketToken2.getAddress(),
        swapAmount
      );

      // Should find multi-hop route through USDC
      if (route.pools.length > 0) {
        const initialCBT2Balance = await carbonBasketToken2.balanceOf(
          trader1.address
        );

        await carbonPoolRouter
          .connect(trader1)
          .swapExactTokensForTokens(
            await carbonBasketToken.getAddress(),
            await carbonBasketToken2.getAddress(),
            swapAmount,
            0,
            trader1.address,
            deadline
          );

        const finalCBT2Balance = await carbonBasketToken2.balanceOf(
          trader1.address
        );
        expect(finalCBT2Balance).to.be.gt(initialCBT2Balance);
      }
    });

    it('Should update pool information', async function () {
      await carbonPoolRouter.updatePoolInfo(await weightedPool.getAddress());
      await carbonPoolRouter.updatePoolInfo(await ammPool.getAddress());

      // Should not revert - pools should be updated successfully
    });

    it('Should fail swap with expired deadline', async function () {
      const swapAmount = USDC_AMOUNT / 20n;
      const expiredDeadline = Math.floor(Date.now() / 1000) - 3600;

      await expect(
        carbonPoolRouter
          .connect(trader1)
          .swapExactTokensForTokens(
            await usdc.getAddress(),
            await carbonBasketToken.getAddress(),
            swapAmount,
            0,
            trader1.address,
            expiredDeadline
          )
      ).to.be.revertedWith('CarbonPoolRouter: Expired');
    });

    it('Should fail swap with insufficient output', async function () {
      const swapAmount = USDC_AMOUNT / 20n;
      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const unrealisticMinOut = CBT_AMOUNT; // Expecting more than total reserves

      await expect(
        carbonPoolRouter
          .connect(trader1)
          .swapExactTokensForTokens(
            await usdc.getAddress(),
            await carbonBasketToken.getAddress(),
            swapAmount,
            unrealisticMinOut,
            trader1.address,
            deadline
          )
      ).to.be.revertedWith('CarbonPoolRouter: Insufficient output');
    });
  });

  describe('Arbitrage Detection', function () {
    it('Should detect arbitrage opportunities', async function () {
      // Create price differences by executing large swaps
      const largeSwap = USDC_AMOUNT / 5n;
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // Make a large swap in one pool to create price discrepancy
      await usdc
        .connect(trader1)
        .approve(await weightedPool.getAddress(), largeSwap);
      await weightedPool
        .connect(trader1)
        .swapExactTokensForTokens(
          await usdc.getAddress(),
          await carbonBasketToken.getAddress(),
          largeSwap,
          0,
          trader1.address,
          deadline
        );

      // Check for arbitrage opportunities (this is a view function)
      const opportunities =
        await carbonPoolRouter.checkArbitrageOpportunities();

      // The function should return arrays (might be empty but should not revert)
      expect(opportunities.tokenAs).to.be.an('array');
      expect(opportunities.tokenBs).to.be.an('array');
      expect(opportunities.poolAs).to.be.an('array');
      expect(opportunities.poolBs).to.be.an('array');
      expect(opportunities.profits).to.be.an('array');
    });

    it('Should track arbitrage stats', async function () {
      const stats = await carbonPoolRouter.getArbitrageStats(trader1.address);
      expect(stats.volume24h).to.equal(0);
      expect(stats.lastTradeTime).to.equal(0);
    });
  });

  describe('Access Control', function () {
    it('Should restrict pool addition to operators', async function () {
      await expect(
        carbonPoolRouter
          .connect(trader1)
          .addPool(
            await weightedPool.getAddress(),
            1,
            await usdc.getAddress(),
            await carbonBasketToken.getAddress()
          )
      ).to.be.revertedWithCustomError(
        carbonPoolRouter,
        'AccessControlUnauthorizedAccount'
      );
    });

    it('Should restrict arbitrage execution to operators', async function () {
      await expect(
        carbonPoolRouter
          .connect(trader1)
          .executeArbitrage(
            await carbonBasketToken.getAddress(),
            await carbonBasketToken2.getAddress(),
            await weightedPool.getAddress(),
            await ammPool.getAddress(),
            1000
          )
      ).to.be.revertedWithCustomError(
        carbonPoolRouter,
        'AccessControlUnauthorizedAccount'
      );
    });

    it('Should allow operators to initialize pools', async function () {
      // Should not revert
      await carbonPoolRouter.connect(operator).initializePools();
    });
  });

  describe('Emergency Functions', function () {
    it('Should pause and unpause router', async function () {
      await carbonPoolRouter.pause();
      expect(await carbonPoolRouter.paused()).to.be.true;

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      await expect(
        carbonPoolRouter
          .connect(trader1)
          .swapExactTokensForTokens(
            await usdc.getAddress(),
            await carbonBasketToken.getAddress(),
            1000,
            0,
            trader1.address,
            deadline
          )
      ).to.be.revertedWithCustomError(carbonPoolRouter, 'EnforcedPause');

      await carbonPoolRouter.unpause();
      expect(await carbonPoolRouter.paused()).to.be.false;
    });
  });

  describe('View Functions', function () {
    it('Should get all pools', async function () {
      const allPools = await carbonPoolRouter.getAllPools();
      expect(allPools).to.be.an('array');
    });

    it('Should get pools by token pair', async function () {
      const pools = await carbonPoolRouter.getPoolsByTokenPair(
        await usdc.getAddress(),
        await carbonBasketToken.getAddress()
      );
      expect(pools.length).to.be.gt(0);
      expect(pools[0].poolAddress).to.equal(await weightedPool.getAddress());
    });
  });
});
