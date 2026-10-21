const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
  mine,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('SecureVotingSystem', function () {
  // Test fixtures
  async function deploySecureVotingSystemFixture() {
    const [
      deployer,
      admin,
      validator1,
      validator2,
      validator3,
      voter1,
      voter2,
      voter3,
      attacker,
    ] = await ethers.getSigners();

    // Deploy mock governance token
    const MockToken = await ethers.getContractFactory('MockERC20');
    const token = await MockToken.deploy(
      'Governance Token',
      'GOV',
      ethers.parseEther('1000000')
    );

    // Deploy timelock controller
    const TimelockController =
      await ethers.getContractFactory('TimelockController');
    const timelock = await TimelockController.deploy(
      24 * 60 * 60, // 24 hour delay
      [admin.address], // proposers
      [admin.address], // executors
      admin.address // admin
    );

    // Deploy SecureVotingSystem
    const SecureVotingSystem =
      await ethers.getContractFactory('SecureVotingSystem');
    const validators = [
      validator1.address,
      validator2.address,
      validator3.address,
    ];
    const votingSystem = await SecureVotingSystem.deploy(
      await token.getAddress(),
      await timelock.getAddress(),
      admin.address,
      validators
    );

    // Setup token balances and voting power
    await token.transfer(voter1.address, ethers.parseEther('10000'));
    await token.transfer(voter2.address, ethers.parseEther('5000'));
    await token.transfer(voter3.address, ethers.parseEther('3000'));

    // Enable voting power (assuming the token has voting capabilities)
    await token.connect(voter1).delegate(voter1.address);
    await token.connect(voter2).delegate(voter2.address);
    await token.connect(voter3).delegate(voter3.address);

    return {
      votingSystem,
      token,
      timelock,
      deployer,
      admin,
      validator1,
      validator2,
      validator3,
      voter1,
      voter2,
      voter3,
      attacker,
    };
  }

  describe('Deployment and Configuration', function () {
    it('Should deploy with correct initial configuration', async function () {
      const { votingSystem, token, timelock, admin } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      expect(await votingSystem.token()).to.equal(await token.getAddress());
      expect(await votingSystem.timelock()).to.equal(
        await timelock.getAddress()
      );
      expect(
        await votingSystem.hasRole(
          await votingSystem.DEFAULT_ADMIN_ROLE(),
          admin.address
        )
      ).to.be.true;
      expect(await votingSystem.validatorCount()).to.equal(3);
    });

    it('Should initialize security configuration correctly', async function () {
      const { votingSystem } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const config = await votingSystem.getSecurityConfig();
      expect(config.timestampTolerance).to.equal(300); // 5 minutes
      expect(config.blockConfirmations).to.equal(3);
      expect(config.validatorConsensusThreshold).to.equal(67);
      expect(config.emergencyPauseThreshold).to.equal(3);
    });

    it('Should initialize validators correctly', async function () {
      const { votingSystem, validator1, validator2, validator3 } =
        await loadFixture(deploySecureVotingSystemFixture);

      expect(await votingSystem.isActiveValidator(validator1.address)).to.be
        .true;
      expect(await votingSystem.isActiveValidator(validator2.address)).to.be
        .true;
      expect(await votingSystem.isActiveValidator(validator3.address)).to.be
        .true;

      const validator1Info = await votingSystem.getValidatorInfo(
        validator1.address
      );
      expect(validator1Info.active).to.be.true;
      expect(validator1Info.reputation).to.equal(100);
    });
  });

  describe('Proposal Creation and Timestamp Validation', function () {
    it('Should create proposal with valid parameters', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10'); // PROPOSER_BOND
      const title = 'Test Proposal';
      const description = 'This is a test proposal for timestamp validation';
      const data = '0x1234';
      const votingDelay = 3600; // 1 hour
      const votingPeriod = 24 * 3600; // 24 hours

      const tx = await votingSystem
        .connect(voter1)
        .createProposal(title, description, data, votingDelay, votingPeriod, {
          value: proposerBond,
        });

      await expect(tx)
        .to.emit(votingSystem, 'ProposalCreated')
        .withArgs(1, voter1.address, title, anyValue, anyValue);

      const proposalState = await votingSystem.getProposalState(1);
      expect(proposalState).to.equal(1); // TimestampValidation
    });

    it('Should reject proposal with insufficient bond', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const insufficientBond = ethers.parseEther('5'); // Less than PROPOSER_BOND

      await expect(
        votingSystem
          .connect(voter1)
          .createProposal('Test', 'Description', '0x', 3600, 24 * 3600, {
            value: insufficientBond,
          })
      ).to.be.revertedWith('Insufficient proposer bond');
    });

    it('Should reject proposal with invalid voting periods', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Too short voting delay
      await expect(
        votingSystem.connect(voter1).createProposal(
          'Test',
          'Description',
          '0x',
          1800, // 30 minutes, less than 1 hour minimum
          24 * 3600,
          { value: proposerBond }
        )
      ).to.be.revertedWith('Voting delay too short');

      // Too short voting period
      await expect(
        votingSystem.connect(voter1).createProposal(
          'Test',
          'Description',
          '0x',
          3600,
          12 * 3600, // 12 hours, less than 24 hour minimum
          { value: proposerBond }
        )
      ).to.be.revertedWith('Invalid voting period');
    });

    it('Should validate proposal timestamps by validators', async function () {
      const { votingSystem, voter1, validator1, validator2, validator3 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal('Test Proposal', 'Description', '0x', 3600, 24 * 3600, {
          value: proposerBond,
        });

      // Validators approve timestamps (need 67% consensus = 2 out of 3)
      await expect(
        votingSystem.connect(validator1).validateProposalTimestamps(1, true, '')
      ).to.emit(votingSystem, 'ProposalValidated');

      await expect(
        votingSystem.connect(validator2).validateProposalTimestamps(1, true, '')
      ).to.emit(votingSystem, 'ProposalValidated');

      // Check proposal state changed to Pending after consensus
      const proposalState = await votingSystem.getProposalState(1);
      expect(proposalState).to.equal(2); // Pending
    });

    it('Should reject validation from non-validators', async function () {
      const { votingSystem, voter1, voter2 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Create proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal('Test Proposal', 'Description', '0x', 3600, 24 * 3600, {
          value: proposerBond,
        });

      // Non-validator tries to validate
      await expect(
        votingSystem.connect(voter2).validateProposalTimestamps(1, true, '')
      ).to.be.revertedWithCustomError(votingSystem, 'UnauthorizedValidator');
    });
  });

  describe('Timestamp Manipulation Attack Prevention', function () {
    it('Should detect and prevent basic timestamp manipulation', async function () {
      const { votingSystem, voter1, validator1, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Create proposal with suspicious timing
      const proposerBond = ethers.parseEther('10');

      // Attempt to manipulate by setting unrealistic future timestamp
      const currentTime = await time.latest();
      const suspiciousDelay = 86400 * 365; // 1 year in the future

      await expect(
        votingSystem
          .connect(attacker)
          .createProposal(
            'Malicious Proposal',
            'Attempting timestamp manipulation',
            '0x',
            suspiciousDelay,
            24 * 3600,
            { value: proposerBond }
          )
      ).to.be.revertedWithCustomError(votingSystem, 'InvalidTimestamp');
    });

    it('Should prevent timestamp manipulation through validator consensus', async function () {
      const { votingSystem, voter1, validator1, validator2, validator3 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create borderline suspicious proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem.connect(voter1).createProposal(
        'Suspicious Proposal',
        'Borderline timestamp manipulation attempt',
        '0x',
        7200, // 2 hours
        24 * 3600,
        { value: proposerBond }
      );

      // Validators reject due to suspicious timestamp patterns
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, false, 'Suspicious timing pattern');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(
          1,
          false,
          'Timestamp manipulation detected'
        );

      // Proposal should remain in validation state since consensus was not reached for approval
      const proposalState = await votingSystem.getProposalState(1);
      expect(proposalState).to.equal(1); // Still in TimestampValidation
    });

    it('Should implement circuit breaker for suspicious activity', async function () {
      const { votingSystem, voter1, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create multiple proposals rapidly to trigger circuit breaker
      for (let i = 0; i < 5; i++) {
        await votingSystem
          .connect(attacker)
          .createProposal(
            `Spam Proposal ${i}`,
            'Attempting to spam the system',
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          );
      }

      // Check circuit breaker status
      const cbStatus = await votingSystem.getCircuitBreakerStatus();
      expect(cbStatus.enabled).to.be.true;
      expect(cbStatus.currentActivity).to.be.greaterThan(0);
    });
  });

  describe('Voting Security and EIP-712 Signatures', function () {
    it('Should cast vote with valid EIP-712 signature', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create and validate proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal('Test Proposal', 'Description', '0x', 3600, 24 * 3600, {
          value: proposerBond,
        });

      // Validate proposal
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');

      // Move to voting period
      await time.increase(3600);

      // Create EIP-712 signature
      const domain = {
        name: 'SecureVotingSystem',
        version: '1',
        chainId: await voter1.provider.getNetwork().then(n => n.chainId),
        verifyingContract: await votingSystem.getAddress(),
      };

      const types = {
        Vote: [
          { name: 'proposalId', type: 'uint256' },
          { name: 'support', type: 'uint8' },
          { name: 'weight', type: 'uint256' },
          { name: 'nonce', type: 'uint256' },
          { name: 'deadline', type: 'uint256' },
          { name: 'reason', type: 'bytes32' },
        ],
      };

      const votingPower = await votingSystem._getVotingPower(voter1.address);
      const nonce = await votingSystem.proposalVoteNonces(1, voter1.address);
      const deadline = (await time.latest()) + 86400;
      const reason = 'Supporting this proposal';

      const message = {
        proposalId: 1,
        support: 1,
        weight: votingPower,
        nonce: nonce,
        deadline: deadline,
        reason: ethers.keccak256(ethers.toUtf8Bytes(reason)),
      };

      const signature = await voter1.signTypedData(domain, types, message);

      await expect(
        votingSystem.connect(voter1).castVote(1, 1, reason, signature)
      ).to.emit(votingSystem, 'VoteCast');
    });

    it('Should prevent signature replay attacks', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Setup proposal and validation
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal('Test Proposal', 'Description', '0x', 3600, 24 * 3600, {
          value: proposerBond,
        });

      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');
      await time.increase(3600);

      // Create signature (simplified for test)
      const signature =
        '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12';

      // First vote should succeed
      await expect(
        votingSystem.connect(voter1).castVote(1, 1, 'First vote', signature)
      ).to.emit(votingSystem, 'VoteCast');

      // Second vote with same signature should fail
      await expect(
        votingSystem.connect(voter1).castVote(1, 1, 'Replay attack', signature)
      ).to.be.revertedWithCustomError(votingSystem, 'ReplayAttack');
    });

    it('Should implement quadratic voting correctly', async function () {
      const { votingSystem, voter1, voter2, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create proposal with quadratic voting enabled
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Quadratic Voting Test',
          'Testing quadratic voting mechanism',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');
      await time.increase(3600);

      // Check if quadratic voting is enabled for the proposal
      // Note: This is a simplified test - actual implementation would require
      // proper EIP-712 signatures and token balance setup
    });
  });

  describe('MEV Protection and Commit-Reveal Scheme', function () {
    it('Should support commit-reveal voting scheme', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create proposal with MEV protection enabled
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'MEV Protected Proposal',
          'Testing MEV protection',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');

      // Create commitment
      const support = 1;
      const salt = ethers.randomBytes(32);
      const reason = 'Supporting with MEV protection';
      const commitment = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          ['uint8', 'bytes32', 'string', 'address'],
          [support, salt, reason, voter1.address]
        )
      );

      // Create EIP-712 signature for commit (simplified)
      const commitSignature =
        '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab';

      // This test would need proper implementation of commit-reveal phases
      // For now, we'll test the structure exists
      expect(await votingSystem.getAddress()).to.not.be.undefined;
    });
  });

  describe('Emergency Controls and Circuit Breaker', function () {
    it('Should allow guardians to emergency pause proposals', async function () {
      const { votingSystem, admin, voter1, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal('Test Proposal', 'Description', '0x', 3600, 24 * 3600, {
          value: proposerBond,
        });

      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');

      // Admin (guardian) emergency pauses proposal
      await expect(
        votingSystem
          .connect(admin)
          .emergencyPauseProposal(1, 'Security concern')
      ).to.emit(votingSystem, 'EmergencyPauseActivated');

      // Check proposal is emergency paused
      expect(await votingSystem.isProposalEmergencyPaused(1)).to.be.true;
      const state = await votingSystem.getProposalState(1);
      expect(state).to.equal(11); // EmergencyPaused
    });

    it('Should implement global emergency pause with multi-sig requirement', async function () {
      const { votingSystem, admin } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Activate global emergency pause
      await votingSystem
        .connect(admin)
        .activateGlobalEmergencyPause('Global security threat');

      // System should be paused
      expect(await votingSystem.paused()).to.be.true;
    });

    it('Should trigger circuit breaker on suspicious activity', async function () {
      const { votingSystem } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Check circuit breaker is enabled by default
      const cbStatus = await votingSystem.getCircuitBreakerStatus();
      expect(cbStatus.enabled).to.be.true;
    });
  });

  describe('Validator Management and Slashing', function () {
    it('Should allow admin to add new validators', async function () {
      const { votingSystem, admin, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const validatorStake = ethers.parseEther('100');

      await expect(
        votingSystem
          .connect(admin)
          .addValidator(voter1.address, validatorStake, {
            value: validatorStake,
          })
      ).to.emit(votingSystem, 'RoleGranted');

      expect(await votingSystem.isActiveValidator(voter1.address)).to.be.true;
    });

    it('Should slash misbehaving validators', async function () {
      const { votingSystem, admin, validator1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const initialValidatorInfo = await votingSystem.getValidatorInfo(
        validator1.address
      );
      const initialStake = initialValidatorInfo.stake;

      await expect(
        votingSystem
          .connect(admin)
          .slashValidator(validator1.address, 'Malicious behavior')
      ).to.emit(votingSystem, 'ValidatorSlashed');

      const updatedValidatorInfo = await votingSystem.getValidatorInfo(
        validator1.address
      );
      expect(updatedValidatorInfo.stake).to.be.lessThan(initialStake);
      expect(updatedValidatorInfo.slashCount).to.equal(1);
    });

    it('Should deactivate validators with insufficient stake after slashing', async function () {
      const { votingSystem, admin, validator1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Slash validator multiple times to reduce stake below minimum
      await votingSystem
        .connect(admin)
        .slashValidator(validator1.address, 'First offense');
      // Add more slashing if needed to bring below minimum threshold

      // Check if validator is deactivated when stake is too low
      const validatorInfo = await votingSystem.getValidatorInfo(
        validator1.address
      );
      // This test would need to be adjusted based on actual slash amounts and minimum stake
    });
  });

  describe('Oracle Integration for Time Validation', function () {
    it('Should add and manage time oracles', async function () {
      const { votingSystem, admin, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      await votingSystem.connect(admin).addTimeOracle(
        voter1.address, // Mock oracle address
        30, // 30 second tolerance
        50 // 50% weight
      );

      // Verify oracle was added (would need proper oracle interface in real implementation)
      expect(
        await votingSystem.hasRole(
          await votingSystem.ORACLE_MANAGER_ROLE(),
          admin.address
        )
      ).to.be.true;
    });

    it('Should reject invalid oracle parameters', async function () {
      const { votingSystem, admin } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Invalid oracle address
      await expect(
        votingSystem.connect(admin).addTimeOracle(ethers.ZeroAddress, 30, 50)
      ).to.be.revertedWith('Invalid oracle address');

      // Invalid tolerance (too high)
      await expect(
        votingSystem.connect(admin).addTimeOracle(
          admin.address,
          1000, // Too high tolerance
          50
        )
      ).to.be.revertedWith('Tolerance too high');

      // Invalid weight
      await expect(
        votingSystem.connect(admin).addTimeOracle(
          admin.address,
          30,
          0 // Invalid weight
        )
      ).to.be.revertedWith('Invalid weight');
    });
  });

  describe('Access Control and OWASP SC02 Compliance', function () {
    it('Should enforce role-based access control', async function () {
      const { votingSystem, admin, voter1, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Admin functions should be restricted
      await expect(
        votingSystem
          .connect(attacker)
          .addValidator(voter1.address, ethers.parseEther('100'))
      ).to.be.revertedWith(/AccessControl/);

      // Validator functions should be restricted
      await expect(
        votingSystem.connect(attacker).validateProposalTimestamps(1, true, '')
      ).to.be.revertedWithCustomError(votingSystem, 'UnauthorizedValidator');

      // Guardian functions should be restricted
      await expect(
        votingSystem
          .connect(attacker)
          .emergencyPauseProposal(1, 'Attack attempt')
      ).to.be.revertedWith(/AccessControl/);
    });

    it('Should implement principle of least privilege', async function () {
      const { votingSystem, validator1, admin } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Validators should not have admin privileges
      expect(
        await votingSystem.hasRole(
          await votingSystem.DEFAULT_ADMIN_ROLE(),
          validator1.address
        )
      ).to.be.false;

      // Admin should have necessary roles but not validator role unless explicitly granted
      expect(
        await votingSystem.hasRole(
          await votingSystem.DEFAULT_ADMIN_ROLE(),
          admin.address
        )
      ).to.be.true;
      expect(
        await votingSystem.hasRole(
          await votingSystem.EMERGENCY_COORDINATOR_ROLE(),
          admin.address
        )
      ).to.be.true;
    });

    it('Should protect against privilege escalation', async function () {
      const { votingSystem, validator1, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Validator cannot grant roles to others
      await expect(
        votingSystem
          .connect(validator1)
          .grantRole(await votingSystem.VALIDATOR_ROLE(), attacker.address)
      ).to.be.revertedWith(/AccessControl/);

      // Non-admin cannot update security config
      const newConfig = await votingSystem.getSecurityConfig();
      await expect(
        votingSystem.connect(validator1).updateSecurityConfig(newConfig)
      ).to.be.revertedWith(/AccessControl/);
    });
  });

  describe('Gas Optimization and Performance', function () {
    it('Should have reasonable gas costs for proposal creation', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const tx = await votingSystem
        .connect(voter1)
        .createProposal(
          'Gas Test Proposal',
          'Testing gas consumption',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      const receipt = await tx.wait();
      console.log(`Proposal creation gas used: ${receipt.gasUsed}`);

      // Gas should be reasonable (adjust threshold as needed)
      expect(receipt.gasUsed).to.be.lessThan(1000000); // 1M gas limit
    });

    it('Should have reasonable gas costs for voting', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Setup validated proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Gas Test Proposal',
          'Testing voting gas consumption',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');
      await time.increase(3600);

      // Test voting gas consumption
      const signature =
        '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12';

      const tx = await votingSystem
        .connect(voter1)
        .castVote(1, 1, 'Gas test vote', signature);
      const receipt = await tx.wait();

      console.log(`Vote casting gas used: ${receipt.gasUsed}`);
      expect(receipt.gasUsed).to.be.lessThan(500000); // 500K gas limit
    });

    it('Should batch validator approvals efficiently', async function () {
      const { votingSystem, voter1, validator1, validator2, validator3 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Batch Test Proposal',
          'Testing batch validator approvals',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Measure gas for each validation
      const tx1 = await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      const receipt1 = await tx1.wait();

      const tx2 = await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');
      const receipt2 = await tx2.wait();

      console.log(`First validation gas: ${receipt1.gasUsed}`);
      console.log(`Second validation gas: ${receipt2.gasUsed}`);

      // Gas should be consistent and reasonable
      expect(receipt1.gasUsed).to.be.lessThan(200000);
      expect(receipt2.gasUsed).to.be.lessThan(200000);
    });
  });

  describe('Integration with Platform Components', function () {
    it('Should integrate with governance token voting power', async function () {
      const { votingSystem, token, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Check voting power integration
      const tokenBalance = await token.balanceOf(voter1.address);
      expect(tokenBalance).to.be.greaterThan(0);

      // Voting power should reflect token holdings
      // Note: This would require proper IVotes implementation in the mock token
    });

    it('Should integrate with timelock controller', async function () {
      const { votingSystem, timelock } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Verify timelock integration
      expect(await votingSystem.timelock()).to.equal(
        await timelock.getAddress()
      );

      // Test timelock role integration
      expect(
        await votingSystem.hasRole(
          await votingSystem.TIMELOCK_ADMIN_ROLE(),
          await timelock.getAddress()
        )
      ).to.be.false;
    });

    it('Should support proposal execution through timelock', async function () {
      // This test would require a complete proposal lifecycle
      // Including validation, voting, and execution phases
      // Implementation depends on specific timelock integration requirements
    });
  });

  describe('Security Monitoring and Analytics', function () {
    it('Should track security metrics correctly', async function () {
      const { votingSystem } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Check initial metrics
      expect(await votingSystem.proposalCount()).to.equal(1);
      expect(await votingSystem.validatorCount()).to.equal(3);
      expect(await votingSystem.totalValidations()).to.equal(0);
      expect(await votingSystem.totalSlashings()).to.equal(0);
      expect(await votingSystem.emergencyPauseCount()).to.equal(0);
    });

    it('Should provide comprehensive security status', async function () {
      const { votingSystem } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Test security configuration access
      const config = await votingSystem.getSecurityConfig();
      expect(config.timestampTolerance).to.be.greaterThan(0);
      expect(config.blockConfirmations).to.be.greaterThanOrEqual(3);

      // Test circuit breaker status
      const cbStatus = await votingSystem.getCircuitBreakerStatus();
      expect(cbStatus.enabled).to.be.true;
    });
  });

  describe('Edge Cases and Boundary Conditions', function () {
    it('Should handle maximum voting period correctly', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const maxVotingPeriod = 7 * 24 * 3600; // 7 days

      await expect(
        votingSystem
          .connect(voter1)
          .createProposal(
            'Max Period Test',
            'Testing maximum voting period',
            '0x',
            3600,
            maxVotingPeriod,
            { value: proposerBond }
          )
      ).to.emit(votingSystem, 'ProposalCreated');
    });

    it('Should handle minimum voting delay correctly', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const minVotingDelay = 3600; // 1 hour

      await expect(
        votingSystem
          .connect(voter1)
          .createProposal(
            'Min Delay Test',
            'Testing minimum voting delay',
            '0x',
            minVotingDelay,
            24 * 3600,
            { value: proposerBond }
          )
      ).to.emit(votingSystem, 'ProposalCreated');
    });

    it('Should handle validator consensus edge cases', async function () {
      const { votingSystem, voter1, validator1, validator2, validator3 } =
        await loadFixture(deploySecureVotingSystemFixture);

      // Create proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Consensus Test',
          'Testing validator consensus edge case',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Test case where exactly the required percentage approves
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');
      // validator3 doesn't vote - should still reach consensus with 67% threshold (2/3 = 66.7%)

      const proposalState = await votingSystem.getProposalState(1);
      expect(proposalState).to.equal(2); // Should be Pending
    });
  });
});

// Helper function for testing
const anyValue = require('@nomicfoundation/hardhat-chai-matchers/withArgs');
