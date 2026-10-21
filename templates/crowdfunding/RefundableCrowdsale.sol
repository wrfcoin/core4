// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BasicCrowdsale.sol";

/**
 * @title RefundableCrowdsale
 * @dev Crowdsale with refund mechanisms and escrow functionality
 * 
 * Features:
 * - Escrow-based fund management
 * - Automatic refunds if soft cap not reached
 * - Partial refunds with prorated distribution
 * - Milestone-based fund release
 * - Refund insurance for investor protection
 * - Emergency refund mechanisms
 * 
 * Use Cases:
 * - Risk-averse token sales
 * - Milestone-driven funding
 * - Investor protection mechanisms
 * - Regulatory compliance
 */
contract RefundableCrowdsale is BasicCrowdsale {
    enum RefundState {
        ACTIVE,         // Crowdsale active, no refunds
        REFUNDING,      // Refunds enabled (soft cap not met)
        CLOSED          // Crowdsale successful, funds released
    }
    
    enum EscrowState {
        LOCKED,         // Funds locked in escrow
        RELEASED,       // Funds released to beneficiary
        REFUNDING       // Funds available for refund
    }
    
    struct Milestone {
        string description;
        uint256 fundingPercentage; // Percentage of funds to release
        uint256 deadline;
        bool completed;
        bool votingEnabled;
        uint256 votesFor;
        uint256 votesAgainst;
        uint256 votingDeadline;
    }
    
    struct RefundInsurance {
        bool enabled;
        uint256 insuranceRate; // Percentage of contribution
        uint256 maxCoverage;   // Maximum coverage per investor
        address insuranceProvider;
        uint256 totalCoverage;
    }
    
    // Refund and escrow state
    RefundState public refundState;
    EscrowState public escrowState;
    mapping(address => bool) public refundClaimed;
    
    // Escrow management
    uint256 public escrowBalance;
    uint256 public releasedFunds;
    mapping(address => uint256) public escrowContributions;
    
    // Milestone management
    mapping(uint256 => Milestone) public milestones;
    uint256 public milestoneCount;
    uint256 public currentMilestone;
    mapping(uint256 => mapping(address => bool)) public milestoneVotes;
    
    // Refund insurance
    RefundInsurance public refundInsurance;
    mapping(address => uint256) public insuranceCoverage;
    mapping(address => bool) public insuranceClaimed;
    
    // Time-based refunds
    uint256 public refundGracePeriod = 7 days;
    uint256 public refundDeadline;
    bool public emergencyRefundEnabled;
    
    // Partial refund mechanisms
    bool public partialRefundsEnabled;
    uint256 public partialRefundPercentage; // If between soft and hard cap
    
    // Events
    event RefundStateChanged(RefundState oldState, RefundState newState);
    event EscrowStateChanged(EscrowState oldState, EscrowState newState);
    event RefundProcessed(address indexed beneficiary, uint256 amount);
    event FundsReleased(uint256 amount, uint256 milestone);
    event MilestoneAdded(uint256 indexed milestoneId, string description, uint256 deadline);
    event MilestoneCompleted(uint256 indexed milestoneId, uint256 releasedAmount);
    event MilestoneVote(uint256 indexed milestoneId, address indexed voter, bool support);
    event InsuranceClaimProcessed(address indexed beneficiary, uint256 amount);
    event EmergencyRefundEnabled(uint256 timestamp);
    event PartialRefundEnabled(uint256 percentage);
    
    // Errors
    error RefundNotAvailable();
    error RefundAlreadyClaimed();
    error FundsAlreadyReleased();
    error MilestoneNotReached();
    error MilestoneVotingClosed();
    error AlreadyVoted();
    error InsufficientEscrowBalance();
    error InsuranceNotAvailable();
    error EmergencyRefundNotEnabled();
    error PartialRefundNotEnabled();
    
    modifier onlyRefunding() {
        if (refundState != RefundState.REFUNDING) {
            revert RefundNotAvailable();
        }
        _;
    }
    
    modifier onlyIfRefundAvailable() {
        if (refundState == RefundState.CLOSED) {
            revert RefundNotAvailable();
        }
        _;
    }
    
    constructor(
        address _token,
        uint256 _startTime,
        uint256 _endTime,
        uint256 _rate,
        uint256 _hardCap,
        uint256 _softCap,
        uint256 _minContribution,
        uint256 _maxContribution,
        address payable _wallet,
        address initialOwner
    ) BasicCrowdsale(
        _token,
        _startTime,
        _endTime,
        _rate,
        _hardCap,
        _softCap,
        _minContribution,
        _maxContribution,
        _wallet,
        initialOwner
    ) {
        refundState = RefundState.ACTIVE;
        escrowState = EscrowState.LOCKED;
        refundDeadline = _endTime + refundGracePeriod;
        
        _setupDefaultMilestones();
        _setupRefundInsurance();
    }
    
    /**
     * @dev Override to handle escrow deposits
     */
    function _validateAndProcessPurchase(
        address beneficiary,
        uint256 weiAmount,
        uint256 tokenAmount
    ) internal override {
        super._validateAndProcessPurchase(beneficiary, weiAmount, tokenAmount);
        
        // Add to escrow
        escrowBalance += weiAmount;
        escrowContributions[beneficiary] += weiAmount;
        
        // Handle insurance if enabled
        if (refundInsurance.enabled) {
            _processInsurance(beneficiary, weiAmount);
        }
    }
    
    /**
     * @dev Override finalize to handle refund state
     */
    function finalize() external override onlyRole(ADMIN_ROLE) {
        if (block.timestamp <= endTime && weiRaised < hardCap) {
            revert SaleNotActive();
        }
        if (finalized) {
            revert AlreadyFinalized();
        }
        
        finalized = true;
        
        if (softCapReached) {
            _enableFundRelease();
        } else {
            _enableRefunds();
        }
        
        emit CrowdsaleFinalized(weiRaised, tokensSold);
    }
    
    /**
     * @dev Enable refunds (called when soft cap not reached)
     */
    function _enableRefunds() internal {
        RefundState oldState = refundState;
        refundState = RefundState.REFUNDING;
        escrowState = EscrowState.REFUNDING;
        
        emit RefundStateChanged(oldState, refundState);
        emit EscrowStateChanged(EscrowState.LOCKED, escrowState);
    }
    
    /**
     * @dev Enable fund release (called when soft cap reached)
     */
    function _enableFundRelease() internal {
        RefundState oldState = refundState;
        refundState = RefundState.CLOSED;
        
        emit RefundStateChanged(oldState, refundState);
    }
    
    /**
     * @dev Claim full refund
     */
    function claimRefund() external override nonReentrant onlyIfRefundAvailable {
        require(contributions[msg.sender] > 0, "RefundableCrowdsale: No contribution to refund");
        require(!refundClaimed[msg.sender], "RefundableCrowdsale: Refund already claimed");
        
        uint256 refundAmount = 0;
        
        if (refundState == RefundState.REFUNDING) {
            // Full refund if soft cap not reached
            refundAmount = contributions[msg.sender];
        } else if (partialRefundsEnabled && !softCapReached) {
            // Partial refund based on percentage
            refundAmount = contributions[msg.sender] * partialRefundPercentage / 100;
        } else if (emergencyRefundEnabled) {
            // Emergency refund
            refundAmount = contributions[msg.sender];
        } else {
            revert RefundNotAvailable();
        }
        
        require(refundAmount > 0, "RefundableCrowdsale: No refund available");
        require(address(this).balance >= refundAmount, "RefundableCrowdsale: Insufficient balance");
        
        refundClaimed[msg.sender] = true;
        escrowBalance -= refundAmount;
        
        payable(msg.sender).transfer(refundAmount);
        
        emit RefundProcessed(msg.sender, refundAmount);
    }
    
    /**
     * @dev Claim insurance if available
     */
    function claimInsurance() external nonReentrant {
        if (!refundInsurance.enabled) {
            revert InsuranceNotAvailable();
        }
        
        require(refundState == RefundState.REFUNDING, "RefundableCrowdsale: Insurance not available");
        require(!insuranceClaimed[msg.sender], "RefundableCrowdsale: Insurance already claimed");
        require(insuranceCoverage[msg.sender] > 0, "RefundableCrowdsale: No insurance coverage");
        
        uint256 coverageAmount = insuranceCoverage[msg.sender];
        insuranceClaimed[msg.sender] = true;
        
        // Transfer from insurance provider (simplified)
        payable(msg.sender).transfer(coverageAmount);
        
        emit InsuranceClaimProcessed(msg.sender, coverageAmount);
    }
    
    /**
     * @dev Release funds for completed milestone
     */
    function releaseMilestoneFunds(uint256 milestoneId) 
        external 
        onlyRole(ADMIN_ROLE) 
        nonReentrant 
    {
        require(milestoneId < milestoneCount, "RefundableCrowdsale: Invalid milestone");
        require(refundState == RefundState.CLOSED, "RefundableCrowdsale: Cannot release funds");
        require(escrowState == EscrowState.LOCKED, "RefundableCrowdsale: Funds already released");
        
        Milestone storage milestone = milestones[milestoneId];
        require(milestone.completed, "RefundableCrowdsale: Milestone not completed");
        require(block.timestamp >= milestone.deadline || _milestoneVotePassed(milestoneId), "RefundableCrowdsale: Milestone conditions not met");
        
        uint256 releaseAmount = escrowBalance * milestone.fundingPercentage / 100;
        require(releaseAmount > 0, "RefundableCrowdsale: No funds to release");
        
        releasedFunds += releaseAmount;
        escrowBalance -= releaseAmount;
        
        wallet.transfer(releaseAmount);
        
        emit FundsReleased(releaseAmount, milestoneId);
        emit MilestoneCompleted(milestoneId, releaseAmount);
        
        // Check if all funds released
        if (escrowBalance == 0) {
            escrowState = EscrowState.RELEASED;
            emit EscrowStateChanged(EscrowState.LOCKED, EscrowState.RELEASED);
        }
    }
    
    /**
     * @dev Add a milestone
     */
    function addMilestone(
        string calldata description,
        uint256 fundingPercentage,
        uint256 deadline,
        bool votingEnabled,
        uint256 votingDuration
    ) external onlyRole(ADMIN_ROLE) {
        milestones[milestoneCount] = Milestone({
            description: description,
            fundingPercentage: fundingPercentage,
            deadline: deadline,
            completed: false,
            votingEnabled: votingEnabled,
            votesFor: 0,
            votesAgainst: 0,
            votingDeadline: votingEnabled ? deadline + votingDuration : 0
        });
        
        emit MilestoneAdded(milestoneCount, description, deadline);
        milestoneCount++;
    }
    
    /**
     * @dev Mark milestone as completed
     */
    function completeMilestone(uint256 milestoneId) external onlyRole(ADMIN_ROLE) {
        require(milestoneId < milestoneCount, "RefundableCrowdsale: Invalid milestone");
        milestones[milestoneId].completed = true;
    }
    
    /**
     * @dev Vote on milestone completion
     */
    function voteOnMilestone(uint256 milestoneId, bool support) external {
        require(milestoneId < milestoneCount, "RefundableCrowdsale: Invalid milestone");
        require(contributions[msg.sender] > 0, "RefundableCrowdsale: No voting power");
        require(!milestoneVotes[milestoneId][msg.sender], "RefundableCrowdsale: Already voted");
        
        Milestone storage milestone = milestones[milestoneId];
        require(milestone.votingEnabled, "RefundableCrowdsale: Voting not enabled");
        require(block.timestamp <= milestone.votingDeadline, "RefundableCrowdsale: Voting period ended");
        
        milestoneVotes[milestoneId][msg.sender] = true;
        
        uint256 votingPower = contributions[msg.sender];
        if (support) {
            milestone.votesFor += votingPower;
        } else {
            milestone.votesAgainst += votingPower;
        }
        
        emit MilestoneVote(milestoneId, msg.sender, support);
    }
    
    /**
     * @dev Enable emergency refunds
     */
    function enableEmergencyRefund() external onlyRole(EMERGENCY_ROLE) {
        emergencyRefundEnabled = true;
        emit EmergencyRefundEnabled(block.timestamp);
    }
    
    /**
     * @dev Enable partial refunds
     */
    function enablePartialRefunds(uint256 percentage) external onlyRole(ADMIN_ROLE) {
        require(percentage <= 100, "RefundableCrowdsale: Invalid percentage");
        partialRefundsEnabled = true;
        partialRefundPercentage = percentage;
        emit PartialRefundEnabled(percentage);
    }
    
    /**
     * @dev Setup refund insurance
     */
    function setupRefundInsurance(
        uint256 rate,
        uint256 maxCoverage,
        address provider
    ) external onlyRole(ADMIN_ROLE) {
        refundInsurance = RefundInsurance({
            enabled: true,
            insuranceRate: rate,
            maxCoverage: maxCoverage,
            insuranceProvider: provider,
            totalCoverage: 0
        });
    }
    
    /**
     * @dev Process insurance for contribution
     */
    function _processInsurance(address beneficiary, uint256 contribution) internal {
        if (!refundInsurance.enabled) return;
        
        uint256 coverage = contribution * refundInsurance.insuranceRate / 100;
        if (coverage > refundInsurance.maxCoverage) {
            coverage = refundInsurance.maxCoverage;
        }
        
        insuranceCoverage[beneficiary] += coverage;
        refundInsurance.totalCoverage += coverage;
    }
    
    /**
     * @dev Check if milestone vote passed
     */
    function _milestoneVotePassed(uint256 milestoneId) internal view returns (bool) {
        Milestone storage milestone = milestones[milestoneId];
        if (!milestone.votingEnabled) return true;
        
        uint256 totalVotes = milestone.votesFor + milestone.votesAgainst;
        if (totalVotes == 0) return false;
        
        return milestone.votesFor > milestone.votesAgainst;
    }
    
    /**
     * @dev Get refund info for address
     */
    function getRefundInfo(address account) external view returns (
        uint256 contribution,
        uint256 availableRefund,
        bool refundClaimedStatus,
        uint256 insuranceCoverageAmount,
        bool insuranceClaimedStatus,
        RefundState currentRefundState
    ) {
        uint256 availableAmount = 0;
        
        if (refundState == RefundState.REFUNDING) {
            availableAmount = contributions[account];
        } else if (partialRefundsEnabled) {
            availableAmount = contributions[account] * partialRefundPercentage / 100;
        } else if (emergencyRefundEnabled) {
            availableAmount = contributions[account];
        }
        
        return (
            contributions[account],
            availableAmount,
            refundClaimed[account],
            insuranceCoverage[account],
            insuranceClaimed[account],
            refundState
        );
    }
    
    /**
     * @dev Get milestone info
     */
    function getMilestoneInfo(uint256 milestoneId) external view returns (
        string memory description,
        uint256 fundingPercentage,
        uint256 deadline,
        bool completed,
        bool votingEnabled,
        uint256 votesFor,
        uint256 votesAgainst,
        uint256 votingDeadline
    ) {
        require(milestoneId < milestoneCount, "RefundableCrowdsale: Invalid milestone");
        Milestone storage milestone = milestones[milestoneId];
        
        return (
            milestone.description,
            milestone.fundingPercentage,
            milestone.deadline,
            milestone.completed,
            milestone.votingEnabled,
            milestone.votesFor,
            milestone.votesAgainst,
            milestone.votingDeadline
        );
    }
    
    /**
     * @dev Get escrow info
     */
    function getEscrowInfo() external view returns (
        uint256 totalEscrow,
        uint256 totalReleased,
        uint256 availableForRelease,
        EscrowState currentEscrowState
    ) {
        return (
            escrowBalance + releasedFunds,
            releasedFunds,
            escrowBalance,
            escrowState
        );
    }
    
    // Private setup functions
    
    function _setupDefaultMilestones() private {
        // Milestone 1: Project kickoff (25% of funds)
        milestones[0] = Milestone({
            description: "Project development kickoff",
            fundingPercentage: 25,
            deadline: endTime + 30 days,
            completed: false,
            votingEnabled: true,
            votesFor: 0,
            votesAgainst: 0,
            votingDeadline: endTime + 37 days
        });
        
        // Milestone 2: Alpha release (35% of funds)
        milestones[1] = Milestone({
            description: "Alpha version release",
            fundingPercentage: 35,
            deadline: endTime + 90 days,
            completed: false,
            votingEnabled: true,
            votesFor: 0,
            votesAgainst: 0,
            votingDeadline: endTime + 97 days
        });
        
        // Milestone 3: Beta release (40% of funds)
        milestones[2] = Milestone({
            description: "Beta version and mainnet launch",
            fundingPercentage: 40,
            deadline: endTime + 180 days,
            completed: false,
            votingEnabled: true,
            votesFor: 0,
            votesAgainst: 0,
            votingDeadline: endTime + 187 days
        });
        
        milestoneCount = 3;
    }
    
    function _setupRefundInsurance() private {
        refundInsurance = RefundInsurance({
            enabled: false,
            insuranceRate: 5, // 5% of contribution
            maxCoverage: 1 ether,
            insuranceProvider: address(0),
            totalCoverage: 0
        });
    }
}