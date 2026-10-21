const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('CarbonBasketToken', function () {
  let carbonBasketToken;
  let enhancedCarbonCredits;
  let wrfToken;
  let owner;
  let manager;
  let rebalancer;
  let user1;
  let user2;

  const basketConfig = {
    name: 'Nature-Based Solutions Basket',
    symbol: 'NBS',
    allowedProjectTypes: ['forestry', 'blue-carbon', 'soil-carbon'],
    allowedRegistries: ['VCS', 'GS', 'CAR'],
    minVintage: 2020,
    maxVintage: 2025,
    minQualityScore: 500,
    retirementRate: 100, // 1% annual retirement rate
  };

  beforeEach(async function () {
    [owner, manager, rebalancer, user1, user2] = await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    wrfToken = await WRFToken.deploy();

    // Deploy Simple Mock Enhanced Carbon Credits for testing
    const SimpleMockEnhancedCarbonCredits = await ethers.getContractFactory(
      'SimpleMockEnhancedCarbonCredits'
    );
    enhancedCarbonCredits = await SimpleMockEnhancedCarbonCredits.deploy();

    // Deploy Carbon Basket Token
    const CarbonBasketToken =
      await ethers.getContractFactory('CarbonBasketToken');
    carbonBasketToken = await CarbonBasketToken.deploy(
      await enhancedCarbonCredits.getAddress(),
      basketConfig
    );

    // Setup roles
    const MANAGER_ROLE = await carbonBasketToken.MANAGER_ROLE();
    const REBALANCER_ROLE = await carbonBasketToken.REBALANCER_ROLE();
    const AUTHORIZED_ISSUER_ROLE =
      await enhancedCarbonCredits.AUTHORIZED_ISSUER_ROLE();
    const REGISTRY_CONNECTOR_ROLE =
      await enhancedCarbonCredits.REGISTRY_CONNECTOR_ROLE();

    await carbonBasketToken.grantRole(MANAGER_ROLE, manager.address);
    await carbonBasketToken.grantRole(REBALANCER_ROLE, rebalancer.address);
    await enhancedCarbonCredits.grantRole(
      AUTHORIZED_ISSUER_ROLE,
      owner.address
    );
    await enhancedCarbonCredits.grantRole(
      REGISTRY_CONNECTOR_ROLE,
      owner.address
    );

    // Mint WRF tokens for testing
    await wrfToken.transfer(user1.address, ethers.parseEther('10000'));
    await wrfToken.transfer(user2.address, ethers.parseEther('10000'));

    // Approve contracts
    await wrfToken
      .connect(user1)
      .approve(
        await enhancedCarbonCredits.getAddress(),
        ethers.parseEther('10000')
      );
    await wrfToken
      .connect(user2)
      .approve(
        await enhancedCarbonCredits.getAddress(),
        ethers.parseEther('10000')
      );
  });

  async function createAndImmobilizeCredits(
    projectType,
    vintage,
    registryStandard,
    amount
  ) {
    const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;
    const projectId = `${registryStandard}-${projectType}-${vintage}`;
    const certHash = `QmHash${Math.random().toString(36).substring(7)}`;

    await enhancedCarbonCredits.createBatch(
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

    await enhancedCarbonCredits.immobilizeCredits(
      tokenId,
      amount,
      user1.address
    );

    return tokenId;
  }

  describe('Deployment', function () {
    it('Should deploy with correct configuration', async function () {
      const config = await carbonBasketToken.config();
      expect(config.name).to.equal(basketConfig.name);
      expect(config.symbol).to.equal(basketConfig.symbol);
      expect(config.minVintage).to.equal(basketConfig.minVintage);
      expect(config.maxVintage).to.equal(basketConfig.maxVintage);
      expect(config.minQualityScore).to.equal(basketConfig.minQualityScore);
      expect(config.retirementRate).to.equal(basketConfig.retirementRate);
    });

    it('Should have correct token properties', async function () {
      expect(await carbonBasketToken.name()).to.equal(basketConfig.name);
      expect(await carbonBasketToken.symbol()).to.equal(basketConfig.symbol);
      expect(await carbonBasketToken.decimals()).to.equal(18);
    });

    it('Should setup roles correctly', async function () {
      const DEFAULT_ADMIN_ROLE = await carbonBasketToken.DEFAULT_ADMIN_ROLE();
      const MANAGER_ROLE = await carbonBasketToken.MANAGER_ROLE();
      const REBALANCER_ROLE = await carbonBasketToken.REBALANCER_ROLE();

      expect(await carbonBasketToken.hasRole(DEFAULT_ADMIN_ROLE, owner.address))
        .to.be.true;
      expect(await carbonBasketToken.hasRole(MANAGER_ROLE, manager.address)).to
        .be.true;
      expect(
        await carbonBasketToken.hasRole(REBALANCER_ROLE, rebalancer.address)
      ).to.be.true;
    });
  });

  describe('Deposit Functionality', function () {
    let tokenId;
    const depositAmount = ethers.parseUnits('1000', 0);

    beforeEach(async function () {
      tokenId = await createAndImmobilizeCredits(
        'forestry',
        2023,
        'VCS',
        depositAmount
      );
    });

    it('Should deposit eligible carbon credits', async function () {
      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      const tx = await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, depositAmount);

      await expect(tx)
        .to.emit(carbonBasketToken, 'CarbonCreditsDeposited')
        .withArgs(user1.address, tokenId, depositAmount, depositAmount);

      expect(await carbonBasketToken.balanceOf(user1.address)).to.equal(
        depositAmount
      );
      expect(await carbonBasketToken.totalCarbonSupply()).to.equal(
        depositAmount
      );
    });

    it('Should reject ineligible carbon credits', async function () {
      // Create credits with old vintage (not eligible)
      const oldTokenId = await createAndImmobilizeCredits(
        'forestry',
        2019,
        'VCS',
        depositAmount
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      await expect(
        carbonBasketToken.connect(user1).deposit(oldTokenId, depositAmount)
      ).to.be.revertedWith('CarbonBasketToken: Token not eligible');
    });

    it('Should update basket statistics correctly', async function () {
      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      await carbonBasketToken.connect(user1).deposit(tokenId, depositAmount);

      const stats = await carbonBasketToken.getBasketStats();
      expect(stats._totalSupply).to.equal(depositAmount);
      expect(stats._totalCarbonSupply).to.equal(depositAmount);
      expect(stats._holdingsCount).to.equal(1);
      expect(stats._averageQuality).to.be.gt(0);
    });

    it('Should handle multiple deposits correctly', async function () {
      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      // First deposit
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, depositAmount / 2n);

      // Second deposit
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, depositAmount / 2n);

      expect(await carbonBasketToken.balanceOf(user1.address)).to.equal(
        depositAmount
      );
      expect(await carbonBasketToken.totalCarbonSupply()).to.equal(
        depositAmount
      );

      const holdings = await carbonBasketToken.getHoldings();
      expect(holdings.length).to.equal(1);
      expect(holdings[0].amount).to.equal(depositAmount);
    });
  });

  describe('Withdrawal Functionality', function () {
    let tokenId1, tokenId2;
    const depositAmount = ethers.parseUnits('1000', 0);

    beforeEach(async function () {
      tokenId1 = await createAndImmobilizeCredits(
        'forestry',
        2023,
        'VCS',
        depositAmount
      );
      tokenId2 = await createAndImmobilizeCredits(
        'blue-carbon',
        2022,
        'GS',
        depositAmount
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      // Deposit both tokens
      await carbonBasketToken.connect(user1).deposit(tokenId1, depositAmount);
      await carbonBasketToken.connect(user1).deposit(tokenId2, depositAmount);
    });

    it('Should withdraw carbon credits', async function () {
      const withdrawAmount = ethers.parseUnits('500', 0);

      const tx = await carbonBasketToken
        .connect(user1)
        .withdraw(withdrawAmount, tokenId1);

      await expect(tx).to.emit(carbonBasketToken, 'CarbonCreditsWithdrawn');

      expect(await carbonBasketToken.balanceOf(user1.address)).to.equal(
        depositAmount * 2n - withdrawAmount
      );
      expect(
        await enhancedCarbonCredits.balanceOf(user1.address, tokenId1)
      ).to.equal(withdrawAmount);
    });

    it('Should select lowest quality credits when no preference', async function () {
      const withdrawAmount = ethers.parseUnits('500', 0);

      await carbonBasketToken.connect(user1).withdraw(withdrawAmount, 0);

      expect(await carbonBasketToken.balanceOf(user1.address)).to.equal(
        depositAmount * 2n - withdrawAmount
      );
    });

    it('Should prevent withdrawal of more than balance', async function () {
      const excessiveAmount = depositAmount * 3n;

      await expect(
        carbonBasketToken.connect(user1).withdraw(excessiveAmount, 0)
      ).to.be.revertedWith('CarbonBasketToken: Insufficient balance');
    });

    it('Should update basket statistics after withdrawal', async function () {
      const withdrawAmount = ethers.parseUnits('500', 0);

      await carbonBasketToken.connect(user1).withdraw(withdrawAmount, tokenId1);

      const stats = await carbonBasketToken.getBasketStats();
      expect(stats._totalSupply).to.equal(depositAmount * 2n - withdrawAmount);
      expect(stats._totalCarbonSupply).to.equal(
        depositAmount * 2n - withdrawAmount
      );
    });
  });

  describe('Auto Retirement Functionality', function () {
    let tokenId1, tokenId2;
    const depositAmount = ethers.parseUnits('1000', 0);

    beforeEach(async function () {
      tokenId1 = await createAndImmobilizeCredits(
        'forestry',
        2020,
        'VCS',
        depositAmount
      );
      tokenId2 = await createAndImmobilizeCredits(
        'blue-carbon',
        2023,
        'GS',
        depositAmount
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      await carbonBasketToken.connect(user1).deposit(tokenId1, depositAmount);
      await carbonBasketToken.connect(user1).deposit(tokenId2, depositAmount);
    });

    it('Should perform auto retirement', async function () {
      // Fast forward time to simulate passage of time
      await ethers.provider.send('evm_increaseTime', [365 * 24 * 60 * 60]); // 1 year
      await ethers.provider.send('evm_mine');

      const tx = await carbonBasketToken
        .connect(rebalancer)
        .performAutoRetirement();

      await expect(tx).to.emit(carbonBasketToken, 'BasketRebalanced');
    });

    it('Should prevent frequent rebalancing', async function () {
      await carbonBasketToken.connect(rebalancer).performAutoRetirement();

      await expect(
        carbonBasketToken.connect(rebalancer).performAutoRetirement()
      ).to.be.revertedWith('CarbonBasketToken: Too frequent');
    });

    it('Should only allow rebalancer to perform auto retirement', async function () {
      await expect(
        carbonBasketToken.connect(user1).performAutoRetirement()
      ).to.be.revertedWithCustomError(
        carbonBasketToken,
        'AccessControlUnauthorizedAccount'
      );
    });
  });

  describe('Quality Management', function () {
    it('Should calculate quality scores correctly', async function () {
      // Test with VCS forestry 2023 credits
      const tokenId = await createAndImmobilizeCredits(
        'forestry',
        2023,
        'VCS',
        ethers.parseUnits('1000', 0)
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, ethers.parseUnits('1000', 0));

      const stats = await carbonBasketToken.getBasketStats();
      expect(stats._averageQuality).to.be.gt(basketConfig.minQualityScore);
    });

    it('Should maintain quality floor', async function () {
      const tokenId = await createAndImmobilizeCredits(
        'forestry',
        2023,
        'VCS',
        ethers.parseUnits('1000', 0)
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, ethers.parseUnits('1000', 0));

      const qualityFloor = await carbonBasketToken.qualityFloor();
      expect(qualityFloor).to.be.gte(basketConfig.minQualityScore);
    });
  });

  describe('Access Control', function () {
    it('Should allow manager to update configuration', async function () {
      const newConfig = {
        ...basketConfig,
        minQualityScore: 600,
      };

      await carbonBasketToken.connect(owner).updateConfig(newConfig);

      const config = await carbonBasketToken.config();
      expect(config.minQualityScore).to.equal(600);
    });

    it('Should prevent unauthorized configuration updates', async function () {
      const newConfig = {
        ...basketConfig,
        minQualityScore: 600,
      };

      await expect(
        carbonBasketToken.connect(user1).updateConfig(newConfig)
      ).to.be.revertedWithCustomError(
        carbonBasketToken,
        'AccessControlUnauthorizedAccount'
      );
    });

    it('Should allow manager to set authorized operators', async function () {
      await carbonBasketToken
        .connect(manager)
        .setAuthorizedOperator(user1.address, true);
      expect(await carbonBasketToken.authorizedOperators(user1.address)).to.be
        .true;

      await carbonBasketToken
        .connect(manager)
        .setAuthorizedOperator(user1.address, false);
      expect(await carbonBasketToken.authorizedOperators(user1.address)).to.be
        .false;
    });
  });

  describe('Emergency Functions', function () {
    it('Should allow admin to pause/unpause', async function () {
      await carbonBasketToken.connect(owner).pause();

      const tokenId = await createAndImmobilizeCredits(
        'forestry',
        2023,
        'VCS',
        ethers.parseUnits('1000', 0)
      );

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      await expect(
        carbonBasketToken
          .connect(user1)
          .deposit(tokenId, ethers.parseUnits('1000', 0))
      ).to.be.revertedWithCustomError(carbonBasketToken, 'EnforcedPause');

      await carbonBasketToken.connect(owner).unpause();

      // Should work after unpause
      await carbonBasketToken
        .connect(user1)
        .deposit(tokenId, ethers.parseUnits('1000', 0));
    });
  });

  describe('ERC1155 Receiver', function () {
    it('Should implement ERC1155 receiver interface', async function () {
      const tokenId = await createAndImmobilizeCredits(
        'forestry',
        2023,
        'VCS',
        ethers.parseUnits('1000', 0)
      );

      // Direct transfer should work
      await enhancedCarbonCredits
        .connect(user1)
        .safeTransferFrom(
          user1.address,
          await carbonBasketToken.getAddress(),
          tokenId,
          ethers.parseUnits('100', 0),
          '0x'
        );

      expect(
        await enhancedCarbonCredits.balanceOf(
          await carbonBasketToken.getAddress(),
          tokenId
        )
      ).to.equal(ethers.parseUnits('100', 0));
    });
  });

  describe('Edge Cases', function () {
    it('Should handle zero deposits gracefully', async function () {
      const tokenId = await createAndImmobilizeCredits(
        'forestry',
        2023,
        'VCS',
        ethers.parseUnits('1000', 0)
      );

      await expect(
        carbonBasketToken.connect(user1).deposit(tokenId, 0)
      ).to.be.revertedWith('CarbonBasketToken: Invalid amount');
    });

    it('Should handle empty basket statistics', async function () {
      const stats = await carbonBasketToken.getBasketStats();
      expect(stats._totalSupply).to.equal(0);
      expect(stats._totalCarbonSupply).to.equal(0);
      expect(stats._averageQuality).to.equal(0);
      expect(stats._holdingsCount).to.equal(0);
    });

    it('Should maintain invariants under stress', async function () {
      // Create multiple different tokens
      const tokens = [];
      for (let i = 0; i < 5; i++) {
        const tokenId = await createAndImmobilizeCredits(
          basketConfig.allowedProjectTypes[
            i % basketConfig.allowedProjectTypes.length
          ],
          2021 + i,
          basketConfig.allowedRegistries[
            i % basketConfig.allowedRegistries.length
          ],
          ethers.parseUnits('100', 0)
        );
        tokens.push(tokenId);
      }

      await enhancedCarbonCredits
        .connect(user1)
        .setApprovalForAll(await carbonBasketToken.getAddress(), true);

      // Deposit all tokens
      for (const tokenId of tokens) {
        await carbonBasketToken
          .connect(user1)
          .deposit(tokenId, ethers.parseUnits('100', 0));
      }

      // Check balance before withdrawal
      const balanceBeforeWithdrawal = await carbonBasketToken.balanceOf(
        user1.address
      );
      const statsBeforeWithdrawal = await carbonBasketToken.getBasketStats();
      const holdingsBeforeWithdrawal = await carbonBasketToken.getHoldings();

      console.log(
        'Balance before withdrawal:',
        balanceBeforeWithdrawal.toString()
      );
      console.log(
        'Total supply before:',
        statsBeforeWithdrawal._totalSupply.toString()
      );
      console.log('Holdings count before:', holdingsBeforeWithdrawal.length);
      console.log(
        'Holdings before:',
        holdingsBeforeWithdrawal.map(h => ({
          tokenId: h.tokenId.toString(),
          amount: h.amount.toString(),
          qualityScore: h.qualityScore.toString(),
        }))
      );

      // Withdraw some
      await carbonBasketToken
        .connect(user1)
        .withdraw(ethers.parseUnits('150', 0), 0);

      // Check invariants
      const stats = await carbonBasketToken.getBasketStats();
      expect(stats._totalSupply).to.equal(stats._totalCarbonSupply);
      expect(stats._totalSupply).to.equal(ethers.parseUnits('350', 0));
    });
  });
});
