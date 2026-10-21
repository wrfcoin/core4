// SPDX-License-Identifier: MIT
import { expect } from 'chai';
import hre from 'hardhat';
const { ethers } = hre;
import { loadFixture, time } from '@nomicfoundation/hardhat-network-helpers';

/**
 * Comprehensive Security Test Suite for SecureMultiSigWallet
 *
 * SECURITY VULNERABILITIES ADDRESSED:
 * - OWASP SC02: Access Control bypass attempts
 * - Signature replay attacks and manipulation
 * - Unauthorized transaction execution
 * - Emergency controls bypass attempts
 * - Economic attacks targeting $5M TVL
 * - EIP-712 signature verification vulnerabilities
 * - Multi-signature threshold manipulation
 * - Nonce and timestamp security
 */
describe('SecureMultiSigWallet - Comprehensive Security Tests', function () {
  let wallet, owners, attacker, user;
  let deployer, owner1, owner2, owner3, emergencyAddress;

  // EIP-712 domain setup for signature testing
  const setupEIP712Domain = async contractAddress => ({
    name: 'SecureMultiSigWallet',
    version: '1',
    chainId: (await ethers.provider.getNetwork()).chainId,
    verifyingContract: contractAddress,
  });

  // Transaction types enum
  const TransactionType = {
    TRANSFER: 0,
    CALL: 1,
    CREATE: 2,
    ADD_OWNER: 3,
    REMOVE_OWNER: 4,
    CHANGE_REQUIREMENT: 5,
    EMERGENCY: 6,
  };

  // Security test fixture
  async function deployWalletFixture() {
    [deployer, owner1, owner2, owner3, emergencyAddress, attacker, user] =
      await ethers.getSigners();
    owners = [owner1, owner2, owner3];

    const SecureMultiSigWallet = await ethers.getContractFactory(
      'SecureMultiSigWallet'
    );

    const wallet = await SecureMultiSigWallet.deploy(
      [owner1.address, owner2.address, owner3.address], // Initial owners
      2, // Required signatures
      emergencyAddress.address // Emergency address
    );

    await wallet.waitForDeployment();

    // Fund the wallet for testing
    await deployer.sendTransaction({
      to: await wallet.getAddress(),
      value: ethers.parseEther('100'), // 100 ETH for testing
    });

    return {
      wallet,
      owners,
      deployer,
      owner1,
      owner2,
      owner3,
      emergencyAddress,
      attacker,
      user,
    };
  }

  describe('🔐 Signature Bypass Protection', function () {
    it('Should prevent signature bypass attacks', async function () {
      const { wallet, owner1, owner2, attacker } =
        await loadFixture(deployWalletFixture);

      // Create a legitimate transaction
      const txData = ethers.AbiCoder.defaultAbiCoder().encode(
        ['address', 'uint256'],
        [attacker.address, ethers.parseEther('1')]
      );

      const deadline = Math.floor(Date.now() / 1000) + 3600; // 1 hour from now
      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          attacker.address,
          ethers.parseEther('1'),
          txData,
          'Malicious transfer attempt',
          TransactionType.TRANSFER,
          deadline
        );

      // Try to confirm with invalid signature
      const wrongSignature = await owner1.signMessage('wrong data');

      await expect(
        wallet.connect(owner1).confirmTransaction(txId, wrongSignature)
      ).to.be.revertedWith('Invalid signature');
    });

    it('Should prevent signature replay attacks', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const contractAddress = await wallet.getAddress();
      const domain = await setupEIP712Domain(contractAddress);

      const txData = ethers.AbiCoder.defaultAbiCoder().encode(
        ['address', 'uint256'],
        [owner2.address, ethers.parseEther('1')]
      );

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const nonce = await wallet.getNonce();

      // Create transaction
      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          owner2.address,
          ethers.parseEther('1'),
          txData,
          'Legitimate transfer',
          TransactionType.TRANSFER,
          deadline
        );

      // Create EIP-712 signature
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
        to: owner2.address,
        value: ethers.parseEther('1'),
        data: txData,
        nonce: nonce,
        deadline: deadline,
        description: 'Legitimate transfer',
      };

      const signature = await owner1.signTypedData(domain, types, message);

      // Confirm transaction
      await wallet.connect(owner1).confirmTransaction(txId, signature);

      // Try to replay the same signature
      await expect(
        wallet.connect(owner1).confirmTransaction(txId, signature)
      ).to.be.revertedWith('Signature already used');
    });

    it('Should prevent signature manipulation', async function () {
      const { wallet, owner1, owner2, attacker } =
        await loadFixture(deployWalletFixture);

      // Create transaction with manipulated signature components
      const txData = '0x';
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          owner2.address,
          ethers.parseEther('1'),
          txData,
          'Test transfer',
          TransactionType.TRANSFER,
          deadline
        );

      // Create malformed signature (wrong length)
      const malformedSignature = '0x1234567890';

      await expect(
        wallet.connect(owner1).confirmTransaction(txId, malformedSignature)
      ).to.be.reverted;
    });
  });

  describe('🚫 Access Control Bypass Prevention', function () {
    it('Should prevent unauthorized transaction submission', async function () {
      const { wallet, attacker } = await loadFixture(deployWalletFixture);

      await expect(
        wallet
          .connect(attacker)
          .submitTransaction(
            attacker.address,
            ethers.parseEther('50'),
            '0x',
            'Unauthorized transfer',
            TransactionType.TRANSFER,
            Math.floor(Date.now() / 1000) + 3600
          )
      ).to.be.revertedWith(/AccessControl:/);
    });

    it('Should prevent unauthorized owner addition', async function () {
      const { wallet, attacker } = await loadFixture(deployWalletFixture);

      await expect(
        wallet.connect(attacker).addOwner(attacker.address)
      ).to.be.revertedWith(/AccessControl:/);
    });

    it('Should prevent unauthorized requirement changes', async function () {
      const { wallet, attacker } = await loadFixture(deployWalletFixture);

      await expect(
        wallet.connect(attacker).changeRequirement(1)
      ).to.be.revertedWith(/AccessControl:/);
    });

    it('Should prevent emergency control abuse', async function () {
      const { wallet, attacker } = await loadFixture(deployWalletFixture);

      await expect(
        wallet.connect(attacker).emergencyPause()
      ).to.be.revertedWith(/AccessControl:/);

      await expect(
        wallet.connect(attacker).emergencyUnpause()
      ).to.be.revertedWith(/AccessControl:/);
    });
  });

  describe('💰 Economic Security ($5M TVL Protection)', function () {
    it('Should prevent large value transfers without sufficient signatures', async function () {
      const { wallet, owner1, owner2, attacker } =
        await loadFixture(deployWalletFixture);

      // Fund wallet with significant amount
      await owner1.sendTransaction({
        to: await wallet.getAddress(),
        value: ethers.parseEther('5000'), // Simulate $5M equivalent
      });

      const largeValue = ethers.parseEther('2500'); // $2.5M equivalent
      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          attacker.address,
          largeValue,
          '0x',
          'Large value transfer',
          TransactionType.TRANSFER,
          deadline
        );

      // Should require multiple signatures for high-value transactions
      const requiredSigs = await wallet.requiredSignatures();
      expect(requiredSigs).to.be.gte(2);

      // Single signature should not be sufficient
      const contractAddress = await wallet.getAddress();
      const domain = await setupEIP712Domain(contractAddress);
      const nonce = await wallet.getNonce();

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
        to: attacker.address,
        value: largeValue,
        data: '0x',
        nonce: nonce,
        deadline: deadline,
        description: 'Large value transfer',
      };

      const signature1 = await owner1.signTypedData(domain, types, message);
      await wallet.connect(owner1).confirmTransaction(txId, signature1);

      // Should not execute with only one signature
      const tx = await wallet.transactions(txId);
      expect(tx.executed).to.be.false;
    });

    it('Should prevent daily limit bypass', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const dailyLimit = await wallet.dailyLimit();
      const excessiveAmount = dailyLimit + ethers.parseEther('1');

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            excessiveAmount,
            '0x',
            'Exceeds daily limit',
            TransactionType.TRANSFER,
            deadline
          )
      ).to.be.revertedWith('Exceeds daily limit');
    });

    it('Should track and enforce cumulative daily spending', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const dailyLimit = await wallet.dailyLimit();
      const halfLimit = dailyLimit / 2n;

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // First transaction - should succeed
      const txId1 = await wallet
        .connect(owner1)
        .submitTransaction(
          owner2.address,
          halfLimit,
          '0x',
          'First transfer',
          TransactionType.TRANSFER,
          deadline
        );

      // Execute first transaction
      const contractAddress = await wallet.getAddress();
      const domain = await setupEIP712Domain(contractAddress);
      const nonce1 = await wallet.getNonce();

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

      const message1 = {
        to: owner2.address,
        value: halfLimit,
        data: '0x',
        nonce: nonce1,
        deadline: deadline,
        description: 'First transfer',
      };

      const signature1_1 = await owner1.signTypedData(domain, types, message1);
      const signature1_2 = await owner2.signTypedData(domain, types, message1);

      await wallet.connect(owner1).confirmTransaction(txId1, signature1_1);
      await wallet.connect(owner2).confirmTransaction(txId1, signature1_2);

      // Second transaction that would exceed daily limit
      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            halfLimit + ethers.parseEther('1'),
            '0x',
            'Would exceed limit',
            TransactionType.TRANSFER,
            deadline
          )
      ).to.be.revertedWith('Exceeds daily limit');
    });
  });

  describe('⏰ Timestamp and Deadline Security', function () {
    it('Should reject expired transactions', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const pastDeadline = Math.floor(Date.now() / 1000) - 3600; // 1 hour ago

      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            ethers.parseEther('1'),
            '0x',
            'Expired transaction',
            TransactionType.TRANSFER,
            pastDeadline
          )
      ).to.be.revertedWith('Transaction deadline has passed');
    });

    it('Should enforce maximum transaction lifetime', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const farFutureDeadline = Math.floor(Date.now() / 1000) + 8 * 24 * 3600; // 8 days

      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            ethers.parseEther('1'),
            '0x',
            'Too far future',
            TransactionType.TRANSFER,
            farFutureDeadline
          )
      ).to.be.revertedWith('Deadline too far in future');
    });

    it('Should prevent timestamp manipulation attacks', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transaction',
          TransactionType.TRANSFER,
          deadline
        );

      // Fast forward time beyond deadline
      await time.increase(3700); // More than 1 hour

      const contractAddress = await wallet.getAddress();
      const domain = await setupEIP712Domain(contractAddress);
      const nonce = await wallet.getNonce();

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
        to: owner2.address,
        value: ethers.parseEther('1'),
        data: '0x',
        nonce: nonce,
        deadline: deadline,
        description: 'Test transaction',
      };

      const signature = await owner1.signTypedData(domain, types, message);

      await expect(
        wallet.connect(owner1).confirmTransaction(txId, signature)
      ).to.be.revertedWith('Transaction has expired');
    });
  });

  describe('🔄 Reentrancy Attack Prevention', function () {
    it('Should prevent reentrancy during transaction execution', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      // Deploy a malicious contract that attempts reentrancy
      const MaliciousReceiver = await ethers.getContractFactory(
        'MaliciousFlashLoanReceiver'
      );
      const maliciousReceiver = await MaliciousReceiver.deploy();
      await maliciousReceiver.waitForDeployment();

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const callData = maliciousReceiver.interface.encodeFunctionData(
        'attemptReentrancy',
        []
      );

      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          await maliciousReceiver.getAddress(),
          ethers.parseEther('1'),
          callData,
          'Reentrancy attack',
          TransactionType.CALL,
          deadline
        );

      const contractAddress = await wallet.getAddress();
      const domain = await setupEIP712Domain(contractAddress);
      const nonce = await wallet.getNonce();

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
        to: await maliciousReceiver.getAddress(),
        value: ethers.parseEther('1'),
        data: callData,
        nonce: nonce,
        deadline: deadline,
        description: 'Reentrancy attack',
      };

      const signature1 = await owner1.signTypedData(domain, types, message);
      const signature2 = await owner2.signTypedData(domain, types, message);

      await wallet.connect(owner1).confirmTransaction(txId, signature1);

      // Transaction should fail due to reentrancy protection
      await expect(
        wallet.connect(owner2).confirmTransaction(txId, signature2)
      ).to.be.revertedWith('ReentrancyGuard: reentrant call');
    });
  });

  describe('⚡ Emergency Controls Security', function () {
    it('Should only allow emergency address to pause', async function () {
      const { wallet, emergencyAddress, attacker } =
        await loadFixture(deployWalletFixture);

      // Emergency address should be able to pause
      await wallet.connect(emergencyAddress).emergencyPause();
      expect(await wallet.paused()).to.be.true;

      // Unpause for next test
      await wallet.connect(emergencyAddress).emergencyUnpause();

      // Non-emergency address should not be able to pause
      await expect(
        wallet.connect(attacker).emergencyPause()
      ).to.be.revertedWith(/AccessControl:/);
    });

    it('Should prevent operations when paused', async function () {
      const { wallet, emergencyAddress, owner1, owner2 } =
        await loadFixture(deployWalletFixture);

      // Pause the contract
      await wallet.connect(emergencyAddress).emergencyPause();

      // All operations should be blocked
      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            ethers.parseEther('1'),
            '0x',
            'Should fail',
            TransactionType.TRANSFER,
            Math.floor(Date.now() / 1000) + 3600
          )
      ).to.be.revertedWith('Pausable: paused');
    });

    it('Should log emergency actions for audit', async function () {
      const { wallet, emergencyAddress } =
        await loadFixture(deployWalletFixture);

      const tx = await wallet.connect(emergencyAddress).emergencyPause();
      const receipt = await tx.wait();

      // Check for emergency pause event
      const pauseEvent = receipt.logs.find(
        log => log.topics[0] === wallet.interface.getEvent('Paused').topicHash
      );
      expect(pauseEvent).to.not.be.undefined;
    });
  });

  describe('📊 Multi-Signature Threshold Security', function () {
    it('Should prevent execution with insufficient signatures', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test transfer',
          TransactionType.TRANSFER,
          deadline
        );

      const contractAddress = await wallet.getAddress();
      const domain = await setupEIP712Domain(contractAddress);
      const nonce = await wallet.getNonce();

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
        to: owner2.address,
        value: ethers.parseEther('1'),
        data: '0x',
        nonce: nonce,
        deadline: deadline,
        description: 'Test transfer',
      };

      const signature = await owner1.signTypedData(domain, types, message);
      await wallet.connect(owner1).confirmTransaction(txId, signature);

      // Transaction should not execute with only one signature
      const tx = await wallet.transactions(txId);
      expect(tx.executed).to.be.false;
      expect(tx.confirmations).to.equal(1);
    });

    it('Should prevent requirement manipulation attacks', async function () {
      const { wallet, owner1, attacker } =
        await loadFixture(deployWalletFixture);

      // Attacker cannot change requirement
      await expect(
        wallet.connect(attacker).changeRequirement(1)
      ).to.be.revertedWith(/AccessControl:/);

      // Should maintain minimum requirement
      await expect(
        wallet.connect(owner1).changeRequirement(1)
      ).to.be.revertedWith('Invalid requirement');
    });
  });

  describe('🔍 Audit Trail and Monitoring', function () {
    it('Should emit events for all security-sensitive operations', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const tx = await wallet
        .connect(owner1)
        .submitTransaction(
          owner2.address,
          ethers.parseEther('1'),
          '0x',
          'Test audit',
          TransactionType.TRANSFER,
          deadline
        );

      const receipt = await tx.wait();

      // Check for transaction submission event
      const submitEvent = receipt.logs.find(
        log =>
          log.topics[0] ===
          wallet.interface.getEvent('TransactionSubmitted').topicHash
      );
      expect(submitEvent).to.not.be.undefined;
    });

    it('Should maintain complete transaction history', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      const txId = await wallet
        .connect(owner1)
        .submitTransaction(
          owner2.address,
          ethers.parseEther('1'),
          '0x',
          'History test',
          TransactionType.TRANSFER,
          deadline
        );

      const transaction = await wallet.transactions(txId);
      expect(transaction.to).to.equal(owner2.address);
      expect(transaction.value).to.equal(ethers.parseEther('1'));
      expect(transaction.description).to.equal('History test');
    });
  });

  describe('💸 Gas Limit and DoS Protection', function () {
    it('Should prevent gas limit DoS attacks', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      // Create transaction with excessive gas requirement
      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const largeData = '0x' + '00'.repeat(100000); // Large data payload

      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            ethers.parseEther('1'),
            largeData,
            'DoS attempt',
            TransactionType.CALL,
            deadline
          )
      ).to.be.revertedWith('Transaction data too large');
    });

    it('Should limit the number of pending transactions', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      // Submit maximum allowed transactions
      const maxPending = await wallet.MAX_PENDING_TRANSACTIONS();

      for (let i = 0; i < maxPending; i++) {
        await wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            ethers.parseEther('0.1'),
            '0x',
            `Transaction ${i}`,
            TransactionType.TRANSFER,
            deadline
          );
      }

      // Next transaction should fail
      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            ethers.parseEther('0.1'),
            '0x',
            'Should fail',
            TransactionType.TRANSFER,
            deadline
          )
      ).to.be.revertedWith('Too many pending transactions');
    });
  });

  describe('🎯 Edge Cases and Error Conditions', function () {
    it('Should handle zero-value transactions securely', async function () {
      const { wallet, owner1, owner2 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            owner2.address,
            0,
            '0x',
            'Zero value',
            TransactionType.TRANSFER,
            deadline
          )
      ).to.be.revertedWith('Invalid transaction value');
    });

    it('Should handle invalid addresses securely', async function () {
      const { wallet, owner1 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;

      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            ethers.ZeroAddress,
            ethers.parseEther('1'),
            '0x',
            'Invalid address',
            TransactionType.TRANSFER,
            deadline
          )
      ).to.be.revertedWith('Invalid recipient address');
    });

    it('Should handle contract destruction attempts', async function () {
      const { wallet, owner1 } = await loadFixture(deployWalletFixture);

      const deadline = Math.floor(Date.now() / 1000) + 3600;
      const destructData =
        wallet.interface.encodeFunctionData('emergencyPause');

      // Self-destruct should not be possible through normal transactions
      await expect(
        wallet
          .connect(owner1)
          .submitTransaction(
            await wallet.getAddress(),
            0,
            destructData,
            'Self destruct attempt',
            TransactionType.CALL,
            deadline
          )
      ).to.be.revertedWith('Self-calls not allowed');
    });
  });
});
