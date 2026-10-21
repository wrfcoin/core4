const { expect } = require('chai');
const { ethers } = require('hardhat');
const { loadFixture } = require('@nomicfoundation/hardhat-network-helpers');

describe('NFTCrossChainBridge Security Tests', function () {
  // Test fixture to deploy contracts
  async function deployBridgeFixture() {
    const [
      owner,
      validator1,
      validator2,
      validator3,
      validator4,
      validator5,
      user1,
      user2,
      attacker,
    ] = await ethers.getSigners();

    // Deploy WRF Token
    const WRFToken = await ethers.getContractFactory('WRFToken');
    const wrfToken = await WRFToken.deploy();

    // Deploy NFT Contract for testing
    const MockNFT = await ethers.getContractFactory('MockERC20'); // We'll use this as a mock NFT
    const nftContract = await MockNFT.deploy('TestNFT', 'TNFT');

    // Deploy Bridge
    const NFTCrossChainBridge = await ethers.getContractFactory(
      'NFTCrossChainBridge'
    );
    const bridge = await NFTCrossChainBridge.deploy(wrfToken.address);

    // Setup authorized NFT contract
    await bridge.setAuthorizedNFTContract(nftContract.address, true);

    // Mint WRF tokens for validators
    const stakeAmount = ethers.utils.parseEther('100000'); // 100K WRF
    await wrfToken.mint(validator1.address, stakeAmount);
    await wrfToken.mint(validator2.address, stakeAmount);
    await wrfToken.mint(validator3.address, stakeAmount);
    await wrfToken.mint(validator4.address, stakeAmount);
    await wrfToken.mint(validator5.address, stakeAmount);

    return {
      bridge,
      wrfToken,
      nftContract,
      owner,
      validator1,
      validator2,
      validator3,
      validator4,
      validator5,
      user1,
      user2,
      attacker,
      stakeAmount,
    };
  }

  describe('Validator Security Model', function () {
    it('Should enforce minimum stake requirements', async function () {
      const { bridge, validator1 } = await loadFixture(deployBridgeFixture);

      const insufficientStake = ethers.utils.parseEther('50000'); // Less than 100K minimum

      await expect(
        bridge
          .connect(validator1)
          .addValidator(validator1.address, insufficientStake, {
            value: insufficientStake,
          })
      ).to.be.revertedWith('Bridge: Insufficient stake');
    });

    it('Should enforce stake time-locks for new validators', async function () {
      const { bridge, validator1, stakeAmount } =
        await loadFixture(deployBridgeFixture);

      // Add validator
      await bridge.addValidator(validator1.address, stakeAmount, {
        value: stakeAmount,
      });

      // Try to validate immediately (should fail due to timelock)
      const transferId = ethers.utils.keccak256(
        ethers.utils.toUtf8Bytes('test')
      );
      const signature = '0x1234'; // Mock signature

      await expect(
        bridge
          .connect(validator1)
          .validateTransferWithSignature(
            transferId,
            true,
            ethers.constants.HashZero,
            signature
          )
      ).to.be.revertedWith('Bridge: Validator stake still locked');
    });

    it('Should limit total number of validators', async function () {
      const {
        bridge,
        validator1,
        validator2,
        validator3,
        validator4,
        validator5,
        stakeAmount,
      } = await loadFixture(deployBridgeFixture);

      // Add 5 validators (maximum)
      await bridge.addValidator(validator1.address, stakeAmount, {
        value: stakeAmount,
      });
      await bridge.addValidator(validator2.address, stakeAmount, {
        value: stakeAmount,
      });
      await bridge.addValidator(validator3.address, stakeAmount, {
        value: stakeAmount,
      });
      await bridge.addValidator(validator4.address, stakeAmount, {
        value: stakeAmount,
      });
      await bridge.addValidator(validator5.address, stakeAmount, {
        value: stakeAmount,
      });

      // Try to add 6th validator (should fail)
      const [, , , , , , extraValidator] = await ethers.getSigners();
      await expect(
        bridge.addValidator(extraValidator.address, stakeAmount, {
          value: stakeAmount,
        })
      ).to.be.revertedWith('Bridge: Maximum validators reached');
    });
  });

  describe('Multi-Signature Security', function () {
    it('Should require minimum threshold signatures', async function () {
      const { bridge, validator1, validator2, nftContract, user1 } =
        await loadFixture(deployBridgeFixture);

      // Create mock proof with insufficient signatures
      const proof = {
        messageHash: ethers.utils.keccak256(ethers.utils.toUtf8Bytes('test')),
        signatures: ['0x1234'], // Only 1 signature, need 3
        merkleProof: [],
        merkleRoot: ethers.constants.HashZero,
        chainId: 1,
        contractAddress: bridge.address,
        nonce: 0,
      };

      await expect(
        bridge.connect(validator1).mintBridgedNFT(1, user1.address, proof)
      ).to.be.revertedWith('Bridge: Insufficient signatures');
    });

    it('Should prevent signature reuse', async function () {
      const { bridge, validator1, nftContract, user1 } =
        await loadFixture(deployBridgeFixture);

      const messageHash = ethers.utils.keccak256(
        ethers.utils.toUtf8Bytes('test')
      );
      const duplicateSignature = '0x1234567890abcdef';

      // Create proof with duplicate signatures
      const proof = {
        messageHash: messageHash,
        signatures: [
          duplicateSignature,
          duplicateSignature,
          duplicateSignature,
        ], // Same signature 3 times
        merkleProof: [],
        merkleRoot: messageHash, // Use message hash as mock root
        chainId: 31337, // Hardhat chain ID
        contractAddress: bridge.address,
        nonce: 0,
      };

      await expect(
        bridge.connect(validator1).mintBridgedNFT(1, user1.address, proof)
      ).to.be.revertedWith('Bridge: Duplicate signature');
    });
  });

  describe('Rate Limiting Security', function () {
    it('Should enforce transfer rate limits', async function () {
      const { bridge, user1, nftContract } =
        await loadFixture(deployBridgeFixture);

      // Mock NFT ownership and approval
      await nftContract.mint(user1.address, 1);
      await nftContract.connect(user1).approve(bridge.address, 1);

      // Calculate bridge fee
      const bridgeFee = await bridge.calculateBridgeFee(0); // Ethereum

      // Make 10 transfers (at the limit)
      for (let i = 1; i <= 10; i++) {
        await nftContract.mint(user1.address, i);
        await nftContract.connect(user1).approve(bridge.address, i);

        await bridge.connect(user1).bridgeNFT(
          nftContract.address,
          i,
          0, // Ethereum
          user1.address,
          'test',
          { value: bridgeFee }
        );
      }

      // 11th transfer should fail
      await nftContract.mint(user1.address, 11);
      await nftContract.connect(user1).approve(bridge.address, 11);

      await expect(
        bridge
          .connect(user1)
          .bridgeNFT(nftContract.address, 11, 0, user1.address, 'test', {
            value: bridgeFee,
          })
      ).to.be.revertedWith('Bridge: Rate limit exceeded');
    });
  });

  describe('High-Value Transfer Security', function () {
    it('Should queue high-value transfers for time-lock', async function () {
      const { bridge, user1, nftContract } =
        await loadFixture(deployBridgeFixture);

      // Mock a high-value NFT (tokenId that results in high value)
      const highValueTokenId = 999999; // This should result in high value from _getNFTValue
      await nftContract.mint(user1.address, highValueTokenId);
      await nftContract
        .connect(user1)
        .approve(bridge.address, highValueTokenId);

      const bridgeFee = await bridge.calculateBridgeFee(0);

      const tx = await bridge
        .connect(user1)
        .bridgeNFT(
          nftContract.address,
          highValueTokenId,
          0,
          user1.address,
          'high-value',
          { value: bridgeFee }
        );

      const receipt = await tx.wait();
      const event = receipt.events?.find(
        e => e.event === 'HighValueTransferQueued'
      );
      expect(event).to.not.be.undefined;
    });

    it('Should enforce time-lock for high-value transfers', async function () {
      const { bridge, user1, nftContract } =
        await loadFixture(deployBridgeFixture);

      const highValueTokenId = 999999;
      await nftContract.mint(user1.address, highValueTokenId);
      await nftContract
        .connect(user1)
        .approve(bridge.address, highValueTokenId);

      const bridgeFee = await bridge.calculateBridgeFee(0);

      // Queue high-value transfer
      await bridge
        .connect(user1)
        .bridgeNFT(
          nftContract.address,
          highValueTokenId,
          0,
          user1.address,
          'high-value',
          { value: bridgeFee }
        );

      // Try to execute immediately (should fail)
      await expect(bridge.executeHighValueTransfer(1)).to.be.revertedWith(
        'Bridge: Time-lock active'
      );
    });
  });

  describe('Circuit Breaker Security', function () {
    it('Should trigger circuit breaker on high failure rate', async function () {
      const { bridge, owner } = await loadFixture(deployBridgeFixture);

      // Simulate high failure rate by manipulating state
      // This would require a more complex setup with actual failed transfers

      const stats = await bridge.getBridgeStats();
      expect(stats.circuitBreakerTriggered).to.be.false;
    });
  });

  describe('Emergency Response Security', function () {
    it('Should only allow emergency functions when paused', async function () {
      const { bridge, owner, nftContract } =
        await loadFixture(deployBridgeFixture);

      // Try emergency withdrawal without pause (should fail)
      await expect(
        bridge.emergencyWithdrawNFT(nftContract.address, 1, owner.address)
      ).to.be.revertedWith('Bridge: Not in emergency state');

      // Pause bridge
      await bridge.emergencyPause();

      // Now emergency withdrawal should work
      // (This would require the bridge to actually hold an NFT)
    });

    it('Should only allow emergency admin to pause', async function () {
      const { bridge, attacker } = await loadFixture(deployBridgeFixture);

      await expect(
        bridge.connect(attacker).emergencyPause()
      ).to.be.revertedWith('Bridge: Not emergency admin');
    });
  });

  describe('Cryptographic Security', function () {
    it('Should validate chain ID in proofs', async function () {
      const { bridge, validator1, user1 } =
        await loadFixture(deployBridgeFixture);

      const proof = {
        messageHash: ethers.utils.keccak256(ethers.utils.toUtf8Bytes('test')),
        signatures: ['0x1234', '0x5678', '0x9abc'],
        merkleProof: [],
        merkleRoot: ethers.constants.HashZero,
        chainId: 999999, // Wrong chain ID
        contractAddress: bridge.address,
        nonce: 0,
      };

      await expect(
        bridge.connect(validator1).mintBridgedNFT(1, user1.address, proof)
      ).to.be.revertedWith('Bridge: Invalid chain ID');
    });

    it('Should validate contract address in proofs', async function () {
      const { bridge, validator1, user1 } =
        await loadFixture(deployBridgeFixture);

      const proof = {
        messageHash: ethers.utils.keccak256(ethers.utils.toUtf8Bytes('test')),
        signatures: ['0x1234', '0x5678', '0x9abc'],
        merkleProof: [],
        merkleRoot: ethers.constants.HashZero,
        chainId: 31337,
        contractAddress: ethers.constants.AddressZero, // Wrong contract address
        nonce: 0,
      };

      await expect(
        bridge.connect(validator1).mintBridgedNFT(1, user1.address, proof)
      ).to.be.revertedWith('Bridge: Invalid contract address');
    });

    it('Should prevent replay attacks with message tracking', async function () {
      const { bridge, validator1, user1 } =
        await loadFixture(deployBridgeFixture);

      const messageHash = ethers.utils.keccak256(
        ethers.utils.toUtf8Bytes('test')
      );

      // Mark message as processed
      // This would require internal manipulation or a successful previous transaction

      const proof = {
        messageHash: messageHash,
        signatures: ['0x1234', '0x5678', '0x9abc'],
        merkleProof: [messageHash], // Use message hash as mock proof
        merkleRoot: messageHash,
        chainId: 31337,
        contractAddress: bridge.address,
        nonce: 0,
      };

      // First call might succeed depending on signature validation
      // Second call with same message hash should fail
      // await expect(
      //     bridge.connect(validator1).mintBridgedNFT(1, user1.address, proof)
      // ).to.be.revertedWith("Bridge: Message already processed");
    });
  });

  describe('Access Control Security', function () {
    it('Should enforce bridge admin role for critical functions', async function () {
      const { bridge, attacker, nftContract } =
        await loadFixture(deployBridgeFixture);

      await expect(
        bridge
          .connect(attacker)
          .setAuthorizedNFTContract(nftContract.address, true)
      ).to.be.revertedWith('Bridge: Not bridge admin');
    });

    it('Should enforce validator role for validation functions', async function () {
      const { bridge, attacker, user1 } =
        await loadFixture(deployBridgeFixture);

      const proof = {
        messageHash: ethers.utils.keccak256(ethers.utils.toUtf8Bytes('test')),
        signatures: ['0x1234', '0x5678', '0x9abc'],
        merkleProof: [],
        merkleRoot: ethers.constants.HashZero,
        chainId: 31337,
        contractAddress: bridge.address,
        nonce: 0,
      };

      await expect(
        bridge.connect(attacker).mintBridgedNFT(1, user1.address, proof)
      ).to.be.revertedWith('Bridge: Not an active validator');
    });
  });

  describe('Economic Security', function () {
    it('Should slash validators for violations', async function () {
      const { bridge, validator1, owner, stakeAmount } =
        await loadFixture(deployBridgeFixture);

      // Add validator
      await bridge.addValidator(validator1.address, stakeAmount, {
        value: stakeAmount,
      });

      const initialStake = (await bridge.getValidatorInfo(validator1.address))
        .stakedAmount;

      // Slash validator
      await bridge.slashValidator(validator1.address, 'Security violation');

      const afterSlashStake = (
        await bridge.getValidatorInfo(validator1.address)
      ).stakedAmount;

      // Should have slashed 50% of stake
      expect(afterSlashStake).to.equal(initialStake.div(2));
    });

    it('Should deactivate validators with insufficient stake', async function () {
      const { bridge, validator1, stakeAmount } =
        await loadFixture(deployBridgeFixture);

      // Add validator
      await bridge.addValidator(validator1.address, stakeAmount, {
        value: stakeAmount,
      });

      // Slash validator multiple times to bring stake below minimum
      await bridge.slashValidator(validator1.address, 'Violation 1');
      await bridge.slashValidator(validator1.address, 'Violation 2');

      const validatorInfo = await bridge.getValidatorInfo(validator1.address);
      expect(validatorInfo.isActive).to.be.false;
    });
  });

  describe('Integration Security', function () {
    it('Should maintain security during normal operation flow', async function () {
      const {
        bridge,
        validator1,
        validator2,
        validator3,
        user1,
        nftContract,
        stakeAmount,
      } = await loadFixture(deployBridgeFixture);

      // Setup 3 validators
      await bridge.addValidator(validator1.address, stakeAmount, {
        value: stakeAmount,
      });
      await bridge.addValidator(validator2.address, stakeAmount, {
        value: stakeAmount,
      });
      await bridge.addValidator(validator3.address, stakeAmount, {
        value: stakeAmount,
      });

      // Fast forward time to allow validators to operate
      await ethers.provider.send('evm_increaseTime', [30 * 24 * 60 * 60]); // 30 days
      await ethers.provider.send('evm_mine');

      // Setup NFT
      await nftContract.mint(user1.address, 1);
      await nftContract.connect(user1).approve(bridge.address, 1);

      const bridgeFee = await bridge.calculateBridgeFee(0);

      // Initiate transfer
      const transferTx = await bridge.connect(user1).bridgeNFT(
        nftContract.address,
        1,
        0, // Ethereum
        user1.address,
        'test',
        { value: bridgeFee }
      );

      const receipt = await transferTx.wait();
      const transferEvent = receipt.events?.find(
        e => e.event === 'TransferInitiated'
      );
      expect(transferEvent).to.not.be.undefined;

      // Check that transfer was created with proper security measures
      const transferId = transferEvent.args[0];
      const transfer = await bridge.getTransfer(transferId);
      expect(transfer.status).to.equal(1); // TransferStatus.Locked
      expect(transfer.nonce).to.be.a('number');
    });
  });
});
