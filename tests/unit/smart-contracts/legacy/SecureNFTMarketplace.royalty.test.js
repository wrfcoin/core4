const { expect } = require('chai');
const { ethers } = require('hardhat');

describe('SecureNFTMarketplace - Royalty Calculation Security Tests', function () {
  let marketplace, wrfToken, mockNFT;
  let owner,
    seller,
    buyer,
    royaltyRecipient1,
    royaltyRecipient2,
    royaltyRecipient3;
  let attacker;

  // Test constants
  const BASIS_POINTS = 10000;
  const MAX_ROYALTY_PERCENTAGE = 1000; // 10%
  const MAX_INDIVIDUAL_ROYALTY = 500; // 5%
  const MARKETPLACE_FEE_BASE = 250; // 2.5%

  beforeEach(async function () {
    [
      owner,
      seller,
      buyer,
      royaltyRecipient1,
      royaltyRecipient2,
      royaltyRecipient3,
      attacker,
    ] = await ethers.getSigners();

    // Deploy WRF Token (Mock)
    const WRFToken = await ethers.getContractFactory(
      'contracts/mocks/MockERC20.sol:MockERC20'
    );
    wrfToken = await WRFToken.deploy('WRF Token', 'WRF');

    // Deploy Mock NFT
    const MockNFT = await ethers.getContractFactory(
      'contracts/mocks/MockERC721.sol:MockERC721'
    );
    mockNFT = await MockNFT.deploy('Test NFT', 'TNFT');

    // Deploy SecureNFTMarketplace
    const SecureNFTMarketplace = await ethers.getContractFactory(
      'SecureNFTMarketplace'
    );
    marketplace = await SecureNFTMarketplace.deploy(
      await wrfToken.getAddress()
    );

    // Set up authorization
    await marketplace.setAuthorizedNFTContract(
      await mockNFT.getAddress(),
      true
    );

    // Mint NFT to seller
    await mockNFT.mint(seller.address, 1);
    await mockNFT.connect(seller).approve(await marketplace.getAddress(), 1);

    // Distribute WRF tokens
    await wrfToken.transfer(buyer.address, ethers.parseEther('100000'));
    await wrfToken
      .connect(buyer)
      .approve(await marketplace.getAddress(), ethers.parseEther('100000'));
  });

  describe('Royalty Calculation Security', function () {
    it('Should prevent arithmetic overflow in royalty calculations', async function () {
      // Set up royalties
      const recipients = [royaltyRecipient1.address];
      const percentages = [500]; // 5%

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      // Test with maximum safe price
      const maxSafePrice = ethers.parseEther('1000000000'); // Very large but safe amount

      const [totalRoyalties, royaltyRecipients, amounts] =
        await marketplace.calculateExpectedRoyalties(
          await mockNFT.getAddress(),
          1,
          maxSafePrice
        );

      expect(totalRoyalties).to.equal(
        (maxSafePrice * BigInt(500)) / BigInt(BASIS_POINTS)
      );
      expect(royaltyRecipients[0]).to.equal(royaltyRecipient1.address);
      expect(amounts[0]).to.equal(totalRoyalties);
    });

    it('Should prevent underflow in royalty calculations', async function () {
      // Set up royalties
      const recipients = [royaltyRecipient1.address];
      const percentages = [1]; // 0.01% - very small percentage

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      // Test with small price
      const smallPrice = 100; // Very small amount

      const [totalRoyalties, royaltyRecipients, amounts] =
        await marketplace.calculateExpectedRoyalties(
          await mockNFT.getAddress(),
          1,
          smallPrice
        );

      // Should handle small calculations correctly
      expect(totalRoyalties).to.equal(
        (BigInt(smallPrice) * BigInt(1)) / BigInt(BASIS_POINTS)
      );
      expect(royaltyRecipients[0]).to.equal(royaltyRecipient1.address);
    });

    it('Should validate total royalty percentage with SafeMath', async function () {
      const recipients = [royaltyRecipient1.address, royaltyRecipient2.address];
      const percentages = [499, 502]; // Total: 1001 basis points > 1000 limit

      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages)
      ).to.be.revertedWith('SecureNFTMarketplace: Total royalties too high');
    });

    it('Should validate individual royalty percentages', async function () {
      const recipients = [royaltyRecipient1.address];
      const percentages = [501]; // 5.01% > 5% individual limit

      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages)
      ).to.be.revertedWith('SecureNFTMarketplace: Individual royalty too high');
    });

    it('Should handle multiple royalty recipients with SafeMath', async function () {
      const recipients = [
        royaltyRecipient1.address,
        royaltyRecipient2.address,
        royaltyRecipient3.address,
      ];
      const percentages = [300, 400, 300]; // 3%, 4%, 3% = 10% total

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      const salePrice = ethers.parseEther('1000');

      const [totalRoyalties, royaltyRecipients, amounts] =
        await marketplace.calculateExpectedRoyalties(
          await mockNFT.getAddress(),
          1,
          salePrice
        );

      // Verify calculations
      expect(totalRoyalties).to.equal(
        (salePrice * BigInt(1000)) / BigInt(BASIS_POINTS)
      ); // 10%
      expect(amounts[0]).to.equal(
        (salePrice * BigInt(300)) / BigInt(BASIS_POINTS)
      ); // 3%
      expect(amounts[1]).to.equal(
        (salePrice * BigInt(400)) / BigInt(BASIS_POINTS)
      ); // 4%
      expect(amounts[2]).to.equal(
        (salePrice * BigInt(300)) / BigInt(BASIS_POINTS)
      ); // 3%

      // Verify sum equals total
      const calculatedTotal = amounts.reduce((sum, amount) => sum + amount, 0n);
      expect(calculatedTotal).to.equal(totalRoyalties);
    });

    it('Should prevent price manipulation through royalty calculation', async function () {
      const recipients = [royaltyRecipient1.address];
      const percentages = [1000]; // 10% maximum

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      // Test with price that would cause overflow if not protected
      const extremePrice = ethers.parseEther('999999999999999999'); // Near uint256 max / BASIS_POINTS

      await expect(
        marketplace.calculateExpectedRoyalties(
          await mockNFT.getAddress(),
          1,
          extremePrice
        )
      ).to.not.be.reverted;
    });

    it('Should emit security events during royalty calculations', async function () {
      const recipients = [royaltyRecipient1.address];
      const percentages = [500]; // 5%

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      // Create a listing
      const listingPrice = ethers.parseEther('1000');
      await marketplace.connect(seller).createFixedPriceListing(
        await mockNFT.getAddress(),
        1,
        listingPrice,
        0, // PaymentToken.Native
        ethers.ZeroAddress,
        3600, // 1 hour
        false,
        []
      );

      // Purchase to trigger royalty calculation
      const listingId = 1;
      await expect(
        marketplace
          .connect(buyer)
          .buyFixedPrice(listingId, { value: listingPrice })
      ).to.emit(marketplace, 'RoyaltyCalculationSecured');
    });
  });

  describe('Arithmetic Security Validation', function () {
    it('Should validate arithmetic security for royalty calculations', async function () {
      const recipients = [royaltyRecipient1.address];
      const percentages = [500]; // 5%

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      const [isSecure, securityReport] =
        await marketplace.validateRoyaltyArithmetic(
          await mockNFT.getAddress(),
          1,
          ethers.parseEther('1000')
        );

      expect(isSecure).to.be.true;
      expect(securityReport).to.equal('All arithmetic operations are secure');
    });

    it('Should detect insecure price configurations', async function () {
      const recipients = [royaltyRecipient1.address];
      const percentages = [500]; // 5%

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      // Test with zero price
      const [isSecure, securityReport] =
        await marketplace.validateRoyaltyArithmetic(
          await mockNFT.getAddress(),
          1,
          0
        );

      expect(isSecure).to.be.false;
      expect(securityReport).to.equal('Zero sale price detected');
    });

    it('Should detect excessive royalty configurations', async function () {
      const recipients = [
        royaltyRecipient1.address,
        royaltyRecipient2.address,
        royaltyRecipient3.address,
        attacker.address,
        owner.address,
        seller.address, // 6 recipients > MAX_ROYALTY_RECIPIENTS (5)
      ];
      const percentages = [100, 100, 100, 100, 100, 100];

      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages)
      ).to.be.revertedWith('SecureNFTMarketplace: Too many royalty recipients');
    });
  });

  describe('Edge Cases and Attack Scenarios', function () {
    it('Should handle zero royalty percentage correctly', async function () {
      // Should not allow zero royalty percentage
      const recipients = [royaltyRecipient1.address];
      const percentages = [0];

      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages)
      ).to.be.revertedWith(
        'SecureNFTMarketplace: Royalty percentage must be greater than 0'
      );
    });

    it('Should prevent royalty manipulation attack', async function () {
      // Attacker tries to set high royalties to drain buyer funds
      const recipients = [attacker.address];
      const percentages = [1001]; // 10.01% > maximum

      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages)
      ).to.be.revertedWith('SecureNFTMarketplace: Individual royalty too high');
    });

    it('Should handle royalty calculation with minimum values', async function () {
      const recipients = [royaltyRecipient1.address];
      const percentages = [1]; // 0.01% minimum

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      const minPrice = 1; // Minimum price

      const [totalRoyalties, royaltyRecipients, amounts] =
        await marketplace.calculateExpectedRoyalties(
          await mockNFT.getAddress(),
          1,
          minPrice
        );

      // Should handle minimum calculations without underflow
      expect(totalRoyalties).to.equal(0); // 1 * 1 / 10000 = 0 (rounded down)
      expect(royaltyRecipients[0]).to.equal(royaltyRecipient1.address);
      expect(amounts[0]).to.equal(0);
    });

    it('Should maintain precision in complex royalty distributions', async function () {
      const recipients = [
        royaltyRecipient1.address,
        royaltyRecipient2.address,
        royaltyRecipient3.address,
      ];
      const percentages = [333, 333, 334]; // 3.33%, 3.33%, 3.34% = 10%

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      const salePrice = ethers.parseEther('1000');

      const [totalRoyalties, royaltyRecipients, amounts] =
        await marketplace.calculateExpectedRoyalties(
          await mockNFT.getAddress(),
          1,
          salePrice
        );

      // Verify precision is maintained
      const expectedTotal = (salePrice * BigInt(1000)) / BigInt(BASIS_POINTS);
      expect(totalRoyalties).to.equal(expectedTotal);

      // Verify individual amounts
      expect(amounts[0]).to.equal(
        (salePrice * BigInt(333)) / BigInt(BASIS_POINTS)
      );
      expect(amounts[1]).to.equal(
        (salePrice * BigInt(333)) / BigInt(BASIS_POINTS)
      );
      expect(amounts[2]).to.equal(
        (salePrice * BigInt(334)) / BigInt(BASIS_POINTS)
      );

      // Verify sum equals total (accounting for rounding)
      const calculatedTotal = amounts.reduce((sum, amount) => sum + amount, 0n);
      expect(calculatedTotal).to.equal(totalRoyalties);
    });

    it('Should gas optimize royalty calculations', async function () {
      const recipients = [royaltyRecipient1.address];
      const percentages = [500]; // 5%

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      const salePrice = ethers.parseEther('1000');

      // Test gas usage for royalty calculation
      const gas = await marketplace.calculateExpectedRoyalties.estimateGas(
        await mockNFT.getAddress(),
        1,
        salePrice
      );

      console.log(`Royalty calculation gas usage: ${gas}`);
      expect(gas).to.be.below(100000); // Should be reasonably efficient
    });
  });

  describe('Integration with Purchase Flow', function () {
    it('Should integrate royalty calculations in complete purchase flow', async function () {
      const recipients = [royaltyRecipient1.address, royaltyRecipient2.address];
      const percentages = [300, 200]; // 3% + 2% = 5% total

      await marketplace
        .connect(seller)
        .setRoyalties(await mockNFT.getAddress(), 1, recipients, percentages);

      // Create listing
      const listingPrice = ethers.parseEther('1000');
      await marketplace.connect(seller).createFixedPriceListing(
        await mockNFT.getAddress(),
        1,
        listingPrice,
        0, // PaymentToken.Native
        ethers.ZeroAddress,
        3600,
        false,
        []
      );

      // Track balances before purchase
      const sellerBalanceBefore = await marketplace.pendingWithdrawals(
        seller.address
      );
      const recipient1BalanceBefore = await marketplace.pendingWithdrawals(
        royaltyRecipient1.address
      );
      const recipient2BalanceBefore = await marketplace.pendingWithdrawals(
        royaltyRecipient2.address
      );

      // Execute purchase
      await marketplace
        .connect(buyer)
        .buyFixedPrice(1, { value: listingPrice });

      // Verify royalty distributions
      const expectedRoyalty1 =
        (listingPrice * BigInt(300)) / BigInt(BASIS_POINTS);
      const expectedRoyalty2 =
        (listingPrice * BigInt(200)) / BigInt(BASIS_POINTS);
      const expectedMarketplaceFee =
        (listingPrice * BigInt(MARKETPLACE_FEE_BASE)) / BigInt(BASIS_POINTS);
      const expectedSellerAmount =
        listingPrice -
        expectedRoyalty1 -
        expectedRoyalty2 -
        expectedMarketplaceFee;

      expect(
        await marketplace.pendingWithdrawals(royaltyRecipient1.address)
      ).to.equal(recipient1BalanceBefore + expectedRoyalty1);
      expect(
        await marketplace.pendingWithdrawals(royaltyRecipient2.address)
      ).to.equal(recipient2BalanceBefore + expectedRoyalty2);
      expect(await marketplace.pendingWithdrawals(seller.address)).to.equal(
        sellerBalanceBefore + expectedSellerAmount
      );
    });
  });
});

// Mock ERC721 contract for testing
const MockERC721 = `
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";

contract MockERC721 is ERC721 {
    uint256 private _nextTokenId = 1;

    constructor(string memory name, string memory symbol) ERC721(name, symbol) {}

    function mint(address to, uint256 tokenId) external {
        _mint(to, tokenId);
    }

    function mint(address to) external returns (uint256) {
        uint256 tokenId = _nextTokenId++;
        _mint(to, tokenId);
        return tokenId;
    }
}
`;
