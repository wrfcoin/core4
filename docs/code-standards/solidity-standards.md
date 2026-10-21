# Solidity Smart Contract Commenting Standards

This guide provides comprehensive commenting standards for Solidity smart contracts in the WRFCoin
platform.

## NatSpec Documentation

### Contract Documentation

Use NatSpec comments for all contracts with comprehensive descriptions:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/AccessControl.sol";

/**
 * @title WRFCoin Carbon Credit Exchange
 * @author WRFCoin Development Team
 * @notice Decentralized exchange for trading carbon credits backed by verified weather data
 * @dev Implements automated market maker (AMM) with weather-based pricing algorithms
 *
 * This contract enables the trading of carbon credits that are generated and verified
 * through the WRFCoin weather data network. Each carbon credit represents a verified
 * amount of carbon sequestration or emission reduction based on meteorological data.
 *
 * Key Features:
 * - Automated market making with dynamic pricing
 * - Weather data integration for credit verification
 * - Time-locked staking for long-term commitments
 * - Governance token distribution to participants
 * - Integration with cross-chain bridges for multi-chain trading
 *
 * Security Considerations:
 * - Reentrancy protection on all state-changing functions
 * - Role-based access control for administrative functions
 * - Input validation and overflow protection
 * - Emergency pause functionality for critical issues
 *
 * Gas Optimization:
 * - Batch operations for multiple trades
 * - Packed structs to minimize storage costs
 * - Event-based state tracking to reduce storage reads
 *
 * @custom:version 2.1.0
 * @custom:security-contact security@wrfcoin.com
 * @custom:audited-by OpenZeppelin, Consensys Diligence
 */
contract CarbonCreditExchange is ERC20, ReentrancyGuard, AccessControl {
    // Contract implementation...
}
```

### Function Documentation

Use comprehensive NatSpec for all public and external functions:

````solidity
/**
 * @notice Swaps carbon credits for WRF tokens using AMM pricing
 * @dev Implements constant product formula (x * y = k) with weather-adjusted fees
 *
 * The swap function calculates pricing based on current liquidity pools and applies
 * dynamic fees that adjust based on real-time weather data. During extreme weather
 * events, trading fees increase to account for higher carbon credit demand.
 *
 * Requirements:
 * - Credit amount must be greater than minimum trade size (100 credits)
 * - User must have approved this contract to transfer credits
 * - Liquidity pool must have sufficient WRF tokens for the swap
 * - Weather oracle must be operational for fee calculation
 *
 * @param creditAmount Amount of carbon credits to swap (in base units)
 * @param minTokensOut Minimum WRF tokens expected (slippage protection)
 * @param weatherStationId ID of weather station for regional pricing
 * @param deadline Transaction deadline timestamp (prevents stale transactions)
 *
 * @return tokensOut Actual amount of WRF tokens received
 * @return effectiveFeeRate Applied fee rate (in basis points)
 *
 * @custom:throws InsufficientCredits When user doesn't have enough credits
 * @custom:throws SlippageExceeded When price moves beyond tolerance
 * @custom:throws DeadlineExpired When transaction is submitted too late
 * @custom:throws WeatherOracleOffline When weather data is unavailable
 * @custom:throws LiquidityInsufficient When pool lacks adequate reserves
 *
 * @custom:gas-estimate 85000 (without weather data), 120000 (with weather data)
 * @custom:example
 * ```solidity
 * // Swap 1000 carbon credits for WRF tokens
 * uint256 tokens = exchange.swapCreditsForTokens(
 *     1000 * 10**18,           // 1000 credits
 *     950 * 10**18,            // Minimum 950 WRF tokens (5% slippage)
 *     "tokyo-weather-001",     // Tokyo weather station
 *     block.timestamp + 300    // 5 minute deadline
 * );
 * ```
 */
function swapCreditsForTokens(
    uint256 creditAmount,
    uint256 minTokensOut,
    string calldata weatherStationId,
    uint256 deadline
) external nonReentrant returns (uint256 tokensOut, uint256 effectiveFeeRate) {
    // VALIDATION: Input parameter validation
    require(creditAmount >= MINIMUM_TRADE_SIZE, "CarbonExchange: Amount below minimum");
    require(minTokensOut > 0, "CarbonExchange: Invalid minimum output");
    require(deadline >= block.timestamp, "CarbonExchange: Transaction expired");

    // SECURITY: Verify user has approved credit transfer
    require(
        carbonCredits.allowance(msg.sender, address(this)) >= creditAmount,
        "CarbonExchange: Insufficient allowance"
    );

    // PRICING: Calculate swap amount using AMM formula with weather adjustments
    (tokensOut, effectiveFeeRate) = _calculateSwapOutput(
        creditAmount,
        weatherStationId
    );

    // SLIPPAGE: Verify output meets minimum requirement
    require(tokensOut >= minTokensOut, "CarbonExchange: Slippage exceeded");

    // LIQUIDITY: Verify pool has sufficient tokens
    require(
        wrfToken.balanceOf(address(this)) >= tokensOut,
        "CarbonExchange: Insufficient liquidity"
    );

    // ATOMICITY: Execute the swap
    _executeSwap(msg.sender, creditAmount, tokensOut);

    // EVENTS: Emit swap event for off-chain tracking
    emit CreditSwap(
        msg.sender,
        creditAmount,
        tokensOut,
        effectiveFeeRate,
        weatherStationId,
        block.timestamp
    );
}
````

### Event Documentation

Document events with parameter explanations and usage context:

```solidity
/**
 * @notice Emitted when carbon credits are swapped for WRF tokens
 * @dev Used by off-chain systems for portfolio tracking and analytics
 *
 * This event provides comprehensive information about credit swaps for:
 * - Portfolio management systems
 * - Tax reporting and compliance
 * - Market analytics and reporting
 * - Regulatory audit trails
 *
 * @param user Address of the user performing the swap
 * @param creditAmount Amount of carbon credits swapped (base units)
 * @param tokenAmount Amount of WRF tokens received (base units)
 * @param feeRate Applied fee rate in basis points (e.g., 250 = 2.5%)
 * @param weatherStation Weather station ID used for pricing calculation
 * @param timestamp Block timestamp when swap was executed
 *
 * @custom:indexing user, weatherStation
 * @custom:gas-cost ~2000 gas for emission
 */
