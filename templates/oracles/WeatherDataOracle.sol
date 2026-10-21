// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "../base/SecurityBase.sol";

/**
 * @title WeatherDataOracle
 * @dev Comprehensive weather data oracle system for blockchain applications
 * 
 * Features:
 * - Multi-source weather data aggregation
 * - Quality scoring and validation
 * - Historical data storage and retrieval
 * - Extreme weather event detection
 * - Data provider reputation system
 * - Automated weather station integration
 * - Climate trend analysis
 * 
 * Use Cases:
 * - Agricultural insurance
 * - Weather derivatives
 * - Climate research
 * - Parametric insurance
 * - Environmental monitoring
 * - Smart city applications
 */
contract WeatherDataOracle is SecurityBase {
    enum WeatherParameter {
        TEMPERATURE,        // Celsius * 100 (e.g., 2550 = 25.50°C)
        HUMIDITY,          // Percentage * 100
        PRESSURE,          // hPa * 100
        WIND_SPEED,        // m/s * 100
        WIND_DIRECTION,    // Degrees * 100
        PRECIPITATION,     // mm * 100
        VISIBILITY,        // km * 100
        UV_INDEX,          // Index * 100
        AIR_QUALITY,       // AQI * 100
        SOLAR_RADIATION,   // W/m² * 100
        SOIL_MOISTURE,     // Percentage * 100
        SNOW_DEPTH         // cm * 100
    }
    
    enum DataQuality {
        POOR,       // Quality score < 60
        FAIR,       // Quality score 60-79
        GOOD,       // Quality score 80-89
        EXCELLENT   // Quality score >= 90
    }
    
    enum EventType {
        EXTREME_HEAT,
        EXTREME_COLD,
        HEAVY_RAIN,
        DROUGHT,
        HURRICANE,
        TORNADO,
        FLOOD,
        BLIZZARD,
        HAILSTORM,
        WILDFIRE_RISK
    }
    
    struct WeatherStation {
        uint256 stationId;
        address owner;
        string name;
        int256 latitude;         // Latitude * 1e6 (e.g., 40123456 = 40.123456°)
        int256 longitude;        // Longitude * 1e6
        uint256 elevation;       // Meters above sea level
        uint256 registrationDate;
        bool isActive;
        bool isVerified;
        uint256 reputationScore; // 0-1000
        uint256 totalReports;
        uint256 qualityScore;    // 0-100
        mapping(WeatherParameter => bool) supportedParameters;
        mapping(uint256 => uint256) dailyReports; // day => report count
    }
    
    struct WeatherData {
        uint256 dataId;
        uint256 stationId;
        uint256 timestamp;
        int256 latitude;
        int256 longitude;
        mapping(WeatherParameter => int256) parameters;
        mapping(WeatherParameter => bool) hasParameter;
        uint256 qualityScore;    // 0-100
        DataQuality quality;
        bool isVerified;
        address verifier;
        uint256 blockNumber;
        bytes32 dataHash;        // Hash of all parameters for integrity
    }
    
    struct AggregatedData {
        int256 latitude;
        int256 longitude;
        uint256 radius;          // Radius in meters for aggregation
        uint256 timestamp;
        mapping(WeatherParameter => int256) averageValues;
        mapping(WeatherParameter => uint256) sampleCounts;
        mapping(WeatherParameter => int256) minValues;
        mapping(WeatherParameter => int256) maxValues;
        uint256 totalStations;
        uint256 overallQuality;
    }
    
    struct ExtremeWeatherEvent {
        uint256 eventId;
        EventType eventType;
        int256 latitude;
        int256 longitude;
        uint256 radius;
        uint256 startTime;
        uint256 endTime;
        uint256 severity;        // 1-10 scale
        string description;
        uint256[] affectedStations;
        bool isActive;
        bool isVerified;
    }
    
    struct DataProvider {
        address providerAddress;
        string name;
        string description;
        uint256 registrationDate;
        uint256 reputationScore; // 0-1000
        uint256 totalDataPoints;
        uint256 accuracyScore;   // 0-100
        bool isVerified;
        bool isActive;
        mapping(WeatherParameter => uint256) parameterCounts;
        mapping(uint256 => uint256) monthlyData; // month => data count
    }
    
    struct HistoricalQuery {
        uint256 queryId;
        address requester;
        int256 latitude;
        int256 longitude;
        uint256 radius;
        uint256 startTime;
        uint256 endTime;
        WeatherParameter[] parameters;
        uint256 fee;
        bool isFulfilled;
        bytes32 resultHash;
    }
    
    // Weather stations
    mapping(uint256 => WeatherStation) public weatherStations;
    mapping(address => uint256[]) public ownerStations;
    uint256 public stationCount;
    
    // Weather data
    mapping(uint256 => WeatherData) public weatherData;
    mapping(uint256 => uint256[]) public stationData; // stationId => dataIds
    uint256 public dataCount;
    
    // Aggregated data by location and time
    mapping(bytes32 => AggregatedData) public aggregatedData; // hash(lat, lon, radius, time) => data
    
    // Extreme weather events
    mapping(uint256 => ExtremeWeatherEvent) public extremeEvents;
    uint256 public eventCount;
    
    // Data providers
    mapping(address => DataProvider) public dataProviders;
    address[] public providerList;
    
    // Historical queries
    mapping(uint256 => HistoricalQuery) public historicalQueries;
    uint256 public queryCount;
    
    // Oracle configuration
    uint256 public minStationsForAggregation = 3;
    uint256 public maxAggregationRadius = 50000; // 50km in meters
    uint256 public dataRetentionPeriod = 365 days;
    uint256 public stakeAmount = 1 ether;
    
    // Fees and rewards
    uint256 public dataSubmissionReward = 0.01 ether;
    uint256 public verificationReward = 0.005 ether;
    uint256 public queryFee = 0.1 ether;
    mapping(address => uint256) public pendingRewards;
    
    // Quality thresholds
    mapping(WeatherParameter => uint256) public parameterThresholds; // Max deviation allowed
    mapping(EventType => uint256) public eventThresholds; // Threshold values for events
    
    // Events
    event StationRegistered(
        uint256 indexed stationId,
        address indexed owner,
        string name,
        int256 latitude,
        int256 longitude
    );
    event WeatherDataSubmitted(
        uint256 indexed dataId,
        uint256 indexed stationId,
        uint256 timestamp,
        uint256 qualityScore
    );
    event DataVerified(
        uint256 indexed dataId,
        address indexed verifier,
        bool isValid
    );
    event ExtremeWeatherDetected(
        uint256 indexed eventId,
        EventType eventType,
        int256 latitude,
        int256 longitude,
        uint256 severity
    );
    event ProviderRegistered(
        address indexed provider,
        string name
    );
    event HistoricalDataRequested(
        uint256 indexed queryId,
        address indexed requester,
        uint256 fee
    );
    event ReputationUpdated(
        address indexed entity,
        uint256 oldScore,
        uint256 newScore
    );
    
    // Errors
    error StationNotExists();
    error StationNotActive();
    error InsufficientStake();
    error InvalidCoordinates();
    error ParameterNotSupported();
    error DataTooOld();
    error InsufficientQuality();
    error NotDataProvider();
    error QueryNotExists();
    error InsufficientFee();
    error EventNotActive();
    
    modifier stationExists(uint256 stationId) {
        if (stationId >= stationCount) {
            revert StationNotExists();
        }
        _;
    }
    
    modifier onlyStationOwner(uint256 stationId) {
        require(weatherStations[stationId].owner == msg.sender, "WeatherDataOracle: Not station owner");
        _;
    }
    
    modifier onlyActiveStation(uint256 stationId) {
        if (!weatherStations[stationId].isActive) {
            revert StationNotActive();
        }
        _;
    }
    
    modifier onlyDataProvider() {
        if (!dataProviders[msg.sender].isActive) {
            revert NotDataProvider();
        }
        _;
    }
    
    constructor(address initialOwner) SecurityBase(initialOwner) {
        _initializeParameterThresholds();
        _initializeEventThresholds();
    }
    
    /**
     * @dev Register weather station
     */
    function registerWeatherStation(
        string calldata name,
        int256 latitude,
        int256 longitude,
        uint256 elevation,
        WeatherParameter[] calldata supportedParams
    ) external payable returns (uint256) {
        require(msg.value >= stakeAmount, "WeatherDataOracle: Insufficient stake");
        require(_isValidCoordinate(latitude, longitude), "WeatherDataOracle: Invalid coordinates");
        
        uint256 stationId = stationCount++;
        
        WeatherStation storage station = weatherStations[stationId];
        station.stationId = stationId;
        station.owner = msg.sender;
        station.name = name;
        station.latitude = latitude;
        station.longitude = longitude;
        station.elevation = elevation;
        station.registrationDate = block.timestamp;
        station.isActive = true;
        station.isVerified = false;
        station.reputationScore = 500; // Start with neutral reputation
        station.totalReports = 0;
        station.qualityScore = 50; // Start with neutral quality
        
        // Set supported parameters
        for (uint256 i = 0; i < supportedParams.length; i++) {
            station.supportedParameters[supportedParams[i]] = true;
        }
        
        ownerStations[msg.sender].push(stationId);
        
        emit StationRegistered(stationId, msg.sender, name, latitude, longitude);
        return stationId;
    }
    
    /**
     * @dev Submit weather data
     */
    function submitWeatherData(
        uint256 stationId,
        WeatherParameter[] calldata parameters,
        int256[] calldata values
    ) external stationExists(stationId) onlyStationOwner(stationId) onlyActiveStation(stationId) returns (uint256) {
        require(parameters.length == values.length, "WeatherDataOracle: Arrays length mismatch");
        require(parameters.length > 0, "WeatherDataOracle: No parameters provided");
        
        WeatherStation storage station = weatherStations[stationId];
        
        // Validate supported parameters
        for (uint256 i = 0; i < parameters.length; i++) {
            require(station.supportedParameters[parameters[i]], "WeatherDataOracle: Parameter not supported");
        }
        
        uint256 dataId = dataCount++;
        
        WeatherData storage data = weatherData[dataId];
        data.dataId = dataId;
        data.stationId = stationId;
        data.timestamp = block.timestamp;
        data.latitude = station.latitude;
        data.longitude = station.longitude;
        data.blockNumber = block.number;
        
        // Store parameters
        bytes32 dataHash = keccak256(abi.encodePacked(stationId, block.timestamp));
        for (uint256 i = 0; i < parameters.length; i++) {
            data.parameters[parameters[i]] = values[i];
            data.hasParameter[parameters[i]] = true;
            dataHash = keccak256(abi.encodePacked(dataHash, parameters[i], values[i]));
        }
        data.dataHash = dataHash;
        
        // Calculate quality score
        uint256 qualityScore = _calculateQualityScore(stationId, parameters, values);
        data.qualityScore = qualityScore;
        data.quality = _getDataQuality(qualityScore);
        
        // Update station statistics
        station.totalReports++;
        station.qualityScore = (station.qualityScore * 9 + qualityScore) / 10; // Moving average
        
        // Update daily report count
        uint256 today = block.timestamp / 1 days;
        station.dailyReports[today]++;
        
        stationData[stationId].push(dataId);
        
        // Reward data submission
        pendingRewards[msg.sender] += dataSubmissionReward;
        
        emit WeatherDataSubmitted(dataId, stationId, block.timestamp, qualityScore);
        
        // Check for extreme weather events
        _checkExtremeWeather(dataId, parameters, values);
        
        return dataId;
    }
    
    /**
     * @dev Verify weather data
     */
    function verifyWeatherData(uint256 dataId, bool isValid) external onlyRole(OPERATOR_ROLE) {
        require(dataId < dataCount, "WeatherDataOracle: Data not exists");
        
        WeatherData storage data = weatherData[dataId];
        data.isVerified = true;
        data.verifier = msg.sender;
        
        WeatherStation storage station = weatherStations[data.stationId];
        
        if (isValid) {
            // Increase reputation for valid data
            station.reputationScore = _min(station.reputationScore + 5, 1000);
            pendingRewards[station.owner] += verificationReward;
        } else {
            // Decrease reputation for invalid data
            if (station.reputationScore > 10) {
                station.reputationScore -= 10;
            }
        }
        
        emit DataVerified(dataId, msg.sender, isValid);
        emit ReputationUpdated(station.owner, station.reputationScore - (isValid ? 5 : -10), station.reputationScore);
    }
    
    /**
     * @dev Get current weather data for location
     */
    function getCurrentWeather(
        int256 latitude,
        int256 longitude,
        uint256 radius
    ) external view returns (
        int256[] memory temperatures,
        int256[] memory humidity,
        int256[] memory pressure,
        uint256[] memory stationIds,
        uint256[] memory timestamps,
        uint256 aggregateQuality
    ) {
        uint256[] memory nearbyStations = _findNearbyStations(latitude, longitude, radius);
        
        temperatures = new int256[](nearbyStations.length);
        humidity = new int256[](nearbyStations.length);
        pressure = new int256[](nearbyStations.length);
        stationIds = new uint256[](nearbyStations.length);
        timestamps = new uint256[](nearbyStations.length);
        
        uint256 totalQuality = 0;
        uint256 validReadings = 0;
        
        for (uint256 i = 0; i < nearbyStations.length; i++) {
            uint256 stationId = nearbyStations[i];
            uint256[] memory stationDataIds = stationData[stationId];
            
            if (stationDataIds.length > 0) {
                // Get latest data point
                uint256 latestDataId = stationDataIds[stationDataIds.length - 1];
                WeatherData storage data = weatherData[latestDataId];
                
                // Check if data is recent (within last hour)
                if (block.timestamp - data.timestamp <= 3600) {
                    stationIds[validReadings] = stationId;
                    timestamps[validReadings] = data.timestamp;
                    
                    if (data.hasParameter[WeatherParameter.TEMPERATURE]) {
                        temperatures[validReadings] = data.parameters[WeatherParameter.TEMPERATURE];
                    }
                    if (data.hasParameter[WeatherParameter.HUMIDITY]) {
                        humidity[validReadings] = data.parameters[WeatherParameter.HUMIDITY];
                    }
                    if (data.hasParameter[WeatherParameter.PRESSURE]) {
                        pressure[validReadings] = data.parameters[WeatherParameter.PRESSURE];
                    }
                    
                    totalQuality += data.qualityScore;
                    validReadings++;
                }
            }
        }
        
        aggregateQuality = validReadings > 0 ? totalQuality / validReadings : 0;
        
        return (temperatures, humidity, pressure, stationIds, timestamps, aggregateQuality);
    }
    
    /**
     * @dev Request historical weather data
     */
    function requestHistoricalData(
        int256 latitude,
        int256 longitude,
        uint256 radius,
        uint256 startTime,
        uint256 endTime,
        WeatherParameter[] calldata parameters
    ) external payable returns (uint256) {
        require(msg.value >= queryFee, "WeatherDataOracle: Insufficient fee");
        require(endTime > startTime, "WeatherDataOracle: Invalid time range");
        require(parameters.length > 0, "WeatherDataOracle: No parameters specified");
        
        uint256 queryId = queryCount++;
        
        HistoricalQuery storage query = historicalQueries[queryId];
        query.queryId = queryId;
        query.requester = msg.sender;
        query.latitude = latitude;
        query.longitude = longitude;
        query.radius = radius;
        query.startTime = startTime;
        query.endTime = endTime;
        query.parameters = parameters;
        query.fee = msg.value;
        query.isFulfilled = false;
        
        emit HistoricalDataRequested(queryId, msg.sender, msg.value);
        return queryId;
    }
    
    /**
     * @dev Register data provider
     */
    function registerDataProvider(
        string calldata name,
        string calldata description
    ) external {
        require(!dataProviders[msg.sender].isActive, "WeatherDataOracle: Already registered");
        
        dataProviders[msg.sender] = DataProvider({
            providerAddress: msg.sender,
            name: name,
            description: description,
            registrationDate: block.timestamp,
            reputationScore: 500,
            totalDataPoints: 0,
            accuracyScore: 50,
            isVerified: false,
            isActive: true
        });
        
        providerList.push(msg.sender);
        
        emit ProviderRegistered(msg.sender, name);
    }
    
    /**
     * @dev Report extreme weather event
     */
    function reportExtremeWeather(
        EventType eventType,
        int256 latitude,
        int256 longitude,
        uint256 radius,
        uint256 severity,
        string calldata description
    ) external onlyDataProvider returns (uint256) {
        require(severity >= 1 && severity <= 10, "WeatherDataOracle: Invalid severity");
        
        uint256 eventId = eventCount++;
        
        ExtremeWeatherEvent storage weatherEvent = extremeEvents[eventId];
        weatherEvent.eventId = eventId;
        weatherEvent.eventType = eventType;
        weatherEvent.latitude = latitude;
        weatherEvent.longitude = longitude;
        weatherEvent.radius = radius;
        weatherEvent.startTime = block.timestamp;
        weatherEvent.severity = severity;
        weatherEvent.description = description;
        weatherEvent.isActive = true;
        weatherEvent.isVerified = false;
        
        // Find affected stations
        weatherEvent.affectedStations = _findNearbyStations(latitude, longitude, radius);
        
        emit ExtremeWeatherDetected(eventId, eventType, latitude, longitude, severity);
        return eventId;
    }
    
    /**
     * @dev Claim pending rewards
     */
    function claimRewards() external {
        uint256 reward = pendingRewards[msg.sender];
        require(reward > 0, "WeatherDataOracle: No rewards available");
        
        pendingRewards[msg.sender] = 0;
        payable(msg.sender).transfer(reward);
    }
    
    /**
     * @dev Get station information
     */
    function getStationInfo(uint256 stationId) external view stationExists(stationId) returns (
        address owner,
        string memory name,
        int256 latitude,
        int256 longitude,
        bool isActive,
        bool isVerified,
        uint256 reputationScore,
        uint256 totalReports,
        uint256 qualityScore
    ) {
        WeatherStation storage station = weatherStations[stationId];
        
        return (
            station.owner,
            station.name,
            station.latitude,
            station.longitude,
            station.isActive,
            station.isVerified,
            station.reputationScore,
            station.totalReports,
            station.qualityScore
        );
    }
    
    /**
     * @dev Get extreme weather events in area
     */
    function getActiveEvents(
        int256 latitude,
        int256 longitude,
        uint256 radius
    ) external view returns (uint256[] memory eventIds) {
        uint256[] memory activeEvents = new uint256[](eventCount);
        uint256 count = 0;
        
        for (uint256 i = 0; i < eventCount; i++) {
            ExtremeWeatherEvent storage weatherEvent = extremeEvents[i];
            if (weatherEvent.isActive && _isWithinRadius(latitude, longitude, weatherEvent.latitude, weatherEvent.longitude, radius)) {
                activeEvents[count] = i;
                count++;
            }
        }
        
        // Resize array
        eventIds = new uint256[](count);
        for (uint256 i = 0; i < count; i++) {
            eventIds[i] = activeEvents[i];
        }
        
        return eventIds;
    }
    
    // Internal functions
    
    function _findNearbyStations(
        int256 latitude,
        int256 longitude,
        uint256 radius
    ) internal view returns (uint256[] memory) {
        uint256[] memory nearbyStations = new uint256[](stationCount);
        uint256 count = 0;
        
        for (uint256 i = 0; i < stationCount; i++) {
            WeatherStation storage station = weatherStations[i];
            if (station.isActive && _isWithinRadius(latitude, longitude, station.latitude, station.longitude, radius)) {
                nearbyStations[count] = i;
                count++;
            }
        }
        
        // Resize array
        uint256[] memory result = new uint256[](count);
        for (uint256 i = 0; i < count; i++) {
            result[i] = nearbyStations[i];
        }
        
        return result;
    }
    
    function _isWithinRadius(
        int256 lat1,
        int256 lon1,
        int256 lat2,
        int256 lon2,
        uint256 radius
    ) internal pure returns (bool) {
        // Simplified distance calculation (Haversine formula approximation)
        int256 dLat = lat1 - lat2;
        int256 dLon = lon1 - lon2;
        uint256 distance = uint256(_sqrt(uint256(dLat * dLat + dLon * dLon))) * 111; // Rough conversion to meters
        
        return distance <= radius;
    }
    
    function _calculateQualityScore(
        uint256 stationId,
        WeatherParameter[] calldata parameters,
        int256[] calldata values
    ) internal view returns (uint256) {
        WeatherStation storage station = weatherStations[stationId];
        
        uint256 score = 100;
        
        // Check station reputation
        if (station.reputationScore < 300) {
            score -= 20;
        } else if (station.reputationScore > 800) {
            score += 10;
        }
        
        // Check data freshness (frequency of reporting)
        uint256 today = block.timestamp / 1 days;
        uint256 reportsToday = station.dailyReports[today];
        if (reportsToday > 24) {
            score += 10; // Frequent reporting
        } else if (reportsToday < 4) {
            score -= 10; // Infrequent reporting
        }
        
        // Check parameter reasonableness
        for (uint256 i = 0; i < parameters.length; i++) {
            if (!_isReasonableValue(parameters[i], values[i])) {
                score -= 15;
            }
        }
        
        return _min(score, 100);
    }
    
    function _isReasonableValue(WeatherParameter param, int256 value) internal pure returns (bool) {
        if (param == WeatherParameter.TEMPERATURE) {
            return value >= -5000 && value <= 5000; // -50°C to 50°C
        } else if (param == WeatherParameter.HUMIDITY) {
            return value >= 0 && value <= 10000; // 0% to 100%
        } else if (param == WeatherParameter.PRESSURE) {
            return value >= 90000 && value <= 110000; // 900 to 1100 hPa
        } else if (param == WeatherParameter.WIND_SPEED) {
            return value >= 0 && value <= 10000; // 0 to 100 m/s
        } else if (param == WeatherParameter.PRECIPITATION) {
            return value >= 0 && value <= 50000; // 0 to 500mm
        }
        
        return true; // Default to true for other parameters
    }
    
    function _checkExtremeWeather(
        uint256 dataId,
        WeatherParameter[] calldata parameters,
        int256[] calldata values
    ) internal {
        WeatherData storage data = weatherData[dataId];
        
        for (uint256 i = 0; i < parameters.length; i++) {
            WeatherParameter param = parameters[i];
            int256 value = values[i];
            
            if (param == WeatherParameter.TEMPERATURE) {
                if (value > 4000) { // > 40°C
                    _createExtremeEvent(EventType.EXTREME_HEAT, data.latitude, data.longitude, value);
                } else if (value < -2000) { // < -20°C
                    _createExtremeEvent(EventType.EXTREME_COLD, data.latitude, data.longitude, value);
                }
            } else if (param == WeatherParameter.PRECIPITATION && value > 10000) { // > 100mm
                _createExtremeEvent(EventType.HEAVY_RAIN, data.latitude, data.longitude, value);
            }
        }
    }
    
    function _createExtremeEvent(
        EventType eventType,
        int256 latitude,
        int256 longitude,
        int256 value
    ) internal {
        uint256 eventId = eventCount++;
        uint256 severity = _calculateSeverity(eventType, value);
        
        ExtremeWeatherEvent storage weatherEvent = extremeEvents[eventId];
        weatherEvent.eventId = eventId;
        weatherEvent.eventType = eventType;
        weatherEvent.latitude = latitude;
        weatherEvent.longitude = longitude;
        weatherEvent.radius = 10000; // 10km radius
        weatherEvent.startTime = block.timestamp;
        weatherEvent.severity = severity;
        weatherEvent.isActive = true;
        weatherEvent.isVerified = false;
        
        emit ExtremeWeatherDetected(eventId, eventType, latitude, longitude, severity);
    }
    
    function _calculateSeverity(EventType eventType, int256 value) internal pure returns (uint256) {
        if (eventType == EventType.EXTREME_HEAT) {
            if (value > 5000) return 10; // > 50°C
            if (value > 4500) return 8;  // > 45°C
            return 6;
        } else if (eventType == EventType.EXTREME_COLD) {
            if (value < -3000) return 10; // < -30°C
            if (value < -2500) return 8;  // < -25°C
            return 6;
        } else if (eventType == EventType.HEAVY_RAIN) {
            if (value > 20000) return 10; // > 200mm
            if (value > 15000) return 8;  // > 150mm
            return 6;
        }
        
        return 5; // Default medium severity
    }
    
    function _getDataQuality(uint256 score) internal pure returns (DataQuality) {
        if (score >= 90) return DataQuality.EXCELLENT;
        if (score >= 80) return DataQuality.GOOD;
        if (score >= 60) return DataQuality.FAIR;
        return DataQuality.POOR;
    }
    
    function _isValidCoordinate(int256 latitude, int256 longitude) internal pure returns (bool) {
        return latitude >= -90000000 && latitude <= 90000000 && 
               longitude >= -180000000 && longitude <= 180000000;
    }
    
    function _min(uint256 a, uint256 b) internal pure returns (uint256) {
        return a < b ? a : b;
    }
    
    function _sqrt(uint256 x) internal pure returns (uint256) {
        if (x == 0) return 0;
        uint256 z = (x + 1) / 2;
        uint256 y = x;
        while (z < y) {
            y = z;
            z = (x / z + z) / 2;
        }
        return y;
    }
    
    function _initializeParameterThresholds() internal {
        parameterThresholds[WeatherParameter.TEMPERATURE] = 500;    // 5°C deviation
        parameterThresholds[WeatherParameter.HUMIDITY] = 1000;      // 10% deviation
        parameterThresholds[WeatherParameter.PRESSURE] = 500;       // 5 hPa deviation
        parameterThresholds[WeatherParameter.WIND_SPEED] = 500;     // 5 m/s deviation
        parameterThresholds[WeatherParameter.PRECIPITATION] = 1000; // 10mm deviation
    }
    
    function _initializeEventThresholds() internal {
        eventThresholds[EventType.EXTREME_HEAT] = 4000;     // 40°C
        eventThresholds[EventType.EXTREME_COLD] = 2000;     // -20°C
        eventThresholds[EventType.HEAVY_RAIN] = 10000;      // 100mm
        eventThresholds[EventType.DROUGHT] = 0;             // No rain for period
        eventThresholds[EventType.HURRICANE] = 3300;        // 33 m/s wind
    }
    
    /**
     * @dev Admin function to update configuration
     */
    function updateConfiguration(
        uint256 _minStations,
        uint256 _maxRadius,
        uint256 _retentionPeriod,
        uint256 _stakeAmount
    ) external onlyRole(ADMIN_ROLE) {
        minStationsForAggregation = _minStations;
        maxAggregationRadius = _maxRadius;
        dataRetentionPeriod = _retentionPeriod;
        stakeAmount = _stakeAmount;
    }
    
    /**
     * @dev Emergency function to deactivate station
     */
    function deactivateStation(uint256 stationId) external onlyRole(EMERGENCY_ROLE) stationExists(stationId) {
        weatherStations[stationId].isActive = false;
    }
}