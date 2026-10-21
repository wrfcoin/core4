import { expect } from 'chai';
import pkg from 'hardhat';
const { ethers } = pkg;

describe('NFTMarketplace Security Tests', function () {
  let marketplace, nft, wrfToken;
  let owner, seller, buyer, attacker, royaltyRecipient;
  let listingId;

  beforeEach(async function () {
    [owner, seller, buyer, attacker, royaltyRecipient] =
      await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    wrfToken = await WRFToken.deploy();

    // Deploy ComputationalNFT
    const ComputationalNFT =
      await ethers.getContractFactory('ComputationalNFT');
    nft = await ComputationalNFT.deploy(await wrfToken.getAddress());

    // Deploy NFTMarketplace
    const NFTMarketplace = await ethers.getContractFactory('NFTMarketplace');
    marketplace = await NFTMarketplace.deploy(await wrfToken.getAddress());

    // Setup
    await marketplace.setAuthorizedNFTContract(await nft.getAddress(), true);

    // Mint WRF tokens for seller and approve spending
    await wrfToken.transfer(seller.address, ethers.parseEther('1000'));
    await wrfToken
      .connect(seller)
      .approve(await nft.getAddress(), ethers.parseEther('1000'));

    // Create a compute job which mints an NFT
    await nft.connect(seller).createComputeJob(
      0, // ModelType.WeatherForecast
      'WRF-GFS-Ensemble',
      '0x1234', // input data
      '{"cpu": 4, "memory": "8GB"}',
      3600, // 1 hour
      ethers.parseEther('100'), // stake 100 WRF
      'ipfs://test-metadata' // token URI path
    );

    const tokenId = 1;
    await nft.connect(seller).approve(await marketplace.getAddress(), tokenId);

    // Create a test listing
    listingId = await marketplace
      .connect(seller)
      .createFixedPriceListing.staticCall(
        await nft.getAddress(),
        tokenId,
        ethers.parseEther('1.0'), // 1 ETH
        0, // PaymentToken.Native
        ethers.ZeroAddress,
        86400, // 1 day
        false,
        []
      );

    await marketplace
      .connect(seller)
      .createFixedPriceListing(
        await nft.getAddress(),
        tokenId,
        ethers.parseEther('1.0'),
        0,
        ethers.ZeroAddress,
        86400,
        false,
        []
      );
  });

  describe('Critical Security Fixes', function () {
    it('CRITICAL FIX: Should prevent payment validation bypass in buyFixedPrice', async function () {
      // Attempt to buy 1 ETH NFT with only 1 wei - should fail
      await expect(
        marketplace.connect(buyer).buyFixedPrice(listingId, {
          value: 1, // 1 wei instead of 1 ETH
        })
      ).to.be.revertedWith('Marketplace: Insufficient payment');
    });

    it('CRITICAL FIX: Should require exact or higher payment for fixed price', async function () {
      // Should succeed with exact payment
      await expect(
        marketplace.connect(buyer).buyFixedPrice(listingId, {
          value: ethers.parseEther('1.0'),
        })
      ).to.not.be.reverted;
    });

    it('CRITICAL FIX: Should refund excess payment', async function () {
      const initialBalance = await ethers.provider.getBalance(buyer.address);

      // Pay more than required
      const tx = await marketplace.connect(buyer).buyFixedPrice(listingId, {
        value: ethers.parseEther('2.0'), // Pay 2 ETH for 1 ETH NFT
      });

      const receipt = await tx.wait();
      const gasUsed = receipt.gasUsed * receipt.gasPrice;

      // Check that excess is added to pending withdrawals
      const pendingWithdrawal = await marketplace.pendingWithdrawals(
        buyer.address
      );
      expect(pendingWithdrawal).to.equal(ethers.parseEther('1.0')); // 1 ETH excess
    });

    it('CRITICAL FIX: Should prevent Dutch auction payment bypass', async function () {
      // Create Dutch auction
      await nft.connect(seller).createComputeJob(
        0, // ModelType.WeatherForecast
        'WRF-GFS-Ensemble',
        '0x5678', // input data
        '{"cpu": 4, "memory": "8GB"}',
        3600, // 1 hour
        ethers.parseEther('100'), // stake 100 WRF
        'ipfs://test-metadata2' // token URI path
      );
      await nft.connect(seller).approve(await marketplace.getAddress(), 2);

      const dutchListingId = await marketplace
        .connect(seller)
        .createDutchAuction.staticCall(
          await nft.getAddress(),
          2,
          ethers.parseEther('2.0'), // Start at 2 ETH
          ethers.parseEther('0.5'), // End at 0.5 ETH
          3600, // 1 hour
          0, // PaymentToken.Native
          ethers.ZeroAddress
        );

      await marketplace
        .connect(seller)
        .createDutchAuction(
          await nft.getAddress(),
          2,
          ethers.parseEther('2.0'),
          ethers.parseEther('0.5'),
          3600,
          0,
          ethers.ZeroAddress
        );

      // Get current price (should be close to start price initially)
      const currentPrice =
        await marketplace.getCurrentDutchAuctionPrice(dutchListingId);

      // Attempt to buy with insufficient payment - should fail
      await expect(
        marketplace.connect(buyer).buyDutchAuction(dutchListingId, {
          value: 1, // 1 wei
        })
      ).to.be.revertedWith('Marketplace: Insufficient payment');
    });
  });

  describe('Reentrancy Protection Tests', function () {
    let auction;

    beforeEach(async function () {
      // Create auction for reentrancy tests
      await nft.connect(seller).createComputeJob(
        0, // ModelType.WeatherForecast
        'WRF-GFS-Ensemble',
        '0x9abc', // input data
        '{"cpu": 4, "memory": "8GB"}',
        3600, // 1 hour
        ethers.parseEther('100'), // stake 100 WRF
        'ipfs://test-metadata3' // token URI path
      );
      await nft.connect(seller).approve(await marketplace.getAddress(), 3);

      auction = await marketplace.connect(seller).createAuction.staticCall(
        await nft.getAddress(),
        3,
        ethers.parseEther('0.1'), // Start at 0.1 ETH
        ethers.parseEther('0.05'), // Reserve 0.05 ETH
        3600, // 1 hour
        0, // PaymentToken.Native
        ethers.ZeroAddress
      );

      await marketplace
        .connect(seller)
        .createAuction(
          await nft.getAddress(),
          3,
          ethers.parseEther('0.1'),
          ethers.parseEther('0.05'),
          3600,
          0,
          ethers.ZeroAddress
        );
    });

    it('CRITICAL FIX: Should follow CEI pattern in placeBid', async function () {
      // Place first bid
      await marketplace.connect(buyer).placeBid(auction, {
        value: ethers.parseEther('0.1'),
      });

      // Place second bid from attacker
      await marketplace.connect(attacker).placeBid(auction, {
        value: ethers.parseEther('0.11'),
      });

      // Check that buyer's refund is correctly queued
      const pendingWithdrawal = await marketplace.pendingWithdrawals(
        buyer.address
      );
      expect(pendingWithdrawal).to.equal(ethers.parseEther('0.1'));

      // Check that attacker is now highest bidder
      const highestBidder = await marketplace.highestBidder(auction);
      expect(highestBidder).to.equal(attacker.address);
    });

    it('Should prevent reentrancy during bid placement', async function () {
      // This test ensures state is updated before any external interactions
      await marketplace.connect(buyer).placeBid(auction, {
        value: ethers.parseEther('0.1'),
      });

      // Verify auction state is immediately updated
      expect(await marketplace.highestBidder(auction)).to.equal(buyer.address);
      expect(await marketplace.highestBid(auction)).to.equal(
        ethers.parseEther('0.1')
      );
    });
  });

  describe('Array Bounds Protection', function () {
    it('Should limit allowed buyers array', async function () {
      const tooManyBuyers = new Array(101).fill(buyer.address);

      await expect(
        marketplace
          .connect(seller)
          .createFixedPriceListing(
            await nft.getAddress(),
            1,
            ethers.parseEther('1.0'),
            0,
            ethers.ZeroAddress,
            86400,
            true,
            tooManyBuyers
          )
      ).to.be.revertedWith('Marketplace: Too many allowed buyers');
    });

    it('Should limit royalty recipients', async function () {
      const tooManyRecipients = new Array(6).fill(royaltyRecipient.address);
      const percentages = new Array(6).fill(100); // 1% each

      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(
            await nft.getAddress(),
            1,
            tooManyRecipients,
            percentages
          )
      ).to.be.revertedWith('Marketplace: Too many royalty recipients');
    });

    it('Should limit individual royalty percentage', async function () {
      await expect(
        marketplace.connect(seller).setRoyalties(
          await nft.getAddress(),
          1,
          [royaltyRecipient.address],
          [600] // 6% - above 5% limit
        )
      ).to.be.revertedWith('Marketplace: Individual royalty too high');
    });
  });

  describe('Enhanced Access Controls', function () {
    it('Should restrict royalty setting to token owner or authorized admin', async function () {
      // Attacker cannot set royalties for someone else's NFT
      await expect(
        marketplace
          .connect(attacker)
          .setRoyalties(
            await nft.getAddress(),
            1,
            [royaltyRecipient.address],
            [100]
          )
      ).to.be.revertedWith('Marketplace: Not authorized to set royalties');
    });

    it('Should allow owner to set royalties', async function () {
      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(
            await nft.getAddress(),
            1,
            [royaltyRecipient.address],
            [100]
          )
      ).to.not.be.reverted;
    });
  });

  describe('Complete Function Implementation', function () {
    it('Should implement withdrawToken function', async function () {
      // Transfer some WRF tokens to marketplace
      await wrfToken.transfer(
        await marketplace.getAddress(),
        ethers.parseEther('100')
      );

      // Only owner should be able to withdraw
      await expect(
        marketplace.connect(attacker).withdrawToken(await wrfToken.getAddress())
      ).to.be.revertedWith('Marketplace: Only owner can withdraw tokens');

      // Owner should be able to withdraw
      await expect(
        marketplace.connect(owner).withdrawToken(await wrfToken.getAddress())
      ).to.not.be.reverted;
    });

    it('Should validate token contract address', async function () {
      await expect(
        marketplace.connect(owner).withdrawToken(ethers.ZeroAddress)
      ).to.be.revertedWith('Marketplace: Invalid token contract');
    });
  });

  describe('Input Validation', function () {
    it('Should validate payment token types', async function () {
      // Should reject ETH for token-based listings
      await nft.connect(seller).createComputeJob(
        0, // ModelType.WeatherForecast
        'WRF-GFS-Ensemble',
        '0xdef0', // input data
        '{"cpu": 4, "memory": "8GB"}',
        3600, // 1 hour
        ethers.parseEther('100'), // stake 100 WRF
        'ipfs://test-metadata4' // token URI path
      );
      await nft.connect(seller).approve(await marketplace.getAddress(), 4);

      const tokenListingId = await marketplace
        .connect(seller)
        .createFixedPriceListing.staticCall(
          await nft.getAddress(),
          4,
          ethers.parseEther('1.0'),
          1, // PaymentToken.WRF
          await wrfToken.getAddress(),
          86400,
          false,
          []
        );

      await marketplace
        .connect(seller)
        .createFixedPriceListing(
          await nft.getAddress(),
          4,
          ethers.parseEther('1.0'),
          1,
          await wrfToken.getAddress(),
          86400,
          false,
          []
        );

      await expect(
        marketplace.connect(buyer).buyFixedPrice(tokenListingId, {
          value: ethers.parseEther('1.0'), // Sending ETH for token payment
        })
      ).to.be.revertedWith('Marketplace: ETH not accepted for token payments');
    });

    it('Should prevent zero address royalty recipients', async function () {
      await expect(
        marketplace
          .connect(seller)
          .setRoyalties(await nft.getAddress(), 1, [ethers.ZeroAddress], [100])
      ).to.be.revertedWith('Marketplace: Invalid recipient address');
    });
  });

  describe('Fee Calculation Protection', function () {
    it("Should ensure fees don't exceed sale price", async function () {
      // Set extremely high royalties that would exceed sale price
      await marketplace.connect(seller).setRoyalties(
        await nft.getAddress(),
        1,
        [royaltyRecipient.address, owner.address],
        [500, 500] // 5% + 5% = 10% royalties
      );

      // With marketplace fee (2.5%) + royalties (10%) = 12.5% total
      // This should still work for normal prices, but let's test edge case
      await expect(
        marketplace.connect(buyer).buyFixedPrice(listingId, {
          value: ethers.parseEther('1.0'),
        })
      ).to.not.be.reverted; // Should work as total is within reasonable bounds
    });
  });

  describe('Gas Optimization Validation', function () {
    it('Should efficiently handle array operations', async function () {
      // Test with maximum allowed buyers
      const maxBuyers = new Array(100).fill(buyer.address);

      const tx = await marketplace
        .connect(seller)
        .createFixedPriceListing(
          await nft.getAddress(),
          1,
          ethers.parseEther('1.0'),
          0,
          ethers.ZeroAddress,
          86400,
          true,
          maxBuyers
        );

      const receipt = await tx.wait();

      // Should complete within reasonable gas limits
      expect(receipt.gasUsed).to.be.below(1000000); // 1M gas limit
    });
  });
});
