const { expect } = require('chai');
const { ethers } = require('hardhat');
const { time } = require('@nomicfoundation/hardhat-network-helpers');

describe('SecureCrossChainMessageVerifier - Security Tests', function () {
  let verifier;
  let admin, validator1, validator2, validator3, validator4, validator5;
  let relayer, bridgeOperator, securityMonitor, emergencyResponder;
  let attacker, user1, user2;

  // Test constants
  const ETHEREUM_CHAIN_ID = 1;
  const POLYGON_CHAIN_ID = 137;
  const BSC_CHAIN_ID = 56;
  const MIN_SIGNATURE_THRESHOLD = 3;
  const MAX_MESSAGE_AGE = 3600; // 1 hour
  const RATE_LIMIT_WINDOW = 3600; // 1 hour

  beforeEach(async function () {
    [
      admin,
      validator1,
      validator2,
      validator3,
      validator4,
      validator5,
      relayer,
      bridgeOperator,
      securityMonitor,
      emergencyResponder,
      attacker,
      user1,
      user2,
    ] = await ethers.getSigners();

    // Deploy SecureCrossChainMessageVerifier
    const Verifier = await ethers.getContractFactory(
      'SecureCrossChainMessageVerifier'
    );
    verifier = await Verifier.deploy();

    // Set up roles
    const VALIDATOR_ROLE = await verifier.VALIDATOR_ROLE();
    const MESSAGE_RELAYER_ROLE = await verifier.MESSAGE_RELAYER_ROLE();
    const BRIDGE_OPERATOR_ROLE = await verifier.BRIDGE_OPERATOR_ROLE();
    const SECURITY_MONITOR_ROLE = await verifier.SECURITY_MONITOR_ROLE();
    const EMERGENCY_RESPONDER_ROLE = await verifier.EMERGENCY_RESPONDER_ROLE();

    await verifier.grantRole(MESSAGE_RELAYER_ROLE, relayer.address);
    await verifier.grantRole(BRIDGE_OPERATOR_ROLE, bridgeOperator.address);
    await verifier.grantRole(SECURITY_MONITOR_ROLE, securityMonitor.address);
    await verifier.grantRole(
      EMERGENCY_RESPONDER_ROLE,
      emergencyResponder.address
    );

    // Add validators
    await verifier.addValidator(validator1.address);
    await verifier.addValidator(validator2.address);
    await verifier.addValidator(validator3.address);
    await verifier.addValidator(validator4.address);
    await verifier.addValidator(validator5.address);

    // Add supported chains
    await verifier.addSupportedChain(ETHEREUM_CHAIN_ID);
    await verifier.addSupportedChain(POLYGON_CHAIN_ID);
    await verifier.addSupportedChain(BSC_CHAIN_ID);
  });

  async function createValidMessage(nonce = 1) {
    const payload = ethers.solidityPacked(
      ['string', 'uint256'],
      ['transfer', ethers.parseEther('100')]
    );

    const message = {
      messageId: 0, // Will be set by contract
      sourceChainId: ETHEREUM_CHAIN_ID,
      targetChainId: POLYGON_CHAIN_ID,
      sourceContract: user1.address,
      targetContract: user2.address,
      payload: payload,
      nonce: nonce,
      timestamp: Math.floor(Date.now() / 1000),
      deadline: Math.floor(Date.now() / 1000) + 3600,
      messageHash: ethers.ZeroHash, // Will be calculated
      merkleRoot: ethers.ZeroHash,
      merkleProof: [],
      gasLimit: 100000,
      gasPrice: ethers.parseUnits('20', 'gwei'),
      feeRecipient: admin.address,
      feeAmount: ethers.parseEther('0.01'),
    };

    // Calculate message hash
    message.messageHash = ethers.keccak256(
      ethers.AbiCoder.defaultAbiCoder().encode(
        [
          'uint256',
          'uint256',
          'address',
          'address',
          'bytes',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'uint256',
          'address',
          'uint256',
        ],
        [
          message.sourceChainId,
          message.targetChainId,
          message.sourceContract,
          message.targetContract,
          message.payload,
          message.nonce,
          message.timestamp,
          message.deadline,
          message.gasLimit,
          message.gasPrice,
          message.feeRecipient,
          message.feeAmount,
        ]
      )
    );

    message.merkleRoot = message.messageHash; // Simple case - single message

    return message;
  }

  async function signMessage(message, signers) {
    const messageHash = message.messageHash;
    const signatures = [];

    for (const signer of signers) {
      const signature = await signer.signMessage(ethers.getBytes(messageHash));
      signatures.push(signature);
    }

    return signatures;
  }

  describe('Access Control Security', function () {
    it('Should prevent unauthorized message submission', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(attacker).submitMessage(message, signatures)
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent unauthorized message execution', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      // Submit message as authorized relayer
      await verifier.connect(relayer).submitMessage(message, signatures);

      // Try to execute as unauthorized user
      await expect(
        verifier.connect(attacker).executeMessage(message)
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent unauthorized validator addition', async function () {
      await expect(
        verifier.connect(attacker).addValidator(attacker.address)
      ).to.be.revertedWith('AccessControl:');
    });

    it('Should prevent unauthorized emergency stop', async function () {
      await expect(
        verifier.connect(attacker).activateEmergencyStop('Attack attempt')
      ).to.be.revertedWith('AccessControl:');
    });
  });

  describe('Signature Validation Security', function () {
    it('Should reject messages with insufficient signatures', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [validator1, validator2]); // Only 2 signatures

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Insufficient signatures'
      );
    });

    it('Should reject messages with invalid signatures', async function () {
      const message = await createValidMessage();
      const signatures = [
        '0x1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890',
        '0x1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890',
        '0x1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890',
      ];

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith('ECDSA: invalid signature');
    });

    it('Should reject messages signed by unauthorized validators', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [attacker, user1, user2]); // Unauthorized signers

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Insufficient signatures'
      );
    });

    it('Should reject duplicate signatures from same validator', async function () {
      const message = await createValidMessage();
      const signature1 = await validator1.signMessage(
        ethers.getBytes(message.messageHash)
      );
      const signatures = [signature1, signature1, signature1]; // Same signature repeated

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Insufficient signatures'
      );
    });

    it('Should accept valid signatures from authorized validators', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.emit(verifier, 'MessageReceived');
    });
  });

  describe('Replay Attack Prevention', function () {
    it('Should prevent nonce reuse', async function () {
      const message1 = await createValidMessage(1);
      const signatures1 = await signMessage(message1, [
        validator1,
        validator2,
        validator3,
      ]);

      // Submit first message
      await verifier.connect(relayer).submitMessage(message1, signatures1);

      // Try to submit second message with same nonce
      const message2 = await createValidMessage(1); // Same nonce
      const signatures2 = await signMessage(message2, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message2, signatures2)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Nonce already used'
      );
    });

    it('Should prevent execution of same message twice', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      // Submit and execute message
      await verifier.connect(relayer).submitMessage(message, signatures);
      await verifier.connect(bridgeOperator).executeMessage(message);

      // Try to execute again
      await expect(
        verifier.connect(bridgeOperator).executeMessage(message)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Message already executed'
      );
    });

    it('Should track processed nonces correctly', async function () {
      const message = await createValidMessage(42);
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await verifier.connect(relayer).submitMessage(message, signatures);

      expect(await verifier.isMessageProcessed(ETHEREUM_CHAIN_ID, 42)).to.be
        .true;
      expect(await verifier.isMessageProcessed(ETHEREUM_CHAIN_ID, 43)).to.be
        .false;
    });
  });

  describe('Message Structure Validation', function () {
    it('Should reject messages with same source and target chain', async function () {
      const message = await createValidMessage();
      message.targetChainId = message.sourceChainId; // Same chain
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith('SecureCrossChainMessageVerifier: Same chain');
    });

    it('Should reject messages with zero address contracts', async function () {
      const message = await createValidMessage();
      message.sourceContract = ethers.ZeroAddress; // Invalid source
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith('SecureCrossChainMessageVerifier: Invalid source');
    });

    it('Should reject messages with empty payload', async function () {
      const message = await createValidMessage();
      message.payload = '0x'; // Empty payload
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith('SecureCrossChainMessageVerifier: Empty payload');
    });

    it('Should reject expired messages', async function () {
      const message = await createValidMessage();
      message.deadline = Math.floor(Date.now() / 1000) - 3600; // Expired 1 hour ago
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith('SecureCrossChainMessageVerifier: Message expired');
    });

    it('Should reject messages with invalid gas limits', async function () {
      const message = await createValidMessage();
      message.gasLimit = 10000; // Below minimum
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Invalid gas limit'
      );
    });
  });

  describe('Chain Support Validation', function () {
    it('Should reject messages from unsupported chains', async function () {
      const message = await createValidMessage();
      message.sourceChainId = 999; // Unsupported chain
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Chain not supported'
      );
    });

    it('Should reject messages to unsupported chains', async function () {
      const message = await createValidMessage();
      message.targetChainId = 999; // Unsupported chain
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Chain not supported'
      );
    });
  });

  describe('Timestamp Validation', function () {
    it('Should reject messages with old timestamps', async function () {
      const message = await createValidMessage();
      message.timestamp = Math.floor(Date.now() / 1000) - MAX_MESSAGE_AGE - 1; // Too old
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Invalid timestamp'
      );
    });

    it('Should reject messages with future timestamps', async function () {
      const message = await createValidMessage();
      message.timestamp = Math.floor(Date.now() / 1000) + 600; // 10 minutes in future
      message.messageHash = ethers.keccak256(
        ethers.AbiCoder.defaultAbiCoder().encode(
          [
            'uint256',
            'uint256',
            'address',
            'address',
            'bytes',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'uint256',
            'address',
            'uint256',
          ],
          [
            message.sourceChainId,
            message.targetChainId,
            message.sourceContract,
            message.targetContract,
            message.payload,
            message.nonce,
            message.timestamp,
            message.deadline,
            message.gasLimit,
            message.gasPrice,
            message.feeRecipient,
            message.feeAmount,
          ]
        )
      );

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Invalid timestamp'
      );
    });
  });

  describe('Rate Limiting Security', function () {
    it('Should enforce rate limiting for message submissions', async function () {
      // Submit first message
      const message1 = await createValidMessage(1);
      const signatures1 = await signMessage(message1, [
        validator1,
        validator2,
        validator3,
      ]);
      await verifier.connect(relayer).submitMessage(message1, signatures1);

      // Try to submit second message immediately (should succeed as first message in window)
      const message2 = await createValidMessage(2);
      const signatures2 = await signMessage(message2, [
        validator1,
        validator2,
        validator3,
      ]);
      await verifier.connect(relayer).submitMessage(message2, signatures2);

      // Simulate many messages to trigger rate limit
      for (let i = 3; i <= 100; i++) {
        const message = await createValidMessage(i);
        const signatures = await signMessage(message, [
          validator1,
          validator2,
          validator3,
        ]);
        await verifier.connect(relayer).submitMessage(message, signatures);
      }

      // This should fail due to rate limit
      const message101 = await createValidMessage(101);
      const signatures101 = await signMessage(message101, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message101, signatures101)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Rate limit exceeded'
      );
    });
  });

  describe('Emergency Controls', function () {
    it('Should prevent operations during emergency stop', async function () {
      // Activate emergency stop
      await verifier
        .connect(emergencyResponder)
        .activateEmergencyStop('Security incident');

      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Emergency stop active'
      );
    });

    it('Should allow operations after emergency stop is deactivated', async function () {
      // Activate and then deactivate emergency stop
      await verifier.connect(emergencyResponder).activateEmergencyStop('Test');
      await verifier.connect(emergencyResponder).deactivateEmergencyStop();

      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.emit(verifier, 'MessageReceived');
    });

    it('Should prevent unauthorized emergency stop deactivation', async function () {
      await verifier.connect(emergencyResponder).activateEmergencyStop('Test');

      await expect(
        verifier.connect(attacker).deactivateEmergencyStop()
      ).to.be.revertedWith('AccessControl:');
    });
  });

  describe('Message Hash Validation', function () {
    it('Should reject messages with invalid hash', async function () {
      const message = await createValidMessage();
      message.messageHash = ethers.keccak256(
        ethers.toUtf8Bytes('invalid hash')
      ); // Wrong hash

      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Invalid message hash'
      );
    });

    it('Should accept messages with correct hash', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.emit(verifier, 'MessageReceived');
    });
  });

  describe('Validator Management Security', function () {
    it('Should prevent adding duplicate validators', async function () {
      await expect(
        verifier.addValidator(validator1.address)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Already validator'
      );
    });

    it('Should prevent removing validators below threshold', async function () {
      // Remove validators until we're at the threshold
      await verifier.removeValidator(validator5.address);
      await verifier.removeValidator(validator4.address);
      // Now we have 4 validators (admin + 3 added), threshold is 3

      // Try to remove one more (would leave only 3, breaking threshold of 3)
      await expect(
        verifier.removeValidator(validator3.address)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Would break threshold'
      );
    });

    it('Should allow updating threshold within valid range', async function () {
      await expect(verifier.updateSignatureThreshold(4)).to.emit(
        verifier,
        'ThresholdUpdated'
      );
    });

    it('Should prevent setting threshold too low', async function () {
      await expect(verifier.updateSignatureThreshold(2)).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Threshold too low'
      );
    });

    it('Should prevent setting threshold too high', async function () {
      await expect(verifier.updateSignatureThreshold(10)).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Threshold too high'
      );
    });
  });

  describe('Comprehensive Attack Scenarios', function () {
    it('Should prevent coordinated signature forgery attack', async function () {
      // Attacker tries to use signatures from previous valid message for new message
      const message1 = await createValidMessage(1);
      const signatures1 = await signMessage(message1, [
        validator1,
        validator2,
        validator3,
      ]);

      await verifier.connect(relayer).submitMessage(message1, signatures1);

      // Create different message but try to use old signatures
      const message2 = await createValidMessage(2);

      await expect(
        verifier.connect(relayer).submitMessage(message2, signatures1) // Wrong signatures
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Insufficient signatures'
      );
    });

    it('Should handle complete end-to-end secure message processing', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      // Submit message
      await expect(
        verifier.connect(relayer).submitMessage(message, signatures)
      ).to.emit(verifier, 'MessageReceived');

      // Verify validation result
      const validationResult = await verifier.getValidationResult(
        message.messageHash
      );
      expect(validationResult.isValid).to.be.true;
      expect(validationResult.signatureValid).to.be.true;
      expect(validationResult.merkleProofValid).to.be.true;

      // Execute message
      await expect(
        verifier.connect(bridgeOperator).executeMessage(message)
      ).to.emit(verifier, 'MessageExecuted');
    });

    it('Should prevent execution of unvalidated messages', async function () {
      const message = await createValidMessage();

      // Try to execute without submitting/validating first
      await expect(
        verifier.connect(bridgeOperator).executeMessage(message)
      ).to.be.revertedWith(
        'SecureCrossChainMessageVerifier: Message not validated'
      );
    });
  });

  describe('Security Metrics and Monitoring', function () {
    it('Should provide accurate security metrics', async function () {
      const metrics = await verifier.getSecurityMetrics();

      expect(metrics.totalValidators).to.equal(6); // Admin + 5 added validators
      expect(metrics.signatureThreshold_).to.equal(MIN_SIGNATURE_THRESHOLD);
      expect(metrics.supportedChains_).to.equal(3); // ETH, POLYGON, BSC
      expect(metrics.emergencyStop_).to.be.false;
    });

    it('Should track validator authorization correctly', async function () {
      expect(await verifier.isAuthorizedValidator(validator1.address)).to.be
        .true;
      expect(await verifier.isAuthorizedValidator(attacker.address)).to.be
        .false;
    });

    it('Should track chain support correctly', async function () {
      expect(await verifier.isChainSupported(ETHEREUM_CHAIN_ID)).to.be.true;
      expect(await verifier.isChainSupported(999)).to.be.false;
    });
  });

  describe('Gas Optimization Validation', function () {
    it('Should maintain reasonable gas costs for security operations', async function () {
      const message = await createValidMessage();
      const signatures = await signMessage(message, [
        validator1,
        validator2,
        validator3,
      ]);

      const tx = await verifier
        .connect(relayer)
        .submitMessage(message, signatures);
      const receipt = await tx.wait();

      console.log(`Message submission gas used: ${receipt.gasUsed}`);

      // Should be under reasonable limit for cross-chain operations
      expect(receipt.gasUsed).to.be.below(500000);
    });
  });
});
