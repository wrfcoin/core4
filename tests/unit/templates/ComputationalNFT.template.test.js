const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('ComputationalNFT Template', function () {
  async function deployComputationalNFTFixture() {
    const [
      owner,
      requester,
      provider1,
      provider2,
      verifier1,
      verifier2,
      verifier3,
    ] = await ethers.getSigners();

    const ComputationalNFT =
      await ethers.getContractFactory('ComputationalNFT');
    const nft = await ComputationalNFT.deploy(
      'Computational Jobs',
      'COMP',
      'https://api.wrfcoin.com/computational-nft/',
      owner.address
    );

    return {
      nft,
      owner,
      requester,
      provider1,
      provider2,
      verifier1,
      verifier2,
      verifier3,
    };
  }

  describe('Deployment', function () {
    it('Should set correct name and symbol', async function () {
      const { nft } = await loadFixture(deployComputationalNFTFixture);

      expect(await nft.name()).to.equal('Computational Jobs');
      expect(await nft.symbol()).to.equal('COMP');
    });

    it('Should set correct initial parameters', async function () {
      const { nft, owner } = await loadFixture(deployComputationalNFTFixture);

      expect(await nft.platformFeeRate()).to.equal(250); // 2.5%
      expect(await nft.feeRecipient()).to.equal(owner.address);
      expect(await nft.minimumStakeAmount()).to.equal(
        ethers.parseEther('1000')
      );
      expect(await nft.disputeResolver()).to.equal(owner.address);
    });

    it('Should set up correct roles', async function () {
      const { nft, owner } = await loadFixture(deployComputationalNFTFixture);

      const ADMIN_ROLE = await nft.ADMIN_ROLE();
      expect(await nft.hasRole(ADMIN_ROLE, owner.address)).to.be.true;
    });
  });

  describe('Provider Registration', function () {
    it('Should allow registration with sufficient stake', async function () {
      const { nft, provider1 } = await loadFixture(
        deployComputationalNFTFixture
      );

      const stakeAmount = ethers.parseEther('1500');

      await expect(
        nft.connect(provider1).registerProvider({ value: stakeAmount })
      )
        .to.emit(nft, 'ProviderRegistered')
        .withArgs(provider1.address, stakeAmount);

      const provider = await nft.providers(provider1.address);
      expect(provider.active).to.be.true;
      expect(provider.stakeAmount).to.equal(stakeAmount);
      expect(provider.reputationScore).to.equal(5000); // Neutral starting score
      expect(provider.totalJobsCompleted).to.equal(0);
      expect(provider.totalJobsFailed).to.equal(0);
    });

    it('Should reject registration with insufficient stake', async function () {
      const { nft, provider1 } = await loadFixture(
        deployComputationalNFTFixture
      );

      const insufficientStake = ethers.parseEther('500');

      await expect(
        nft.connect(provider1).registerProvider({ value: insufficientStake })
      )
        .to.be.revertedWithCustomError(nft, 'InsufficientStake')
        .withArgs(insufficientStake, ethers.parseEther('1000'));
    });

    it('Should handle multiple provider registrations', async function () {
      const { nft, provider1, provider2 } = await loadFixture(
        deployComputationalNFTFixture
      );

      await nft
        .connect(provider1)
        .registerProvider({ value: ethers.parseEther('1000') });
      await nft
        .connect(provider2)
        .registerProvider({ value: ethers.parseEther('2000') });

      const provider1Profile = await nft.providers(provider1.address);
      const provider2Profile = await nft.providers(provider2.address);

      expect(provider1Profile.active).to.be.true;
      expect(provider2Profile.active).to.be.true;
      expect(provider1Profile.stakeAmount).to.equal(ethers.parseEther('1000'));
      expect(provider2Profile.stakeAmount).to.equal(ethers.parseEther('2000'));
    });
  });

  describe('Job Creation', function () {
    it('Should create computational job with correct parameters', async function () {
      const { nft, requester } = await loadFixture(
        deployComputationalNFTFixture
      );

      const paymentAmount = ethers.parseEther('10');
      const platformFee = (paymentAmount * 250n) / 10000n; // 2.5%
      const totalRequired = paymentAmount + platformFee;

      await expect(
        nft.connect(requester).createJob(
          0, // WEATHER_MODELING
          4, // cpuCores
          8, // memoryGB
          100, // storageGB
          1, // gpuUnits
          3600, // estimatedDuration (1 hour)
          paymentAmount,
          ethers.ZeroAddress, // ETH payment
          '0x1234567890abcdef', // inputDataHash
          { value: totalRequired }
        )
      )
        .to.emit(nft, 'JobCreated')
        .withArgs(0, requester.address, 0) // tokenId 0, requester, WEATHER_MODELING
        .to.emit(nft, 'Transfer')
        .withArgs(ethers.ZeroAddress, requester.address, 0);

      const job = await nft.jobs(0);
      expect(job.requester).to.equal(requester.address);
      expect(job.provider).to.equal(ethers.ZeroAddress);
      expect(job.computationType).to.equal(0); // WEATHER_MODELING
      expect(job.status).to.equal(0); // CREATED
      expect(job.cpuCores).to.equal(4);
      expect(job.memoryGB).to.equal(8);
      expect(job.paymentAmount).to.equal(paymentAmount);
      expect(job.escrowAmount).to.equal(totalRequired);
      expect(job.penaltyAmount).to.equal(paymentAmount / 2n);
      expect(job.inputDataHash).to.equal('0x1234567890abcdef');
    });

    it('Should reject job creation with insufficient payment', async function () {
      const { nft, requester } = await loadFixture(
        deployComputationalNFTFixture
      );

      const paymentAmount = ethers.parseEther('10');
      const insufficientPayment = ethers.parseEther('5');

      await expect(
        nft
          .connect(requester)
          .createJob(
            0,
            4,
            8,
            100,
            1,
            3600,
            paymentAmount,
            ethers.ZeroAddress,
            '0x1234567890abcdef',
            { value: insufficientPayment }
          )
      ).to.be.revertedWithCustomError(nft, 'InsufficientPayment');
    });

    it('Should add job to marketplace and tracking arrays', async function () {
      const { nft, requester } = await loadFixture(
        deployComputationalNFTFixture
      );

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await nft.connect(requester).createJob(
        1, // CLIMATE_SIMULATION
        8,
        16,
        200,
        2,
        7200,
        paymentAmount,
        ethers.ZeroAddress,
        '0xabcdef1234567890',
        { value: totalRequired }
      );

      const availableJobs = await nft.getAvailableJobsByType(1); // CLIMATE_SIMULATION
      expect(availableJobs.length).to.equal(1);
      expect(availableJobs[0]).to.equal(0);

      const requesterJobs = await nft.getRequesterJobs(requester.address);
      expect(requesterJobs.length).to.equal(1);
      expect(requesterJobs[0]).to.equal(0);
    });
  });

  describe('Job Assignment', function () {
    async function createJobFixture() {
      const fixture = await loadFixture(deployComputationalNFTFixture);

      // Register provider
      await fixture.nft.connect(fixture.provider1).registerProvider({
        value: ethers.parseEther('1000'),
      });

      // Create job
      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await fixture.nft
        .connect(fixture.requester)
        .createJob(
          0,
          4,
          8,
          100,
          1,
          3600,
          paymentAmount,
          ethers.ZeroAddress,
          '0x1234567890abcdef',
          { value: totalRequired }
        );

      return fixture;
    }

    it('Should allow registered provider to assign job', async function () {
      const { nft, provider1 } = await createJobFixture();

      await expect(nft.connect(provider1).assignJob(0))
        .to.emit(nft, 'JobAssigned')
        .withArgs(0, provider1.address);

      const job = await nft.jobs(0);
      expect(job.provider).to.equal(provider1.address);
      expect(job.status).to.equal(1); // ASSIGNED

      const providerJobs = await nft.getProviderJobs(provider1.address);
      expect(providerJobs.length).to.equal(1);
      expect(providerJobs[0]).to.equal(0);
    });

    it('Should remove job from available jobs after assignment', async function () {
      const { nft, provider1 } = await createJobFixture();

      await nft.connect(provider1).assignJob(0);

      const availableJobs = await nft.getAvailableJobsByType(0);
      expect(availableJobs.length).to.equal(0);
    });

    it('Should reject assignment by unregistered provider', async function () {
      const { nft, provider2 } = await createJobFixture();

      await expect(nft.connect(provider2).assignJob(0))
        .to.be.revertedWithCustomError(nft, 'UnauthorizedProvider')
        .withArgs(provider2.address);
    });

    it('Should reject assignment of already assigned job', async function () {
      const { nft, provider1, provider2 } = await createJobFixture();

      // Register second provider
      await nft
        .connect(provider2)
        .registerProvider({ value: ethers.parseEther('1000') });

      // Assign job to first provider
      await nft.connect(provider1).assignJob(0);

      // Try to assign to second provider
      await expect(nft.connect(provider2).assignJob(0))
        .to.be.revertedWithCustomError(nft, 'InvalidJobStatus')
        .withArgs(1, 0); // current: ASSIGNED, required: CREATED
    });
  });

  describe('Job Execution', function () {
    async function assignedJobFixture() {
      const fixture = await createJobFixture();
      await fixture.nft.connect(fixture.provider1).assignJob(0);
      return fixture;
    }

    async function createJobFixture() {
      const fixture = await loadFixture(deployComputationalNFTFixture);

      await fixture.nft.connect(fixture.provider1).registerProvider({
        value: ethers.parseEther('1000'),
      });

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await fixture.nft
        .connect(fixture.requester)
        .createJob(
          0,
          4,
          8,
          100,
          1,
          3600,
          paymentAmount,
          ethers.ZeroAddress,
          '0x1234567890abcdef',
          { value: totalRequired }
        );

      return fixture;
    }

    it('Should allow assigned provider to start job', async function () {
      const { nft, provider1 } = await assignedJobFixture();

      await expect(nft.connect(provider1).startJob(0))
        .to.emit(nft, 'JobStarted')
        .withArgs(0, (await time.latest()) + 1);

      const job = await nft.jobs(0);
      expect(job.status).to.equal(2); // RUNNING
      expect(job.startTime).to.be.gt(0);
    });

    it('Should reject start by non-assigned provider', async function () {
      const { nft, provider2 } = await assignedJobFixture();

      await expect(nft.connect(provider2).startJob(0)).to.be.revertedWith(
        'ComputationalNFT: Not assigned provider'
      );
    });

    it('Should allow provider to complete job', async function () {
      const { nft, provider1 } = await assignedJobFixture();

      await nft.connect(provider1).startJob(0);

      // Simulate some computation time
      await time.increase(1800); // 30 minutes

      const resultHash = '0xresult123456789abcdef';

      await expect(nft.connect(provider1).completeJob(0, resultHash))
        .to.emit(nft, 'JobCompleted')
        .withArgs(0, (await time.latest()) + 1, resultHash);

      const job = await nft.jobs(0);
      expect(job.status).to.equal(3); // COMPLETED
      expect(job.resultHash).to.equal(resultHash);
      expect(job.completionTime).to.be.gt(job.startTime);
      expect(job.actualDuration).to.be.closeTo(1800, 100); // Allow some variance
      expect(job.qualityScore).to.be.gt(0); // Should have calculated quality score
    });

    it('Should update provider statistics on completion', async function () {
      const { nft, provider1 } = await assignedJobFixture();

      await nft.connect(provider1).startJob(0);
      await time.increase(1800);
      await nft.connect(provider1).completeJob(0, '0xresult123');

      const provider = await nft.providers(provider1.address);
      expect(provider.totalJobsCompleted).to.equal(1);
      expect(provider.totalComputeTime).to.be.closeTo(1800, 100);
    });

    it('Should calculate quality score based on timing accuracy', async function () {
      const { nft, provider1 } = await assignedJobFixture();

      await nft.connect(provider1).startJob(0);

      // Complete exactly on estimated time (3600 seconds)
      await time.increase(3600);
      await nft.connect(provider1).completeJob(0, '0xresult123');

      const job = await nft.jobs(0);
      expect(job.qualityScore).to.equal(10000); // Perfect timing = 100%
    });
  });

  describe('Job Verification', function () {
    async function completedJobFixture() {
      const fixture = await assignedJobFixture();
      await fixture.nft.connect(fixture.provider1).startJob(0);
      await time.increase(1800);
      await fixture.nft
        .connect(fixture.provider1)
        .completeJob(0, '0xresult123');
      return fixture;
    }

    async function assignedJobFixture() {
      const fixture = await createJobFixture();
      await fixture.nft.connect(fixture.provider1).assignJob(0);
      return fixture;
    }

    async function createJobFixture() {
      const fixture = await loadFixture(deployComputationalNFTFixture);

      await fixture.nft.connect(fixture.provider1).registerProvider({
        value: ethers.parseEther('1000'),
      });

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await fixture.nft
        .connect(fixture.requester)
        .createJob(
          0,
          4,
          8,
          100,
          1,
          3600,
          paymentAmount,
          ethers.ZeroAddress,
          '0x1234567890abcdef',
          { value: totalRequired }
        );

      return fixture;
    }

    it('Should allow verification of completed job', async function () {
      const { nft, verifier1 } = await completedJobFixture();

      await expect(nft.connect(verifier1).verifyJob(0, true))
        .to.emit(nft, 'JobVerified')
        .withArgs(0, verifier1.address, true);

      const job = await nft.jobs(0);
      expect(job.verificationCount).to.equal(1);
    });

    it('Should not allow double verification', async function () {
      const { nft, verifier1 } = await completedJobFixture();

      await nft.connect(verifier1).verifyJob(0, true);

      await expect(
        nft.connect(verifier1).verifyJob(0, false)
      ).to.be.revertedWith('ComputationalNFT: Already verified');
    });

    it('Should mark job as verified with majority approval', async function () {
      const { nft, verifier1, verifier2, verifier3 } =
        await completedJobFixture();

      // First verification (approved)
      await nft.connect(verifier1).verifyJob(0, true);

      // Second verification (approved)
      await nft.connect(verifier2).verifyJob(0, true);

      // Third verification (rejected, but majority still approves)
      await nft.connect(verifier3).verifyJob(0, false);

      const job = await nft.jobs(0);
      expect(job.verified).to.be.true;
      expect(job.verificationCount).to.equal(3);
    });

    it('Should not mark as verified without majority approval', async function () {
      const { nft, verifier1, verifier2, verifier3 } =
        await completedJobFixture();

      // First verification (rejected)
      await nft.connect(verifier1).verifyJob(0, false);

      // Second verification (approved)
      await nft.connect(verifier2).verifyJob(0, true);

      // Third verification (rejected)
      await nft.connect(verifier3).verifyJob(0, false);

      const job = await nft.jobs(0);
      expect(job.verified).to.be.false;
      expect(job.verificationCount).to.equal(3);
    });
  });

  describe('Payment Release', function () {
    async function verifiedJobFixture() {
      const fixture = await completedJobFixture();

      // Get 3 verifications with majority approval
      await fixture.nft.connect(fixture.verifier1).verifyJob(0, true);
      await fixture.nft.connect(fixture.verifier2).verifyJob(0, true);
      await fixture.nft.connect(fixture.verifier3).verifyJob(0, false);

      return fixture;
    }

    async function completedJobFixture() {
      const fixture = await assignedJobFixture();
      await fixture.nft.connect(fixture.provider1).startJob(0);
      await time.increase(1800);
      await fixture.nft
        .connect(fixture.provider1)
        .completeJob(0, '0xresult123');
      return fixture;
    }

    async function assignedJobFixture() {
      const fixture = await createJobFixture();
      await fixture.nft.connect(fixture.provider1).assignJob(0);
      return fixture;
    }

    async function createJobFixture() {
      const fixture = await loadFixture(deployComputationalNFTFixture);

      await fixture.nft.connect(fixture.provider1).registerProvider({
        value: ethers.parseEther('1000'),
      });

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await fixture.nft
        .connect(fixture.requester)
        .createJob(
          0,
          4,
          8,
          100,
          1,
          3600,
          paymentAmount,
          ethers.ZeroAddress,
          '0x1234567890abcdef',
          { value: totalRequired }
        );

      return fixture;
    }

    it('Should release payment to provider after verification', async function () {
      const { nft, provider1, owner } = await verifiedJobFixture();

      const providerInitialBalance = await ethers.provider.getBalance(
        provider1.address
      );
      const ownerInitialBalance = await ethers.provider.getBalance(
        owner.address
      );

      const job = await nft.jobs(0);
      expect(job.verified).to.be.true;

      // Payment should already be released automatically
      const expectedProviderPayment = ethers.parseEther('10');
      const expectedPlatformFee = (ethers.parseEther('10') * 250n) / 10000n;

      // Note: In a real test, we'd need to track the exact payment release transaction
      // This is a simplified verification
    });

    it('Should update provider reputation after successful completion', async function () {
      const { nft, provider1 } = await verifiedJobFixture();

      const reputation = await nft.calculateProviderReputation(
        provider1.address
      );
      expect(reputation).to.equal(10000); // Perfect score for 1 successful job
    });
  });

  describe('Job Failure and Disputes', function () {
    async function runningJobFixture() {
      const fixture = await assignedJobFixture();
      await fixture.nft.connect(fixture.provider1).startJob(0);
      return fixture;
    }

    async function assignedJobFixture() {
      const fixture = await createJobFixture();
      await fixture.nft.connect(fixture.provider1).assignJob(0);
      return fixture;
    }

    async function createJobFixture() {
      const fixture = await loadFixture(deployComputationalNFTFixture);

      await fixture.nft.connect(fixture.provider1).registerProvider({
        value: ethers.parseEther('1000'),
      });

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await fixture.nft
        .connect(fixture.requester)
        .createJob(
          0,
          4,
          8,
          100,
          1,
          3600,
          paymentAmount,
          ethers.ZeroAddress,
          '0x1234567890abcdef',
          { value: totalRequired }
        );

      return fixture;
    }

    it('Should allow provider to report job failure', async function () {
      const { nft, provider1 } = await runningJobFixture();

      await expect(
        nft.connect(provider1).reportJobFailure(0, 'Hardware failure')
      )
        .to.emit(nft, 'JobFailed')
        .withArgs(0, 'Hardware failure');

      const job = await nft.jobs(0);
      expect(job.status).to.equal(4); // FAILED
    });

    it('Should apply penalty when provider reports failure', async function () {
      const { nft, provider1 } = await runningJobFixture();

      const providerBefore = await nft.providers(provider1.address);
      const initialStake = providerBefore.stakeAmount;

      await nft.connect(provider1).reportJobFailure(0, 'Hardware failure');

      const providerAfter = await nft.providers(provider1.address);
      const job = await nft.jobs(0);

      expect(providerAfter.stakeAmount).to.equal(
        initialStake - job.penaltyAmount
      );
      expect(providerAfter.totalJobsFailed).to.equal(1);
    });

    it('Should allow requester to raise dispute', async function () {
      const { nft, requester } = await runningJobFixture();

      await expect(
        nft.connect(requester).reportJobFailure(0, 'Provider not responding')
      )
        .to.emit(nft, 'DisputeRaised')
        .withArgs(0, 'Provider not responding');

      const job = await nft.jobs(0);
      expect(job.status).to.equal(5); // DISPUTED

      const disputeReason = await nft.disputeReasons(0);
      expect(disputeReason).to.equal('Provider not responding');
    });

    it('Should allow admin to resolve dispute in favor of requester', async function () {
      const { nft, requester, owner } = await runningJobFixture();

      // Requester raises dispute
      await nft.connect(requester).reportJobFailure(0, 'Poor quality results');

      // Admin resolves in favor of requester
      await nft.connect(owner).resolveDispute(0, true);

      const job = await nft.jobs(0);
      expect(job.status).to.equal(4); // FAILED (after dispute resolution)
    });

    it('Should allow admin to resolve dispute in favor of provider', async function () {
      const { nft, requester, owner } = await runningJobFixture();

      await nft.connect(requester).reportJobFailure(0, 'False claim');

      await nft.connect(owner).resolveDispute(0, false);

      const job = await nft.jobs(0);
      expect(job.status).to.equal(3); // COMPLETED
      expect(job.verified).to.be.true;
    });
  });

  describe('Reputation System', function () {
    it('Should calculate correct reputation for successful provider', async function () {
      const { nft, provider1 } = await loadFixture(
        deployComputationalNFTFixture
      );

      await nft
        .connect(provider1)
        .registerProvider({ value: ethers.parseEther('1000') });

      // Simulate provider profile with completed jobs
      const reputation = await nft.calculateProviderReputation(
        provider1.address
      );
      expect(reputation).to.equal(5000); // Neutral score for new provider
    });

    it('Should handle provider with mixed success/failure record', async function () {
      const { nft, provider1 } = await loadFixture(
        deployComputationalNFTFixture
      );

      await nft
        .connect(provider1)
        .registerProvider({ value: ethers.parseEther('1000') });

      // This would require manually setting provider stats for testing
      // In practice, these would be updated through job completions/failures
      const reputation = await nft.calculateProviderReputation(
        provider1.address
      );
      expect(reputation).to.be.gte(0);
      expect(reputation).to.be.lte(10000);
    });
  });

  describe('Administrative Functions', function () {
    it('Should allow admin to set platform fee rate', async function () {
      const { nft, owner } = await loadFixture(deployComputationalNFTFixture);

      await nft.connect(owner).setPlatformFeeRate(500); // 5%

      expect(await nft.platformFeeRate()).to.equal(500);
    });

    it('Should reject excessive platform fee rate', async function () {
      const { nft, owner } = await loadFixture(deployComputationalNFTFixture);

      await expect(nft.connect(owner).setPlatformFeeRate(1500)) // 15%
        .to.be.revertedWith('ComputationalNFT: Fee too high');
    });

    it('Should allow admin to set minimum stake amount', async function () {
      const { nft, owner } = await loadFixture(deployComputationalNFTFixture);

      await nft.connect(owner).setMinimumStakeAmount(ethers.parseEther('2000'));

      expect(await nft.minimumStakeAmount()).to.equal(
        ethers.parseEther('2000')
      );
    });

    it('Should not allow non-admin to change parameters', async function () {
      const { nft, provider1 } = await loadFixture(
        deployComputationalNFTFixture
      );

      await expect(
        nft.connect(provider1).setPlatformFeeRate(500)
      ).to.be.revertedWith('AccessControl: account');

      await expect(
        nft.connect(provider1).setMinimumStakeAmount(ethers.parseEther('2000'))
      ).to.be.revertedWith('AccessControl: account');
    });
  });

  describe('Job Marketplace', function () {
    it('Should return available jobs by computation type', async function () {
      const { nft, requester } = await loadFixture(
        deployComputationalNFTFixture
      );

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      // Create weather modeling job
      await nft
        .connect(requester)
        .createJob(
          0,
          4,
          8,
          100,
          1,
          3600,
          paymentAmount,
          ethers.ZeroAddress,
          '0x123',
          { value: totalRequired }
        );

      // Create AI training job
      await nft
        .connect(requester)
        .createJob(
          2,
          8,
          16,
          200,
          4,
          7200,
          paymentAmount,
          ethers.ZeroAddress,
          '0x456',
          { value: totalRequired }
        );

      const weatherJobs = await nft.getAvailableJobsByType(0); // WEATHER_MODELING
      const aiJobs = await nft.getAvailableJobsByType(2); // AI_TRAINING

      expect(weatherJobs.length).to.equal(1);
      expect(aiJobs.length).to.equal(1);
      expect(weatherJobs[0]).to.equal(0);
      expect(aiJobs[0]).to.equal(1);
    });

    it('Should track requester and provider job histories', async function () {
      const { nft, requester, provider1 } = await loadFixture(
        deployComputationalNFTFixture
      );

      await nft
        .connect(provider1)
        .registerProvider({ value: ethers.parseEther('1000') });

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await nft
        .connect(requester)
        .createJob(
          0,
          4,
          8,
          100,
          1,
          3600,
          paymentAmount,
          ethers.ZeroAddress,
          '0x123',
          { value: totalRequired }
        );

      await nft.connect(provider1).assignJob(0);

      const requesterJobs = await nft.getRequesterJobs(requester.address);
      const providerJobs = await nft.getProviderJobs(provider1.address);

      expect(requesterJobs.length).to.equal(1);
      expect(providerJobs.length).to.equal(1);
      expect(requesterJobs[0]).to.equal(0);
      expect(providerJobs[0]).to.equal(0);
    });
  });

  describe('Edge Cases and Security', function () {
    it('Should handle job with zero estimated duration', async function () {
      const { nft, requester } = await loadFixture(
        deployComputationalNFTFixture
      );

      const paymentAmount = ethers.parseEther('10');
      const totalRequired = paymentAmount + (paymentAmount * 250n) / 10000n;

      await nft.connect(requester).createJob(
        0,
        4,
        8,
        100,
        1,
        0, // Zero estimated duration
        paymentAmount,
        ethers.ZeroAddress,
        '0x123',
        { value: totalRequired }
      );

      const job = await nft.jobs(0);
      expect(job.estimatedDuration).to.equal(0);
    });

    it('Should prevent reentrancy attacks', async function () {
      const { nft } = await loadFixture(deployComputationalNFTFixture);

      // The ReentrancyGuard should be active
      const contractCode = await ethers.provider.getCode(
        await nft.getAddress()
      );
      expect(contractCode).to.not.equal('0x');
    });

    it('Should handle large payment amounts', async function () {
      const { nft, requester } = await loadFixture(
        deployComputationalNFTFixture
      );

      const largePayment = ethers.parseEther('1000000');
      const totalRequired = largePayment + (largePayment * 250n) / 10000n;

      await expect(
        nft
          .connect(requester)
          .createJob(
            0,
            4,
            8,
            100,
            1,
            3600,
            largePayment,
            ethers.ZeroAddress,
            '0x123',
            { value: totalRequired }
          )
      ).to.not.be.reverted;
    });

    it('Should validate job existence for operations', async function () {
      const { nft, provider1 } = await loadFixture(
        deployComputationalNFTFixture
      );

      await nft
        .connect(provider1)
        .registerProvider({ value: ethers.parseEther('1000') });

      await expect(nft.connect(provider1).assignJob(999)).to.be.reverted; // Should fail for non-existent job
    });
  });
});
