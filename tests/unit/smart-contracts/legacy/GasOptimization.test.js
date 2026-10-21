const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

/**
 * Gas Optimization and Performance Benchmark Test Suite
 *
 * This comprehensive test suite validates gas efficiency and performance
 * of the SecureVotingSystem under various conditions:
 *
 * 1. Gas consumption benchmarks for all major operations
 * 2. Performance under high load conditions
 * 3. Optimization verification for batch operations
 * 4. Memory usage and storage efficiency tests
 * 5. Gas cost analysis for security features
 */
describe('Gas Optimization and Performance Benchmarks', function () {
  async function deployOptimizedVotingSystemFixture() {
    const [
      deployer,
      admin,
      validator1,
      validator2,
      validator3,
      voter1,
      voter2,
      voter3,
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
      24 * 60 * 60,
      [admin.address],
      [admin.address],
      admin.address
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

    // Setup token balances for performance testing
    const addresses = [voter1, voter2, voter3];
    const balances = [
      ethers.parseEther('100000'), // Large holder
      ethers.parseEther('50000'), // Medium holder
      ethers.parseEther('10000'), // Small holder
    ];

    for (let i = 0; i < addresses.length; i++) {
      await token.transfer(addresses[i].address, balances[i]);
      await token.connect(addresses[i]).delegate(addresses[i].address);
    }

    return {
      votingSystem,
      token,
      timelock,
      admin,
      validator1,
      validator2,
      validator3,
      voter1,
      voter2,
      voter3,
    };
  }

  describe('Proposal Creation Gas Benchmarks', function () {
    it('Should have optimal gas consumption for basic proposal creation', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const gasEstimate = await votingSystem
        .connect(voter1)
        .createProposal.estimateGas(
          'Gas Benchmark Proposal',
          'Testing gas consumption for proposal creation',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      console.log(`Proposal creation gas estimate: ${gasEstimate.toString()}`);

      // Execute transaction and measure actual gas used
      const tx = await votingSystem
        .connect(voter1)
        .createProposal(
          'Gas Benchmark Proposal',
          'Testing gas consumption for proposal creation',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      const receipt = await tx.wait();
      console.log(
        `Proposal creation actual gas used: ${receipt.gasUsed.toString()}`
      );

      // Gas should be reasonable for the security features provided
      expect(receipt.gasUsed).to.be.lessThan(
        ethers.parseUnits('800000', 'wei')
      ); // 800K gas limit
      expect(receipt.gasUsed).to.be.greaterThan(
        ethers.parseUnits('100000', 'wei')
      ); // Should be doing meaningful work
    });

    it('Should scale efficiently with proposal data size', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const gasResults = [];

      // Test different proposal data sizes
      const dataSizes = [0, 32, 256, 1024]; // bytes

      for (const size of dataSizes) {
        const proposalData =
          size > 0 ? ethers.hexlify(ethers.randomBytes(size)) : '0x';

        const tx = await votingSystem
          .connect(voter1)
          .createProposal(
            `Gas Test ${size}`,
            `Testing gas with ${size} bytes of data`,
            proposalData,
            3600,
            24 * 3600,
            { value: proposerBond }
          );

        const receipt = await tx.wait();
        gasResults.push({
          size: size,
          gasUsed: receipt.gasUsed.toString(),
        });

        console.log(`Proposal with ${size} bytes data: ${receipt.gasUsed} gas`);
      }

      // Gas should scale predictably with data size
      for (let i = 1; i < gasResults.length; i++) {
        const prevGas = BigInt(gasResults[i - 1].gasUsed);
        const currentGas = BigInt(gasResults[i].gasUsed);
        const gasIncrease = currentGas - prevGas;

        console.log(
          `Gas increase from ${gasResults[i - 1].size} to ${gasResults[i].size} bytes: ${gasIncrease}`
        );

        // Reasonable scaling - should not be exponential
        expect(gasIncrease).to.be.lessThan(ethers.parseUnits('200000', 'wei'));
      }
    });

    it('Should optimize storage for proposal metadata', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create proposal and measure storage-related gas costs
      const tx = await votingSystem.connect(voter1).createProposal(
        'Storage Optimization Test',
        'Testing storage efficiency for proposal metadata',
        '0x1234', // Small data payload
        3600,
        24 * 3600,
        { value: proposerBond }
      );

      const receipt = await tx.wait();

      // Analyze gas breakdown (this would require detailed gas profiling)
      console.log(`Storage optimization test gas: ${receipt.gasUsed}`);

      // Verify proposal data is stored efficiently
      const proposal = await votingSystem.proposals(1);
      expect(proposal.id).to.equal(1);
      expect(proposal.proposer).to.equal(voter1.address);
    });
  });

  describe('Validator Consensus Gas Efficiency', function () {
    it('Should minimize gas costs for validator consensus', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Create proposal first
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Consensus Gas Test',
          'Testing validator consensus gas costs',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Measure gas for first validator approval
      const tx1 = await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, 'First approval');
      const receipt1 = await tx1.wait();
      console.log(`First validator approval gas: ${receipt1.gasUsed}`);

      // Measure gas for second validator approval (should trigger state change)
      const tx2 = await votingSystem
        .connect(validator2)
        .validateProposalTimestamps(1, true, 'Second approval - consensus');
      const receipt2 = await tx2.wait();
      console.log(
        `Second validator approval gas (consensus): ${receipt2.gasUsed}`
      );

      // Gas costs should be reasonable
      expect(receipt1.gasUsed).to.be.lessThan(
        ethers.parseUnits('150000', 'wei')
      );
      expect(receipt2.gasUsed).to.be.lessThan(
        ethers.parseUnits('200000', 'wei')
      ); // May be higher due to state transition
    });

    it('Should batch validator operations efficiently', async function () {
      const { votingSystem, voter1, validator1, validator2, validator3 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Create multiple proposals for batch testing
      const proposerBond = ethers.parseEther('10');
      const proposalIds = [];

      for (let i = 0; i < 3; i++) {
        await votingSystem
          .connect(voter1)
          .createProposal(
            `Batch Test ${i}`,
            `Batch validation test proposal ${i}`,
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          );
        proposalIds.push(i + 1);
      }

      // Measure gas for sequential validations
      const gasUsed = [];
      for (const proposalId of proposalIds) {
        const tx = await votingSystem
          .connect(validator1)
          .validateProposalTimestamps(proposalId, true, 'Batch validation');
        const receipt = await tx.wait();
        gasUsed.push(receipt.gasUsed);
        console.log(`Validation ${proposalId} gas: ${receipt.gasUsed}`);
      }

      // Gas should be consistent across validations
      const avgGas =
        gasUsed.reduce((sum, gas) => sum + gas, BigInt(0)) /
        BigInt(gasUsed.length);
      console.log(`Average validation gas: ${avgGas}`);

      for (const gas of gasUsed) {
        const deviation = gas > avgGas ? gas - avgGas : avgGas - gas;
        expect(deviation).to.be.lessThan(avgGas / BigInt(10)); // Less than 10% deviation
      }
    });

    it('Should optimize repeated validator calls', async function () {
      const { votingSystem, voter1, validator1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      // Create proposals
      const proposerBond = ethers.parseEther('10');
      for (let i = 0; i < 5; i++) {
        await votingSystem
          .connect(voter1)
          .createProposal(
            `Repeated Call Test ${i}`,
            `Testing repeated validator calls optimization`,
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          );
      }

      // Measure gas for repeated validator calls from same validator
      const gasResults = [];
      for (let i = 1; i <= 5; i++) {
        const tx = await votingSystem
          .connect(validator1)
          .validateProposalTimestamps(i, true, `Repeated call ${i}`);
        const receipt = await tx.wait();
        gasResults.push(receipt.gasUsed);
        console.log(`Repeated call ${i} gas: ${receipt.gasUsed}`);
      }

      // Later calls should not be significantly more expensive due to warm storage
      const firstCallGas = gasResults[0];
      const lastCallGas = gasResults[gasResults.length - 1];
      const gasIncrease =
        lastCallGas > firstCallGas ? lastCallGas - firstCallGas : BigInt(0);

      console.log(`Gas increase from first to last call: ${gasIncrease}`);
      expect(gasIncrease).to.be.lessThan(firstCallGas / BigInt(5)); // Should not increase by more than 20%
    });
  });

  describe('Voting Operation Gas Benchmarks', function () {
    it('Should optimize gas for standard voting operations', async function () {
      const { votingSystem, voter1, voter2, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Setup validated proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Voting Gas Test',
          'Testing voting operation gas efficiency',
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

      // First vote
      const tx1 = await votingSystem
        .connect(voter1)
        .castVote(1, 1, 'First vote', signature);
      const receipt1 = await tx1.wait();
      console.log(`First vote gas: ${receipt1.gasUsed}`);

      // Second vote (different voter)
      const signature2 =
        '0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab';
      const tx2 = await votingSystem
        .connect(voter2)
        .castVote(1, 0, 'Second vote', signature2);
      const receipt2 = await tx2.wait();
      console.log(`Second vote gas: ${receipt2.gasUsed}`);

      // Voting gas should be reasonable
      expect(receipt1.gasUsed).to.be.lessThan(
        ethers.parseUnits('300000', 'wei')
      );
      expect(receipt2.gasUsed).to.be.lessThan(
        ethers.parseUnits('300000', 'wei')
      );
    });

    it('Should scale efficiently with voter count', async function () {
      const { votingSystem, voter1, voter2, voter3, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Setup proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Scalability Test',
          'Testing voting scalability',
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

      // Test multiple voters
      const voters = [voter1, voter2, voter3];
      const gasResults = [];

      for (let i = 0; i < voters.length; i++) {
        const signature = ethers.hexlify(ethers.randomBytes(65)); // Random signature for each vote
        const tx = await votingSystem
          .connect(voters[i])
          .castVote(1, 1, `Vote ${i}`, signature);
        const receipt = await tx.wait();
        gasResults.push(receipt.gasUsed);
        console.log(`Vote ${i + 1} gas: ${receipt.gasUsed}`);
      }

      // Gas should remain relatively stable as more voters participate
      const avgGas =
        gasResults.reduce((sum, gas) => sum + gas, BigInt(0)) /
        BigInt(gasResults.length);
      console.log(`Average voting gas: ${avgGas}`);

      for (const gas of gasResults) {
        const deviation = gas > avgGas ? gas - avgGas : avgGas - gas;
        expect(deviation).to.be.lessThan(avgGas / BigInt(4)); // Less than 25% deviation
      }
    });

    it('Should optimize quadratic voting calculations', async function () {
      const { votingSystem, voter1, validator1, validator2, admin } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Enable quadratic voting
      const currentConfig = await votingSystem.getSecurityConfig();
      const newConfig = { ...currentConfig, quadraticVotingEnabled: true };
      await votingSystem.connect(admin).updateSecurityConfig(newConfig);

      // Create proposal with quadratic voting
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Quadratic Voting Test',
          'Testing quadratic voting gas efficiency',
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

      // Test quadratic voting gas cost
      const signature = ethers.hexlify(ethers.randomBytes(65));
      const tx = await votingSystem
        .connect(voter1)
        .castVote(1, 1, 'Quadratic vote', signature);
      const receipt = await tx.wait();

      console.log(`Quadratic voting gas: ${receipt.gasUsed}`);

      // Quadratic voting should have acceptable gas overhead
      expect(receipt.gasUsed).to.be.lessThan(
        ethers.parseUnits('400000', 'wei')
      );
    });
  });

  describe('Security Feature Gas Overhead Analysis', function () {
    it('Should measure gas overhead of EIP-712 signature verification', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Create and validate proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'EIP-712 Overhead Test',
          'Measuring EIP-712 signature verification overhead',
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

      // Measure gas with EIP-712 verification
      const signature = ethers.hexlify(ethers.randomBytes(65));
      const tx = await votingSystem
        .connect(voter1)
        .castVote(1, 1, 'EIP-712 test', signature);
      const receipt = await tx.wait();

      console.log(`EIP-712 verification gas overhead: ${receipt.gasUsed}`);

      // EIP-712 overhead should be reasonable (signature verification is ~3000-5000 gas)
      expect(receipt.gasUsed).to.be.lessThan(
        ethers.parseUnits('350000', 'wei')
      );
    });

    it('Should measure circuit breaker gas overhead', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      // Enable circuit breaker
      const cbStatus = await votingSystem.getCircuitBreakerStatus();
      expect(cbStatus.enabled).to.be.true;

      // Create proposal with circuit breaker active
      const proposerBond = ethers.parseEther('10');
      const tx = await votingSystem
        .connect(voter1)
        .createProposal(
          'Circuit Breaker Overhead Test',
          'Measuring circuit breaker gas overhead',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      const receipt = await tx.wait();
      console.log(`Circuit breaker overhead gas: ${receipt.gasUsed}`);

      // Circuit breaker should add minimal overhead
      expect(receipt.gasUsed).to.be.lessThan(
        ethers.parseUnits('850000', 'wei')
      );
    });

    it('Should measure timestamp validation overhead', async function () {
      const { votingSystem, voter1, validator1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      // Create proposal (includes timestamp validation)
      const proposerBond = ethers.parseEther('10');
      const tx1 = await votingSystem
        .connect(voter1)
        .createProposal(
          'Timestamp Validation Test',
          'Measuring timestamp validation overhead',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      const receipt1 = await tx1.wait();
      console.log(
        `Proposal creation with timestamp validation: ${receipt1.gasUsed}`
      );

      // Validator timestamp validation
      const tx2 = await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, 'Validation test');
      const receipt2 = await tx2.wait();
      console.log(`Validator timestamp validation: ${receipt2.gasUsed}`);

      // Timestamp validation overhead should be acceptable
      expect(receipt2.gasUsed).to.be.lessThan(
        ethers.parseUnits('200000', 'wei')
      );
    });

    it('Should analyze emergency control gas costs', async function () {
      const { votingSystem, admin, voter1, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Create proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Emergency Control Test',
          'Testing emergency control gas costs',
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

      // Test emergency pause gas cost
      const tx = await votingSystem
        .connect(admin)
        .emergencyPauseProposal(1, 'Testing emergency pause gas cost');
      const receipt = await tx.wait();

      console.log(`Emergency pause gas cost: ${receipt.gasUsed}`);
      expect(receipt.gasUsed).to.be.lessThan(
        ethers.parseUnits('100000', 'wei')
      );
    });
  });

  describe('Storage Optimization and Memory Usage', function () {
    it('Should optimize storage layout for proposals', async function () {
      const { votingSystem, voter1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');

      // Create proposals to test storage efficiency
      const storageGasCosts = [];

      for (let i = 0; i < 3; i++) {
        const tx = await votingSystem
          .connect(voter1)
          .createProposal(
            `Storage Test ${i}`,
            `Testing storage layout optimization ${i}`,
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          );

        const receipt = await tx.wait();
        storageGasCosts.push(receipt.gasUsed);
        console.log(`Storage test ${i} gas: ${receipt.gasUsed}`);
      }

      // Storage costs should be predictable and not increase dramatically
      for (let i = 1; i < storageGasCosts.length; i++) {
        const increase = storageGasCosts[i] - storageGasCosts[i - 1];
        console.log(`Storage gas increase for proposal ${i}: ${increase}`);
        // Should not have significant increase due to storage layout optimization
        expect(increase).to.be.lessThan(ethers.parseUnits('50000', 'wei'));
      }
    });

    it('Should minimize storage for validator consensus data', async function () {
      const { votingSystem, voter1, validator1, validator2, validator3 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Create proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Validator Storage Test',
          'Testing validator consensus storage efficiency',
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      // Test storage efficiency of validator approvals
      const validators = [validator1, validator2, validator3];
      const validationGasCosts = [];

      for (let i = 0; i < validators.length; i++) {
        const tx = await votingSystem
          .connect(validators[i])
          .validateProposalTimestamps(1, true, `Validator ${i}`);
        const receipt = await tx.wait();
        validationGasCosts.push(receipt.gasUsed);
        console.log(`Validator ${i} approval gas: ${receipt.gasUsed}`);
      }

      // Validator storage should be efficient
      const avgValidationGas =
        validationGasCosts.reduce((sum, gas) => sum + gas, BigInt(0)) /
        BigInt(validationGasCosts.length);
      console.log(`Average validation gas: ${avgValidationGas}`);

      expect(avgValidationGas).to.be.lessThan(
        ethers.parseUnits('150000', 'wei')
      );
    });

    it('Should optimize vote receipt storage', async function () {
      const { votingSystem, voter1, voter2, voter3, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Setup proposal
      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Vote Storage Test',
          'Testing vote receipt storage optimization',
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

      // Test vote storage efficiency
      const voters = [voter1, voter2, voter3];
      const voteGasCosts = [];

      for (let i = 0; i < voters.length; i++) {
        const signature = ethers.hexlify(ethers.randomBytes(65));
        const tx = await votingSystem
          .connect(voters[i])
          .castVote(1, 1, `Vote ${i}`, signature);
        const receipt = await tx.wait();
        voteGasCosts.push(receipt.gasUsed);
        console.log(`Vote ${i} storage gas: ${receipt.gasUsed}`);
      }

      // Vote storage should be consistent and efficient
      const maxGas = voteGasCosts.reduce(
        (max, gas) => (gas > max ? gas : max),
        BigInt(0)
      );
      const minGas = voteGasCosts.reduce(
        (min, gas) => (gas < min ? gas : min),
        voteGasCosts[0]
      );
      const gasRange = maxGas - minGas;

      console.log(`Vote storage gas range: ${gasRange}`);
      expect(gasRange).to.be.lessThan(ethers.parseUnits('50000', 'wei')); // Should be consistent
    });
  });

  describe('High Load Performance Testing', function () {
    it('Should maintain performance under high proposal load', async function () {
      const { votingSystem, voter1, voter2, voter3 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const proposalCount = 10;
      const createGasCosts = [];

      console.log(
        `Creating ${proposalCount} proposals for high load testing...`
      );

      for (let i = 0; i < proposalCount; i++) {
        const voter = [voter1, voter2, voter3][i % 3]; // Rotate voters

        const tx = await votingSystem
          .connect(voter)
          .createProposal(
            `High Load Test ${i}`,
            `Performance test proposal ${i}`,
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          );

        const receipt = await tx.wait();
        createGasCosts.push(receipt.gasUsed);

        if (i % 5 === 0) {
          console.log(`Proposal ${i} gas: ${receipt.gasUsed}`);
        }
      }

      // Analyze performance degradation
      const firstFiveAvg =
        createGasCosts.slice(0, 5).reduce((sum, gas) => sum + gas, BigInt(0)) /
        BigInt(5);
      const lastFiveAvg =
        createGasCosts.slice(-5).reduce((sum, gas) => sum + gas, BigInt(0)) /
        BigInt(5);

      console.log(`First 5 proposals average gas: ${firstFiveAvg}`);
      console.log(`Last 5 proposals average gas: ${lastFiveAvg}`);

      const performanceDegradation = lastFiveAvg - firstFiveAvg;
      console.log(`Performance degradation: ${performanceDegradation}`);

      // Performance should not degrade significantly under load
      expect(performanceDegradation).to.be.lessThan(firstFiveAvg / BigInt(2)); // Less than 50% increase
    });

    it('Should handle concurrent validator operations efficiently', async function () {
      const { votingSystem, voter1, validator1, validator2, validator3 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Create multiple proposals
      const proposerBond = ethers.parseEther('10');
      const proposalCount = 5;

      for (let i = 0; i < proposalCount; i++) {
        await votingSystem
          .connect(voter1)
          .createProposal(
            `Concurrent Test ${i}`,
            `Concurrent validation test ${i}`,
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          );
      }

      // Test concurrent validator operations
      const validators = [validator1, validator2, validator3];
      const concurrentGasCosts = [];

      for (let proposalId = 1; proposalId <= proposalCount; proposalId++) {
        const validationPromises = validators.map(async (validator, index) => {
          const tx = await votingSystem
            .connect(validator)
            .validateProposalTimestamps(
              proposalId,
              true,
              `Concurrent validation ${proposalId}-${index}`
            );
          const receipt = await tx.wait();
          return receipt.gasUsed;
        });

        const gasResults = await Promise.all(validationPromises);
        concurrentGasCosts.push(...gasResults);

        console.log(
          `Proposal ${proposalId} concurrent validation gas: ${gasResults.map(g => g.toString()).join(', ')}`
        );
      }

      // Concurrent operations should maintain efficiency
      const avgConcurrentGas =
        concurrentGasCosts.reduce((sum, gas) => sum + gas, BigInt(0)) /
        BigInt(concurrentGasCosts.length);
      console.log(`Average concurrent validation gas: ${avgConcurrentGas}`);

      expect(avgConcurrentGas).to.be.lessThan(
        ethers.parseUnits('200000', 'wei')
      );
    });

    it('Should optimize batch voting operations', async function () {
      const { votingSystem, voter1, voter2, voter3, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Create and validate multiple proposals
      const proposerBond = ethers.parseEther('10');
      const batchSize = 5;

      for (let i = 0; i < batchSize; i++) {
        await votingSystem
          .connect(voter1)
          .createProposal(
            `Batch Vote Test ${i}`,
            `Batch voting optimization test ${i}`,
            '0x',
            3600,
            24 * 3600,
            { value: proposerBond }
          );
      }

      // Validate all proposals
      for (let i = 1; i <= batchSize; i++) {
        await votingSystem
          .connect(validator1)
          .validateProposalTimestamps(i, true, '');
        await votingSystem
          .connect(validator2)
          .validateProposalTimestamps(i, true, '');
      }

      await time.increase(3600);

      // Test batch voting performance
      const voters = [voter1, voter2, voter3];
      const batchVotingGas = [];

      for (let voterIndex = 0; voterIndex < voters.length; voterIndex++) {
        const voterGasCosts = [];

        for (let proposalId = 1; proposalId <= batchSize; proposalId++) {
          const signature = ethers.hexlify(ethers.randomBytes(65));
          const tx = await votingSystem
            .connect(voters[voterIndex])
            .castVote(
              proposalId,
              1,
              `Batch vote ${voterIndex}-${proposalId}`,
              signature
            );
          const receipt = await tx.wait();
          voterGasCosts.push(receipt.gasUsed);
        }

        batchVotingGas.push(voterGasCosts);
        console.log(
          `Voter ${voterIndex} batch voting gas: ${voterGasCosts.map(g => g.toString()).join(', ')}`
        );
      }

      // Batch voting should maintain consistent performance
      for (const voterGasCosts of batchVotingGas) {
        const avgGas =
          voterGasCosts.reduce((sum, gas) => sum + gas, BigInt(0)) /
          BigInt(voterGasCosts.length);
        const maxDeviation = voterGasCosts.reduce((max, gas) => {
          const deviation = gas > avgGas ? gas - avgGas : avgGas - gas;
          return deviation > max ? deviation : max;
        }, BigInt(0));

        console.log(`Max deviation from average: ${maxDeviation}`);
        expect(maxDeviation).to.be.lessThan(avgGas / BigInt(5)); // Less than 20% deviation
      }
    });
  });

  describe('Gas Limit Compliance Testing', function () {
    it('Should stay within block gas limits for all operations', async function () {
      const { votingSystem, voter1, validator1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      const blockGasLimit = BigInt(30000000); // Typical mainnet block gas limit
      const proposerBond = ethers.parseEther('10');

      // Test proposal creation within gas limits
      const tx1 = await votingSystem.connect(voter1).createProposal(
        'Gas Limit Compliance Test',
        'Ensuring all operations stay within block gas limits',
        ethers.hexlify(ethers.randomBytes(1000)), // Large data payload
        3600,
        24 * 3600,
        { value: proposerBond }
      );

      const receipt1 = await tx1.wait();
      console.log(
        `Large proposal creation gas: ${receipt1.gasUsed} (${((receipt1.gasUsed * BigInt(100)) / blockGasLimit).toString()}% of block limit)`
      );
      expect(receipt1.gasUsed).to.be.lessThan(blockGasLimit / BigInt(10)); // Less than 10% of block limit

      // Test validator operations within gas limits
      const tx2 = await votingSystem
        .connect(validator1)
        .validateProposalTimestamps(1, true, 'Gas limit compliance test');
      const receipt2 = await tx2.wait();
      console.log(
        `Validator operation gas: ${receipt2.gasUsed} (${((receipt2.gasUsed * BigInt(100)) / blockGasLimit).toString()}% of block limit)`
      );
      expect(receipt2.gasUsed).to.be.lessThan(blockGasLimit / BigInt(20)); // Less than 5% of block limit
    });

    it('Should optimize for standard wallet gas limits', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      const walletGasLimit = BigInt(300000); // Standard wallet transaction gas limit
      const proposerBond = ethers.parseEther('10');

      // Create and validate proposal
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Wallet Gas Limit Test',
          'Testing compatibility with standard wallet gas limits',
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

      // Test voting within wallet gas limits
      const signature = ethers.hexlify(ethers.randomBytes(65));
      const tx = await votingSystem
        .connect(voter1)
        .castVote(1, 1, 'Wallet gas test', signature);
      const receipt = await tx.wait();

      console.log(`Voting gas for wallet compatibility: ${receipt.gasUsed}`);
      expect(receipt.gasUsed).to.be.lessThan(walletGasLimit);
    });
  });

  describe('Gas Optimization Regression Testing', function () {
    it('Should maintain gas efficiency across contract upgrades', async function () {
      // This test would compare gas costs with previous versions
      // For now, we'll test that current implementation meets efficiency targets

      const { votingSystem, voter1 } = await loadFixture(
        deployOptimizedVotingSystemFixture
      );

      const proposerBond = ethers.parseEther('10');
      const tx = await votingSystem
        .connect(voter1)
        .createProposal(
          'Regression Test',
          "Ensuring gas efficiency doesn't regress",
          '0x',
          3600,
          24 * 3600,
          { value: proposerBond }
        );

      const receipt = await tx.wait();

      // Define efficiency targets based on security features provided
      const gasTargets = {
        proposalCreation: BigInt(800000), // Including security features
        validation: BigInt(150000), // Validator consensus
        voting: BigInt(300000), // EIP-712 + security checks
        emergencyPause: BigInt(100000), // Emergency controls
      };

      console.log(`Current proposal creation gas: ${receipt.gasUsed}`);
      console.log(`Target: ${gasTargets.proposalCreation}`);

      expect(receipt.gasUsed).to.be.lessThan(gasTargets.proposalCreation);
    });

    it('Should benchmark against industry standards', async function () {
      const { votingSystem, voter1, validator1, validator2 } =
        await loadFixture(deployOptimizedVotingSystemFixture);

      // Industry benchmark: OpenZeppelin Governor typically uses ~200-400K gas for voting
      // Our system should be competitive while providing additional security features

      const proposerBond = ethers.parseEther('10');
      await votingSystem
        .connect(voter1)
        .createProposal(
          'Industry Benchmark',
          'Comparing against industry standard governance systems',
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

      const signature = ethers.hexlify(ethers.randomBytes(65));
      const tx = await votingSystem
        .connect(voter1)
        .castVote(1, 1, 'Industry benchmark', signature);
      const receipt = await tx.wait();

      console.log(`Our system voting gas: ${receipt.gasUsed}`);
      console.log(`Industry standard range: 200,000 - 400,000 gas`);

      // We expect slightly higher gas due to additional security features,
      // but should remain competitive
      expect(receipt.gasUsed).to.be.lessThan(BigInt(500000)); // 25% overhead acceptable for security features
    });
  });
});

// Utility functions for gas analysis
function calculateGasEfficiency(gasUsed, baselineGas) {
  const efficiency = (BigInt(100) * (baselineGas - gasUsed)) / baselineGas;
  return efficiency;
}

function analyzeGasPattern(gasArray) {
  const avgGas =
    gasArray.reduce((sum, gas) => sum + gas, BigInt(0)) /
    BigInt(gasArray.length);
  const maxGas = gasArray.reduce(
    (max, gas) => (gas > max ? gas : max),
    BigInt(0)
  );
  const minGas = gasArray.reduce(
    (min, gas) => (gas < min ? gas : min),
    gasArray[0]
  );

  return {
    average: avgGas,
    maximum: maxGas,
    minimum: minGas,
    variance: maxGas - minGas,
    consistency: ((avgGas - (maxGas - minGas)) * BigInt(100)) / avgGas,
  };
}