event CreditSwap(
    address indexed user,
    uint256 creditAmount,
    uint256 tokenAmount,
    uint256 feeRate,
    string indexed weatherStation,
    uint256 timestamp
);

/**
 * @notice Emitted when weather data affects carbon credit pricing
 * @dev Critical for understanding price volatility and market dynamics
 *
 * Weather events can significantly impact carbon credit demand and pricing.
 * This event tracks price adjustments based on meteorological conditions.
 *
 * @param stationId Weather station reporting the data
 * @param eventType Type of weather event (storm, heatwave, drought, etc.)
 * @param severity Event severity score (1-10 scale)
 * @param priceMultiplier Pricing adjustment factor (1e18 = no change)
 * @param duration Expected duration of price impact (in seconds)
 *
 * @custom:indexing stationId, eventType
 * @custom:monitoring High severity events (>7) trigger automated alerts
 */
event WeatherPriceAdjustment(
    string indexed stationId,
    string indexed eventType,
    uint8 severity,
    uint256 priceMultiplier,
    uint256 duration
);
```

### Modifier Documentation

Document modifiers with their purpose and security considerations:

```solidity
/**
 * @notice Ensures function can only be called during active trading hours
 * @dev Prevents trading during maintenance windows and emergency shutdowns
 *
 * Trading hours are configurable by governance and can be adjusted for:
 * - Regular maintenance windows
 * - Emergency security responses
 * - Regulatory compliance requirements
 * - Cross-chain bridge maintenance
 *
 * Default trading hours: 24/7 with 4-hour maintenance window weekly
 *
 * @custom:gas-cost ~1500 gas overhead
 * @custom:security Prevents sandwich attacks during low-liquidity periods
 */
