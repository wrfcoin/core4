const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('SocialVerification', function () {
  // Deployment fixture
  async function deploySocialVerificationFixture() {
    const [owner, oracle, user1, user2, user3, admin] =
      await ethers.getSigners();

    // Deploy SocialVerification contract
    const SocialVerification =
      await ethers.getContractFactory('SocialVerification');
    const socialVerification = await SocialVerification.deploy();
    await socialVerification.waitForDeployment();

    // Setup roles
    const ORACLE_ROLE = await socialVerification.ORACLE_ROLE();
    const ADMIN_ROLE = await socialVerification.ADMIN_ROLE();

    await socialVerification.grantRole(ORACLE_ROLE, oracle.address);
    await socialVerification.grantRole(ADMIN_ROLE, admin.address);
    await socialVerification.setOracleAuthorization(oracle.address, true);

    // Platform enums
    const SocialPlatform = {
      GITHUB: 0,
      TWITTER: 1,
      LINKEDIN: 2,
    };

    const VerificationStatus = {
      PENDING: 0,
      VERIFIED: 1,
      EXPIRED: 2,
      REVOKED: 3,
      REJECTED: 4,
    };

    return {
      socialVerification,
      owner,
      oracle,
      user1,
      user2,
      user3,
      admin,
      SocialPlatform,
      VerificationStatus,
      ORACLE_ROLE,
      ADMIN_ROLE,
    };
  }

  describe('Deployment', function () {
    it('Should deploy with correct initial settings', async function () {
      const { socialVerification, owner } = await loadFixture(
        deploySocialVerificationFixture
      );

      // Check owner has admin role
      const ADMIN_ROLE = await socialVerification.ADMIN_ROLE();
      expect(await socialVerification.hasRole(ADMIN_ROLE, owner.address)).to.be
        .true;

      // Check platforms are enabled
      expect(await socialVerification.platformEnabled(0)).to.be.true; // GITHUB
      expect(await socialVerification.platformEnabled(1)).to.be.true; // TWITTER
      expect(await socialVerification.platformEnabled(2)).to.be.true; // LINKEDIN

      // Check default discount percentages
      expect(await socialVerification.platformDiscounts(0)).to.equal(100_000); // GitHub 10%
      expect(await socialVerification.platformDiscounts(1)).to.equal(50_000); // Twitter 5%
      expect(await socialVerification.platformDiscounts(2)).to.equal(70_000); // LinkedIn 7%

      // Check constants
      expect(await socialVerification.MAX_SOCIAL_DISCOUNT()).to.equal(200_000); // 20%
      expect(await socialVerification.VERIFICATION_VALIDITY_PERIOD()).to.equal(
        365 * 24 * 60 * 60
      ); // 1 year
    });

    it('Should have correct access control setup', async function () {
      const { socialVerification, owner, oracle, ORACLE_ROLE, ADMIN_ROLE } =
        await loadFixture(deploySocialVerificationFixture);

      expect(await socialVerification.hasRole(ADMIN_ROLE, owner.address)).to.be
        .true;
      expect(await socialVerification.hasRole(ORACLE_ROLE, oracle.address)).to
        .be.true;
    });
  });

  describe('Verification Submission', function () {
    it('Should allow users to submit verification requests', async function () {
      const { socialVerification, user1, SocialPlatform } = await loadFixture(
        deploySocialVerificationFixture
      );

      // Create identity fingerprint
      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      // Create GitHub verification proof data
      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [
          180 * 24 * 60 * 60, // accountAge (6 months)
          15, // publicRepositories
          100, // commitsLastYear
          500, // contributionsScore
          1, // tier (CONTRIBUTOR)
          false, // ecosystemContributor
          ['repo1', 'repo2', 'repo3'], // topRepositories
        ]
      );

      const signature = '0x' + '00'.repeat(65); // Dummy signature for testing

      await expect(
        socialVerification
          .connect(user1)
          .submitVerification(
            SocialPlatform.GITHUB,
            'testuser',
            identityFingerprint,
            githubData,
            signature
          )
      )
        .to.emit(socialVerification, 'VerificationSubmitted')
        .withArgs(
          user1.address,
          SocialPlatform.GITHUB,
          'testuser',
          ethers.keccak256(githubData),
          (await time.latest()) + 1 // Next block timestamp
        );

      // Check verification was created
      const verification = await socialVerification.verifications(
        user1.address,
        SocialPlatform.GITHUB
      );
      expect(verification.participant).to.equal(user1.address);
      expect(verification.platform).to.equal(SocialPlatform.GITHUB);
      expect(verification.profileIdentifier).to.equal('testuser');
      expect(verification.status).to.equal(0); // PENDING
    });

    it('Should prevent duplicate verification submissions', async function () {
      const { socialVerification, user1, SocialPlatform } = await loadFixture(
        deploySocialVerificationFixture
      );

      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      const signature = '0x' + '00'.repeat(65);

      // First submission should succeed
      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          signature
        );

      // Second submission should fail
      await expect(
        socialVerification.connect(user1).submitVerification(
          SocialPlatform.GITHUB,
          'testuser2',
          identityFingerprint,
          githubData,
          '0x' + '11'.repeat(65) // Different signature
        )
      ).to.be.revertedWithCustomError(
        socialVerification,
        'VerificationAlreadyExists'
      );
    });

    it('Should prevent duplicate identity fingerprints', async function () {
      const { socialVerification, user1, user2, SocialPlatform } =
        await loadFixture(deploySocialVerificationFixture);

      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('same@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      const signature1 = '0x' + '00'.repeat(65);
      const signature2 = '0x' + '11'.repeat(65);

      // First user submits verification
      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'user1',
          identityFingerprint,
          githubData,
          signature1
        );

      // Second user tries to submit with same identity fingerprint
      await expect(
        socialVerification
          .connect(user2)
          .submitVerification(
            SocialPlatform.GITHUB,
            'user2',
            identityFingerprint,
            githubData,
            signature2
          )
      ).to.be.revertedWithCustomError(socialVerification, 'DuplicateIdentity');
    });

    it('Should enforce rate limiting', async function () {
      const { socialVerification, user1, SocialPlatform } = await loadFixture(
        deploySocialVerificationFixture
      );

      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      const signature = '0x' + '00'.repeat(65);

      // First submission succeeds
      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          signature
        );

      // Immediate second submission should fail (different platform to avoid duplicate error)
      const identityFingerprint2 = {
        ...identityFingerprint,
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1+2@example.com')),
      };

      await expect(
        socialVerification
          .connect(user1)
          .submitVerification(
            SocialPlatform.TWITTER,
            'testuser',
            identityFingerprint2,
            githubData,
            '0x' + '11'.repeat(65)
          )
      ).to.be.revertedWithCustomError(socialVerification, 'RateLimitExceeded');
    });
  });

  describe('Oracle Verification', function () {
    async function setupPendingVerification() {
      const fixture = await loadFixture(deploySocialVerificationFixture);
      const { socialVerification, user1, SocialPlatform } = fixture;

      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      const signature = '0x' + '00'.repeat(65);

      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          signature
        );

      return fixture;
    }

    it('Should allow oracle to verify pending requests', async function () {
      const {
        socialVerification,
        oracle,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await setupPendingVerification();

      const discountPercentage = 80_000; // 8% discount (less than max 10%)
      const tier = 1; // CONTRIBUTOR tier

      await expect(
        socialVerification
          .connect(oracle)
          .verifyAccount(
            user1.address,
            SocialPlatform.GITHUB,
            VerificationStatus.VERIFIED,
            discountPercentage,
            tier
          )
      )
        .to.emit(socialVerification, 'VerificationCompleted')
        .withArgs(
          user1.address,
          SocialPlatform.GITHUB,
          VerificationStatus.VERIFIED,
          discountPercentage,
          (await time.latest()) + 1 + 365 * 24 * 60 * 60 // expiration timestamp
        );

      // Check verification was updated
      const verification = await socialVerification.verifications(
        user1.address,
        SocialPlatform.GITHUB
      );
      expect(verification.status).to.equal(VerificationStatus.VERIFIED);
      expect(verification.discountPercentage).to.equal(discountPercentage);

      // Check statistics were updated
      expect(await socialVerification.totalActiveVerifications()).to.equal(1);
    });

    it('Should reject verification if oracle marks as rejected', async function () {
      const {
        socialVerification,
        oracle,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await setupPendingVerification();

      await expect(
        socialVerification
          .connect(oracle)
          .verifyAccount(
            user1.address,
            SocialPlatform.GITHUB,
            VerificationStatus.REJECTED,
            0,
            0
          )
      )
        .to.emit(socialVerification, 'VerificationCompleted')
        .withArgs(
          user1.address,
          SocialPlatform.GITHUB,
          VerificationStatus.REJECTED,
          0,
          (await time.latest()) + 1 + 365 * 24 * 60 * 60
        );

      const verification = await socialVerification.verifications(
        user1.address,
        SocialPlatform.GITHUB
      );
      expect(verification.status).to.equal(VerificationStatus.REJECTED);
      expect(verification.discountPercentage).to.equal(0);
    });

    it('Should prevent unauthorized accounts from verifying', async function () {
      const {
        socialVerification,
        user2,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await setupPendingVerification();

      await expect(
        socialVerification
          .connect(user2)
          .verifyAccount(
            user1.address,
            SocialPlatform.GITHUB,
            VerificationStatus.VERIFIED,
            80_000,
            1
          )
      ).to.be.revertedWithCustomError(socialVerification, 'UnauthorizedOracle');
    });

    it('Should prevent excessive discount percentages', async function () {
      const {
        socialVerification,
        oracle,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await setupPendingVerification();

      const excessiveDiscount = 150_000; // 15% (more than GitHub max of 10%)

      await expect(
        socialVerification
          .connect(oracle)
          .verifyAccount(
            user1.address,
            SocialPlatform.GITHUB,
            VerificationStatus.VERIFIED,
            excessiveDiscount,
            1
          )
      ).to.be.revertedWithCustomError(
        socialVerification,
        'InvalidDiscountPercentage'
      );
    });

    it('Should support batch verification', async function () {
      const {
        socialVerification,
        oracle,
        user1,
        user2,
        SocialPlatform,
        VerificationStatus,
      } = await loadFixture(deploySocialVerificationFixture);

      // Setup two pending verifications
      const identityFingerprint1 = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1111111111')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device1')),
        behaviorFingerprint: ethers.keccak256(ethers.toUtf8Bytes('behavior1')),
        creationTimestamp: await time.latest(),
      };

      const identityFingerprint2 = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user2@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+2222222222')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device2')),
        behaviorFingerprint: ethers.keccak256(ethers.toUtf8Bytes('behavior2')),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'user1',
          identityFingerprint1,
          githubData,
          '0x' + '00'.repeat(65)
        );

      await time.increase(24 * 60 * 60 + 1); // Advance time to avoid rate limiting

      await socialVerification
        .connect(user2)
        .submitVerification(
          SocialPlatform.GITHUB,
          'user2',
          identityFingerprint2,
          githubData,
          '0x' + '11'.repeat(65)
        );

      // Batch verify both
      await socialVerification
        .connect(oracle)
        .batchVerifyAccounts(
          [user1.address, user2.address],
          [SocialPlatform.GITHUB, SocialPlatform.GITHUB],
          [VerificationStatus.VERIFIED, VerificationStatus.VERIFIED],
          [80_000, 90_000],
          [1, 1]
        );

      // Check both were verified
      const verification1 = await socialVerification.verifications(
        user1.address,
        SocialPlatform.GITHUB
      );
      const verification2 = await socialVerification.verifications(
        user2.address,
        SocialPlatform.GITHUB
      );

      expect(verification1.status).to.equal(VerificationStatus.VERIFIED);
      expect(verification2.status).to.equal(VerificationStatus.VERIFIED);
      expect(await socialVerification.totalActiveVerifications()).to.equal(2);
    });
  });

  describe('Discount Calculation', function () {
    async function setupMultiPlatformVerifications() {
      const fixture = await loadFixture(deploySocialVerificationFixture);
      const {
        socialVerification,
        oracle,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = fixture;

      // Create different identity fingerprints for each platform
      const githubFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('github_behavior')
        ),
        creationTimestamp: await time.latest(),
      };

      const twitterFingerprint = {
        emailHash: ethers.keccak256(
          ethers.toUtf8Bytes('user1+twitter@example.com')
        ),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567891')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device124')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('twitter_behavior')
        ),
        creationTimestamp: await time.latest(),
      };

      const linkedinFingerprint = {
        emailHash: ethers.keccak256(
          ethers.toUtf8Bytes('user1+linkedin@example.com')
        ),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567892')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device125')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('linkedin_behavior')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      const twitterData = ethers.AbiCoder.defaultAbiCoder().encode(
        ['bool', 'uint256', 'uint256', 'uint256', 'uint256', 'bool', 'string'],
        [
          true,
          365 * 24 * 60 * 60,
          2000,
          500,
          (await time.latest()) - 30 * 24 * 60 * 60,
          true,
          'https://twitter.com/user1',
        ]
      );

      const linkedinData = ethers.AbiCoder.defaultAbiCoder().encode(
        ['uint256', 'uint256', 'bool', 'bool', 'uint256', 'uint256', 'string'],
        [
          600,
          365 * 24 * 60 * 60,
          true,
          true,
          50,
          10,
          'https://linkedin.com/in/user1',
        ]
      );

      // Submit GitHub verification
      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'user1',
          githubFingerprint,
          githubData,
          '0x' + '00'.repeat(65)
        );

      await time.increase(24 * 60 * 60 + 1); // Wait for rate limiting

      // Submit Twitter verification
      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.TWITTER,
          'user1_twitter',
          twitterFingerprint,
          twitterData,
          '0x' + '11'.repeat(65)
        );

      await time.increase(24 * 60 * 60 + 1); // Wait for rate limiting

      // Submit LinkedIn verification
      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.LINKEDIN,
          'user1_linkedin',
          linkedinFingerprint,
          linkedinData,
          '0x' + '22'.repeat(65)
        );

      // Oracle verifies all platforms
      await socialVerification.connect(oracle).verifyAccount(
        user1.address,
        SocialPlatform.GITHUB,
        VerificationStatus.VERIFIED,
        100_000,
        1 // 10%
      );
      await socialVerification.connect(oracle).verifyAccount(
        user1.address,
        SocialPlatform.TWITTER,
        VerificationStatus.VERIFIED,
        50_000,
        0 // 5%
      );
      await socialVerification.connect(oracle).verifyAccount(
        user1.address,
        SocialPlatform.LINKEDIN,
        VerificationStatus.VERIFIED,
        70_000,
        0 // 7%
      );

      return fixture;
    }

    it('Should calculate combined discount correctly', async function () {
      const { socialVerification, user1 } =
        await setupMultiPlatformVerifications();

      const [totalDiscount, platformDiscounts, hasActiveVerifications] =
        await socialVerification.getCombinedDiscount(user1.address);

      expect(totalDiscount).to.equal(220_000); // 10% + 5% + 7% = 22%, but capped at 20%
      expect(totalDiscount).to.equal(200_000); // Should be capped at MAX_SOCIAL_DISCOUNT

      expect(platformDiscounts[0]).to.equal(100_000); // GitHub 10%
      expect(platformDiscounts[1]).to.equal(50_000); // Twitter 5%
      expect(platformDiscounts[2]).to.equal(70_000); // LinkedIn 7%
      expect(hasActiveVerifications).to.be.true;
    });

    it('Should return correct individual platform verification status', async function () {
      const { socialVerification, user1, SocialPlatform } =
        await setupMultiPlatformVerifications();

      // Check GitHub verification
      const [githubVerified, githubDiscount, githubExpiration] =
        await socialVerification.isVerified(
          user1.address,
          SocialPlatform.GITHUB
        );

      expect(githubVerified).to.be.true;
      expect(githubDiscount).to.equal(100_000);
      expect(githubExpiration).to.be.gt(await time.latest());

      // Check Twitter verification
      const [twitterVerified, twitterDiscount] =
        await socialVerification.isVerified(
          user1.address,
          SocialPlatform.TWITTER
        );

      expect(twitterVerified).to.be.true;
      expect(twitterDiscount).to.equal(50_000);

      // Check LinkedIn verification
      const [linkedinVerified, linkedinDiscount] =
        await socialVerification.isVerified(
          user1.address,
          SocialPlatform.LINKEDIN
        );

      expect(linkedinVerified).to.be.true;
      expect(linkedinDiscount).to.equal(70_000);
    });

    it('Should handle expired verifications correctly', async function () {
      const { socialVerification, user1, SocialPlatform } =
        await setupMultiPlatformVerifications();

      // Fast forward past expiration
      await time.increase(366 * 24 * 60 * 60); // More than 1 year

      // Check expired verifications
      const [githubVerified, githubDiscount] =
        await socialVerification.isVerified(
          user1.address,
          SocialPlatform.GITHUB
        );

      expect(githubVerified).to.be.false;
      expect(githubDiscount).to.equal(0);

      // Combined discount should be 0
      const [totalDiscount, , hasActiveVerifications] =
        await socialVerification.getCombinedDiscount(user1.address);

      expect(totalDiscount).to.equal(0);
      expect(hasActiveVerifications).to.be.false;
    });
  });

  describe('Verification Management', function () {
    it('Should allow renewal of verifications', async function () {
      const {
        socialVerification,
        oracle,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await loadFixture(deploySocialVerificationFixture);

      // Setup verified account
      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          '0x' + '00'.repeat(65)
        );

      await socialVerification
        .connect(oracle)
        .verifyAccount(
          user1.address,
          SocialPlatform.GITHUB,
          VerificationStatus.VERIFIED,
          90_000,
          1
        );

      // Fast forward to renewal period (within 30 days of expiration)
      await time.increase(340 * 24 * 60 * 60); // 340 days (within 30-day renewal window)

      // Create new proof data for renewal
      const newGithubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [545 * 24 * 60 * 60, 20, 200, 800, 2, false, ['repo1', 'repo2']] // Updated stats
      );

      await expect(
        socialVerification
          .connect(user1)
          .renewVerification(SocialPlatform.GITHUB, newGithubData)
      ).to.emit(socialVerification, 'VerificationSubmitted');

      // Check verification is back to pending status
      const verification = await socialVerification.verifications(
        user1.address,
        SocialPlatform.GITHUB
      );
      expect(verification.status).to.equal(VerificationStatus.PENDING);
    });

    it('Should allow admin to revoke verifications', async function () {
      const {
        socialVerification,
        oracle,
        admin,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await loadFixture(deploySocialVerificationFixture);

      // Setup verified account
      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          '0x' + '00'.repeat(65)
        );

      await socialVerification
        .connect(oracle)
        .verifyAccount(
          user1.address,
          SocialPlatform.GITHUB,
          VerificationStatus.VERIFIED,
          90_000,
          1
        );

      // Admin revokes verification
      await expect(
        socialVerification
          .connect(admin)
          .revokeVerification(
            user1.address,
            SocialPlatform.GITHUB,
            'Violation of terms'
          )
      )
        .to.emit(socialVerification, 'VerificationStatusChanged')
        .withArgs(
          user1.address,
          SocialPlatform.GITHUB,
          VerificationStatus.VERIFIED,
          VerificationStatus.REVOKED,
          'Violation of terms'
        );

      // Check verification was revoked
      const verification = await socialVerification.verifications(
        user1.address,
        SocialPlatform.GITHUB
      );
      expect(verification.status).to.equal(VerificationStatus.REVOKED);
      expect(verification.discountPercentage).to.equal(0);
    });

    it('Should handle verification expiration', async function () {
      const { socialVerification, SocialPlatform } = await loadFixture(
        deploySocialVerificationFixture
      );

      // This would test the checkAndExpireVerifications function
      // Implementation would depend on having test data with expired verifications
      const participants = [];
      const platforms = [];

      await socialVerification.checkAndExpireVerifications(
        participants,
        platforms
      );
      // No verifications to expire in this test
    });
  });

  describe('Rate Limiting', function () {
    it('Should track rate limiting correctly', async function () {
      const { socialVerification, user1 } = await loadFixture(
        deploySocialVerificationFixture
      );

      // Check initial rate limit status
      const [canVerify, cooldownRemaining, attemptsRemaining] =
        await socialVerification.checkRateLimit(user1.address);

      expect(canVerify).to.be.true;
      expect(cooldownRemaining).to.equal(0);
      expect(attemptsRemaining).to.equal(5); // MAX_VERIFICATION_ATTEMPTS

      // After a verification attempt, rate limiting should be active
      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          '0x' + '00'.repeat(65)
        );

      // Check rate limit status after verification
      const [canVerifyAfter, cooldownRemainingAfter, attemptsRemainingAfter] =
        await socialVerification.checkRateLimit(user1.address);

      expect(canVerifyAfter).to.be.false; // Should be blocked by cooldown
      expect(cooldownRemainingAfter).to.be.gt(0);
      expect(attemptsRemainingAfter).to.equal(4); // One attempt used
    });
  });

  describe('Integration Functions', function () {
    it('Should provide correct integration data for DynamicStakeCalculator', async function () {
      const {
        socialVerification,
        oracle,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await loadFixture(deploySocialVerificationFixture);

      // Setup GitHub verification
      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          '0x' + '00'.repeat(65)
        );

      await socialVerification
        .connect(oracle)
        .verifyAccount(
          user1.address,
          SocialPlatform.GITHUB,
          VerificationStatus.VERIFIED,
          100_000,
          1
        );

      // Test integration functions
      const discount = await socialVerification.getSocialVerificationDiscount(
        user1.address
      );
      expect(discount).to.equal(900_000); // 1e6 - 100_000 = 900_000 (90% multiplier)

      const hasGithub = await socialVerification.hasGitHubVerification(
        user1.address
      );
      expect(hasGithub).to.be.true;

      const hasTwitter = await socialVerification.hasTwitterVerification(
        user1.address
      );
      expect(hasTwitter).to.be.false;

      const hasLinkedIn = await socialVerification.hasLinkedInVerification(
        user1.address
      );
      expect(hasLinkedIn).to.be.false;
    });
  });

  describe('Administrative Functions', function () {
    it('Should allow admin to configure platforms', async function () {
      const { socialVerification, admin, SocialPlatform } = await loadFixture(
        deploySocialVerificationFixture
      );

      // Configure GitHub platform
      await expect(
        socialVerification.connect(admin).configurePlatform(
          SocialPlatform.GITHUB,
          true,
          120_000 // 12% discount
        )
      )
        .to.emit(socialVerification, 'PlatformConfigurationUpdated')
        .withArgs(SocialPlatform.GITHUB, true, 120_000);

      // Check configuration was updated
      expect(await socialVerification.platformEnabled(SocialPlatform.GITHUB)).to
        .be.true;
      expect(
        await socialVerification.platformDiscounts(SocialPlatform.GITHUB)
      ).to.equal(120_000);
    });

    it('Should allow admin to manage oracle authorization', async function () {
      const { socialVerification, admin, user2 } = await loadFixture(
        deploySocialVerificationFixture
      );

      // Authorize new oracle
      await expect(
        socialVerification
          .connect(admin)
          .setOracleAuthorization(user2.address, true)
      )
        .to.emit(socialVerification, 'OracleAuthorizationUpdated')
        .withArgs(user2.address, true, admin.address);

      // Check oracle was authorized
      const ORACLE_ROLE = await socialVerification.ORACLE_ROLE();
      expect(await socialVerification.hasRole(ORACLE_ROLE, user2.address)).to.be
        .true;
      expect(await socialVerification.oracleActive(user2.address)).to.be.true;

      // Deauthorize oracle
      await socialVerification
        .connect(admin)
        .setOracleAuthorization(user2.address, false);
      expect(await socialVerification.hasRole(ORACLE_ROLE, user2.address)).to.be
        .false;
      expect(await socialVerification.oracleActive(user2.address)).to.be.false;
    });

    it('Should support pause and unpause functionality', async function () {
      const { socialVerification, admin, user1, SocialPlatform } =
        await loadFixture(deploySocialVerificationFixture);

      // Pause contract
      await socialVerification.connect(admin).pause();

      // Verify contract is paused
      expect(await socialVerification.paused()).to.be.true;

      // Try to submit verification while paused (should fail)
      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      await expect(
        socialVerification
          .connect(user1)
          .submitVerification(
            SocialPlatform.GITHUB,
            'testuser',
            identityFingerprint,
            githubData,
            '0x' + '00'.repeat(65)
          )
      ).to.be.revertedWith('Pausable: paused');

      // Unpause contract
      await socialVerification.connect(admin).unpause();
      expect(await socialVerification.paused()).to.be.false;

      // Now verification should work
      await expect(
        socialVerification
          .connect(user1)
          .submitVerification(
            SocialPlatform.GITHUB,
            'testuser',
            identityFingerprint,
            githubData,
            '0x' + '00'.repeat(65)
          )
      ).to.emit(socialVerification, 'VerificationSubmitted');
    });
  });

  describe('Statistics and Reporting', function () {
    it('Should provide accurate verification statistics', async function () {
      const { socialVerification } = await loadFixture(
        deploySocialVerificationFixture
      );

      // Get initial stats
      const stats = await socialVerification.getVerificationStats();

      expect(stats[0]).to.equal(0); // totalVerifications
      expect(stats[1]).to.equal(0); // totalParticipants
      expect(stats[2]).to.equal(0); // totalActiveVerifications
      expect(stats[3]).to.equal(0); // GitHub verified count
      expect(stats[4]).to.equal(0); // Twitter verified count
      expect(stats[5]).to.equal(0); // LinkedIn verified count
      expect(stats[6]).to.be.gte(1); // authorized oracles count
      expect(stats[7]).to.equal(1); // GitHub platform enabled
      expect(stats[8]).to.equal(1); // Twitter platform enabled
      expect(stats[9]).to.equal(1); // LinkedIn platform enabled
      expect(stats[10]).to.equal(100_000); // GitHub discount
      expect(stats[11]).to.equal(200_000); // MAX_SOCIAL_DISCOUNT
    });

    it('Should return comprehensive verification info', async function () {
      const {
        socialVerification,
        oracle,
        user1,
        SocialPlatform,
        VerificationStatus,
      } = await loadFixture(deploySocialVerificationFixture);

      // Setup verification
      const identityFingerprint = {
        emailHash: ethers.keccak256(ethers.toUtf8Bytes('user1@example.com')),
        phoneHash: ethers.keccak256(ethers.toUtf8Bytes('+1234567890')),
        deviceFingerprint: ethers.keccak256(ethers.toUtf8Bytes('device123')),
        behaviorFingerprint: ethers.keccak256(
          ethers.toUtf8Bytes('behavior123')
        ),
        creationTimestamp: await time.latest(),
      };

      const githubData = ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'bool',
          'string[]',
        ],
        [180 * 24 * 60 * 60, 15, 100, 500, 1, false, ['repo1']]
      );

      await socialVerification
        .connect(user1)
        .submitVerification(
          SocialPlatform.GITHUB,
          'testuser',
          identityFingerprint,
          githubData,
          '0x' + '00'.repeat(65)
        );

      await socialVerification
        .connect(oracle)
        .verifyAccount(
          user1.address,
          SocialPlatform.GITHUB,
          VerificationStatus.VERIFIED,
          90_000,
          1
        );

      // Get verification info
      const [
        verificationData,
        totalActiveVerifications_,
        identityFingerprintReturned,
      ] = await socialVerification.getVerificationInfo(user1.address);

      expect(verificationData[0].participant).to.equal(user1.address);
      expect(verificationData[0].platform).to.equal(SocialPlatform.GITHUB);
      expect(verificationData[0].status).to.equal(VerificationStatus.VERIFIED);
      expect(verificationData[0].discountPercentage).to.equal(90_000);
      expect(totalActiveVerifications_).to.equal(1);
      expect(identityFingerprintReturned.emailHash).to.equal(
        identityFingerprint.emailHash
      );
    });
  });
});
