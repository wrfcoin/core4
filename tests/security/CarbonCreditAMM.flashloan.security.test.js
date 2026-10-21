const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('CarbonCreditAMM Flash Loan Security Tests', function () {
  let carbonCreditAMM;
  let wrfToken;
  let carbonBasketToken;
  let factory;
  let maliciousContract;

  let owner;
  let user1;
  let user2;
  let attacker;

  const INITIAL_SUPPLY = ethers.parseEther('1000000');
  const INITIAL_LIQUIDITY_WRF = ethers.parseEther('10000');
  const INITIAL_LIQUIDITY_BASKET = ethers.parseEther('20000');
  const FLASH_LOAN_AMOUNT = ethers.parseEther('1000');

  beforeEach(async function () {
    [owner, user1, user2, attacker] = await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    wrfToken = await WRFToken.deploy();

    // Deploy Mock Carbon Basket Token (ERC20 for testing)
    const MockERC20 = await ethers.getContractFactory('MockERC20');
    carbonBasketToken = await MockERC20.deploy(
      'Carbon Basket',
      'CB',
      INITIAL_SUPPLY
    );

    // Deploy AMM Factory
    const CarbonCreditAMMFactory = await ethers.getContractFactory(
      'CarbonCreditAMMFactory'
    );
    factory = await CarbonCreditAMMFactory.deploy();

    // Create AMM pool
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

    carbonCreditAMM = await ethers.getContractAt(
      'CarbonCreditAMM',
      poolAddress
    );

    // Distribute tokens
    await wrfToken.transfer(user1.address, INITIAL_SUPPLY / 2n);
    await carbonBasketToken.transfer(user1.address, INITIAL_SUPPLY / 2n);

    await wrfToken.transfer(attacker.address, INITIAL_LIQUIDITY_WRF);
    await carbonBasketToken.transfer(
      attacker.address,
      INITIAL_LIQUIDITY_BASKET
    );

    // Setup approvals
    await wrfToken
      .connect(user1)
      .approve(await carbonCreditAMM.getAddress(), ethers.MaxUint256);
    await carbonBasketToken
      .connect(user1)
      .approve(await carbonCreditAMM.getAddress(), ethers.MaxUint256);

    // Add initial liquidity
    await wrfToken
      .connect(user1)
      .transfer(await carbonCreditAMM.getAddress(), INITIAL_LIQUIDITY_WRF);
    await carbonBasketToken
      .connect(user1)
      .transfer(await carbonCreditAMM.getAddress(), INITIAL_LIQUIDITY_BASKET);
    await carbonCreditAMM.connect(user1).mint(user1.address);

    // Setup flash loan permissions
    await carbonCreditAMM.setAuthorizedFlashBorrower(attacker.address, true);
    await carbonCreditAMM.grantRole(
      await carbonCreditAMM.FACTORY_ROLE(),
      attacker.address
    );
  });

  describe('Flash Loan Reentrancy Protection', function () {
    beforeEach(async function () {
      // Deploy malicious contract that attempts reentrancy
      const MaliciousFlashLoanReceiver = await ethers.getContractFactory(
        'MaliciousFlashLoanReceiver'
      );
      maliciousContract = await MaliciousFlashLoanReceiver.deploy(
        await carbonCreditAMM.getAddress()
      );

      // Give permissions to malicious contract
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await maliciousContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await maliciousContract.getAddress()
      );
    });

    it('Should prevent reentrancy attacks during flash loan execution', async function () {
      // Malicious contract attempts to call flash loan again during callback
      await expect(
        maliciousContract.initiateReentrancyAttack(FLASH_LOAN_AMOUNT, 0)
      ).to.be.revertedWith('ReentrancyGuard: reentrant call');
    });

    it('Should prevent reentrancy through different AMM functions', async function () {
      // Malicious contract attempts to call swap during flash loan callback
      await expect(
        maliciousContract.initiateSwapReentrancyAttack(FLASH_LOAN_AMOUNT, 0)
      ).to.be.revertedWith('ReentrancyGuard: reentrant call');
    });

    it('Should prevent reentrancy through mint function', async function () {
      // Malicious contract attempts to call mint during flash loan callback
      await expect(
        maliciousContract.initiateMintReentrancyAttack(FLASH_LOAN_AMOUNT, 0)
      ).to.be.revertedWith('ReentrancyGuard: reentrant call');
    });

    it('Should prevent reentrancy through burn function', async function () {
      // Malicious contract attempts to call burn during flash loan callback
      await expect(
        maliciousContract.initiateBurnReentrancyAttack(FLASH_LOAN_AMOUNT, 0)
      ).to.be.revertedWith('ReentrancyGuard: reentrant call');
    });
  });

  describe('K-Invariant Protection', function () {
    it('Should validate k-invariant is preserved after flash loan', async function () {
      const [reserve0Before, reserve1Before] =
        await carbonCreditAMM.getReserves();
      const kBefore = reserve0Before * reserve1Before;

      // Deploy legitimate flash loan receiver that repays properly
      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      // Fund the contract for repayment
      await wrfToken.transfer(
        await legitimateContract.getAddress(),
        FLASH_LOAN_AMOUNT * 2n
      );
      await carbonBasketToken.transfer(
        await legitimateContract.getAddress(),
        FLASH_LOAN_AMOUNT * 2n
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await legitimateContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await legitimateContract.getAddress()
      );

      // Execute flash loan
      await carbonCreditAMM
        .connect(attacker)
        .flashLoan(
          FLASH_LOAN_AMOUNT,
          0,
          await legitimateContract.getAddress(),
          '0x'
        );

      const [reserve0After, reserve1After] =
        await carbonCreditAMM.getReserves();
      const kAfter = reserve0After * reserve1After;

      // K-invariant should be preserved or improved (due to fees)
      expect(kAfter).to.be.gte(kBefore);
    });

    it('Should reject flash loans that violate k-invariant', async function () {
      // Deploy contract that attempts to drain the pool
      const KInvariantViolator =
        await ethers.getContractFactory('KInvariantViolator');
      const violatorContract = await KInvariantViolator.deploy(
        await carbonCreditAMM.getAddress()
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await violatorContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await violatorContract.getAddress()
      );

      // Attempt flash loan that violates k-invariant
      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(
            FLASH_LOAN_AMOUNT,
            0,
            await violatorContract.getAddress(),
            '0x'
          )
      ).to.be.revertedWith('CarbonCreditAMM: K_INVARIANT_VIOLATED');
    });

    it('Should reject flash loans that decrease reserves', async function () {
      // Deploy contract that attempts to not repay the full amount
      const InsufficientRepaymentContract = await ethers.getContractFactory(
        'InsufficientRepaymentContract'
      );
      const contractInstance = await InsufficientRepaymentContract.deploy();

      // Fund partially (insufficient repayment)
      await wrfToken.transfer(
        await contractInstance.getAddress(),
        FLASH_LOAN_AMOUNT / 2n
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await contractInstance.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await contractInstance.getAddress()
      );

      // Attempt flash loan with insufficient repayment
      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(
            FLASH_LOAN_AMOUNT,
            0,
            await contractInstance.getAddress(),
            '0x'
          )
      ).to.be.revertedWith('CarbonCreditAMM: RESERVES_DECREASED');
    });
  });

  describe('Flash Loan Fee Calculation', function () {
    it('Should calculate flash loan fees correctly using dedicated fee rate', async function () {
      const expectedFee0 = (FLASH_LOAN_AMOUNT * 30n) / 10000n; // 0.3%
      const expectedFee1 = 0n;

      // Deploy legitimate receiver
      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      // Fund for repayment with exact fee
      await wrfToken.transfer(
        await legitimateContract.getAddress(),
        FLASH_LOAN_AMOUNT + expectedFee0
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await legitimateContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await legitimateContract.getAddress()
      );

      const balanceBefore = await wrfToken.balanceOf(
        await carbonCreditAMM.getAddress()
      );

      // Execute flash loan
      const tx = await carbonCreditAMM
        .connect(attacker)
        .flashLoan(
          FLASH_LOAN_AMOUNT,
          0,
          await legitimateContract.getAddress(),
          '0x'
        );

      const balanceAfter = await wrfToken.balanceOf(
        await carbonCreditAMM.getAddress()
      );
      const feeCollected = balanceAfter - balanceBefore;

      expect(feeCollected).to.equal(expectedFee0);

      // Check event emission
      await expect(tx)
        .to.emit(carbonCreditAMM, 'FlashLoan')
        .withArgs(
          await legitimateContract.getAddress(),
          FLASH_LOAN_AMOUNT,
          0,
          expectedFee0,
          0
        );
    });

    it('Should use FLASH_LOAN_FEE_BASIS_POINTS constant for fee calculation', async function () {
      const flashLoanFeeRate =
        await carbonCreditAMM.FLASH_LOAN_FEE_BASIS_POINTS();
      expect(flashLoanFeeRate).to.equal(30); // 0.3%

      // Verify it's different from general trading fee if needed
      const generalFeeRate = await carbonCreditAMM.feeBasisPoints();
      // Both can be the same value, but we verify the constant exists
      expect(flashLoanFeeRate).to.be.a('bigint');
    });
  });

  describe('Access Control and Validation', function () {
    it('Should reject flash loans from unauthorized borrowers', async function () {
      // Remove authorization
      await carbonCreditAMM.setAuthorizedFlashBorrower(attacker.address, false);

      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(
            FLASH_LOAN_AMOUNT,
            0,
            await legitimateContract.getAddress(),
            '0x'
          )
      ).to.be.revertedWith('CarbonCreditAMM: UNAUTHORIZED_FLASH_BORROWER');
    });

    it('Should reject flash loans with invalid parameters', async function () {
      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      // Zero amounts
      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(0, 0, await legitimateContract.getAddress(), '0x')
      ).to.be.revertedWith('CarbonCreditAMM: INVALID_FLASH_AMOUNT');

      // Invalid receiver
      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(FLASH_LOAN_AMOUNT, 0, ethers.ZeroAddress, '0x')
      ).to.be.revertedWith('CarbonCreditAMM: INVALID_RECEIVER');

      // Self as receiver
      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(
            FLASH_LOAN_AMOUNT,
            0,
            await carbonCreditAMM.getAddress(),
            '0x'
          )
      ).to.be.revertedWith('CarbonCreditAMM: RECEIVER_IS_POOL');
    });

    it('Should reject flash loans exceeding available liquidity', async function () {
      const [reserve0, reserve1] = await carbonCreditAMM.getReserves();
      const excessiveAmount = reserve0 + 1n;

      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(
            excessiveAmount,
            0,
            await legitimateContract.getAddress(),
            '0x'
          )
      ).to.be.revertedWith('CarbonCreditAMM: INSUFFICIENT_LIQUIDITY');
    });
  });

  describe('Gas Optimization and Performance', function () {
    it('Should execute flash loans efficiently without excessive gas usage', async function () {
      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      // Fund for repayment
      const fee = (FLASH_LOAN_AMOUNT * 30n) / 10000n;
      await wrfToken.transfer(
        await legitimateContract.getAddress(),
        FLASH_LOAN_AMOUNT + fee
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await legitimateContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await legitimateContract.getAddress()
      );

      // Measure gas usage
      const tx = await carbonCreditAMM
        .connect(attacker)
        .flashLoan(
          FLASH_LOAN_AMOUNT,
          0,
          await legitimateContract.getAddress(),
          '0x'
        );

      const receipt = await tx.wait();

      // Flash loan should not consume excessive gas (reasonable limit)
      expect(receipt.gasUsed).to.be.lt(200000); // Adjust based on actual measurements
    });

    it('Should handle multiple consecutive flash loans efficiently', async function () {
      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      // Fund for multiple repayments
      const fee = (FLASH_LOAN_AMOUNT * 30n) / 10000n;
      await wrfToken.transfer(
        await legitimateContract.getAddress(),
        (FLASH_LOAN_AMOUNT + fee) * 5n
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await legitimateContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await legitimateContract.getAddress()
      );

      // Execute multiple flash loans
      for (let i = 0; i < 3; i++) {
        await carbonCreditAMM
          .connect(attacker)
          .flashLoan(
            FLASH_LOAN_AMOUNT / 5n,
            0,
            await legitimateContract.getAddress(),
            '0x'
          );
      }

      // Verify pool integrity
      const [reserve0, reserve1] = await carbonCreditAMM.getReserves();
      expect(reserve0).to.be.gt(INITIAL_LIQUIDITY_WRF); // Should have more due to fees
      expect(reserve1).to.equal(INITIAL_LIQUIDITY_BASKET);
    });
  });

  describe('Edge Cases and Stress Testing', function () {
    it('Should handle flash loan of maximum available liquidity', async function () {
      const [reserve0, reserve1] = await carbonCreditAMM.getReserves();

      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      // Fund for maximum repayment
      const fee0 = (reserve0 * 30n) / 10000n;
      const fee1 = (reserve1 * 30n) / 10000n;
      await wrfToken.transfer(
        await legitimateContract.getAddress(),
        reserve0 + fee0
      );
      await carbonBasketToken.transfer(
        await legitimateContract.getAddress(),
        reserve1 + fee1
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await legitimateContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await legitimateContract.getAddress()
      );

      // Execute maximum flash loan
      await carbonCreditAMM
        .connect(attacker)
        .flashLoan(
          reserve0,
          reserve1,
          await legitimateContract.getAddress(),
          '0x'
        );

      // Verify reserves increased due to fees
      const [newReserve0, newReserve1] = await carbonCreditAMM.getReserves();
      expect(newReserve0).to.equal(reserve0 + fee0);
      expect(newReserve1).to.equal(reserve1 + fee1);
    });

    it('Should handle flash loan when pool is paused', async function () {
      // Pause the contract
      await carbonCreditAMM.pause();

      const LegitimateFlashLoanReceiver = await ethers.getContractFactory(
        'LegitimateFlashLoanReceiver'
      );
      const legitimateContract = await LegitimateFlashLoanReceiver.deploy();

      await expect(
        carbonCreditAMM
          .connect(attacker)
          .flashLoan(
            FLASH_LOAN_AMOUNT,
            0,
            await legitimateContract.getAddress(),
            '0x'
          )
      ).to.be.revertedWith('Pausable: paused');
    });

    it('Should handle complex callback scenarios', async function () {
      // Deploy contract that performs complex operations during callback
      const ComplexFlashLoanReceiver = await ethers.getContractFactory(
        'ComplexFlashLoanReceiver'
      );
      const complexContract = await ComplexFlashLoanReceiver.deploy();

      // Fund for repayment
      const fee = (FLASH_LOAN_AMOUNT * 30n) / 10000n;
      await wrfToken.transfer(
        await complexContract.getAddress(),
        FLASH_LOAN_AMOUNT + fee
      );

      // Setup permissions
      await carbonCreditAMM.setAuthorizedFlashBorrower(
        await complexContract.getAddress(),
        true
      );
      await carbonCreditAMM.grantRole(
        await carbonCreditAMM.FACTORY_ROLE(),
        await complexContract.getAddress()
      );

      // Execute flash loan with complex callback
      await carbonCreditAMM
        .connect(attacker)
        .flashLoan(
          FLASH_LOAN_AMOUNT,
          0,
          await complexContract.getAddress(),
          ethers.toUtf8Bytes('complex_operation_data')
        );

      // Verify successful execution
      const [reserve0, reserve1] = await carbonCreditAMM.getReserves();
      expect(reserve0).to.be.gt(INITIAL_LIQUIDITY_WRF);
    });
  });
});