modifier onlyDuringTradingHours() {
    require(_isTradingActive(), "CarbonExchange: Trading suspended");
    require(!_isMaintenanceMode(), "CarbonExchange: Maintenance in progress");
    _;
}

/**
 * @notice Validates weather station exists and is currently operational
 * @dev Ensures pricing calculations use reliable weather data sources
 *
 * Weather station validation includes:
 * - Station registration status
 * - Data freshness (< 1 hour old)
 * - Station operational status
 * - Data quality score (> 80%)
 *
 * @param stationId Weather station identifier to validate
 *
 * @custom:gas-cost ~3000 gas for validation checks
 * @custom:fallback Uses regional average if station unavailable
 */
modifier validWeatherStation(string calldata stationId) {
    require(
        weatherOracle.isStationOperational(stationId),
        "CarbonExchange: Weather station offline"
    );
    require(
        weatherOracle.getDataAge(stationId) < 1 hours,
        "CarbonExchange: Weather data stale"
    );
    _;
}
```

## Inline Comments

### Gas Optimization Comments

Document gas optimization strategies and trade-offs:

```solidity
contract GasOptimizedCarbonTracker {
    // STORAGE: Pack multiple values into single storage slot (saves ~15,000 gas per operation)
    // Trade-off: Slightly more complex access patterns vs. significant gas savings
    struct PackedCreditInfo {
        uint128 amount;        // Sufficient for 3.4 * 10^38 credits
        uint64 timestamp;      // Unix timestamp (valid until year 2554)
        uint32 stationId;      // Weather station numeric ID
        uint16 qualityScore;   // Quality score 0-10000 (basis points)
        uint8 creditType;      // Credit type enum (0-255 types)
        uint8 verified;        // Boolean packed as uint8
    }

    // OPTIMIZATION: Use mapping instead of array for O(1) lookups vs O(n)
    // Memory: ~32 bytes per entry vs potential gas savings of 10x on reads
    mapping(bytes32 => PackedCreditInfo) public creditDatabase;

    /**
     * @notice Batch processes multiple carbon credit registrations
     * @dev Gas optimization: Process multiple credits in single transaction
     *
     * Batch processing reduces transaction overhead from ~21,000 gas per transaction
     * to ~3,000 gas per additional credit in the batch.
     *
     * @param credits Array of credit data to process
     * @custom:gas-estimate 60000 + (credits.length * 3000)
     */
    function batchRegisterCredits(CreditData[] calldata credits) external {
        uint256 length = credits.length;

        // GAS: Cache array length to avoid repeated SLOAD operations
        // Saves ~200 gas per iteration in loop condition checks

        // GAS: Use unchecked arithmetic where overflow is impossible
        // Array length bounds prevent overflow in loop counter
        unchecked {
            for (uint256 i = 0; i < length; ++i) {
                CreditData calldata credit = credits[i];

                // GAS: Calculate hash once and reuse
                // Avoids duplicate keccak256 operations (3000 gas each)
                bytes32 creditHash = keccak256(
                    abi.encodePacked(credit.stationId, credit.timestamp, credit.amount)
                );

                // VALIDATION: Ensure credit doesn't already exist
                require(
                    creditDatabase[creditHash].amount == 0,
                    "CarbonTracker: Credit already registered"
                );

                // STORAGE: Pack all data into single storage slot
                // Single SSTORE operation (~20,000 gas) vs multiple operations (~60,000+ gas)
                creditDatabase[creditHash] = PackedCreditInfo({
                    amount: uint128(credit.amount),
                    timestamp: uint64(credit.timestamp),
                    stationId: uint32(credit.stationId),
                    qualityScore: uint16(credit.qualityScore),
                    creditType: uint8(credit.creditType),
                    verified: uint8(credit.verified ? 1 : 0)
                });

                // EVENTS: Emit minimal event data to reduce gas costs
                // Each indexed parameter adds ~375 gas, data parameters ~8 gas per byte
                emit CreditRegistered(creditHash, credit.amount, credit.stationId);
            }
        }

        // METRICS: Update batch processing statistics
        _updateBatchMetrics(length);
    }

    /**
     * @notice Efficiently calculates total credits for multiple stations
     * @dev Uses assembly for gas-optimized calculations
     */
    function calculateTotalCredits(uint32[] calldata stationIds)
        external
        view
        returns (uint256 total)
    {
        // GAS: Assembly optimization for tight loops and arithmetic
        // Reduces gas cost by ~30% for large station lists
        assembly {
            let length := stationIds.length
            let data := add(stationIds.offset, 0x20)

            // LOOP: Optimized iteration without Solidity overhead
            for { let i := 0 } lt(i, length) { i := add(i, 1) } {
                let stationId := calldataload(add(data, mul(i, 0x20)))

                // STORAGE: Direct storage access bypassing Solidity checks
                let slot := keccak256(stationId, 0x00)
                let credits := sload(slot)

                // ARITHMETIC: Unchecked addition (overflow handled at application level)
                total := add(total, credits)
            }
        }
    }
}
```

### Security and Reentrancy Comments

Document security measures and attack prevention:

```solidity
contract SecureCarbonVault {
    using SafeERC20 for IERC20;

    // SECURITY: ReentrancyGuard prevents reentrancy attacks on withdrawal functions
    // Mutex pattern ensures single execution context for state-changing operations
    bool private _reentrancyLock;

    // SECURITY: Time-locked withdrawals prevent flash loan attacks
    // Minimum delay allows for monitoring and intervention in suspicious activity
    mapping(address => uint256) public withdrawalRequests;
    uint256 public constant WITHDRAWAL_DELAY = 24 hours;

    /**
     * @notice Deposits carbon credits into secure vault with validation
     * @dev Implements comprehensive security checks for deposit operations
     */
    function deposit(uint256 amount) external nonReentrant {
        // INPUT VALIDATION: Prevent zero-value and overflow attacks
        require(amount > 0, "CarbonVault: Invalid amount");
        require(amount <= type(uint128).max, "CarbonVault: Amount too large");

        // SECURITY: Verify caller owns sufficient credits before state changes
        // Prevents griefing attacks with insufficient balance
        uint256 userBalance = carbonCredits.balanceOf(msg.sender);
        require(userBalance >= amount, "CarbonVault: Insufficient balance");

        // APPROVAL CHECK: Verify contract can transfer credits
        uint256 allowance = carbonCredits.allowance(msg.sender, address(this));
        require(allowance >= amount, "CarbonVault: Insufficient allowance");

        // REENTRANCY: Use CEI pattern (Checks, Effects, Interactions)
        // Update state before external calls to prevent reentrancy
        vaultBalances[msg.sender] += amount;
        totalDeposited += amount;

        // EXTERNAL INTERACTION: Safe transfer with revert on failure
        // transferFrom reverts on failure, maintaining transaction atomicity
        carbonCredits.safeTransferFrom(msg.sender, address(this), amount);

        // AUDIT TRAIL: Emit event for transparency and monitoring
        emit Deposit(msg.sender, amount, block.timestamp);
    }

    /**
     * @notice Initiates time-locked withdrawal request
     * @dev Two-phase withdrawal prevents flash loan and MEV attacks
     */
    function requestWithdrawal(uint256 amount) external {
        // AUTHORIZATION: Verify user has sufficient vault balance
        require(
            vaultBalances[msg.sender] >= amount,
            "CarbonVault: Insufficient vault balance"
        );

        // SECURITY: Prevent withdrawal request spam
        require(
            withdrawalRequests[msg.sender] == 0,
            "CarbonVault: Pending withdrawal exists"
        );

        // TIME LOCK: Set withdrawal eligibility timestamp
        // Prevents immediate withdrawal and allows monitoring
        withdrawalRequests[msg.sender] = block.timestamp + WITHDRAWAL_DELAY;

        emit WithdrawalRequested(msg.sender, amount, block.timestamp + WITHDRAWAL_DELAY);
    }

    /**
     * @notice Executes withdrawal after time lock expires
     * @dev Final withdrawal with comprehensive security validations
     */
    function executeWithdrawal(uint256 amount) external nonReentrant {
        uint256 withdrawalTime = withdrawalRequests[msg.sender];

        // TIME LOCK: Verify withdrawal delay has passed
        require(withdrawalTime != 0, "CarbonVault: No withdrawal requested");
        require(
            block.timestamp >= withdrawalTime,
            "CarbonVault: Withdrawal not yet available"
        );

        // BALANCE CHECK: Verify user still has sufficient vault balance
        // Prevents race conditions with other transactions
        require(
            vaultBalances[msg.sender] >= amount,
            "CarbonVault: Insufficient vault balance"
        );

        // SECURITY: Clear withdrawal request to prevent double-spending
        withdrawalRequests[msg.sender] = 0;

        // CEI PATTERN: Update state before external interactions
        vaultBalances[msg.sender] -= amount;
        totalDeposited -= amount;

        // EXTERNAL INTERACTION: Safe transfer to user
        carbonCredits.safeTransfer(msg.sender, amount);

        emit Withdrawal(msg.sender, amount, block.timestamp);
    }

    /**
     * @notice Emergency pause function for critical security situations
     * @dev Can only be called by emergency responders in governance multisig
     */
    function emergencyPause() external {
        // ACCESS CONTROL: Verify caller has emergency response authority
        require(
            hasRole(EMERGENCY_ROLE, msg.sender),
            "CarbonVault: Unauthorized emergency action"
        );

        // SECURITY: Immediately halt all state-changing operations
        _pause();

        // TRANSPARENCY: Emit emergency event for public visibility
        emit EmergencyPause(msg.sender, block.timestamp);
    }
}
```

### Integration and Oracle Comments

Document external system integrations and data feeds:

```solidity
contract WeatherIntegratedPricing {
    // ORACLE: Chainlink price feeds for reliable external data
    // Multiple oracles prevent single point of failure
    AggregatorV3Interface internal priceFeed;
    AggregatorV3Interface internal weatherDataFeed;

    // FALLBACK: Backup oracle system for high availability
    // Used when primary oracle is unavailable or stale
    IBackupOracle internal backupOracle;

    // CIRCUIT BREAKER: Price deviation limits prevent oracle manipulation
    uint256 public constant MAX_PRICE_DEVIATION = 1000; // 10% in basis points
    uint256 public constant STALE_DATA_THRESHOLD = 3600; // 1 hour

    /**
     * @notice Gets weather-adjusted carbon credit price
     * @dev Integrates multiple data sources with fallback mechanisms
     */
    function getWeatherAdjustedPrice(string calldata stationId)
        external
        view
        returns (uint256 price, uint256 confidence)
    {
        // PRIMARY ORACLE: Fetch current carbon credit base price
        (
            uint80 roundId,
            int256 basePrice,
            uint256 startedAt,
            uint256 updatedAt,
            uint80 answeredInRound
        ) = priceFeed.latestRoundData();

        // ORACLE VALIDATION: Verify price data freshness and validity
        require(basePrice > 0, "WeatherPricing: Invalid base price");
        require(
            block.timestamp - updatedAt <= STALE_DATA_THRESHOLD,
            "WeatherPricing: Stale price data"
        );
        require(
            answeredInRound >= roundId,
            "WeatherPricing: Price data inconsistent"
        );

        // WEATHER DATA: Fetch current weather conditions for pricing adjustment
        WeatherData memory weather = _getWeatherData(stationId);

        // ALGORITHM: Calculate weather impact multiplier
        // Extreme weather increases carbon credit demand and pricing
        uint256 weatherMultiplier = _calculateWeatherMultiplier(weather);

        // CIRCUIT BREAKER: Limit price adjustments to prevent manipulation
        if (weatherMultiplier > (10000 + MAX_PRICE_DEVIATION)) {
            weatherMultiplier = 10000 + MAX_PRICE_DEVIATION;
        } else if (weatherMultiplier < (10000 - MAX_PRICE_DEVIATION)) {
            weatherMultiplier = 10000 - MAX_PRICE_DEVIATION;
        }

        // CALCULATION: Apply weather adjustment to base price
        price = (uint256(basePrice) * weatherMultiplier) / 10000;

        // CONFIDENCE: Calculate confidence score based on data quality
        confidence = _calculateConfidenceScore(weather, updatedAt);

        return (price, confidence);
    }

    /**
     * @notice Fetches weather data with fallback mechanisms
     * @dev Implements redundant data sources for high availability
     */
    function _getWeatherData(string calldata stationId)
        internal
        view
        returns (WeatherData memory weather)
    {
        try weatherDataFeed.latestRoundData() returns (
            uint80 roundId,
            int256 temperature,
            uint256 startedAt,
            uint256 updatedAt,
            uint80 answeredInRound
        ) {
            // PRIMARY SOURCE: Use Chainlink weather oracle when available
            if (block.timestamp - updatedAt <= STALE_DATA_THRESHOLD) {
                return WeatherData({
                    temperature: temperature,
                    timestamp: updatedAt,
                    source: DataSource.PRIMARY_ORACLE,
                    quality: 100
                });
            }
        } catch {
            // FALLBACK: Primary oracle failed, use backup
        }

        // BACKUP ORACLE: Use alternative data source
        try backupOracle.getWeatherData(stationId) returns (
            WeatherData memory backupData
        ) {
            // VALIDATION: Verify backup data is recent and reasonable
            if (
                block.timestamp - backupData.timestamp <= STALE_DATA_THRESHOLD &&
                backupData.quality >= 50
            ) {
                backupData.source = DataSource.BACKUP_ORACLE;
                return backupData;
            }
        } catch {
            // LAST RESORT: Use cached data with reduced confidence
        }

        // CACHED DATA: Use last known good data with warning
        weather = lastKnownWeatherData[stationId];
        weather.source = DataSource.CACHED;
        weather.quality = 25; // Reduced quality for stale data

        // MONITORING: Alert when using cached data
        emit WeatherDataDegraded(stationId, "Using cached data");

        return weather;
    }
}
```

## Contract Structure Comments

### State Variable Documentation

```solidity
contract CarbonCreditNFT {
    // TOKEN METADATA: Standard ERC721 compliance
    string public constant name = "WRFCoin Carbon Credits";
    string public constant symbol = "WRFCC";

    // VERSION CONTROL: Contract version for upgrade compatibility
    uint256 public constant VERSION = 2;

    // GOVERNANCE: Multi-signature wallet for administrative functions
    // Requires 3 of 5 signatures for sensitive operations
    address public governance;

    // TREASURY: Fee collection address for platform sustainability
    // Funds ecosystem development and weather station maintenance
    address public treasury;

    /// @notice Maximum supply of carbon credit NFTs
    /// @dev Hard cap prevents infinite minting and maintains scarcity
    /// Set to global carbon budget equivalent (50 billion tonnes CO2)
    uint256 public constant MAX_SUPPLY = 50_000_000_000;

    /// @notice Current total supply of minted NFTs
    /// @dev Tracked for supply cap enforcement and statistics
    uint256 public totalSupply;

    /// @notice Fee rate for carbon credit trading (in basis points)
    /// @dev 250 basis points = 2.5% trading fee
    /// Used for platform sustainability and weather station incentives
    uint256 public tradingFeeRate = 250;

    /// @notice Minimum carbon credit amount per NFT (in tonnes)
    /// @dev Prevents dust attacks and ensures meaningful credit sizes
    uint256 public constant MIN_CREDIT_AMOUNT = 1 ether; // 1 tonne CO2

    /// @notice Maximum carbon credit amount per NFT (in tonnes)
    /// @dev Prevents concentration risk and promotes distribution
    uint256 public constant MAX_CREDIT_AMOUNT = 10_000 ether; // 10,000 tonnes CO2

    // MAPPING: Token ID to carbon credit metadata
    /// @dev Stores comprehensive information about each carbon credit NFT
    /// Includes weather station data, verification status, and vintage year
    mapping(uint256 => CarbonCreditMetadata) public creditMetadata;

    // MAPPING: Weather station to authorized status
    /// @dev Only authorized stations can issue verified carbon credits
    /// Authorization requires KYC, equipment certification, and data quality standards
    mapping(string => bool) public authorizedStations;

    // MAPPING: User to staking information
    /// @dev Tracks staked NFTs for yield farming and governance participation
    /// Longer staking periods provide higher rewards and voting power
    mapping(address => StakingInfo) public stakingInfo;

    // ARRAY: Historical price data for analytics
    /// @dev Maintains price history for market analysis and reporting
    /// Limited to last 1000 data points for gas efficiency
    PriceDataPoint[] public priceHistory;

    // EVENTS: Contract activity tracking

    /// @notice Emitted when new carbon credit NFT is minted
    /// @param tokenId Unique identifier for the minted NFT
    /// @param recipient Address receiving the NFT
    /// @param creditAmount Amount of carbon credits (in tonnes CO2)
    /// @param weatherStation Weather station ID that verified the credit
    /// @param vintageYear Year the carbon reduction occurred
    event CarbonCreditMinted(
        uint256 indexed tokenId,
        address indexed recipient,
        uint256 creditAmount,
        string indexed weatherStation,
        uint256 vintageYear
    );

    /// @notice Emitted when carbon credits are retired (permanently removed)
    /// @param tokenId NFT identifier being retired
    /// @param owner Address that owned the retired NFT
    /// @param retiredBy Address that initiated the retirement
    /// @param reason Reason for retirement (offsetting, compliance, etc.)
    event CarbonCreditRetired(
        uint256 indexed tokenId,
        address indexed owner,
        address indexed retiredBy,
        string reason
    );
}
```

### Complex Struct Documentation

```solidity
/**
 * @notice Comprehensive metadata for carbon credit NFTs
 * @dev Contains all necessary information for credit verification and trading
 *
 * This struct stores detailed information about each carbon credit NFT including
 * the source weather station, verification status, carbon impact measurements,
 * and temporal information for compliance and auditing purposes.
 */
