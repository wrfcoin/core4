// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";

/**
 * @title CarbonFootprintTracker
 * @dev Personal and corporate carbon footprint tracking system
 * 
 * Features:
 * - Individual carbon footprint tracking
 * - Corporate emission reporting
 * - Activity-based carbon calculation
 * - Offset recommendation and tracking
 * - Emission reduction goals and progress
 * - Carbon intensity benchmarking
 * - Automated offset purchasing
 * 
 * Use Cases:
 * - Personal carbon tracking
 * - Corporate ESG reporting
 * - Supply chain emissions
 * - Carbon neutral certifications
 * - Emission reduction programs
 */
contract CarbonFootprintTracker is SecurityBase {
    enum ActivityType {
        TRANSPORTATION,
        ENERGY_CONSUMPTION,
        WASTE_GENERATION,
        FOOD_CONSUMPTION,
        MANUFACTURING,
        SUPPLY_CHAIN,
        TRAVEL,
        DIGITAL_USAGE,
        BUILDING_OPERATIONS,
        OTHER
    }
    
    enum EmissionScope {
        SCOPE_1,    // Direct emissions
        SCOPE_2,    // Indirect emissions from electricity
        SCOPE_3     // Other indirect emissions
    }
    
    enum EntityType {
        INDIVIDUAL,
        SMALL_BUSINESS,
        CORPORATION,
        NONPROFIT,
        GOVERNMENT
    }
    
    struct CarbonEntity {
        address entityAddress;
        EntityType entityType;
        string name;
        string description;
        uint256 registrationDate;
        uint256 totalEmissions;        // Total lifetime emissions in kg CO2e
        uint256 currentYearEmissions;  // Current year emissions in kg CO2e
        uint256 targetEmissions;       // Annual emission target
        uint256 offsetCredits;         // Purchased offset credits
        bool isVerified;
        address verifier;
        mapping(uint256 => uint256) annualEmissions; // year => emissions
        mapping(ActivityType => uint256) activityEmissions;
        mapping(EmissionScope => uint256) scopeEmissions;
    }
    
    struct EmissionRecord {
        uint256 recordId;
        address entity;
        ActivityType activityType;
        EmissionScope scope;
        uint256 emissionAmount;        // kg CO2e
        string description;
        string evidenceURI;
        uint256 timestamp;
        uint256 reportingPeriod;       // Year/month
        bool isVerified;
        address verifier;
        mapping(string => uint256) activityData; // Additional activity-specific data
    }
    
    struct OffsetPurchase {
        uint256 purchaseId;
        address purchaser;
        uint256 offsetAmount;          // kg CO2e offset
        uint256 pricePerTon;
        address offsetProvider;
        string offsetType;
        uint256 purchaseDate;
        string certificateURI;
        bool isRetired;
    }
    
    struct EmissionGoal {
        uint256 goalId;
        address entity;
        uint256 targetYear;
        uint256 baselineYear;
        uint256 baselineEmissions;
        uint256 targetEmissions;
        uint256 reductionPercentage;
        uint256 deadline;
        bool isAchieved;
        uint256 achievementDate;
        mapping(uint256 => uint256) progressByYear;
    }
    
    struct BenchmarkData {
        EntityType entityType;
        ActivityType activityType;
        uint256 averageEmissions;      // Average per entity
        uint256 topPerformerEmissions; // Top 10% performers
        uint256 sampleSize;
        uint256 lastUpdated;
    }
    
    struct AutoOffset {
        address entity;
        bool isEnabled;
        uint256 offsetPercentage;      // Percentage of emissions to auto-offset
        uint256 maxMonthlyBudget;      // Maximum monthly offset budget
        address preferredProvider;
        ActivityType[] targetActivities;
        uint256 lastOffsetDate;
    }
    
    // Entity management
    mapping(address => CarbonEntity) public carbonEntities;
    address[] public registeredEntities;
    
    // Emission tracking
    mapping(uint256 => EmissionRecord) public emissionRecords;
    mapping(address => uint256[]) public entityRecords;
    uint256 public recordCount;
    
    // Offset tracking
    mapping(uint256 => OffsetPurchase) public offsetPurchases;
    mapping(address => uint256[]) public entityOffsets;
    uint256 public offsetCount;
    
    // Goals and targets
    mapping(uint256 => EmissionGoal) public emissionGoals;
    mapping(address => uint256[]) public entityGoals;
    uint256 public goalCount;
    
    // Benchmarking
    mapping(bytes32 => BenchmarkData) public benchmarks; // hash(entityType, activityType) => data
    
    // Auto-offset settings
    mapping(address => AutoOffset) public autoOffsetSettings;
    
    // Verification system
    mapping(address => bool) public verifiedReporters;
    mapping(address => string) public reporterCredentials;
    
    // Emission factors (kg CO2e per unit)
    mapping(bytes32 => uint256) public emissionFactors; // hash(activityType, unit) => factor
    
    // Global statistics
    uint256 public totalGlobalEmissions;
    uint256 public totalOffsetsRetired;
    uint256 public totalEntitiesRegistered;
    mapping(uint256 => uint256) public annualGlobalEmissions; // year => emissions
    
    // Events
    event EntityRegistered(
        address indexed entity,
        EntityType entityType,
        string name
    );
    event EmissionRecorded(
        uint256 indexed recordId,
        address indexed entity,
        ActivityType activityType,
        uint256 emissionAmount
    );
    event OffsetPurchased(
        uint256 indexed purchaseId,
        address indexed purchaser,
        uint256 offsetAmount,
        uint256 totalPrice
    );
    event GoalSet(
        uint256 indexed goalId,
        address indexed entity,
        uint256 targetYear,
        uint256 reductionPercentage
    );
    event GoalAchieved(
        uint256 indexed goalId,
        address indexed entity,
        uint256 achievementDate
    );
    event AutoOffsetExecuted(
        address indexed entity,
        uint256 offsetAmount,
        uint256 cost
    );
    event BenchmarkUpdated(
        EntityType entityType,
        ActivityType activityType,
        uint256 newAverage
    );
    event VerifierCertified(
        address indexed verifier,
        string credentials
    );
    
    // Errors
    error EntityNotRegistered();
    error RecordNotExists();
    error NotAuthorizedReporter();
    error InsufficientOffsetBudget();
    error GoalNotExists();
    error InvalidEmissionFactor();
    error AutoOffsetNotEnabled();
    
    modifier onlyRegisteredEntity() {
        if (carbonEntities[msg.sender].entityAddress == address(0)) {
            revert EntityNotRegistered();
        }
        _;
    }
    
    modifier onlyVerifiedReporter() {
        if (!verifiedReporters[msg.sender]) {
            revert NotAuthorizedReporter();
        }
        _;
    }
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        verifiedReporters[initialOwner] = true;
        reporterCredentials[initialOwner] = "System Administrator";
        
        _initializeEmissionFactors();
    }
    
    /**
     * @dev Register entity for carbon tracking
     */
    function registerEntity(
        EntityType entityType,
        string calldata name,
        string calldata description
    ) external {
        require(carbonEntities[msg.sender].entityAddress == address(0), "CarbonFootprintTracker: Already registered");
        
        CarbonEntity storage entity = carbonEntities[msg.sender];
        entity.entityAddress = msg.sender;
        entity.entityType = entityType;
        entity.name = name;
        entity.description = description;
        entity.registrationDate = block.timestamp;
        entity.totalEmissions = 0;
        entity.currentYearEmissions = 0;
        entity.targetEmissions = 0;
        entity.offsetCredits = 0;
        entity.isVerified = false;
        
        registeredEntities.push(msg.sender);
        totalEntitiesRegistered++;
        
        emit EntityRegistered(msg.sender, entityType, name);
    }
    
    /**
     * @dev Record emission activity
     */
    function recordEmission(
        ActivityType activityType,
        EmissionScope scope,
        uint256 activityAmount,
        string calldata unit,
        string calldata description,
        string calldata evidenceURI
    ) external onlyRegisteredEntity returns (uint256) {
        // Calculate emissions using emission factors
        bytes32 factorKey = keccak256(abi.encodePacked(activityType, unit));
        uint256 emissionFactor = emissionFactors[factorKey];
        require(emissionFactor > 0, "CarbonFootprintTracker: Unknown emission factor");
        
        uint256 emissionAmount = (activityAmount * emissionFactor) / 1000; // Convert to kg CO2e
        
        uint256 recordId = recordCount++;
        uint256 currentYear = _getCurrentYear();
        
        EmissionRecord storage record = emissionRecords[recordId];
        record.recordId = recordId;
        record.entity = msg.sender;
        record.activityType = activityType;
        record.scope = scope;
        record.emissionAmount = emissionAmount;
        record.description = description;
        record.evidenceURI = evidenceURI;
        record.timestamp = block.timestamp;
        record.reportingPeriod = currentYear;
        record.isVerified = false;
        
        // Update entity emissions
        CarbonEntity storage entity = carbonEntities[msg.sender];
        entity.totalEmissions += emissionAmount;
        entity.currentYearEmissions += emissionAmount;
        entity.annualEmissions[currentYear] += emissionAmount;
        entity.activityEmissions[activityType] += emissionAmount;
        entity.scopeEmissions[scope] += emissionAmount;
        
        // Update global statistics
        totalGlobalEmissions += emissionAmount;
        annualGlobalEmissions[currentYear] += emissionAmount;
        
        entityRecords[msg.sender].push(recordId);
        
        emit EmissionRecorded(recordId, msg.sender, activityType, emissionAmount);
        
        // Check for auto-offset
        _checkAutoOffset(msg.sender, emissionAmount, activityType);
        
        return recordId;
    }
    
    /**
     * @dev Purchase carbon offsets
     */
    function purchaseOffsets(
        uint256 offsetAmount,
        address offsetProvider,
        string calldata offsetType,
        string calldata certificateURI
    ) external payable onlyRegisteredEntity returns (uint256) {
        require(offsetAmount > 0, "CarbonFootprintTracker: Invalid offset amount");
        require(msg.value > 0, "CarbonFootprintTracker: Payment required");
        
        uint256 pricePerTon = msg.value / (offsetAmount / 1000); // Price per ton CO2e
        
        uint256 purchaseId = offsetCount++;
        
        offsetPurchases[purchaseId] = OffsetPurchase({
            purchaseId: purchaseId,
            purchaser: msg.sender,
            offsetAmount: offsetAmount,
            pricePerTon: pricePerTon,
            offsetProvider: offsetProvider,
            offsetType: offsetType,
            purchaseDate: block.timestamp,
            certificateURI: certificateURI,
            isRetired: false
        });
        
        // Update entity offset credits
        carbonEntities[msg.sender].offsetCredits += offsetAmount;
        entityOffsets[msg.sender].push(purchaseId);
        
        // Transfer payment to offset provider
        payable(offsetProvider).transfer(msg.value);
        
        emit OffsetPurchased(purchaseId, msg.sender, offsetAmount, msg.value);
        return purchaseId;
    }
    
    /**
     * @dev Retire offset credits
     */
    function retireOffsets(uint256 offsetAmount) external onlyRegisteredEntity {
        CarbonEntity storage entity = carbonEntities[msg.sender];
        require(entity.offsetCredits >= offsetAmount, "CarbonFootprintTracker: Insufficient offset credits");
        
        entity.offsetCredits -= offsetAmount;
        totalOffsetsRetired += offsetAmount;
        
        // Mark offset purchases as retired (simplified)
        // In production, would track specific purchases being retired
    }
    
    /**
     * @dev Set emission reduction goal
     */
    function setEmissionGoal(
        uint256 targetYear,
        uint256 baselineYear,
        uint256 reductionPercentage,
        uint256 deadline
    ) external onlyRegisteredEntity returns (uint256) {
        require(targetYear > baselineYear, "CarbonFootprintTracker: Invalid target year");
        require(reductionPercentage <= 100, "CarbonFootprintTracker: Invalid reduction percentage");
        
        CarbonEntity storage entity = carbonEntities[msg.sender];
        uint256 baselineEmissions = entity.annualEmissions[baselineYear];
        require(baselineEmissions > 0, "CarbonFootprintTracker: No baseline data");
        
        uint256 targetEmissions = baselineEmissions - (baselineEmissions * reductionPercentage / 100);
        
        uint256 goalId = goalCount++;
        
        EmissionGoal storage goal = emissionGoals[goalId];
        goal.goalId = goalId;
        goal.entity = msg.sender;
        goal.targetYear = targetYear;
        goal.baselineYear = baselineYear;
        goal.baselineEmissions = baselineEmissions;
        goal.targetEmissions = targetEmissions;
        goal.reductionPercentage = reductionPercentage;
        goal.deadline = deadline;
        goal.isAchieved = false;
        
        entityGoals[msg.sender].push(goalId);
        
        // Update entity target
        entity.targetEmissions = targetEmissions;
        
        emit GoalSet(goalId, msg.sender, targetYear, reductionPercentage);
        return goalId;
    }
    
    /**
     * @dev Setup auto-offset configuration
     */
    function setupAutoOffset(
        bool isEnabled,
        uint256 offsetPercentage,
        uint256 maxMonthlyBudget,
        address preferredProvider,
        ActivityType[] calldata targetActivities
    ) external onlyRegisteredEntity {
        require(offsetPercentage <= 100, "CarbonFootprintTracker: Invalid offset percentage");
        
        AutoOffset storage autoOffset = autoOffsetSettings[msg.sender];
        autoOffset.entity = msg.sender;
        autoOffset.isEnabled = isEnabled;
        autoOffset.offsetPercentage = offsetPercentage;
        autoOffset.maxMonthlyBudget = maxMonthlyBudget;
        autoOffset.preferredProvider = preferredProvider;
        autoOffset.targetActivities = targetActivities;
        autoOffset.lastOffsetDate = block.timestamp;
    }
    
    /**
     * @dev Verify emission record
     */
    function verifyEmissionRecord(uint256 recordId) external onlyVerifiedReporter {
        require(recordId < recordCount, "CarbonFootprintTracker: Record not exists");
        
        EmissionRecord storage record = emissionRecords[recordId];
        record.isVerified = true;
        record.verifier = msg.sender;
    }
    
    /**
     * @dev Update emission factors
     */
    function updateEmissionFactor(
        ActivityType activityType,
        string calldata unit,
        uint256 factor
    ) external onlyRole(ADMIN_ROLE) {
        bytes32 factorKey = keccak256(abi.encodePacked(activityType, unit));
        emissionFactors[factorKey] = factor;
    }
    
    /**
     * @dev Update benchmarks
     */
    function updateBenchmark(
        EntityType entityType,
        ActivityType activityType,
        uint256 averageEmissions,
        uint256 topPerformerEmissions,
        uint256 sampleSize
    ) external onlyVerifiedReporter {
        bytes32 benchmarkKey = keccak256(abi.encodePacked(entityType, activityType));
        
        benchmarks[benchmarkKey] = BenchmarkData({
            entityType: entityType,
            activityType: activityType,
            averageEmissions: averageEmissions,
            topPerformerEmissions: topPerformerEmissions,
            sampleSize: sampleSize,
            lastUpdated: block.timestamp
        });
        
        emit BenchmarkUpdated(entityType, activityType, averageEmissions);
    }
    
    /**
     * @dev Check goal progress and mark as achieved
     */
    function checkGoalProgress(uint256 goalId) external {
        require(goalId < goalCount, "CarbonFootprintTracker: Goal not exists");
        
        EmissionGoal storage goal = emissionGoals[goalId];
        require(!goal.isAchieved, "CarbonFootprintTracker: Goal already achieved");
        
        uint256 currentYear = _getCurrentYear();
        CarbonEntity storage entity = carbonEntities[goal.entity];
        
        if (currentYear >= goal.targetYear) {
            uint256 currentEmissions = entity.annualEmissions[goal.targetYear];
            if (currentEmissions <= goal.targetEmissions) {
                goal.isAchieved = true;
                goal.achievementDate = block.timestamp;
                
                emit GoalAchieved(goalId, goal.entity, block.timestamp);
            }
        }
    }
    
    /**
     * @dev Get entity carbon footprint summary
     */
    function getCarbonFootprint(address entity) external view returns (
        uint256 totalEmissions,
        uint256 currentYearEmissions,
        uint256 targetEmissions,
        uint256 offsetCredits,
        uint256 netEmissions,
        bool isVerified
    ) {
        CarbonEntity storage carbonEntity = carbonEntities[entity];
        uint256 net = carbonEntity.currentYearEmissions > carbonEntity.offsetCredits ? 
                     carbonEntity.currentYearEmissions - carbonEntity.offsetCredits : 0;
        
        return (
            carbonEntity.totalEmissions,
            carbonEntity.currentYearEmissions,
            carbonEntity.targetEmissions,
            carbonEntity.offsetCredits,
            net,
            carbonEntity.isVerified
        );
    }
    
    /**
     * @dev Get emission breakdown by activity
     */
    function getEmissionBreakdown(address entity) external view returns (
        uint256[] memory activityEmissions,
        uint256[] memory scopeEmissions
    ) {
        CarbonEntity storage carbonEntity = carbonEntities[entity];
        
        activityEmissions = new uint256[](10); // Number of ActivityType enum values
        scopeEmissions = new uint256[](3);     // Number of EmissionScope enum values
        
        for (uint256 i = 0; i < 10; i++) {
            activityEmissions[i] = carbonEntity.activityEmissions[ActivityType(i)];
        }
        
        for (uint256 i = 0; i < 3; i++) {
            scopeEmissions[i] = carbonEntity.scopeEmissions[EmissionScope(i)];
        }
        
        return (activityEmissions, scopeEmissions);
    }
    
    /**
     * @dev Get benchmark comparison
     */
    function getBenchmarkComparison(
        address entity,
        ActivityType activityType
    ) external view returns (
        uint256 entityEmissions,
        uint256 averageEmissions,
        uint256 topPerformerEmissions,
        uint256 percentileRank
    ) {
        CarbonEntity storage carbonEntity = carbonEntities[entity];
        bytes32 benchmarkKey = keccak256(abi.encodePacked(carbonEntity.entityType, activityType));
        BenchmarkData storage benchmark = benchmarks[benchmarkKey];
        
        uint256 entityActivity = carbonEntity.activityEmissions[activityType];
        uint256 percentile = 50; // Default to median
        
        if (benchmark.averageEmissions > 0) {
            if (entityActivity <= benchmark.topPerformerEmissions) {
                percentile = 10; // Top 10%
            } else if (entityActivity <= benchmark.averageEmissions) {
                percentile = 25; // Better than average
            } else {
                percentile = 75; // Worse than average
            }
        }
        
        return (
            entityActivity,
            benchmark.averageEmissions,
            benchmark.topPerformerEmissions,
            percentile
        );
    }
    
    /**
     * @dev Get global carbon statistics
     */
    function getGlobalStats() external view returns (
        uint256 totalEmissions,
        uint256 totalOffsets,
        uint256 totalEntities,
        uint256 currentYearEmissions
    ) {
        uint256 currentYear = _getCurrentYear();
        
        return (
            totalGlobalEmissions,
            totalOffsetsRetired,
            totalEntitiesRegistered,
            annualGlobalEmissions[currentYear]
        );
    }
    
    // Internal functions
    
    function _checkAutoOffset(
        address entity,
        uint256 emissionAmount,
        ActivityType activityType
    ) internal {
        AutoOffset storage autoOffset = autoOffsetSettings[entity];
        
        if (!autoOffset.isEnabled) return;
        
        // Check if activity type is in target list
        bool isTargetActivity = false;
        for (uint256 i = 0; i < autoOffset.targetActivities.length; i++) {
            if (autoOffset.targetActivities[i] == activityType) {
                isTargetActivity = true;
                break;
            }
        }
        
        if (!isTargetActivity && autoOffset.targetActivities.length > 0) return;
        
        uint256 offsetAmount = (emissionAmount * autoOffset.offsetPercentage) / 100;
        
        // Check monthly budget (simplified)
        if (offsetAmount > autoOffset.maxMonthlyBudget) return;
        
        // Execute auto-offset (simplified - would need price oracle)
        emit AutoOffsetExecuted(entity, offsetAmount, 0);
    }
    
    function _getCurrentYear() internal view returns (uint256) {
        return (block.timestamp / 365 days) + 1970; // Simplified year calculation
    }
    
    function _initializeEmissionFactors() internal {
        // Transportation (kg CO2e per km)
        emissionFactors[keccak256(abi.encodePacked(ActivityType.TRANSPORTATION, "car_km"))] = 120;
        emissionFactors[keccak256(abi.encodePacked(ActivityType.TRANSPORTATION, "flight_km"))] = 250;
        emissionFactors[keccak256(abi.encodePacked(ActivityType.TRANSPORTATION, "train_km"))] = 40;
        
        // Energy (kg CO2e per kWh)
        emissionFactors[keccak256(abi.encodePacked(ActivityType.ENERGY_CONSUMPTION, "kwh"))] = 500;
        
        // Waste (kg CO2e per kg)
        emissionFactors[keccak256(abi.encodePacked(ActivityType.WASTE_GENERATION, "kg"))] = 1000;
        
        // Food (kg CO2e per kg)
        emissionFactors[keccak256(abi.encodePacked(ActivityType.FOOD_CONSUMPTION, "meat_kg"))] = 27000;
        emissionFactors[keccak256(abi.encodePacked(ActivityType.FOOD_CONSUMPTION, "dairy_kg"))] = 9000;
        emissionFactors[keccak256(abi.encodePacked(ActivityType.FOOD_CONSUMPTION, "veg_kg"))] = 2000;
    }
    
    /**
     * @dev Certify reporter (admin function)
     */
    function certifyReporter(
        address reporter,
        string calldata credentials
    ) external onlyRole(ADMIN_ROLE) {
        verifiedReporters[reporter] = true;
        reporterCredentials[reporter] = credentials;
        
        emit VerifierCertified(reporter, credentials);
    }
}