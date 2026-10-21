// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";

/**
 * @title AggregatorV3Interface  
 * @dev Chainlink-compatible interface
 */
interface AggregatorV3Interface {
    function latestRoundData() external view returns (
        uint80 roundId,
        int256 answer,
        uint256 startedAt,
        uint256 updatedAt,
        uint80 answeredInRound
    );
    function decimals() external view returns (uint8);
}

/**
 * @title PriceOracle
 * @dev Price feed integration and aggregation oracle
 * 
 * Features:
 * - Multiple price source aggregation
 * - Chainlink price feed integration
 * - TWAP (Time-Weighted Average Price) calculation
 * - Price deviation monitoring
 * - Circuit breaker functionality
 * - Historical price tracking
 * 
 * Use Cases:
 * - DeFi price feeds
 * - Lending protocol collateral valuation
 * - DEX price discovery
 * - Risk management systems
 */
contract PriceOracle is SecurityBase {
    struct PriceFeed {
        address aggregator;          // Chainlink aggregator address
        uint256 heartbeat;           // Maximum time between updates
        uint256 deviation;           // Maximum price deviation (basis points)
        uint8 decimals;              // Price decimals
        bool active;                 // Whether feed is active
        uint256 lastUpdate;          // Last update timestamp
        int256 lastPrice;            // Last reported price
        string description;          // Feed description
    }
    
    struct PriceData {
        uint256 price;               // Current price
        uint256 timestamp;           // Price timestamp
        uint256 confidence;          // Confidence level (0-10000)
        uint256 volume;              // Trading volume (if available)
        bool valid;                  // Whether price is valid
    }
    
    struct TWAPData {
        uint256 cumulativePrice;     // Cumulative price
        uint256 lastTimestamp;       // Last update timestamp
        uint256 windowSize;          // TWAP window size
        uint256[] priceHistory;      // Price history for TWAP
        uint256[] timestampHistory;  // Timestamp history
        uint256 historyIndex;        // Current index in circular buffer
    }
    
    // Oracle configuration
    mapping(string => PriceFeed) public priceFeeds;
    mapping(string => PriceData) public latestPrices;
    mapping(string => TWAPData) public twapData;
    string[] public supportedAssets;
    
    // Aggregation settings
    mapping(string => address[]) public priceSources; // Multiple sources per asset
    mapping(string => uint256[]) public sourceWeights; // Weights for price aggregation
    mapping(string => uint256) public priceDeviationThresholds; // Max deviation between sources
    
    // Circuit breaker
    mapping(string => bool) public circuitBreakerTriggered;
    mapping(string => uint256) public lastValidPrice;
    mapping(string => uint256) public maxPriceDeviation; // Circuit breaker threshold
    
    // Update controls
    mapping(address => bool) public authorizedUpdaters;
    mapping(string => uint256) public minUpdateInterval;
    mapping(string => uint256) public lastUpdateTime;
    
    // Historical data
    mapping(string => uint256[]) public priceHistory;
    mapping(string => uint256[]) public timestampHistory;
    uint256 public constant MAX_HISTORY_SIZE = 1000;
    
    // Emergency features
    mapping(string => uint256) public emergencyPrices; // Manual emergency prices
    mapping(string => bool) public emergencyMode; // Emergency mode per asset
    bool public globalEmergencyMode;
    
    // Events
    event PriceUpdated(string indexed asset, uint256 price, uint256 timestamp, uint256 confidence);
    event PriceFeedAdded(string indexed asset, address aggregator, uint8 decimals);
    event CircuitBreakerTriggered(string indexed asset, uint256 oldPrice, uint256 newPrice);
    event EmergencyModeActivated(string indexed asset, uint256 emergencyPrice);
    event TWAPUpdated(string indexed asset, uint256 twapPrice, uint256 windowSize);
    event PriceSourceAdded(string indexed asset, address source, uint256 weight);
    
    // Errors
    error AssetNotSupported(string asset);
    error PriceStale(string asset, uint256 lastUpdate, uint256 maxAge);
    error PriceDeviation(string asset, uint256 deviation, uint256 threshold);
    error CircuitBreakerActive(string asset);
    error UnauthorizedUpdater(address updater);
    error InvalidPriceData(string asset);
    error EmergencyModeActive(string asset);
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        authorizedUpdaters[initialOwner] = true;
    }
    
    /**
     * @dev Add a new price feed
     */
    function addPriceFeed(
        string calldata asset,
        address aggregator,
        uint256 heartbeat,
        uint256 deviation,
        uint256 maxDeviation
    ) external onlyRole(ADMIN_ROLE) {
        AggregatorV3Interface feed = AggregatorV3Interface(aggregator);
        
        priceFeeds[asset] = PriceFeed({
            aggregator: aggregator,
            heartbeat: heartbeat,
            deviation: deviation,
            decimals: feed.decimals(),
            active: true,
            lastUpdate: 0,
            lastPrice: 0,
            description: feed.description()
        });
        
        // Initialize TWAP data
        twapData[asset] = TWAPData({
            cumulativePrice: 0,
            lastTimestamp: 0,
            windowSize: 1 hours, // Default 1 hour window
            priceHistory: new uint256[](0),
            timestampHistory: new uint256[](0),
            historyIndex: 0
        });
        
        maxPriceDeviation[asset] = maxDeviation;
        minUpdateInterval[asset] = 60; // 1 minute default
        
        // Add to supported assets if not already present
        bool found = false;
        for (uint256 i = 0; i < supportedAssets.length; i++) {
            if (keccak256(bytes(supportedAssets[i])) == keccak256(bytes(asset))) {
                found = true;
                break;
            }
        }
        if (!found) {
            supportedAssets.push(asset);
        }
        
        emit PriceFeedAdded(asset, aggregator, feed.decimals());
    }
    
    /**
     * @dev Update price from Chainlink feed
     */
    function updatePrice(string calldata asset) external {
        if (!authorizedUpdaters[msg.sender] && !hasRole(ADMIN_ROLE, msg.sender)) {
            revert UnauthorizedUpdater(msg.sender);
        }
        
        PriceFeed storage feed = priceFeeds[asset];
        if (!feed.active) {
            revert AssetNotSupported(asset);
        }
        
        // Check minimum update interval
        if (block.timestamp < lastUpdateTime[asset] + minUpdateInterval[asset]) {
            return; // Skip update if too soon
        }
        
        // Get price from Chainlink
        AggregatorV3Interface aggregator = AggregatorV3Interface(feed.aggregator);
        (, int256 price, , uint256 updatedAt, ) = aggregator.latestRoundData();
        
        // Validate price data
        if (price <= 0 || updatedAt == 0) {
            revert InvalidPriceData(asset);
        }
        
        // Check if price is stale
        if (block.timestamp > updatedAt + feed.heartbeat) {
            revert PriceStale(asset, updatedAt, feed.heartbeat);
        }
        
        uint256 newPrice = uint256(price);
        
        // Check for circuit breaker
        if (feed.lastPrice > 0) {
            uint256 deviation = _calculateDeviation(uint256(feed.lastPrice), newPrice);
            if (deviation > maxPriceDeviation[asset]) {
                circuitBreakerTriggered[asset] = true;
                emit CircuitBreakerTriggered(asset, uint256(feed.lastPrice), newPrice);
                revert CircuitBreakerActive(asset);
            }
        }
        
        // Update price data
        _updatePriceData(asset, newPrice, updatedAt);
        
        // Update feed data
        feed.lastUpdate = updatedAt;
        feed.lastPrice = price;
        lastUpdateTime[asset] = block.timestamp;
    }
    
    /**
     * @dev Update price manually (for authorized updaters)
     */
    function updatePriceManual(
        string calldata asset,
        uint256 price,
        uint256 confidence
    ) external {
        if (!authorizedUpdaters[msg.sender] && !hasRole(ADMIN_ROLE, msg.sender)) {
            revert UnauthorizedUpdater(msg.sender);
        }
        
        if (!_isAssetSupported(asset)) {
            revert AssetNotSupported(asset);
        }
        
        // Check circuit breaker if not in emergency mode
        if (!emergencyMode[asset] && lastValidPrice[asset] > 0) {
            uint256 deviation = _calculateDeviation(lastValidPrice[asset], price);
            if (deviation > maxPriceDeviation[asset]) {
                circuitBreakerTriggered[asset] = true;
                emit CircuitBreakerTriggered(asset, lastValidPrice[asset], price);
                revert CircuitBreakerActive(asset);
            }
        }
        
        _updatePriceData(asset, price, block.timestamp, confidence);
        lastUpdateTime[asset] = block.timestamp;
    }
    
    /**
     * @dev Get latest price for asset
     */
    function getPrice(string calldata asset) external view returns (uint256 price, uint256 timestamp) {
        if (emergencyMode[asset]) {
            return (emergencyPrices[asset], block.timestamp);
        }
        
        if (circuitBreakerTriggered[asset]) {
            revert CircuitBreakerActive(asset);
        }
        
        PriceData storage data = latestPrices[asset];
        if (!data.valid) {
            revert InvalidPriceData(asset);
        }
        
        return (data.price, data.timestamp);
    }
    
    /**
     * @dev Get TWAP price for asset
     */
    function getTWAP(string calldata asset, uint256 windowSize) external view returns (uint256) {
        TWAPData storage twap = twapData[asset];
        
        if (twap.priceHistory.length == 0) {
            // Fallback to latest price if no TWAP data
            return latestPrices[asset].price;
        }
        
        uint256 targetTime = block.timestamp - windowSize;
        uint256 weightedSum = 0;
        uint256 totalWeight = 0;
        
        for (uint256 i = 0; i < twap.priceHistory.length; i++) {
            if (twap.timestampHistory[i] >= targetTime) {
                uint256 weight = block.timestamp - twap.timestampHistory[i];
                weightedSum += twap.priceHistory[i] * weight;
                totalWeight += weight;
            }
        }
        
        return totalWeight > 0 ? weightedSum / totalWeight : latestPrices[asset].price;
    }
    
    /**
     * @dev Get aggregated price from multiple sources
     */
    function getAggregatedPrice(string calldata asset) external view returns (uint256) {
        address[] storage sources = priceSources[asset];
        uint256[] storage weights = sourceWeights[asset];
        
        if (sources.length == 0) {
            // Fallback to single price feed
            return latestPrices[asset].price;
        }
        
        uint256 weightedSum = 0;
        uint256 totalWeight = 0;
        uint256 validSources = 0;
        
        for (uint256 i = 0; i < sources.length; i++) {
            try AggregatorV3Interface(sources[i]).latestRoundData() returns (
                uint80, int256 price, uint256, uint256 updatedAt, uint80
            ) {
                if (price > 0 && block.timestamp <= updatedAt + priceFeeds[asset].heartbeat) {
                    weightedSum += uint256(price) * weights[i];
                    totalWeight += weights[i];
                    validSources++;
                }
            } catch {
                // Skip invalid source
            }
        }
        
        require(validSources > 0, "PriceOracle: No valid price sources");
        return weightedSum / totalWeight;
    }
    
    /**
     * @dev Get price history for asset
     */
    function getPriceHistory(string calldata asset, uint256 count) external view returns (
        uint256[] memory prices,
        uint256[] memory timestamps
    ) {
        uint256[] storage historyPrices = priceHistory[asset];
        uint256[] storage historyTimestamps = timestampHistory[asset];
        
        uint256 length = historyPrices.length < count ? historyPrices.length : count;
        prices = new uint256[](length);
        timestamps = new uint256[](length);
        
        for (uint256 i = 0; i < length; i++) {
            uint256 index = historyPrices.length - 1 - i;
            prices[i] = historyPrices[index];
            timestamps[i] = historyTimestamps[index];
        }
        
        return (prices, timestamps);
    }
    
    /**
     * @dev Add price source for aggregation
     */
    function addPriceSource(
        string calldata asset,
        address source,
        uint256 weight
    ) external onlyRole(ADMIN_ROLE) {
        priceSources[asset].push(source);
        sourceWeights[asset].push(weight);
        
        emit PriceSourceAdded(asset, source, weight);
    }
    
    /**
     * @dev Reset circuit breaker
     */
    function resetCircuitBreaker(string calldata asset) external onlyRole(ADMIN_ROLE) {
        circuitBreakerTriggered[asset] = false;
    }
    
    /**
     * @dev Activate emergency mode
     */
    function activateEmergencyMode(string calldata asset, uint256 emergencyPrice) external onlyRole(EMERGENCY_ROLE) {
        emergencyMode[asset] = true;
        emergencyPrices[asset] = emergencyPrice;
        
        emit EmergencyModeActivated(asset, emergencyPrice);
    }
    
    /**
     * @dev Deactivate emergency mode
     */
    function deactivateEmergencyMode(string calldata asset) external onlyRole(ADMIN_ROLE) {
        emergencyMode[asset] = false;
        emergencyPrices[asset] = 0;
    }
    
    /**
     * @dev Set TWAP window size
     */
    function setTWAPWindow(string calldata asset, uint256 windowSize) external onlyRole(ADMIN_ROLE) {
        twapData[asset].windowSize = windowSize;
    }
    
    /**
     * @dev Add authorized updater
     */
    function addAuthorizedUpdater(address updater) external onlyRole(ADMIN_ROLE) {
        authorizedUpdaters[updater] = true;
    }
    
    /**
     * @dev Remove authorized updater
     */
    function removeAuthorizedUpdater(address updater) external onlyRole(ADMIN_ROLE) {
        authorizedUpdaters[updater] = false;
    }
    
    /**
     * @dev Set minimum update interval
     */
    function setMinUpdateInterval(string calldata asset, uint256 interval) external onlyRole(ADMIN_ROLE) {
        minUpdateInterval[asset] = interval;
    }
    
    /**
     * @dev Set max price deviation
     */
    function setMaxPriceDeviation(string calldata asset, uint256 deviation) external onlyRole(ADMIN_ROLE) {
        maxPriceDeviation[asset] = deviation;
    }
    
    /**
     * @dev Internal function to update price data
     */
    function _updatePriceData(
        string calldata asset,
        uint256 price,
        uint256 timestamp,
        uint256 confidence
    ) internal {
        latestPrices[asset] = PriceData({
            price: price,
            timestamp: timestamp,
            confidence: confidence,
            volume: 0, // Can be extended to include volume
            valid: true
        });
        
        // Update TWAP
        _updateTWAP(asset, price, timestamp);
        
        // Update price history
        _updatePriceHistory(asset, price, timestamp);
        
        lastValidPrice[asset] = price;
        
        emit PriceUpdated(asset, price, timestamp, confidence);
    }
    
    /**
     * @dev Internal function to update price data (overload with default confidence)
     */
    function _updatePriceData(
        string calldata asset,
        uint256 price,
        uint256 timestamp
    ) internal {
        _updatePriceData(asset, price, timestamp, 10000); // Default max confidence
    }
    
    /**
     * @dev Update TWAP data
     */
    function _updateTWAP(string calldata asset, uint256 price, uint256 timestamp) internal {
        TWAPData storage twap = twapData[asset];
        
        if (twap.lastTimestamp > 0) {
            uint256 timeDiff = timestamp - twap.lastTimestamp;
            twap.cumulativePrice += price * timeDiff;
        }
        
        twap.lastTimestamp = timestamp;
        
        // Update circular buffer
        if (twap.priceHistory.length < MAX_HISTORY_SIZE) {
            twap.priceHistory.push(price);
            twap.timestampHistory.push(timestamp);
        } else {
            twap.priceHistory[twap.historyIndex] = price;
            twap.timestampHistory[twap.historyIndex] = timestamp;
            twap.historyIndex = (twap.historyIndex + 1) % MAX_HISTORY_SIZE;
        }
        
        emit TWAPUpdated(asset, price, twap.windowSize);
    }
    
    /**
     * @dev Update price history
     */
    function _updatePriceHistory(string calldata asset, uint256 price, uint256 timestamp) internal {
        if (priceHistory[asset].length >= MAX_HISTORY_SIZE) {
            // Remove oldest entry
            for (uint256 i = 0; i < priceHistory[asset].length - 1; i++) {
                priceHistory[asset][i] = priceHistory[asset][i + 1];
                timestampHistory[asset][i] = timestampHistory[asset][i + 1];
            }
            priceHistory[asset][priceHistory[asset].length - 1] = price;
            timestampHistory[asset][timestampHistory[asset].length - 1] = timestamp;
        } else {
            priceHistory[asset].push(price);
            timestampHistory[asset].push(timestamp);
        }
    }
    
    /**
     * @dev Calculate price deviation in basis points
     */
    function _calculateDeviation(uint256 oldPrice, uint256 newPrice) internal pure returns (uint256) {
        if (oldPrice == 0) return 0;
        
        uint256 diff = oldPrice > newPrice ? oldPrice - newPrice : newPrice - oldPrice;
        return (diff * 10000) / oldPrice;
    }
    
    /**
     * @dev Check if asset is supported
     */
    function _isAssetSupported(string calldata asset) internal view returns (bool) {
        for (uint256 i = 0; i < supportedAssets.length; i++) {
            if (keccak256(bytes(supportedAssets[i])) == keccak256(bytes(asset))) {
                return true;
            }
        }
        return false;
    }
    
    /**
     * @dev Get supported assets
     */
    function getSupportedAssets() external view returns (string[] memory) {
        return supportedAssets;
    }
    
    /**
     * @dev Check if price is stale
     */
    function isPriceStale(string calldata asset) external view returns (bool) {
        PriceFeed storage feed = priceFeeds[asset];
        return block.timestamp > feed.lastUpdate + feed.heartbeat;
    }
    
    /**
     * @dev Get price with validation
     */
    function getPriceWithValidation(string calldata asset) external view returns (
        uint256 price,
        uint256 timestamp,
        bool isValid,
        bool isStale
    ) {
        PriceData storage data = latestPrices[asset];
        PriceFeed storage feed = priceFeeds[asset];
        
        return (
            data.price,
            data.timestamp,
            data.valid && !circuitBreakerTriggered[asset] && !emergencyMode[asset],
            block.timestamp > feed.lastUpdate + feed.heartbeat
        );
    }
}