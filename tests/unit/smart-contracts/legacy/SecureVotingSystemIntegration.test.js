const { expect } = require("chai");
const { ethers } = require("hardhat");
const { loadFixture, time } = require("@nomicfoundation/hardhat-network-helpers");

/**
 * SecureVotingSystem Integration Tests
 * 
 * Tests the integration between SecureVotingSystem and the broader WRFCoin platform,
 * including governance contracts, DeFi protocols, and cross-chain bridges.
 * 
 * Platform Integration Points:
 * - WRFGovernor.sol: Multi-voting mechanism governance
 * - ComprehensiveGovernanceSecurity.sol: Master security layer
 * - Carbon credit trading integration
 * - Cross-chain voting capabilities
 * - DeFi protocol governance integration
 * 
 * OWASP SC02 Access Control Validation:
 * - Role-based access control integration
 * - Multi-signature governance controls
 * - Emergency override mechanisms
 * - Principle of least privilege validation
 */

describe("SecureVotingSystem Platform Integration", function () {
    let secureVotingSystem;
    let wrfGovernor;
    let comprehensiveGovernance;
    let mockToken;
    let carbonCreditToken;
    let crossChainBridge;
    let owner, validator1, validator2, validator3, user1, user2, user3;
    let emergencyCoordinator, guardianCouncil;

    const VALIDATOR_ROLE = ethers.keccak256(ethers.toUtf8Bytes("VALIDATOR_ROLE"));
    const EMERGENCY_COORDINATOR_ROLE = ethers.keccak256(ethers.toUtf8Bytes("EMERGENCY_COORDINATOR_ROLE"));
    const GUARDIAN_ROLE = ethers.keccak256(ethers.toUtf8Bytes("GUARDIAN_ROLE"));

    // Integration deployment fixture
    async function deployIntegrationFixture() {
        [owner, validator1, validator2, validator3, user1, user2, user3, emergencyCoordinator, guardianCouncil] = await ethers.getSigners();

        // Deploy mock tokens
        const MockERC20 = await ethers.getContractFactory("MockERC20");
        mockToken = await MockERC20.deploy("WRFCoin", "WRF", ethers.parseEther("1000000"));
        carbonCreditToken = await MockERC20.deploy("CarbonCredit", "CARBON", ethers.parseEther("500000"));

        // Deploy SecureVotingSystem
        const SecureVotingSystem = await ethers.getContractFactory("SecureVotingSystem");
        secureVotingSystem = await SecureVotingSystem.deploy(
            await mockToken.getAddress(),
            [validator1.address, validator2.address, validator3.address],
            emergencyCoordinator.address
        );

        // Deploy mock WRFGovernor
        const WRFGovernor = await ethers.getContractFactory("MockWRFGovernor");
        wrfGovernor = await WRFGovernor.deploy(
            await mockToken.getAddress(),
            await secureVotingSystem.getAddress()
        );

        // Deploy comprehensive governance security
        const ComprehensiveGovernance = await ethers.getContractFactory("MockComprehensiveGovernance");
        comprehensiveGovernance = await ComprehensiveGovernance.deploy(
            await secureVotingSystem.getAddress(),
            guardianCouncil.address
        );

        // Deploy mock cross-chain bridge
        const CrossChainBridge = await ethers.getContractFactory("MockCrossChainBridge");
        crossChainBridge = await CrossChainBridge.deploy(
            await secureVotingSystem.getAddress()
        );

        // Setup roles and permissions
        await secureVotingSystem.grantRole(VALIDATOR_ROLE, validator1.address);
        await secureVotingSystem.grantRole(VALIDATOR_ROLE, validator2.address);
        await secureVotingSystem.grantRole(VALIDATOR_ROLE, validator3.address);
        await secureVotingSystem.grantRole(EMERGENCY_COORDINATOR_ROLE, emergencyCoordinator.address);

        // Distribute tokens for testing
        await mockToken.transfer(user1.address, ethers.parseEther("10000"));
        await mockToken.transfer(user2.address, ethers.parseEther("10000"));
        await mockToken.transfer(user3.address, ethers.parseEther("10000"));
        await carbonCreditToken.transfer(user1.address, ethers.parseEther("5000"));
        await carbonCreditToken.transfer(user2.address, ethers.parseEther("5000"));

        return {
            secureVotingSystem,
            wrfGovernor,
            comprehensiveGovernance,
            crossChainBridge,
            mockToken,
            carbonCreditToken,
            owner,
            validator1,
            validator2,
            validator3,
            user1,
            user2,
            user3,
            emergencyCoordinator,
            guardianCouncil
        };
    }

    beforeEach(async function () {
        ({
            secureVotingSystem,
            wrfGovernor,
            comprehensiveGovernance,
            crossChainBridge,
            mockToken,
            carbonCreditToken,
            owner,
            validator1,
            validator2,
            validator3,
            user1,
            user2,
            user3,
            emergencyCoordinator,
            guardianCouncil
        } = await loadFixture(deployIntegrationFixture));
    });

    describe("WRFGovernor Integration", function () {
        it("Should integrate with WRFGovernor for multi-mechanism voting", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("test-proposal-1"));
            const description = "Integrate quadratic voting with timestamp protection";
            const votingPeriod = 3 * 24 * 60 * 60;

            // Create proposal through WRFGovernor
            await expect(
                wrfGovernor.createProposal(
                    proposalId,
                    description,
                    votingPeriod,
                    1 // QuadraticVoting type
                )
            ).to.emit(secureVotingSystem, "ProposalCreated")
             .withArgs(proposalId, owner.address, description);

            // Verify proposal exists in SecureVotingSystem
            const proposal = await secureVotingSystem.getProposal(proposalId);
            expect(proposal.creator).to.equal(owner.address);
            expect(proposal.description).to.equal(description);
            expect(proposal.votingMechanism).to.equal(1); // QuadraticVoting

            // Test voting through WRFGovernor with timestamp validation
            const currentTime = await time.latest();
            const voteData = ethers.AbiCoder.defaultAbiCoder().encode(
                ["uint256", "uint256"],
                [100, currentTime + 60] // Vote weight and timestamp
            );

            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));
            
            await expect(
                wrfGovernor.connect(user1).vote(proposalId, true, voteData)
            ).to.emit(secureVotingSystem, "VoteCast");

            // Verify vote was recorded with timestamp validation
            const voteInfo = await secureVotingSystem.getVote(proposalId, user1.address);
            expect(voteInfo.support).to.be.true;
            expect(voteInfo.weight).to.be.gt(0);
        });

        it("Should enforce timestamp manipulation protection in WRFGovernor voting", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("timestamp-test"));
            await wrfGovernor.createProposal(proposalId, "Test timestamp protection", 7200, 0);

            // Attempt to vote with manipulated future timestamp
            const futureTime = (await time.latest()) + 3600; // 1 hour in future
            const maliciousVoteData = ethers.AbiCoder.defaultAbiCoder().encode(
                ["uint256", "uint256"],
                [50, futureTime + 1000] // Extremely future timestamp
            );

            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("50"));

            await expect(
                wrfGovernor.connect(user1).vote(proposalId, true, maliciousVoteData)
            ).to.be.revertedWith("Timestamp outside acceptable range");
        });

        it("Should handle ranked-choice voting integration", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("ranked-choice-test"));
            await wrfGovernor.createProposal(proposalId, "Ranked choice proposal", 7200, 2); // RankedChoice

            // Vote with ranked preferences
            const rankedChoices = [1, 3, 2]; // Preference order
            const voteData = ethers.AbiCoder.defaultAbiCoder().encode(
                ["uint256[]", "uint256"],
                [rankedChoices, await time.latest() + 30]
            );

            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));

            await expect(
                wrfGovernor.connect(user1).vote(proposalId, true, voteData)
            ).to.emit(secureVotingSystem, "RankedVoteCast")
             .withArgs(proposalId, user1.address, rankedChoices);
        });
    });

    describe("ComprehensiveGovernanceSecurity Integration", function () {
        it("Should validate through all security layers", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("security-validation"));
            
            // Create high-stakes proposal requiring comprehensive security
            await comprehensiveGovernance.createSecureProposal(
                proposalId,
                "High-value treasury allocation",
                ethers.parseEther("1000000"), // $1M value trigger
                7200
            );

            // Verify security layers are activated
            const securityLayers = await comprehensiveGovernance.getActiveSecurityLayers(proposalId);
            expect(securityLayers.timestampValidation).to.be.true;
            expect(securityLayers.validatorConsensus).to.be.true;
            expect(securityLayers.emergencyOverride).to.be.true;
            expect(securityLayers.guardianReview).to.be.true;

            // Test voting through comprehensive security
            const secureVoteData = ethers.AbiCoder.defaultAbiCoder().encode(
                ["uint256", "uint256", "bytes32"],
                [100, await time.latest() + 30, ethers.randomBytes(32)]
            );

            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));

            await expect(
                comprehensiveGovernance.connect(user1).secureVote(proposalId, true, secureVoteData)
            ).to.emit(secureVotingSystem, "VoteCast");
        });

        it("Should trigger guardian council intervention for suspicious activity", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("suspicious-activity"));
            await comprehensiveGovernance.createSecureProposal(
                proposalId,
                "Regular proposal",
                ethers.parseEther("10000"),
                7200
            );

            // Simulate suspicious voting pattern (rapid consecutive votes)
            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("1000"));
            await mockToken.connect(user2).approve(await secureVotingSystem.getAddress(), ethers.parseEther("1000"));
            await mockToken.connect(user3).approve(await secureVotingSystem.getAddress(), ethers.parseEther("1000"));

            const baseTime = await time.latest();
            
            // Rapid voting pattern within same block window
            await comprehensiveGovernance.connect(user1).secureVote(
                proposalId,
                true,
                ethers.AbiCoder.defaultAbiCoder().encode(
                    ["uint256", "uint256", "bytes32"],
                    [333, baseTime + 10, ethers.randomBytes(32)]
                )
            );

            await comprehensiveGovernance.connect(user2).secureVote(
                proposalId,
                true,
                ethers.AbiCoder.defaultAbiCoder().encode(
                    ["uint256", "uint256", "bytes32"],
                    [333, baseTime + 11, ethers.randomBytes(32)]
                )
            );

            // Third vote should trigger guardian intervention
            await expect(
                comprehensiveGovernance.connect(user3).secureVote(
                    proposalId,
                    true,
                    ethers.AbiCoder.defaultAbiCoder().encode(
                        ["uint256", "uint256", "bytes32"],
                        [334, baseTime + 12, ethers.randomBytes(32)]
                    )
                )
            ).to.emit(comprehensiveGovernance, "GuardianInterventionTriggered")
             .withArgs(proposalId, "Suspicious voting pattern detected");
        });

        it("Should enforce emergency override protocols", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("emergency-test"));
            await comprehensiveGovernance.createSecureProposal(proposalId, "Emergency test", ethers.parseEther("50000"), 7200);

            // Simulate emergency condition
            await comprehensiveGovernance.connect(guardianCouncil).triggerEmergencyOverride(
                proposalId,
                "Critical security vulnerability detected"
            );

            // Verify proposal is locked
            const proposal = await secureVotingSystem.getProposal(proposalId);
            expect(proposal.emergencyOverride).to.be.true;

            // Verify voting is disabled
            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));
            
            await expect(
                comprehensiveGovernance.connect(user1).secureVote(
                    proposalId,
                    true,
                    ethers.AbiCoder.defaultAbiCoder().encode(
                        ["uint256", "uint256", "bytes32"],
                        [100, await time.latest() + 30, ethers.randomBytes(32)]
                    )
                )
            ).to.be.revertedWith("Emergency override active");
        });
    });

    describe("Carbon Credit Trading Integration", function () {
        it("Should enable carbon credit governance voting", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("carbon-governance"));
            
            // Create carbon credit governance proposal
            await secureVotingSystem.createProposal(
                proposalId,
                "Carbon offset verification standards",
                0, // StandardVoting
                7200,
                ethers.ZeroAddress,
                await carbonCreditToken.getAddress() // Use carbon credits as voting token
            );

            // Vote using carbon credits
            await carbonCreditToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));
            
            const voteData = ethers.AbiCoder.defaultAbiCoder().encode(
                ["uint256", "uint256"],
                [100, await time.latest() + 30]
            );

            await expect(
                secureVotingSystem.connect(user1).castVote(proposalId, true, voteData)
            ).to.emit(secureVotingSystem, "VoteCast")
             .withArgs(proposalId, user1.address, true, 100);

            // Verify carbon credit voting weight
            const proposal = await secureVotingSystem.getProposal(proposalId);
            expect(proposal.forVotes).to.equal(100);
        });

        it("Should validate carbon credit authenticity in voting", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("carbon-authenticity"));
            await secureVotingSystem.createProposal(
                proposalId,
                "Carbon authenticity test",
                0,
                7200,
                ethers.ZeroAddress,
                await carbonCreditToken.getAddress()
            );

            // Attempt vote with insufficient carbon credits
            await expect(
                secureVotingSystem.connect(user3).castVote(
                    proposalId,
                    true,
                    ethers.AbiCoder.defaultAbiCoder().encode(
                        ["uint256", "uint256"],
                        [1000, await time.latest() + 30] // User3 has 0 carbon credits
                    )
                )
            ).to.be.revertedWith("Insufficient token balance");
        });
    });

    describe("Cross-Chain Voting Integration", function () {
        it("Should coordinate cross-chain voting with timestamp synchronization", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("cross-chain-proposal"));
            const chainId = 137; // Polygon
            
            // Initiate cross-chain proposal
            await crossChainBridge.initiateCrossChainProposal(
                proposalId,
                "Cross-chain governance test",
                chainId,
                7200
            );

            // Verify proposal is created with cross-chain metadata
            const proposal = await secureVotingSystem.getProposal(proposalId);
            expect(proposal.crossChainEnabled).to.be.true;
            expect(proposal.sourceChainId).to.equal(chainId);

            // Test cross-chain vote with timestamp validation
            const crossChainVoteData = ethers.AbiCoder.defaultAbiCoder().encode(
                ["uint256", "uint256", "uint256", "bytes32"],
                [100, await time.latest() + 30, chainId, ethers.randomBytes(32)]
            );

            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));

            await expect(
                crossChainBridge.connect(user1).castCrossChainVote(proposalId, true, crossChainVoteData)
            ).to.emit(secureVotingSystem, "CrossChainVoteCast")
             .withArgs(proposalId, user1.address, chainId, true, 100);
        });

        it("Should prevent cross-chain timestamp manipulation", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("cross-chain-security"));
            const chainId = 56; // BSC
            
            await crossChainBridge.initiateCrossChainProposal(proposalId, "Security test", chainId, 7200);

            // Attempt cross-chain vote with manipulated timestamp from different chain
            const manipulatedTime = (await time.latest()) + 7200; // 2 hours future
            const maliciousVoteData = ethers.AbiCoder.defaultAbiCoder().encode(
                ["uint256", "uint256", "uint256", "bytes32"],
                [100, manipulatedTime, chainId, ethers.randomBytes(32)]
            );

            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));

            await expect(
                crossChainBridge.connect(user1).castCrossChainVote(proposalId, true, maliciousVoteData)
            ).to.be.revertedWith("Cross-chain timestamp validation failed");
        });
    });

    describe("OWASP SC02 Access Control Validation", function () {
        it("Should enforce role-based access control across platform integration", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("access-control-test"));
            
            // Test unauthorized proposal creation through WRFGovernor
            await expect(
                wrfGovernor.connect(user1).createRestrictedProposal(
                    proposalId,
                    "Restricted proposal",
                    7200,
                    ethers.parseEther("1000000") // High-value requiring special permissions
                )
            ).to.be.revertedWithCustomError(secureVotingSystem, "AccessControlUnauthorizedAccount");

            // Grant necessary role and retry
            await secureVotingSystem.grantRole(
                ethers.keccak256(ethers.toUtf8Bytes("PROPOSAL_CREATOR_ROLE")),
                user1.address
            );

            await expect(
                wrfGovernor.connect(user1).createRestrictedProposal(proposalId, "Authorized proposal", 7200, ethers.parseEther("1000000"))
            ).to.emit(secureVotingSystem, "ProposalCreated");
        });

        it("Should validate principle of least privilege in validator operations", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("validator-privilege-test"));
            await secureVotingSystem.createProposal(proposalId, "Validator test", 0, 7200, ethers.ZeroAddress, await mockToken.getAddress());

            // Test validator attempting unauthorized emergency pause
            await expect(
                secureVotingSystem.connect(validator1).emergencyPause("Unauthorized pause")
            ).to.be.revertedWithCustomError(secureVotingSystem, "AccessControlUnauthorizedAccount");

            // Only emergency coordinator should be able to pause
            await expect(
                secureVotingSystem.connect(emergencyCoordinator).emergencyPause("Authorized emergency pause")
            ).to.emit(secureVotingSystem, "EmergencyPaused");
        });

        it("Should enforce multi-signature requirements for critical operations", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("multi-sig-test"));
            await secureVotingSystem.createProposal(proposalId, "Multi-sig test", 0, 7200, ethers.ZeroAddress, await mockToken.getAddress());

            // Attempt to finalize high-value proposal with single signature
            await time.increase(7201); // Past voting period
            
            await expect(
                secureVotingSystem.connect(validator1).finalizeProposal(proposalId)
            ).to.be.revertedWith("Multi-signature required for high-value proposals");

            // Require multiple validator confirmations
            await secureVotingSystem.connect(validator1).confirmProposalFinalization(proposalId);
            await secureVotingSystem.connect(validator2).confirmProposalFinalization(proposalId);
            
            // Now finalization should succeed
            await expect(
                secureVotingSystem.connect(validator3).finalizeProposal(proposalId)
            ).to.emit(secureVotingSystem, "ProposalFinalized");
        });

        it("Should audit and log all privileged operations", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("audit-log-test"));
            
            // Create proposal - should log
            await expect(
                secureVotingSystem.createProposal(proposalId, "Audit test", 0, 7200, ethers.ZeroAddress, await mockToken.getAddress())
            ).to.emit(secureVotingSystem, "PrivilegedOperationLogged")
             .withArgs(owner.address, "CREATE_PROPOSAL", proposalId);

            // Validator operation - should log
            await expect(
                secureVotingSystem.connect(validator1).validateTimestamp(proposalId, await time.latest() + 30)
            ).to.emit(secureVotingSystem, "PrivilegedOperationLogged")
             .withArgs(validator1.address, "VALIDATE_TIMESTAMP", proposalId);

            // Emergency operation - should log
            await expect(
                secureVotingSystem.connect(emergencyCoordinator).emergencyPause("Audit test pause")
            ).to.emit(secureVotingSystem, "PrivilegedOperationLogged")
             .withArgs(emergencyCoordinator.address, "EMERGENCY_PAUSE", ethers.ZeroHash);
        });
    });

    describe("Performance Integration Tests", function () {
        it("Should maintain performance under integrated load", async function () {
            const proposalCount = 50;
            const votersPerProposal = 20;
            const startTime = process.hrtime();

            // Create multiple proposals through different integration points
            for (let i = 0; i < proposalCount; i++) {
                const proposalId = ethers.keccak256(ethers.toUtf8Bytes(`load-test-${i}`));
                
                if (i % 3 === 0) {
                    // WRFGovernor integration
                    await wrfGovernor.createProposal(proposalId, `WRF Proposal ${i}`, 7200, 0);
                } else if (i % 3 === 1) {
                    // ComprehensiveGovernance integration
                    await comprehensiveGovernance.createSecureProposal(proposalId, `Secure Proposal ${i}`, ethers.parseEther("10000"), 7200);
                } else {
                    // Direct SecureVotingSystem
                    await secureVotingSystem.createProposal(proposalId, `Direct Proposal ${i}`, 0, 7200, ethers.ZeroAddress, await mockToken.getAddress());
                }

                // Cast votes through various integration points
                for (let j = 0; j < Math.min(votersPerProposal, 9); j++) { // Limit to available signers
                    const voter = [user1, user2, user3, validator1, validator2, validator3, owner, emergencyCoordinator, guardianCouncil][j];
                    await mockToken.connect(voter).approve(await secureVotingSystem.getAddress(), ethers.parseEther("10"));
                    
                    const voteData = ethers.AbiCoder.defaultAbiCoder().encode(
                        ["uint256", "uint256"],
                        [10, await time.latest() + 30]
                    );

                    if (i % 3 === 0) {
                        await wrfGovernor.connect(voter).vote(proposalId, j % 2 === 0, voteData);
                    } else if (i % 3 === 1) {
                        const secureVoteData = ethers.AbiCoder.defaultAbiCoder().encode(
                            ["uint256", "uint256", "bytes32"],
                            [10, await time.latest() + 30, ethers.randomBytes(32)]
                        );
                        await comprehensiveGovernance.connect(voter).secureVote(proposalId, j % 2 === 0, secureVoteData);
                    } else {
                        await secureVotingSystem.connect(voter).castVote(proposalId, j % 2 === 0, voteData);
                    }
                }
            }

            const [seconds, nanoseconds] = process.hrtime(startTime);
            const totalTime = seconds + nanoseconds / 1e9;
            
            // Should complete within reasonable time (adjust based on requirements)
            expect(totalTime).to.be.lessThan(300); // 5 minutes for comprehensive integration test
            
            console.log(`Integration performance test completed in ${totalTime.toFixed(2)} seconds`);
            console.log(`Average time per proposal: ${(totalTime / proposalCount).toFixed(3)} seconds`);
            console.log(`Average time per vote: ${(totalTime / (proposalCount * Math.min(votersPerProposal, 9))).toFixed(4)} seconds`);
        });

        it("Should handle concurrent cross-chain operations", async function () {
            const concurrentProposals = 10;
            const promises = [];

            for (let i = 0; i < concurrentProposals; i++) {
                const proposalId = ethers.keccak256(ethers.toUtf8Bytes(`concurrent-${i}`));
                const chainId = 137 + i; // Different chain IDs
                
                promises.push(
                    crossChainBridge.initiateCrossChainProposal(
                        proposalId,
                        `Concurrent proposal ${i}`,
                        chainId,
                        7200
                    )
                );
            }

            // All operations should complete without race conditions
            await expect(Promise.all(promises)).to.not.be.rejected;

            // Verify all proposals were created correctly
            for (let i = 0; i < concurrentProposals; i++) {
                const proposalId = ethers.keccak256(ethers.toUtf8Bytes(`concurrent-${i}`));
                const proposal = await secureVotingSystem.getProposal(proposalId);
                expect(proposal.crossChainEnabled).to.be.true;
                expect(proposal.sourceChainId).to.equal(137 + i);
            }
        });
    });

    describe("Compliance and Security Documentation", function () {
        it("Should generate compliance reports for integrated operations", async function () {
            const proposalId = ethers.keccak256(ethers.toUtf8Bytes("compliance-report-test"));
            
            // Perform various operations that require compliance tracking
            await secureVotingSystem.createProposal(proposalId, "Compliance test", 0, 7200, ethers.ZeroAddress, await mockToken.getAddress());
            
            await mockToken.connect(user1).approve(await secureVotingSystem.getAddress(), ethers.parseEther("100"));
            await secureVotingSystem.connect(user1).castVote(
                proposalId,
                true,
                ethers.AbiCoder.defaultAbiCoder().encode(
                    ["uint256", "uint256"],
                    [100, await time.latest() + 30]
                )
            );

            // Generate compliance report
            const complianceReport = await secureVotingSystem.generateComplianceReport(proposalId);
            
            expect(complianceReport.proposalId).to.equal(proposalId);
            expect(complianceReport.timestampValidationPassed).to.be.true;
            expect(complianceReport.accessControlValidated).to.be.true;
            expect(complianceReport.owasp_SC02_compliant).to.be.true;
            expect(complianceReport.auditTrailComplete).to.be.true;
        });

        it("Should document security measures across platform integration", async function () {
            // Test security documentation generation
            const securityDoc = await secureVotingSystem.generateSecurityDocumentation();
            
            expect(securityDoc.timestampProtection).to.include("Multi-block confirmation");
            expect(securityDoc.accessControl).to.include("Role-based access control");
            expect(securityDoc.integrationSecurity).to.include("Cross-platform validation");
            expect(securityDoc.emergencyProcedures).to.include("Guardian council oversight");
            expect(securityDoc.complianceFrameworks).to.include("OWASP SC02");
        });
    });
});
