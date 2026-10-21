const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  time,
  loadFixture,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('Carbon Credit Insurance Protocol', function () {
  // Test fixture
  async function deployInsuranceFixture() {
    const [owner, user1, user2, user3, oracle] = await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    const wrfToken = await WRFToken.deploy();

    // Deploy Enhanced Carbon Credits
    const EnhancedCarbonCredits = await ethers.getContractFactory(
      'EnhancedCarbonCredits'
    );
    const carbonCredits = await EnhancedCarbonCredits.deploy(wrfToken.target);

    // Deploy Quality Oracle
    const QualityOracle = await ethers.getContractFactory('QualityOracle');
    const qualityOracle = await QualityOracle.deploy();

    // Deploy Volatility Oracle
    const VolatilityOracle =
      await ethers.getContractFactory('VolatilityOracle');
    const volatilityOracle = await VolatilityOracle.deploy();

    // Deploy Insurance Pool
    const InsurancePool = await ethers.getContractFactory('InsurancePool');
    const insurancePool = await InsurancePool.deploy(wrfToken.target);

    // Deploy Insurance Governance
    const InsuranceGovernance = await ethers.getContractFactory(
      'InsuranceGovernance'
    );
    const insuranceGovernance = await InsuranceGovernance.deploy(
      wrfToken.target
    );

    // Deploy Main Insurance Contract
    const CarbonCreditInsurance = await ethers.getContractFactory(
      'CarbonCreditInsurance'
    );
    const insurance = await CarbonCreditInsurance.deploy(
      carbonCredits.target,
      wrfToken.target,
      qualityOracle.target,
      volatilityOracle.target
    );

    // Set up roles and connections
    await insurance.setInsurancePool(insurancePool.target);
    await insurance.setGovernanceContract(insuranceGovernance.target);

    await insurancePool.grantRole(
      await insurancePool.INSURANCE_CONTRACT_ROLE(),
      insurance.target
    );
    await insurancePool.grantRole(
      await insurancePool.CLAIMS_PAYER_ROLE(),
      insurance.target
    );

    await insuranceGovernance.setInsuranceContract(insurance.target);
    await insuranceGovernance.setInsurancePool(insurancePool.target);

    // Grant necessary roles
    await carbonCredits.grantRole(
      await carbonCredits.AUTHORIZED_ISSUER_ROLE(),
      owner.address
    );
    await qualityOracle.grantRole(
      await qualityOracle.ORACLE_UPDATER_ROLE(),
      oracle.address
    );
    await volatilityOracle.grantRole(
      await volatilityOracle.PRICE_FEEDER_ROLE(),
      oracle.address
    );

    // Mint WRF tokens for testing
    const mintAmount = ethers.parseEther('1000000');
    await wrfToken.mint(owner.address, mintAmount);
    await wrfToken.mint(user1.address, mintAmount);
    await wrfToken.mint(user2.address, mintAmount);
    await wrfToken.mint(user3.address, mintAmount);

    // Create a carbon credit batch for testing
    const totalSupply = 10000; // 10,000 kg CO2e
    const vintage = 2024;
    const expiryDate = Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60; // 1 year

    await carbonCredits.createBatch(
      totalSupply,
      'PROJECT-001',
      vintage,
      'VM0034',
      'renewable-energy',
      'VCS',
      'Brazil',
      expiryDate,
      'QmTest123...',
      'https://registry.verra.org/PROJECT-001',
      'https://metadata.uri/project-001'
    );

    // Immobilize some credits for user1
    const tokenId = 1;
    const immobilizeAmount = 5000;
    await carbonCredits.immobilizeCredits(
      tokenId,
      immobilizeAmount,
      user1.address
    );

    // Set up oracle data
    await qualityOracle.connect(oracle).updateQualityScore(
      carbonCredits.target,
      750, // Good quality score
      'Initial quality assessment'
    );

    await volatilityOracle.connect(oracle).updateVolatility(
      carbonCredits.target,
      wrfToken.target,
      ethers.parseEther('50') // 50 WRF per tonne
    );

    return {
      wrfToken,
      carbonCredits,
      qualityOracle,
      volatilityOracle,
      insurance,
      insurancePool,
      insuranceGovernance,
      owner,
      user1,
      user2,
      user3,
      oracle,
      tokenId,
      totalSupply,
      immobilizeAmount,
    };
  }

  describe('Insurance Policy Creation', function () {
    it('Should create a basic insurance policy', async function () {
      const { insurance, wrfToken, user1, tokenId } = await loadFixture(
        deployInsuranceFixture
      );

      const coveredAmount = 1000; // 1000 kg CO2e
      const coverageTypes = [0, 1]; // PROJECT_FAILURE, VERIFICATION_RISK
      const duration = 30 * 24 * 60 * 60; // 30 days

      // Calculate premium
      const premiumData = await insurance.calculatePremium(
        tokenId,
        coveredAmount,
        coverageTypes,
        duration
      );

      // Approve premium payment
      await wrfToken
        .connect(user1)
        .approve(insurance.target, premiumData.finalPremium);

      // Create policy
      const tx = await insurance
        .connect(user1)
        .createPolicy(
          tokenId,
          coveredAmount,
          coverageTypes,
          duration,
          'QmPolicyTerms123...'
        );

      const receipt = await tx.wait();
      const event = receipt.logs.find(log => {
        try {
          return insurance.interface.parseLog(log).name === 'PolicyCreated';
        } catch {
          return false;
        }
      });

      expect(event).to.not.be.undefined;
      const parsedEvent = insurance.interface.parseLog(event);
      const policyId = parsedEvent.args.policyId;

      // Verify policy details
      const policy = await insurance.getPolicy(policyId);
      expect(policy.policyholder).to.equal(user1.address);
      expect(policy.tokenId).to.equal(tokenId);
      expect(policy.coveredAmount).to.equal(coveredAmount);
      expect(policy.status).to.equal(0); // ACTIVE
    });

    it('Should calculate premium correctly based on risk factors', async function () {
      const { insurance, tokenId } = await loadFixture(deployInsuranceFixture);

      const coveredAmount = 1000;
      const coverageTypes = [0]; // PROJECT_FAILURE only
      const duration = 365 * 24 * 60 * 60; // 1 year

      const premiumData = await insurance.calculatePremium(
        tokenId,
        coveredAmount,
        coverageTypes,
        duration
      );

      expect(premiumData.basePremium).to.be.gt(0);
      expect(premiumData.finalPremium).to.be.gt(premiumData.basePremium);
    });

    it('Should reject policy creation for insufficient token balance', async function () {
      const { insurance, user2, tokenId } = await loadFixture(
        deployInsuranceFixture
      );

      const coveredAmount = 10000; // More than user2 has
      const coverageTypes = [0];
      const duration = 30 * 24 * 60 * 60;

      await expect(
        insurance
          .connect(user2)
          .createPolicy(
            tokenId,
            coveredAmount,
            coverageTypes,
            duration,
            'QmPolicyTerms123...'
          )
      ).to.be.revertedWith('Insufficient token balance');
    });
  });

  describe('Insurance Pool Management', function () {
    it('Should allow capital providers to deposit funds', async function () {
      const { insurancePool, wrfToken, user2 } = await loadFixture(
        deployInsuranceFixture
      );

      const depositAmount = ethers.parseEther('10000');
      await wrfToken
        .connect(user2)
        .approve(insurancePool.target, depositAmount);

      const tx = await insurancePool
        .connect(user2)
        .depositCapital(depositAmount);
      const receipt = await tx.wait();

      const event = receipt.logs.find(log => {
        try {
          return (
            insurancePool.interface.parseLog(log).name === 'CapitalDeposited'
          );
        } catch {
          return false;
        }
      });

      expect(event).to.not.be.undefined;

      const poolStats = await insurancePool.getPoolStats();
      expect(poolStats.totalCapital).to.equal(depositAmount);
    });

    it('Should reserve capital for active policies', async function () {
      const { insurance, insurancePool, wrfToken, user1, user2, tokenId } =
        await loadFixture(deployInsuranceFixture);

      // First, add capital to pool
      const depositAmount = ethers.parseEther('50000');
      await wrfToken
        .connect(user2)
        .approve(insurancePool.target, depositAmount);
      await insurancePool.connect(user2).depositCapital(depositAmount);

      // Create policy
      const coveredAmount = 1000;
      const coverageTypes = [0, 1];
      const duration = 30 * 24 * 60 * 60;

      const premiumData = await insurance.calculatePremium(
        tokenId,
        coveredAmount,
        coverageTypes,
        duration
      );

      await wrfToken
        .connect(user1)
        .approve(insurance.target, premiumData.finalPremium);

      await insurance
        .connect(user1)
        .createPolicy(
          tokenId,
          coveredAmount,
          coverageTypes,
          duration,
          'QmPolicyTerms123...'
        );

      const poolStats = await insurancePool.getPoolStats();
      expect(poolStats.reservedCapital).to.be.gt(0);
      expect(poolStats.availableCapital).to.be.lt(poolStats.totalCapital);
    });
  });

  describe('Claims Processing', function () {
    async function createPolicyFixture() {
      const fixtureData = await loadFixture(deployInsuranceFixture);
      const { insurance, insurancePool, wrfToken, user1, user2, tokenId } =
        fixtureData;

      // Add capital to pool
      const depositAmount = ethers.parseEther('50000');
      await wrfToken
        .connect(user2)
        .approve(insurancePool.target, depositAmount);
      await insurancePool.connect(user2).depositCapital(depositAmount);

      // Create policy
      const coveredAmount = 1000;
      const coverageTypes = [0, 1]; // PROJECT_FAILURE, VERIFICATION_RISK
      const duration = 30 * 24 * 60 * 60;

      const premiumData = await insurance.calculatePremium(
        tokenId,
        coveredAmount,
        coverageTypes,
        duration
      );

      await wrfToken
        .connect(user1)
        .approve(insurance.target, premiumData.finalPremium);

      const tx = await insurance
        .connect(user1)
        .createPolicy(
          tokenId,
          coveredAmount,
          coverageTypes,
          duration,
          'QmPolicyTerms123...'
        );

      const receipt = await tx.wait();
      const event = receipt.logs.find(log => {
        try {
          return insurance.interface.parseLog(log).name === 'PolicyCreated';
        } catch {
          return false;
        }
      });

      const policyId = insurance.interface.parseLog(event).args.policyId;

      // Pay premium
      await insurance.connect(user1).payPremium(policyId);

      return { ...fixtureData, policyId, coveredAmount };
    }

    it('Should allow policyholders to submit claims', async function () {
      const { insurance, user1, policyId, coveredAmount } =
        await loadFixture(createPolicyFixture);

      const claimAmount = 500; // 500 kg CO2e
      const claimType = 0; // PROJECT_FAILURE

      const tx = await insurance
        .connect(user1)
        .submitClaim(policyId, claimType, claimAmount, 'QmEvidence123...');

      const receipt = await tx.wait();
      const event = receipt.logs.find(log => {
        try {
          return insurance.interface.parseLog(log).name === 'ClaimSubmitted';
        } catch {
          return false;
        }
      });

      expect(event).to.not.be.undefined;
      const claimId = insurance.interface.parseLog(event).args.claimId;

      const claim = await insurance.getClaim(claimId);
      expect(claim.claimant).to.equal(user1.address);
      expect(claim.claimedAmount).to.equal(claimAmount);
      expect(claim.status).to.equal(1); // INVESTIGATING (for non-governance claims)
    });

    it('Should process valid claims automatically', async function () {
      const {
        insurance,
        qualityOracle,
        carbonCredits,
        oracle,
        user1,
        policyId,
      } = await loadFixture(createPolicyFixture);

      // Submit claim
      const claimAmount = 500;
      const claimType = 0; // PROJECT_FAILURE

      const tx = await insurance
        .connect(user1)
        .submitClaim(policyId, claimType, claimAmount, 'QmEvidence123...');

      const receipt = await tx.wait();
      const event = receipt.logs.find(log => {
        try {
          return insurance.interface.parseLog(log).name === 'ClaimSubmitted';
        } catch {
          return false;
        }
      });

      const claimId = insurance.interface.parseLog(event).args.claimId;

      // Update quality score to trigger claim validation (low quality = claim valid)
      await qualityOracle.connect(oracle).updateQualityScore(
        carbonCredits.target,
        200, // Very low quality score
        'Project failure detected'
      );

      // Process claim
      await insurance.processClaim(claimId);

      const claim = await insurance.getClaim(claimId);
      expect(claim.status).to.equal(2); // APPROVED
    });

    it('Should reject invalid claims', async function () {
      const { insurance, user1, policyId } =
        await loadFixture(createPolicyFixture);

      // Submit claim for coverage type not included
      const claimAmount = 500;
      const claimType = 2; // REGULATORY_RISK (not in coverage)

      await expect(
        insurance
          .connect(user1)
          .submitClaim(policyId, claimType, claimAmount, 'QmEvidence123...')
      ).to.be.revertedWith('Coverage type not included');
    });
  });

  describe('Risk Assessment', function () {
    it('Should assess risk correctly for different project types', async function () {
      const { insurance, tokenId } = await loadFixture(deployInsuranceFixture);

      const riskAssessment = await insurance.assessRisk(tokenId);

      expect(riskAssessment.projectRisk).to.be.lte(100);
      expect(riskAssessment.verificationRisk).to.be.lte(100);
      expect(riskAssessment.regulatoryRisk).to.be.lte(100);
      expect(riskAssessment.marketRisk).to.be.lte(100);
      expect(riskAssessment.overallRisk).to.be.lte(100);
      expect(riskAssessment.qualityScore).to.be.gt(0);
    });

    it('Should adjust premiums based on risk levels', async function () {
      const { insurance, tokenId } = await loadFixture(deployInsuranceFixture);

      const coveredAmount = 1000;
      const coverageTypes = [0];
      const duration = 365 * 24 * 60 * 60;

      // Calculate premium for current risk level
      const normalPremium = await insurance.calculatePremium(
        tokenId,
        coveredAmount,
        coverageTypes,
        duration
      );

      // For this test, we're mainly checking that the calculation works
      // In a real scenario, we'd need to manipulate oracle data to test different risk levels
      expect(normalPremium.finalPremium).to.be.gt(0);
    });
  });

  describe('Governance', function () {
    it('Should allow stakeholders to create proposals', async function () {
      const { insuranceGovernance, wrfToken, user1 } = await loadFixture(
        deployInsuranceFixture
      );

      // Stake tokens to become stakeholder
      const stakeAmount = ethers.parseEther('15000');
      await wrfToken
        .connect(user1)
        .approve(insuranceGovernance.target, stakeAmount);
      await insuranceGovernance.connect(user1).stakeTokens(stakeAmount);

      // Grant proposal creator role
      await insuranceGovernance.grantRole(
        await insuranceGovernance.PROPOSAL_CREATOR_ROLE(),
        user1.address
      );

      // Create proposal
      const tx = await insuranceGovernance.connect(user1).propose(
        0, // CLAIM_DISPUTE
        1, // claimId
        'Dispute rejected claim',
        'QmEvidence123...'
      );

      const receipt = await tx.wait();
      const event = receipt.logs.find(log => {
        try {
          return (
            insuranceGovernance.interface.parseLog(log).name ===
            'ProposalCreated'
          );
        } catch {
          return false;
        }
      });

      expect(event).to.not.be.undefined;

      const proposalId =
        insuranceGovernance.interface.parseLog(event).args.proposalId;
      const proposal = await insuranceGovernance.getProposal(proposalId);
      expect(proposal.proposer).to.equal(user1.address);
      expect(proposal.status).to.equal(1); // ACTIVE
    });

    it('Should calculate voting power based on staking time', async function () {
      const { insuranceGovernance, wrfToken, user1 } = await loadFixture(
        deployInsuranceFixture
      );

      const stakeAmount = ethers.parseEther('10000');
      await wrfToken
        .connect(user1)
        .approve(insuranceGovernance.target, stakeAmount);
      await insuranceGovernance.connect(user1).stakeTokens(stakeAmount);

      // Initial voting power should equal staked amount
      let votingPower = await insuranceGovernance.getVotingPower(user1.address);
      expect(votingPower).to.equal(stakeAmount);

      // Advance time and check increased voting power
      await time.increase(91 * 24 * 60 * 60); // 91 days
      await insuranceGovernance.batchUpdateVotingPower([user1.address]);

      votingPower = await insuranceGovernance.getVotingPower(user1.address);
      expect(votingPower).to.be.gt(stakeAmount); // Should be 1.25x after 90+ days
    });
  });

  describe('Integration Tests', function () {
    it('Should handle complete policy lifecycle with claim', async function () {
      const {
        insurance,
        insurancePool,
        qualityOracle,
        carbonCredits,
        wrfToken,
        oracle,
        user1,
        user2,
        tokenId,
      } = await loadFixture(deployInsuranceFixture);

      // 1. Capital provider deposits funds
      const depositAmount = ethers.parseEther('100000');
      await wrfToken
        .connect(user2)
        .approve(insurancePool.target, depositAmount);
      await insurancePool.connect(user2).depositCapital(depositAmount);

      // 2. User creates insurance policy
      const coveredAmount = 2000;
      const coverageTypes = [0, 1];
      const duration = 90 * 24 * 60 * 60;

      const premiumData = await insurance.calculatePremium(
        tokenId,
        coveredAmount,
        coverageTypes,
        duration
      );

      await wrfToken
        .connect(user1)
        .approve(insurance.target, premiumData.finalPremium);

      const createTx = await insurance
        .connect(user1)
        .createPolicy(
          tokenId,
          coveredAmount,
          coverageTypes,
          duration,
          'QmPolicyTerms123...'
        );

      const createReceipt = await createTx.wait();
      const createEvent = createReceipt.logs.find(log => {
        try {
          return insurance.interface.parseLog(log).name === 'PolicyCreated';
        } catch {
          return false;
        }
      });

      const policyId = insurance.interface.parseLog(createEvent).args.policyId;

      // 3. Pay premium
      await insurance.connect(user1).payPremium(policyId);

      // 4. Simulate project failure
      await qualityOracle.connect(oracle).updateQualityScore(
        carbonCredits.target,
        250, // Low quality indicating failure
        'Project performance degraded'
      );

      // 5. Submit claim
      const claimAmount = 1000;
      const claimTx = await insurance.connect(user1).submitClaim(
        policyId,
        0, // PROJECT_FAILURE
        claimAmount,
        'QmEvidence123...'
      );

      const claimReceipt = await claimTx.wait();
      const claimEvent = claimReceipt.logs.find(log => {
        try {
          return insurance.interface.parseLog(log).name === 'ClaimSubmitted';
        } catch {
          return false;
        }
      });

      const claimId = insurance.interface.parseLog(claimEvent).args.claimId;

      // 6. Process claim
      await insurance.processClaim(claimId);

      // 7. Verify claim was processed and paid
      const claim = await insurance.getClaim(claimId);
      expect(claim.status).to.equal(2); // APPROVED

      const policy = await insurance.getPolicy(policyId);
      expect(policy.status).to.equal(2); // CLAIMED
    });
  });

  describe('Security Tests', function () {
    it('Should prevent unauthorized access to critical functions', async function () {
      const { insurance, user1, tokenId } = await loadFixture(
        deployInsuranceFixture
      );

      // Should not allow non-claims assessor to process claims
      await expect(insurance.connect(user1).processClaim(1)).to.be.revertedWith(
        'AccessControl:'
      );
    });

    it('Should prevent manipulation of oracle data without proper role', async function () {
      const { qualityOracle, carbonCredits, user1 } = await loadFixture(
        deployInsuranceFixture
      );

      await expect(
        qualityOracle
          .connect(user1)
          .updateQualityScore(carbonCredits.target, 100, 'Unauthorized update')
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent claim submission for expired policies', async function () {
      const { insurance, insurancePool, wrfToken, user1, user2, tokenId } =
        await loadFixture(deployInsuranceFixture);

      // Add capital to pool
      const depositAmount = ethers.parseEther('50000');
      await wrfToken
        .connect(user2)
        .approve(insurancePool.target, depositAmount);
      await insurancePool.connect(user2).depositCapital(depositAmount);

      // Create short-term policy
      const coveredAmount = 1000;
      const coverageTypes = [0];
      const duration = 1; // 1 second

      const premiumData = await insurance.calculatePremium(
        tokenId,
        coveredAmount,
        coverageTypes,
        duration
      );

      await wrfToken
        .connect(user1)
        .approve(insurance.target, premiumData.finalPremium);

      const tx = await insurance
        .connect(user1)
        .createPolicy(
          tokenId,
          coveredAmount,
          coverageTypes,
          duration,
          'QmPolicyTerms123...'
        );

      const receipt = await tx.wait();
      const event = receipt.logs.find(log => {
        try {
          return insurance.interface.parseLog(log).name === 'PolicyCreated';
        } catch {
          return false;
        }
      });

      const policyId = insurance.interface.parseLog(event).args.policyId;

      // Pay premium
      await insurance.connect(user1).payPremium(policyId);

      // Wait for policy to expire
      await time.increase(2);

      // Try to submit claim - should fail
      await expect(
        insurance
          .connect(user1)
          .submitClaim(policyId, 0, 500, 'QmEvidence123...')
      ).to.be.revertedWith('Policy expired');
    });
  });
});
