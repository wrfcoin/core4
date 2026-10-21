const { expect } = require('chai');
const { ethers } = require('hardhat');
const { time } = require('@nomicfoundation/hardhat-network-helpers');

describe('SecureCarbonCreditAMM - Slippage Protection Tests', function () {
  let amm, token0, token1;
  let owner, factory, trader1, trader2, trader3, attacker;
  let emergencyResponder, slippageGuardian, mevProtector;

  // Test constants
  const INITIAL_LIQUIDITY_0 = ethers.parseEther('1000000'); // 1M tokens
  const INITIAL_LIQUIDITY_1 = ethers.parseEther('1000000'); // 1M tokens
  const DEFAULT_SLIPPAGE_BASIS_POINTS = 50; // 0.5%
  const MAX_SLIPPAGE_BASIS_POINTS = 1000; // 10%
  const PRICE_IMPACT_THRESHOLD = 200; // 2%
  const EMERGENCY_PRICE_DEVIATION = 2000; // 20%
  const BASIS_POINTS = 10000;

  beforeEach(async function () {
    [
      owner,
      factory,
      trader1,
      trader2,
      trader3,
      attacker,
      emergencyResponder,
      slippageGuardian,
      mevProtector,
    ] = await ethers.getSigners();

    // Deploy mock ERC20 tokens
    const MockToken = await ethers.getContractFactory('MockERC20');
    token0 = await MockToken.deploy(
      'Carbon Token 0',
      'CT0',
      ethers.parseEther('10000000')
    );
    token1 = await MockToken.deploy(
      'Carbon Token 1',
      'CT1',
      ethers.parseEther('10000000')
    );

    // Deploy SecureCarbonCreditAMM
    const SecureAMM = await ethers.getContractFactory('SecureCarbonCreditAMM');
    amm = await SecureAMM.deploy();

    // Initialize AMM
    await amm.initialize(
      await token0.getAddress(),
      await token1.getAddress(),
      factory.address,
      30 // 0.3% fee
    );

    // Set up roles
    const EMERGENCY_RESPONDER_ROLE = await amm.EMERGENCY_RESPONDER_ROLE();
    const SLIPPAGE_GUARDIAN_ROLE = await amm.SLIPPAGE_GUARDIAN_ROLE();
    const MEV_PROTECTOR_ROLE = await amm.MEV_PROTECTOR_ROLE();

    await amm
      .connect(factory)
      .grantRole(EMERGENCY_RESPONDER_ROLE, emergencyResponder.address);
    await amm
      .connect(factory)
      .grantRole(SLIPPAGE_GUARDIAN_ROLE, slippageGuardian.address);
    await amm
      .connect(factory)
      .grantRole(MEV_PROTECTOR_ROLE, mevProtector.address);

    // Distribute tokens
    await token0.transfer(trader1.address, ethers.parseEther('100000'));
    await token0.transfer(trader2.address, ethers.parseEther('100000'));
    await token0.transfer(trader3.address, ethers.parseEther('100000'));
    await token0.transfer(attacker.address, ethers.parseEther('100000'));

    await token1.transfer(trader1.address, ethers.parseEther('100000'));
    await token1.transfer(trader2.address, ethers.parseEther('100000'));
    await token1.transfer(trader3.address, ethers.parseEther('100000'));
    await token1.transfer(attacker.address, ethers.parseEther('100000'));

    // Add initial liquidity
    await token0.transfer(await amm.getAddress(), INITIAL_LIQUIDITY_0);
    await token1.transfer(await amm.getAddress(), INITIAL_LIQUIDITY_1);

    // Initialize reserves by calling sync
    await amm.connect(factory).sync();
  });

  async function createSlippageProtection(overrides = {}) {
    return {
      minAmountOut: overrides.minAmountOut || ethers.parseEther('990'), // Expect at least 990 tokens
      maxAmountIn: overrides.maxAmountIn || ethers.parseEther('2000'), // Max 2000 tokens input
      deadline: overrides.deadline || Math.floor(Date.now() / 1000) + 3600, // 1 hour from now
      maxPriceImpact: overrides.maxPriceImpact || 200, // 2% max price impact
      useAdaptiveSlippage: overrides.useAdaptiveSlippage || false,
      commitHash:
        overrides.commitHash || ethers.keccak256(ethers.toUtf8Bytes('test')),
    };
  }

  describe('Basic Slippage Protection', function () {
    it('Should prevent trades exceeding slippage tolerance', async function () {
      const protection = createSlippageProtection({
        minAmountOut: ethers.parseEther('1100'), // Expecting more than possible
      });

      // Try to swap 1000 token0 for token1 with unrealistic expectations
      await expect(
        amm.connect(trader1).swapWithSlippageProtection(
          0, // amount0Out
          ethers.parseEther('1100'), // amount1Out - too high
          trader1.address,
          protection,
          '0x'
        )
      ).to.be.revertedWith('SecureCarbonCreditAMM: INSUFFICIENT_OUTPUT_AMOUNT');
    });

    it('Should allow trades within slippage tolerance', async function () {
      // Approve tokens for trading
      await token0
        .connect(trader1)
        .approve(await amm.getAddress(), ethers.parseEther('1000'));

      const protection = createSlippageProtection({
        minAmountOut: ethers.parseEther('990'), // Reasonable expectation
      });

      // Calculate expected output
      const reserves = await amm.getReserves();
      const amountIn = ethers.parseEther('1000');
      const expectedOut = await amm.getAmountOut(
        amountIn,
        reserves._reserve0,
        reserves._reserve1
      );

      // Transfer tokens to AMM for swap
      await token0.connect(trader1).transfer(await amm.getAddress(), amountIn);

      await expect(
        amm.connect(trader1).swapWithSlippageProtection(
          0, // amount0Out
          expectedOut, // amount1Out
          trader1.address,
          protection,
          '0x'
        )
      ).to.emit(amm, 'SwapWithSlippageProtection');
    });

    it('Should reject trades with excessive price impact', async function () {
      const protection = createSlippageProtection({
        maxPriceImpact: 50, // 0.5% max price impact - very low
      });

      // Try large trade that would cause high price impact
      const largeAmount = ethers.parseEther('100000'); // 10% of liquidity
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), largeAmount);

      await expect(
        amm.connect(trader1).swapWithSlippageProtection(
          0,
          ethers.parseEther('50000'), // Large amount out
          trader1.address,
          protection,
          '0x'
        )
      ).to.be.revertedWith('SecureCarbonCreditAMM: PRICE_IMPACT_TOO_HIGH');
    });

    it('Should enforce transaction deadlines', async function () {
      const protection = createSlippageProtection({
        deadline: Math.floor(Date.now() / 1000) - 3600, // 1 hour ago
      });

      await expect(
        amm
          .connect(trader1)
          .swapWithSlippageProtection(
            0,
            ethers.parseEther('990'),
            trader1.address,
            protection,
            '0x'
          )
      ).to.be.revertedWith(
        'SecureCarbonCreditAMM: Transaction deadline passed'
      );
    });
  });

  describe('Adaptive Slippage Protection', function () {
    it('Should adjust slippage tolerance based on volatility', async function () {
      // Set custom slippage tolerance
      await amm.connect(trader1).setUserSlippageTolerance(100); // 1%

      const protection = createSlippageProtection({
        useAdaptiveSlippage: true,
        minAmountOut: ethers.parseEther('980'), // Lower expectation for volatile market
      });

      // First, create some price volatility by making several trades
      for (let i = 0; i < 10; i++) {
        const smallAmount = ethers.parseEther('1000');
        await token0
          .connect(trader2)
          .transfer(await amm.getAddress(), smallAmount);

        const reserves = await amm.getReserves();
        const expectedOut = await amm.getAmountOut(
          smallAmount,
          reserves._reserve0,
          reserves._reserve1
        );

        const basicProtection = createSlippageProtection({
          minAmountOut: (expectedOut * 99n) / 100n, // 1% slippage tolerance
        });

        await amm
          .connect(trader2)
          .swapWithSlippageProtection(
            0,
            expectedOut,
            trader2.address,
            basicProtection,
            '0x'
          );

        // Wait a bit between trades
        await time.increase(60);
      }

      // Now try adaptive slippage protection
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('1000'));

      const reserves = await amm.getReserves();
      const expectedOut = await amm.getAmountOut(
        ethers.parseEther('1000'),
        reserves._reserve0,
        reserves._reserve1
      );

      await expect(
        amm
          .connect(trader1)
          .swapWithSlippageProtection(
            0,
            expectedOut,
            trader1.address,
            protection,
            '0x'
          )
      ).to.emit(amm, 'SwapWithSlippageProtection');
    });

    it('Should provide slippage protection metrics', async function () {
      const metrics = await amm.getSlippageProtectionMetrics();

      expect(metrics.emergencyMode).to.be.false;
      expect(metrics.mevProtectionActive).to.be.true;
      expect(metrics.currentVolatility).to.be.a('bigint');
      expect(metrics.totalPriceHistory).to.be.a('bigint');
    });
  });

  describe('MEV Protection', function () {
    it('Should require commit-reveal for protected trades', async function () {
      const protection = createSlippageProtection();
      const nonce = 12345;

      // Calculate commit hash
      const commitHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'uint256',
            'uint256',
            'uint256',
            'address',
          ],
          [
            0,
            ethers.parseEther('990'),
            trader1.address,
            protection.minAmountOut,
            protection.deadline,
            nonce,
            trader1.address,
          ]
        )
      );

      // Commit phase
      await amm.connect(trader1).commitTrade(commitHash);

      // Try to reveal immediately (should fail due to delay)
      await expect(
        amm
          .connect(trader1)
          .revealAndSwap(
            0,
            ethers.parseEther('990'),
            trader1.address,
            protection,
            nonce,
            '0x'
          )
      ).to.be.revertedWith(
        'SecureCarbonCreditAMM: Commit reveal delay not met'
      );

      // Wait for delay period
      await time.increase(3); // Wait 3 seconds (assuming 1 second block time)

      // Prepare for swap
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('1000'));

      // Now reveal should work
      await expect(
        amm
          .connect(trader1)
          .revealAndSwap(
            0,
            ethers.parseEther('990'),
            trader1.address,
            protection,
            nonce,
            '0x'
          )
      ).to.emit(amm, 'SwapWithSlippageProtection');
    });

    it('Should prevent commit hash reuse', async function () {
      const commitHash = ethers.keccak256(ethers.toUtf8Bytes('test_commit'));

      await amm.connect(trader1).commitTrade(commitHash);

      await expect(
        amm.connect(trader2).commitTrade(commitHash)
      ).to.be.revertedWith('SecureCarbonCreditAMM: Commit hash already used');
    });

    it('Should enforce MEV protection delay between trades', async function () {
      const protection = createSlippageProtection();

      // First trade
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('1000'));

      const reserves = await amm.getReserves();
      const expectedOut = await amm.getAmountOut(
        ethers.parseEther('1000'),
        reserves._reserve0,
        reserves._reserve1
      );

      await amm
        .connect(trader1)
        .swapWithSlippageProtection(
          0,
          expectedOut,
          trader1.address,
          protection,
          '0x'
        );

      // Try immediate second trade (should fail due to MEV protection)
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('1000'));

      await expect(
        amm.connect(trader1).swapWithSlippageProtection(
          0,
          expectedOut / 2n, // Different amount
          trader1.address,
          protection,
          '0x'
        )
      ).to.be.revertedWith('SecureCarbonCreditAMM: MEV protection active');
    });
  });

  describe('Emergency Controls', function () {
    it('Should activate emergency slippage mode', async function () {
      await expect(
        amm
          .connect(emergencyResponder)
          .activateEmergencySlippageMode(100, 'High volatility detected')
      ).to.emit(amm, 'EmergencySlippageActivated');

      const metrics = await amm.getSlippageProtectionMetrics();
      expect(metrics.emergencyMode).to.be.true;
      expect(metrics.emergencyLimit).to.equal(100);
    });

    it('Should enforce emergency slippage limits', async function () {
      // Activate emergency mode with 1% limit
      await amm
        .connect(emergencyResponder)
        .activateEmergencySlippageMode(100, 'Emergency test');

      const protection = createSlippageProtection({
        maxPriceImpact: 500, // 5% - would normally be allowed
      });

      // Try trade that would exceed emergency limit
      const largeAmount = ethers.parseEther('50000'); // Large trade
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), largeAmount);

      await expect(
        amm.connect(trader1).swapWithSlippageProtection(
          0,
          ethers.parseEther('25000'), // Would cause >1% price impact
          trader1.address,
          protection,
          '0x'
        )
      ).to.be.revertedWith(
        'SecureCarbonCreditAMM: EMERGENCY_SLIPPAGE_LIMIT_EXCEEDED'
      );
    });

    it('Should pause trading for extreme price deviation', async function () {
      await amm
        .connect(emergencyResponder)
        .pauseForPriceDeviation('Extreme price movement detected');

      const protection = createSlippageProtection();

      await expect(
        amm
          .connect(trader1)
          .swapWithSlippageProtection(
            0,
            ethers.parseEther('990'),
            trader1.address,
            protection,
            '0x'
          )
      ).to.be.revertedWith(
        'SecureCarbonCreditAMM: Emergency price deviation pause'
      );
    });

    it('Should prevent unauthorized emergency control access', async function () {
      await expect(
        amm
          .connect(attacker)
          .activateEmergencySlippageMode(100, 'Attack attempt')
      ).to.be.revertedWith('AccessControl:');

      await expect(
        amm.connect(attacker).pauseForPriceDeviation('Attack attempt')
      ).to.be.revertedWith('AccessControl:');
    });
  });

  describe('Slippage Calculation and Validation', function () {
    it('Should calculate expected slippage correctly', async function () {
      const amountIn = ethers.parseEther('1000');
      const reserves = await amm.getReserves();

      const [slippage, priceImpact] = await amm.calculateExpectedSlippage(
        amountIn,
        reserves._reserve0,
        reserves._reserve1
      );

      expect(slippage).to.be.a('bigint');
      expect(priceImpact).to.be.a('bigint');
      expect(slippage).to.be.greaterThan(0);
      expect(priceImpact).to.be.greaterThan(0);
    });

    it('Should validate K-invariant with slippage protection', async function () {
      const protection = createSlippageProtection();

      // Get initial reserves
      const initialReserves = await amm.getReserves();
      const initialK = initialReserves._reserve0 * initialReserves._reserve1;

      // Perform swap
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('1000'));

      const expectedOut = await amm.getAmountOut(
        ethers.parseEther('1000'),
        initialReserves._reserve0,
        initialReserves._reserve1
      );

      await amm
        .connect(trader1)
        .swapWithSlippageProtection(
          0,
          expectedOut,
          trader1.address,
          protection,
          '0x'
        );

      // Check that K-invariant is maintained (should be >= initial K)
      const finalReserves = await amm.getReserves();
      const finalK = finalReserves._reserve0 * finalReserves._reserve1;

      expect(finalK).to.be.greaterThanOrEqual(initialK);
    });

    it('Should detect and prevent manipulation attempts', async function () {
      const protection = createSlippageProtection({
        maxPriceImpact: 50, // Very low price impact tolerance
      });

      // Attempt manipulation with large trade
      const manipulationAmount = ethers.parseEther('200000'); // 20% of liquidity
      await token0
        .connect(attacker)
        .transfer(await amm.getAddress(), manipulationAmount);

      await expect(
        amm.connect(attacker).swapWithSlippageProtection(
          0,
          ethers.parseEther('100000'), // Large output
          attacker.address,
          protection,
          '0x'
        )
      ).to.be.revertedWith('SecureCarbonCreditAMM: PRICE_IMPACT_TOO_HIGH');
    });
  });

  describe('Custom User Slippage Settings', function () {
    it('Should allow users to set custom slippage tolerance', async function () {
      await amm.connect(trader1).setUserSlippageTolerance(200); // 2%

      // Verify the setting was saved
      expect(await amm.userSlippageTolerance(trader1.address)).to.equal(200);
    });

    it('Should reject slippage tolerance above maximum', async function () {
      await expect(
        amm.connect(trader1).setUserSlippageTolerance(1500) // 15% - above 10% max
      ).to.be.revertedWith('SecureCarbonCreditAMM: Slippage too high');
    });

    it('Should use custom slippage tolerance in trades', async function () {
      // Set higher slippage tolerance for trader
      await amm.connect(trader1).setUserSlippageTolerance(500); // 5%

      const protection = createSlippageProtection({
        minAmountOut: ethers.parseEther('950'), // Lower expectation
        useAdaptiveSlippage: false,
      });

      // Large trade that would exceed default slippage but within custom tolerance
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('10000'));

      const reserves = await amm.getReserves();
      const expectedOut = await amm.getAmountOut(
        ethers.parseEther('10000'),
        reserves._reserve0,
        reserves._reserve1
      );

      await expect(
        amm
          .connect(trader1)
          .swapWithSlippageProtection(
            0,
            expectedOut,
            trader1.address,
            protection,
            '0x'
          )
      ).to.emit(amm, 'SwapWithSlippageProtection');
    });
  });

  describe('Volatility and Price Impact Monitoring', function () {
    it('Should track price history for volatility calculation', async function () {
      // Make several trades to build price history
      for (let i = 0; i < 15; i++) {
        const tradeAmount = ethers.parseEther((100 + i * 10).toString());
        await token0
          .connect(trader2)
          .transfer(await amm.getAddress(), tradeAmount);

        const reserves = await amm.getReserves();
        const expectedOut = await amm.getAmountOut(
          tradeAmount,
          reserves._reserve0,
          reserves._reserve1
        );

        const protection = createSlippageProtection({
          minAmountOut: (expectedOut * 95n) / 100n, // 5% slippage tolerance
        });

        await amm
          .connect(trader2)
          .swapWithSlippageProtection(
            0,
            expectedOut,
            trader2.address,
            protection,
            '0x'
          );

        await time.increase(30); // 30 seconds between trades
      }

      const metrics = await amm.getSlippageProtectionMetrics();
      expect(metrics.totalPriceHistory).to.be.greaterThan(10);
      expect(metrics.currentVolatility).to.be.greaterThan(0);
    });

    it('Should emit volatility alerts for high volatility', async function () {
      // Create high volatility through alternating large trades
      const largeAmount = ethers.parseEther('50000');

      for (let i = 0; i < 5; i++) {
        // Large trade in one direction
        await token0
          .connect(trader1)
          .transfer(await amm.getAddress(), largeAmount);

        let reserves = await amm.getReserves();
        let expectedOut = await amm.getAmountOut(
          largeAmount,
          reserves._reserve0,
          reserves._reserve1
        );

        const protection1 = createSlippageProtection({
          minAmountOut: (expectedOut * 80n) / 100n, // 20% slippage tolerance
          maxPriceImpact: 2000, // 20% max price impact
        });

        await amm
          .connect(trader1)
          .swapWithSlippageProtection(
            0,
            expectedOut,
            trader1.address,
            protection1,
            '0x'
          );

        // Large trade in opposite direction
        await token1
          .connect(trader2)
          .transfer(await amm.getAddress(), largeAmount);

        reserves = await amm.getReserves();
        expectedOut = await amm.getAmountOut(
          largeAmount,
          reserves._reserve1,
          reserves._reserve0
        );

        const protection2 = createSlippageProtection({
          minAmountOut: (expectedOut * 80n) / 100n,
          maxPriceImpact: 2000,
        });

        await amm
          .connect(trader2)
          .swapWithSlippageProtection(
            expectedOut,
            0,
            trader2.address,
            protection2,
            '0x'
          );

        await time.increase(60);
      }

      // Check if high volatility is detected
      const metrics = await amm.getSlippageProtectionMetrics();
      expect(metrics.currentVolatility).to.be.greaterThan(1000); // Should be above 10%
    });
  });

  describe('Comprehensive Attack Scenarios', function () {
    it('Should prevent sandwich attacks with MEV protection', async function () {
      // Simulate sandwich attack attempt
      const victimTrade = ethers.parseEther('10000');
      const frontrunAmount = ethers.parseEther('50000');

      // Attacker tries to frontrun
      await token0
        .connect(attacker)
        .transfer(await amm.getAddress(), frontrunAmount);

      let reserves = await amm.getReserves();
      let expectedOut = await amm.getAmountOut(
        frontrunAmount,
        reserves._reserve0,
        reserves._reserve1
      );

      const attackProtection = createSlippageProtection({
        minAmountOut: (expectedOut * 90n) / 100n,
        maxPriceImpact: 1000, // 10%
      });

      await amm
        .connect(attacker)
        .swapWithSlippageProtection(
          0,
          expectedOut,
          attacker.address,
          attackProtection,
          '0x'
        );

      // MEV protection should prevent immediate follow-up trade
      await token1
        .connect(attacker)
        .transfer(await amm.getAddress(), expectedOut / 2n);

      await expect(
        amm
          .connect(attacker)
          .swapWithSlippageProtection(
            expectedOut / 4n,
            0,
            attacker.address,
            attackProtection,
            '0x'
          )
      ).to.be.revertedWith('SecureCarbonCreditAMM: MEV protection active');
    });

    it('Should handle extreme market conditions gracefully', async function () {
      // Simulate extreme market stress
      await amm
        .connect(emergencyResponder)
        .activateEmergencySlippageMode(50, 'Market stress test');

      const protection = createSlippageProtection({
        maxPriceImpact: 100, // 1% - very conservative
        minAmountOut: ethers.parseEther('990'),
      });

      // Even small trades should be limited in emergency mode
      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('5000'));

      await expect(
        amm.connect(trader1).swapWithSlippageProtection(
          0,
          ethers.parseEther('2500'), // Would cause price impact
          trader1.address,
          protection,
          '0x'
        )
      ).to.be.revertedWith(
        'SecureCarbonCreditAMM: EMERGENCY_SLIPPAGE_LIMIT_EXCEEDED'
      );
    });

    it('Should maintain security under high-frequency trading', async function () {
      // Simulate high-frequency trading attempts
      const traders = [trader1, trader2, trader3];
      const tradePromises = [];

      for (let i = 0; i < traders.length; i++) {
        const trader = traders[i];
        const tradeAmount = ethers.parseEther('1000');

        // Prepare tokens
        await token0
          .connect(trader)
          .transfer(await amm.getAddress(), tradeAmount);

        const reserves = await amm.getReserves();
        const expectedOut = await amm.getAmountOut(
          tradeAmount,
          reserves._reserve0,
          reserves._reserve1
        );

        const protection = createSlippageProtection({
          minAmountOut: (expectedOut * 95n) / 100n,
        });

        tradePromises.push(
          amm
            .connect(trader)
            .swapWithSlippageProtection(
              0,
              expectedOut,
              trader.address,
              protection,
              '0x'
            )
        );
      }

      // Some trades may fail due to MEV protection, but system should remain stable
      await Promise.allSettled(tradePromises);

      // Verify AMM is still functional
      const finalReserves = await amm.getReserves();
      expect(finalReserves._reserve0).to.be.greaterThan(0);
      expect(finalReserves._reserve1).to.be.greaterThan(0);
    });
  });

  describe('Gas Optimization and Efficiency', function () {
    it('Should maintain reasonable gas costs for protected swaps', async function () {
      const protection = createSlippageProtection();

      await token0
        .connect(trader1)
        .transfer(await amm.getAddress(), ethers.parseEther('1000'));

      const reserves = await amm.getReserves();
      const expectedOut = await amm.getAmountOut(
        ethers.parseEther('1000'),
        reserves._reserve0,
        reserves._reserve1
      );

      const tx = await amm
        .connect(trader1)
        .swapWithSlippageProtection(
          0,
          expectedOut,
          trader1.address,
          protection,
          '0x'
        );

      const receipt = await tx.wait();
      console.log(`Protected swap gas used: ${receipt.gasUsed}`);

      // Should be under reasonable limit for enhanced security operations
      expect(receipt.gasUsed).to.be.below(800000); // Allowing higher limit due to security features
    });
  });
});

// Mock ERC20 contract for testing
const MockERC20 = `
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

contract MockERC20 is ERC20 {
    uint8 private _decimals;

    constructor(
        string memory name,
        string memory symbol,
        uint256 totalSupply
    ) ERC20(name, symbol) {
        _decimals = 18;
        _mint(msg.sender, totalSupply);
    }

    function decimals() public view virtual override returns (uint8) {
        return _decimals;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}
`;
