// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BaseERC721NFT.sol";

/**
 * @title ComputationalNFT
 * @dev NFTs representing computational jobs/resources
 * 
 * Features:
 * - Computational job tokenization
 * - Resource allocation and tracking
 * - Job status and result storage
 * - Payment and escrow system
 * - Computing provider verification
 * - Quality scoring and reputation
 * 
 * Use Cases:
 * - Weather modeling computation
 * - Climate simulation jobs
 * - AI/ML training tasks
 * - Distributed computing marketplace
 */
contract ComputationalNFT is BaseERC721NFT {
    enum JobStatus {
        CREATED,      // Job created but not assigned
        ASSIGNED,     // Assigned to computing provider
        RUNNING,      // Currently being executed
        COMPLETED,    // Successfully completed
        FAILED,       // Failed execution
        DISPUTED,     // In dispute resolution
        CANCELLED     // Cancelled by requester
    }
    
    enum ComputationType {
        WEATHER_MODELING,
        CLIMATE_SIMULATION,
        AI_TRAINING,
        DATA_PROCESSING,
        CRYPTOGRAPHIC,
        GENERAL_PURPOSE
    }
    
    struct ComputationalJob {
        uint256 tokenId;
        address requester;           // Who requested the computation
        address provider;            // Who will execute the computation
        ComputationType computationType;
        JobStatus status;
        
        // Resource requirements
        uint256 cpuCores;           // Required CPU cores
        uint256 memoryGB;           // Required memory in GB
        uint256 storageGB;          // Required storage in GB
        uint256 gpuUnits;           // Required GPU units
        uint256 estimatedDuration;   // Estimated duration in seconds
        
        // Economic terms
        uint256 paymentAmount;      // Payment for completion
        uint256 escrowAmount;       // Amount held in escrow
        uint256 penaltyAmount;      // Penalty for failure
        address paymentToken;       // Token used for payment
        
        // Execution data
        uint256 startTime;          // When execution started
        uint256 completionTime;     // When execution completed
        string resultHash;          // Hash of computation result
        string inputDataHash;       // Hash of input data
        uint256 actualDuration;     // Actual execution time
        
        // Quality metrics
        uint256 qualityScore;       // Quality score (0-10000 basis points)
        uint256 verificationCount;  // Number of verifications
        bool verified;              // Whether result is verified
    }
    
    struct ProviderProfile {
        uint256 totalJobsCompleted;
        uint256 totalJobsFailed;
        uint256 totalComputeTime;
        uint256 reputationScore;    // Reputation score (0-10000)
        uint256[] completedJobs;    // Array of completed job token IDs
        mapping(ComputationType => uint256) specializationScores;
        bool active;
        uint256 stakeAmount;        // Staked tokens as collateral
    }
    
    // Core data
    mapping(uint256 => ComputationalJob) public jobs;
    mapping(address => ProviderProfile) public providers;
    mapping(address => uint256[]) public requesterJobs;
    mapping(address => uint256[]) public providerJobs;
    
    // Job marketplace
    uint256[] public availableJobs;
    mapping(ComputationType => uint256[]) public jobsByType;
    
    // Verification and disputes
    mapping(uint256 => address[]) public jobVerifiers;
    mapping(uint256 => mapping(address => bool)) public verificationVotes;
    mapping(uint256 => string) public disputeReasons;
    address public disputeResolver;
    
    // Economic parameters
    uint256 public platformFeeRate = 250; // 2.5% in basis points
    address public feeRecipient;
    uint256 public minimumStakeAmount = 1000 * 10**18; // Minimum stake for providers
    
    // Events
    event JobCreated(uint256 indexed tokenId, address indexed requester, ComputationType computationType);
    event JobAssigned(uint256 indexed tokenId, address indexed provider);
    event JobStarted(uint256 indexed tokenId, uint256 startTime);
    event JobCompleted(uint256 indexed tokenId, uint256 completionTime, string resultHash);
    event JobFailed(uint256 indexed tokenId, string reason);
    event JobVerified(uint256 indexed tokenId, address indexed verifier, bool approved);
    event ProviderRegistered(address indexed provider, uint256 stakeAmount);
    event DisputeRaised(uint256 indexed tokenId, string reason);
    
    // Errors
    error InsufficientJobPayment(uint256 provided, uint256 required);
    error JobNotAvailable(uint256 tokenId);
    error UnauthorizedProvider(address provider);
    error InvalidJobStatus(JobStatus current, JobStatus required);
    error InsufficientStake(uint256 provided, uint256 required);
    error JobAlreadyAssigned(uint256 tokenId);
    
    constructor(
        string memory name,
        string memory symbol,
        string memory baseTokenURI,
        address initialOwner
    ) BaseERC721NFT(name, symbol, baseTokenURI, initialOwner) {
        feeRecipient = initialOwner;
        disputeResolver = initialOwner;
    }
    
    /**
     * @dev Create a new computational job
     */
    function createJob(
        ComputationType computationType,
        uint256 cpuCores,
        uint256 memoryGB,
        uint256 storageGB,
        uint256 gpuUnits,
        uint256 estimatedDuration,
        uint256 paymentAmount,
        address paymentToken,
        string calldata inputDataHash
    ) external payable nonReentrant returns (uint256 tokenId) {
        // Calculate required escrow (payment + platform fee)
        uint256 platformFee = (paymentAmount * platformFeeRate) / 10000;
        uint256 totalRequired = paymentAmount + platformFee;
        
        if (msg.value < totalRequired) {
            revert InsufficientJobPayment(msg.value, totalRequired);
        }
        
        // Mint NFT to requester
        tokenId = getNextTokenId();
        _safeMint(msg.sender, tokenId);
        
        // Create job
        jobs[tokenId] = ComputationalJob({
            tokenId: tokenId,
            requester: msg.sender,
            provider: address(0),
            computationType: computationType,
            status: JobStatus.CREATED,
            cpuCores: cpuCores,
            memoryGB: memoryGB,
            storageGB: storageGB,
            gpuUnits: gpuUnits,
            estimatedDuration: estimatedDuration,
            paymentAmount: paymentAmount,
            escrowAmount: totalRequired,
            penaltyAmount: paymentAmount / 2, // 50% penalty for failure
            paymentToken: paymentToken,
            startTime: 0,
            completionTime: 0,
            resultHash: "",
            inputDataHash: inputDataHash,
            actualDuration: 0,
            qualityScore: 0,
            verificationCount: 0,
            verified: false
        });
        
        // Add to marketplace
        availableJobs.push(tokenId);
        jobsByType[computationType].push(tokenId);
        requesterJobs[msg.sender].push(tokenId);
        
        emit JobCreated(tokenId, msg.sender, computationType);
        return tokenId;
    }
    
    /**
     * @dev Register as a computing provider
     */
    function registerProvider() external payable {
        if (msg.value < minimumStakeAmount) {
            revert InsufficientStake(msg.value, minimumStakeAmount);
        }
        
        providers[msg.sender] = ProviderProfile({
            totalJobsCompleted: 0,
            totalJobsFailed: 0,
            totalComputeTime: 0,
            reputationScore: 5000, // Start with neutral reputation
            completedJobs: new uint256[](0),
            active: true,
            stakeAmount: msg.value
        });
        
        emit ProviderRegistered(msg.sender, msg.value);
    }
    
    /**
     * @dev Assign job to computing provider
     */
    function assignJob(uint256 tokenId) external {
        ComputationalJob storage job = jobs[tokenId];
        
        if (job.status != JobStatus.CREATED) {
            revert InvalidJobStatus(job.status, JobStatus.CREATED);
        }
        
        ProviderProfile storage provider = providers[msg.sender];
        if (!provider.active) {
            revert UnauthorizedProvider(msg.sender);
        }
        
        // Assign job
        job.provider = msg.sender;
        job.status = JobStatus.ASSIGNED;
        
        // Remove from available jobs
        _removeFromAvailableJobs(tokenId);
        
        // Add to provider's jobs
        providerJobs[msg.sender].push(tokenId);
        
        emit JobAssigned(tokenId, msg.sender);
    }
    
    /**
     * @dev Start job execution
     */
    function startJob(uint256 tokenId) external {
        ComputationalJob storage job = jobs[tokenId];
        
        require(job.provider == msg.sender, "ComputationalNFT: Not assigned provider");
        if (job.status != JobStatus.ASSIGNED) {
            revert InvalidJobStatus(job.status, JobStatus.ASSIGNED);
        }
        
        job.status = JobStatus.RUNNING;
        job.startTime = block.timestamp;
        
        emit JobStarted(tokenId, block.timestamp);
    }
    
    /**
     * @dev Complete job execution
     */
    function completeJob(uint256 tokenId, string calldata resultHash) external {
        ComputationalJob storage job = jobs[tokenId];
        
        require(job.provider == msg.sender, "ComputationalNFT: Not assigned provider");
        if (job.status != JobStatus.RUNNING) {
            revert InvalidJobStatus(job.status, JobStatus.RUNNING);
        }
        
        job.status = JobStatus.COMPLETED;
        job.completionTime = block.timestamp;
        job.resultHash = resultHash;
        job.actualDuration = block.timestamp - job.startTime;
        
        // Update provider stats
        ProviderProfile storage provider = providers[msg.sender];
        provider.totalJobsCompleted++;
        provider.totalComputeTime += job.actualDuration;
        provider.completedJobs.push(tokenId);
        
        // Calculate quality score based on timing accuracy
        uint256 timingAccuracy = _calculateTimingAccuracy(job.estimatedDuration, job.actualDuration);
        job.qualityScore = timingAccuracy;
        
        emit JobCompleted(tokenId, block.timestamp, resultHash);
    }
    
    /**
     * @dev Verify job result
     */
    function verifyJob(uint256 tokenId, bool approved) external {
        ComputationalJob storage job = jobs[tokenId];
        require(job.status == JobStatus.COMPLETED, "ComputationalNFT: Job not completed");
        require(!verificationVotes[tokenId][msg.sender], "ComputationalNFT: Already verified");
        
        // Add verifier
        jobVerifiers[tokenId].push(msg.sender);
        verificationVotes[tokenId][msg.sender] = approved;
        job.verificationCount++;
        
        // If majority approval and sufficient verifications, mark as verified
        if (job.verificationCount >= 3) {
            uint256 approvals = 0;
            for (uint i = 0; i < jobVerifiers[tokenId].length; i++) {
                if (verificationVotes[tokenId][jobVerifiers[tokenId][i]]) {
                    approvals++;
                }
            }
            
            if (approvals * 2 > job.verificationCount) {
                job.verified = true;
                _releasePayment(tokenId);
            }
        }
        
        emit JobVerified(tokenId, msg.sender, approved);
    }
    
    /**
     * @dev Report job failure
     */
    function reportJobFailure(uint256 tokenId, string calldata reason) external {
        ComputationalJob storage job = jobs[tokenId];
        require(
            job.provider == msg.sender || job.requester == msg.sender,
            "ComputationalNFT: Unauthorized"
        );
        
        job.status = JobStatus.FAILED;
        
        // Apply penalties and refunds
        if (job.provider == msg.sender) {
            // Provider reporting failure - apply penalty
            _applyFailurePenalty(tokenId);
        } else {
            // Requester reporting failure - initiate dispute
            job.status = JobStatus.DISPUTED;
            disputeReasons[tokenId] = reason;
            emit DisputeRaised(tokenId, reason);
        }
        
        emit JobFailed(tokenId, reason);
    }
    
    /**
     * @dev Resolve dispute (admin function)
     */
    function resolveDispute(uint256 tokenId, bool favorRequester) external onlyRole(ADMIN_ROLE) {
        ComputationalJob storage job = jobs[tokenId];
        require(job.status == JobStatus.DISPUTED, "ComputationalNFT: Not in dispute");
        
        if (favorRequester) {
            // Refund requester, penalize provider
            job.status = JobStatus.FAILED;
            _applyFailurePenalty(tokenId);
        } else {
            // Release payment to provider
            job.status = JobStatus.COMPLETED;
            job.verified = true;
            _releasePayment(tokenId);
        }
    }
    
    /**
     * @dev Get available jobs by type
     */
    function getAvailableJobsByType(ComputationType computationType) external view returns (uint256[] memory) {
        return jobsByType[computationType];
    }
    
    /**
     * @dev Get provider's job history
     */
    function getProviderJobs(address provider) external view returns (uint256[] memory) {
        return providerJobs[provider];
    }
    
    /**
     * @dev Get requester's job history
     */
    function getRequesterJobs(address requester) external view returns (uint256[] memory) {
        return requesterJobs[requester];
    }
    
    /**
     * @dev Calculate provider reputation
     */
    function calculateProviderReputation(address provider) external view returns (uint256) {
        ProviderProfile storage profile = providers[provider];
        if (profile.totalJobsCompleted == 0) return 5000; // Neutral score
        
        uint256 successRate = (profile.totalJobsCompleted * 10000) / 
                              (profile.totalJobsCompleted + profile.totalJobsFailed);
        
        // Weight by total jobs (more experience = higher weight)
        uint256 experienceWeight = profile.totalJobsCompleted > 100 ? 10000 : 
                                  profile.totalJobsCompleted * 100;
        
        return (successRate * experienceWeight) / 10000;
    }
    
    /**
     * @dev Internal function to release payment
     */
    function _releasePayment(uint256 tokenId) internal {
        ComputationalJob storage job = jobs[tokenId];
        
        // Calculate payment breakdown
        uint256 platformFee = (job.paymentAmount * platformFeeRate) / 10000;
        uint256 providerPayment = job.paymentAmount;
        
        // Transfer payments
        payable(job.provider).transfer(providerPayment);
        payable(feeRecipient).transfer(platformFee);
        
        // Update provider reputation
        ProviderProfile storage provider = providers[job.provider];
        provider.reputationScore = this.calculateProviderReputation(job.provider);
    }
    
    /**
     * @dev Internal function to apply failure penalty
     */
    function _applyFailurePenalty(uint256 tokenId) internal {
        ComputationalJob storage job = jobs[tokenId];
        
        // Refund requester (minus platform fee)
        uint256 platformFee = (job.paymentAmount * platformFeeRate) / 10000;
        uint256 refundAmount = job.escrowAmount - platformFee;
        payable(job.requester).transfer(refundAmount);
        payable(feeRecipient).transfer(platformFee);
        
        // Penalize provider's stake
        ProviderProfile storage provider = providers[job.provider];
        if (provider.stakeAmount >= job.penaltyAmount) {
            provider.stakeAmount -= job.penaltyAmount;
            provider.totalJobsFailed++;
            
            // Update reputation
            provider.reputationScore = this.calculateProviderReputation(job.provider);
        }
    }
    
    /**
     * @dev Calculate timing accuracy score
     */
    function _calculateTimingAccuracy(uint256 estimated, uint256 actual) internal pure returns (uint256) {
        if (estimated == 0) return 5000; // Neutral score if no estimate
        
        uint256 difference = estimated > actual ? estimated - actual : actual - estimated;
        uint256 accuracy = estimated > difference ? 
                          ((estimated - difference) * 10000) / estimated : 0;
        
        return accuracy;
    }
    
    /**
     * @dev Remove job from available jobs array
     */
    function _removeFromAvailableJobs(uint256 tokenId) internal {
        for (uint i = 0; i < availableJobs.length; i++) {
            if (availableJobs[i] == tokenId) {
                availableJobs[i] = availableJobs[availableJobs.length - 1];
                availableJobs.pop();
                break;
            }
        }
    }
    
    /**
     * @dev Set platform fee rate
     */
    function setPlatformFeeRate(uint256 newRate) external onlyRole(ADMIN_ROLE) {
        require(newRate <= 1000, "ComputationalNFT: Fee too high"); // Max 10%
        platformFeeRate = newRate;
    }
    
    /**
     * @dev Set minimum stake amount
     */
    function setMinimumStakeAmount(uint256 newAmount) external onlyRole(ADMIN_ROLE) {
        minimumStakeAmount = newAmount;
    }
}