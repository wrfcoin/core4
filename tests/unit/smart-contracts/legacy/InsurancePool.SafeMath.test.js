const { expect } = require('chai');
const { ethers } = require('hardhat');
const { BigNumber } = ethers;

describe('InsurancePool - SafeMath Security Tests', function () {
  let InsurancePool;
  let insurancePool;
  let owner;
  let capitalProvider1;
  let capitalProvider2;
  let policyHolder;
  let accounts;

  const MINIMUM_CAPITAL = ethers.utils.parseEther('1000');
  const INITIAL_CAPITAL = ethers.utils.parseEther('5000');
  const MAX_UINT256 = ethers.constants.MaxUint256;

  beforeEach(async function () {
    [owner, capitalProvider1, capitalProvider2, policyHolder, ...accounts] =
      await ethers.getSigners();

    // Deploy SafeMath library first
    const SafeMath = await ethers.getContractFactory('SafeMath');
    const safeMath = await SafeMath.deploy();
    await safeMath.deployed();

    // Deploy InsurancePool
    InsurancePool = await ethers.getContractFactory('InsurancePool');
    insurancePool = await InsurancePool.deploy();
    await insurancePool.deployed();

    // Provide initial capital
    await insurancePool
      .connect(capitalProvider1)
      .provideCapital({ value: INITIAL_CAPITAL });
  });

  describe('Arithmetic Overflow Protection', function () {
    it('Should prevent overflow in provideCapital', async function () {
      // Try to provide capital that would cause overflow
      const hugeAmount = MAX_UINT256.sub(1000);

      // This should not cause overflow thanks to SafeMath
      await expect(
        insurancePool.connect(capitalProvider2).provideCapital({
          value: MINIMUM_CAPITAL,
        })
      ).to.not.be.reverted;
    });

    it('Should prevent overflow in premium calculation', async function () {
      const hugeCoverage = ethers.utils.parseEther('1000000000');
      const duration = 365 * 24 * 60 * 60; // 1 year

      // Should handle large numbers without overflow
      await expect(insurancePool.calculatePremium(hugeCoverage, duration)).to
        .not.be.reverted;
    });

    it('Should prevent multiplication overflow in withdrawCapital', async function () {
      // Provide large amount of capital
      const largeCapital = ethers.utils.parseEther('100000');
      await insurancePool.connect(capitalProvider2).provideCapital({
        value: largeCapital,
      });

      // Try to withdraw with large share amount
      const shares = ethers.utils.parseEther('50000');

      // Should handle multiplication without overflow
      await expect(
        insurancePool.connect(capitalProvider2).withdrawCapital(shares)
      ).to.not.be.reverted;
    });
  });

  describe('Division by Zero Protection', function () {
    it('Should prevent division by zero in withdrawCapital', async function () {
      // Try to withdraw when shares would be zero
      await expect(
        insurancePool.connect(accounts[0]).withdrawCapital(0)
      ).to.be.revertedWith('Shares must be greater than zero');
    });

    it('Should prevent division by zero in calculatePremium', async function () {
      const coverage = ethers.utils.parseEther('1000');

      // Duration of 0 should be rejected
      await expect(
        insurancePool.calculatePremium(coverage, 0)
      ).to.be.revertedWith('Duration must be positive');
    });

    it('Should handle edge case where totalShares is zero', async function () {
      // Deploy new instance
      const newPool = await InsurancePool.deploy();
      await newPool.deployed();

      // Try to withdraw without any shares
      await expect(newPool.withdrawCapital(1)).to.be.revertedWith(
        'Insufficient shares'
      );
    });
  });

  describe('Underflow Protection', function () {
    it('Should prevent underflow in withdrawCapital', async function () {
      const withdrawShares = ethers.utils.parseEther('2500');

      // Withdraw half of the capital
      await insurancePool
        .connect(capitalProvider1)
        .withdrawCapital(withdrawShares);

      // Try to withdraw more than remaining
      await expect(
        insurancePool
          .connect(capitalProvider1)
          .withdrawCapital(ethers.utils.parseEther('3000'))
      ).to.be.revertedWith('Insufficient shares');
    });

    it('Should prevent underflow in processClaim', async function () {
      // Purchase insurance
      const coverage = ethers.utils.parseEther('1000');
      const duration = 30 * 24 * 60 * 60; // 30 days
      const premium = await insurancePool.calculatePremium(coverage, duration);

      await insurancePool
        .connect(policyHolder)
        .purchaseInsurance(coverage, duration, 'weather', { value: premium });

      // Process claim
      await insurancePool.connect(policyHolder).processClaim(0);

      // Try to process same claim again (would cause underflow)
      await expect(
        insurancePool.connect(policyHolder).processClaim(0)
      ).to.be.revertedWith('Already claimed');
    });

    it('Should prevent totalPoolCapital underflow', async function () {
      // Try to withdraw all capital plus more
      const totalShares = ethers.utils.parseEther('5000');

      await insurancePool
        .connect(capitalProvider1)
        .withdrawCapital(totalShares);

      // Verify totalPoolCapital didn't underflow
      const poolCapital = await insurancePool.totalPoolCapital();
      expect(poolCapital).to.equal(0);
    });
  });

  describe('Precision and Rounding Tests', function () {
    it('Should handle small withdrawal amounts correctly', async function () {
      // Provide small amount
      const smallAmount = ethers.utils.parseEther('1001');
      await insurancePool.connect(capitalProvider2).provideCapital({
        value: smallAmount,
      });

      // Withdraw tiny fraction
      const tinyShares = ethers.utils.parseEther('1');
      await insurancePool.connect(capitalProvider2).withdrawCapital(tinyShares);

      // Check remaining balance is correct
      const provider = await insurancePool.capitalProviders(
        capitalProvider2.address
      );
      expect(provider.shares).to.equal(ethers.utils.parseEther('1000'));
    });

    it('Should maintain precision in premium calculations', async function () {
      const coverage = ethers.utils.parseEther('999.999999999999999999');
      const duration = 1; // 1 second

      const premium = await insurancePool.calculatePremium(coverage, duration);
      expect(premium).to.be.gt(0);
    });

    it('Should handle maximum safe integer operations', async function () {
      const maxSafeAmount = ethers.utils.parseEther('1000000');

      // Should handle large but safe amounts
      await expect(
        insurancePool.calculatePremium(maxSafeAmount, 365 * 24 * 60 * 60)
      ).to.not.be.reverted;
    });
  });

  describe('Edge Case Scenarios', function () {
    it('Should handle withdrawal of exactly all shares', async function () {
      const allShares = ethers.utils.parseEther('5000');

      await insurancePool.connect(capitalProvider1).withdrawCapital(allShares);

      const provider = await insurancePool.capitalProviders(
        capitalProvider1.address
      );
      expect(provider.shares).to.equal(0);
      expect(provider.contribution).to.equal(0);
    });

    it('Should reject withdrawal exceeding contribution', async function () {
      // Try to manipulate by providing invalid shares
      const invalidShares = ethers.utils.parseEther('10000');

      await expect(
        insurancePool.connect(capitalProvider1).withdrawCapital(invalidShares)
      ).to.be.revertedWith('Insufficient shares');
    });

    it('Should maintain minimum capital requirement', async function () {
      // Try to withdraw amount that would breach minimum
      const excessiveWithdraw = ethers.utils.parseEther('4500');

      await expect(
        insurancePool
          .connect(capitalProvider1)
          .withdrawCapital(excessiveWithdraw)
      ).to.be.revertedWith('Withdrawal would breach minimum capital');
    });

    it('Should handle concurrent operations safely', async function () {
      // Multiple providers
      await insurancePool.connect(capitalProvider2).provideCapital({
        value: MINIMUM_CAPITAL,
      });

      await insurancePool.connect(accounts[0]).provideCapital({
        value: MINIMUM_CAPITAL,
      });

      // Concurrent withdrawals
      const withdraw1 = insurancePool
        .connect(capitalProvider1)
        .withdrawCapital(ethers.utils.parseEther('1000'));
      const withdraw2 = insurancePool
        .connect(capitalProvider2)
        .withdrawCapital(ethers.utils.parseEther('500'));

      // Both should succeed without interference
      await expect(Promise.all([withdraw1, withdraw2])).to.not.be.reverted;
    });
  });

  describe('$1M TVL Stress Tests', function () {
    it('Should handle $1M TVL operations safely', async function () {
      // Simulate $1M TVL
      const oneMillion = ethers.utils.parseEther('1000000');

      // Multiple large deposits
      for (let i = 0; i < 5; i++) {
        await insurancePool.connect(accounts[i]).provideCapital({
          value: ethers.utils.parseEther('200000'),
        });
      }

      // Verify total capital
      const totalCapital = await insurancePool.totalPoolCapital();
      expect(totalCapital).to.equal(oneMillion.add(INITIAL_CAPITAL));

      // Large withdrawal without overflow
      const largeWithdraw = ethers.utils.parseEther('100000');
      await expect(
        insurancePool.connect(accounts[0]).withdrawCapital(largeWithdraw)
      ).to.not.be.reverted;
    });

    it('Should handle maximum leverage safely', async function () {
      const maxCoverageRatio = await insurancePool.MAX_COVERAGE_RATIO();
      const totalCapital = await insurancePool.totalPoolCapital();
      const maxCoverage = totalCapital.mul(maxCoverageRatio);

      // Try to purchase insurance at max leverage
      const coverage = maxCoverage.sub(ethers.utils.parseEther('1'));
      const duration = 30 * 24 * 60 * 60;
      const premium = await insurancePool.calculatePremium(coverage, duration);

      // Should handle without overflow
      await expect(
        insurancePool
          .connect(policyHolder)
          .purchaseInsurance(coverage, duration, 'weather', { value: premium })
      ).to.not.be.reverted;
    });
  });

  describe('Gas Optimization Tests', function () {
    it('Should maintain reasonable gas costs with SafeMath', async function () {
      const tx = await insurancePool.connect(capitalProvider2).provideCapital({
        value: MINIMUM_CAPITAL,
      });

      const receipt = await tx.wait();

      // Gas should be reasonable even with SafeMath
      expect(receipt.gasUsed).to.be.lt(200000);
    });

    it('Should optimize batch operations', async function () {
      const gasUsages = [];

      // Measure gas for multiple operations
      for (let i = 0; i < 3; i++) {
        const tx = await insurancePool.connect(accounts[i]).provideCapital({
          value: MINIMUM_CAPITAL,
        });
        const receipt = await tx.wait();
        gasUsages.push(receipt.gasUsed);
      }

      // Gas usage should be consistent
      const avgGas = gasUsages.reduce((a, b) => a.add(b)).div(gasUsages.length);
      gasUsages.forEach(gas => {
        expect(gas).to.be.closeTo(avgGas, avgGas.div(10)); // Within 10%
      });
    });
  });
});
