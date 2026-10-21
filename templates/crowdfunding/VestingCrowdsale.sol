// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BasicCrowdsale.sol";

/**
 * @title VestingCrowdsale
 * @dev Advanced crowdsale with sophisticated vesting schedules
 * 
 * Features:
 * - Multiple vesting schedules (linear, cliff, step)
 * - Per-investor custom vesting
 * - Vesting schedule templates
 * - Early bird bonuses with extended vesting
 * - Team/advisor allocation with different vesting
 * - Revocable vesting for team members
 * 
 * Use Cases:
 * - Token sales with investor protection
 * - Team token allocation
 * - Advisor compensation
 * - Long-term ecosystem building
 */
contract VestingCrowdsale is BasicCrowdsale {
    enum VestingType {
        LINEAR,     // Gradual unlock over time
        CLIFF,      // Unlock after cliff period
        STEP,       // Unlock in predefined steps
        IMMEDIATE   // Immediate unlock (no vesting)
    }
    
    struct VestingSchedule {
        VestingType vestingType;
        uint256 startTime;
        uint256 cliffDuration;     // Time before any tokens unlock
        uint256 vestingDuration;   // Total vesting period
        uint256 stepCount;         // For step vesting
        bool revocable;            // Can be revoked by admin
        bool revoked;              // Has been revoked
    }
    
    // Vesting schedules
    mapping(address => VestingSchedule) public vestingSchedules;
    mapping(string => VestingSchedule) public vestingTemplates;
    
    // Tier-based vesting (based on contribution amount)
    struct VestingTier {
        uint256 minContribution;
        VestingType vestingType;
        uint256 cliffDuration;
        uint256 vestingDuration;
        uint256 bonusPercentage; // Bonus tokens for this tier
    }
    mapping(uint256 => VestingTier) public vestingTiers;
    uint256 public tierCount;
    
    // Early bird bonuses
    struct EarlyBirdPhase {
        uint256 endTime;
        uint256 bonusPercentage;
        uint256 additionalVestingDuration; // Additional vesting for bonus
    }
    mapping(uint256 => EarlyBirdPhase) public earlyBirdPhases;
    uint256 public earlyBirdPhaseCount;
    
    // Team and advisor allocations
    mapping(address => uint256) public teamAllocation;
    mapping(address => uint256) public advisorAllocation;
    mapping(address => bool) public teamMember;
    mapping(address => bool) public advisor;
    
    // Events
    event VestingScheduleCreated(
        address indexed beneficiary,
        VestingType vestingType,
        uint256 startTime,
        uint256 cliffDuration,
        uint256 vestingDuration
    );
    event VestingRevoked(address indexed beneficiary, uint256 unvestedAmount);
    event VestingTemplateAdded(string name, VestingType vestingType);
    event EarlyBirdPhaseAdded(uint256 phase, uint256 endTime, uint256 bonus);
    event TeamAllocationSet(address indexed member, uint256 amount);
    event AdvisorAllocationSet(address indexed advisor, uint256 amount);
    
    // Errors
    error InvalidVestingType();
    error VestingAlreadyRevoked();
    error NotRevocable();
    error InvalidTierConfiguration();
    error TemplateNotFound();
    
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
        _setupDefaultTemplates();
        _setupDefaultTiers();
        _setupEarlyBirdPhases();
    }
    
    /**
     * @dev Override to apply tier-based vesting
     */
    function _validateAndProcessPurchase(
        address beneficiary,
        uint256 weiAmount,
        uint256 tokenAmount
    ) internal override {
        super._validateAndProcessPurchase(beneficiary, weiAmount, tokenAmount);
        
        // Apply tier-based vesting and bonuses
        _applyTierVesting(beneficiary, weiAmount, tokenAmount);
        
        // Apply early bird bonuses
        _applyEarlyBirdBonus(beneficiary, tokenAmount);
    }
    
    /**
     * @dev Apply tier-based vesting and bonuses
     */
    function _applyTierVesting(
        address beneficiary,
        uint256 weiAmount,
        uint256 tokenAmount
    ) internal {
        uint256 totalContribution = contributions[beneficiary];
        
        // Find applicable tier
        for (uint256 i = tierCount; i > 0; i--) {
            VestingTier storage tier = vestingTiers[i - 1];
            if (totalContribution >= tier.minContribution) {
                // Apply tier vesting if not already set
                if (vestingSchedules[beneficiary].vestingType == VestingType.LINEAR) {
                    _setVestingSchedule(
                        beneficiary,
                        tier.vestingType,
                        endTime, // Start after sale
                        tier.cliffDuration,
                        tier.vestingDuration,
                        false // Not revocable for investors
                    );
                }
                
                // Apply tier bonus
                if (tier.bonusPercentage > 0) {
                    uint256 bonusTokens = tokenAmount * tier.bonusPercentage / 100;
                    tokenBalances[beneficiary] += bonusTokens;
                    tokensSold += bonusTokens;
                }
                break;
            }
        }
    }
    
    /**
     * @dev Apply early bird bonuses
     */
    function _applyEarlyBirdBonus(address beneficiary, uint256 tokenAmount) internal {
        for (uint256 i = 0; i < earlyBirdPhaseCount; i++) {
            EarlyBirdPhase storage phase = earlyBirdPhases[i];
            if (block.timestamp <= phase.endTime) {
                uint256 bonusTokens = tokenAmount * phase.bonusPercentage / 100;
                
                if (bonusTokens > 0) {
                    tokenBalances[beneficiary] += bonusTokens;
                    tokensSold += bonusTokens;
                    
                    // Extend vesting for bonus tokens
                    VestingSchedule storage schedule = vestingSchedules[beneficiary];
                    schedule.vestingDuration += phase.additionalVestingDuration;
                }
                break;
            }
        }
    }
    
    /**
     * @dev Set custom vesting schedule for beneficiary
     */
    function setVestingSchedule(
        address beneficiary,
        VestingType vestingType,
        uint256 startTime,
        uint256 cliffDuration,
        uint256 vestingDuration,
        bool revocable
    ) external onlyRole(ADMIN_ROLE) {
        _setVestingSchedule(
            beneficiary,
            vestingType,
            startTime,
            cliffDuration,
            vestingDuration,
            revocable
        );
    }
    
    /**
     * @dev Internal function to set vesting schedule
     */
    function _setVestingSchedule(
        address beneficiary,
        VestingType vestingType,
        uint256 startTime,
        uint256 cliffDuration,
        uint256 vestingDuration,
        bool revocable
    ) internal {
        vestingSchedules[beneficiary] = VestingSchedule({
            vestingType: vestingType,
            startTime: startTime,
            cliffDuration: cliffDuration,
            vestingDuration: vestingDuration,
            stepCount: vestingType == VestingType.STEP ? 12 : 0, // Default monthly steps
            revocable: revocable,
            revoked: false
        });
        
        emit VestingScheduleCreated(
            beneficiary,
            vestingType,
            startTime,
            cliffDuration,
            vestingDuration
        );
    }
    
    /**
     * @dev Apply vesting template to beneficiary
     */
    function applyVestingTemplate(
        address beneficiary,
        string calldata templateName,
        uint256 startTime
    ) external onlyRole(ADMIN_ROLE) {
        VestingSchedule storage template = vestingTemplates[templateName];
        if (template.vestingType == VestingType.LINEAR && template.vestingDuration == 0) {
            revert TemplateNotFound();
        }
        
        _setVestingSchedule(
            beneficiary,
            template.vestingType,
            startTime,
            template.cliffDuration,
            template.vestingDuration,
            template.revocable
        );
    }
    
    /**
     * @dev Override getVestedAmount to support different vesting types
     */
    function getVestedAmount(address beneficiary) public view override returns (uint256) {
        VestingSchedule storage schedule = vestingSchedules[beneficiary];
        
        if (schedule.revoked) {
            return vestingClaimed[beneficiary]; // No additional vesting if revoked
        }
        
        uint256 totalTokens = tokenBalances[beneficiary];
        uint256 start = schedule.startTime;
        uint256 cliff = schedule.cliffDuration;
        uint256 duration = schedule.vestingDuration;
        
        if (block.timestamp < start + cliff) {
            return 0; // Before cliff
        }
        
        if (schedule.vestingType == VestingType.IMMEDIATE) {
            return totalTokens;
        }
        
        if (block.timestamp >= start + duration) {
            return totalTokens; // Fully vested
        }
        
        if (schedule.vestingType == VestingType.LINEAR) {
            uint256 elapsed = block.timestamp - (start + cliff);
            uint256 vestingPeriod = duration - cliff;
            return totalTokens * elapsed / vestingPeriod;
        }
        
        if (schedule.vestingType == VestingType.STEP) {
            uint256 elapsed = block.timestamp - (start + cliff);
            uint256 vestingPeriod = duration - cliff;
            uint256 stepDuration = vestingPeriod / schedule.stepCount;
            uint256 completedSteps = elapsed / stepDuration;
            return totalTokens * completedSteps / schedule.stepCount;
        }
        
        if (schedule.vestingType == VestingType.CLIFF) {
            return 0; // Only unlocks at the end
        }
        
        return 0;
    }
    
    /**
     * @dev Revoke vesting for team member
     */
    function revokeVesting(address beneficiary) external onlyRole(ADMIN_ROLE) {
        VestingSchedule storage schedule = vestingSchedules[beneficiary];
        
        if (!schedule.revocable) {
            revert NotRevocable();
        }
        
        if (schedule.revoked) {
            revert VestingAlreadyRevoked();
        }
        
        schedule.revoked = true;
        
        uint256 vestedAmount = getVestedAmount(beneficiary);
        uint256 unvestedAmount = tokenBalances[beneficiary] - vestedAmount;
        
        // Reduce their balance to only vested amount
        tokenBalances[beneficiary] = vestedAmount;
        
        emit VestingRevoked(beneficiary, unvestedAmount);
    }
    
    /**
     * @dev Set team member allocation
     */
    function setTeamAllocation(
        address member,
        uint256 amount,
        uint256 vestingStart,
        uint256 cliffDuration,
        uint256 vestingDuration
    ) external onlyRole(ADMIN_ROLE) {
        teamMember[member] = true;
        teamAllocation[member] = amount;
        tokenBalances[member] += amount;
        
        _setVestingSchedule(
            member,
            VestingType.LINEAR,
            vestingStart,
            cliffDuration,
            vestingDuration,
            true // Revocable for team
        );
        
        emit TeamAllocationSet(member, amount);
    }
    
    /**
     * @dev Set advisor allocation
     */
    function setAdvisorAllocation(
        address advisorAddress,
        uint256 amount,
        uint256 vestingStart,
        uint256 cliffDuration,
        uint256 vestingDuration
    ) external onlyRole(ADMIN_ROLE) {
        advisor[advisorAddress] = true;
        advisorAllocation[advisorAddress] = amount;
        tokenBalances[advisorAddress] += amount;
        
        _setVestingSchedule(
            advisorAddress,
            VestingType.LINEAR,
            vestingStart,
            cliffDuration,
            vestingDuration,
            true // Revocable for advisors
        );
        
        emit AdvisorAllocationSet(advisorAddress, amount);
    }
    
    /**
     * @dev Add vesting template
     */
    function addVestingTemplate(
        string calldata name,
        VestingType vestingType,
        uint256 cliffDuration,
        uint256 vestingDuration,
        bool revocable
    ) external onlyRole(ADMIN_ROLE) {
        vestingTemplates[name] = VestingSchedule({
            vestingType: vestingType,
            startTime: 0, // Set when applied
            cliffDuration: cliffDuration,
            vestingDuration: vestingDuration,
            stepCount: vestingType == VestingType.STEP ? 12 : 0,
            revocable: revocable,
            revoked: false
        });
        
        emit VestingTemplateAdded(name, vestingType);
    }
    
    /**
     * @dev Add vesting tier
     */
    function addVestingTier(
        uint256 minContribution,
        VestingType vestingType,
        uint256 cliffDuration,
        uint256 vestingDuration,
        uint256 bonusPercentage
    ) external onlyRole(ADMIN_ROLE) {
        vestingTiers[tierCount] = VestingTier({
            minContribution: minContribution,
            vestingType: vestingType,
            cliffDuration: cliffDuration,
            vestingDuration: vestingDuration,
            bonusPercentage: bonusPercentage
        });
        tierCount++;
    }
    
    /**
     * @dev Get detailed vesting info for address
     */
    function getVestingInfo(address beneficiary) external view returns (
        VestingType vestingType,
        uint256 startTime,
        uint256 cliffDuration,
        uint256 vestingDuration,
        uint256 totalTokens,
        uint256 vestedTokens,
        uint256 claimedTokens,
        bool isRevocable,
        bool isRevoked
    ) {
        VestingSchedule storage schedule = vestingSchedules[beneficiary];
        
        return (
            schedule.vestingType,
            schedule.startTime,
            schedule.cliffDuration,
            schedule.vestingDuration,
            tokenBalances[beneficiary],
            getVestedAmount(beneficiary),
            vestingClaimed[beneficiary],
            schedule.revocable,
            schedule.revoked
        );
    }
    
    // Private setup functions
    
    function _setupDefaultTemplates() private {
        // Investor template: 6 month cliff, 24 month vesting
        vestingTemplates["investor"] = VestingSchedule({
            vestingType: VestingType.LINEAR,
            startTime: 0,
            cliffDuration: 180 days,
            vestingDuration: 720 days,
            stepCount: 0,
            revocable: false,
            revoked: false
        });
        
        // Team template: 12 month cliff, 36 month vesting
        vestingTemplates["team"] = VestingSchedule({
            vestingType: VestingType.LINEAR,
            startTime: 0,
            cliffDuration: 365 days,
            vestingDuration: 1095 days,
            stepCount: 0,
            revocable: true,
            revoked: false
        });
        
        // Advisor template: 6 month cliff, 18 month vesting
        vestingTemplates["advisor"] = VestingSchedule({
            vestingType: VestingType.LINEAR,
            startTime: 0,
            cliffDuration: 180 days,
            vestingDuration: 540 days,
            stepCount: 0,
            revocable: true,
            revoked: false
        });
    }
    
    function _setupDefaultTiers() private {
        // Tier 1: Small investors
        vestingTiers[0] = VestingTier({
            minContribution: 0.1 ether,
            vestingType: VestingType.LINEAR,
            cliffDuration: 90 days,
            vestingDuration: 360 days,
            bonusPercentage: 0
        });
        
        // Tier 2: Medium investors
        vestingTiers[1] = VestingTier({
            minContribution: 1 ether,
            vestingType: VestingType.LINEAR,
            cliffDuration: 180 days,
            vestingDuration: 540 days,
            bonusPercentage: 5
        });
        
        // Tier 3: Large investors
        vestingTiers[2] = VestingTier({
            minContribution: 10 ether,
            vestingType: VestingType.LINEAR,
            cliffDuration: 180 days,
            vestingDuration: 720 days,
            bonusPercentage: 10
        });
        
        tierCount = 3;
    }
    
    function _setupEarlyBirdPhases() private {
        // Phase 1: First 24 hours, 20% bonus
        earlyBirdPhases[0] = EarlyBirdPhase({
            endTime: startTime + 1 days,
            bonusPercentage: 20,
            additionalVestingDuration: 180 days
        });
        
        // Phase 2: First week, 10% bonus
        earlyBirdPhases[1] = EarlyBirdPhase({
            endTime: startTime + 7 days,
            bonusPercentage: 10,
            additionalVestingDuration: 90 days
        });
        
        earlyBirdPhaseCount = 2;
    }
}