struct CarbonCreditMetadata {
    /// @notice Amount of CO2 equivalent represented by this credit (in wei units)
    /// @dev Denominated in tonnes * 10^18 for precise fractional amounts
    uint256 carbonAmount;

    /// @notice Weather station that verified this carbon credit
    /// @dev Must be from an authorized and certified weather station
    string weatherStationId;

    /// @notice Year when the carbon reduction/sequestration occurred
    /// @dev Used for vintage pricing and regulatory compliance
    uint256 vintageYear;

    /// @notice Geographic coordinates where carbon impact was measured
    /// @dev Latitude and longitude in decimal degrees * 10^6 for precision
    struct {
        int256 latitude;   // Range: -90,000,000 to 90,000,000
        int256 longitude;  // Range: -180,000,000 to 180,000,000
    } coordinates;

    /// @notice Type of carbon credit (sequestration, reduction, avoidance)
    /// @dev Enum values: 0=Sequestration, 1=Reduction, 2=Avoidance
    CreditType creditType;

    /// @notice Verification status and quality scores
    /// @dev Comprehensive verification data for trust and pricing
    struct {
        bool isVerified;           // Third-party verification status
        uint8 dataQuality;         // Quality score 0-100
        uint8 methodologyVersion;  // Verification methodology version
        uint256 verificationDate;  // Timestamp of verification
        address verifier;          // Address of verifying organization
    } verification;

