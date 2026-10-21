const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('WeightedCarbonPool', function () {
  let weightedCarbonPool;
  let usdc;
  let carbonBasketToken;
  let qualityOracle;
  let volatilityOracle;
  let manager;
  let user1;
  let user2;

  const BONE = ethers.parseEther('1');
  const USDC_DECIMALS = 6;
  const USDC_AMOUNT = 10000n * 10n ** BigInt(USDC_DECIMALS); // 10,000 USDC
  const CBT_AMOUNT = ethers.parseEther('1000'); // 1,000 CBT

  beforeEach(async function () {
    [manager, user1, user2] = await ethers.getSigners();

    // Deploy mock USDC (6 decimals)
    const MockERC20 = await ethers.getContractFactory('MockERC20');
    usdc = await MockERC20.deploy('USD Coin', 'USDC', USDC_DECIMALS);

    // Deploy CarbonBasketToken
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

    const CarbonBasketToken =
      await ethers.getContractFactory('CarbonBasketToken');
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

    // Deploy WeightedCarbonPool
    const WeightedCarbonPool =
      await ethers.getContractFactory('WeightedCarbonPool');
    weightedCarbonPool = await WeightedCarbonPool.deploy();

    // Initialize the pool (80/20 USDC/CBT)
    await weightedCarbonPool.initialize(
      await usdc.getAddress(),
      await carbonBasketToken.getAddress(),
      ethers.parseEther('0.8'), // 80% USDC weight
      ethers.parseEther('0.2'), // 20% CBT weight
      ethers.parseEther('0.003'), // 0.3% swap fee
      await volatilityOracle.getAddress(),
      await qualityOracle.getAddress(),
      manager.address
    );

    // Mint tokens to users
    await usdc.mint(user1.address, USDC_AMOUNT * 10n);
    await usdc.mint(user2.address, USDC_AMOUNT * 10n);
    await carbonBasketToken.transfer(user1.address, CBT_AMOUNT * 10n);
    await carbonBasketToken.transfer(user2.address, CBT_AMOUNT * 10n);

    // Approve pool
    await usdc
      .connect(user1)
      .approve(await weightedCarbonPool.getAddress(), ethers.MaxUint256);
    await usdc
      .connect(user2)
      .approve(await weightedCarbonPool.getAddress(), ethers.MaxUint256);
    await carbonBasketToken
      .connect(user1)
      .approve(await weightedCarbonPool.getAddress(), ethers.MaxUint256);
    await carbonBasketToken
      .connect(user2)
      .approve(await weightedCarbonPool.getAddress(), ethers.MaxUint256);
  });

  describe('Initialization', function () {
    it('Should initialize with correct parameters', async function () {
      expect(await weightedCarbonPool.usdc()).to.equal(await usdc.getAddress());
      expect(await weightedCarbonPool.carbonBasketToken()).to.equal(
        await carbonBasketToken.getAddress()
      );
      expect(await weightedCarbonPool.manager()).to.equal(manager.address);

      const weights = await weightedCarbonPool.getWeights();
      expect(weights._weightUSDC).to.equal(ethers.parseEther('0.8'));
      expect(weights._weightCBT).to.equal(ethers.parseEther('0.2'));

      expect(await weightedCarbonPool.swapFee()).to.equal(
        ethers.parseEther('0.003')
      );
    });

    it('Should have correct token properties', async function () {
      expect(await weightedCarbonPool.name()).to.equal('Weighted Carbon LP');
      expect(await weightedCarbonPool.symbol()).to.equal('WCLP');
      expect(await weightedCarbonPool.decimals()).to.equal(18);
    });
  });

  describe('Liquidity Provision', function () {
    it('Should add initial liquidity correctly', async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const tx = await weightedCarbonPool.connect(user1).addLiquidity(
        USDC_AMOUNT,
        CBT_AMOUNT,
        0, // minLPTokens
        deadline
      );

      await expect(tx)
        .to.emit(weightedCarbonPool, 'LiquidityAdded')
        .withArgs(user1.address, USDC_AMOUNT, CBT_AMOUNT, ethers.anyValue);

      const lpBalance = await weightedCarbonPool.balanceOf(user1.address);
      expect(lpBalance).to.be.gt(0);

      const reserves = await weightedCarbonPool.getReserves();
      expect(reserves.usdcReserve).to.equal(USDC_AMOUNT);
      expect(reserves.cbtReserve).to.equal(CBT_AMOUNT);
    });

    it('Should add subsequent liquidity proportionally', async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // First liquidity provision
      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);

      const initialLPBalance = await weightedCarbonPool.balanceOf(
        user1.address
      );

      // Second liquidity provision (same ratio)
      await weightedCarbonPool
        .connect(user2)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);

      const secondLPBalance = await weightedCarbonPool.balanceOf(user2.address);

      // LP tokens should be approximately equal for same amounts
      expect(secondLPBalance).to.be.closeTo(
        initialLPBalance,
        ethers.parseEther('0.1')
      );
    });

    it('Should handle single-sided liquidity provision', async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // First provide balanced liquidity
      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);

      // Then provide single-sided USDC
      const singleSidedAmount = USDC_AMOUNT / 2n;

      const tx = await weightedCarbonPool.connect(user2).addLiquidity(
        singleSidedAmount,
        0, // No CBT
        0,
        deadline
      );

      await expect(tx).to.emit(weightedCarbonPool, 'LiquidityAdded');

      const lpBalance = await weightedCarbonPool.balanceOf(user2.address);
      expect(lpBalance).to.be.gt(0);
    });

    it('Should fail with expired deadline', async function () {
      const expiredDeadline = Math.floor(Date.now() / 1000) - 3600;

      await expect(
        weightedCarbonPool
          .connect(user1)
          .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, expiredDeadline)
      ).to.be.revertedWith('WeightedCarbonPool: Expired');
    });

    it('Should fail with insufficient LP tokens', async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const unrealisticMinLP = ethers.parseEther('1000000');

      await expect(
        weightedCarbonPool
          .connect(user1)
          .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, unrealisticMinLP, deadline)
      ).to.be.revertedWith('WeightedCarbonPool: Insufficient LP tokens');
    });
  });

  describe('Liquidity Removal', function () {
    beforeEach(async function () {
      // Setup initial liquidity
      const deadline = Math.floor(Date.now() / 1000) + 3600;
      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);
    });

    it('Should remove liquidity correctly', async function () {
      const lpBalance = await weightedCarbonPool.balanceOf(user1.address);
      const removeAmount = lpBalance / 2n; // Remove 50%
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const initialUSDCBalance = await usdc.balanceOf(user1.address);
      const initialCBTBalance = await carbonBasketToken.balanceOf(
        user1.address
      );

      const tx = await weightedCarbonPool.connect(user1).removeLiquidity(
        removeAmount,
        0, // minUSDC
        0, // minCBT
        deadline
      );

      await expect(tx).to.emit(weightedCarbonPool, 'LiquidityRemoved');

      const finalUSDCBalance = await usdc.balanceOf(user1.address);
      const finalCBTBalance = await carbonBasketToken.balanceOf(user1.address);

      expect(finalUSDCBalance).to.be.gt(initialUSDCBalance);
      expect(finalCBTBalance).to.be.gt(initialCBTBalance);
      expect(await weightedCarbonPool.balanceOf(user1.address)).to.equal(
        lpBalance - removeAmount
      );
    });

    it('Should fail with insufficient balance', async function () {
      const lpBalance = await weightedCarbonPool.balanceOf(user1.address);
      const excessiveAmount = lpBalance + ethers.parseEther('1');
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      await expect(
        weightedCarbonPool
          .connect(user1)
          .removeLiquidity(excessiveAmount, 0, 0, deadline)
      ).to.be.revertedWith('WeightedCarbonPool: Insufficient balance');
    });

    it('Should fail with minimum amount requirements', async function () {
      const lpBalance = await weightedCarbonPool.balanceOf(user1.address);
      const removeAmount = lpBalance / 2n;
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      await expect(
        weightedCarbonPool.connect(user1).removeLiquidity(
          removeAmount,
          USDC_AMOUNT, // Unrealistic minimum
          CBT_AMOUNT, // Unrealistic minimum
          deadline
        )
      ).to.be.revertedWith('WeightedCarbonPool: Insufficient USDC');
    });
  });

  describe('Swapping', function () {
    beforeEach(async function () {
      // Setup initial liquidity
      const deadline = Math.floor(Date.now() / 1000) + 3600;
      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);
    });

    it('Should swap USDC for CBT', async function () {
      const swapAmount = USDC_AMOUNT / 10n; // 10% of liquidity
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const initialCBTBalance = await carbonBasketToken.balanceOf(
        user2.address
      );

      const amountOut = await weightedCarbonPool.getAmountOut(
        await usdc.getAddress(),
        swapAmount
      );

      const tx = await weightedCarbonPool
        .connect(user2)
        .swapExactTokensForTokens(
          await usdc.getAddress(),
          await carbonBasketToken.getAddress(),
          swapAmount,
          0, // minAmountOut
          user2.address,
          deadline
        );

      await expect(tx)
        .to.emit(weightedCarbonPool, 'Swap')
        .withArgs(
          user2.address,
          await usdc.getAddress(),
          await carbonBasketToken.getAddress(),
          swapAmount,
          ethers.anyValue,
          user2.address
        );

      const finalCBTBalance = await carbonBasketToken.balanceOf(user2.address);
      expect(finalCBTBalance - initialCBTBalance).to.be.closeTo(
        amountOut,
        ethers.parseEther('0.1')
      );
    });

    it('Should swap CBT for USDC', async function () {
      const swapAmount = CBT_AMOUNT / 10n; // 10% of liquidity
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const initialUSDCBalance = await usdc.balanceOf(user2.address);

      const amountOut = await weightedCarbonPool.getAmountOut(
        await carbonBasketToken.getAddress(),
        swapAmount
      );

      await weightedCarbonPool
        .connect(user2)
        .swapExactTokensForTokens(
          await carbonBasketToken.getAddress(),
          await usdc.getAddress(),
          swapAmount,
          0,
          user2.address,
          deadline
        );

      const finalUSDCBalance = await usdc.balanceOf(user2.address);
      expect(finalUSDCBalance - initialUSDCBalance).to.be.closeTo(
        amountOut,
        100n
      ); // Allow for rounding in USDC
    });

    it('Should fail with insufficient output', async function () {
      const swapAmount = USDC_AMOUNT / 10n;
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const amountOut = await weightedCarbonPool.getAmountOut(
        await usdc.getAddress(),
        swapAmount
      );

      await expect(
        weightedCarbonPool.connect(user2).swapExactTokensForTokens(
          await usdc.getAddress(),
          await carbonBasketToken.getAddress(),
          swapAmount,
          amountOut * 2n, // Unrealistic expectation
          user2.address,
          deadline
        )
      ).to.be.revertedWith('WeightedCarbonPool: Insufficient output');
    });

    it('Should fail with invalid token pair', async function () {
      const MockToken = await ethers.getContractFactory('MockERC20');
      const invalidToken = await MockToken.deploy('Invalid', 'INV', 18);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      await expect(
        weightedCarbonPool
          .connect(user2)
          .swapExactTokensForTokens(
            await invalidToken.getAddress(),
            await usdc.getAddress(),
            1000,
            0,
            user2.address,
            deadline
          )
      ).to.be.revertedWith('WeightedCarbonPool: Invalid token pair');
    });

    it('Should update price tracking', async function () {
      const swapAmount = USDC_AMOUNT / 20n; // Small swap
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const initialPrice = await weightedCarbonPool.getSpotPrice();

      await weightedCarbonPool
        .connect(user2)
        .swapExactTokensForTokens(
          await usdc.getAddress(),
          await carbonBasketToken.getAddress(),
          swapAmount,
          0,
          user2.address,
          deadline
        );

      const finalPrice = await weightedCarbonPool.getSpotPrice();

      // Price should change after swap
      expect(finalPrice).to.not.equal(initialPrice);
    });
  });

  describe('Fee Management', function () {
    it('Should update swap fee (manager only)', async function () {
      const newFee = ethers.parseEther('0.005'); // 0.5%

      await expect(weightedCarbonPool.connect(manager).setSwapFee(newFee))
        .to.emit(weightedCarbonPool, 'SwapFeeUpdated')
        .withArgs(newFee);

      expect(await weightedCarbonPool.swapFee()).to.equal(newFee);
    });

    it('Should fail to update fee if not manager', async function () {
      const newFee = ethers.parseEther('0.005');

      await expect(
        weightedCarbonPool.connect(user1).setSwapFee(newFee)
      ).to.be.revertedWith('WeightedCarbonPool: Only manager');
    });

    it('Should fail with fee too high', async function () {
      const excessiveFee = ethers.parseEther('0.15'); // 15%

      await expect(
        weightedCarbonPool.connect(manager).setSwapFee(excessiveFee)
      ).to.be.revertedWith('WeightedCarbonPool: Fee too high');
    });
  });

  describe('Weight Management', function () {
    it('Should update weights (manager only)', async function () {
      const newWeightUSDC = ethers.parseEther('0.6'); // 60%
      const newWeightCBT = ethers.parseEther('0.4'); // 40%

      await expect(
        weightedCarbonPool
          .connect(manager)
          .updateWeights(newWeightUSDC, newWeightCBT)
      )
        .to.emit(weightedCarbonPool, 'WeightsUpdated')
        .withArgs(newWeightUSDC, newWeightCBT);

      const weights = await weightedCarbonPool.getWeights();
      expect(weights._weightUSDC).to.equal(newWeightUSDC);
      expect(weights._weightCBT).to.equal(newWeightCBT);
    });

    it('Should fail with invalid weights', async function () {
      await expect(
        weightedCarbonPool.connect(manager).updateWeights(
          ethers.parseEther('0.6'),
          ethers.parseEther('0.5') // Doesn't sum to 1
        )
      ).to.be.revertedWith('WeightedCarbonPool: Invalid weights');
    });

    it('Should fail if not manager', async function () {
      await expect(
        weightedCarbonPool
          .connect(user1)
          .updateWeights(ethers.parseEther('0.6'), ethers.parseEther('0.4'))
      ).to.be.revertedWith('WeightedCarbonPool: Only manager');
    });
  });

  describe('Impermanent Loss Protection', function () {
    it('Should track deposits for ILP', async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);

      const deposits = await weightedCarbonPool.getUserDeposits(user1.address);
      expect(deposits.length).to.equal(1);
      expect(deposits[0].usdcAmount).to.equal(USDC_AMOUNT);
      expect(deposits[0].cbtAmount).to.equal(CBT_AMOUNT);
      expect(deposits[0].lpTokens).to.be.gt(0);
    });

    it('Should track multiple deposits', async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // First deposit
      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);

      // Second deposit
      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT / 2n, CBT_AMOUNT / 2n, 0, deadline);

      const deposits = await weightedCarbonPool.getUserDeposits(user1.address);
      expect(deposits.length).to.equal(2);
    });
  });

  describe('Emergency Functions', function () {
    it('Should pause and unpause (manager only)', async function () {
      await weightedCarbonPool.connect(manager).pause();
      expect(await weightedCarbonPool.paused()).to.be.true;

      // Should fail when paused
      const deadline = Math.floor(Date.now() / 1000) + 3600;
      await expect(
        weightedCarbonPool
          .connect(user1)
          .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline)
      ).to.be.revertedWithCustomError(weightedCarbonPool, 'EnforcedPause');

      await weightedCarbonPool.connect(manager).unpause();
      expect(await weightedCarbonPool.paused()).to.be.false;
    });

    it('Should fail pause/unpause if not manager', async function () {
      await expect(
        weightedCarbonPool.connect(user1).pause()
      ).to.be.revertedWith('WeightedCarbonPool: Only manager');
    });
  });

  describe('View Functions', function () {
    beforeEach(async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;
      await weightedCarbonPool
        .connect(user1)
        .addLiquidity(USDC_AMOUNT, CBT_AMOUNT, 0, deadline);
    });

    it('Should get reserves', async function () {
      const reserves = await weightedCarbonPool.getReserves();
      expect(reserves.usdcReserve).to.equal(USDC_AMOUNT);
      expect(reserves.cbtReserve).to.equal(CBT_AMOUNT);
    });

    it('Should get spot price', async function () {
      const spotPrice = await weightedCarbonPool.getSpotPrice();
      expect(spotPrice).to.be.gt(0);
    });

    it('Should quote swap amounts', async function () {
      const swapAmount = USDC_AMOUNT / 10n;
      const amountOut = await weightedCarbonPool.getAmountOut(
        await usdc.getAddress(),
        swapAmount
      );
      expect(amountOut).to.be.gt(0);
    });

    it('Should handle zero amount quotes', async function () {
      const amountOut = await weightedCarbonPool.getAmountOut(
        await usdc.getAddress(),
        0
      );
      expect(amountOut).to.equal(0);
    });
  });
});
