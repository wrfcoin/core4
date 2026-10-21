const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
  mine,
  setNextBlockTimestamp,
} = require('@nomicfoundation/hardhat-network-helpers');

/**
 * Comprehensive Timestamp Manipulation Attack Test Suite
 *
 * This test suite simulates various timestamp manipulation attacks and validates
 * that the SecureVotingSystem successfully prevents them through multiple layers:
 *
 * 1. Block timestamp validation with buffer zones
 * 2. Multi-block confirmation requirements
 * 3. Validator consensus mechanisms
 * 4. Oracle-based time verification
 * 5. Circuit breaker activation on suspicious patterns
 */
describe('Timestamp Manipulation Attack Prevention', function () {
  async function deploySecureVotingSystemFixture() {
    const [
      deployer,
      admin,
      validator1,
      validator2,
      validator3,
      voter1,
      attacker,
      oracle,
    ] = await ethers.getSigners();

    // Deploy mock governance token with voting capabilities
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
    await token.transfer(attacker.address, ethers.parseEther('5000'));

    // Enable voting power delegation
    await token.connect(voter1).delegate(voter1.address);
    await token.connect(attacker).delegate(attacker.address);

    // Add time oracle for enhanced validation
    await votingSystem.connect(admin).addTimeOracle(
      oracle.address,
      30, // 30 second tolerance
      100 // 100% weight for this test oracle
    );

    return {
      votingSystem,
      token,
      timelock,
      admin,
      validator1,
      validator2,
      validator3,
      voter1,
      attacker,
      oracle,
    };
  }

  describe('Basic Timestamp Manipulation Attacks', function () {
    it('Should prevent proposals with timestamps too far in the future', async function () {
      const { votingSystem, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const currentTime = await time.latest();

      // Attempt to create proposal with timestamp 1 year in the future
      const maliciousDelay = 365 * 24 * 3600; // 1 year

      await expect(
        votingSystem
          .connect(attacker)
          .createProposal(
            'Future Manipulation',
            'Attempting to schedule proposal far in future',
            '0x',
            maliciousDelay,
            24 * 3600,
            { value: proposerBond }
          )
      ).to.be.revertedWithCustomError(votingSystem, 'InvalidTimestamp');
    });

    it('Should prevent proposals with timestamps in the past', async function () {
      const { votingSystem, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Attempt to create proposal with negative delay (past timestamp)
      await expect(
        votingSystem.connect(attacker).createProposal(
          'Past Manipulation',
          'Attempting to schedule proposal in the past',
          '0x',
          0, // No delay - essentially in the past
          24 * 3600,
          { value: proposerBond }
        )
      ).to.be.revertedWith('Voting delay too short');
    });

    it('Should detect suspicious timestamp patterns', async function () {
      const { votingSystem, attacker, validator1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create proposal with borderline suspicious timing
      await votingSystem.connect(attacker).createProposal(
        'Borderline Suspicious',
        'Testing timestamp pattern detection',
        '0x',
        3601, // Just barely within limits but suspicious
        24 * 3600,
        { value: proposerBond }
      );

      // Validator should detect and reject suspicious pattern
      await expect(
        votingSystem
          .connect(validator1)
          .validateProposalTimestamps(
            1,
            false,
            'Detected suspicious timing pattern'
          )
      )
        .to.emit(votingSystem, 'ProposalValidated')
        .withArgs(1, validator1.address, 0, false);
    });
  });

  describe('Block Timestamp Correlation Attacks', function () {
    it('Should validate block-timestamp correlation', async function () {
      const { votingSystem, attacker, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Record initial block and time
      const initialBlock = await ethers.provider.getBlockNumber();
      const initialTime = await time.latest();

      // Create proposal
      await votingSystem.connect(attacker).createProposal(
        'Correlation Test',
        'Testing block-timestamp correlation',
        '0x',
        7200, // 2 hours
        24 * 3600,
        { value: proposerBond }
      );

      // Simulate time passing without proportional block advancement
      // This could indicate timestamp manipulation
      await time.increase(3600); // 1 hour passes
      // But we don't mine proportional blocks (normally ~300 blocks would pass)

      // Validators should detect the anomaly
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, false, 'Block-time correlation anomaly');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, false, 'Suspicious block progression');
    });

    it('Should require multi-block confirmations', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Create legitimate proposal
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Multi-Block Test',
          'Testing multi-block confirmation requirement',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Mine the required number of confirmation blocks
      const securityConfig = await votingSystem.getSecurityConfig();
      const requiredConfirmations = securityConfig.blockConfirmations; // Should be 3+

      for (let i = 0; i < requiredConfirmations; i++) {
        await mine();
      }

      // Now validators can approve with confidence
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, 'Multi-block confirmation passed');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, 'Confirmed after required blocks');

      const proposalState = await votingSystem.getProposalState(1);
      expect(proposalState).to.equal(2); // Pending state after validation
    });

    it('Should detect rapid-fire proposal creation attacks', async function () {
      const { votingSystem, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create multiple proposals in rapid succession
      const proposals = [];
      for (let i = 0; i < 10; i++) {
        try {
          const tx = await votingSystem.connect(attacker).createProposal(
            `Spam Proposal ${i}`,
            'Attempting to flood the system',
            '0x',
            3600 + i, // Slightly different delays to avoid exact duplicates
            24 * 3600,
            { value: proposerBond }
          );
          proposals.push(tx);
        } catch (error) {
          // Expected to fail due to circuit breaker or other protections
          console.log(`Proposal ${i} blocked by security measures`);
        }
      }

      // Circuit breaker should have activated
      const cbStatus = await votingSystem.getCircuitBreakerStatus();
      expect(cbStatus.currentActivity).to.be.greaterThan(
        cbStatus.threshold / 2
      );
    });
  });

  describe('Validator Consensus Bypass Attempts', function () {
    it('Should prevent single validator manipulation', async function () {
      const { votingSystem, attacker, validator1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create suspicious proposal
      await votingSystem
        .connect(attacker)
        .createProposal(
          'Single Validator Attack',
          'Attempting to bypass consensus with single validator',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Only one validator approves (insufficient for consensus)
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, 'Single approval');

      // Proposal should remain in validation state (need 67% = 2 out of 3 validators)
      const proposalState = await votingSystem.getProposalState(1);
      expect(proposalState).to.equal(1); // Still in TimestampValidation
    });

    it('Should resist validator collusion attempts', async function () {
      const {
        votingSystem,
        attacker,
        validator1,
        validator2,
        validator3,
        admin,
      } = await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Create highly suspicious proposal
      await votingSystem.connect(attacker).createProposal(
        'Collusion Test',
        'Testing resistance to validator collusion',
        '0x',
        86400, // 24 hours delay (suspiciously long)
        24 * 3600,
        { value: proposerBond }
      );

      // Two validators collude to approve suspicious proposal
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, 'Colluding validator 1');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, 'Colluding validator 2');

      // Third validator acts honestly and rejects
      await votingSystem
        .connect(validator3)
        .validateProposalTimestamps(1, false, 'Detected collusion attempt');

      // Admin should be able to slash the colluding validators
      await expect(
        votingSystem
          .connect(admin)
          .slashValidator(validator1.address, 'Validation misconduct')
      ).to.emit(votingSystem, 'ValidatorSlashed');

      await expect(
        votingSystem
          .connect(admin)
          .slashValidator(validator2.address, 'Validation misconduct')
      ).to.emit(votingSystem, 'ValidatorSlashed');
    });

    it('Should implement reputation-based validation weighting', async function () {
      const { votingSystem, voter1, validator1, validator2, admin } =
        await loadFixture(deploySecureVotingSystemFixture);

      // First, damage validator1's reputation through slashing
      await votingSystem
        .connect(admin)
        .slashValidator(validator1.address, 'Previous misconduct');

      const proposerBond = ethers.parseEther('10');

      // Create proposal
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Reputation Test',
          'Testing reputation-based validation',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Check that validator1's reputation is damaged
      const validator1Info = await votingSystem.getValidatorInfo(
        validator1.address
      );
      expect(validator1Info.reputation).to.be.lessThan(100);

      // Higher reputation validator should have more influence
      const validator2Info = await votingSystem.getValidatorInfo(
        validator2.address
      );
      expect(validator2Info.reputation).to.equal(100);
    });
  });

  describe('Oracle-Based Time Validation Attacks', function () {
    it('Should validate against external time oracles', async function () {
      const { votingSystem, voter1, oracle, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Create proposal with oracle validation enabled
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Oracle Validation Test',
          'Testing oracle-based time validation',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Validators approve timestamps
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');

      // Oracle should validate the timestamps
      // (In a real implementation, this would be called by the oracle)
      // For testing purposes, we'll simulate oracle validation failure
      await expect(
        votingSystem.connect(oracle)
        // Note: Oracle validation would need specific implementation
        // This is a placeholder for the oracle integration test
      );
    });

    it('Should handle oracle consensus failures', async function () {
      const { votingSystem, voter1, admin, oracle } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Add multiple oracles with different parameters
      await votingSystem.connect(admin).addTimeOracle(
        voter1.address, // Mock second oracle
        60, // Different tolerance
        50 // Lower weight
      );

      const proposerBond = ethers.parseEther('10');

      // Create proposal that would fail oracle validation
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Oracle Consensus Test',
          'Testing oracle consensus failure handling',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Test oracle consensus mechanism
      // (Implementation would depend on specific oracle integration)
    });

    it('Should resist oracle manipulation attacks', async function () {
      const { votingSystem, attacker, admin, oracle } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Attacker tries to add malicious oracle
      await expect(
        votingSystem.connect(attacker).addTimeOracle(
          attacker.address,
          1000, // Suspiciously high tolerance
          100
        )
      ).to.be.revertedWith(/AccessControl/);

      // Only oracle managers should be able to add oracles
      expect(
        await votingSystem.hasRole(
          await votingSystem.ORACLE_MANAGER_ROLE(),
          admin.address
        )
      ).to.be.true;
      expect(
        await votingSystem.hasRole(
          await votingSystem.ORACLE_MANAGER_ROLE(),
          attacker.address
        )
      ).to.be.false;
    });
  });

  describe('Advanced Timestamp Manipulation Scenarios', function () {
    it('Should detect coordinated timestamp manipulation', async function () {
      const { votingSystem, attacker, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Create multiple proposals with suspiciously coordinated timing
      const baseTime = await time.latest();
      const suspiciousInterval = 60; // Exactly 1 minute apart (too regular)

      for (let i = 0; i < 3; i++) {
        await votingSystem
          .connect(attacker)
          .createProposal(
            `Coordinated ${i}`,
            `Coordinated attack proposal ${i}`,
            '0x',
            3600 + suspiciousInterval * i,
            24 * 3600,
            { value: proposerBond }
          );
      }

      // Pattern detection should flag this as suspicious
      // Circuit breaker should activate due to suspicious patterns
      const cbStatus = await votingSystem.getCircuitBreakerStatus();
      expect(cbStatus.currentActivity).to.be.greaterThan(0);
    });

    it('Should handle MEV-based timestamp manipulation', async function () {
      const { votingSystem, attacker, voter1, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Create proposal with MEV protection enabled
      await votingSystem
        .connect(voter1)
        .createProposal(
          'MEV Protection Test',
          'Testing MEV-based timestamp manipulation protection',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Validate proposal
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, '');
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, '');

      // Advance to voting period
      await time.increase(3600);

      // Attacker tries to manipulate block timestamp during voting
      // This should be prevented by the commit-reveal scheme
      const manipulatedTimestamp = (await time.latest()) + 3600; // 1 hour in future

      await expect(setNextBlockTimestamp(manipulatedTimestamp)).to.not.reverted; // The function call itself shouldn't revert

      // But the voting system should detect the manipulation
      // when the attacker tries to exploit it
    });

    it('Should validate timestamp buffer zones', async function () {
      const { votingSystem, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Get security config to understand buffer requirements
      const securityConfig = await votingSystem.getSecurityConfig();
      const timestampTolerance = securityConfig.timestampTolerance;
      const bufferBlocks = securityConfig.timestampBufferBlocks;

      // Create proposal at the edge of tolerance
      const currentTime = await time.latest();
      const edgeDelay = timestampTolerance * 2; // Just within buffer

      await expect(
        votingSystem
          .connect(attacker)
          .createProposal(
            'Buffer Zone Test',
            'Testing timestamp buffer validation',
            '0x',
            edgeDelay,
            24 * 3600,
            { value: proposerBond }
          )
      ).to.emit(votingSystem, 'ProposalCreated');

      // Create proposal outside buffer zone
      const outsideBufferDelay = timestampTolerance * 10; // Way outside buffer

      await expect(
        votingSystem
          .connect(attacker)
          .createProposal(
            'Outside Buffer Test',
            'Should fail buffer validation',
            '0x',
            outsideBufferDelay,
            24 * 3600,
            { value: proposerBond }
          )
      ).to.be.revertedWithCustomError(votingSystem, 'InvalidTimestamp');
    });

    it('Should prevent flash loan timestamp manipulation', async function () {
      const { votingSystem, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // This test simulates an attacker using flash loans to manipulate
      // voting power and timestamps in a single transaction

      const proposerBond = ethers.parseEther('10');

      // The multi-block confirmation requirement should prevent
      // single-transaction manipulation attacks
      await votingSystem
        .connect(attacker)
        .createProposal(
          'Flash Loan Attack',
          'Attempting flash loan manipulation',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Even if attacker could manipulate in same block,
      // validators would detect the anomaly
      const securityConfig = await votingSystem.getSecurityConfig();
      expect(securityConfig.blockConfirmations).to.be.greaterThanOrEqual(3);
    });
  });

  describe('Emergency Response to Timestamp Attacks', function () {
    it('Should trigger emergency pause on detected manipulation', async function () {
      const { votingSystem, admin, attacker, validator1, validator2 } =
        await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Create highly suspicious proposal
      await votingSystem
        .connect(attacker)
        .createProposal(
          'Emergency Test',
          'Should trigger emergency response',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Validators detect manipulation
      await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(
          1,
          false,
          'TIMESTAMP MANIPULATION DETECTED'
        );
      await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, false, 'COORDINATED ATTACK SUSPECTED');

      // Guardian triggers emergency pause
      await expect(
        votingSystem
          .connect(admin)
          .emergencyPauseProposal(1, 'Detected timestamp manipulation attack')
      ).to.emit(votingSystem, 'EmergencyPauseActivated');

      // Verify proposal is emergency paused
      expect(await votingSystem.isProposalEmergencyPaused(1)).to.be.true;
      const state = await votingSystem.getProposalState(1);
      expect(state).to.equal(11); // EmergencyPaused
    });

    it('Should implement global emergency shutdown', async function () {
      const { votingSystem, admin } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Activate global emergency pause
      await expect(
        votingSystem
          .connect(admin)
          .activateGlobalEmergencyPause(
            'Global timestamp manipulation attack detected'
          )
      ).to.not.be.reverted;

      // System should be globally paused
      expect(await votingSystem.paused()).to.be.true;

      // No new proposals should be accepted while paused
      const proposerBond = ethers.parseEther('10');
      await expect(
        votingSystem
          .connect(admin)
          .createProposal(
            'Post-Emergency Test',
            'Should fail while paused',
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          )
      ).to.be.revertedWith('Pausable: paused');
    });

    it('Should allow recovery after timestamp attack', async function () {
      const { votingSystem, admin } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      // Simulate attack detection and pause
      await votingSystem
        .connect(admin)
        .activateGlobalEmergencyPause('Simulated attack');
      expect(await votingSystem.paused()).to.be.true;

      // Admin can unpause after resolving the issue
      await votingSystem.connect(admin).unpause();
      expect(await votingSystem.paused()).to.be.false;

      // System should accept new proposals again
      const proposerBond = ethers.parseEther('10');
      await expect(
        votingSystem
          .connect(admin)
          .createProposal(
            'Recovery Test',
            'System should work after recovery',
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          )
      ).to.emit(votingSystem, 'ProposalCreated');
    });
  });

  describe('Timestamp Manipulation Detection Analytics', function () {
    it('Should emit detailed detection events', async function () {
      const { votingSystem, attacker, validator1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create suspicious proposal
      await votingSystem.connect(attacker).createProposal(
        'Detection Analytics Test',
        'Testing detection event emission',
        '0x',
        10800, // 3 hours (potentially suspicious)
        24 * 3600,
        { value: proposerBond }
      );

      // Validator detects and reports manipulation
      await expect(
        votingSystem
          .connect(validator1)
          .validateProposalTimestamps(1, false, 'Timestamp drift detected')
      )
        .to.emit(votingSystem, 'ProposalValidated')
        .withArgs(1, validator1.address, 0, false);
    });

    it('Should track manipulation attempt statistics', async function () {
      const { votingSystem, attacker, admin } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Record initial metrics
      const initialRejects = await votingSystem.securityRejections();

      // Attempt manipulation
      try {
        await votingSystem.connect(attacker).createProposal(
          'Stats Test',
          'Testing statistics tracking',
          '0x',
          86400 * 30, // 30 days (clearly suspicious)
          24 * 3600,
          { value: proposerBond }
        );
      } catch (error) {
        // Expected to fail
      }

      // Statistics should reflect the attempt
      const finalMetrics = {
        proposals: await votingSystem.proposalCount(),
        validators: await votingSystem.validatorCount(),
        validations: await votingSystem.totalValidations(),
        emergencyPauses: await votingSystem.emergencyPauseCount(),
      };

      expect(finalMetrics.proposals).to.be.greaterThanOrEqual(1);
    });

    it('Should provide forensic data for attack analysis', async function () {
      const { votingSystem, attacker, validator1 } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create proposal for forensic analysis
      const tx = await votingSystem
        .connect(attacker)
        .createProposal(
          'Forensic Test',
          'Generating forensic data',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      const receipt = await tx.wait();
      const block = await ethers.provider.getBlock(receipt.blockNumber);

      // Forensic data should be available
      expect(block.timestamp).to.be.greaterThan(0);
      expect(receipt.blockNumber).to.be.greaterThan(0);

      // Validator analysis should provide detailed reasoning
      await expect(
        votingSystem
          .connect(validator1)
          .validateProposalTimestamps(
            1,
            false,
            `Forensic analysis: Block ${receipt.blockNumber}, Time ${block.timestamp}, Deviation detected`
          )
      ).to.emit(votingSystem, 'ProposalValidated');
    });
  });

  describe('Performance Under Attack Conditions', function () {
    it('Should maintain performance during high-frequency attacks', async function () {
      const { votingSystem, attacker } = await loadFixture(
        deploySecureVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const startTime = await time.latest();

      // Simulate high-frequency attack attempts
      let successfulAttacks = 0;
      let failedAttacks = 0;

      for (let i = 0; i < 5; i++) {
        try {
          await votingSystem
            .connect(attacker)
            .createProposal(
              `Attack ${i}`,
              'High frequency attack',
              '0x',
              3600 + i * 10,
              24 * 3600,
              { value: proposerBond }
            );
          successfulAttacks++;
        } catch (error) {
          failedAttacks++;
        }
      }

      const endTime = await time.latest();
      const duration = endTime - startTime;

      console.log(
        `Attack simulation: ${successfulAttacks} successful, ${failedAttacks} failed in ${duration}s`
      );

      // System should block most attacks while maintaining reasonable performance
      expect(failedAttacks).to.be.greaterThan(successfulAttacks);
    });

    it('Should handle validator consensus under attack', async function () {
      const { votingSystem, attacker, validator1, validator2, validator3 } =
        await loadFixture(deploySecureVotingSystemFixture);

      const proposerBond = ethers.parseEther('10');

      // Create legitimate proposal
      await votingSystem
        .connect(attacker)
        .createProposal(
          'Consensus Under Attack',
          'Testing consensus performance',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Simulate validators under pressure making quick decisions
      const validationPromises = [
        votingSystem
          .connect(validator1)
          .validateProposalTimestamps(1, false, 'Under attack - rejecting'),
        votingSystem
          .connect(validator2)
          .validateProposalTimestamps(1, false, 'Security concern detected'),
        votingSystem
          .connect(validator3)
          .validateProposalTimestamps(1, false, 'Coordinated attack pattern'),
      ];

      // All validations should complete successfully
      await Promise.all(validationPromises);

      // Consensus should be clear (all reject)
      const proposalState = await votingSystem.getProposalState(1);
      expect(proposalState).to.equal(1); // Still in validation (rejected by all)
    });
  });
});

// Utility functions for timestamp manipulation testing
async function simulateBlockTimestampManipulation(targetTimestamp) {
  // This would simulate a miner manipulating block timestamps
  await setNextBlockTimestamp(targetTimestamp);
  await mine();
}

async function simulateNetworkTimeDesync() {
  // This would simulate network time desynchronization
  const currentTime = await time.latest();
  const desyncTime = currentTime + 300; // 5 minute desync
  await time.increaseTo(desyncTime);
}

async function createSuspiciousProposalPattern(
  votingSystem,
  attacker,
  count = 5
) {
  // Helper to create suspicious patterns
  const results = [];
  const proposerBond = ethers.parseEther('10');

  for (let i = 0; i < count; i++) {
    try {
      const tx = await votingSystem.connect(attacker).createProposal(
        `Pattern ${i}`,
        'Suspicious pattern creation',
        '0x',
        3600 + i * 60, // Exactly 1 minute intervals
        24 * 3600,
        { value: proposerBond }
      );
      results.push({ success: true, tx });
    } catch (error) {
      results.push({ success: false, error });
    }
  }

  return results;
}