    /// @notice Environmental impact measurements
    /// @dev Additional environmental data for comprehensive impact assessment
    struct {
        uint256 temperatureImpact;     // Temperature change in millikelvins
        uint256 biodiversityScore;     // Biodiversity impact score 0-1000
        uint256 waterConservation;     // Water saved in liters
        uint256 soilHealthImprovement; // Soil quality improvement 0-100
    } environmentalImpact;

    /// @notice Economic and social impact data
    /// @dev Tracks broader sustainability impacts beyond carbon
    struct {
        uint256 jobsCreated;           // Number of jobs created
        uint256 communityInvestment;   // Local investment amount (USD cents)
        uint256 technologyTransfer;    // Tech transfer score 0-100
        string[] sdgGoals;            // UN SDG goals addressed
    } socialImpact;

    /// @notice Retirement information (if applicable)
    /// @dev Tracks if and when credits were permanently retired
    struct {
        bool isRetired;            // Whether credit has been retired
        uint256 retirementDate;    // Timestamp of retirement
        address retiredBy;         // Address that retired the credit
        string retirementReason;   // Reason for retirement
        string complianceProgram;  // Compliance program if applicable
    } retirement;

    /// @notice Additional metadata for extended functionality
    /// @dev Flexible metadata storage for future enhancements
    mapping(string => string) additionalData;
}

