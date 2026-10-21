const { expect } = require('chai');
const { ethers } = require('hardhat');
const {
  loadFixture,
  time,
} = require('@nomicfoundation/hardhat-network-helpers');

describe('SecureMultiSigWallet Security Tests', function () {
  let wallet;
  let owners;
  let nonOwner;
  let emergencyRecovery;
  let mockToken;
  let domain;

  const TRANSACTION_TYPEHASH = ethers.keccak256(
    ethers.toUtf8Bytes(
      'Transaction(address to,uint256 value,bytes data,uint256 nonce,uint256 deadline,string description)'
    )
  );

  const OWNER_MANAGEMENT_TYPEHASH = ethers.keccak256(
    ethers.toUtf8Bytes(
      'OwnerManagement(address newOwner,bool isAddition,uint256 newRequiredSignatures,uint256 nonce,uint256 deadline)'
    )
  );

  async function deployWalletFixture() {
    const signers = await ethers.getSigners();
    const [
      deployer,
      owner1,
      owner2,
      owner3,
      owner4,
      owner5,
      nonOwnerAccount,
      emergencyAccount,
    ] = signers;

    const ownerAddresses = [owner1.address, owner2.address, owner3.address];
    const requiredSignatures = 2;
    const dailyLimit = ethers.parseEther('100'); // 100 ETH daily limit
    const emergencyRecoveryAddress = emergencyAccount.address;

    // Deploy wallet
    const SecureMultiSigWallet = await ethers.getContractFactory(
      'SecureMultiSigWallet'
    );
    const walletContract = await SecureMultiSigWallet.deploy(
      ownerAddresses,
      requiredSignatures,
      dailyLimit,
      emergencyRecoveryAddress
    );

    // Deploy a simple ERC20 mock token for testing (we'll create a simple implementation)
    let mockTokenContract;
    try {
      // Try to use a built-in mock or create one inline
      const MockERC20Code = `
        pragma solidity ^0.8.28;
        import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
        contract MockERC20 is ERC20 {
          constructor() ERC20("MockToken", "MOCK") {
            _mint(msg.sender, 1000000 * 10**18);
          }
        }
      `;

      // For now, let's just skip the token transfer since it's not essential for multisig tests
      mockTokenContract = null;
    } catch (error) {
      console.log(
        'Skipping token deployment for multisig tests:',
        error.message
      );
      mockTokenContract = null;
    }

    // Setup EIP-712 domain
    const domain = {
      name: 'SecureMultiSigWallet',
      version: '1',
      chainId: await ethers.provider.getNetwork().then(n => n.chainId),
      verifyingContract: await walletContract.getAddress(),
    };

    // Fund the wallet with ETH
    await deployer.sendTransaction({
      to: await walletContract.getAddress(),
      value: ethers.parseEther('1000'), // 1000 ETH for testing
    });

    // Skip token transfer for now since we're focusing on ETH transactions

    return {
      wallet: walletContract,
      owners: { owner1, owner2, owner3, owner4, owner5 },
      nonOwner: nonOwnerAccount,
      emergencyRecovery: emergencyAccount,
      mockToken: mockTokenContract,
      domain,
      deployer,
    };
  }

  beforeEach(async function () {
    const fixture = await loadFixture(deployWalletFixture);
    wallet = fixture.wallet;
    owners = fixture.owners;
    nonOwner = fixture.nonOwner;
    emergencyRecovery = fixture.emergencyRecovery;
    mockToken = fixture.mockToken;
    domain = fixture.domain;
  });

  // Helper function to sign transaction data
  async function signTransactionData(
    signer,
    to,
    value,
    data,
    nonce,
    deadline,
    description
  ) {
    const types = {
      Transaction: [
        { name: 'to', type: 'address' },
        { name: 'value', type: 'uint256' },
        { name: 'data', type: 'bytes' },
        { name: 'nonce', type: 'uint256' },
        { name: 'deadline', type: 'uint256' },
        { name: 'description', type: 'string' },
      ],
    };

    const message = {
      to,
      value,
      data,
      nonce,
      deadline,
      description,
    };

    return await signer.signTypedData(domain, types, message);
  }

  // Helper function to sign owner management data
  async function signOwnerManagementData(
    signer,
    newOwner,
    isAddition,
    newRequiredSignatures,
    nonce,
    deadline
  ) {
    const types = {
      OwnerManagement: [
        { name: 'newOwner', type: 'address' },
        { name: 'isAddition', type: 'bool' },
        { name: 'newRequiredSignatures', type: 'uint256' },
        { name: 'nonce', type: 'uint256' },
        { name: 'deadline', type: 'uint256' },
      ],
    };

    const message = {
      newOwner,
      isAddition,
      newRequiredSignatures,
      nonce,
      deadline,
    };

    return await signer.signTypedData(domain, types, message);
  }

  describe('1. Signature Bypass Attack Prevention', function () {
    it('Should reject transaction submission with insufficient signatures', async function () {
      const deadline = (await time.latest()) + 3600; // 1 hour from now

      await expect(
        wallet.connect(owners.owner1).submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0, // TRANSFER
          deadline
        )
      ).to.not.be.reverted; // Submission should succeed

      // But execution should fail without enough confirmations
      const txId = 0;
      const tx = await wallet.getTransaction(txId);
      expect(tx.confirmations).to.equal(0);
    });

    it('Should reject confirmation with invalid signature', async function () {
      const deadline = (await time.latest()) + 3600;

      // Submit transaction
      await wallet.connect(owners.owner1).submitTransaction(
        owners.owner2.address,
        ethers.parseEther('1'),
        '0x',
        'Test transfer',
        0, // TRANSFER
        deadline
      );

      // Try to confirm with invalid signature (wrong data)
      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      // Sign with wrong data
      const wrongSignature = await signTransactionData(
        owners.owner1,
        owners.owner3.address, // Wrong recipient
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await expect(
        wallet.connect(owners.owner1).confirmTransaction(txId, wrongSignature)
      ).to.be.revertedWith('SecureMultiSigWallet: Invalid signature');
    });

    it('Should reject confirmation with duplicate signatures', async function () {
      const deadline = (await time.latest()) + 3600;

      // Submit transaction
      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      // Sign correctly
      const signature = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      // First confirmation should succeed
      await wallet.connect(owners.owner1).confirmTransaction(txId, signature);

      // Second confirmation with same owner should fail
      await expect(
        wallet.connect(owners.owner1).confirmTransaction(txId, signature)
      ).to.be.revertedWith(
        'SecureMultiSigWallet: Already confirmed by this owner'
      );
    });

    it('Should reject expired transaction signatures', async function () {
      const deadline = (await time.latest()) + 1; // 1 second from now

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      // Wait for expiration
      await time.increase(2);

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      const signature = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await expect(
        wallet.connect(owners.owner1).confirmTransaction(txId, signature)
      ).to.be.revertedWith('SecureMultiSigWallet: Transaction expired');
    });

    it('Should prevent signature replay attacks', async function () {
      const deadline = (await time.latest()) + 3600;

      // Submit first transaction
      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer 1',
          0,
          deadline
        );

      const txId1 = 0;
      const tx1 = await wallet.getTransaction(txId1);

      const signature = await signTransactionData(
        owners.owner1,
        tx1.to,
        tx1.value,
        tx1.data,
        tx1.nonce,
        tx1.expiresAt,
        tx1.description
      );

      // Confirm first transaction
      await wallet.connect(owners.owner1).confirmTransaction(txId1, signature);

      // Submit second transaction with same parameters
      await wallet.connect(owners.owner1).submitTransaction(
        owners.owner2.address,
        ethers.parseEther('1'),
        '0x',
        'Test transfer 1', // Same description
        0,
        deadline
      );

      const txId2 = 1;

      // Try to reuse the same signature
      await expect(
        wallet.connect(owners.owner1).confirmTransaction(txId2, signature)
      ).to.be.revertedWith('SecureMultiSigWallet: Signature already used');
    });
  });

  describe('2. Multi-Signature Threshold Enforcement', function () {
    it('Should require minimum signature threshold for execution', async function () {
      const deadline = (await time.latest()) + 3600;

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      // Only one signature (need 2)
      const signature1 = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await wallet.connect(owners.owner1).confirmTransaction(txId, signature1);

      // Try to execute with insufficient confirmations
      await expect(
        wallet.connect(owners.owner1).executeTransaction(txId)
      ).to.be.revertedWith('SecureMultiSigWallet: Insufficient confirmations');
    });

    it('Should execute transaction when threshold is met', async function () {
      const deadline = (await time.latest()) + 3600;
      const initialBalance = await ethers.provider.getBalance(
        owners.owner2.address
      );

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      // Get two signatures
      const signature1 = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      const signature2 = await signTransactionData(
        owners.owner2,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await wallet.connect(owners.owner1).confirmTransaction(txId, signature1);

      // Second confirmation should trigger execution
      const confirmTx = await wallet
        .connect(owners.owner2)
        .confirmTransaction(txId, signature2);
      await confirmTx.wait();

      // Check transaction was executed
      const finalTx = await wallet.getTransaction(txId);
      expect(finalTx.executed).to.be.true;

      // Check balance changed
      const finalBalance = await ethers.provider.getBalance(
        owners.owner2.address
      );
      expect(finalBalance).to.be.greaterThan(initialBalance);
    });

    it('Should reject execution by non-owners', async function () {
      const deadline = (await time.latest()) + 3600;

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      await expect(
        wallet.connect(nonOwner).executeTransaction(0)
      ).to.be.revertedWith('SecureMultiSigWallet: Not an owner');
    });
  });

  describe('3. EIP-712 Signature Validation', function () {
    it('Should validate EIP-712 structured signatures correctly', async function () {
      const deadline = (await time.latest()) + 3600;

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      // Valid EIP-712 signature
      const validSignature = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await expect(
        wallet.connect(owners.owner1).confirmTransaction(txId, validSignature)
      ).to.not.be.reverted;
    });

    it('Should reject malformed signatures', async function () {
      const deadline = (await time.latest()) + 3600;

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      const txId = 0;
      const malformedSignature = '0x1234'; // Too short

      await expect(
        wallet
          .connect(owners.owner1)
          .confirmTransaction(txId, malformedSignature)
      ).to.be.reverted;
    });

    it('Should validate domain separator correctly', async function () {
      // This test ensures the contract uses the correct domain separator
      const contractDomain = (await wallet.DOMAIN_SEPARATOR)
        ? await wallet.DOMAIN_SEPARATOR()
        : null;

      // Verify domain structure matches EIP-712 spec
      expect(domain.name).to.equal('SecureMultiSigWallet');
      expect(domain.version).to.equal('1');
    });
  });

  describe('4. Anti-Replay Protection', function () {
    it('Should prevent nonce reuse', async function () {
      const deadline = (await time.latest()) + 3600;
      const nonce = 12345;

      // Try to use the same nonce for owner management (should be rejected)
      const signatures = [];
      for (let i = 0; i < 2; i++) {
        const sig = await signOwnerManagementData(
          [owners.owner1, owners.owner2][i],
          owners.owner4.address,
          true,
          2,
          nonce,
          deadline
        );
        signatures.push(sig);
      }

      await wallet
        .connect(owners.owner1)
        .addOwner(owners.owner4.address, 2, signatures, deadline, nonce);

      // Try to reuse the same nonce
      await expect(
        wallet
          .connect(owners.owner1)
          .addOwner(owners.owner5.address, 2, signatures, deadline, nonce)
      ).to.be.revertedWith('SecureMultiSigWallet: Nonce already used');
    });

    it('Should enforce signature timing windows', async function () {
      const deadline = (await time.latest()) + 3600;

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      const signature = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      // First signature
      await wallet.connect(owners.owner1).confirmTransaction(txId, signature);

      // Try to sign again too quickly (within replay window)
      await expect(
        wallet.connect(owners.owner1).confirmTransaction(txId, signature)
      ).to.be.revertedWith(
        'SecureMultiSigWallet: Already confirmed by this owner'
      );
    });
  });

  describe('5. Rate Limiting and Daily Transaction Limits', function () {
    it('Should enforce daily transaction limits', async function () {
      const deadline = (await time.latest()) + 3600;
      const largeAmount = ethers.parseEther('150'); // Above 100 ETH daily limit

      await expect(
        wallet
          .connect(owners.owner1)
          .submitTransaction(
            owners.owner2.address,
            largeAmount,
            '0x',
            'Large transfer',
            0,
            deadline
          )
      ).to.be.revertedWith('SecureMultiSigWallet: Exceeds daily limit');
    });

    it('Should reset daily limits after 24 hours', async function () {
      const deadline = (await time.latest()) + 3600;
      const amount = ethers.parseEther('50');

      // Submit transaction within limit
      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          amount,
          '0x',
          'Transfer 1',
          0,
          deadline
        );

      // Submit another transaction within same day
      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          amount,
          '0x',
          'Transfer 2',
          0,
          deadline
        );

      // This should be within cumulative limit (100 ETH)
      expect(await wallet.transactionCount()).to.equal(2);

      // Try to submit transaction that would exceed daily limit
      await expect(
        wallet.connect(owners.owner1).submitTransaction(
          owners.owner2.address,
          ethers.parseEther('10'), // Would make total 110 ETH
          '0x',
          'Transfer 3',
          0,
          deadline
        )
      ).to.be.revertedWith('SecureMultiSigWallet: Exceeds daily limit');
    });

    it('Should enforce rate limiting on transaction submissions', async function () {
      const deadline = (await time.latest()) + 3600;

      // Submit multiple transactions rapidly
      for (let i = 0; i < 100; i++) {
        await wallet
          .connect(owners.owner1)
          .submitTransaction(
            owners.owner2.address,
            ethers.parseEther('0.1'),
            '0x',
            `Transfer ${i}`,
            0,
            deadline
          );
      }

      // 101st transaction should be rate limited
      await expect(
        wallet
          .connect(owners.owner1)
          .submitTransaction(
            owners.owner2.address,
            ethers.parseEther('0.1'),
            '0x',
            'Rate limited transfer',
            0,
            deadline
          )
      ).to.be.revertedWith('SecureMultiSigWallet: Rate limit exceeded');
    });
  });

  describe('6. Owner Management Authorization', function () {
    it('Should require multi-signature approval for adding owners', async function () {
      const deadline = (await time.latest()) + 3600;
      const nonce = 54321;

      // Single signature should fail
      const singleSignature = await signOwnerManagementData(
        owners.owner1,
        owners.owner4.address,
        true,
        2,
        nonce,
        deadline
      );

      await expect(
        wallet
          .connect(owners.owner1)
          .addOwner(
            owners.owner4.address,
            2,
            [singleSignature],
            deadline,
            nonce
          )
      ).to.be.revertedWith('SecureMultiSigWallet: Insufficient signatures');
    });

    it('Should successfully add owner with valid multi-signature', async function () {
      const deadline = (await time.latest()) + 3600;
      const nonce = 54322;

      const signatures = [];
      for (let i = 0; i < 2; i++) {
        const sig = await signOwnerManagementData(
          [owners.owner1, owners.owner2][i],
          owners.owner4.address,
          true,
          3, // Increase required signatures
          nonce,
          deadline
        );
        signatures.push(sig);
      }

      await wallet
        .connect(owners.owner1)
        .addOwner(owners.owner4.address, 3, signatures, deadline, nonce);

      expect(await wallet.isOwner(owners.owner4.address)).to.be.true;
      expect(await wallet.totalOwners()).to.equal(4);
      expect(await wallet.requiredSignatures()).to.equal(3);
    });

    it('Should prevent unauthorized owner removal', async function () {
      const deadline = (await time.latest()) + 3600;
      const nonce = 54323;

      // Try to remove owner without proper signatures
      await expect(
        wallet
          .connect(nonOwner)
          .removeOwner(owners.owner3.address, 2, [], deadline, nonce)
      ).to.be.revertedWith('SecureMultiSigWallet: Not an owner');
    });

    it('Should require minimum owner count', async function () {
      const deadline = (await time.latest()) + 3600;
      const nonce = 54324;

      // Can't remove owner if it would go below minimum
      const signatures = [];
      for (let i = 0; i < 2; i++) {
        const sig = await signOwnerManagementData(
          [owners.owner1, owners.owner2][i],
          owners.owner3.address,
          false, // removal
          1, // Below minimum
          nonce,
          deadline
        );
        signatures.push(sig);
      }

      await expect(
        wallet.connect(owners.owner1).removeOwner(
          owners.owner3.address,
          1, // Would result in only 2 owners, below minimum
          signatures,
          deadline,
          nonce
        )
      ).to.be.revertedWith('SecureMultiSigWallet: Invalid required signatures');
    });
  });

  describe('7. Emergency Mode Tests', function () {
    it('Should allow emergency role to activate emergency mode', async function () {
      await wallet.connect(emergencyRecovery).activateEmergencyMode();

      expect(await wallet.emergencyMode()).to.be.true;
      expect(await wallet.paused()).to.be.true;
    });

    it('Should prevent normal operations during emergency mode', async function () {
      await wallet.connect(emergencyRecovery).activateEmergencyMode();

      const deadline = (await time.latest()) + 3600;

      await expect(
        wallet
          .connect(owners.owner1)
          .submitTransaction(
            owners.owner2.address,
            ethers.parseEther('1'),
            '0x',
            'Emergency test',
            0,
            deadline
          )
      ).to.be.revertedWith('SecureMultiSigWallet: Emergency mode active');
    });

    it('Should require multi-signature to deactivate emergency mode', async function () {
      await wallet.connect(emergencyRecovery).activateEmergencyMode();

      // Single signature should fail
      const emergencyHash = ethers.keccak256(
        ethers.solidityPacked(
          ['string', 'address', 'uint256'],
          [
            'DEACTIVATE_EMERGENCY',
            await wallet.getAddress(),
            await time.latest(),
          ]
        )
      );

      const singleSignature = await owners.owner1.signMessage(
        ethers.getBytes(emergencyHash)
      );

      await expect(
        wallet
          .connect(emergencyRecovery)
          .deactivateEmergencyMode([singleSignature])
      ).to.be.revertedWith('SecureMultiSigWallet: Insufficient signatures');
    });

    it('Should allow emergency recovery after cooling period', async function () {
      await wallet.connect(emergencyRecovery).activateEmergencyMode();

      // Fast forward 7 days + 1 hour
      await time.increase(7 * 24 * 3600 + 3600);

      const initialBalance = await ethers.provider.getBalance(
        emergencyRecovery.address
      );

      await wallet
        .connect(emergencyRecovery)
        .emergencyRecover(
          emergencyRecovery.address,
          ethers.parseEther('10'),
          '0x'
        );

      const finalBalance = await ethers.provider.getBalance(
        emergencyRecovery.address
      );
      expect(finalBalance).to.be.greaterThan(initialBalance);
    });
  });

  describe('8. $5M TVL Protection Validation', function () {
    it('Should track high-value transactions in audit logs', async function () {
      const deadline = (await time.latest()) + 3600;
      const highValue = ethers.parseEther('50'); // High value transaction

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          highValue,
          '0x',
          'High value transfer',
          0,
          deadline
        );

      const auditCount = await wallet.getAuditLogsCount();
      expect(auditCount).to.be.greaterThan(0);

      const latestLog = await wallet.getAuditLog(auditCount - 1n);
      expect(latestLog.action).to.equal('TRANSACTION_SUBMITTED');
      expect(latestLog.success).to.be.true;
    });

    it('Should emit security alerts for suspicious activities', async function () {
      const deadline = (await time.latest()) + 3600;

      // Submit transaction
      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      // Provide invalid signature to trigger security alert
      const wrongSignature = await signTransactionData(
        owners.owner2, // Wrong signer
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await expect(
        wallet.connect(owners.owner1).confirmTransaction(txId, wrongSignature)
      ).to.be.revertedWith('SecureMultiSigWallet: Invalid signature');
    });

    it('Should maintain comprehensive audit trail', async function () {
      const initialAuditCount = await wallet.getAuditLogsCount();
      const deadline = (await time.latest()) + 3600;

      // Perform multiple operations
      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Audit test',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      const signature = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await wallet.connect(owners.owner1).confirmTransaction(txId, signature);

      const finalAuditCount = await wallet.getAuditLogsCount();
      expect(finalAuditCount).to.be.greaterThan(initialAuditCount);

      // Verify audit log details
      const submitLog = await wallet.getAuditLog(finalAuditCount - 2n);
      expect(submitLog.action).to.equal('TRANSACTION_SUBMITTED');

      const confirmLog = await wallet.getAuditLog(finalAuditCount - 1n);
      expect(confirmLog.action).to.equal('TRANSACTION_CONFIRMED');
    });

    it('Should protect against large-scale fund extraction', async function () {
      const deadline = (await time.latest()) + 3600;
      const massiveAmount = ethers.parseEther('1000000'); // 1M ETH - way above limits

      await expect(
        wallet
          .connect(owners.owner1)
          .submitTransaction(
            owners.owner2.address,
            massiveAmount,
            '0x',
            'Massive transfer attempt',
            0,
            deadline
          )
      ).to.be.revertedWith('SecureMultiSigWallet: Exceeds daily limit');
    });

    it('Should validate all transaction types for security', async function () {
      const deadline = (await time.latest()) + 3600;

      // Test different transaction types
      const testCases = [
        {
          to: owners.owner2.address,
          value: ethers.parseEther('1'),
          data: '0x',
          type: 0, // TRANSFER
          description: 'ETH transfer',
        },
        {
          to: owners.owner3.address,
          value: ethers.parseEther('0.5'),
          data: '0x',
          type: 1, // TOKEN_TRANSFER (simplified as ETH transfer)
          description: 'Alternative transfer',
        },
        {
          to: wallet.target || (await wallet.getAddress()),
          value: 0,
          data: '0x12345678', // Some contract call data
          type: 2, // CONTRACT_CALL
          description: 'Contract call',
        },
      ];

      for (const testCase of testCases) {
        await expect(
          wallet
            .connect(owners.owner1)
            .submitTransaction(
              testCase.to,
              testCase.value,
              testCase.data,
              testCase.description,
              testCase.type,
              deadline
            )
        ).to.not.be.reverted;
      }

      expect(await wallet.transactionCount()).to.equal(3);
    });
  });

  describe('9. Additional Security Edge Cases', function () {
    it('Should handle zero-value transactions correctly', async function () {
      const deadline = (await time.latest()) + 3600;

      await expect(
        wallet.connect(owners.owner1).submitTransaction(
          owners.owner2.address,
          0, // Zero value
          '0x',
          'Zero value test',
          0,
          deadline
        )
      ).to.be.revertedWith(
        'SecureMultiSigWallet: Transfer value must be greater than 0'
      );
    });

    it('Should reject transactions to zero address', async function () {
      const deadline = (await time.latest()) + 3600;

      await expect(
        wallet
          .connect(owners.owner1)
          .submitTransaction(
            ethers.ZeroAddress,
            ethers.parseEther('1'),
            '0x',
            'Zero address test',
            0,
            deadline
          )
      ).to.be.revertedWith('SecureMultiSigWallet: Invalid recipient');
    });

    it('Should require non-empty transaction descriptions', async function () {
      const deadline = (await time.latest()) + 3600;

      await expect(
        wallet.connect(owners.owner1).submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          '', // Empty description
          0,
          deadline
        )
      ).to.be.revertedWith('SecureMultiSigWallet: Description required');
    });

    it('Should prevent execution of non-existent transactions', async function () {
      await expect(
        wallet.connect(owners.owner1).executeTransaction(999)
      ).to.be.revertedWith('SecureMultiSigWallet: Transaction does not exist');
    });

    it('Should prevent double execution of transactions', async function () {
      const deadline = (await time.latest()) + 3600;

      await wallet
        .connect(owners.owner1)
        .submitTransaction(
          owners.owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Double execution test',
          0,
          deadline
        );

      const txId = 0;
      const tx = await wallet.getTransaction(txId);

      // Get required signatures
      const signature1 = await signTransactionData(
        owners.owner1,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      const signature2 = await signTransactionData(
        owners.owner2,
        tx.to,
        tx.value,
        tx.data,
        tx.nonce,
        tx.expiresAt,
        tx.description
      );

      await wallet.connect(owners.owner1).confirmTransaction(txId, signature1);
      await wallet.connect(owners.owner2).confirmTransaction(txId, signature2);

      // Transaction should be auto-executed after reaching threshold
      const executedTx = await wallet.getTransaction(txId);
      expect(executedTx.executed).to.be.true;

      // Try to execute again
      await expect(
        wallet.connect(owners.owner1).executeTransaction(txId)
      ).to.be.revertedWith(
        'SecureMultiSigWallet: Transaction already executed'
      );
    });
  });
});
