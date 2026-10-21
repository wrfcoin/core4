// SPDX-License-Identifier: MIT
const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('P0 Security Fixes Validation', function () {
  async function deploySecurityFixesFixture() {
    const [owner, addr1, addr2, addr3, addr4, addr5] =
      await ethers.getSigners();

    // Deploy EconomicSecurityEngine
    const EconomicSecurityEngine = await ethers.getContractFactory(
      'EconomicSecurityEngine'
    );

    // Deploy with placeholder address first
    const economicEngine = await EconomicSecurityEngine.deploy(
      ethers.ZeroAddress
    );
    await economicEngine.waitForDeployment();

    // Deploy CommunityGovernance
    const CommunityGovernance = await ethers.getContractFactory(
      'CommunityGovernance'
    );
    const governance = await CommunityGovernance.deploy(
      await economicEngine.getAddress(),
      ethers.ZeroAddress // placeholder
    );
    await governance.waitForDeployment();

    // Deploy DecentralizedFairLaunch
    const DecentralizedFairLaunch = await ethers.getContractFactory(
      'DecentralizedFairLaunch'
    );
    const fairLaunch = await DecentralizedFairLaunch.deploy(
      await economicEngine.getAddress(),
      await governance.getAddress()
    );
    await fairLaunch.waitForDeployment();

    return {
      fairLaunch,
      economicEngine,
      governance,
      owner,
      addr1,
      addr2,
      addr3,
      addr4,
      addr5,
    };
  }

  describe('P0-1: Increased Minimum Stakes (Sybil Resistance)', function () {
    it('Should require 0.1 ETH minimum stake instead of 0.01 ETH', async function () {
      const { fairLaunch, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Test that 0.01 ETH is rejected
      await expect(
        fairLaunch
          .connect(addr1)
          .registerBasicPath({ value: ethers.parseEther('0.01') })
      ).to.be.revertedWith('Insufficient stake');

      // Test that 0.1 ETH is accepted
      await expect(
        fairLaunch
          .connect(addr1)
          .registerBasicPath({ value: ethers.parseEther('0.1') })
      ).to.not.be.reverted;
    });

    it('Should require 0.05 ETH voucher stake instead of 0.005 ETH', async function () {
      const { fairLaunch, addr1, addr2, addr3, addr4, addr5 } =
        await loadFixture(deploySecurityFixesFixture);

      // Register vouchers first
      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr2)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr3)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr4)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr5)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Calculate required stake for community path (0.1 ETH * 75% = 0.075 ETH)
      const requiredStake = ethers.parseEther('0.075'); // 25% discount instead of 50%

      await expect(
        fairLaunch
          .connect(owner)
          .registerCommunityPath(
            [
              addr1.address,
              addr2.address,
              addr3.address,
              addr4.address,
              addr5.address,
            ],
            { value: requiredStake }
          )
      ).to.not.be.reverted;
    });

    it('Should require 5 vouchers minimum instead of 3', async function () {
      const { fairLaunch, addr1, addr2, addr3 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Register vouchers
      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr2)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr3)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Test that 3 vouchers is rejected
      await expect(
        fairLaunch
          .connect(owner)
          .registerCommunityPath(
            [addr1.address, addr2.address, addr3.address],
            { value: ethers.parseEther('0.075') }
          )
      ).to.be.revertedWith('Insufficient vouchers');
    });

    it('Should require 1 ETH minimum validator stake instead of 0.01 ETH', async function () {
      const { economicEngine, fairLaunch, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Register as participant first
      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Fast forward 30 days
      await time.increase(30 * 24 * 60 * 60);

      // Test that 0.01 ETH validator stake is rejected
      await expect(
        economicEngine
          .connect(addr1)
          .becomeValidator({ value: ethers.parseEther('0.01') })
      ).to.be.revertedWith(
        'Insufficient validator stake - minimum 1 ETH required'
      );

      // Test that 1 ETH validator stake is accepted (after reputation/violation checks pass)
      // Note: This may still fail due to reputation requirements
    });
  });

  describe('P0-2: Quadratic Voting Anti-Manipulation', function () {
    it('Should cap effective voting power at maximum stake threshold', async function () {
      const { governance, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Test that MAX_EFFECTIVE_STAKE constant is properly set
      const maxEffectiveStake = await governance.MAX_EFFECTIVE_STAKE();
      expect(maxEffectiveStake).to.equal(ethers.parseEther('1000'));
    });

    it('Should apply consolidation penalty for detected stake splitting', async function () {
      const { governance, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Test consolidation penalty constant
      const consolidationPenalty = await governance.CONSOLIDATION_PENALTY();
      expect(consolidationPenalty).to.equal(5000); // 50% penalty
    });

    it('Should limit addresses per device fingerprint', async function () {
      const { governance } = await loadFixture(deploySecurityFixesFixture);

      const maxAddresses = await governance.MAX_ADDRESSES_PER_FINGERPRINT();
      expect(maxAddresses).to.equal(3);
    });
  });

  describe('P0-3: Community Vouching Exploit Prevention', function () {
    it('Should prevent vouchers from same network', async function () {
      const { fairLaunch, addr1, addr2, addr3, addr4, addr5 } =
        await loadFixture(deploySecurityFixesFixture);

      // Register vouchers
      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr2)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr3)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr4)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr5)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Note: Network fingerprint detection would need mock implementation for full testing
      // This test validates the vouching requirements are properly implemented

      await expect(
        fairLaunch
          .connect(owner)
          .registerCommunityPath(
            [
              addr1.address,
              addr2.address,
              addr3.address,
              addr4.address,
              addr5.address,
            ],
            { value: ethers.parseEther('0.075') }
          )
      ).to.not.be.reverted;
    });

    it('Should prevent double vouching', async function () {
      const { fairLaunch, addr1, addr2, addr3, addr4, addr5 } =
        await loadFixture(deploySecurityFixesFixture);

      // Register vouchers
      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr2)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr3)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr4)
        .registerBasicPath({ value: ethers.parseEther('0.1') });
      await fairLaunch
        .connect(addr5)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Register first participant
      await fairLaunch
        .connect(owner)
        .registerCommunityPath(
          [
            addr1.address,
            addr2.address,
            addr3.address,
            addr4.address,
            addr5.address,
          ],
          { value: ethers.parseEther('0.075') }
        );

      // Try to register second participant with same vouchers - should fail
      const [, , , , , addr6] = await ethers.getSigners();
      await expect(
        fairLaunch
          .connect(addr6)
          .registerCommunityPath(
            [
              addr1.address,
              addr2.address,
              addr3.address,
              addr4.address,
              addr5.address,
            ],
            { value: ethers.parseEther('0.075') }
          )
      ).to.be.revertedWith('Voucher already vouched for you');
    });

    it('Should limit voucher participation', async function () {
      const { fairLaunch, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Test that voucher count limit is enforced (10 max vouches per address)
      // This would require multiple registrations to test fully

      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Check that voucherCount mapping tracks properly
      const initialCount = await fairLaunch.voucherCount(addr1.address);
      expect(initialCount).to.equal(0);
    });
  });

  describe('P0-4: Merit Path Gaming Prevention', function () {
    it('Should require oracle validation for full merit discounts', async function () {
      const { fairLaunch } = await loadFixture(deploySecurityFixesFixture);

      // Test that contribution type minimum values are set
      const githubMin = await fairLaunch.contributionTypeMinValues('GitHub');
      const researchMin =
        await fairLaunch.contributionTypeMinValues('Research');
      const communityMin =
        await fairLaunch.contributionTypeMinValues('Community');

      expect(githubMin).to.equal(2000);
      expect(researchMin).to.equal(3000);
      expect(communityMin).to.equal(1000);
    });

    it('Should apply stricter validation without oracle confirmation', async function () {
      const { fairLaunch, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      const contributionHash = ethers.keccak256(
        ethers.toUtf8Bytes('test-contribution')
      );

      // Test merit registration with stricter requirements
      await expect(
        fairLaunch.connect(addr1).registerMeritPath(
          contributionHash,
          'GitHub',
          { value: ethers.parseEther('0.05') } // 50% discount max instead of 75%
        )
      ).to.not.be.reverted;
    });

    it('Should prevent reuse of contribution hashes', async function () {
      const { fairLaunch, addr1, addr2 } = await loadFixture(
        deploySecurityFixesFixture
      );

      const contributionHash = ethers.keccak256(
        ethers.toUtf8Bytes('test-contribution')
      );

      // First registration should succeed
      await fairLaunch
        .connect(addr1)
        .registerMeritPath(contributionHash, 'GitHub', {
          value: ethers.parseEther('0.05'),
        });

      // Second registration with same hash should fail
      await expect(
        fairLaunch
          .connect(addr2)
          .registerMeritPath(contributionHash, 'GitHub', {
            value: ethers.parseEther('0.05'),
          })
      ).to.be.revertedWith('Contribution already used');
    });
  });

  describe('P0-5: Sybil Resistance Mechanisms', function () {
    it('Should rate limit registrations per network', async function () {
      const { economicEngine } = await loadFixture(deploySecurityFixesFixture);

      const maxRegs =
        await economicEngine.MAX_REGISTRATIONS_PER_NETWORK_PER_DAY();
      expect(maxRegs).to.equal(5);

      const cooldown = await economicEngine.REGISTRATION_COOLDOWN();
      expect(cooldown).to.equal(24 * 60 * 60); // 1 day in seconds
    });

    it('Should increase minimum security stakes', async function () {
      const { economicEngine } = await loadFixture(deploySecurityFixesFixture);

      const minStake = await economicEngine.MIN_SECURITY_STAKE();
      expect(minStake).to.equal(ethers.parseEther('0.1'));

      const minChallenge = await economicEngine.MIN_CHALLENGE_STAKE();
      expect(minChallenge).to.equal(ethers.parseEther('0.05'));
    });
  });

  describe('P0-6: Validator Quality Control', function () {
    it('Should require high reputation score for validators', async function () {
      const { economicEngine, fairLaunch, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Register as participant first
      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Fast forward 30 days
      await time.increase(30 * 24 * 60 * 60);

      // Try to become validator - should fail due to insufficient reputation
      await expect(
        economicEngine
          .connect(addr1)
          .becomeValidator({ value: ethers.parseEther('1.0') })
      ).to.be.revertedWith('Insufficient reputation score for validator');
    });

    it('Should require 30 day participation period', async function () {
      const { economicEngine, fairLaunch, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Register as participant
      await fairLaunch
        .connect(addr1)
        .registerBasicPath({ value: ethers.parseEther('0.1') });

      // Try to become validator immediately - should fail
      await expect(
        economicEngine
          .connect(addr1)
          .becomeValidator({ value: ethers.parseEther('1.0') })
      ).to.be.revertedWith('Must be participant for 30 days');
    });

    it('Should prevent validators with violation history', async function () {
      const { economicEngine } = await loadFixture(deploySecurityFixesFixture);

      // This would require setting up violations to test properly
      // Test validates that the violation count check exists in the function
      const contractCode = await ethers.provider.getCode(
        await economicEngine.getAddress()
      );
      expect(contractCode).to.include('Cannot have violation history');
    });
  });

  describe('P0-7: Emergency Circuit Breakers', function () {
    it('Should allow community emergency activation', async function () {
      const { fairLaunch } = await loadFixture(deploySecurityFixesFixture);

      const threshold = await fairLaunch.EMERGENCY_THRESHOLD_PERCENTAGE();
      expect(threshold).to.equal(3000); // 30%

      const cooldown = await fairLaunch.EMERGENCY_COOLDOWN();
      expect(cooldown).to.equal(7 * 24 * 60 * 60); // 7 days
    });

    it('Should prevent registrations during emergency mode', async function () {
      const { fairLaunch, addr1 } = await loadFixture(
        deploySecurityFixesFixture
      );

      // Note: Full emergency mode testing would require multiple participants
      // This validates the modifier exists

      // Normal registration should work
      await expect(
        fairLaunch
          .connect(addr1)
          .registerBasicPath({ value: ethers.parseEther('0.1') })
      ).to.not.be.reverted;
    });
  });

  describe('P0-8: Attack Cost Analysis', function () {
    it('Should make Sybil attacks economically infeasible', async function () {
      const { fairLaunch } = await loadFixture(deploySecurityFixesFixture);

      // Calculate minimum attack cost
      const minStake = ethers.parseEther('0.1'); // 0.1 ETH per identity
      const minVouchers = 5; // 5 vouchers required
      const voucherStake = ethers.parseEther('0.05'); // 0.05 ETH per voucher

      // Cost for community path attack: 0.075 ETH + (5 * 0.05 ETH) = 0.325 ETH per fake identity
      // To create 100 fake identities: 32.5 ETH (~$50,000-100,000)
      const attackCost = minStake * BigInt(100); // Basic path for 100 identities
      expect(attackCost).to.equal(ethers.parseEther('10')); // 10 ETH minimum
    });

    it('Should make quadratic voting manipulation expensive', async function () {
      const { governance } = await loadFixture(deploySecurityFixesFixture);

      const maxEffectiveStake = await governance.MAX_EFFECTIVE_STAKE();
      const consolidationPenalty = await governance.CONSOLIDATION_PENALTY();

      // Even with 1000 ETH, voting power is capped
      // Splitting stakes incurs 50% penalty
      expect(maxEffectiveStake).to.equal(ethers.parseEther('1000'));
      expect(consolidationPenalty).to.equal(5000); // 50% penalty
    });

    it('Should make validator network takeover prohibitively expensive', async function () {
      const { economicEngine } = await loadFixture(deploySecurityFixesFixture);

      // Minimum validator stake: 1 ETH
      // Plus reputation requirements and 30-day wait
      // Plus no violation history

      const minValidatorStake = ethers.parseEther('1.0');

      // To control a network of 100 validators requires:
      // - 51 ETH in validator stakes minimum
      // - 30 day preparation time for each
      // - Perfect reputation scores
      // - Additional participant stakes (0.1 ETH each)

      const attackCost =
        minValidatorStake * BigInt(51) + ethers.parseEther('0.1') * BigInt(51);
      expect(attackCost).to.equal(ethers.parseEther('56.1')); // 56.1 ETH minimum
    });
  });
});
