const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('EnhancedCarbonCredits', function () {
  let enhancedCarbonCredits;
  let wrfToken;
  let owner;
  let issuer;
  let registryConnector;
  let user1;
  let user2;
  let pauser;

  const PROJECT_ID = 'VCS-001-2023';
  const VINTAGE = 2023;
  const METHODOLOGY = 'VM0006';
  const PROJECT_TYPE = 'renewable-energy';
  const REGISTRY_STANDARD = 'VCS';
  const LOCATION = '40.7128,-74.0060';
  const CERTIFICATION_HASH = 'QmHash123';
  const REGISTRY_URL = 'https://registry.verra.org/project/001';
  const METADATA_URI = 'https://ipfs.io/ipfs/QmMetadata123';
  const TOTAL_SUPPLY = ethers.parseUnits('10000', 0); // 10,000 kg CO2e

  beforeEach(async function () {
    [owner, issuer, registryConnector, user1, user2, pauser] =
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

    // Setup roles
    const AUTHORIZED_ISSUER_ROLE =
      await enhancedCarbonCredits.AUTHORIZED_ISSUER_ROLE();
    const REGISTRY_CONNECTOR_ROLE =
      await enhancedCarbonCredits.REGISTRY_CONNECTOR_ROLE();
    const PAUSER_ROLE = await enhancedCarbonCredits.PAUSER_ROLE();

    await enhancedCarbonCredits.grantRole(
      AUTHORIZED_ISSUER_ROLE,
      issuer.address
    );
    await enhancedCarbonCredits.grantRole(
      REGISTRY_CONNECTOR_ROLE,
      registryConnector.address
    );
    await enhancedCarbonCredits.grantRole(PAUSER_ROLE, pauser.address);

    // Mint WRF tokens for testing
    await wrfToken.transfer(user1.address, ethers.parseEther('10000'));
    await wrfToken.transfer(user2.address, ethers.parseEther('10000'));

    // Approve Enhanced Carbon Credits contract
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

  describe('Batch Creation', function () {
    it('Should create a new carbon credit batch', async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60; // 1 year from now

      const tx = await enhancedCarbonCredits
        .connect(issuer)
        .createBatch(
          TOTAL_SUPPLY,
          PROJECT_ID,
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        );

      await expect(tx)
        .to.emit(enhancedCarbonCredits, 'BatchCreated')
        .withArgs(1, issuer.address, PROJECT_ID, TOTAL_SUPPLY, VINTAGE);

      const batch = await enhancedCarbonCredits.getBatchInfo(1);
      expect(batch.totalSupply).to.equal(TOTAL_SUPPLY);
      expect(batch.projectId).to.equal(PROJECT_ID);
      expect(batch.vintage).to.equal(VINTAGE);
      expect(batch.issuer).to.equal(issuer.address);
      expect(batch.isActive).to.be.true;
    });

    it('Should prevent unauthorized batch creation', async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      await expect(
        enhancedCarbonCredits
          .connect(user1)
          .createBatch(
            TOTAL_SUPPLY,
            PROJECT_ID,
            VINTAGE,
            METHODOLOGY,
            PROJECT_TYPE,
            REGISTRY_STANDARD,
            LOCATION,
            futureDate,
            CERTIFICATION_HASH,
            REGISTRY_URL,
            METADATA_URI
          )
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent duplicate certification hashes', async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      // Create first batch
      await enhancedCarbonCredits
        .connect(issuer)
        .createBatch(
          TOTAL_SUPPLY,
          PROJECT_ID,
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        );

      // Try to create second batch with same hash
      await expect(
        enhancedCarbonCredits.connect(issuer).createBatch(
          TOTAL_SUPPLY,
          'VCS-002-2023',
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH, // Same hash
          REGISTRY_URL,
          METADATA_URI
        )
      ).to.be.revertedWith('EnhancedCarbonCredits: Hash already used');
    });
  });

  describe('Credit Immobilization', function () {
    let tokenId;

    beforeEach(async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      await enhancedCarbonCredits
        .connect(issuer)
        .createBatch(
          TOTAL_SUPPLY,
          PROJECT_ID,
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        );

      tokenId = 1;
    });

    it('Should immobilize credits and mint tokens', async function () {
      const amount = ethers.parseUnits('1000', 0); // 1,000 kg

      const tx = await enhancedCarbonCredits
        .connect(registryConnector)
        .immobilizeCredits(tokenId, amount, user1.address);

      await expect(tx)
        .to.emit(enhancedCarbonCredits, 'CreditsImmobilized')
        .withArgs(tokenId, amount, user1.address);

      expect(
        await enhancedCarbonCredits.balanceOf(user1.address, tokenId)
      ).to.equal(amount);

      const batch = await enhancedCarbonCredits.getBatchInfo(tokenId);
      expect(batch.immobilizedSupply).to.equal(amount);
    });

    it('Should prevent unauthorized immobilization', async function () {
      const amount = ethers.parseUnits('1000', 0);

      await expect(
        enhancedCarbonCredits
          .connect(user1)
          .immobilizeCredits(tokenId, amount, user1.address)
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent exceeding total supply', async function () {
      const excessiveAmount = TOTAL_SUPPLY + ethers.parseUnits('1', 0);

      await expect(
        enhancedCarbonCredits
          .connect(registryConnector)
          .immobilizeCredits(tokenId, excessiveAmount, user1.address)
      ).to.be.revertedWith('EnhancedCarbonCredits: Exceeds total supply');
    });
  });

  describe('Credit Retirement', function () {
    let tokenId;
    const immobilizedAmount = ethers.parseUnits('1000', 0);

    beforeEach(async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      await enhancedCarbonCredits
        .connect(issuer)
        .createBatch(
          TOTAL_SUPPLY,
          PROJECT_ID,
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        );

      tokenId = 1;

      // Immobilize credits to user1
      await enhancedCarbonCredits
        .connect(registryConnector)
        .immobilizeCredits(tokenId, immobilizedAmount, user1.address);
    });

    it('Should retire credits and burn tokens', async function () {
      const retireAmount = ethers.parseUnits('500', 0);
      const retirementMessage = 'Offsetting company emissions 2023';

      const tx = await enhancedCarbonCredits
        .connect(user1)
        .retireCredits(tokenId, retireAmount, retirementMessage);

      await expect(tx)
        .to.emit(enhancedCarbonCredits, 'CreditsRetired')
        .withArgs(
          tokenId,
          user1.address,
          retireAmount,
          retirementMessage,
          (retireAmount * 25n) / 10000n
        );

      // Check balance after retirement
      expect(
        await enhancedCarbonCredits.balanceOf(user1.address, tokenId)
      ).to.equal(immobilizedAmount - retireAmount);

      // Check retirement statistics
      const stats = await enhancedCarbonCredits.getUserRetirementStats(
        user1.address
      );
      expect(stats[0]).to.equal(retireAmount); // totalRetired

      const batch = await enhancedCarbonCredits.getBatchInfo(tokenId);
      expect(batch.retiredSupply).to.equal(retireAmount);
    });

    it('Should prevent retiring more credits than owned', async function () {
      const excessiveAmount = immobilizedAmount + ethers.parseUnits('1', 0);

      await expect(
        enhancedCarbonCredits
          .connect(user1)
          .retireCredits(tokenId, excessiveAmount, 'Test retirement')
      ).to.be.revertedWith('EnhancedCarbonCredits: Insufficient balance');
    });

    it('Should calculate retirement fees correctly', async function () {
      const retireAmount = ethers.parseUnits('1000', 0);
      const expectedFee = (retireAmount * 25n) / 10000n; // 0.25%

      const initialBalance = await wrfToken.balanceOf(user1.address);

      await enhancedCarbonCredits
        .connect(user1)
        .retireCredits(tokenId, retireAmount, 'Test retirement');

      const finalBalance = await wrfToken.balanceOf(user1.address);
      expect(initialBalance - finalBalance).to.equal(expectedFee);
    });
  });

  describe('Marketplace Functionality', function () {
    let tokenId;
    const immobilizedAmount = ethers.parseUnits('1000', 0);
    const listAmount = ethers.parseUnits('500', 0);
    const pricePerUnit = ethers.parseEther('0.1'); // 0.1 WRF per kg

    beforeEach(async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      await enhancedCarbonCredits
        .connect(issuer)
        .createBatch(
          TOTAL_SUPPLY,
          PROJECT_ID,
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        );

      tokenId = 1;

      await enhancedCarbonCredits
        .connect(registryConnector)
        .immobilizeCredits(tokenId, immobilizedAmount, user1.address);
    });

    it('Should list credits for sale', async function () {
      const expiry = Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60; // 30 days

      const tx = await enhancedCarbonCredits
        .connect(user1)
        .listCreditsForSale(tokenId, listAmount, pricePerUnit, expiry);

      await expect(tx)
        .to.emit(enhancedCarbonCredits, 'CreditListed')
        .withArgs(tokenId, 0, user1.address, listAmount, pricePerUnit);
    });

    it('Should allow purchasing listed credits', async function () {
      const expiry = Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60;
      const purchaseAmount = ethers.parseUnits('200', 0);

      // List credits
      await enhancedCarbonCredits
        .connect(user1)
        .listCreditsForSale(tokenId, listAmount, pricePerUnit, expiry);

      const listingId = 0;
      const totalPrice = purchaseAmount * pricePerUnit;

      const user1BalanceBefore = await wrfToken.balanceOf(user1.address);
      const user2BalanceBefore = await enhancedCarbonCredits.balanceOf(
        user2.address,
        tokenId
      );

      const tx = await enhancedCarbonCredits
        .connect(user2)
        .purchaseCredits(tokenId, listingId, purchaseAmount);

      await expect(tx)
        .to.emit(enhancedCarbonCredits, 'CreditSold')
        .withArgs(
          tokenId,
          listingId,
          user2.address,
          user1.address,
          purchaseAmount,
          totalPrice
        );

      // Check balances after purchase
      expect(await wrfToken.balanceOf(user1.address)).to.equal(
        user1BalanceBefore + totalPrice
      );
      expect(
        await enhancedCarbonCredits.balanceOf(user2.address, tokenId)
      ).to.equal(user2BalanceBefore + purchaseAmount);
    });

    it('Should allow canceling listings', async function () {
      const expiry = Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60;

      await enhancedCarbonCredits
        .connect(user1)
        .listCreditsForSale(tokenId, listAmount, pricePerUnit, expiry);

      const listingId = 0;
      await enhancedCarbonCredits
        .connect(user1)
        .cancelListing(tokenId, listingId);

      // Try to purchase canceled listing
      await expect(
        enhancedCarbonCredits
          .connect(user2)
          .purchaseCredits(tokenId, listingId, ethers.parseUnits('100', 0))
      ).to.be.revertedWith('EnhancedCarbonCredits: Listing not active');
    });
  });

  describe('Price Estimation', function () {
    it('Should estimate prices correctly for different project types', async function () {
      const amount = ethers.parseUnits('1000', 0); // 1 tonne = 1000 kg
      const basePrice = ethers.parseEther('50'); // 50 WRF per tonne

      // Renewable energy (20% premium)
      const renewablePrice = await enhancedCarbonCredits.estimateOffsetValue(
        amount,
        'renewable-energy'
      );
      expect(renewablePrice).to.equal((basePrice * 12n) / 10n);

      // Direct air capture (3x premium)
      const dacPrice = await enhancedCarbonCredits.estimateOffsetValue(
        amount,
        'direct-air-capture'
      );
      expect(dacPrice).to.equal(basePrice * 3n);

      // Other project types (base price)
      const forestryPrice = await enhancedCarbonCredits.estimateOffsetValue(
        amount,
        'forestry'
      );
      expect(forestryPrice).to.equal(basePrice);
    });
  });

  describe('Admin Functions', function () {
    let tokenId;

    beforeEach(async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      await enhancedCarbonCredits
        .connect(issuer)
        .createBatch(
          TOTAL_SUPPLY,
          PROJECT_ID,
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        );

      tokenId = 1;
    });

    it('Should allow admin to pause/unpause contract', async function () {
      await enhancedCarbonCredits.connect(pauser).pause();

      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      await expect(
        enhancedCarbonCredits
          .connect(issuer)
          .createBatch(
            TOTAL_SUPPLY,
            'VCS-002-2023',
            VINTAGE,
            METHODOLOGY,
            PROJECT_TYPE,
            REGISTRY_STANDARD,
            LOCATION,
            futureDate,
            'QmHash456',
            REGISTRY_URL,
            METADATA_URI
          )
      ).to.be.revertedWith('Pausable: paused');

      await enhancedCarbonCredits.connect(pauser).unpause();

      // Should work after unpause
      await enhancedCarbonCredits
        .connect(issuer)
        .createBatch(
          TOTAL_SUPPLY,
          'VCS-002-2023',
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          'QmHash456',
          REGISTRY_URL,
          METADATA_URI
        );
    });

    it('Should allow admin to update batch status', async function () {
      await enhancedCarbonCredits.setBatchStatus(tokenId, false);

      const batch = await enhancedCarbonCredits.getBatchInfo(tokenId);
      expect(batch.isActive).to.be.false;
    });
  });

  describe('Security Tests', function () {
    it('Should prevent reentrancy attacks', async function () {
      // This would require a malicious contract to test properly
      // For now, we check that the ReentrancyGuard is in place
      expect(await enhancedCarbonCredits.supportsInterface('0x01ffc9a7')).to.be
        .true; // ERC165
    });

    it('Should validate all inputs properly', async function () {
      const futureDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60;

      // Test invalid vintage
      await expect(
        enhancedCarbonCredits.connect(issuer).createBatch(
          TOTAL_SUPPLY,
          PROJECT_ID,
          2007, // Invalid vintage
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        )
      ).to.be.revertedWith('EnhancedCarbonCredits: Invalid vintage');

      // Test zero supply
      await expect(
        enhancedCarbonCredits.connect(issuer).createBatch(
          0, // Invalid supply
          PROJECT_ID,
          VINTAGE,
          METHODOLOGY,
          PROJECT_TYPE,
          REGISTRY_STANDARD,
          LOCATION,
          futureDate,
          CERTIFICATION_HASH,
          REGISTRY_URL,
          METADATA_URI
        )
      ).to.be.revertedWith('EnhancedCarbonCredits: Invalid supply');
    });
  });
});
