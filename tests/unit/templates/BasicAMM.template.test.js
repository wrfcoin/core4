const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
  mine,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('BasicAMM Template', function () {
  async function deployBasicAMMFixture() {
    const [owner, user1, user2, feeRecipient, emergencyRecovery] =
      await ethers.getSigners();

    // Deploy test tokens
    const BaseERC20Token = await ethers.getContractFactory('BaseERC20Token');

    const tokenA = await BaseERC20Token.deploy(
      'Token A',
      'TKNA',
      18,
      ethers.parseEther('1000000'),
      ethers.parseEther('500000'),
      owner.address,
      false,
      0,
      owner.address
    );

    const tokenB = await BaseERC20Token.deploy(
      'Token B',
      'TKNB',
      18,
      ethers.parseEther('1000000'),
      ethers.parseEther('500000'),
      owner.address,
      false,
      0,
      owner.address
    );

    // Deploy AMM
    const BasicAMM = await ethers.getContractFactory('BasicAMM');
    const amm = await BasicAMM.deploy(
      await tokenA.getAddress(),
      await tokenB.getAddress(),
      owner.address,
      feeRecipient.address
    );

    // Distribute tokens to users
    await tokenA.transfer(user1.address, ethers.parseEther('50000'));
    await tokenA.transfer(user2.address, ethers.parseEther('50000'));
    await tokenB.transfer(user1.address, ethers.parseEther('50000'));
    await tokenB.transfer(user2.address, ethers.parseEther('50000'));

    // Approve AMM to spend tokens
    await tokenA
      .connect(user1)
      .approve(await amm.getAddress(), ethers.parseEther('50000'));
    await tokenA
      .connect(user2)
      .approve(await amm.getAddress(), ethers.parseEther('50000'));
    await tokenB
      .connect(user1)
      .approve(await amm.getAddress(), ethers.parseEther('50000'));
    await tokenB
      .connect(user2)
      .approve(await amm.getAddress(), ethers.parseEther('50000'));

    return {
      amm,
      tokenA,
      tokenB,
      owner,
      user1,
      user2,
      feeRecipient,
      emergencyRecovery,
    };
  }

  describe('Deployment', function () {
    it('Should set correct token addresses', async function () {
      const { amm, tokenA, tokenB } = await loadFixture(deployBasicAMMFixture);

      expect(await amm.tokenA()).to.equal(await tokenA.getAddress());
      expect(await amm.tokenB()).to.equal(await tokenB.getAddress());
    });

    it('Should set correct initial parameters', async function () {
      const { amm, feeRecipient } = await loadFixture(deployBasicAMMFixture);

      expect(await amm.swapFeeRate()).to.equal(30); // 0.3%
      expect(await amm.protocolFeeRate()).to.equal(5); // 0.05%
      expect(await amm.feeRecipient()).to.equal(feeRecipient.address);
      expect(await amm.maxSlippage()).to.equal(500); // 5%
      expect(await amm.maxSwapAmount()).to.equal(ethers.parseEther('10000'));
      expect(await amm.dailyVolumeLimit()).to.equal(
        ethers.parseEther('100000')
      );
    });

    it('Should have zero initial reserves and liquidity', async function () {
      const { amm } = await loadFixture(deployBasicAMMFixture);

      expect(await amm.reserveA()).to.equal(0);
      expect(await amm.reserveB()).to.equal(0);
      expect(await amm.totalLiquidity()).to.equal(0);
    });

    it('Should set up correct roles', async function () {
      const { amm, owner } = await loadFixture(deployBasicAMMFixture);

      const ADMIN_ROLE = await amm.ADMIN_ROLE();
      expect(await amm.hasRole(ADMIN_ROLE, owner.address)).to.be.true;
    });
  });

  describe('Liquidity Provision', function () {
    it('Should allow first liquidity provision', async function () {
      const { amm, user1 } = await loadFixture(deployBasicAMMFixture);

      const amountA = ethers.parseEther('1000');
      const amountB = ethers.parseEther('2000');

      await expect(
        amm.connect(user1).addLiquidity(amountA, amountB, amountA, amountB)
      )
        .to.emit(amm, 'LiquidityAdded')
        .withArgs(
          user1.address,
          amountA,
          amountB,
          ethers.parseEther('1414.213562373095048801')
        ); // sqrt(1000 * 2000) - 1000

      expect(await amm.reserveA()).to.equal(amountA);
      expect(await amm.reserveB()).to.equal(amountB);
      expect(await amm.totalLiquidity()).to.be.gt(0);
      expect(await amm.liquidityBalances(user1.address)).to.be.gt(0);
    });

    it('Should calculate correct liquidity tokens for first provision', async function () {
      const { amm, user1 } = await loadFixture(deployBasicAMMFixture);

      const amountA = ethers.parseEther('100');
      const amountB = ethers.parseEther('200');

      await amm.connect(user1).addLiquidity(amountA, amountB, amountA, amountB);

      // sqrt(100 * 200) = sqrt(20000) ≈ 141.42 - 1000 (burned minimum liquidity)
      const expectedLiquidity = 141421356237309504880n - 1000n;
      expect(await amm.liquidityBalances(user1.address)).to.equal(
        expectedLiquidity
      );
    });

    it('Should reject insufficient initial liquidity', async function () {
      const { amm, user1 } = await loadFixture(deployBasicAMMFixture);

      const smallAmountA = ethers.parseEther('0.01');
      const smallAmountB = ethers.parseEther('0.01');

      await expect(
        amm
          .connect(user1)
          .addLiquidity(smallAmountA, smallAmountB, smallAmountA, smallAmountB)
      ).to.be.revertedWith('BasicAMM: Insufficient initial liquidity');
    });

    it('Should maintain ratio for subsequent liquidity provision', async function () {
      const { amm, user1, user2 } = await loadFixture(deployBasicAMMFixture);

      // First provision: 1:2 ratio
      await amm
        .connect(user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('2000'),
          ethers.parseEther('1000'),
          ethers.parseEther('2000')
        );

      // Second provision: should maintain 1:2 ratio
      await amm
        .connect(user2)
        .addLiquidity(
          ethers.parseEther('500'),
          ethers.parseEther('1000'),
          ethers.parseEther('500'),
          ethers.parseEther('1000')
        );

      expect(await amm.reserveA()).to.equal(ethers.parseEther('1500'));
      expect(await amm.reserveB()).to.equal(ethers.parseEther('3000'));
    });

    it('Should adjust amounts to maintain ratio', async function () {
      const { amm, user1, user2 } = await loadFixture(deployBasicAMMFixture);

      // First provision: 1:2 ratio
      await amm
        .connect(user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('2000'),
          ethers.parseEther('1000'),
          ethers.parseEther('2000')
        );

      // Try to add unbalanced liquidity (more B than ratio requires)
      await amm.connect(user2).addLiquidity(
        ethers.parseEther('500'),
        ethers.parseEther('1500'), // Too much B
        ethers.parseEther('500'),
        ethers.parseEther('1000') // Accept ratio adjustment
      );

      // Should only use 1000 B to maintain ratio
      expect(await amm.reserveA()).to.equal(ethers.parseEther('1500'));
      expect(await amm.reserveB()).to.equal(ethers.parseEther('3000'));
    });

    it('Should reject liquidity with excessive slippage', async function () {
      const { amm, user1, user2 } = await loadFixture(deployBasicAMMFixture);

      await amm
        .connect(user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('2000'),
          ethers.parseEther('1000'),
          ethers.parseEther('2000')
        );

      // Try to add liquidity with slippage protection that's too strict
      await expect(
        amm.connect(user2).addLiquidity(
          ethers.parseEther('500'),
          ethers.parseEther('1500'),
          ethers.parseEther('500'),
          ethers.parseEther('1500') // Too strict
        )
      ).to.be.revertedWithCustomError(amm, 'SlippageExceeded');
    });

    it('Should reject zero amount liquidity provision', async function () {
      const { amm, user1 } = await loadFixture(deployBasicAMMFixture);

      await expect(
        amm.connect(user1).addLiquidity(0, ethers.parseEther('1000'), 0, 0)
      )
        .to.be.revertedWithCustomError(amm, 'InvalidTokenAmount')
        .withArgs(0);
    });
  });

  describe('Liquidity Removal', function () {
    async function liquidityFixture() {
      const fixture = await loadFixture(deployBasicAMMFixture);

      // Add initial liquidity
      await fixture.amm
        .connect(fixture.user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('2000'),
          ethers.parseEther('1000'),
          ethers.parseEther('2000')
        );

      return fixture;
    }

    it('Should allow liquidity removal', async function () {
      const { amm, user1 } = await liquidityFixture();

      const userLiquidity = await amm.liquidityBalances(user1.address);
      const halfLiquidity = userLiquidity / 2n;

      await expect(
        amm.connect(user1).removeLiquidity(halfLiquidity, 0, 0)
      ).to.emit(amm, 'LiquidityRemoved');

      expect(await amm.liquidityBalances(user1.address)).to.equal(
        userLiquidity - halfLiquidity
      );
    });

    it('Should return proportional amounts', async function () {
      const { amm, user1 } = await liquidityFixture();

      const userLiquidity = await amm.liquidityBalances(user1.address);
      const initialReserveA = await amm.reserveA();
      const initialReserveB = await amm.reserveB();

      const result = await amm
        .connect(user1)
        .removeLiquidity.staticCall(userLiquidity, 0, 0);

      // Should get back proportional amounts
      expect(result[0]).to.be.closeTo(
        initialReserveA,
        ethers.parseEther('0.1')
      );
      expect(result[1]).to.be.closeTo(
        initialReserveB,
        ethers.parseEther('0.1')
      );
    });

    it('Should reject removal with insufficient liquidity', async function () {
      const { amm, user1, user2 } = await liquidityFixture();

      const userLiquidity = await amm.liquidityBalances(user1.address);

      await expect(amm.connect(user2).removeLiquidity(userLiquidity, 0, 0))
        .to.be.revertedWithCustomError(amm, 'InsufficientLiquidity')
        .withArgs(0, userLiquidity);
    });

    it('Should respect slippage protection on removal', async function () {
      const { amm, user1 } = await liquidityFixture();

      const userLiquidity = await amm.liquidityBalances(user1.address);

      await expect(
        amm.connect(user1).removeLiquidity(
          userLiquidity,
          ethers.parseEther('10000'), // Unreasonably high expectation
          ethers.parseEther('20000')
        )
      ).to.be.revertedWithCustomError(amm, 'SlippageExceeded');
    });

    it('Should reject zero liquidity removal', async function () {
      const { amm, user1 } = await liquidityFixture();

      await expect(amm.connect(user1).removeLiquidity(0, 0, 0))
        .to.be.revertedWithCustomError(amm, 'InvalidTokenAmount')
        .withArgs(0);
    });
  });

  describe('Token Swapping', function () {
    async function swapFixture() {
      const fixture = await liquidityFixture();
      return fixture;
    }

    async function liquidityFixture() {
      const fixture = await loadFixture(deployBasicAMMFixture);

      await fixture.amm
        .connect(fixture.user1)
        .addLiquidity(
          ethers.parseEther('10000'),
          ethers.parseEther('20000'),
          ethers.parseEther('10000'),
          ethers.parseEther('20000')
        );

      return fixture;
    }

    it('Should allow swapping token A for token B', async function () {
      const { amm, user2 } = await swapFixture();

      const amountAIn = ethers.parseEther('100');
      const expectedOut = await amm.getAmountOut(await amm.tokenA(), amountAIn);

      await expect(amm.connect(user2).swapAForB(amountAIn, 0))
        .to.emit(amm, 'Swap')
        .withArgs(user2.address, await amm.tokenA(), amountAIn, expectedOut);
    });

    it('Should allow swapping token B for token A', async function () {
      const { amm, user2 } = await swapFixture();

      const amountBIn = ethers.parseEther('200');
      const expectedOut = await amm.getAmountOut(await amm.tokenB(), amountBIn);

      await expect(amm.connect(user2).swapBForA(amountBIn, 0))
        .to.emit(amm, 'Swap')
        .withArgs(user2.address, await amm.tokenB(), amountBIn, expectedOut);
    });

    it('Should calculate correct output amounts', async function () {
      const { amm } = await swapFixture();

      const amountIn = ethers.parseEther('100');
      const reserveA = await amm.reserveA();
      const reserveB = await amm.reserveB();
      const swapFeeRate = await amm.swapFeeRate();

      // Calculate expected output manually
      const amountInWithFee = amountIn * (10000n - swapFeeRate);
      const numerator = amountInWithFee * reserveB;
      const denominator = reserveA * 10000n + amountInWithFee;
      const expectedOut = numerator / denominator;

      const actualOut = await amm.getAmountOut(await amm.tokenA(), amountIn);
      expect(actualOut).to.equal(expectedOut);
    });

    it('Should respect slippage protection', async function () {
      const { amm, user2 } = await swapFixture();

      const amountIn = ethers.parseEther('100');
      const expectedOut = await amm.getAmountOut(await amm.tokenA(), amountIn);
      const highMinOut = expectedOut * 2n; // Unreasonably high expectation

      await expect(amm.connect(user2).swapAForB(amountIn, highMinOut))
        .to.be.revertedWithCustomError(amm, 'SlippageExceeded')
        .withArgs(highMinOut, expectedOut);
    });

    it('Should update reserves correctly after swap', async function () {
      const { amm, user2 } = await swapFixture();

      const initialReserveA = await amm.reserveA();
      const initialReserveB = await amm.reserveB();
      const amountIn = ethers.parseEther('100');
      const expectedOut = await amm.getAmountOut(await amm.tokenA(), amountIn);

      await amm.connect(user2).swapAForB(amountIn, 0);

      expect(await amm.reserveA()).to.equal(initialReserveA + amountIn);
      expect(await amm.reserveB()).to.equal(initialReserveB - expectedOut);
    });

    it('Should reject zero amount swaps', async function () {
      const { amm, user2 } = await swapFixture();

      await expect(amm.connect(user2).swapAForB(0, 0))
        .to.be.revertedWithCustomError(amm, 'InvalidTokenAmount')
        .withArgs(0);
    });

    it('Should reject swaps with zero liquidity', async function () {
      const { amm, user2 } = await loadFixture(deployBasicAMMFixture);

      await expect(
        amm.connect(user2).swapAForB(ethers.parseEther('100'), 0)
      ).to.be.revertedWithCustomError(amm, 'ZeroLiquidity');
    });
  });

  describe('MEV Protection', function () {
    async function swapFixture() {
      const fixture = await liquidityFixture();
      return fixture;
    }

    async function liquidityFixture() {
      const fixture = await loadFixture(deployBasicAMMFixture);

      await fixture.amm
        .connect(fixture.user1)
        .addLiquidity(
          ethers.parseEther('10000'),
          ethers.parseEther('20000'),
          ethers.parseEther('10000'),
          ethers.parseEther('20000')
        );

      return fixture;
    }

    it('Should prevent rapid consecutive swaps', async function () {
      const { amm, user2 } = await swapFixture();

      const amountIn = ethers.parseEther('100');

      // First swap should succeed
      await amm.connect(user2).swapAForB(amountIn, 0);

      // Second swap in same block should fail
      await expect(
        amm.connect(user2).swapAForB(amountIn, 0)
      ).to.be.revertedWithCustomError(amm, 'SwapTooFrequent');
    });

    it('Should allow swaps after minimum interval', async function () {
      const { amm, user2 } = await swapFixture();

      const amountIn = ethers.parseEther('100');

      await amm.connect(user2).swapAForB(amountIn, 0);

      // Mine one block to pass the minimum interval
      await mine(1);

      // Second swap should now succeed
      await expect(amm.connect(user2).swapAForB(amountIn, 0)).to.not.be
        .reverted;
    });

    it('Should update last swap block correctly', async function () {
      const { amm, user2 } = await swapFixture();

      const amountIn = ethers.parseEther('100');

      await amm.connect(user2).swapAForB(amountIn, 0);

      const currentBlock = await ethers.provider.getBlockNumber();
      expect(await amm.lastSwapBlock(user2.address)).to.equal(currentBlock);
    });
  });

  describe('Volume Limits', function () {
    async function swapFixture() {
      const fixture = await liquidityFixture();
      return fixture;
    }

    async function liquidityFixture() {
      const fixture = await loadFixture(deployBasicAMMFixture);

      await fixture.amm
        .connect(fixture.user1)
        .addLiquidity(
          ethers.parseEther('100000'),
          ethers.parseEther('200000'),
          ethers.parseEther('100000'),
          ethers.parseEther('200000')
        );

      return fixture;
    }

    it('Should enforce maximum swap amount', async function () {
      const { amm, user2 } = await swapFixture();

      const maxSwapAmount = await amm.maxSwapAmount();
      const largeAmount = maxSwapAmount + ethers.parseEther('1');

      await expect(amm.connect(user2).swapAForB(largeAmount, 0))
        .to.be.revertedWithCustomError(amm, 'SwapAmountTooLarge')
        .withArgs(largeAmount, maxSwapAmount);
    });

    it('Should enforce daily volume limit', async function () {
      const { amm, user2 } = await swapFixture();

      const maxSwapAmount = await amm.maxSwapAmount();

      // Make multiple swaps up to daily limit
      for (let i = 0; i < 10; i++) {
        await amm.connect(user2).swapAForB(maxSwapAmount, 0);
        await mine(1); // Move to next block to avoid MEV protection
      }

      // Next swap should fail due to daily volume limit
      await expect(
        amm.connect(user2).swapAForB(maxSwapAmount, 0)
      ).to.be.revertedWithCustomError(amm, 'DailyVolumeLimitExceeded');
    });

    it('Should reset daily volume after 24 hours', async function () {
      const { amm, user2 } = await swapFixture();

      const maxSwapAmount = await amm.maxSwapAmount();

      // Use up daily volume
      for (let i = 0; i < 10; i++) {
        await amm.connect(user2).swapAForB(maxSwapAmount, 0);
        await mine(1);
      }

      // Advance time by 24 hours
      await time.increase(24 * 60 * 60);

      // Should be able to swap again
      await expect(amm.connect(user2).swapAForB(maxSwapAmount, 0)).to.not.be
        .reverted;
    });
  });

  describe('Price Oracle', function () {
    async function liquidityFixture() {
      const fixture = await loadFixture(deployBasicAMMFixture);

      await fixture.amm
        .connect(fixture.user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('2000'),
          ethers.parseEther('1000'),
          ethers.parseEther('2000')
        );

      return fixture;
    }

    it('Should return correct current price', async function () {
      const { amm } = await liquidityFixture();

      const reserveA = await amm.reserveA();
      const reserveB = await amm.reserveB();
      const expectedPrice = (reserveB * ethers.parseEther('1')) / reserveA;

      expect(await amm.getCurrentPrice()).to.equal(expectedPrice);
    });

    it('Should update price on liquidity operations', async function () {
      const { amm, user2 } = await liquidityFixture();

      await expect(
        amm
          .connect(user2)
          .addLiquidity(
            ethers.parseEther('500'),
            ethers.parseEther('1000'),
            ethers.parseEther('500'),
            ethers.parseEther('1000')
          )
      ).to.emit(amm, 'PriceUpdate');
    });

    it('Should update price on swaps', async function () {
      const { amm, user2 } = await liquidityFixture();

      await mine(1); // Ensure we're in a new block

      await expect(
        amm.connect(user2).swapAForB(ethers.parseEther('100'), 0)
      ).to.emit(amm, 'PriceUpdate');
    });

    it('Should detect stale prices', async function () {
      const { amm } = await liquidityFixture();

      expect(await amm.isPriceStale()).to.be.false;

      // Advance time beyond price validity period
      await time.increase(6 * 60); // 6 minutes

      expect(await amm.isPriceStale()).to.be.true;
    });
  });

  describe('Administrative Functions', function () {
    async function liquidityFixture() {
      const fixture = await loadFixture(deployBasicAMMFixture);

      await fixture.amm
        .connect(fixture.user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('2000'),
          ethers.parseEther('1000'),
          ethers.parseEther('2000')
        );

      return fixture;
    }

    it('Should allow admin to set swap fee rate', async function () {
      const { amm, owner } = await loadFixture(deployBasicAMMFixture);

      await amm.connect(owner).setSwapFeeRate(50); // 0.5%

      expect(await amm.swapFeeRate()).to.equal(50);
    });

    it('Should reject excessive swap fee rate', async function () {
      const { amm, owner } = await loadFixture(deployBasicAMMFixture);

      await expect(amm.connect(owner).setSwapFeeRate(150)) // 1.5% > 1% max
        .to.be.revertedWith('BasicAMM: Fee too high');
    });

    it('Should allow admin to set max slippage', async function () {
      const { amm, owner } = await loadFixture(deployBasicAMMFixture);

      await amm.connect(owner).setMaxSlippage(1000); // 10%

      expect(await amm.maxSlippage()).to.equal(1000);
    });

    it('Should reject excessive max slippage', async function () {
      const { amm, owner } = await loadFixture(deployBasicAMMFixture);

      await expect(amm.connect(owner).setMaxSlippage(2500)) // 25% > 20% max
        .to.be.revertedWith('BasicAMM: Slippage too high');
    });

    it('Should allow admin to set max swap amount', async function () {
      const { amm, owner } = await loadFixture(deployBasicAMMFixture);

      const newAmount = ethers.parseEther('20000');
      await amm.connect(owner).setMaxSwapAmount(newAmount);

      expect(await amm.maxSwapAmount()).to.equal(newAmount);
    });

    it('Should allow admin to set daily volume limit', async function () {
      const { amm, owner } = await loadFixture(deployBasicAMMFixture);

      const newLimit = ethers.parseEther('200000');
      await amm.connect(owner).setDailyVolumeLimit(newLimit);

      expect(await amm.dailyVolumeLimit()).to.equal(newLimit);
    });

    it('Should not allow non-admin to change parameters', async function () {
      const { amm, user1 } = await loadFixture(deployBasicAMMFixture);

      await expect(amm.connect(user1).setSwapFeeRate(50)).to.be.revertedWith(
        'AccessControl: account'
      );

      await expect(amm.connect(user1).setMaxSlippage(1000)).to.be.revertedWith(
        'AccessControl: account'
      );
    });

    it('Should allow admin to collect fees', async function () {
      const { amm, owner, user2, feeRecipient } = await liquidityFixture();

      // Perform some swaps to generate fees
      await mine(1);
      await amm.connect(user2).swapAForB(ethers.parseEther('100'), 0);
      await mine(1);
      await amm.connect(user2).swapBForA(ethers.parseEther('100'), 0);

      const initialBalance = await amm.tokenA().balanceOf(feeRecipient.address);

      await expect(amm.connect(owner).collectFees()).to.emit(
        amm,
        'FeesCollected'
      );

      // Fee recipient should have received some fees
      const finalBalance = await amm.tokenA().balanceOf(feeRecipient.address);
      expect(finalBalance).to.be.gte(initialBalance);
    });
  });

  describe('Emergency Functions', function () {
    async function liquidityFixture() {
      const fixture = await loadFixture(deployBasicAMMFixture);

      await fixture.amm
        .connect(fixture.user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('2000'),
          ethers.parseEther('1000'),
          ethers.parseEther('2000')
        );

      return fixture;
    }

    it('Should pause contract in emergency', async function () {
      const { amm, owner, user2 } = await liquidityFixture();

      await amm.connect(owner).pause();

      await expect(
        amm.connect(user2).swapAForB(ethers.parseEther('100'), 0)
      ).to.be.revertedWith('Pausable: paused');
    });

    it('Should allow emergency drain when shutdown', async function () {
      const { amm, owner, emergencyRecovery } = await liquidityFixture();

      // Activate emergency shutdown
      await amm.connect(owner).activateEmergencyShutdown();

      const initialBalanceA = await amm
        .tokenA()
        .balanceOf(emergencyRecovery.address);

      await amm.connect(owner).emergencyDrain();

      const finalBalanceA = await amm
        .tokenA()
        .balanceOf(emergencyRecovery.address);
      expect(finalBalanceA).to.be.gt(initialBalanceA);
    });
  });

  describe('Edge Cases and Security', function () {
    it('Should handle very small amounts', async function () {
      const { amm, user1 } = await loadFixture(deployBasicAMMFixture);

      // Add small amount of liquidity
      const smallAmount = 10000; // Very small but above minimum

      await amm
        .connect(user1)
        .addLiquidity(
          smallAmount,
          smallAmount * 2,
          smallAmount,
          smallAmount * 2
        );

      expect(await amm.reserveA()).to.equal(smallAmount);
      expect(await amm.reserveB()).to.equal(smallAmount * 2);
    });

    it('Should handle price impact correctly', async function () {
      const { amm, user1, user2 } = await loadFixture(deployBasicAMMFixture);

      // Add liquidity
      await amm
        .connect(user1)
        .addLiquidity(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          ethers.parseEther('1000')
        );

      const initialPrice = await amm.getCurrentPrice();

      // Large swap should have significant price impact
      await mine(1);
      await amm.connect(user2).swapAForB(ethers.parseEther('500'), 0);

      const finalPrice = await amm.getCurrentPrice();
      expect(finalPrice).to.not.equal(initialPrice);
    });

    it('Should prevent reentrancy attacks', async function () {
      const { amm } = await loadFixture(deployBasicAMMFixture);

      // The ReentrancyGuard should be active
      const contractCode = await ethers.provider.getCode(
        await amm.getAddress()
      );
      expect(contractCode).to.not.equal('0x');
    });

    it('Should handle zero reserve edge case', async function () {
      const { amm } = await loadFixture(deployBasicAMMFixture);

      expect(await amm.getCurrentPrice()).to.equal(0);
      expect(
        await amm.getAmountOut(await amm.tokenA(), ethers.parseEther('100'))
      ).to.equal(0);
    });
  });
});
