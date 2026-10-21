// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "./BasicCrowdsale.sol";

/**
 * @title WhitelistCrowdsale
 * @dev Enhanced crowdsale with sophisticated whitelist and KYC features
 * 
 * Features:
 * - Multi-tier whitelist system
 * - KYC verification levels
 * - Geographic restrictions
 * - Accredited investor verification
 * - Time-based whitelist phases
 * - Referral system with rewards
 * - Anti-sybil protection
 * 
 * Use Cases:
 * - Compliant token sales
 * - Regulated fundraising
 * - Geographic compliance
 * - Accredited investor sales
 * - Community-driven allocation
 */
contract WhitelistCrowdsale is BasicCrowdsale {
    enum WhitelistTier {
        NONE,           // Not whitelisted
        BASIC,          // Basic whitelist
        PREMIUM,        // Premium tier with higher limits
        VIP,            // VIP tier with exclusive benefits
        INSTITUTIONAL   // Institutional investors
    }
    
    enum KYCLevel {
        NONE,           // No KYC
        BASIC,          // Basic identity verification
        ENHANCED,       // Enhanced due diligence
        INSTITUTIONAL   // Institutional KYC
    }
    
    struct WhitelistEntry {
        WhitelistTier tier;
        KYCLevel kycLevel;
        uint256 maxContribution;
        uint256 minContribution;
        bool isAccredited;
        bool isBlocked;
        uint256 whitelistTime;
        uint256 expiryTime;
        string jurisdiction;
        address referrer;
    }
    
    struct WhitelistPhase {
        uint256 startTime;
        uint256 endTime;
        WhitelistTier requiredTier;
        KYCLevel requiredKYC;
        uint256 phaseAllocation; // Tokens allocated for this phase
        uint256 phaseSold;
        bool isActive;
    }
    
    struct ReferralProgram {
        uint256 referrerBonus; // Percentage bonus for referrer
        uint256 refereeBonus;  // Percentage bonus for referee
        uint256 minReferrals;  // Min referrals needed for bonus
        bool isActive;
    }
    
    // Advanced whitelist management
    mapping(address => WhitelistEntry) public advancedWhitelist;
    mapping(WhitelistTier => uint256) public tierLimits;
    mapping(KYCLevel => uint256) public kycLimits;
    
    // Geographic restrictions
    mapping(string => bool) public allowedJurisdictions;
    mapping(string => bool) public blockedJurisdictions;
    string[] public allowedJurisdictionList;
    string[] public blockedJurisdictionList;
    
    // Phases
    mapping(uint256 => WhitelistPhase) public whitelistPhases;
    uint256 public currentPhase;
    uint256 public phaseCount;
    
    // Referral system
    ReferralProgram public referralProgram;
    mapping(address => address[]) public referrals;
    mapping(address => uint256) public referralRewards;
    mapping(address => uint256) public referralCount;
    
    // Anti-sybil protection
    mapping(bytes32 => bool) public usedIdentityHashes;
    mapping(address => bytes32) public identityHashes;
    uint256 public maxAddressesPerIdentity = 1;
    
    // Accredited investor requirements
    uint256 public accreditedInvestorMinContribution = 50 ether;
    bool public accreditedInvestorOnly;
    
    // Events
    event AddressWhitelisted(
        address indexed account,
        WhitelistTier tier,
        KYCLevel kycLevel,
        string jurisdiction
    );
    event AddressRemovedFromWhitelist(address indexed account);
    event KYCUpdated(address indexed account, KYCLevel oldLevel, KYCLevel newLevel);
    event PhaseStarted(uint256 indexed phase, uint256 startTime, WhitelistTier requiredTier);
    event JurisdictionUpdated(string jurisdiction, bool allowed);
    event ReferralRecorded(address indexed referrer, address indexed referee);
    event ReferralRewardClaimed(address indexed referrer, uint256 amount);
    event IdentityHashRegistered(address indexed account, bytes32 identityHash);
    
    // Errors
    error NotWhitelistedForTier();
    error InsufficientKYCLevel();
    error JurisdictionNotAllowed();
    error JurisdictionBlocked();
    error NotAccreditedInvestor();
    error PhaseNotActive();
    error PhaseAllocationExceeded();
    error ContributionExceedsTierLimit();
    error ContributionBelowTierMinimum();
    error IdentityHashAlreadyUsed();
    error MaxAddressesPerIdentityExceeded();
    error InvalidReferrer();
    
    modifier onlyWhitelistedTier(WhitelistTier requiredTier) {
        if (uint256(advancedWhitelist[msg.sender].tier) < uint256(requiredTier)) {
            revert NotWhitelistedForTier();
        }
        _;
    }
    
    modifier onlyKYCLevel(KYCLevel requiredLevel) {
        if (uint256(advancedWhitelist[msg.sender].kycLevel) < uint256(requiredLevel)) {
            revert InsufficientKYCLevel();
        }
        _;
    }
    
    modifier onlyAllowedJurisdiction() {
        string memory jurisdiction = advancedWhitelist[msg.sender].jurisdiction;
        
        if (blockedJurisdictions[jurisdiction]) {
            revert JurisdictionBlocked();
        }
        
        if (allowedJurisdictionList.length > 0 && !allowedJurisdictions[jurisdiction]) {
            revert JurisdictionNotAllowed();
        }
        _;
    }
    
    modifier onlyAccreditedIfRequired() {
        if (accreditedInvestorOnly && !advancedWhitelist[msg.sender].isAccredited) {
            revert NotAccreditedInvestor();
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
        _setupDefaultTiers();
        _setupDefaultKYCLevels();
        _setupReferralProgram();
        _setupDefaultPhases();
    }
    
    /**
     * @dev Override buyTokens with enhanced whitelist checks
     */
    function buyTokens(address beneficiary) 
        public 
        payable 
        override
        nonReentrant 
        onlyWhileOpen 
        onlyAllowedJurisdiction
        onlyAccreditedIfRequired
        whenNotPaused 
    {
        _validateWhitelistPurchase(beneficiary, msg.value);
        super.buyTokens(beneficiary);
    }
    
    /**
     * @dev Validate whitelist requirements for purchase
     */
    function _validateWhitelistPurchase(address beneficiary, uint256 weiAmount) internal view {
        WhitelistEntry storage entry = advancedWhitelist[beneficiary];
        
        // Check if address is blocked
        require(!entry.isBlocked, "WhitelistCrowdsale: Address is blocked");
        
        // Check whitelist expiry
        if (entry.expiryTime > 0 && block.timestamp > entry.expiryTime) {
            revert NotWhitelisted();
        }
        
        // Check current phase requirements
        if (phaseCount > 0) {
            WhitelistPhase storage phase = whitelistPhases[currentPhase];
            if (phase.isActive) {
                if (uint256(entry.tier) < uint256(phase.requiredTier)) {
                    revert NotWhitelistedForTier();
                }
                if (uint256(entry.kycLevel) < uint256(phase.requiredKYC)) {
                    revert InsufficientKYCLevel();
                }
                if (phase.phaseSold + (weiAmount * rate) > phase.phaseAllocation) {
                    revert PhaseAllocationExceeded();
                }
            }
        }
        
        // Check tier-specific limits
        uint256 newContribution = contributions[beneficiary] + weiAmount;
        if (entry.maxContribution > 0 && newContribution > entry.maxContribution) {
            revert ContributionExceedsTierLimit();
        }
        if (entry.minContribution > 0 && newContribution < entry.minContribution) {
            revert ContributionBelowTierMinimum();
        }
    }
    
    /**
     * @dev Add address to whitelist with full details
     */
    function addToWhitelistDetailed(
        address account,
        WhitelistTier tier,
        KYCLevel kycLevel,
        uint256 maxContribution,
        uint256 minContribution,
        bool isAccredited,
        string calldata jurisdiction,
        uint256 expiryTime,
        address referrer
    ) external onlyRole(OPERATOR_ROLE) {
        require(account != address(0), "WhitelistCrowdsale: Invalid address");
        
        // Validate referrer
        if (referrer != address(0)) {
            require(advancedWhitelist[referrer].tier != WhitelistTier.NONE, "WhitelistCrowdsale: Invalid referrer");
            _recordReferral(referrer, account);
        }
        
        advancedWhitelist[account] = WhitelistEntry({
            tier: tier,
            kycLevel: kycLevel,
            maxContribution: maxContribution,
            minContribution: minContribution,
            isAccredited: isAccredited,
            isBlocked: false,
            whitelistTime: block.timestamp,
            expiryTime: expiryTime,
            jurisdiction: jurisdiction,
            referrer: referrer
        });
        
        emit AddressWhitelisted(account, tier, kycLevel, jurisdiction);
    }
    
    /**
     * @dev Batch whitelist addresses
     */
    function batchWhitelist(
        address[] calldata accounts,
        WhitelistTier tier,
        KYCLevel kycLevel,
        string calldata jurisdiction
    ) external onlyRole(OPERATOR_ROLE) {
        for (uint256 i = 0; i < accounts.length; i++) {
            advancedWhitelist[accounts[i]] = WhitelistEntry({
                tier: tier,
                kycLevel: kycLevel,
                maxContribution: tierLimits[tier],
                minContribution: 0,
                isAccredited: false,
                isBlocked: false,
                whitelistTime: block.timestamp,
                expiryTime: 0,
                jurisdiction: jurisdiction,
                referrer: address(0)
            });
            
            emit AddressWhitelisted(accounts[i], tier, kycLevel, jurisdiction);
        }
    }
    
    /**
     * @dev Update KYC level for address
     */
    function updateKYCLevel(address account, KYCLevel newLevel) 
        external 
        onlyRole(OPERATOR_ROLE) 
    {
        KYCLevel oldLevel = advancedWhitelist[account].kycLevel;
        advancedWhitelist[account].kycLevel = newLevel;
        
        emit KYCUpdated(account, oldLevel, newLevel);
    }
    
    /**
     * @dev Block/unblock address
     */
    function setAddressBlocked(address account, bool blocked) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        advancedWhitelist[account].isBlocked = blocked;
    }
    
    /**
     * @dev Register identity hash for anti-sybil protection
     */
    function registerIdentityHash(address account, bytes32 identityHash) 
        external 
        onlyRole(OPERATOR_ROLE) 
    {
        if (usedIdentityHashes[identityHash]) {
            revert IdentityHashAlreadyUsed();
        }
        
        usedIdentityHashes[identityHash] = true;
        identityHashes[account] = identityHash;
        
        emit IdentityHashRegistered(account, identityHash);
    }
    
    /**
     * @dev Start a whitelist phase
     */
    function startWhitelistPhase(
        uint256 phaseIndex,
        uint256 startTime,
        uint256 endTime,
        WhitelistTier requiredTier,
        KYCLevel requiredKYC,
        uint256 allocation
    ) external onlyRole(ADMIN_ROLE) {
        whitelistPhases[phaseIndex] = WhitelistPhase({
            startTime: startTime,
            endTime: endTime,
            requiredTier: requiredTier,
            requiredKYC: requiredKYC,
            phaseAllocation: allocation,
            phaseSold: 0,
            isActive: true
        });
        
        if (phaseIndex >= phaseCount) {
            phaseCount = phaseIndex + 1;
        }
        
        emit PhaseStarted(phaseIndex, startTime, requiredTier);
    }
    
    /**
     * @dev Set current active phase
     */
    function setCurrentPhase(uint256 phaseIndex) external onlyRole(ADMIN_ROLE) {
        require(phaseIndex < phaseCount, "WhitelistCrowdsale: Invalid phase");
        currentPhase = phaseIndex;
    }
    
    /**
     * @dev Add allowed jurisdiction
     */
    function addAllowedJurisdiction(string calldata jurisdiction) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        if (!allowedJurisdictions[jurisdiction]) {
            allowedJurisdictions[jurisdiction] = true;
            allowedJurisdictionList.push(jurisdiction);
            emit JurisdictionUpdated(jurisdiction, true);
        }
    }
    
    /**
     * @dev Add blocked jurisdiction
     */
    function addBlockedJurisdiction(string calldata jurisdiction) 
        external 
        onlyRole(ADMIN_ROLE) 
    {
        if (!blockedJurisdictions[jurisdiction]) {
            blockedJurisdictions[jurisdiction] = true;
            blockedJurisdictionList.push(jurisdiction);
            emit JurisdictionUpdated(jurisdiction, false);
        }
    }
    
    /**
     * @dev Record referral
     */
    function _recordReferral(address referrer, address referee) internal {
        referrals[referrer].push(referee);
        referralCount[referrer]++;
        
        emit ReferralRecorded(referrer, referee);
    }
    
    /**
     * @dev Claim referral rewards
     */
    function claimReferralRewards() external nonReentrant {
        uint256 rewardAmount = calculateReferralRewards(msg.sender);
        require(rewardAmount > 0, "WhitelistCrowdsale: No rewards available");
        
        referralRewards[msg.sender] += rewardAmount;
        
        // Mint reward tokens or transfer from reserve
        token.transfer(msg.sender, rewardAmount);
        
        emit ReferralRewardClaimed(msg.sender, rewardAmount);
    }
    
    /**
     * @dev Calculate referral rewards for address
     */
    function calculateReferralRewards(address referrer) public view returns (uint256) {
        if (!referralProgram.isActive || referralCount[referrer] < referralProgram.minReferrals) {
            return 0;
        }
        
        uint256 totalRefereeContributions = 0;
        address[] memory userReferrals = referrals[referrer];
        
        for (uint256 i = 0; i < userReferrals.length; i++) {
            totalRefereeContributions += contributions[userReferrals[i]];
        }
        
        uint256 refereeTokens = totalRefereeContributions * rate;
        return refereeTokens * referralProgram.referrerBonus / 100;
    }
    
    /**
     * @dev Get whitelist info for address
     */
    function getWhitelistInfo(address account) external view returns (
        WhitelistTier tier,
        KYCLevel kycLevel,
        uint256 maxContribution,
        uint256 minContribution,
        bool isAccredited,
        bool isBlocked,
        string memory jurisdiction,
        address referrer,
        uint256 referralCount_
    ) {
        WhitelistEntry storage entry = advancedWhitelist[account];
        
        return (
            entry.tier,
            entry.kycLevel,
            entry.maxContribution,
            entry.minContribution,
            entry.isAccredited,
            entry.isBlocked,
            entry.jurisdiction,
            entry.referrer,
            referralCount[account]
        );
    }
    
    /**
     * @dev Get current phase info
     */
    function getCurrentPhaseInfo() external view returns (
        uint256 phaseIndex,
        uint256 startTime,
        uint256 endTime,
        WhitelistTier requiredTier,
        KYCLevel requiredKYC,
        uint256 allocation,
        uint256 sold,
        bool isActive
    ) {
        if (phaseCount == 0) {
            return (0, 0, 0, WhitelistTier.NONE, KYCLevel.NONE, 0, 0, false);
        }
        
        WhitelistPhase storage phase = whitelistPhases[currentPhase];
        
        return (
            currentPhase,
            phase.startTime,
            phase.endTime,
            phase.requiredTier,
            phase.requiredKYC,
            phase.phaseAllocation,
            phase.phaseSold,
            phase.isActive
        );
    }
    
    // Private setup functions
    
    function _setupDefaultTiers() private {
        tierLimits[WhitelistTier.BASIC] = 1 ether;
        tierLimits[WhitelistTier.PREMIUM] = 10 ether;
        tierLimits[WhitelistTier.VIP] = 50 ether;
        tierLimits[WhitelistTier.INSTITUTIONAL] = 1000 ether;
    }
    
    function _setupDefaultKYCLevels() private {
        kycLimits[KYCLevel.BASIC] = 1 ether;
        kycLimits[KYCLevel.ENHANCED] = 10 ether;
        kycLimits[KYCLevel.INSTITUTIONAL] = 1000 ether;
    }
    
    function _setupReferralProgram() private {
        referralProgram = ReferralProgram({
            referrerBonus: 5, // 5% bonus for referrer
            refereeBonus: 2,  // 2% bonus for referee
            minReferrals: 3,  // Need 3 referrals to qualify
            isActive: true
        });
    }
    
    function _setupDefaultPhases() private {
        // Phase 0: VIP pre-sale
        whitelistPhases[0] = WhitelistPhase({
            startTime: startTime,
            endTime: startTime + 3 days,
            requiredTier: WhitelistTier.VIP,
            requiredKYC: KYCLevel.ENHANCED,
            phaseAllocation: hardCap / 4, // 25% allocation
            phaseSold: 0,
            isActive: false
        });
        
        // Phase 1: Premium whitelist
        whitelistPhases[1] = WhitelistPhase({
            startTime: startTime + 3 days,
            endTime: startTime + 7 days,
            requiredTier: WhitelistTier.PREMIUM,
            requiredKYC: KYCLevel.BASIC,
            phaseAllocation: hardCap / 2, // 50% allocation
            phaseSold: 0,
            isActive: false
        });
        
        // Phase 2: Public whitelist
        whitelistPhases[2] = WhitelistPhase({
            startTime: startTime + 7 days,
            endTime: endTime,
            requiredTier: WhitelistTier.BASIC,
            requiredKYC: KYCLevel.BASIC,
            phaseAllocation: hardCap / 4, // 25% allocation
            phaseSold: 0,
            isActive: false
        });
        
        phaseCount = 3;
    }
}