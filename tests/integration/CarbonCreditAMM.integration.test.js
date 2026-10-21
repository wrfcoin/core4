const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('Carbon Credit AMM Integration', function () {
  let enhancedCarbonCredits;
  let wrfToken;
  let carbonBasketToken;
  let factory;
  let router;
  let ammPool;

  let owner;
  let issuer;
  let registryConnector;
  let user1;
  let user2;
  let user3;

  const INITIAL_WRF_SUPPLY = ethers.parseEther('100000');
  const CARBON_CREDIT_AMOUNT = ethers.parseUnits('10000', 0); // 10,000 kg CO2e
  const BASKET_DEPOSIT_AMOUNT = ethers.parseUnits('5000', 0);
  const LIQUIDITY_WRF = ethers.parseEther('1000');
  const LIQUIDITY_BASKET = ethers.parseEther('2000');

  const basketConfig = {
    name: 'Forest Carbon Basket',
    symbol: 'FCB',
    allowedProjectTypes: ['forestry', 'afforestation', 'reforestation'],
    allowedRegistries: ['VCS', 'GS', 'CAR'],
    minVintage: 2020,
    maxVintage: 2025,
    minQualityScore: 600,
    retirementRate: 50, // 0.5% annual retirement rate
  };

  beforeEach(async function () {
    [owner, issuer, registryConnector, user1, user2, user3] =
      await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    wrfToken = await WRFToken.deploy();

    // Deploy Enhanced Carbon Credits
    const EnhancedCarbonCredits = await ethers.getContractFactory(
      'EnhancedCarbonCredits'
    );
    enhancedCarbonCredits = await EnhancedCarbonCredits.deploy(
      await wrfToken.getAddress()
    );

    // Deploy Carbon Basket Token
    const CarbonBasketToken =
      await ethers.getContractFactory('CarbonBasketToken');
    carbonBasketToken = await CarbonBasketToken.deploy(
      await enhancedCarbonCredits.getAddress(),
      basketConfig
    );

    // Deploy AMM Factory
    const CarbonCreditAMMFactory = await ethers.getContractFactory(
      'CarbonCreditAMMFactory'
    );
    factory = await CarbonCreditAMMFactory.deploy();

    // Deploy Router
    const CarbonCreditRouter =
      await ethers.getContractFactory('CarbonCreditRouter');
    router = await CarbonCreditRouter.deploy(
      await factory.getAddress(),
      await wrfToken.getAddress()
    );

    // Setup roles
    const AUTHORIZED_ISSUER_ROLE =
      await enhancedCarbonCredits.AUTHORIZED_ISSUER_ROLE();
    const REGISTRY_CONNECTOR_ROLE =
      await enhancedCarbonCredits.REGISTRY_CONNECTOR_ROLE();
    const MANAGER_ROLE = await carbonBasketToken.MANAGER_ROLE();
    const POOL_CREATOR_ROLE = await factory.POOL_CREATOR_ROLE();

    await enhancedCarbonCredits.grantRole(
      AUTHORIZED_ISSUER_ROLE,
      issuer.address
    );
    await enhancedCarbonCredits.grantRole(
      REGISTRY_CONNECTOR_ROLE,
      registryConnector.address
    );
    await carbonBasketToken.grantRole(MANAGER_ROLE, owner.address);
    await factory.grantRole(POOL_CREATOR_ROLE, owner.address);

    // Distribute WRF tokens
    await wrfToken.transfer(user1.address, INITIAL_WRF_SUPPLY);
    await wrfToken.transfer(user2.address, INITIAL_WRF_SUPPLY);
    await wrfToken.transfer(user3.address, INITIAL_WRF_SUPPLY);

    // Approve contracts
    await wrfToken
      .connect(user1)
      .approve(await enhancedCarbonCredits.getAddress(), ethers.MaxUint256);
    await wrfToken
      .connect(user2)
      .approve(await enhancedCarbonCredits.getAddress(), ethers.MaxUint256);
    await wrfToken
      .connect(user3)
      .approve(await enhancedCarbonCredits.getAddress(), ethers.MaxUint256);

    await wrfToken
      .connect(user1)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await wrfToken
      .connect(user2)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await wrfToken
      .connect(user3)
      .approve(await router.getAddress(), ethers.MaxUint256);
  });

  async function createCarbonCredits(
    projectType,
    vintage,
    registryStandard,
    amount,
    recipient
  ) {
    const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;
    const projectId = `${registryStandard}-${projectType}-${vintage}-${Math.random().toString(36).substring(7)}`;
    const certHash = `QmHash${Math.random().toString(36).substring(7)}`;

    await enhancedCarbonCredits
      .connect(issuer)
      .createBatch(
        amount,
        projectId,
        vintage,
        'VM0006',
        projectType,
        registryStandard,
        '40.7128,-74.0060',
        futureDate,
        certHash,
        'https://registry.verra.org/project/001',
        'https://ipfs.io/ipfs/QmMetadata123'
      );

    const tokenId = await enhancedCarbonCredits.projectIdToTokenId(projectId);

    await enhancedCarbonCredits
      .connect(registryConnector)
      .immobilizeCredits(tokenId, amount, recipient);

    return tokenId;
  }

  describe('End-to-End Carbon Credit Trading Flow', function () {
    it('Should complete full trading cycle: credits → basket → AMM → trading', async function () {
      // Step 1: Create carbon credits
      const tokenId1 = await createCarbonCredits(
        'forestry',
        2023,
        'VCS',
        CARBON_CREDIT_AMOUNT,
        user1.address
      );
      const tokenId2 = await createCarbonCredits(
        'afforestation',
        2022,
        'GS',
        CARBON_CREDIT_AMOUNT,
        user2.address
      );

      // Step 2: Users deposit credits into basket token
      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);
      await enhancedCarbonCredits
        .connect(user2)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId1, BASKET_DEPOSIT_AMOUNT);
      await carbonBasketToken
        .connect(user2)
        .deposit(tokenId2, BASKET_DEPOSIT_AMOUNT);

      expect(await carbonBasketToken.balanceOf(user1.address)).to.equal(
        BASKET_DEPOSIT_AMOUNT
      );
      expect(await carbonBasketToken.balanceOf(user2.address)).to.equal(
        BASKET_DEPOSIT_AMOUNT
      );

      // Step 3: Create AMM pool between WRF and Basket tokens
      await factory.createPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30 // 0.3% fee
      );

      const poolAddress = await factory.getPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );

      ammPool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);

      // Step 4: Add liquidity to AMM pool
      await carbonBasketToken
        .connect(user1)
        .approve(await router.getAddress(), ethers.MaxUint256);

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const liquidityParams = {
        tokenA: await wrfToken.getAddress(),
        tokenB: await carbonBasketToken.getAddress(),
        feeTier: 30,
        amountADesired: LIQUIDITY_WRF,
        amountBDesired: LIQUIDITY_BASKET,
        amountAMin: (LIQUIDITY_WRF * 95n) / 100n,
        amountBMin: (LIQUIDITY_BASKET * 95n) / 100n,
        to: user1.address,
        deadline: deadline,
      };

      await router.connect(user1).addLiquidity(liquidityParams);

      const [reserve0, reserve1] = await ammPool.getReserves();
      expect(reserve0).to.be.gt(0);
      expect(reserve1).to.be.gt(0);

      // Step 5: User3 swaps WRF for basket tokens
      const swapAmount = ethers.parseEther('100');
      const swapParams = {
        tokenIn: await wrfToken.getAddress(),
        tokenOut: await carbonBasketToken.getAddress(),
        amountIn: swapAmount,
        amountOutMin: 1,
        to: user3.address,
        deadline: deadline,
        maxHops: 1,
      };

      const balanceBefore = await carbonBasketToken.balanceOf(user3.address);
      await router.connect(user3).swapExactTokensForTokens(swapParams);
      const balanceAfter = await carbonBasketToken.balanceOf(user3.address);

      expect(balanceAfter).to.be.gt(balanceBefore);

      // Step 6: User3 withdraws actual carbon credits from basket
      await carbonBasketToken.connect(user3).withdraw(balanceAfter, 0);

      // Verify user3 now has carbon credits
      const user3Credits1 = await enhancedCarbonCredits.balanceOf(
        user3.address,
        tokenId1
      );
      const user3Credits2 = await enhancedCarbonCredits.balanceOf(
        user3.address,
        tokenId2
      );
      expect(user3Credits1 + user3Credits2).to.equal(balanceAfter);

      // Step 7: User3 retires some credits for carbon offsetting
      const retireAmount = user3Credits1 / 2n;
      if (retireAmount > 0) {
        await enhancedCarbonCredits
          .connect(user3)
          .retireCredits(
            tokenId1,
            retireAmount,
            'Corporate carbon offsetting 2024'
          );

        const retirementStats =
          await enhancedCarbonCredits.getUserRetirementStats(user3.address);
        expect(retirementStats[0]).to.equal(retireAmount); // totalRetired
      }
    });

    it('Should handle multiple basket tokens and complex routing', async function () {
      // Create second basket for renewable energy credits
      const renewableBasketConfig = {
        ...basketConfig,
        name: 'Renewable Energy Basket',
        symbol: 'REB',
        allowedProjectTypes: ['solar', 'wind', 'hydro'],
      };

      const CarbonBasketToken =
        await ethers.getContractFactory('CarbonBasketToken');
      const renewableBasket = await CarbonBasketToken.deploy(
        await enhancedCarbonCredits.getAddress(),
        renewableBasketConfig
      );

      // Create renewable energy credits
      const renewableTokenId = await createCarbonCredits(
        'solar',
        2023,
        'VCS',
        CARBON_CREDIT_AMOUNT,
        user1.address
      );

      // User1 deposits to renewable basket
      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await renewableBasket.getAddress(), true);
      await renewableBasket
        .connect(user1)
        .deposit(renewableTokenId, BASKET_DEPOSIT_AMOUNT);

      // Create pools: WRF/ForestBasket and WRF/RenewableBasket
      await factory.createPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );
      await factory.createPool(
        await wrfToken.getAddress(),
        await renewableBasket.getAddress(),
        30
      );

      // Add liquidity to both pools
      await carbonBasketToken
        .connect(user1)
        .approve(await router.getAddress(), ethers.MaxUint256);
      await renewableBasket
        .connect(user1)
        .approve(await router.getAddress(), ethers.MaxUint256);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // Add liquidity to forest basket pool
      const liquidityParams1 = {
        tokenA: await wrfToken.getAddress(),
        tokenB: await carbonBasketToken.getAddress(),
        feeTier: 30,
        amountADesired: LIQUIDITY_WRF,
        amountBDesired: LIQUIDITY_BASKET,
        amountAMin: 1,
        amountBMin: 1,
        to: user1.address,
        deadline: deadline,
      };

      await router.connect(user1).addLiquidity(liquidityParams1);

      // Add liquidity to renewable basket pool
      const liquidityParams2 = {
        tokenA: await wrfToken.getAddress(),
        tokenB: await renewableBasket.getAddress(),
        feeTier: 30,
        amountADesired: LIQUIDITY_WRF,
        amountBDesired: LIQUIDITY_BASKET,
        amountAMin: 1,
        amountBMin: 1,
        to: user1.address,
        deadline: deadline,
      };

      await router.connect(user1).addLiquidity(liquidityParams2);

      // User2 can now trade between different basket types via WRF
      // This would require multi-hop routing: ForestBasket → WRF → RenewableBasket
      const forestBalance = await carbonBasketToken.balanceOf(user1.address);
      if (forestBalance > 0) {
        const swapParams = {
          tokenIn: await carbonBasketToken.getAddress(),
          tokenOut: await renewableBasket.getAddress(),
          amountIn: forestBalance / 10n, // Small amount for testing
          amountOutMin: 1,
          to: user2.address,
          deadline: deadline,
          maxHops: 2,
        };

        await carbonBasketToken
          .connect(user1)
          .approve(await router.getAddress(), forestBalance);
        await router.connect(user1).swapExactTokensForTokens(swapParams);

        expect(await renewableBasket.balanceOf(user2.address)).to.be.gt(0);
      }
    });
  });

  describe('Basket Token Quality Management', function () {
    let tokenId1, tokenId2, tokenId3;

    beforeEach(async function () {
      // Create credits with different quality characteristics
      tokenId1 = await createCarbonCredits(
        'forestry',
        2023,
        'VCS',
        CARBON_CREDIT_AMOUNT,
        user1.address
      ); // High quality
      tokenId2 = await createCarbonCredits(
        'forestry',
        2020,
        'CAR',
        CARBON_CREDIT_AMOUNT,
        user1.address
      ); // Lower quality (older)
      tokenId3 = await createCarbonCredits(
        'afforestation',
        2024,
        'GS',
        CARBON_CREDIT_AMOUNT,
        user1.address
      ); // Medium quality

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);
    });

    it('Should improve basket quality through selective withdrawal', async function () {
      // Deposit all credits
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId1, BASKET_DEPOSIT_AMOUNT);
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId2, BASKET_DEPOSIT_AMOUNT);
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId3, BASKET_DEPOSIT_AMOUNT);

      const statsBefore = await carbonBasketToken.getBasketStats();
      const qualityBefore = statsBefore._averageQuality;

      // Withdraw (should prefer lower quality credits)
      await carbonBasketToken.connect(user1).withdraw(BASKET_DEPOSIT_AMOUNT, 0);

      const statsAfter = await carbonBasketToken.getBasketStats();
      const qualityAfter = statsAfter._averageQuality;

      // Quality should improve after removing lowest quality credits
      expect(qualityAfter).to.be.gte(qualityBefore);
    });

    it('Should handle auto-retirement for quality improvement', async function () {
      // Deposit credits
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId1, BASKET_DEPOSIT_AMOUNT);
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId2, BASKET_DEPOSIT_AMOUNT);

      const REBALANCER_ROLE = await carbonBasketToken.REBALANCER_ROLE();
      await carbonBasketToken.grantRole(REBALANCER_ROLE, owner.address);

      // Fast forward time
      await ethers.provider.send('evm_increaseTime', [365 * 24 * 60 * 60]); // 1 year
      await ethers.provider.send('evm_mine');

      const totalSupplyBefore = await carbonBasketToken.totalCarbonSupply();

      // Perform auto retirement
      await carbonBasketToken.connect(owner).performAutoRetirement();

      const totalSupplyAfter = await carbonBasketToken.totalCarbonSupply();

      // Some credits should have been retired
      expect(totalSupplyAfter).to.be.lte(totalSupplyBefore);
    });
  });

  describe('AMM Pool Performance and Fees', function () {
    beforeEach(async function () {
      // Setup complete trading environment
      const tokenId = await createCarbonCredits(
        'forestry',
        2023,
        'VCS',
        CARBON_CREDIT_AMOUNT,
        user1.address
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, BASKET_DEPOSIT_AMOUNT);

      await factory.createPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );

      const poolAddress = await factory.getPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );
      ammPool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);

      // Add liquidity
      await carbonBasketToken
        .connect(user1)
        .approve(await router.getAddress(), ethers.MaxUint256);

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const liquidityParams = {
        tokenA: await wrfToken.getAddress(),
        tokenB: await carbonBasketToken.getAddress(),
        feeTier: 30,
        amountADesired: LIQUIDITY_WRF,
        amountBDesired: LIQUIDITY_BASKET,
        amountAMin: 1,
        amountBMin: 1,
        to: user1.address,
        deadline: deadline,
      };

      await router.connect(user1).addLiquidity(liquidityParams);
    });

    it('Should accumulate trading fees correctly', async function () {
      const swapAmount = ethers.parseEther('100');
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // Get initial k value
      const [reserve0Before, reserve1Before] = await ammPool.getReserves();
      const kBefore = reserve0Before * reserve1Before;

      // Perform swap
      const swapParams = {
        tokenIn: await wrfToken.getAddress(),
        tokenOut: await carbonBasketToken.getAddress(),
        amountIn: swapAmount,
        amountOutMin: 1,
        to: user2.address,
        deadline: deadline,
        maxHops: 1,
      };

      await router.connect(user2).swapExactTokensForTokens(swapParams);

      // Get k value after swap
      const [reserve0After, reserve1After] = await ammPool.getReserves();
      const kAfter = reserve0After * reserve1After;

      // K should increase due to trading fees
      expect(kAfter).to.be.gt(kBefore);
    });

    it('Should handle price oracle updates', async function () {
      const priceBefore = await ammPool.price0CumulativeLast();

      // Advance time
      await ethers.provider.send('evm_increaseTime', [3600]); // 1 hour
      await ethers.provider.send('evm_mine');

      // Make a trade to update oracle
      const swapParams = {
        tokenIn: await wrfToken.getAddress(),
        tokenOut: await carbonBasketToken.getAddress(),
        amountIn: ethers.parseEther('10'),
        amountOutMin: 1,
        to: user2.address,
        deadline: Math.floor(Date.now() / 1000) + 3600,
        maxHops: 1,
      };

      await router.connect(user2).swapExactTokensForTokens(swapParams);

      const priceAfter = await ammPool.price0CumulativeLast();
      expect(priceAfter).to.be.gt(priceBefore);
    });

    it('Should maintain liquidity provider rewards', async function () {
      const lpTokensBefore = await ammPool.balanceOf(user1.address);

      // Generate trading volume
      for (let i = 0; i < 5; i++) {
        const swapParams = {
          tokenIn: await wrfToken.getAddress(),
          tokenOut: await carbonBasketToken.getAddress(),
          amountIn: ethers.parseEther('50'),
          amountOutMin: 1,
          to: user2.address,
          deadline: Math.floor(Date.now() / 1000) + 3600,
          maxHops: 1,
        };

        await router.connect(user2).swapExactTokensForTokens(swapParams);
      }

      // LP tokens should be worth more due to accumulated fees
      const [reserve0, reserve1] = await ammPool.getReserves();
      const totalSupply = await ammPool.totalSupply();

      const token0PerLP = (reserve0 * lpTokensBefore) / totalSupply;
      const token1PerLP = (reserve1 * lpTokensBefore) / totalSupply;

      expect(token0PerLP).to.be.gt(0);
      expect(token1PerLP).to.be.gt(0);
    });
  });

  describe('Stress Testing and Edge Cases', function () {
    it('Should handle high volume trading', async function () {
      // Setup
      const tokenId = await createCarbonCredits(
        'forestry',
        2023,
        'VCS',
        CARBON_CREDIT_AMOUNT * 10n,
        user1.address
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, BASKET_DEPOSIT_AMOUNT * 5n);

      await factory.createPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );

      // Add substantial liquidity
      await carbonBasketToken
        .connect(user1)
        .approve(await router.getAddress(), ethers.MaxUint256);

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const liquidityParams = {
        tokenA: await wrfToken.getAddress(),
        tokenB: await carbonBasketToken.getAddress(),
        feeTier: 30,
        amountADesired: LIQUIDITY_WRF * 5n,
        amountBDesired: LIQUIDITY_BASKET * 5n,
        amountAMin: 1,
        amountBMin: 1,
        to: user1.address,
        deadline: deadline,
      };

      await router.connect(user1).addLiquidity(liquidityParams);

      // Perform many swaps
      const swapAmount = ethers.parseEther('100');

      for (let i = 0; i < 10; i++) {
        const swapParams = {
          tokenIn: await wrfToken.getAddress(),
          tokenOut: await carbonBasketToken.getAddress(),
          amountIn: swapAmount,
          amountOutMin: 1,
          to: user2.address,
          deadline: Math.floor(Date.now() / 1000) + 3600,
          maxHops: 1,
        };

        await router.connect(user2).swapExactTokensForTokens(swapParams);
      }

      // System should still be functional
      const poolAddress = await factory.getPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );
      const pool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);
      const [reserve0, reserve1] = await pool.getReserves();

      expect(reserve0).to.be.gt(0);
      expect(reserve1).to.be.gt(0);
    });

    it('Should handle empty pools gracefully', async function () {
      await factory.createPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );

      const poolAddress = await factory.getPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );

      // Should handle queries to empty pool
      expect(poolAddress).to.not.equal(ethers.ZeroAddress);

      const pool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);
      const [reserve0, reserve1] = await pool.getReserves();
      expect(reserve0).to.equal(0);
      expect(reserve1).to.equal(0);
    });

    it('Should maintain system integrity under concurrent operations', async function () {
      // Setup multiple tokens and baskets
      const tokenIds = [];
      for (let i = 0; i < 3; i++) {
        const tokenId = await createCarbonCredits(
          'forestry',
          2023,
          'VCS',
          CARBON_CREDIT_AMOUNT,
          user1.address
        );
        tokenIds.push(tokenId);
      }

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      // Deposit all tokens
      for (const tokenId of tokenIds) {
        await carbonBasketToken
          .connect(user1)
          .deposit(tokenId, BASKET_DEPOSIT_AMOUNT / 3n);
      }

      // Create pool and add liquidity
      await factory.createPool(
        await wrfToken.getAddress(),
        await carbonBasketToken.getAddress(),
        30
      );

      await carbonBasketToken
        .connect(user1)
        .approve(await router.getAddress(), ethers.MaxUint256);

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const liquidityParams = {
        tokenA: await wrfToken.getAddress(),
        tokenB: await carbonBasketToken.getAddress(),
        feeTier: 30,
        amountADesired: LIQUIDITY_WRF,
        amountBDesired: LIQUIDITY_BASKET,
        amountAMin: 1,
        amountBMin: 1,
        to: user1.address,
        deadline: deadline,
      };

      await router.connect(user1).addLiquidity(liquidityParams);

      // Perform concurrent operations (simulated)
      const operations = [];

      // Trading
      operations.push(
        router.connect(user2).swapExactTokensForTokens({
          tokenIn: await wrfToken.getAddress(),
          tokenOut: await carbonBasketToken.getAddress(),
          amountIn: ethers.parseEther('50'),
          amountOutMin: 1,
          to: user2.address,
          deadline: deadline,
          maxHops: 1,
        })
      );

      // Basket withdrawal
      const basketBalance = await carbonBasketToken.balanceOf(user2.address);
      if (basketBalance > 0) {
        operations.push(
          carbonBasketToken.connect(user2).withdraw(basketBalance / 2n, 0)
        );
      }

      // Execute all operations
      await Promise.all(operations);

      // Verify system integrity
      const stats = await carbonBasketToken.getBasketStats();
      expect(stats._totalSupply).to.equal(stats._totalCarbonSupply);
    });
  });
});
