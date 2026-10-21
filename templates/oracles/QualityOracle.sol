// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";

/**
 * @title QualityOracle
 * @dev Data quality scoring and validation oracle
 * 
 * Features:
 * - Multi-dimensional quality assessment
 * - Reputation-based scoring
 * - Automated quality metrics
 * - Historical quality tracking
 * - Incentive mechanisms for quality data
 * - Dispute resolution system
 * 
 * Use Cases:
 * - Weather data quality assessment
 * - Carbon credit verification
 * - IoT sensor data validation
 * - Research data integrity scoring
 */
contract QualityOracle is SecurityBase {
    enum QualityDimension {
        ACCURACY,       // How accurate the data is
        COMPLETENESS,   // How complete the data is
        TIMELINESS,     // How timely the data is
        CONSISTENCY,    // How consistent the data is
        RELIABILITY,    // How reliable the source is
        TRANSPARENCY    // How transparent the methodology is
    }
    
    struct QualityScore {
        uint256 accuracy;       // 0-10000 (0-100.00%)
        uint256 completeness;   // 0-10000
        uint256 timeliness;     // 0-10000
        uint256 consistency;    // 0-10000
        uint256 reliability;    // 0-10000
        uint256 transparency;   // 0-10000
        uint256 overall;        // Weighted average
        uint256 timestamp;      // When score was calculated
        address evaluator;      // Who evaluated the quality
    }
    
    struct DataSource {
        address source;         // Data source address
        string name;            // Source name/identifier
        uint256 totalSubmissions; // Total data submissions
        uint256 totalScore;     // Cumulative quality score
        uint256 averageScore;   // Average quality score
        uint256 reputation;     // Source reputation (0-10000)
        uint256 lastUpdate;     // Last quality update
        bool active;            // Whether source is active
        mapping(uint256 => QualityScore) qualityHistory; // Historical scores
        uint256 historyCount;   // Number of historical entries
    }
    
    struct QualityReport {
        bytes32 dataHash;       // Hash of data being evaluated
        address dataSource;     // Source of the data
        QualityScore score;     // Quality assessment
        string methodology;     // Assessment methodology used
        string comments;        // Additional comments
        bool disputed;          // Whether report is disputed
        uint256 validatorCount; // Number of validators
        mapping(address => bool) validators; // Validators who confirmed
    }
    
    struct QualityWeights {
        uint256 accuracy;
        uint256 completeness;
        uint256 timeliness;
        uint256 consistency;
        uint256 reliability;
        uint256 transparency;
    }
    
    // Core data structures
    mapping(address => DataSource) public dataSources;
    mapping(bytes32 => QualityReport) public qualityReports;
    mapping(address => bool) public authorizedEvaluators;
    mapping(address => uint256) public evaluatorReputations;
    
    address[] public registeredSources;
    bytes32[] public reportHashes;
    
    // Quality configuration
    QualityWeights public defaultWeights;
    mapping(string => QualityWeights) public categoryWeights; // Different weights per data category
    
    uint256 public minValidators = 3; // Minimum validators for quality consensus
    uint256 public qualityThreshold = 7000; // Minimum quality score for acceptance (70%)
    uint256 public disputePeriod = 7 days; // Period for disputing quality reports
    
    // Incentive system
    mapping(address => uint256) public rewardBalances; // Evaluator reward balances
    uint256 public qualityRewardPool; // Pool for quality evaluation rewards
    uint256 public baseRewardAmount = 10 * 10**18; // Base reward for evaluation
    
    // Historical tracking
    mapping(string => uint256[]) public categoryQualityHistory; // Quality trends by category
    mapping(string => uint256[]) public categoryTimestamps;
    
    // Events
    event DataSourceRegistered(address indexed source, string name);
    event QualityReportSubmitted(bytes32 indexed dataHash, address indexed source, uint256 overallScore);
    event QualityScoreUpdated(address indexed source, uint256 newScore, uint256 newReputation);
    event QualityDisputed(bytes32 indexed dataHash, address indexed disputer, string reason);
    event EvaluatorAuthorized(address indexed evaluator);
    event RewardClaimed(address indexed evaluator, uint256 amount);
    event QualityThresholdUpdated(uint256 oldThreshold, uint256 newThreshold);
    
    // Errors
    error UnauthorizedEvaluator(address evaluator);
    error DataSourceNotRegistered(address source);
    error QualityReportExists(bytes32 dataHash);
    error QualityBelowThreshold(uint256 score, uint256 threshold);
    error DisputePeriodExpired(bytes32 dataHash);
    error InsufficientValidators(uint256 current, uint256 required);
    error InvalidQualityScore(uint256 score);
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        // Set default weights (equal weighting)
        defaultWeights = QualityWeights({
            accuracy: 2000,      // 20%
            completeness: 1500,  // 15%
            timeliness: 1500,    // 15%
            consistency: 1500,   // 15%
            reliability: 2000,   // 20%
            transparency: 1500   // 15%
        });
        
        authorizedEvaluators[initialOwner] = true;
        evaluatorReputations[initialOwner] = 10000; // Max reputation
    }
    
    /**
     * @dev Register a new data source
     */
    function registerDataSource(address source, string calldata name) external onlyRole(ADMIN_ROLE) {
        require(!dataSources[source].active, "QualityOracle: Source already registered");
        
        DataSource storage newSource = dataSources[source];
        newSource.source = source;
        newSource.name = name;
        newSource.totalSubmissions = 0;
        newSource.totalScore = 0;
        newSource.averageScore = 0;
        newSource.reputation = 5000; // Start with neutral reputation
        newSource.lastUpdate = block.timestamp;
        newSource.active = true;
        newSource.historyCount = 0;
        
        registeredSources.push(source);
        
        emit DataSourceRegistered(source, name);
    }
    
    /**
     * @dev Submit quality assessment for data
     */
    function submitQualityReport(
        bytes32 dataHash,
        address dataSource,
        uint256[6] calldata scores, // [accuracy, completeness, timeliness, consistency, reliability, transparency]
        string calldata methodology,
        string calldata comments,
        string calldata category
    ) external returns (uint256 overallScore) {
        if (!authorizedEvaluators[msg.sender]) {
            revert UnauthorizedEvaluator(msg.sender);
        }
        
        if (!dataSources[dataSource].active) {
            revert DataSourceNotRegistered(dataSource);
        }
        
        // Validate scores
        for (uint256 i = 0; i < 6; i++) {
            if (scores[i] > 10000) {
                revert InvalidQualityScore(scores[i]);
            }
        }
        
        // Check if report already exists
        if (qualityReports[dataHash].score.timestamp != 0) {
            revert QualityReportExists(dataHash);
        }
        
        // Calculate weighted overall score
        QualityWeights memory weights = bytes(category).length > 0 ? 
            categoryWeights[category] : defaultWeights;
        
        overallScore = (
            scores[0] * weights.accuracy +
            scores[1] * weights.completeness +
            scores[2] * weights.timeliness +
            scores[3] * weights.consistency +
            scores[4] * weights.reliability +
            scores[5] * weights.transparency
        ) / 10000;
        
        // Create quality score
        QualityScore memory score = QualityScore({
            accuracy: scores[0],
            completeness: scores[1],
            timeliness: scores[2],
            consistency: scores[3],
            reliability: scores[4],
            transparency: scores[5],
            overall: overallScore,
            timestamp: block.timestamp,
            evaluator: msg.sender
        });
        
        // Create quality report
        QualityReport storage report = qualityReports[dataHash];
        report.dataHash = dataHash;
        report.dataSource = dataSource;
        report.score = score;
        report.methodology = methodology;
        report.comments = comments;
        report.disputed = false;
        report.validatorCount = 1;
        report.validators[msg.sender] = true;
        
        reportHashes.push(dataHash);
        
        // Update data source metrics
        _updateDataSourceQuality(dataSource, overallScore);
        
        // Update category history
        if (bytes(category).length > 0) {
            categoryQualityHistory[category].push(overallScore);
            categoryTimestamps[category].push(block.timestamp);
        }
        
        // Reward evaluator
        _rewardEvaluator(msg.sender, overallScore);
        
        emit QualityReportSubmitted(dataHash, dataSource, overallScore);
        return overallScore;
    }
    
    /**
     * @dev Validate an existing quality report
     */
    function validateQualityReport(bytes32 dataHash) external {
        if (!authorizedEvaluators[msg.sender]) {
            revert UnauthorizedEvaluator(msg.sender);
        }
        
        QualityReport storage report = qualityReports[dataHash];
        require(report.score.timestamp != 0, "QualityOracle: Report does not exist");
        require(!report.validators[msg.sender], "QualityOracle: Already validated");
        
        report.validators[msg.sender] = true;
        report.validatorCount++;
        
        // Reward validator
        _rewardEvaluator(msg.sender, report.score.overall);
    }
    
    /**
     * @dev Dispute a quality report
     */
    function disputeQualityReport(bytes32 dataHash, string calldata reason) external {
        QualityReport storage report = qualityReports[dataHash];
        require(report.score.timestamp != 0, "QualityOracle: Report does not exist");
        
        if (block.timestamp > report.score.timestamp + disputePeriod) {
            revert DisputePeriodExpired(dataHash);
        }
        
        report.disputed = true;
        
        emit QualityDisputed(dataHash, msg.sender, reason);
    }
    
    /**
     * @dev Get quality score for data
     */
    function getQualityScore(bytes32 dataHash) external view returns (
        uint256 overall,
        uint256[6] memory scores,
        uint256 timestamp,
        bool isValid
    ) {
        QualityReport storage report = qualityReports[dataHash];
        
        if (report.score.timestamp == 0) {
            return (0, [uint256(0), 0, 0, 0, 0, 0], 0, false);
        }
        
        // Check if report has enough validators and is not disputed
        isValid = report.validatorCount >= minValidators && 
                 !report.disputed &&
                 report.score.overall >= qualityThreshold;
        
        scores = [
            report.score.accuracy,
            report.score.completeness,
            report.score.timeliness,
            report.score.consistency,
            report.score.reliability,
            report.score.transparency
        ];
        
        return (report.score.overall, scores, report.score.timestamp, isValid);
    }
    
    /**
     * @dev Get data source quality metrics
     */
    function getDataSourceQuality(address source) external view returns (
        uint256 averageScore,
        uint256 reputation,
        uint256 totalSubmissions,
        uint256 lastUpdate
    ) {
        DataSource storage ds = dataSources[source];
        return (ds.averageScore, ds.reputation, ds.totalSubmissions, ds.lastUpdate);
    }
    
    /**
     * @dev Get category quality trend
     */
    function getCategoryQualityTrend(string calldata category, uint256 periods) external view returns (
        uint256[] memory scores,
        uint256[] memory timestamps
    ) {
        uint256[] storage categoryScores = categoryQualityHistory[category];
        uint256[] storage categoryTimes = categoryTimestamps[category];
        
        uint256 length = categoryScores.length < periods ? categoryScores.length : periods;
        scores = new uint256[](length);
        timestamps = new uint256[](length);
        
        for (uint256 i = 0; i < length; i++) {
            uint256 index = categoryScores.length - 1 - i;
            scores[i] = categoryScores[index];
            timestamps[i] = categoryTimes[index];
        }
        
        return (scores, timestamps);
    }
    
    /**
     * @dev Get quality reports for data source
     */
    function getSourceQualityReports(address source, uint256 count) external view returns (
        bytes32[] memory hashes,
        uint256[] memory scores,
        uint256[] memory timestamps
    ) {
        uint256 foundCount = 0;
        
        // Count matching reports first
        for (uint256 i = 0; i < reportHashes.length; i++) {
            if (qualityReports[reportHashes[i]].dataSource == source) {
                foundCount++;
                if (foundCount >= count) break;
            }
        }
        
        uint256 resultLength = foundCount < count ? foundCount : count;
        hashes = new bytes32[](resultLength);
        scores = new uint256[](resultLength);
        timestamps = new uint256[](resultLength);
        
        uint256 resultIndex = 0;
        for (uint256 i = reportHashes.length; i > 0 && resultIndex < resultLength; i--) {
            bytes32 hash = reportHashes[i - 1];
            if (qualityReports[hash].dataSource == source) {
                hashes[resultIndex] = hash;
                scores[resultIndex] = qualityReports[hash].score.overall;
                timestamps[resultIndex] = qualityReports[hash].score.timestamp;
                resultIndex++;
            }
        }
        
        return (hashes, scores, timestamps);
    }
    
    /**
     * @dev Calculate composite quality score for multiple data points
     */
    function calculateCompositeScore(bytes32[] calldata dataHashes) external view returns (
        uint256 compositeScore,
        uint256 confidence
    ) {
        require(dataHashes.length > 0, "QualityOracle: No data hashes provided");
        
        uint256 totalScore = 0;
        uint256 validReports = 0;
        uint256 totalValidators = 0;
        
        for (uint256 i = 0; i < dataHashes.length; i++) {
            QualityReport storage report = qualityReports[dataHashes[i]];
            
            if (report.score.timestamp != 0 && 
                !report.disputed && 
                report.validatorCount >= minValidators) {
                
                totalScore += report.score.overall;
                totalValidators += report.validatorCount;
                validReports++;
            }
        }
        
        if (validReports == 0) {
            return (0, 0);
        }
        
        compositeScore = totalScore / validReports;
        confidence = (totalValidators * 10000) / (validReports * minValidators); // Confidence based on validator surplus
        
        if (confidence > 10000) confidence = 10000; // Cap at 100%
        
        return (compositeScore, confidence);
    }
    
    /**
     * @dev Update data source quality metrics
     */
    function _updateDataSourceQuality(address source, uint256 newScore) internal {
        DataSource storage ds = dataSources[source];
        
        // Update average score
        ds.totalScore += newScore;
        ds.totalSubmissions++;
        ds.averageScore = ds.totalScore / ds.totalSubmissions;
        
        // Update reputation based on score and consistency
        uint256 reputationAdjustment = 0;
        if (newScore >= 8000) { // High quality
            reputationAdjustment = 50;
        } else if (newScore >= 6000) { // Medium quality
            reputationAdjustment = 10;
        } else { // Low quality
            reputationAdjustment = newScore < 4000 ? 100 : 30; // Penalty for very low quality
        }
        
        if (newScore >= 6000 && ds.reputation < 10000) {
            ds.reputation = ds.reputation + reputationAdjustment > 10000 ? 
                           10000 : ds.reputation + reputationAdjustment;
        } else if (newScore < 6000 && ds.reputation > 0) {
            ds.reputation = ds.reputation > reputationAdjustment ? 
                           ds.reputation - reputationAdjustment : 0;
        }
        
        ds.lastUpdate = block.timestamp;
        
        // Store in history
        ds.qualityHistory[ds.historyCount] = QualityScore({
            accuracy: 0, // Individual dimensions not stored in source history
            completeness: 0,
            timeliness: 0,
            consistency: 0,
            reliability: 0,
            transparency: 0,
            overall: newScore,
            timestamp: block.timestamp,
            evaluator: msg.sender
        });
        ds.historyCount++;
        
        emit QualityScoreUpdated(source, ds.averageScore, ds.reputation);
    }
    
    /**
     * @dev Reward evaluator for quality assessment
     */
    function _rewardEvaluator(address evaluator, uint256 qualityScore) internal {
        // Higher quality assessments get better rewards
        uint256 rewardMultiplier = qualityScore / 100; // 0-100x multiplier
        uint256 reward = (baseRewardAmount * rewardMultiplier) / 100;
        
        // Adjust by evaluator reputation
        uint256 finalReward = (reward * evaluatorReputations[evaluator]) / 10000;
        
        rewardBalances[evaluator] += finalReward;
        
        // Update evaluator reputation slightly
        if (qualityScore >= 7000 && evaluatorReputations[evaluator] < 10000) {
            evaluatorReputations[evaluator] += 10;
            if (evaluatorReputations[evaluator] > 10000) {
                evaluatorReputations[evaluator] = 10000;
            }
        }
    }
    
    /**
     * @dev Claim accumulated rewards
     */
    function claimRewards() external nonReentrant {
        uint256 balance = rewardBalances[msg.sender];
        require(balance > 0, "QualityOracle: No rewards to claim");
        
        rewardBalances[msg.sender] = 0;
        
        // Transfer rewards (assuming contract has sufficient balance)
        payable(msg.sender).transfer(balance);
        
        emit RewardClaimed(msg.sender, balance);
    }
    
    /**
     * @dev Fund reward pool
     */
    function fundRewardPool() external payable onlyRole(ADMIN_ROLE) {
        qualityRewardPool += msg.value;
    }
    
    /**
     * @dev Set quality weights for category
     */
    function setCategoryWeights(
        string calldata category,
        uint256[6] calldata weights
    ) external onlyRole(ADMIN_ROLE) {
        // Validate weights sum to 10000
        uint256 totalWeight = weights[0] + weights[1] + weights[2] + weights[3] + weights[4] + weights[5];
        require(totalWeight == 10000, "QualityOracle: Weights must sum to 10000");
        
        categoryWeights[category] = QualityWeights({
            accuracy: weights[0],
            completeness: weights[1],
            timeliness: weights[2],
            consistency: weights[3],
            reliability: weights[4],
            transparency: weights[5]
        });
    }
    
    /**
     * @dev Add authorized evaluator
     */
    function addEvaluator(address evaluator, uint256 initialReputation) external onlyRole(ADMIN_ROLE) {
        require(initialReputation <= 10000, "QualityOracle: Invalid reputation");
        authorizedEvaluators[evaluator] = true;
        evaluatorReputations[evaluator] = initialReputation;
        
        emit EvaluatorAuthorized(evaluator);
    }
    
    /**
     * @dev Remove evaluator authorization
     */
    function removeEvaluator(address evaluator) external onlyRole(ADMIN_ROLE) {
        authorizedEvaluators[evaluator] = false;
        evaluatorReputations[evaluator] = 0;
    }
    
    /**
     * @dev Set quality threshold
     */
    function setQualityThreshold(uint256 threshold) external onlyRole(ADMIN_ROLE) {
        require(threshold <= 10000, "QualityOracle: Invalid threshold");
        uint256 oldThreshold = qualityThreshold;
        qualityThreshold = threshold;
        
        emit QualityThresholdUpdated(oldThreshold, threshold);
    }
    
    /**
     * @dev Set minimum validators
     */
    function setMinValidators(uint256 minVal) external onlyRole(ADMIN_ROLE) {
        minValidators = minVal;
    }
    
    /**
     * @dev Set base reward amount
     */
    function setBaseRewardAmount(uint256 amount) external onlyRole(ADMIN_ROLE) {
        baseRewardAmount = amount;
    }
    
    /**
     * @dev Get registered data sources
     */
    function getRegisteredSources() external view returns (address[] memory) {
        return registeredSources;
    }
    
    /**
     * @dev Get evaluator info
     */
    function getEvaluatorInfo(address evaluator) external view returns (
        bool authorized,
        uint256 reputation,
        uint256 rewardBalance
    ) {
        return (
            authorizedEvaluators[evaluator],
            evaluatorReputations[evaluator],
            rewardBalances[evaluator]
        );
    }
}