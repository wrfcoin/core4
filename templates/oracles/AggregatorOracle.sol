// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";

/**
 * @title IDataOracle
 * @dev Interface for data oracles
 */
interface IDataOracle {
    function getLatestValue() external view returns (uint256 value, uint256 timestamp, uint256 confidence);
    function getValue(string calldata key) external view returns (uint256 value, uint256 timestamp, uint256 confidence);
}

/**
 * @title AggregatorOracle
 * @dev Multi-source data aggregation with consensus mechanisms
 * 
 * Features:
 * - Multiple oracle source aggregation
 * - Weighted consensus algorithms
 * - Outlier detection and filtering
 * - Dynamic source reliability scoring
 * - Temporal aggregation (TWAP, median over time)
 * - Fault tolerance and fallback mechanisms
 * 
 * Use Cases:
 * - Price feed aggregation
 * - Weather data consensus
 * - IoT sensor data aggregation
 * - Multi-chain oracle bridging
 */
contract AggregatorOracle is SecurityBase {
    enum AggregationMethod {
        WEIGHTED_AVERAGE,
        MEDIAN,
        MODE,
        OUTLIER_FILTERED_AVERAGE,
        CONFIDENCE_WEIGHTED,
        TEMPORAL_WEIGHTED
    }
    
    struct DataSource {
        address oracle;          // Oracle contract address
        string name;             // Source identifier
        uint256 weight;          // Weight in aggregation (basis points)
        uint256 reliability;     // Reliability score (0-10000)
        uint256 latency;         // Expected latency in seconds
        uint256 lastUpdate;      // Last successful update timestamp
        uint256 successCount;    // Number of successful submissions
        uint256 failureCount;    // Number of failed submissions
        bool active;             // Whether source is active
        bytes4 selector;         // Function selector for data retrieval
    }
    
    struct AggregatedValue {
        uint256 value;           // Aggregated value
        uint256 confidence;      // Confidence level (0-10000)
        uint256 timestamp;       // When aggregation was performed
        uint256 sourceCount;     // Number of sources used
        AggregationMethod method; // Method used for aggregation
        uint256 variance;        // Statistical variance of sources
        bool valid;              // Whether aggregation is valid
    }
    
    struct SourceData {
        uint256 value;
        uint256 timestamp;
        uint256 confidence;
        bool valid;
    }
    
    struct AggregationConfig {
        AggregationMethod defaultMethod;
        uint256 minSources;          // Minimum sources required
        uint256 maxVariance;         // Maximum allowed variance
        uint256 outlierThreshold;    // Outlier detection threshold (basis points)
        uint256 stalenessThreshold;  // Maximum age for data (seconds)
        uint256 confidenceThreshold; // Minimum confidence required
        bool requireConsensus;       // Whether consensus is required
        uint256 consensusThreshold;  // Percentage agreement needed (basis points)
    }
    
    // Core data structures
    mapping(string => DataSource[]) public dataSources; // asset => sources
    mapping(string => AggregatedValue) public aggregatedValues;
    mapping(string => AggregationConfig) public aggregationConfigs;
    mapping(string => mapping(address => uint256)) public sourceIndices; // asset => oracle => index
    
    // Historical data
    mapping(string => uint256[]) public valueHistory;
    mapping(string => uint256[]) public timestampHistory;
    mapping(string => uint256) public historyIndex; // Circular buffer index
    uint256 public constant MAX_HISTORY = 1000;
    
    // Outlier detection
    mapping(string => uint256[]) public recentValues; // For outlier detection
    mapping(string => uint256) public outlierCount;
    
    // Emergency and fallback
    mapping(string => uint256) public emergencyValues;
    mapping(string => bool) public emergencyMode;
    mapping(string => address) public fallbackOracles;
    
    // Source reliability tracking
    mapping(string => mapping(address => uint256[])) public reliabilityHistory;
    mapping(string => mapping(address => uint256)) public responseTimeHistory;
    
    // Events
    event DataSourceAdded(string indexed asset, address indexed oracle, string name, uint256 weight);
    event DataSourceUpdated(string indexed asset, address indexed oracle, uint256 newWeight, uint256 newReliability);
    event ValueAggregated(string indexed asset, uint256 value, uint256 confidence, AggregationMethod method);
    event OutlierDetected(string indexed asset, address indexed oracle, uint256 value, uint256 expected);
    event SourceFailure(string indexed asset, address indexed oracle, string reason);
    event EmergencyModeActivated(string indexed asset, uint256 emergencyValue);
    event AggregationMethodChanged(string indexed asset, AggregationMethod oldMethod, AggregationMethod newMethod);
    
    // Errors
    error AssetNotConfigured(string asset);
    error InsufficientSources(string asset, uint256 available, uint256 required);
    error HighVariance(string asset, uint256 variance, uint256 threshold);
    error NoValidSources(string asset);
    error OutlierThresholdExceeded(string asset);
    error ConsensusNotReached(string asset, uint256 agreement, uint256 threshold);
    error SourceAlreadyExists(string asset, address oracle);
    error SourceNotFound(string asset, address oracle);
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        // Set default aggregation configs will be done per asset
    }
    
    /**
     * @dev Add a data source for an asset
     */
    function addDataSource(
        string calldata asset,
        address oracle,
        string calldata name,
        uint256 weight,
        uint256 reliability,
        uint256 latency,
        bytes4 selector
    ) external onlyRole(ADMIN_ROLE) {
        DataSource[] storage sources = dataSources[asset];
        
        // Check if source already exists
        for (uint256 i = 0; i < sources.length; i++) {
            if (sources[i].oracle == oracle) {
                revert SourceAlreadyExists(asset, oracle);
            }
        }
        
        sources.push(DataSource({
            oracle: oracle,
            name: name,
            weight: weight,
            reliability: reliability,
            latency: latency,
            lastUpdate: 0,
            successCount: 0,
            failureCount: 0,
            active: true,
            selector: selector
        }));
        
        sourceIndices[asset][oracle] = sources.length - 1;
        
        emit DataSourceAdded(asset, oracle, name, weight);
    }
    
    /**
     * @dev Update data source configuration
     */
    function updateDataSource(
        string calldata asset,
        address oracle,
        uint256 newWeight,
        uint256 newReliability,
        bool active
    ) external onlyRole(ADMIN_ROLE) {
        DataSource[] storage sources = dataSources[asset];
        uint256 index = sourceIndices[asset][oracle];
        
        if (index >= sources.length || sources[index].oracle != oracle) {
            revert SourceNotFound(asset, oracle);
        }
        
        sources[index].weight = newWeight;
        sources[index].reliability = newReliability;
        sources[index].active = active;
        
        emit DataSourceUpdated(asset, oracle, newWeight, newReliability);
    }
    
    /**
     * @dev Set aggregation configuration for an asset
     */
    function setAggregationConfig(
        string calldata asset,
        AggregationMethod method,
        uint256 minSources,
        uint256 maxVariance,
        uint256 outlierThreshold,
        uint256 stalenessThreshold,
        uint256 confidenceThreshold,
        bool requireConsensus,
        uint256 consensusThreshold
    ) external onlyRole(ADMIN_ROLE) {
        aggregationConfigs[asset] = AggregationConfig({
            defaultMethod: method,
            minSources: minSources,
            maxVariance: maxVariance,
            outlierThreshold: outlierThreshold,
            stalenessThreshold: stalenessThreshold,
            confidenceThreshold: confidenceThreshold,
            requireConsensus: requireConsensus,
            consensusThreshold: consensusThreshold
        });
    }
    
    /**
     * @dev Aggregate data from all sources for an asset
     */
    function aggregateValue(string calldata asset) external nonReentrant returns (uint256) {
        AggregationConfig storage config = aggregationConfigs[asset];
        DataSource[] storage sources = dataSources[asset];
        
        if (sources.length == 0) {
            revert AssetNotConfigured(asset);
        }
        
        // Collect data from all active sources
        SourceData[] memory sourceData = new SourceData[](sources.length);
        uint256 validSources = 0;
        
        for (uint256 i = 0; i < sources.length; i++) {
            if (sources[i].active) {
                SourceData memory data = _fetchSourceData(asset, i);
                if (data.valid && _isDataFresh(data.timestamp, config.stalenessThreshold)) {
                    sourceData[validSources] = data;
                    validSources++;
                    sources[i].successCount++;
                    sources[i].lastUpdate = block.timestamp;
                } else {
                    sources[i].failureCount++;
                    emit SourceFailure(asset, sources[i].oracle, "Stale or invalid data");
                }
            }
        }
        
        if (validSources < config.minSources) {
            revert InsufficientSources(asset, validSources, config.minSources);
        }
        
        // Remove outliers if configured
        if (config.outlierThreshold > 0) {
            validSources = _removeOutliers(sourceData, validSources, config.outlierThreshold);
        }
        
        if (validSources == 0) {
            revert NoValidSources(asset);
        }
        
        // Perform aggregation
        (uint256 aggregatedValue, uint256 confidence, uint256 variance) = _performAggregation(
            sourceData,
            validSources,
            config.defaultMethod,
            asset
        );
        
        // Validate results
        if (variance > config.maxVariance) {
            revert HighVariance(asset, variance, config.maxVariance);
        }
        
        if (config.requireConsensus) {
            uint256 consensus = _calculateConsensus(sourceData, validSources, aggregatedValue);
            if (consensus < config.consensusThreshold) {
                revert ConsensusNotReached(asset, consensus, config.consensusThreshold);
            }
        }
        
        // Store aggregated value
        aggregatedValues[asset] = AggregatedValue({
            value: aggregatedValue,
            confidence: confidence,
            timestamp: block.timestamp,
            sourceCount: validSources,
            method: config.defaultMethod,
            variance: variance,
            valid: true
        });
        
        // Update history
        _updateHistory(asset, aggregatedValue);
        
        // Update source reliabilities
        _updateSourceReliabilities(asset, sourceData, validSources, aggregatedValue);
        
        emit ValueAggregated(asset, aggregatedValue, confidence, config.defaultMethod);
        return aggregatedValue;
    }
    
    /**
     * @dev Get the latest aggregated value for an asset
     */
    function getAggregatedValue(string calldata asset) external view returns (
        uint256 value,
        uint256 confidence,
        uint256 timestamp,
        bool valid
    ) {
        if (emergencyMode[asset]) {
            return (emergencyValues[asset], 10000, block.timestamp, true);
        }
        
        AggregatedValue storage aggregated = aggregatedValues[asset];
        return (aggregated.value, aggregated.confidence, aggregated.timestamp, aggregated.valid);
    }
    
    /**
     * @dev Get TWAP (Time-Weighted Average Price) for an asset
     */
    function getTWAP(string calldata asset, uint256 period) external view returns (uint256) {
        uint256[] storage values = valueHistory[asset];
        uint256[] storage timestamps = timestampHistory[asset];
        
        if (values.length == 0) {
            return aggregatedValues[asset].value;
        }
        
        uint256 targetTime = block.timestamp > period ? block.timestamp - period : 0;
        uint256 weightedSum = 0;
        uint256 totalWeight = 0;
        
        for (uint256 i = 0; i < values.length; i++) {
            if (timestamps[i] >= targetTime) {
                uint256 weight = block.timestamp - timestamps[i] + 1;
                weightedSum += values[i] * weight;
                totalWeight += weight;
            }
        }
        
        return totalWeight > 0 ? weightedSum / totalWeight : aggregatedValues[asset].value;
    }
    
    /**
     * @dev Get median value over a time period
     */
    function getMedian(string calldata asset, uint256 period) external view returns (uint256) {
        uint256[] storage values = valueHistory[asset];
        uint256[] storage timestamps = timestampHistory[asset];
        
        if (values.length == 0) {
            return aggregatedValues[asset].value;
        }
        
        uint256 targetTime = block.timestamp > period ? block.timestamp - period : 0;
        uint256[] memory periodValues = new uint256[](values.length);
        uint256 count = 0;
        
        for (uint256 i = 0; i < values.length; i++) {
            if (timestamps[i] >= targetTime) {
                periodValues[count] = values[i];
                count++;
            }
        }
        
        if (count == 0) {
            return aggregatedValues[asset].value;
        }
        
        return _quickSelectMedian(periodValues, 0, count - 1);
    }
    
    /**
     * @dev Activate emergency mode with manual value
     */
    function activateEmergencyMode(string calldata asset, uint256 emergencyValue) external onlyRole(EMERGENCY_ROLE) {
        emergencyMode[asset] = true;
        emergencyValues[asset] = emergencyValue;
        
        emit EmergencyModeActivated(asset, emergencyValue);
    }
    
    /**
     * @dev Deactivate emergency mode
     */
    function deactivateEmergencyMode(string calldata asset) external onlyRole(ADMIN_ROLE) {
        emergencyMode[asset] = false;
        emergencyValues[asset] = 0;
    }
    
    /**
     * @dev Set fallback oracle for an asset
     */
    function setFallbackOracle(string calldata asset, address fallbackOracle) external onlyRole(ADMIN_ROLE) {
        fallbackOracles[asset] = fallbackOracle;
    }
    
    /**
     * @dev Fetch data from a specific source
     */
    function _fetchSourceData(string calldata asset, uint256 sourceIndex) internal view returns (SourceData memory) {
        DataSource storage source = dataSources[asset][sourceIndex];
        
        try IDataOracle(source.oracle).getLatestValue() returns (
            uint256 value,
            uint256 timestamp,
            uint256 confidence
        ) {
            return SourceData({
                value: value,
                timestamp: timestamp,
                confidence: confidence,
                valid: value > 0 && timestamp > 0
            });
        } catch {
            return SourceData({
                value: 0,
                timestamp: 0,
                confidence: 0,
                valid: false
            });
        }
    }
    
    /**
     * @dev Check if data is fresh based on staleness threshold
     */
    function _isDataFresh(uint256 dataTimestamp, uint256 stalenessThreshold) internal view returns (bool) {
        return block.timestamp <= dataTimestamp + stalenessThreshold;
    }
    
    /**
     * @dev Remove outliers from source data
     */
    function _removeOutliers(
        SourceData[] memory data,
        uint256 validCount,
        uint256 outlierThreshold
    ) internal pure returns (uint256) {
        if (validCount < 3) return validCount; // Need at least 3 data points for outlier detection
        
        // Calculate median
        uint256[] memory values = new uint256[](validCount);
        for (uint256 i = 0; i < validCount; i++) {
            values[i] = data[i].value;
        }
        
        uint256 median = _quickSelectMedian(values, 0, validCount - 1);
        
        // Remove outliers
        uint256 newCount = 0;
        for (uint256 i = 0; i < validCount; i++) {
            uint256 deviation = data[i].value > median ? 
                               data[i].value - median : 
                               median - data[i].value;
            uint256 deviationPercent = (deviation * 10000) / median;
            
            if (deviationPercent <= outlierThreshold) {
                if (newCount != i) {
                    data[newCount] = data[i];
                }
                newCount++;
            }
        }
        
        return newCount;
    }
    
    /**
     * @dev Perform aggregation based on method
     */
    function _performAggregation(
        SourceData[] memory data,
        uint256 count,
        AggregationMethod method,
        string calldata asset
    ) internal view returns (uint256 aggregatedValue, uint256 confidence, uint256 variance) {
        if (method == AggregationMethod.WEIGHTED_AVERAGE) {
            return _weightedAverage(data, count, asset);
        } else if (method == AggregationMethod.MEDIAN) {
            return _median(data, count);
        } else if (method == AggregationMethod.CONFIDENCE_WEIGHTED) {
            return _confidenceWeightedAverage(data, count);
        } else {
            // Default to simple average
            return _simpleAverage(data, count);
        }
    }
    
    /**
     * @dev Calculate weighted average
     */
    function _weightedAverage(
        SourceData[] memory data,
        uint256 count,
        string calldata asset
    ) internal view returns (uint256 value, uint256 confidence, uint256 variance) {
        DataSource[] storage sources = dataSources[asset];
        uint256 weightedSum = 0;
        uint256 totalWeight = 0;
        uint256 totalConfidence = 0;
        
        for (uint256 i = 0; i < count; i++) {
            uint256 weight = sources[i].weight * sources[i].reliability / 10000;
            weightedSum += data[i].value * weight;
            totalWeight += weight;
            totalConfidence += data[i].confidence;
        }
        
        value = totalWeight > 0 ? weightedSum / totalWeight : 0;
        confidence = count > 0 ? totalConfidence / count : 0;
        variance = _calculateVariance(data, count, value);
        
        return (value, confidence, variance);
    }
    
    /**
     * @dev Calculate median
     */
    function _median(SourceData[] memory data, uint256 count) internal pure returns (uint256 value, uint256 confidence, uint256 variance) {
        uint256[] memory values = new uint256[](count);
        uint256 totalConfidence = 0;
        
        for (uint256 i = 0; i < count; i++) {
            values[i] = data[i].value;
            totalConfidence += data[i].confidence;
        }
        
        value = _quickSelectMedian(values, 0, count - 1);
        confidence = count > 0 ? totalConfidence / count : 0;
        variance = _calculateVariance(data, count, value);
        
        return (value, confidence, variance);
    }
    
    /**
     * @dev Calculate confidence-weighted average
     */
    function _confidenceWeightedAverage(SourceData[] memory data, uint256 count) internal pure returns (uint256 value, uint256 confidence, uint256 variance) {
        uint256 weightedSum = 0;
        uint256 totalWeight = 0;
        uint256 totalConfidence = 0;
        
        for (uint256 i = 0; i < count; i++) {
            uint256 weight = data[i].confidence;
            weightedSum += data[i].value * weight;
            totalWeight += weight;
            totalConfidence += data[i].confidence;
        }
        
        value = totalWeight > 0 ? weightedSum / totalWeight : 0;
        confidence = count > 0 ? totalConfidence / count : 0;
        variance = _calculateVariance(data, count, value);
        
        return (value, confidence, variance);
    }
    
    /**
     * @dev Calculate simple average
     */
    function _simpleAverage(SourceData[] memory data, uint256 count) internal pure returns (uint256 value, uint256 confidence, uint256 variance) {
        uint256 sum = 0;
        uint256 totalConfidence = 0;
        
        for (uint256 i = 0; i < count; i++) {
            sum += data[i].value;
            totalConfidence += data[i].confidence;
        }
        
        value = count > 0 ? sum / count : 0;
        confidence = count > 0 ? totalConfidence / count : 0;
        variance = _calculateVariance(data, count, value);
        
        return (value, confidence, variance);
    }
    
    /**
     * @dev Calculate variance
     */
    function _calculateVariance(SourceData[] memory data, uint256 count, uint256 mean) internal pure returns (uint256) {
        if (count == 0) return 0;
        
        uint256 sumSquaredDiff = 0;
        for (uint256 i = 0; i < count; i++) {
            uint256 diff = data[i].value > mean ? data[i].value - mean : mean - data[i].value;
            sumSquaredDiff += diff * diff;
        }
        
        return sumSquaredDiff / count;
    }
    
    /**
     * @dev Calculate consensus percentage
     */
    function _calculateConsensus(SourceData[] memory data, uint256 count, uint256 aggregatedValue) internal pure returns (uint256) {
        if (count == 0) return 0;
        
        uint256 agreements = 0;
        uint256 threshold = aggregatedValue / 20; // 5% threshold
        
        for (uint256 i = 0; i < count; i++) {
            uint256 diff = data[i].value > aggregatedValue ? 
                          data[i].value - aggregatedValue : 
                          aggregatedValue - data[i].value;
            if (diff <= threshold) {
                agreements++;
            }
        }
        
        return (agreements * 10000) / count;
    }
    
    /**
     * @dev Update historical data
     */
    function _updateHistory(string calldata asset, uint256 value) internal {
        uint256[] storage values = valueHistory[asset];
        uint256[] storage timestamps = timestampHistory[asset];
        
        if (values.length < MAX_HISTORY) {
            values.push(value);
            timestamps.push(block.timestamp);
        } else {
            uint256 index = historyIndex[asset];
            values[index] = value;
            timestamps[index] = block.timestamp;
            historyIndex[asset] = (index + 1) % MAX_HISTORY;
        }
    }
    
    /**
     * @dev Update source reliabilities based on performance
     */
    function _updateSourceReliabilities(
        string calldata asset,
        SourceData[] memory data,
        uint256 count,
        uint256 aggregatedValue
    ) internal {
        DataSource[] storage sources = dataSources[asset];
        
        for (uint256 i = 0; i < count; i++) {
            uint256 diff = data[i].value > aggregatedValue ? 
                          data[i].value - aggregatedValue : 
                          aggregatedValue - data[i].value;
            uint256 accuracy = aggregatedValue > 0 ? 
                              10000 - ((diff * 10000) / aggregatedValue) : 10000;
            
            // Update reliability (moving average)
            sources[i].reliability = (sources[i].reliability * 9 + accuracy) / 10;
        }
    }
    
    /**
     * @dev Quick select algorithm for median
     */
    function _quickSelectMedian(uint256[] memory arr, uint256 left, uint256 right) internal pure returns (uint256) {
        if (left == right) return arr[left];
        
        uint256 length = right - left + 1;
        uint256 medianIndex = left + length / 2;
        
        return _quickSelect(arr, left, right, medianIndex);
    }
    
    /**
     * @dev Quick select algorithm
     */
    function _quickSelect(uint256[] memory arr, uint256 left, uint256 right, uint256 k) internal pure returns (uint256) {
        if (left == right) return arr[left];
        
        uint256 pivotIndex = _partition(arr, left, right);
        
        if (k == pivotIndex) {
            return arr[k];
        } else if (k < pivotIndex) {
            return _quickSelect(arr, left, pivotIndex - 1, k);
        } else {
            return _quickSelect(arr, pivotIndex + 1, right, k);
        }
    }
    
    /**
     * @dev Partition function for quick select
     */
    function _partition(uint256[] memory arr, uint256 left, uint256 right) internal pure returns (uint256) {
        uint256 pivot = arr[right];
        uint256 i = left;
        
        for (uint256 j = left; j < right; j++) {
            if (arr[j] <= pivot) {
                (arr[i], arr[j]) = (arr[j], arr[i]);
                i++;
            }
        }
        
        (arr[i], arr[right]) = (arr[right], arr[i]);
        return i;
    }
    
    /**
     * @dev Get data sources for an asset
     */
    function getDataSources(string calldata asset) external view returns (DataSource[] memory) {
        return dataSources[asset];
    }
    
    /**
     * @dev Get aggregation statistics
     */
    function getAggregationStats(string calldata asset) external view returns (
        uint256 sourceCount,
        uint256 averageReliability,
        uint256 lastAggregation,
        uint256 totalSuccesses,
        uint256 totalFailures
    ) {
        DataSource[] storage sources = dataSources[asset];
        
        sourceCount = sources.length;
        uint256 totalReliability = 0;
        totalSuccesses = 0;
        totalFailures = 0;
        
        for (uint256 i = 0; i < sources.length; i++) {
            totalReliability += sources[i].reliability;
            totalSuccesses += sources[i].successCount;
            totalFailures += sources[i].failureCount;
        }
        
        averageReliability = sourceCount > 0 ? totalReliability / sourceCount : 0;
        lastAggregation = aggregatedValues[asset].timestamp;
        
        return (sourceCount, averageReliability, lastAggregation, totalSuccesses, totalFailures);
    }
}