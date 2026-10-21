const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('CarbonCreditAMMFactory', function () {
  let factory;
  let tokenA;
  let tokenB;
  let owner;
  let poolCreator;
  let feeSetter;
  let user1;

  beforeEach(async function () {
    [owner, poolCreator, feeSetter, user1] = await ethers.getSigners();

    // Deploy test tokens
    const ERC20Mock = await ethers.getContractFactory('WRFToken');
    tokenA = await ERC20Mock.deploy();
    tokenB = await ERC20Mock.deploy();

    // Ensure tokenA < tokenB for consistent ordering
    if ((await tokenA.getAddress()) > (await tokenB.getAddress())) {
      [tokenA, tokenB] = [tokenB, tokenA];
    }

    // Deploy factory
    const CarbonCreditAMMFactory = await ethers.getContractFactory(
      'CarbonCreditAMMFactory'
    );
    factory = await CarbonCreditAMMFactory.deploy();

    // Setup roles
    const POOL_CREATOR_ROLE = await factory.POOL_CREATOR_ROLE();
    const FEE_SETTER_ROLE = await factory.FEE_SETTER_ROLE();

    await factory.grantRole(POOL_CREATOR_ROLE, poolCreator.address);
    await factory.grantRole(FEE_SETTER_ROLE, feeSetter.address);
  });

  describe('Deployment', function () {
    it('Should deploy with correct initial state', async function () {
      expect(await factory.governance()).to.equal(owner.address);
      expect(await factory.feeTo()).to.equal(ethers.ZeroAddress);
      expect(await factory.allPoolsLength()).to.equal(0);
    });

    it('Should initialize default fee tiers', async function () {
      const feeTiers = await factory.getSupportedFeeTiers();
      expect(feeTiers.length).to.equal(4);

      // Check default fee tiers: 30, 50, 100, 300 basis points
      expect(feeTiers).to.include(30n);
      expect(feeTiers).to.include(50n);
      expect(feeTiers).to.include(100n);
      expect(feeTiers).to.include(300n);
    });

    it('Should setup roles correctly', async function () {
      const DEFAULT_ADMIN_ROLE = await factory.DEFAULT_ADMIN_ROLE();
      const POOL_CREATOR_ROLE = await factory.POOL_CREATOR_ROLE();
      const FEE_SETTER_ROLE = await factory.FEE_SETTER_ROLE();

      expect(await factory.hasRole(DEFAULT_ADMIN_ROLE, owner.address)).to.be
        .true;
      expect(await factory.hasRole(POOL_CREATOR_ROLE, poolCreator.address)).to
        .be.true;
      expect(await factory.hasRole(FEE_SETTER_ROLE, feeSetter.address)).to.be
        .true;
    });
  });

  describe('Pool Creation', function () {
    const feeTier = 30; // 0.3%

    it('Should create new pool successfully', async function () {
      const tx = await factory
        .connect(poolCreator)
        .createPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          feeTier
        );

      await expect(tx)
        .to.emit(factory, 'PoolCreated')
        .withArgs(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          feeTier,
          await factory.getPool(
            await tokenA.getAddress(),
            await tokenB.getAddress(),
            feeTier
          ),
          1
        );

      expect(await factory.allPoolsLength()).to.equal(1);
    });

    it('Should create pool with correct token ordering', async function () {
      await factory.connect(poolCreator).createPool(
        await tokenB.getAddress(), // Pass in reverse order
        await tokenA.getAddress(),
        feeTier
      );

      const poolAddress = await factory.getPool(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        feeTier
      );

      expect(poolAddress).to.not.equal(ethers.ZeroAddress);

      // Check reverse order also works
      const poolAddressReverse = await factory.getPool(
        await tokenB.getAddress(),
        await tokenA.getAddress(),
        feeTier
      );

      expect(poolAddress).to.equal(poolAddressReverse);
    });

    it('Should prevent unauthorized pool creation', async function () {
      await expect(
        factory
          .connect(user1)
          .createPool(
            await tokenA.getAddress(),
            await tokenB.getAddress(),
            feeTier
          )
      ).to.be.revertedWith('CarbonCreditAMMFactory: FORBIDDEN');
    });

    it('Should prevent creating duplicate pools', async function () {
      await factory
        .connect(poolCreator)
        .createPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          feeTier
        );

      await expect(
        factory
          .connect(poolCreator)
          .createPool(
            await tokenA.getAddress(),
            await tokenB.getAddress(),
            feeTier
          )
      ).to.be.revertedWith('CarbonCreditAMMFactory: POOL_EXISTS');
    });

    it('Should prevent creating pools with invalid fee tiers', async function () {
      await expect(
        factory.connect(poolCreator).createPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          999 // Non-existent fee tier
        )
      ).to.be.revertedWith('CarbonCreditAMMFactory: INVALID_FEE_TIER');
    });

    it('Should prevent creating pools with identical tokens', async function () {
      await expect(
        factory
          .connect(poolCreator)
          .createPool(
            await tokenA.getAddress(),
            await tokenA.getAddress(),
            feeTier
          )
      ).to.be.revertedWith('CarbonCreditAMMFactory: IDENTICAL_ADDRESSES');
    });

    it('Should prevent creating pools with zero address', async function () {
      await expect(
        factory
          .connect(poolCreator)
          .createPool(ethers.ZeroAddress, await tokenB.getAddress(), feeTier)
      ).to.be.revertedWith('CarbonCreditAMMFactory: ZERO_ADDRESS');
    });
  });

  describe('Pool Address Prediction', function () {
    const feeTier = 30;

    it('Should predict pool address correctly', async function () {
      const predictedAddress = await factory.predictPoolAddress(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        feeTier
      );

      await factory
        .connect(poolCreator)
        .createPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          feeTier
        );

      const actualAddress = await factory.getPool(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        feeTier
      );

      expect(predictedAddress).to.equal(actualAddress);
    });

    it('Should predict same address regardless of token order', async function () {
      const predicted1 = await factory.predictPoolAddress(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        feeTier
      );

      const predicted2 = await factory.predictPoolAddress(
        await tokenB.getAddress(),
        await tokenA.getAddress(),
        feeTier
      );

      expect(predicted1).to.equal(predicted2);
    });
  });

  describe('Fee Tier Management', function () {
    it('Should add new fee tier', async function () {
      const newFeeTier = 200; // 2%
      const tickSpacing = 400;

      await factory.connect(feeSetter).addFeeTier(newFeeTier, tickSpacing);

      const feeTiers = await factory.getSupportedFeeTiers();
      expect(feeTiers).to.include(BigInt(newFeeTier));

      const feeInfo = await factory.feeTiers(newFeeTier);
      expect(feeInfo.feeBasisPoints).to.equal(newFeeTier);
      expect(feeInfo.tickSpacing).to.equal(tickSpacing);
      expect(feeInfo.isActive).to.be.true;
    });

    it('Should prevent adding excessive fee tier', async function () {
      await expect(
        factory.connect(feeSetter).addFeeTier(1001, 100) // > 10%
      ).to.be.revertedWith('CarbonCreditAMMFactory: FEE_TOO_HIGH');
    });

    it('Should prevent adding duplicate fee tier', async function () {
      await expect(
        factory.connect(feeSetter).addFeeTier(30, 100) // Already exists
      ).to.be.revertedWith('CarbonCreditAMMFactory: FEE_TIER_EXISTS');
    });

    it('Should update fee tier status', async function () {
      await factory.connect(feeSetter).updateFeeTier(30, false);

      const feeInfo = await factory.feeTiers(30);
      expect(feeInfo.isActive).to.be.false;

      // Should not be able to create pools with inactive fee tier
      await expect(
        factory
          .connect(poolCreator)
          .createPool(await tokenA.getAddress(), await tokenB.getAddress(), 30)
      ).to.be.revertedWith('CarbonCreditAMMFactory: INVALID_FEE_TIER');
    });

    it('Should prevent unauthorized fee tier management', async function () {
      await expect(
        factory.connect(user1).addFeeTier(200, 400)
      ).to.be.revertedWithCustomError(
        factory,
        'AccessControlUnauthorizedAccount'
      );

      await expect(
        factory.connect(user1).updateFeeTier(30, false)
      ).to.be.revertedWithCustomError(
        factory,
        'AccessControlUnauthorizedAccount'
      );
    });
  });

  describe('Pool Information Queries', function () {
    beforeEach(async function () {
      // Create multiple pools
      await factory
        .connect(poolCreator)
        .createPool(await tokenA.getAddress(), await tokenB.getAddress(), 30);
      await factory
        .connect(poolCreator)
        .createPool(await tokenA.getAddress(), await tokenB.getAddress(), 50);
    });

    it('Should get all pools', async function () {
      const allPools = await factory.getAllPools();
      expect(allPools.length).to.equal(2);
    });

    it('Should get pools for a specific pair', async function () {
      const [poolAddresses, fees] = await factory.getPoolsForPair(
        await tokenA.getAddress(),
        await tokenB.getAddress()
      );

      expect(poolAddresses.length).to.equal(2);
      expect(fees.length).to.equal(2);
      expect(fees).to.include(30n);
      expect(fees).to.include(50n);
    });

    it('Should check pool existence', async function () {
      expect(
        await factory.poolExists(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          30
        )
      ).to.be.true;

      expect(
        await factory.poolExists(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          999
        )
      ).to.be.false;
    });

    it('Should get factory statistics', async function () {
      const [totalPools, activePools, totalFeeTiers, feeRecipient] =
        await factory.getFactoryStats();

      expect(totalPools).to.equal(2);
      expect(activePools).to.equal(2);
      expect(totalFeeTiers).to.equal(4); // Default fee tiers
      expect(feeRecipient).to.equal(ethers.ZeroAddress);
    });
  });

  describe('Governance Functions', function () {
    it('Should set fee recipient', async function () {
      const newFeeRecipient = user1.address;

      const tx = await factory.setFeeTo(newFeeRecipient);

      await expect(tx)
        .to.emit(factory, 'FeeToChanged')
        .withArgs(ethers.ZeroAddress, newFeeRecipient);

      expect(await factory.feeTo()).to.equal(newFeeRecipient);
    });

    it('Should set governance address', async function () {
      const newGovernance = user1.address;

      await factory.setGovernance(newGovernance);

      expect(await factory.governance()).to.equal(newGovernance);
    });

    it('Should prevent setting zero address as governance', async function () {
      await expect(
        factory.setGovernance(ethers.ZeroAddress)
      ).to.be.revertedWith('CarbonCreditAMMFactory: ZERO_ADDRESS');
    });

    it('Should grant and revoke pool creator role', async function () {
      const POOL_CREATOR_ROLE = await factory.POOL_CREATOR_ROLE();

      await factory.grantPoolCreatorRole(user1.address);
      expect(await factory.hasRole(POOL_CREATOR_ROLE, user1.address)).to.be
        .true;

      await factory.revokePoolCreatorRole(user1.address);
      expect(await factory.hasRole(POOL_CREATOR_ROLE, user1.address)).to.be
        .false;
    });
  });

  describe('Pool Management', function () {
    let poolAddress;

    beforeEach(async function () {
      await factory
        .connect(poolCreator)
        .createPool(await tokenA.getAddress(), await tokenB.getAddress(), 30);

      poolAddress = await factory.getPool(
        await tokenA.getAddress(),
        await tokenB.getAddress(),
        30
      );
    });

    it('Should set pool status', async function () {
      // Deactivate pool
      await factory.setPoolStatus(poolAddress, false);

      const poolInfo = await factory.poolInfo(poolAddress);
      expect(poolInfo.isActive).to.be.false;

      // Reactivate pool
      await factory.setPoolStatus(poolAddress, true);

      const updatedPoolInfo = await factory.poolInfo(poolAddress);
      expect(updatedPoolInfo.isActive).to.be.true;
    });

    it('Should prevent setting status for non-existent pool', async function () {
      await expect(
        factory.setPoolStatus(user1.address, false)
      ).to.be.revertedWith('CarbonCreditAMMFactory: POOL_NOT_EXISTS');
    });
  });

  describe('Emergency Functions', function () {
    it('Should allow admin to pause/unpause', async function () {
      await factory.pause();

      await expect(
        factory
          .connect(poolCreator)
          .createPool(await tokenA.getAddress(), await tokenB.getAddress(), 30)
      ).to.be.revertedWithCustomError(factory, 'EnforcedPause');

      await factory.unpause();

      // Should work after unpause
      await factory
        .connect(poolCreator)
        .createPool(await tokenA.getAddress(), await tokenB.getAddress(), 30);
    });

    it('Should prevent unauthorized pause/unpause', async function () {
      await expect(
        factory.connect(user1).pause()
      ).to.be.revertedWithCustomError(
        factory,
        'AccessControlUnauthorizedAccount'
      );

      await expect(
        factory.connect(user1).unpause()
      ).to.be.revertedWithCustomError(
        factory,
        'AccessControlUnauthorizedAccount'
      );
    });
  });

  describe('Edge Cases', function () {
    it('Should handle maximum number of fee tiers', async function () {
      // Add many fee tiers to test array limits
      const feeTiers = [75, 125, 175, 225, 275, 325, 375, 425, 475, 525];

      for (const tier of feeTiers) {
        await factory.connect(feeSetter).addFeeTier(tier, tier * 2);
      }

      const allTiers = await factory.getSupportedFeeTiers();
      expect(allTiers.length).to.equal(14); // 4 default + 10 added
    });

    it('Should maintain consistent state after multiple operations', async function () {
      // Create pools with different fee tiers
      const feeTiers = [30, 50, 100];

      for (const tier of feeTiers) {
        await factory
          .connect(poolCreator)
          .createPool(
            await tokenA.getAddress(),
            await tokenB.getAddress(),
            tier
          );
      }

      // Verify all pools exist
      for (const tier of feeTiers) {
        const poolAddress = await factory.getPool(
          await tokenA.getAddress(),
          await tokenB.getAddress(),
          tier
        );
        expect(poolAddress).to.not.equal(ethers.ZeroAddress);
      }

      expect(await factory.allPoolsLength()).to.equal(3);
    });

    it('Should handle token address edge cases correctly', async function () {
      // Test with addresses that might cause sorting issues
      const MockToken = await ethers.getContractFactory('WRFToken');
      const token1 = await MockToken.deploy();
      const token2 = await MockToken.deploy();

      // Should work regardless of which token has lower address
      await factory
        .connect(poolCreator)
        .createPool(await token1.getAddress(), await token2.getAddress(), 30);

      const poolAddress1 = await factory.getPool(
        await token1.getAddress(),
        await token2.getAddress(),
        30
      );

      const poolAddress2 = await factory.getPool(
        await token2.getAddress(),
        await token1.getAddress(),
        30
      );

      expect(poolAddress1).to.equal(poolAddress2);
      expect(poolAddress1).to.not.equal(ethers.ZeroAddress);
    });
  });
});
