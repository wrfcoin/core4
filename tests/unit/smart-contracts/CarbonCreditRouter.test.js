const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('CarbonCreditRouter', function () {
  let router;
  let factory;
  let tokenA;
  let tokenB;
  let tokenC;
  let wrfToken;
  let owner;
  let user1;
  let user2;

  const INITIAL_SUPPLY = ethers.parseEther('100000');
  const LIQUIDITY_AMOUNT_A = ethers.parseEther('1000');
  const LIQUIDITY_AMOUNT_B = ethers.parseEther('2000');
  const FEE_TIER = 30; // 0.3%

  beforeEach(async function () {
    [owner, user1, user2] = await ethers.getSigners();

    // Deploy test tokens
    const ERC20Mock = await ethers.getContractFactory('WRFToken');
    wrfToken = await ERC20Mock.deploy();
    tokenA = await ERC20Mock.deploy();
    tokenB = await ERC20Mock.deploy();
    tokenC = await ERC20Mock.deploy();

    // Ensure consistent token ordering
    const tokens = [wrfToken, tokenA, tokenB, tokenC];
    tokens.sort((a, b) => (a.target < b.target ? -1 : 1));
    [tokenA, tokenB, tokenC, wrfToken] = tokens;

    // Deploy factory
    const CarbonCreditAMMFactory = await ethers.getContractFactory(
      'CarbonCreditAMMFactory'
    );
    factory = await CarbonCreditAMMFactory.deploy();

    // Deploy router
    const CarbonCreditRouter =
      await ethers.getContractFactory('CarbonCreditRouter');
    router = await CarbonCreditRouter.deploy(
      await factory.getAddress(),
      await wrfToken.getAddress() // Using WRF as WETH for testing
    );

    // Create pools
    const POOL_CREATOR_ROLE = await factory.POOL_CREATOR_ROLE();
    await factory.grantRole(POOL_CREATOR_ROLE, owner.address);

    await factory.createPool(
      await tokenA.getAddress(),
      await tokenB.getAddress(),
      FEE_TIER
    );
    await factory.createPool(
      await tokenB.getAddress(),
      await tokenC.getAddress(),
      FEE_TIER
    );

    // Transfer tokens to users
    await tokenA.transfer(user1.address, INITIAL_SUPPLY);
    await tokenB.transfer(user1.address, INITIAL_SUPPLY);
    await tokenC.transfer(user1.address, INITIAL_SUPPLY);
    await wrfToken.transfer(user1.address, INITIAL_SUPPLY);

    await tokenA.transfer(user2.address, INITIAL_SUPPLY);
    await tokenB.transfer(user2.address, INITIAL_SUPPLY);
    await tokenC.transfer(user2.address, INITIAL_SUPPLY);
    await wrfToken.transfer(user2.address, INITIAL_SUPPLY);

    // Approve router
    await tokenA
      .connect(user1)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await tokenB
      .connect(user1)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await tokenC
      .connect(user1)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await wrfToken
      .connect(user1)
      .approve(await router.getAddress(), ethers.MaxUint256);

    await tokenA
      .connect(user2)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await tokenB
      .connect(user2)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await tokenC
      .connect(user2)
      .approve(await router.getAddress(), ethers.MaxUint256);
    await wrfToken
      .connect(user2)
      .approve(await router.getAddress(), ethers.MaxUint256);
  });

  async function addLiquidity(tokenAAddr, tokenBAddr, amountA, amountB) {
    const deadline = Math.floor(Date.now() / 1000) + 3600; // 1 hour from now

    const liquidityParams = {
      tokenA: tokenAAddr,
      tokenB: tokenBAddr,
      feeTier: FEE_TIER,
      amountADesired: amountA,
      amountBDesired: amountB,
      amountAMin: (amountA * 95n) / 100n, // 5% slippage tolerance
      amountBMin: (amountB * 95n) / 100n,
      to: user1.address,
      deadline: deadline,
    };

    return await router.connect(user1).addLiquidity(liquidityParams);
  }

  describe('Deployment', function () {
    it('Should deploy with correct parameters', async function () {
      expect(await router.factory()).to.equal(await factory.getAddress());
      expect(await router.WETH()).to.equal(await wrfToken.getAddress());
    });

    it('Should setup roles correctly', async function () {
      const DEFAULT_ADMIN_ROLE = await router.DEFAULT_ADMIN_ROLE();
      const ROUTER_ADMIN_ROLE = await router.ROUTER_ADMIN_ROLE();

      expect(await router.hasRole(DEFAULT_ADMIN_ROLE, owner.address)).to.be
        .true;
      expect(await router.hasRole(ROUTER_ADMIN_ROLE, owner.address)).to.be.true;
    });

    it('Should have correct initial configuration', async function () {
      expect(await router.maxSlippageBasisPoints()).to.equal(300); // 3%
      expect(await router.routingFeeBasisPoints()).to.equal(5); // 0.05%
    });
  });

  describe('Liquidity Management', function () {
    describe('Add Liquidity', function () {
      it('Should add liquidity to existing pool', async function () {
        const tx = await addLiquidity(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          LIQUIDITY_AMOUNT_A,
          LIQUIDITY_AMOUNT_B
        );

        await expect(tx)
          .to.emit(router, 'LiquidityAdded')
          .withArgs(
            await tokenA.getAddress(),
            await tokenB.getAddress(),
            LIQUIDITY_AMOUNT_A,
            LIQUIDITY_AMOUNT_B,
            await router.getAddress(), // This should be the actual liquidity amount
            user1.address
          );
      });

      it('Should handle reversed token order', async function () {
        // Should work regardless of token order passed to function
        await addLiquidity(
          await tokenB.getAddress(), // Reversed order
          await tokenA.getAddress(),
          LIQUIDITY_AMOUNT_B,
          LIQUIDITY_AMOUNT_A
        );

        const poolAddress = await factory.getPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          FEE_TIER
        );

        const pool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);
        const [reserve0, reserve1] = await pool.getReserves();

        expect(reserve0).to.be.gt(0);
        expect(reserve1).to.be.gt(0);
      });

      it('Should prevent adding liquidity with expired deadline', async function () {
        const pastDeadline = Math.floor(Date.now() / 1000) - 3600; // 1 hour ago

        const liquidityParams = {
          tokenA: await tokenA.getAddress(),
          tokenB: await tokenB.getAddress(),
          feeTier: FEE_TIER,
          amountADesired: LIQUIDITY_AMOUNT_A,
          amountBDesired: LIQUIDITY_AMOUNT_B,
          amountAMin: (LIQUIDITY_AMOUNT_A * 95n) / 100n,
          amountBMin: (LIQUIDITY_AMOUNT_B * 95n) / 100n,
          to: user1.address,
          deadline: pastDeadline,
        };

        await expect(
          router.connect(user1).addLiquidity(liquidityParams)
        ).to.be.revertedWith('CarbonCreditRouter: EXPIRED');
      });

      it("Should create pool if it doesn't exist", async function () {
        // Try to add liquidity to non-existent pair
        const TokenD = await ethers.getContractFactory('WRFToken');
        const tokenD = await TokenD.deploy();

        await tokenD.transfer(user1.address, INITIAL_SUPPLY);
        await tokenD
          .connect(user1)
          .approve(await router.getAddress(), ethers.MaxUint256);

        // This should create the pool and add liquidity
        await addLiquidity(
          await tokenA.getAddress(),
          await tokenD.getAddress(),
          LIQUIDITY_AMOUNT_A,
          LIQUIDITY_AMOUNT_A
        );

        const poolAddress = await factory.getPool(
          await tokenA.getAddress(),
          await tokenD.getAddress(),
          FEE_TIER
        );

        expect(poolAddress).to.not.equal(ethers.ZeroAddress);
      });
    });

    describe('Remove Liquidity', function () {
      beforeEach(async function () {
        await addLiquidity(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          LIQUIDITY_AMOUNT_A,
          LIQUIDITY_AMOUNT_B
        );
      });

      it('Should remove liquidity successfully', async function () {
        const poolAddress = await factory.getPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          FEE_TIER
        );

        const pool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);
        const liquidity = await pool.balanceOf(user1.address);

        await pool.connect(user1).approve(await router.getAddress(), liquidity);

        const deadline = Math.floor(Date.now() / 1000) + 3600;

        const tx = await router.connect(user1).removeLiquidity(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          FEE_TIER,
          liquidity,
          1, // Minimum amounts (very low for testing)
          1,
          user1.address,
          deadline
        );

        await expect(tx).to.emit(router, 'LiquidityRemoved');
      });

      it('Should prevent removing liquidity with insufficient amounts', async function () {
        const poolAddress = await factory.getPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          FEE_TIER
        );

        const pool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);
        const liquidity = await pool.balanceOf(user1.address);

        await pool.connect(user1).approve(await router.getAddress(), liquidity);

        const deadline = Math.floor(Date.now() / 1000) + 3600;

        await expect(
          router.connect(user1).removeLiquidity(
            await tokenA.getAddress(),
            await tokenB.getAddress(),
            FEE_TIER,
            liquidity,
            ethers.parseEther('2000'), // Too high minimum
            ethers.parseEther('4000'), // Too high minimum
            user1.address,
            deadline
          )
        ).to.be.revertedWith('CarbonCreditRouter: INSUFFICIENT_A_AMOUNT');
      });
    });
  });

  describe('Token Swapping', function () {
    const swapAmount = ethers.parseEther('100');

    beforeEach(async function () {
      // Add liquidity to both pools
      await addLiquidity(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        LIQUIDITY_AMOUNT_A,
        LIQUIDITY_AMOUNT_B
      );

      await addLiquidity(
        await tokenB.getAddress(),
        await tokenC.getAddress(),
        LIQUIDITY_AMOUNT_B,
        LIQUIDITY_AMOUNT_A
      );
    });

    describe('Exact Input Swaps', function () {
      it('Should perform direct swap successfully', async function () {
        const deadline = Math.floor(Date.now() / 1000) + 3600;

        const swapParams = {
          tokenIn: await tokenA.getAddress(),
          tokenOut: await tokenB.getAddress(),
          amountIn: swapAmount,
          amountOutMin: 1, // Very low for testing
          to: user2.address,
          deadline: deadline,
          maxHops: 3,
        };

        const balanceBefore = await tokenB.balanceOf(user2.address);

        const tx = await router
          .connect(user1)
          .swapExactTokensForTokens(swapParams);

        await expect(tx).to.emit(router, 'SwapExecuted');

        const balanceAfter = await tokenB.balanceOf(user2.address);
        expect(balanceAfter).to.be.gt(balanceBefore);
      });

      it('Should find optimal route', async function () {
        const route = await router.getOptimalRoute(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          swapAmount,
          3
        );

        expect(route.pools.length).to.be.gt(0);
        expect(route.tokens.length).to.equal(route.pools.length + 1);
        expect(route.expectedOutput).to.be.gt(0);
      });

      it('Should charge routing fee', async function () {
        const deadline = Math.floor(Date.now() / 1000) + 3600;
        const routingFee = (swapAmount * 5n) / 10000n; // 0.05%

        const swapParams = {
          tokenIn: await tokenA.getAddress(),
          tokenOut: await tokenB.getAddress(),
          amountIn: swapAmount,
          amountOutMin: 1,
          to: user2.address,
          deadline: deadline,
          maxHops: 3,
        };

        const balanceBefore = await tokenA.balanceOf(user1.address);

        await router.connect(user1).swapExactTokensForTokens(swapParams);

        const balanceAfter = await tokenA.balanceOf(user1.address);
        expect(balanceBefore - balanceAfter).to.equal(swapAmount);

        // Check that routing fee was collected
        const routerBalance = await tokenA.balanceOf(await router.getAddress());
        expect(routerBalance).to.equal(routingFee);
      });

      it('Should prevent swaps with insufficient output', async function () {
        const deadline = Math.floor(Date.now() / 1000) + 3600;

        const swapParams = {
          tokenIn: await tokenA.getAddress(),
          tokenOut: await tokenB.getAddress(),
          amountIn: swapAmount,
          amountOutMin: ethers.parseEther('10000'), // Unreasonably high
          to: user2.address,
          deadline: deadline,
          maxHops: 3,
        };

        await expect(
          router.connect(user1).swapExactTokensForTokens(swapParams)
        ).to.be.revertedWith('CarbonCreditRouter: INSUFFICIENT_OUTPUT_AMOUNT');
      });
    });

    describe('Exact Output Swaps', function () {
      it('Should perform exact output swap', async function () {
        const deadline = Math.floor(Date.now() / 1000) + 3600;
        const desiredOutput = ethers.parseEther('50');

        const tx = await router.connect(user1).swapTokensForExactTokens(
          desiredOutput,
          ethers.parseEther('200'), // Max input
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          user2.address,
          deadline,
          3
        );

        await expect(tx).to.emit(router, 'SwapExecuted');

        const balance = await tokenB.balanceOf(user2.address);
        expect(balance).to.be.gte(INITIAL_SUPPLY + desiredOutput - 1n); // Allow for rounding
      });

      it('Should prevent swaps with excessive input', async function () {
        const deadline = Math.floor(Date.now() / 1000) + 3600;
        const desiredOutput = ethers.parseEther('50');

        await expect(
          router.connect(user1).swapTokensForExactTokens(
            desiredOutput,
            ethers.parseEther('1'), // Too low max input
            await tokenA.getAddress(),
            await tokenB.getAddress(),
            user2.address,
            deadline,
            3
          )
        ).to.be.revertedWith('CarbonCreditRouter: EXCESSIVE_INPUT_AMOUNT');
      });
    });
  });

  describe('Quote Functions', function () {
    beforeEach(async function () {
      await addLiquidity(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        LIQUIDITY_AMOUNT_A,
        LIQUIDITY_AMOUNT_B
      );
    });

    it('Should get amounts out correctly', async function () {
      const path = [await tokenA.getAddress(), await tokenB.getAddress()];
      const feeTiers = [FEE_TIER];
      const amountIn = ethers.parseEther('100');

      const amounts = await router.getAmountsOut(amountIn, path, feeTiers);

      expect(amounts.length).to.equal(2);
      expect(amounts[0]).to.equal(amountIn);
      expect(amounts[1]).to.be.gt(0);
    });

    it('Should get amounts in correctly', async function () {
      const path = [await tokenA.getAddress(), await tokenB.getAddress()];
      const feeTiers = [FEE_TIER];
      const amountOut = ethers.parseEther('50');

      const amounts = await router.getAmountsIn(amountOut, path, feeTiers);

      expect(amounts.length).to.equal(2);
      expect(amounts[0]).to.be.gt(0);
      expect(amounts[1]).to.equal(amountOut);
    });

    it('Should handle invalid paths', async function () {
      const invalidPath = [await tokenA.getAddress()]; // Too short
      const feeTiers = [];

      await expect(
        router.getAmountsOut(ethers.parseEther('100'), invalidPath, feeTiers)
      ).to.be.revertedWith('CarbonCreditRouter: INVALID_PATH');
    });

    it('Should handle mismatched path and fee tiers', async function () {
      const path = [await tokenA.getAddress(), await tokenB.getAddress()];
      const feeTiers = []; // Wrong length

      await expect(
        router.getAmountsOut(ethers.parseEther('100'), path, feeTiers)
      ).to.be.revertedWith('CarbonCreditRouter: INVALID_FEE_TIERS');
    });
  });

  describe('Access Control and Configuration', function () {
    it('Should allow admin to set routing fee', async function () {
      const ROUTER_ADMIN_ROLE = await router.ROUTER_ADMIN_ROLE();
      await router.grantRole(ROUTER_ADMIN_ROLE, user1.address);

      await router.connect(user1).setRoutingFee(10); // 0.1%

      expect(await router.routingFeeBasisPoints()).to.equal(10);
    });

    it('Should prevent setting excessive routing fee', async function () {
      const ROUTER_ADMIN_ROLE = await router.ROUTER_ADMIN_ROLE();
      await router.grantRole(ROUTER_ADMIN_ROLE, user1.address);

      await expect(
        router.connect(user1).setRoutingFee(101) // > 1%
      ).to.be.revertedWith('CarbonCreditRouter: FEE_TOO_HIGH');
    });

    it('Should allow admin to set authorized callers', async function () {
      await router.setAuthorizedCaller(user1.address, true);
      expect(await router.authorizedCallers(user1.address)).to.be.true;

      await router.setAuthorizedCaller(user1.address, false);
      expect(await router.authorizedCallers(user1.address)).to.be.false;
    });

    it('Should allow admin to withdraw fees', async function () {
      // First generate some fees
      await addLiquidity(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        LIQUIDITY_AMOUNT_A,
        LIQUIDITY_AMOUNT_B
      );

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const swapParams = {
        tokenIn: await tokenA.getAddress(),
        tokenOut: await tokenB.getAddress(),
        amountIn: ethers.parseEther('100'),
        amountOutMin: 1,
        to: user2.address,
        deadline: deadline,
        maxHops: 3,
      };

      await router.connect(user1).swapExactTokensForTokens(swapParams);

      const routerBalance = await tokenA.balanceOf(await router.getAddress());
      expect(routerBalance).to.be.gt(0);

      const balanceBefore = await tokenA.balanceOf(owner.address);

      await router.withdrawFees(
        await tokenA.getAddress(),
        owner.address,
        routerBalance
      );

      const balanceAfter = await tokenA.balanceOf(owner.address);
      expect(balanceAfter - balanceBefore).to.equal(routerBalance);
    });
  });

  describe('Emergency Functions', function () {
    it('Should allow admin to pause/unpause', async function () {
      await router.pause();

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const liquidityParams = {
        tokenA: await tokenA.getAddress(),
        tokenB: await tokenB.getAddress(),
        feeTier: FEE_TIER,
        amountADesired: LIQUIDITY_AMOUNT_A,
        amountBDesired: LIQUIDITY_AMOUNT_B,
        amountAMin: 1,
        amountBMin: 1,
        to: user1.address,
        deadline: deadline,
      };

      await expect(
        router.connect(user1).addLiquidity(liquidityParams)
      ).to.be.revertedWithCustomError(router, 'EnforcedPause');

      await router.unpause();

      // Should work after unpause
      await router.connect(user1).addLiquidity(liquidityParams);
    });
  });

  describe('Edge Cases', function () {
    it('Should handle zero amounts gracefully', async function () {
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const swapParams = {
        tokenIn: await tokenA.getAddress(),
        tokenOut: await tokenB.getAddress(),
        amountIn: 0,
        amountOutMin: 0,
        to: user2.address,
        deadline: deadline,
        maxHops: 3,
      };

      // This should fail at the AMM level due to zero input
      await expect(router.connect(user1).swapExactTokensForTokens(swapParams))
        .to.be.reverted;
    });

    it('Should handle non-existent pools', async function () {
      const TokenD = await ethers.getContractFactory('WRFToken');
      const tokenD = await TokenD.deploy();

      const route = await router.getOptimalRoute(
        await tokenA.getAddress(),
        await tokenD.getAddress(),
        ethers.parseEther('100'),
        3
      );

      expect(route.pools.length).to.equal(0);
      expect(route.expectedOutput).to.equal(0);
    });

    it('Should maintain consistent state under stress', async function () {
      // Perform many operations
      await addLiquidity(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        LIQUIDITY_AMOUNT_A,
        LIQUIDITY_AMOUNT_B
      );

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      for (let i = 0; i < 5; i++) {
        const swapParams = {
          tokenIn: await tokenA.getAddress(),
          tokenOut: await tokenB.getAddress(),
          amountIn: ethers.parseEther('10'),
          amountOutMin: 1,
          to: user2.address,
          deadline: deadline,
          maxHops: 3,
        };

        await router.connect(user1).swapExactTokensForTokens(swapParams);
      }

      // Router should still function correctly
      const route = await router.getOptimalRoute(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        ethers.parseEther('100'),
        3
      );

      expect(route.pools.length).to.be.gt(0);
    });
  });
});
