const { expect } = require('chai');
const { ethers } = require('hardhat');
const { time } = require('@nomicfoundation/hardhat-network-helpers');

describe('SecureEnhancedCarbonCredits - Minting Security Tests', function () {
  let carbonCredits, wrfToken;
  let admin,
    authorizedIssuer,
    registryConnector,
    mintingAuditor,
    emergencyResponder;
  let attacker, user1, user2;
  let signer1, signer2, signer3, signer4, signer5;

  // Test constants
  const GLOBAL_SUPPLY_CAP = ethers.parseEther('1000000000'); // 1B tonnes
  const BATCH_SUPPLY_CAP = ethers.parseEther('10000000'); // 10M tonnes per batch
  const MINTING_DELAY = 24 * 60 * 60; // 24 hours
  const MULTISIG_THRESHOLD = 3;

  beforeEach(async function () {
    [
      admin,
      authorizedIssuer,
      registryConnector,
      mintingAuditor,
      emergencyResponder,
      attacker,
      user1,
      user2,
      signer1,
      signer2,
      signer3,
      signer4,
      signer5,
    ] = await ethers.getSigners();

    // Deploy mock WRF token
    const WRFToken = await ethers.getContractFactory('MockERC20');
    wrfToken = await WRFToken.deploy(
      'WRF Token',
      'WRF',
      ethers.parseEther('1000000')
    );

    // Deploy SecureEnhancedCarbonCredits
    const CarbonCredits = await ethers.getContractFactory(
      'SecureEnhancedCarbonCredits'
    );
    carbonCredits = await CarbonCredits.deploy(await wrfToken.getAddress());

    // Set up roles
    const AUTHORIZED_ISSUER_ROLE = await carbonCredits.AUTHORIZED_ISSUER_ROLE();
    const REGISTRY_CONNECTOR_ROLE =
      await carbonCredits.REGISTRY_CONNECTOR_ROLE();
    const MINTING_AUDITOR_ROLE = await carbonCredits.MINTING_AUDITOR_ROLE();
    const EMERGENCY_RESPONDER_ROLE =
      await carbonCredits.EMERGENCY_RESPONDER_ROLE();

    await carbonCredits.grantRole(
      AUTHORIZED_ISSUER_ROLE,
      authorizedIssuer.address
    );
    await carbonCredits.grantRole(
      REGISTRY_CONNECTOR_ROLE,
      registryConnector.address
    );
    await carbonCredits.grantRole(MINTING_AUDITOR_ROLE, mintingAuditor.address);
    await carbonCredits.grantRole(
      EMERGENCY_RESPONDER_ROLE,
      emergencyResponder.address
    );

    // Add authorized signers for multisig
    await carbonCredits.addAuthorizedSigner(signer1.address);
    await carbonCredits.addAuthorizedSigner(signer2.address);
    await carbonCredits.addAuthorizedSigner(signer3.address);
    await carbonCredits.addAuthorizedSigner(signer4.address);
    await carbonCredits.addAuthorizedSigner(signer5.address);

    // Distribute WRF tokens for testing
    await wrfToken.transfer(user1.address, ethers.parseEther('10000'));
    await wrfToken.transfer(user2.address, ethers.parseEther('10000'));
  });

  describe('Access Control Security', function () {
    it('Should prevent unauthorized batch creation', async function () {
      await expect(
        carbonCredits.connect(attacker).createBatchSecure(
          ethers.parseEther('1000'), // totalSupply
          ethers.parseEther('1000'), // maxCap
          'PROJECT_001',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_001',
          'https://registry.example.com/PROJECT_001',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_001')),
          'https://metadata.example.com/001'
        )
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent unauthorized minting requests', async function () {
      // First create a valid batch
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_001',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_001',
          'https://registry.example.com/PROJECT_001',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_001')),
          'https://metadata.example.com/001'
        );

      await expect(
        carbonCredits.connect(attacker).submitMintingRequest(
          1, // tokenId
          ethers.parseEther('100'),
          user1.address,
          ethers.keccak256(ethers.toUtf8Bytes('mint_proof_001'))
        )
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent unauthorized minting approval', async function () {
      await expect(
        carbonCredits
          .connect(attacker)
          .approveMintingRequest(1, 'Fraudulent approval')
      ).to.be.revertedWith('AccessControl:');
    });
  });

  describe('Supply Cap Protection', function () {
    let tokenId;

    beforeEach(async function () {
      // Create a batch with limited supply
      const tx = await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'), // Max cap same as initial supply
          'PROJECT_002',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_002',
          'https://registry.example.com/PROJECT_002',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_002')),
          'https://metadata.example.com/002'
        );
      const receipt = await tx.wait();
      tokenId = 1; // First token ID
    });

    it('Should prevent minting beyond batch supply cap', async function () {
      await expect(
        carbonCredits.connect(registryConnector).submitMintingRequest(
          tokenId,
          ethers.parseEther('1001'), // Exceeds cap
          user1.address,
          ethers.keccak256(ethers.toUtf8Bytes('mint_proof_003'))
        )
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Exceeds batch cap');
    });

    it('Should prevent creating batches exceeding global cap', async function () {
      // Create a massive batch that would exceed global cap
      await expect(
        carbonCredits
          .connect(authorizedIssuer)
          .createBatchSecure(
            GLOBAL_SUPPLY_CAP + 1n,
            GLOBAL_SUPPLY_CAP + 1n,
            'PROJECT_MASSIVE',
            2024,
            'VM0006',
            'forestry',
            'VCS',
            'Brazil',
            Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
            'cert_hash_massive',
            'https://registry.example.com/PROJECT_MASSIVE',
            ethers.keccak256(ethers.toUtf8Bytes('registry_proof_massive')),
            'https://metadata.example.com/massive'
          )
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Exceeds global cap');
    });

    it('Should prevent batch caps exceeding the maximum allowed', async function () {
      await expect(
        carbonCredits.connect(authorizedIssuer).createBatchSecure(
          ethers.parseEther('1000'),
          BATCH_SUPPLY_CAP + 1n, // Exceeds maximum batch cap
          'PROJECT_OVERSIZED',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_oversized',
          'https://registry.example.com/PROJECT_OVERSIZED',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_oversized')),
          'https://metadata.example.com/oversized'
        )
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Exceeds batch cap');
    });
  });

  describe('Minting Delay and Approval Process', function () {
    let tokenId, requestId;

    beforeEach(async function () {
      // Create a batch
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_003',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_003',
          'https://registry.example.com/PROJECT_003',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_003')),
          'https://metadata.example.com/003'
        );
      tokenId = 1;

      // Submit a minting request
      const tx = await carbonCredits
        .connect(registryConnector)
        .submitMintingRequest(
          tokenId,
          ethers.parseEther('100'),
          user1.address,
          ethers.keccak256(ethers.toUtf8Bytes('mint_proof_004'))
        );
      const receipt = await tx.wait();
      requestId = 1; // First request ID
    });

    it('Should prevent approval before minting delay', async function () {
      await expect(
        carbonCredits
          .connect(mintingAuditor)
          .approveMintingRequest(requestId, 'Premature approval attempt')
      ).to.be.revertedWith(
        'SecureEnhancedCarbonCredits: Minting delay not met'
      );
    });

    it('Should allow approval after minting delay', async function () {
      // Fast forward time past the minting delay
      await time.increase(MINTING_DELAY + 1);

      await expect(
        carbonCredits
          .connect(mintingAuditor)
          .approveMintingRequest(requestId, 'Approved after delay')
      ).to.emit(carbonCredits, 'MintingRequestApproved');
    });

    it('Should prevent execution without approval', async function () {
      await expect(
        carbonCredits
          .connect(registryConnector)
          .executeMintingRequest(requestId)
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Not approved');
    });

    it('Should prevent execution of expired approval', async function () {
      // Fast forward past minting delay
      await time.increase(MINTING_DELAY + 1);

      // Approve the request
      await carbonCredits
        .connect(mintingAuditor)
        .approveMintingRequest(requestId, 'Approved');

      // Fast forward past approval expiry (7 days)
      await time.increase(8 * 24 * 60 * 60);

      await expect(
        carbonCredits
          .connect(registryConnector)
          .executeMintingRequest(requestId)
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Approval expired');
    });
  });

  describe('Rate Limiting', function () {
    let tokenId;

    beforeEach(async function () {
      // Create a batch
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_004',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_004',
          'https://registry.example.com/PROJECT_004',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_004')),
          'https://metadata.example.com/004'
        );
      tokenId = 1;
    });

    it('Should enforce rate limiting for minting requests', async function () {
      // Submit first request
      await carbonCredits
        .connect(registryConnector)
        .submitMintingRequest(
          tokenId,
          ethers.parseEther('100'),
          user1.address,
          ethers.keccak256(ethers.toUtf8Bytes('mint_proof_005'))
        );

      // Attempt immediate second request
      await expect(
        carbonCredits
          .connect(registryConnector)
          .submitMintingRequest(
            tokenId,
            ethers.parseEther('100'),
            user2.address,
            ethers.keccak256(ethers.toUtf8Bytes('mint_proof_006'))
          )
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Rate limit exceeded');
    });

    it('Should allow requests after rate limit period', async function () {
      // Submit first request
      await carbonCredits
        .connect(registryConnector)
        .submitMintingRequest(
          tokenId,
          ethers.parseEther('100'),
          user1.address,
          ethers.keccak256(ethers.toUtf8Bytes('mint_proof_007'))
        );

      // Fast forward past rate limit (1 hour)
      await time.increase(60 * 60 + 1);

      // Should now allow second request
      await expect(
        carbonCredits
          .connect(registryConnector)
          .submitMintingRequest(
            tokenId,
            ethers.parseEther('100'),
            user2.address,
            ethers.keccak256(ethers.toUtf8Bytes('mint_proof_008'))
          )
      ).to.emit(carbonCredits, 'MintingRequestSubmitted');
    });
  });

  describe('Registry Proof Protection', function () {
    let tokenId;

    beforeEach(async function () {
      // Create a batch
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_005',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_005',
          'https://registry.example.com/PROJECT_005',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_005')),
          'https://metadata.example.com/005'
        );
      tokenId = 1;
    });

    it('Should prevent reuse of registry proofs', async function () {
      const proofHash = ethers.keccak256(ethers.toUtf8Bytes('mint_proof_009'));

      // Submit first request with proof
      await carbonCredits
        .connect(registryConnector)
        .submitMintingRequest(
          tokenId,
          ethers.parseEther('100'),
          user1.address,
          proofHash
        );

      // Fast forward past rate limit
      await time.increase(60 * 60 + 1);

      // Attempt to use same proof again
      await expect(
        carbonCredits.connect(registryConnector).submitMintingRequest(
          tokenId,
          ethers.parseEther('100'),
          user2.address,
          proofHash // Same proof hash
        )
      ).to.be.revertedWith(
        'SecureEnhancedCarbonCredits: Registry proof already used'
      );
    });

    it('Should prevent batch creation with reused registry proofs', async function () {
      const usedProof = ethers.keccak256(
        ethers.toUtf8Bytes('registry_proof_005')
      );

      await expect(
        carbonCredits.connect(authorizedIssuer).createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_006',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_006',
          'https://registry.example.com/PROJECT_006',
          usedProof, // Reused registry proof
          'https://metadata.example.com/006'
        )
      ).to.be.revertedWith(
        'SecureEnhancedCarbonCredits: Registry proof already used'
      );
    });
  });

  describe('Emergency Controls', function () {
    let tokenId, requestId;

    beforeEach(async function () {
      // Create a batch
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_006',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_006',
          'https://registry.example.com/PROJECT_006',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_006')),
          'https://metadata.example.com/006'
        );
      tokenId = 1;

      // Submit and approve a minting request
      await carbonCredits
        .connect(registryConnector)
        .submitMintingRequest(
          tokenId,
          ethers.parseEther('100'),
          user1.address,
          ethers.keccak256(ethers.toUtf8Bytes('mint_proof_010'))
        );
      requestId = 1;

      await time.increase(MINTING_DELAY + 1);
      await carbonCredits
        .connect(mintingAuditor)
        .approveMintingRequest(requestId, 'Approved');
    });

    it('Should prevent minting when emergency paused', async function () {
      // Emergency pause minting
      await carbonCredits
        .connect(admin)
        .createMultisigOperation(
          'EMERGENCY_PAUSE',
          '0x',
          Math.floor(Date.now() / 1000) + 60 * 60
        );

      // Get required signatures
      await carbonCredits.connect(signer1).signMultisigOperation(1);
      await carbonCredits.connect(signer2).signMultisigOperation(1);
      await carbonCredits.connect(signer3).signMultisigOperation(1);

      // Now minting should be paused
      await expect(
        carbonCredits
          .connect(registryConnector)
          .executeMintingRequest(requestId)
      ).to.be.revertedWith(
        'SecureEnhancedCarbonCredits: Emergency minting paused'
      );
    });

    it('Should prevent minting from frozen batch', async function () {
      // Emergency freeze the batch
      await carbonCredits
        .connect(emergencyResponder)
        .emergencyFreezeBatch(tokenId, 'Suspected fraudulent activity');

      await expect(
        carbonCredits
          .connect(registryConnector)
          .executeMintingRequest(requestId)
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Batch frozen');
    });

    it('Should prevent new requests when emergency paused', async function () {
      // Emergency pause minting
      await carbonCredits
        .connect(admin)
        .createMultisigOperation(
          'EMERGENCY_PAUSE',
          '0x',
          Math.floor(Date.now() / 1000) + 60 * 60
        );

      // Get required signatures
      await carbonCredits.connect(signer1).signMultisigOperation(1);
      await carbonCredits.connect(signer2).signMultisigOperation(1);
      await carbonCredits.connect(signer3).signMultisigOperation(1);

      await expect(
        carbonCredits
          .connect(registryConnector)
          .submitMintingRequest(
            tokenId,
            ethers.parseEther('100'),
            user2.address,
            ethers.keccak256(ethers.toUtf8Bytes('mint_proof_011'))
          )
      ).to.be.revertedWith(
        'SecureEnhancedCarbonCredits: Emergency minting paused'
      );
    });
  });

  describe('Multi-signature Security', function () {
    it('Should require threshold signatures for critical operations', async function () {
      // Create emergency pause operation
      await carbonCredits
        .connect(admin)
        .createMultisigOperation(
          'EMERGENCY_PAUSE',
          '0x',
          Math.floor(Date.now() / 1000) + 60 * 60
        );

      // Single signature should not execute
      await carbonCredits.connect(signer1).signMultisigOperation(1);
      expect(await carbonCredits.emergencyMintingPaused()).to.be.false;

      // Second signature should not execute
      await carbonCredits.connect(signer2).signMultisigOperation(1);
      expect(await carbonCredits.emergencyMintingPaused()).to.be.false;

      // Third signature should trigger execution
      await carbonCredits.connect(signer3).signMultisigOperation(1);
      expect(await carbonCredits.emergencyMintingPaused()).to.be.true;
    });

    it('Should prevent unauthorized signers from participating', async function () {
      // Create operation
      await carbonCredits
        .connect(admin)
        .createMultisigOperation(
          'EMERGENCY_PAUSE',
          '0x',
          Math.floor(Date.now() / 1000) + 60 * 60
        );

      // Unauthorized signer should fail
      await expect(
        carbonCredits.connect(attacker).signMultisigOperation(1)
      ).to.be.revertedWith(
        'SecureEnhancedCarbonCredits: Not authorized signer'
      );
    });

    it('Should prevent double signing by same address', async function () {
      // Create operation
      await carbonCredits
        .connect(admin)
        .createMultisigOperation(
          'EMERGENCY_PAUSE',
          '0x',
          Math.floor(Date.now() / 1000) + 60 * 60
        );

      // First signature
      await carbonCredits.connect(signer1).signMultisigOperation(1);

      // Attempt to sign again
      await expect(
        carbonCredits.connect(signer1).signMultisigOperation(1)
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Already signed');
    });
  });

  describe('Comprehensive Attack Scenarios', function () {
    it('Should prevent coordinated minting attack', async function () {
      // Create multiple batches with different issuers trying to exceed global cap
      const largeSupply = GLOBAL_SUPPLY_CAP / 3n;

      // Create three large batches
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          largeSupply,
          largeSupply,
          'PROJECT_LARGE_1',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_large_1',
          'https://registry.example.com/PROJECT_LARGE_1',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_large_1')),
          'https://metadata.example.com/large_1'
        );

      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          largeSupply,
          largeSupply,
          'PROJECT_LARGE_2',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_large_2',
          'https://registry.example.com/PROJECT_LARGE_2',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_large_2')),
          'https://metadata.example.com/large_2'
        );

      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          largeSupply,
          largeSupply,
          'PROJECT_LARGE_3',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_large_3',
          'https://registry.example.com/PROJECT_LARGE_3',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_large_3')),
          'https://metadata.example.com/large_3'
        );

      // Fourth batch should fail due to global cap
      await expect(
        carbonCredits
          .connect(authorizedIssuer)
          .createBatchSecure(
            largeSupply,
            largeSupply,
            'PROJECT_LARGE_4',
            2024,
            'VM0006',
            'forestry',
            'VCS',
            'Brazil',
            Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
            'cert_hash_large_4',
            'https://registry.example.com/PROJECT_LARGE_4',
            ethers.keccak256(ethers.toUtf8Bytes('registry_proof_large_4')),
            'https://metadata.example.com/large_4'
          )
      ).to.be.revertedWith('SecureEnhancedCarbonCredits: Exceeds global cap');
    });

    it('Should handle complete end-to-end secure minting process', async function () {
      // Create batch
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_E2E',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_e2e',
          'https://registry.example.com/PROJECT_E2E',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_e2e')),
          'https://metadata.example.com/e2e'
        );

      // Submit minting request
      await carbonCredits
        .connect(registryConnector)
        .submitMintingRequest(
          1,
          ethers.parseEther('100'),
          user1.address,
          ethers.keccak256(ethers.toUtf8Bytes('mint_proof_e2e'))
        );

      // Wait for minting delay
      await time.increase(MINTING_DELAY + 1);

      // Approve request
      await carbonCredits
        .connect(mintingAuditor)
        .approveMintingRequest(1, 'Approved after thorough audit');

      // Execute minting
      await expect(
        carbonCredits.connect(registryConnector).executeMintingRequest(1)
      ).to.emit(carbonCredits, 'SecureMinting');

      // Verify minting occurred
      expect(await carbonCredits.balanceOf(user1.address, 1)).to.equal(
        ethers.parseEther('100')
      );
    });
  });

  describe('Security Metrics and Monitoring', function () {
    beforeEach(async function () {
      // Create test batch
      await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_METRICS',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_metrics',
          'https://registry.example.com/PROJECT_METRICS',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_metrics')),
          'https://metadata.example.com/metrics'
        );
    });

    it('Should provide accurate global security metrics', async function () {
      const metrics = await carbonCredits.getGlobalSecurityMetrics();

      expect(metrics.totalSupply).to.equal(ethers.parseEther('1000'));
      expect(metrics.remainingGlobalCap).to.equal(
        GLOBAL_SUPPLY_CAP - ethers.parseEther('1000')
      );
      expect(metrics.emergencyPaused).to.be.false;
      expect(metrics.activeBatches).to.equal(1);
    });

    it('Should provide comprehensive batch security info', async function () {
      const securityInfo = await carbonCredits.getBatchSecurityInfo(1);

      expect(securityInfo.batch.totalSupply).to.equal(
        ethers.parseEther('1000')
      );
      expect(securityInfo.batch.maxSupplyCap).to.equal(
        ethers.parseEther('1000')
      );
      expect(securityInfo.remainingCap).to.equal(ethers.parseEther('1000'));
      expect(securityInfo.auditRequired).to.be.false; // Recently created
      expect(securityInfo.pendingRequests).to.equal(0);
    });
  });

  describe('Gas Optimization Validation', function () {
    it('Should maintain reasonable gas costs for security operations', async function () {
      // Test batch creation gas usage
      const tx = await carbonCredits
        .connect(authorizedIssuer)
        .createBatchSecure(
          ethers.parseEther('1000'),
          ethers.parseEther('1000'),
          'PROJECT_GAS',
          2024,
          'VM0006',
          'forestry',
          'VCS',
          'Brazil',
          Math.floor(Date.now() / 1000) + 365 * 24 * 60 * 60,
          'cert_hash_gas',
          'https://registry.example.com/PROJECT_GAS',
          ethers.keccak256(ethers.toUtf8Bytes('registry_proof_gas')),
          'https://metadata.example.com/gas'
        );

      const receipt = await tx.wait();
      console.log(`Batch creation gas used: ${receipt.gasUsed}`);

      // Should be under reasonable limit (adjust based on requirements)
      expect(receipt.gasUsed).to.be.below(1000000);
    });
  });
});