/**
 * @notice Staking information for carbon credit NFT holders
 * @dev Tracks staking status, rewards, and governance participation
 */
struct StakingInfo {
    /// @notice Array of staked token IDs
    /// @dev Limited to 100 NFTs per user for gas efficiency
    uint256[] stakedTokens;

    /// @notice Staking start timestamp
    /// @dev Used for calculating time-based rewards
    uint256 stakingStartTime;

    /// @notice Total staking duration for all tokens (in seconds)
    /// @dev Accumulated across all staking periods
    uint256 totalStakingDuration;

    /// @notice Accumulated staking rewards (in WRF tokens)
    /// @dev Calculated based on staking duration and token rarity
    uint256 accumulatedRewards;

    /// @notice Last reward calculation timestamp
    /// @dev Prevents double-claiming and enables accurate calculations
    uint256 lastRewardCalculation;

    /// @notice Staking tier based on duration and amount
    /// @dev Enum: Bronze=0, Silver=1, Gold=2, Platinum=3, Diamond=4
    StakingTier tier;

    /// @notice Governance voting power multiplier
    /// @dev Based on staking tier and duration (100 = 1x, 200 = 2x, etc.)
    uint256 votingPowerMultiplier;

    /// @notice Lock-up period end timestamp
    /// @dev Some staking tiers require minimum lock-up periods
    uint256 lockupEndTime;
}
```

## Comment Validation and Quality

### Required NatSpec Tags

Use all relevant NatSpec tags for comprehensive documentation:

```solidity
/**
 * @title [Contract/Interface Title]
 * @author [Author Name/Organization]
 * @notice [User-friendly description]
 * @dev [Developer notes and technical details]
 * @param paramName [Parameter description]
 * @return returnName [Return value description]
 * @custom:security-contact [Security contact email]
 * @custom:version [Contract version]
 * @custom:audited-by [Audit firm names]
 * @custom:gas-estimate [Estimated gas costs]
 * @custom:example [Usage examples]
 */
```

### Security Comment Categories

Use consistent prefixes for security-related comments:

- `SECURITY:` - Security measures and protections
- `VALIDATION:` - Input validation and sanitization
- `ACCESS CONTROL:` - Permission and authorization checks
- `REENTRANCY:` - Reentrancy protection measures
- `OVERFLOW:` - Arithmetic overflow/underflow protection
- `ORACLE:` - External data source security
- `CIRCUIT BREAKER:` - Emergency stop mechanisms
- `AUDIT TRAIL:` - Event logging for transparency

### Best Practices Summary

1. **Always use NatSpec** for public interfaces
2. **Document gas costs** for expensive operations
3. **Explain security measures** and attack vectors
4. **Provide usage examples** for complex functions
5. **Document state changes** and side effects
6. **Include version information** for upgradeable contracts
7. **Reference external standards** (EIP numbers, audits)
8. **Comment complex mathematical formulas** and algorithms

---

**Previous**: [Rust Commenting Standards](./rust-standards.md) | **Next**:
[Documentation Templates](./documentation-templates.md)

**Last Updated**: January 2025  
**Version**: 1.0.0
