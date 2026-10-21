const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  time,
  loadFixture,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('Carbon Credit Tracking System', function () {
  async function deployCarbonCreditTrackerFixture() {
    const [owner, issuer, registry, user1, user2, retiringEntity] =
      await ethers.getSigners();

    // Deploy CarbonCreditTracker
    const CarbonCreditTracker = await ethers.getContractFactory(
      'CarbonCreditTracker'
    );
    const tracker = await CarbonCreditTracker.deploy();
    await tracker.waitForDeployment();

    // Grant roles
    const AUTHORIZED_ISSUER_ROLE = await tracker.AUTHORIZED_ISSUER_ROLE();
    const REGISTRY_CONNECTOR_ROLE = await tracker.REGISTRY_CONNECTOR_ROLE();

    await tracker.grantRole(AUTHORIZED_ISSUER_ROLE, issuer.address);
    await tracker.grantRole(REGISTRY_CONNECTOR_ROLE, registry.address);

    return {
      tracker,
      owner,
      issuer,
      registry,
      user1,
      user2,
      retiringEntity,
      AUTHORIZED_ISSUER_ROLE,
      REGISTRY_CONNECTOR_ROLE,
    };
  }

  describe('Deployment and Configuration', function () {
    it('Should deploy successfully and set up roles', async function () {
      const {
        tracker,
        owner,
        issuer,
        registry,
        AUTHORIZED_ISSUER_ROLE,
        REGISTRY_CONNECTOR_ROLE,
      } = await loadFixture(deployCarbonCreditTrackerFixture);

      expect(
        await tracker.hasRole(await tracker.DEFAULT_ADMIN_ROLE(), owner.address)
      ).to.be.true;
      expect(await tracker.hasRole(AUTHORIZED_ISSUER_ROLE, issuer.address)).to
        .be.true;
      expect(await tracker.hasRole(REGISTRY_CONNECTOR_ROLE, registry.address))
        .to.be.true;
    });
  });

  describe('Lifecycle Creation', function () {
    it('Should create a lifecycle record successfully', async function () {
      const { tracker, issuer } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      const tokenId = 1;
      const projectId = 'VCS-123-2024';
      const serialNumber = 'VCS-123-2024-001-001';
      const registryStandard = 'VCS';
      const totalSupply = ethers.parseEther('1000');

      const tx = await tracker
        .connect(issuer)
        .createLifecycle(
          tokenId,
          projectId,
          serialNumber,
          registryStandard,
          issuer.address,
          totalSupply
        );

      await expect(tx)
        .to.emit(tracker, 'CreditLifecycleCreated')
        .withArgs(
          ethers.keccak256(
            ethers.solidityPacked(
              ['uint256', 'string', 'string', 'string', 'uint256', 'address'],
              [
                tokenId,
                projectId,
                serialNumber,
                registryStandard,
                await time.latest(),
                issuer.address,
              ]
            )
          ),
          tokenId,
          projectId,
          registryStandard,
          issuer.address
        );
    });

    it('Should prevent duplicate serial numbers', async function () {
      const { tracker, issuer } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      const serialNumber = 'VCS-123-2024-001-001';

      // Create first lifecycle
      await tracker
        .connect(issuer)
        .createLifecycle(
          1,
          'VCS-123-2024',
          serialNumber,
          'VCS',
          issuer.address,
          ethers.parseEther('1000')
        );

      // Try to create second with same serial number
      await expect(
        tracker
          .connect(issuer)
          .createLifecycle(
            2,
            'VCS-124-2024',
            serialNumber,
            'VCS',
            issuer.address,
            ethers.parseEther('500')
          )
      ).to.be.revertedWith('CarbonCreditTracker: Serial number already used');
    });

    it('Should prevent unauthorized lifecycle creation', async function () {
      const { tracker, user1 } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      await expect(
        tracker
          .connect(user1)
          .createLifecycle(
            1,
            'VCS-123-2024',
            'VCS-123-2024-001-001',
            'VCS',
            user1.address,
            ethers.parseEther('1000')
          )
      ).to.be.revertedWith('CarbonCreditTracker: Not authorized issuer');
    });

    it('Should validate lifecycle parameters', async function () {
      const { tracker, issuer } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      // Invalid serial number
      await expect(
        tracker
          .connect(issuer)
          .createLifecycle(
            1,
            'VCS-123-2024',
            '',
            'VCS',
            issuer.address,
            ethers.parseEther('1000')
          )
      ).to.be.revertedWith('CarbonCreditTracker: Invalid serial number');

      // Invalid total supply
      await expect(
        tracker
          .connect(issuer)
          .createLifecycle(
            1,
            'VCS-123-2024',
            'VCS-123-2024-001-001',
            'VCS',
            issuer.address,
            0
          )
      ).to.be.revertedWith('CarbonCreditTracker: Invalid total supply');
    });
  });

  describe('Transfer Recording', function () {
    async function deployWithLifecycleFixture() {
      const baseFixture = await deployCarbonCreditTrackerFixture();
      const { tracker, issuer } = baseFixture;

      const tokenId = 1;
      const projectId = 'VCS-123-2024';
      const serialNumber = 'VCS-123-2024-001-001';
      const registryStandard = 'VCS';
      const totalSupply = ethers.parseEther('1000');

      const lifecycleId = ethers.keccak256(
        ethers.solidityPacked(
          ['uint256', 'string', 'string', 'string', 'uint256', 'address'],
          [
            tokenId,
            projectId,
            serialNumber,
            registryStandard,
            await time.latest(),
            issuer.address,
          ]
        )
      );

      await tracker
        .connect(issuer)
        .createLifecycle(
          tokenId,
          projectId,
          serialNumber,
          registryStandard,
          issuer.address,
          totalSupply
        );

      return { ...baseFixture, lifecycleId, tokenId, totalSupply };
    }

    it('Should record transfers correctly', async function () {
      const { tracker, user1, user2, lifecycleId } = await loadFixture(
        deployWithLifecycleFixture
      );

      const transferAmount = ethers.parseEther('100');
      const transferReason = 'Carbon credit purchase';

      const tx = await tracker.recordTransfer(
        lifecycleId,
        user1.address,
        user2.address,
        transferAmount,
        transferReason
      );

      await expect(tx)
        .to.emit(tracker, 'CreditTransferred')
        .withArgs(
          lifecycleId,
          user1.address,
          user2.address,
          transferAmount,
          await time.latest(),
          transferReason
        );

      // Check transfer history
      const transferHistory = await tracker.getTransferHistory(lifecycleId);
      expect(transferHistory.length).to.equal(1);
      expect(transferHistory[0].from).to.equal(user1.address);
      expect(transferHistory[0].to).to.equal(user2.address);
      expect(transferHistory[0].amount).to.equal(transferAmount);
      expect(transferHistory[0].transferReason).to.equal(transferReason);
    });

    it('Should update lifecycle status after first transfer', async function () {
      const { tracker, user1, user2, lifecycleId } = await loadFixture(
        deployWithLifecycleFixture
      );

      // Check initial status
      let lifecycle = await tracker.getLifecycle(lifecycleId);
      expect(lifecycle.status).to.equal(0); // ISSUED

      // Record transfer
      await tracker.recordTransfer(
        lifecycleId,
        user1.address,
        user2.address,
        ethers.parseEther('100'),
        'First transfer'
      );

      // Check updated status
      lifecycle = await tracker.getLifecycle(lifecycleId);
      expect(lifecycle.status).to.equal(1); // ACTIVE
      expect(lifecycle.currentLocation).to.equal('ACTIVE_TRADING');
    });

    it('Should validate transfer parameters', async function () {
      const { tracker, user1, user2 } = await loadFixture(
        deployWithLifecycleFixture
      );

      const invalidLifecycleId = ethers.keccak256(
        ethers.toUtf8Bytes('invalid')
      );

      // Invalid lifecycle ID
      await expect(
        tracker.recordTransfer(
          invalidLifecycleId,
          user1.address,
          user2.address,
          ethers.parseEther('100'),
          'Transfer'
        )
      ).to.be.revertedWith('CarbonCreditTracker: Lifecycle not found');

      // Invalid amount
      const { lifecycleId } = await loadFixture(deployWithLifecycleFixture);
      await expect(
        tracker.recordTransfer(
          lifecycleId,
          user1.address,
          user2.address,
          0,
          'Transfer'
        )
      ).to.be.revertedWith('CarbonCreditTracker: Invalid amount');
    });
  });

  describe('Retirement and Certificate Generation', function () {
    async function deployWithActiveLifecycleFixture() {
      const baseFixture = await deployWithLifecycleFixture();
      const { tracker, user1, user2, lifecycleId } = baseFixture;

      // Make lifecycle active by recording a transfer
      await tracker.recordTransfer(
        lifecycleId,
        user1.address,
        user2.address,
        ethers.parseEther('100'),
        'Initial transfer'
      );

      return baseFixture;
    }

    it('Should record retirement and generate certificate', async function () {
      const { tracker, registry, retiringEntity, lifecycleId } =
        await loadFixture(deployWithActiveLifecycleFixture);

      const retirementAmount = ethers.parseEther('50');
      const retirementMessage = 'Offset for company operations';

      const tx = await tracker
        .connect(registry)
        .recordRetirement(
          lifecycleId,
          retiringEntity.address,
          retirementAmount,
          retirementMessage
        );

      // Check events
      await expect(tx).to.emit(tracker, 'CreditRetired');
      await expect(tx).to.emit(tracker, 'RetirementCertificateGenerated');

      // Check lifecycle state updates
      const lifecycle = await tracker.getLifecycle(lifecycleId);
      expect(lifecycle.retiredSupply).to.equal(retirementAmount);
      expect(lifecycle.activeSupply).to.equal(ethers.parseEther('950')); // 1000 - 50
      expect(lifecycle.status).to.equal(3); // PARTIALLY_RETIRED

      // Check retirement history
      const retirementHistory = await tracker.getRetirementHistory(lifecycleId);
      expect(retirementHistory.length).to.equal(1);
      expect(retirementHistory[0].retiringEntity).to.equal(
        retiringEntity.address
      );
      expect(retirementHistory[0].amount).to.equal(retirementAmount);
      expect(retirementHistory[0].retirementMessage).to.equal(
        retirementMessage
      );
    });

    it('Should handle full retirement correctly', async function () {
      const { tracker, registry, retiringEntity, lifecycleId, totalSupply } =
        await loadFixture(deployWithActiveLifecycleFixture);

      // Retire all credits
      await tracker
        .connect(registry)
        .recordRetirement(
          lifecycleId,
          retiringEntity.address,
          totalSupply,
          'Full retirement'
        );

      const lifecycle = await tracker.getLifecycle(lifecycleId);
      expect(lifecycle.retiredSupply).to.equal(totalSupply);
      expect(lifecycle.activeSupply).to.equal(0);
      expect(lifecycle.status).to.equal(4); // FULLY_RETIRED
    });

    it('Should prevent retirement of more than active supply', async function () {
      const { tracker, registry, retiringEntity, lifecycleId, totalSupply } =
        await loadFixture(deployWithActiveLifecycleFixture);

      const excessiveAmount = totalSupply + ethers.parseEther('1');

      await expect(
        tracker
          .connect(registry)
          .recordRetirement(
            lifecycleId,
            retiringEntity.address,
            excessiveAmount,
            'Invalid retirement'
          )
      ).to.be.revertedWith('CarbonCreditTracker: Insufficient active supply');
    });

    it('Should only allow registry connectors to record retirement', async function () {
      const { tracker, user1, retiringEntity, lifecycleId } = await loadFixture(
        deployWithActiveLifecycleFixture
      );

      await expect(
        tracker
          .connect(user1)
          .recordRetirement(
            lifecycleId,
            retiringEntity.address,
            ethers.parseEther('50'),
            'Unauthorized retirement'
          )
      ).to.be.revertedWith('CarbonCreditTracker: Not registry connector');
    });

    it('Should generate valid retirement certificates', async function () {
      const { tracker, registry, retiringEntity, lifecycleId } =
        await loadFixture(deployWithActiveLifecycleFixture);

      const retirementAmount = ethers.parseEther('50');
      const retirementMessage = 'Company carbon offset';

      const tx = await tracker
        .connect(registry)
        .recordRetirement(
          lifecycleId,
          retiringEntity.address,
          retirementAmount,
          retirementMessage
        );

      const receipt = await tx.wait();
      const retirementEvent = receipt.logs.find(
        log =>
          log.topics[0] ===
          tracker.interface.getEvent('RetirementCertificateGenerated').topicHash
      );

      const decodedEvent = tracker.interface.decodeEventLog(
        'RetirementCertificateGenerated',
        retirementEvent.data,
        retirementEvent.topics
      );

      const certificateId = decodedEvent.certificateId;

      // Verify certificate
      const certificate = await tracker.getRetirementCertificate(certificateId);
      expect(certificate.lifecycleId).to.equal(lifecycleId);
      expect(certificate.retiringEntity).to.equal(retiringEntity.address);
      expect(certificate.amount).to.equal(retirementAmount);
      expect(certificate.retirementMessage).to.equal(retirementMessage);
      expect(certificate.isValid).to.be.true;

      // Verify certificate authenticity
      expect(await tracker.verifyCertificate(certificateId)).to.be.true;
    });
  });

  describe('Query Functions', function () {
    async function deployWithDataFixture() {
      const baseFixture = await deployWithActiveLifecycleFixture();
      const { tracker, registry, retiringEntity, user1, user2, lifecycleId } =
        baseFixture;

      // Add multiple transfers
      await tracker.recordTransfer(
        lifecycleId,
        user1.address,
        user2.address,
        ethers.parseEther('200'),
        'Transfer 1'
      );
      await tracker.recordTransfer(
        lifecycleId,
        user2.address,
        user1.address,
        ethers.parseEther('100'),
        'Transfer 2'
      );

      // Add retirement
      await tracker
        .connect(registry)
        .recordRetirement(
          lifecycleId,
          retiringEntity.address,
          ethers.parseEther('150'),
          'Retirement for offset'
        );

      return baseFixture;
    }

    it('Should return correct lifecycle information', async function () {
      const { tracker, issuer, lifecycleId } = await loadFixture(
        deployWithDataFixture
      );

      const lifecycle = await tracker.getLifecycle(lifecycleId);

      expect(lifecycle.tokenId).to.equal(1);
      expect(lifecycle.projectId).to.equal('VCS-123-2024');
      expect(lifecycle.serialNumber).to.equal('VCS-123-2024-001-001');
      expect(lifecycle.registryStandard).to.equal('VCS');
      expect(lifecycle.originalIssuer).to.equal(issuer.address);
      expect(lifecycle.totalSupply).to.equal(ethers.parseEther('1000'));
      expect(lifecycle.retiredSupply).to.equal(ethers.parseEther('150'));
      expect(lifecycle.isVerified).to.be.false;
      expect(lifecycle.status).to.equal(3); // PARTIALLY_RETIRED
    });

    it('Should return lifecycle summary correctly', async function () {
      const { tracker, lifecycleId } = await loadFixture(deployWithDataFixture);

      const summary = await tracker.getLifecycleSummary(lifecycleId);

      expect(summary.tokenId).to.equal(1);
      expect(summary.projectId).to.equal('VCS-123-2024');
      expect(summary.serialNumber).to.equal('VCS-123-2024-001-001');
      expect(summary.activeSupply).to.equal(ethers.parseEther('850')); // 1000 - 150 retired
      expect(summary.retiredSupply).to.equal(ethers.parseEther('150'));
      expect(summary.isVerified).to.be.false;
      expect(summary.transferCount).to.equal(3); // Initial + 2 more = 3 total
      expect(summary.retirementCount).to.equal(1);
    });

    it('Should check serial number usage correctly', async function () {
      const { tracker } = await loadFixture(deployWithDataFixture);

      expect(await tracker.isSerialNumberUsed('VCS-123-2024-001-001')).to.be
        .true;
      expect(await tracker.isSerialNumberUsed('VCS-123-2024-001-002')).to.be
        .false;
    });

    it('Should return transfer and retirement histories', async function () {
      const { tracker, lifecycleId } = await loadFixture(deployWithDataFixture);

      const transferHistory = await tracker.getTransferHistory(lifecycleId);
      const retirementHistory = await tracker.getRetirementHistory(lifecycleId);

      expect(transferHistory.length).to.equal(3);
      expect(retirementHistory.length).to.equal(1);

      // Check transfer details
      expect(transferHistory[1].transferReason).to.equal('Transfer 1');
      expect(transferHistory[2].transferReason).to.equal('Transfer 2');

      // Check retirement details
      expect(retirementHistory[0].retirementMessage).to.equal(
        'Retirement for offset'
      );
      expect(retirementHistory[0].amount).to.equal(ethers.parseEther('150'));
    });
  });

  describe('Double-Counting Prevention', function () {
    it('Should prevent reuse of serial numbers across projects', async function () {
      const { tracker, issuer } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      const serialNumber = 'VCS-REUSE-TEST-001';

      // Create first lifecycle
      await tracker
        .connect(issuer)
        .createLifecycle(
          1,
          'PROJECT-A',
          serialNumber,
          'VCS',
          issuer.address,
          ethers.parseEther('1000')
        );

      // Try to create second lifecycle with same serial number
      await expect(
        tracker
          .connect(issuer)
          .createLifecycle(
            2,
            'PROJECT-B',
            serialNumber,
            'GS',
            issuer.address,
            ethers.parseEther('500')
          )
      ).to.be.revertedWith('CarbonCreditTracker: Serial number already used');
    });

    it('Should track serial numbers in mapping', async function () {
      const { tracker, issuer } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      const serialNumber = 'VCS-TRACKING-TEST-001';
      const projectId = 'VCS-123-2024';

      const lifecycleId = await tracker
        .connect(issuer)
        .createLifecycle.staticCall(
          1,
          projectId,
          serialNumber,
          'VCS',
          issuer.address,
          ethers.parseEther('1000')
        );

      await tracker
        .connect(issuer)
        .createLifecycle(
          1,
          projectId,
          serialNumber,
          'VCS',
          issuer.address,
          ethers.parseEther('1000')
        );

      expect(await tracker.serialToLifecycle(serialNumber)).to.equal(
        lifecycleId
      );
    });
  });

  describe('Access Control and Security', function () {
    it('Should enforce role-based access control', async function () {
      const { tracker, user1, user2 } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      // Test unauthorized lifecycle creation
      await expect(
        tracker
          .connect(user1)
          .createLifecycle(
            1,
            'PROJECT-UNAUTHORIZED',
            'UNAUTH-001',
            'VCS',
            user1.address,
            ethers.parseEther('1000')
          )
      ).to.be.revertedWith('CarbonCreditTracker: Not authorized issuer');

      // Test unauthorized retirement
      const validLifecycleId = ethers.keccak256(ethers.toUtf8Bytes('valid'));
      await expect(
        tracker
          .connect(user2)
          .recordRetirement(
            validLifecycleId,
            user1.address,
            ethers.parseEther('100'),
            'Unauthorized retirement'
          )
      ).to.be.revertedWith('CarbonCreditTracker: Not registry connector');
    });

    it('Should allow admin to pause and unpause', async function () {
      const { tracker, owner, issuer } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      // Pause contract
      await tracker.connect(owner).pause();

      // Test that functions are paused
      await expect(
        tracker
          .connect(issuer)
          .createLifecycle(
            1,
            'PAUSED-PROJECT',
            'PAUSED-001',
            'VCS',
            issuer.address,
            ethers.parseEther('1000')
          )
      ).to.be.revertedWith('Pausable: paused');

      // Unpause contract
      await tracker.connect(owner).unpause();

      // Test that functions work again
      await expect(
        tracker
          .connect(issuer)
          .createLifecycle(
            1,
            'UNPAUSED-PROJECT',
            'UNPAUSED-001',
            'VCS',
            issuer.address,
            ethers.parseEther('1000')
          )
      ).to.not.be.reverted;
    });

    it('Should prevent non-admin from pausing', async function () {
      const { tracker, user1 } = await loadFixture(
        deployCarbonCreditTrackerFixture
      );

      await expect(tracker.connect(user1).pause()).to.be.reverted; // Will be reverted with access control error
    });
  });

  describe('Edge Cases and Error Handling', function () {
    it('Should handle non-existent lifecycle queries gracefully', async function () {
      const { tracker } = await loadFixture(deployCarbonCreditTrackerFixture);

      const nonExistentId = ethers.keccak256(ethers.toUtf8Bytes('nonexistent'));

      // Should return default values for non-existent lifecycle
      const lifecycle = await tracker.getLifecycle(nonExistentId);
      expect(lifecycle.tokenId).to.equal(0);
      expect(lifecycle.projectId).to.equal('');

      // Should revert for lifecycle summary of non-existent lifecycle
      await expect(
        tracker.getLifecycleSummary(nonExistentId)
      ).to.be.revertedWith('CarbonCreditTracker: Lifecycle not found');

      // History functions should return empty arrays
      const transferHistory = await tracker.getTransferHistory(nonExistentId);
      const retirementHistory =
        await tracker.getRetirementHistory(nonExistentId);
      expect(transferHistory.length).to.equal(0);
      expect(retirementHistory.length).to.equal(0);
    });

    it('Should handle invalid certificate queries', async function () {
      const { tracker } = await loadFixture(deployCarbonCreditTrackerFixture);

      // Non-existent certificate should return default values
      const certificate =
        await tracker.getRetirementCertificate('INVALID-CERT');
      expect(certificate.certificateId).to.equal('');
      expect(certificate.isValid).to.be.false;

      // Certificate verification should return false
      expect(await tracker.verifyCertificate('INVALID-CERT')).to.be.false;
    });
  });
});
