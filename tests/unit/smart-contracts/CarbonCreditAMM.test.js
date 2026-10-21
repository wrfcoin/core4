const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('CarbonCreditAMM', function () {
  let ammPool;
  let factory;
  let token0;
  let token1;
  let wrfToken;
  let carbonBasketToken;
  let owner;
  let user1;
  let user2;
  let feeReceiver;

  const INITIAL_SUPPLY = ethers.parseEther('10000');
  const LIQUIDITY_AMOUNT_0 = ethers.parseEther('1000');
  const LIQUIDITY_AMOUNT_1 = ethers.parseEther('2000');
  const FEE_BASIS_POINTS = 30; // 0.3%

  beforeEach(async function () {
    [owner, user1, user2, feeReceiver] = await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    wrfToken = await WRFToken.deploy();

    // Deploy a mock ERC20 for testing (using WRF as second token)
    const MockERC20 = await ethers.getContractFactory('WRFToken');
    const mockToken = await MockERC20.deploy();

    // Ensure consistent token ordering (token0 < token1)
    if ((await wrfToken.getAddress()) < (await mockToken.getAddress())) {
      token0 = wrfToken;
      token1 = mockToken;
    } else {
      token0 = mockToken;
      token1 = wrfToken;
    }

    // Deploy Factory
    const CarbonCreditAMMFactory = await ethers.getContractFactory(
      'CarbonCreditAMMFactory'
    );
    factory = await CarbonCreditAMMFactory.deploy();

    // Create AMM Pool
    await factory.createPool(
      await token0.getAddress(),
      await token1.getAddress(),
      FEE_BASIS_POINTS
    );

    const poolAddress = await factory.getPool(
      await token0.getAddress(),
      await token1.getAddress(),
      FEE_BASIS_POINTS
    );

    ammPool = await ethers.getContractAt('CarbonCreditAMM', poolAddress);

    // Transfer tokens to users
    await token0.transfer(user1.address, INITIAL_SUPPLY);
    await token1.transfer(user1.address, INITIAL_SUPPLY);
    await token0.transfer(user2.address, INITIAL_SUPPLY);
    await token1.transfer(user2.address, INITIAL_SUPPLY);

    // Approve AMM pool
    await token0.connect(user1).approve(poolAddress, ethers.MaxUint256);
    await token1.connect(user1).approve(poolAddress, ethers.MaxUint256);
    await token0.connect(user2).approve(poolAddress, ethers.MaxUint256);
    await token1.connect(user2).approve(poolAddress, ethers.MaxUint256);
  });

  describe('Deployment and Initialization', function () {
    it('Should initialize with correct parameters', async function () {
      expect(await ammPool.token0()).to.equal(await token0.getAddress());
      expect(await ammPool.token1()).to.equal(await token1.getAddress());
      expect(await ammPool.feeBasisPoints()).to.equal(FEE_BASIS_POINTS);
      expect(await ammPool.factory()).to.equal(await factory.getAddress());
    });

    it('Should start with zero reserves', async function () {
      const [reserve0, reserve1] = await ammPool.getReserves();
      expect(reserve0).to.equal(0);
      expect(reserve1).to.equal(0);
    });

    it('Should have correct LP token name and symbol', async function () {
      expect(await ammPool.name()).to.equal('Carbon Credit LP');
      expect(await ammPool.symbol()).to.equal('CC-LP');
    });
  });

  describe('Liquidity Provision', function () {
    it('Should mint initial liquidity correctly', async function () {
      // Transfer tokens to pool
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);

      const tx = await ammPool.mint(user1.address);

      await expect(tx)
        .to.emit(ammPool, 'Mint')
        .withArgs(user1.address, LIQUIDITY_AMOUNT_0, LIQUIDITY_AMOUNT_1);

      const expectedLiquidity =
        Math.sqrt(Number(LIQUIDITY_AMOUNT_0) * Number(LIQUIDITY_AMOUNT_1)) -
        1000;
      expect(await ammPool.balanceOf(user1.address)).to.be.closeTo(
        ethers.parseUnits(expectedLiquidity.toString(), 0),
        ethers.parseUnits('1000', 0)
      );

      const [reserve0, reserve1] = await ammPool.getReserves();
      expect(reserve0).to.equal(LIQUIDITY_AMOUNT_0);
      expect(reserve1).to.equal(LIQUIDITY_AMOUNT_1);
    });

    it('Should mint proportional liquidity for subsequent adds', async function () {
      // First liquidity provision
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);

      const firstLiquidity = await ammPool.balanceOf(user1.address);

      // Second liquidity provision (double the amount)
      await token0
        .connect(user2)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user2)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user2.address);

      const secondLiquidity = await ammPool.balanceOf(user2.address);
      expect(secondLiquidity).to.be.closeTo(
        firstLiquidity,
        ethers.parseUnits('1000', 0)
      );
    });

    it('Should prevent minting with zero liquidity', async function () {
      await expect(ammPool.mint(user1.address)).to.be.revertedWith(
        'CarbonCreditAMM: INSUFFICIENT_LIQUIDITY_MINTED'
      );
    });
  });

  describe('Liquidity Removal', function () {
    beforeEach(async function () {
      // Add initial liquidity
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);
    });

    it('Should burn liquidity and return tokens', async function () {
      const liquidity = await ammPool.balanceOf(user1.address);

      // Transfer LP tokens to pool for burning
      await ammPool
        .connect(user1)
        .transfer(await ammPool.getAddress(), liquidity);

      const tx = await ammPool.burn(user1.address);

      await expect(tx).to.emit(ammPool, 'Burn');

      // Check that tokens were returned (minus fees)
      expect(await token0.balanceOf(user1.address)).to.be.gt(
        INITIAL_SUPPLY - LIQUIDITY_AMOUNT_0
      );
      expect(await token1.balanceOf(user1.address)).to.be.gt(
        INITIAL_SUPPLY - LIQUIDITY_AMOUNT_1
      );
    });

    it('Should prevent burning with insufficient liquidity', async function () {
      await expect(ammPool.burn(user1.address)).to.be.revertedWith(
        'CarbonCreditAMM: INSUFFICIENT_LIQUIDITY_BURNED'
      );
    });
  });

  describe('Token Swapping', function () {
    const swapAmount = ethers.parseEther('100');

    beforeEach(async function () {
      // Add initial liquidity
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);
    });

    it('Should swap token0 for token1', async function () {
      const [reserve0Before, reserve1Before] = await ammPool.getReserves();

      // Calculate expected output
      const expectedOutput = await ammPool.getAmountOut(
        swapAmount,
        reserve0Before,
        reserve1Before
      );

      // Transfer tokens to pool
      await token0
        .connect(user2)
        .transfer(await ammPool.getAddress(), swapAmount);

      const tx = await ammPool.swap(0, expectedOutput, user2.address, '0x');

      await expect(tx)
        .to.emit(ammPool, 'Swap')
        .withArgs(
          user2.address,
          swapAmount,
          0,
          0,
          expectedOutput,
          user2.address
        );

      expect(await token1.balanceOf(user2.address)).to.equal(
        INITIAL_SUPPLY + expectedOutput
      );
    });

    it('Should swap token1 for token0', async function () {
      const [reserve0Before, reserve1Before] = await ammPool.getReserves();

      // Calculate expected output
      const expectedOutput = await ammPool.getAmountOut(
        swapAmount,
        reserve1Before,
        reserve0Before
      );

      // Transfer tokens to pool
      await token1
        .connect(user2)
        .transfer(await ammPool.getAddress(), swapAmount);

      const tx = await ammPool.swap(expectedOutput, 0, user2.address, '0x');

      await expect(tx)
        .to.emit(ammPool, 'Swap')
        .withArgs(
          user2.address,
          0,
          swapAmount,
          expectedOutput,
          0,
          user2.address
        );

      expect(await token0.balanceOf(user2.address)).to.equal(
        INITIAL_SUPPLY + expectedOutput
      );
    });

    it('Should maintain constant product invariant', async function () {
      const [reserve0Before, reserve1Before] = await ammPool.getReserves();
      const kBefore = reserve0Before * reserve1Before;

      // Perform swap
      await token0
        .connect(user2)
        .transfer(await ammPool.getAddress(), swapAmount);
      const expectedOutput = await ammPool.getAmountOut(
        swapAmount,
        reserve0Before,
        reserve1Before
      );
      await ammPool.swap(0, expectedOutput, user2.address, '0x');

      const [reserve0After, reserve1After] = await ammPool.getReserves();
      const kAfter = reserve0After * reserve1After;

      // K should increase due to fees
      expect(kAfter).to.be.gte(kBefore);
    });

    it('Should calculate swap amounts correctly', async function () {
      const [reserve0, reserve1] = await ammPool.getReserves();

      const amountOut = await ammPool.getAmountOut(
        swapAmount,
        reserve0,
        reserve1
      );
      expect(amountOut).to.be.gt(0);

      const amountIn = await ammPool.getAmountIn(amountOut, reserve0, reserve1);
      expect(amountIn).to.be.closeTo(swapAmount, ethers.parseEther('1'));
    });

    it('Should prevent swaps with insufficient output', async function () {
      await expect(ammPool.swap(0, 0, user2.address, '0x')).to.be.revertedWith(
        'CarbonCreditAMM: INSUFFICIENT_OUTPUT_AMOUNT'
      );
    });

    it('Should prevent swaps exceeding reserves', async function () {
      const [reserve0, reserve1] = await ammPool.getReserves();

      await expect(
        ammPool.swap(reserve0, 0, user2.address, '0x')
      ).to.be.revertedWith('CarbonCreditAMM: INSUFFICIENT_LIQUIDITY');
    });
  });

  describe('Flash Loans', function () {
    const flashAmount = ethers.parseEther('100');

    beforeEach(async function () {
      // Add initial liquidity
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);

      // Deploy a simple flash loan receiver for testing
      const FlashLoanReceiver = await ethers.getContractFactory(
        'MockFlashLoanReceiver'
      );
      this.flashReceiver = await FlashLoanReceiver.deploy();

      // Authorize flash borrower
      await ammPool.setAuthorizedFlashBorrower(
        await this.flashReceiver.getAddress(),
        true
      );
    });

    it('Should execute flash loan correctly', async function () {
      // Fund the receiver with tokens to pay back the loan + fee
      const fee = (flashAmount * BigInt(FEE_BASIS_POINTS)) / 10000n;
      await token0.transfer(await this.flashReceiver.getAddress(), fee);

      const tx = await this.flashReceiver.executeFlashLoan(
        await ammPool.getAddress(),
        flashAmount,
        0,
        '0x'
      );

      await expect(tx).to.emit(ammPool, 'FlashLoan');
    });

    it('Should prevent unauthorized flash loans', async function () {
      await expect(
        ammPool.connect(user2).flashLoan(flashAmount, 0, user2.address, '0x')
      ).to.be.revertedWith('CarbonCreditAMM: UNAUTHORIZED_FLASH_BORROWER');
    });

    it('Should require fee payment for flash loans', async function () {
      // Don't fund the receiver with enough tokens
      await expect(
        this.flashReceiver.executeFlashLoan(
          await ammPool.getAddress(),
          flashAmount,
          0,
          '0x'
        )
      ).to.be.revertedWith('CarbonCreditAMM: INSUFFICIENT_FLASH_REPAYMENT_0');
    });
  });

  describe('Fee Management', function () {
    it('Should allow authorized users to set fees', async function () {
      const FEE_SETTER_ROLE = await ammPool.FEE_SETTER_ROLE();
      await ammPool.grantRole(FEE_SETTER_ROLE, owner.address);

      await ammPool.setFees(50, 10); // 0.5% trading fee, 0.1% protocol fee

      expect(await ammPool.feeBasisPoints()).to.equal(50);
      expect(await ammPool.protocolFeeBasisPoints()).to.equal(10);
    });

    it('Should prevent setting excessive fees', async function () {
      const FEE_SETTER_ROLE = await ammPool.FEE_SETTER_ROLE();
      await ammPool.grantRole(FEE_SETTER_ROLE, owner.address);

      await expect(ammPool.setFees(1001, 10)) // > 10%
        .to.be.revertedWith('CarbonCreditAMM: FEE_TOO_HIGH');

      await expect(ammPool.setFees(50, 201)) // > 2%
        .to.be.revertedWith('CarbonCreditAMM: PROTOCOL_FEE_TOO_HIGH');
    });

    it('Should allow setting fee recipient', async function () {
      await ammPool.setFeeTo(feeReceiver.address);
      expect(await ammPool.feeTo()).to.equal(feeReceiver.address);
    });
  });

  describe('Price Oracle', function () {
    beforeEach(async function () {
      // Add initial liquidity
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);
    });

    it('Should track cumulative prices', async function () {
      const priceBefore = await ammPool.price0CumulativeLast();

      // Advance time and mine block
      await ethers.provider.send('evm_increaseTime', [3600]); // 1 hour
      await ethers.provider.send('evm_mine');

      // Make a small trade to update oracle
      const swapAmount = ethers.parseEther('10');
      await token0
        .connect(user2)
        .transfer(await ammPool.getAddress(), swapAmount);
      const [reserve0, reserve1] = await ammPool.getReserves();
      const expectedOutput = await ammPool.getAmountOut(
        swapAmount,
        reserve0,
        reserve1
      );
      await ammPool.swap(0, expectedOutput, user2.address, '0x');

      const priceAfter = await ammPool.price0CumulativeLast();
      expect(priceAfter).to.be.gt(priceBefore);
    });
  });

  describe('Emergency Functions', function () {
    beforeEach(async function () {
      // Add initial liquidity
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);
    });

    it('Should allow admin to pause/unpause', async function () {
      await ammPool.pause();

      await expect(ammPool.mint(user1.address)).to.be.revertedWithCustomError(
        ammPool,
        'EnforcedPause'
      );

      await ammPool.unpause();

      // Should work after unpause
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), ethers.parseEther('100'));
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), ethers.parseEther('200'));
      await ammPool.mint(user1.address);
    });

    it('Should allow skim function to recover excess tokens', async function () {
      // Send extra tokens directly to pool
      const extraAmount = ethers.parseEther('100');
      await token0.transfer(await ammPool.getAddress(), extraAmount);

      await ammPool.skim(user2.address);

      expect(await token0.balanceOf(user2.address)).to.equal(
        INITIAL_SUPPLY + extraAmount
      );
    });

    it('Should allow sync function to update reserves', async function () {
      // Send tokens directly to pool (bypassing normal flows)
      await token0.transfer(
        await ammPool.getAddress(),
        ethers.parseEther('100')
      );

      await ammPool.sync();

      const [reserve0, reserve1] = await ammPool.getReserves();
      expect(reserve0).to.equal(LIQUIDITY_AMOUNT_0 + ethers.parseEther('100'));
    });
  });

  describe('Edge Cases and Security', function () {
    it('Should handle minimum liquidity lock correctly', async function () {
      // First liquidity provision should lock 1000 tokens
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);

      // Check that minimum liquidity is locked
      expect(await ammPool.balanceOf(ethers.ZeroAddress)).to.equal(1000);
    });

    it('Should prevent K manipulation', async function () {
      // Add initial liquidity
      await token0
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_0);
      await token1
        .connect(user1)
        .transfer(await ammPool.getAddress(), LIQUIDITY_AMOUNT_1);
      await ammPool.mint(user1.address);

      const [reserve0, reserve1] = await ammPool.getReserves();
      const swapAmount = ethers.parseEther('100');

      // Transfer tokens but try to extract more than formula allows
      await token0
        .connect(user2)
        .transfer(await ammPool.getAddress(), swapAmount);

      const expectedOutput = await ammPool.getAmountOut(
        swapAmount,
        reserve0,
        reserve1
      );

      // Try to extract more than allowed
      await expect(
        ammPool.swap(0, expectedOutput + 1n, user2.address, '0x')
      ).to.be.revertedWith('CarbonCreditAMM: K');
    });

    it('Should handle zero reserve edge case', async function () {
      await expect(
        ammPool.getAmountOut(
          ethers.parseEther('100'),
          0,
          ethers.parseEther('1000')
        )
      ).to.be.revertedWith('CarbonCreditAMM: INSUFFICIENT_LIQUIDITY');
    });

    it('Should prevent overflow in calculations', async function () {
      // This test ensures our calculations don't overflow
      const largeAmount = ethers.parseEther('1000000000');

      await expect(
        ammPool.getAmountOut(
          largeAmount,
          ethers.parseEther('1'),
          ethers.parseEther('1')
        )
      ).to.not.be.reverted;
    });
  });
});

// Mock Flash Loan Receiver for testing
const mockFlashLoanReceiverCode = `
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../contracts/CarbonCreditAMM.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

contract MockFlashLoanReceiver {
    function executeFlashLoan(
        address pool,
        uint256 amount0,
        uint256 amount1,
        bytes calldata data
    ) external {
        CarbonCreditAMM(pool).flashLoan(amount0, amount1, address(this), data);
    }

    function executeOperation(bytes calldata data) external pure {
        // Simple implementation that does nothing
    }

    function executeFlashLoan(
        uint256 amount0,
        uint256 amount1,
        uint256 fee0,
        uint256 fee1,
        bytes calldata data
    ) external {
        // Repay the loan with fees
        if (amount0 > 0) {
            address token0 = CarbonCreditAMM(msg.sender).token0();
            IERC20(token0).transfer(msg.sender, amount0 + fee0);
        }
        if (amount1 > 0) {
            address token1 = CarbonCreditAMM(msg.sender).token1();
            IERC20(token1).transfer(msg.sender, amount1 + fee1);
        }
    }
}
`;
