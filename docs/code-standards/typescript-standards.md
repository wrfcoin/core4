# TypeScript/JavaScript Code Commenting Standards

This guide provides comprehensive commenting standards for TypeScript and JavaScript code in the
WRFCoin platform.

## JSDoc Comments

### Function Documentation

Use JSDoc comments for all public functions:

````typescript
/**
 * Retrieves real-time weather data from specified sensor network
 *
 * This function aggregates data from multiple weather stations within a geographic
 * region and applies quality control filters according to WMO standards.
 *
 * @param location - Geographic coordinates for data retrieval
 * @param options - Configuration options for data filtering and aggregation
 * @returns Promise resolving to validated weather data
 * @throws {ValidationError} When location coordinates are invalid
 * @throws {NetworkError} When sensor network is unreachable
 *
 * @example
 * ```typescript
 * const weatherData = await getWeatherData(
 *   { lat: 35.6762, lng: 139.6503 },
 *   { quality: 'high', aggregation: 'hourly' }
 * );
 * ```
 *
 * @see {@link https://community.wmo.int/activity-areas/imop/standards | WMO Standards}
 * @since 2.1.0
 */
async function getWeatherData(
  location: GeographicCoordinates,
  options: WeatherDataOptions = {}
): Promise<WeatherData> {
  // Implementation details...
}
````

### Class Documentation

Document classes with their purpose, responsibilities, and usage:

````typescript
/**
 * Manages real-time weather station connections and data streaming
 *
 * The WeatherStationManager handles WebSocket connections to weather stations,
 * implements automatic reconnection logic, and provides event-driven data updates.
 * It maintains connection pools for high-throughput scenarios and implements
 * circuit breaker patterns for resilience.
 *
 * @example Basic Usage
 * ```typescript
 * const manager = new WeatherStationManager({
 *   maxConnections: 100,
 *   reconnectInterval: 5000
 * });
 *
 * manager.on('data', (stationId, data) => {
 *   console.log(`Received data from ${stationId}:`, data);
 * });
 *
 * await manager.connect('station-tokyo-001');
 * ```
 *
 * @example Advanced Configuration
 * ```typescript
 * const manager = new WeatherStationManager({
 *   connectionPool: {
 *     min: 10,
 *     max: 200,
 *     timeout: 30000
 *   },
 *   dataValidation: {
 *     schema: WeatherDataSchema,
 *     strict: true
 *   },
 *   circuitBreaker: {
 *     threshold: 5,
 *     timeout: 60000
 *   }
 * });
 * ```
 */
class WeatherStationManager extends EventEmitter {
  /**
   * Current active connections to weather stations
   *
   * Map of station IDs to their connection metadata, including connection
   * status, last data timestamp, and error counts for circuit breaker logic.
   */
  private readonly connections = new Map<string, StationConnection>();

  /**
   * Configuration options for the station manager
   *
   * Immutable configuration set during construction. Includes connection
   * pool settings, data validation rules, and resilience patterns.
   */
  private readonly config: WeatherStationConfig;

  /**
   * Creates a new weather station manager instance
   *
   * @param config - Configuration options for connection management
   */
  constructor(config: WeatherStationConfig) {
    super();
    this.config = Object.freeze({ ...config });
  }

  /**
   * Establishes connection to a weather station
   *
   * Creates WebSocket connection with automatic reconnection logic.
   * Implements exponential backoff for failed connections and validates
   * station authentication before allowing data streaming.
   *
   * @param stationId - Unique identifier for the weather station
   * @param options - Connection-specific options (optional)
   * @returns Promise resolving when connection is established
   *
   * @throws {AuthenticationError} When station credentials are invalid
   * @throws {NetworkError} When network connection fails
   * @throws {RateLimitError} When connection rate limits are exceeded
   */
  async connect(stationId: string, options?: ConnectionOptions): Promise<void> {
    // Implementation with detailed inline comments for complex logic
  }
}
````

### Interface Documentation

Document interfaces with property explanations and usage examples:

```typescript
/**
 * Configuration options for weather data retrieval and processing
 *
 * Defines parameters for data quality filtering, temporal aggregation,
 * and output formatting when requesting weather information from sensors.
 */
interface WeatherDataOptions {
  /**
   * Data quality level for filtering sensor readings
   *
   * - 'raw': Unfiltered data directly from sensors
   * - 'basic': Basic quality control (range checks, spike detection)
   * - 'high': Advanced QC including peer comparison and temporal consistency
   * - 'research': Research-grade processing with manual validation
   *
   * @default 'basic'
   */
  quality?: 'raw' | 'basic' | 'high' | 'research';

  /**
   * Temporal aggregation period for data summarization
   *
   * Determines how raw sensor measurements are aggregated over time.
   * Shorter intervals provide higher resolution but larger data volumes.
   *
   * @default 'hourly'
   */
  aggregation?: 'minute' | 'hourly' | 'daily';

  /**
   * Maximum age of acceptable data in milliseconds
   *
   * Data older than this threshold will be excluded from results.
   * Used to ensure real-time applications receive fresh data.
   *
   * @default 300000 (5 minutes)
   */
  maxAge?: number;

  /**
   * Geographic radius for multi-station aggregation (in meters)
   *
   * When specified, data from multiple stations within this radius
   * will be combined using weighted averaging based on distance.
   *
   * @default undefined (single station only)
   */
  radius?: number;

  /**
   * Custom validation schema for data filtering
   *
   * Optional JSON schema for additional data validation beyond
   * standard quality control procedures.
   */
  validationSchema?: JSONSchema;
}
```

### Type Documentation

Document complex types and their usage:

```typescript
/**
 * Geographic coordinates with precision metadata
 *
 * Represents a location on Earth using decimal degrees with
 * associated precision information for data quality assessment.
 */
type GeographicCoordinates = {
  /** Latitude in decimal degrees (-90 to 90) */
  lat: number;

  /** Longitude in decimal degrees (-180 to 180) */
  lng: number;

  /**
   * Coordinate precision in meters
   *
   * Represents the accuracy of the coordinate measurement.
   * Used for spatial data quality assessment and interpolation weighting.
   */
  precision?: number;

  /**
   * Elevation above mean sea level in meters
   *
   * Optional elevation data for pressure altitude corrections
   * and orographic weather modeling.
   */
  elevation?: number;
};

/**
 * Weather measurement data point with metadata
 *
 * Represents a single weather observation including the measured
 * values, quality indicators, and traceability information.
 */
type WeatherMeasurement = {
  /** Unique identifier for this measurement */
  readonly id: string;

  /** ISO 8601 timestamp of the measurement */
  readonly timestamp: string;

  /** Station that recorded this measurement */
  readonly stationId: string;

  /** Geographic location of the measurement */
  readonly location: GeographicCoordinates;

  /** Measured weather parameters */
  readonly measurements: {
    /** Temperature in degrees Celsius */
    temperature?: number;

    /** Relative humidity as percentage (0-100) */
    humidity?: number;

    /** Atmospheric pressure in hectopascals */
    pressure?: number;

    /** Wind speed in meters per second */
    windSpeed?: number;

    /** Wind direction in degrees (0-360) */
    windDirection?: number;

    /** Precipitation amount in millimeters */
    precipitation?: number;
  };

  /** Data quality indicators for each measurement */
  readonly quality: {
    /** Overall quality score (0-1, where 1 is highest quality) */
    score: number;

    /** Flags indicating specific quality issues */
    flags: QualityFlag[];

    /** Quality control procedure version used */
    procedure: string;
  };
};
```

## Inline Comments

### Business Logic Comments

Explain business rules and domain-specific logic:

```typescript
async function calculateCarbonCredits(weatherData: WeatherMeasurement[]): Promise<CarbonCredit[]> {
  // BUSINESS RULE: Carbon credits calculated using ISO 14064-2 methodology
  // Only measurements from certified weather stations qualify for credits
  const certifiedData = weatherData.filter(
    data =>
      // Station must be WMO-certified and operational for >1 year
      data.station.certifications.includes('WMO') &&
      data.station.operationalSince < Date.now() - 365 * 24 * 60 * 60 * 1000
  );

  // COMPLIANCE: EU ETS Phase 4 requires hourly data granularity
  const hourlyData = aggregateToHourly(certifiedData);

  // ALGORITHM: Carbon impact calculation based on temperature differential
  // Reference: "Climate Impact Quantification" - IPCC AR6 methodology
  const carbonImpact = hourlyData.map(measurement => {
    // Baseline temperature derived from 30-year historical average
    const baseline = await getHistoricalBaseline(measurement.location);
    const temperatureDiff = measurement.temperature - baseline.temperature;

    // Carbon coefficient varies by geographic region and season
    // Positive diff = warming = carbon debt, Negative = cooling = carbon credit
    const regionCoefficient = getRegionCarbonCoefficient(measurement.location);
    const seasonalAdjustment = getSeasonalAdjustment(measurement.timestamp);

    return temperatureDiff * regionCoefficient * seasonalAdjustment;
  });

  return generateCarbonCredits(carbonImpact);
}
```

### Performance and Optimization Comments

Document performance-critical code:

```typescript
class WeatherDataCache {
  // PERFORMANCE: LRU cache with 10MB memory limit for real-time queries
  // Eviction strategy: Least recently used items removed when memory limit reached
  private readonly cache = new Map<string, CacheEntry>();
  private readonly maxMemoryMB = 10;
  private currentMemoryMB = 0;

  async getData(key: string): Promise<WeatherData | null> {
    // OPTIMIZATION: Check cache first for sub-millisecond response times
    // Cache hit rate target: >85% for production workloads
    const cached = this.cache.get(key);
    if (cached && !this.isExpired(cached)) {
      // Move to end for LRU tracking (O(1) operation)
      this.cache.delete(key);
      this.cache.set(key, cached);
      return cached.data;
    }

    // PERFORMANCE: Database query optimization with prepared statements
    // Average query time: <50ms for single station, <200ms for regional queries
    const data = await this.database.query(
      // OPTIMIZATION: Use covering index on (station_id, timestamp) for fast lookups
      'SELECT * FROM weather_data WHERE station_id = $1 AND timestamp >= $2',
      [key, Date.now() - this.cacheTTL]
    );

    // MEMORY: Evict oldest entries if cache size exceeds limit
    while (this.currentMemoryMB > this.maxMemoryMB) {
      this.evictOldestEntry();
    }

    // Cache the result with memory usage tracking
    this.addToCache(key, data);
    return data;
  }

  private evictOldestEntry(): void {
    // ALGORITHM: LRU eviction using Map insertion order (ES2015+ guarantee)
    // First entry is the oldest due to our cache access pattern
    const [oldestKey] = this.cache.keys();
    if (oldestKey) {
      const entry = this.cache.get(oldestKey)!;
      this.currentMemoryMB -= this.calculateMemoryUsage(entry);
      this.cache.delete(oldestKey);
    }
  }
}
```

### Security and Validation Comments

Document security-sensitive operations:

```typescript
async function validateWeatherStationAuth(request: AuthRequest): Promise<AuthResult> {
  // SECURITY: Multi-factor authentication for weather station access
  // Prevents unauthorized data injection attacks on sensor network

  // Phase 1: API key validation with rate limiting
  // SECURITY: Protect against brute force attacks with exponential backoff
  const apiKeyValid = await this.rateLimiter.checkAndIncrement(
    request.apiKey,
    { maxAttempts: 5, windowMs: 15 * 60 * 1000 } // 5 attempts per 15 minutes
  );

  if (!apiKeyValid) {
    // AUDIT: Log security event for monitoring dashboard
    await this.securityLogger.logEvent({
      type: 'AUTH_FAILURE',
      stationId: request.stationId,
      clientIP: request.clientIP,
      reason: 'RATE_LIMIT_EXCEEDED',
    });
    throw new AuthenticationError('Rate limit exceeded');
  }

  // Phase 2: Digital signature verification
  // SECURITY: Prevents man-in-the-middle attacks and data tampering
  const signature = await this.crypto.verifySignature(
    request.payload,
    request.signature,
    request.stationId // Public key retrieved from HSM using station ID
  );

  if (!signature.valid) {
    // SECURITY: Potential compromise - alert security team
    await this.alerting.sendSecurityAlert({
      severity: 'HIGH',
      message: `Invalid signature from station ${request.stationId}`,
      clientIP: request.clientIP,
    });
    throw new AuthenticationError('Invalid signature');
  }

  // Phase 3: Temporal validation to prevent replay attacks
  // SECURITY: Request timestamp must be within 5-minute window
  const timestampDiff = Math.abs(Date.now() - request.timestamp);
  const maxSkewMs = 5 * 60 * 1000; // 5 minutes

  if (timestampDiff > maxSkewMs) {
    // AUDIT: Log potential replay attack
    await this.securityLogger.logEvent({
      type: 'REPLAY_ATTACK_SUSPECTED',
      stationId: request.stationId,
      timestampSkew: timestampDiff,
    });
    throw new AuthenticationError('Request timestamp out of acceptable range');
  }

  return { authenticated: true, stationId: request.stationId };
}
```

### Error Handling Comments

Document error scenarios and recovery strategies:

```typescript
async function processWeatherDataBatch(batch: WeatherDataBatch): Promise<ProcessingResult> {
  const results: ProcessingResult = {
    processed: 0,
    failed: 0,
    errors: [],
  };

  for (const dataPoint of batch.data) {
    try {
      // RESILIENCE: Individual data point failures don't stop batch processing
      // This ensures maximum data throughput even with partial sensor failures
      await this.validateAndStore(dataPoint);
      results.processed++;
    } catch (error) {
      results.failed++;

      if (error instanceof ValidationError) {
        // RECOVERY: Validation errors are logged but not retried
        // Invalid sensor data cannot be corrected automatically
        results.errors.push({
          type: 'VALIDATION_ERROR',
          dataPoint: dataPoint.id,
          message: error.message,
          retryable: false,
        });
      } else if (error instanceof NetworkError) {
        // RECOVERY: Network errors are queued for retry with exponential backoff
        // Temporary network issues often resolve within minutes
        await this.retryQueue.add(dataPoint, {
          maxRetries: 3,
          backoffStrategy: 'exponential',
          baseDelay: 1000,
        });

        results.errors.push({
          type: 'NETWORK_ERROR',
          dataPoint: dataPoint.id,
          message: error.message,
          retryable: true,
        });
      } else if (error instanceof DatabaseError) {
        // RECOVERY: Database errors trigger circuit breaker pattern
        // Prevents cascading failures when database is overloaded
        this.circuitBreaker.recordFailure();

        if (this.circuitBreaker.isOpen()) {
          // FALLBACK: Store to message queue when database unavailable
          await this.messageQueue.enqueue(dataPoint);
        }

        results.errors.push({
          type: 'DATABASE_ERROR',
          dataPoint: dataPoint.id,
          message: error.message,
          retryable: !this.circuitBreaker.isOpen(),
        });
      } else {
        // MONITORING: Unexpected errors trigger alerts for investigation
        await this.monitoring.alertUnexpectedError(error, {
          context: 'weather_data_processing',
          dataPoint: dataPoint.id,
          batchId: batch.id,
        });

        results.errors.push({
          type: 'UNEXPECTED_ERROR',
          dataPoint: dataPoint.id,
          message: error.message,
          retryable: false,
        });
      }
    }
  }

  // METRICS: Record processing statistics for performance monitoring
  await this.metrics.recordBatchProcessing({
    batchSize: batch.data.length,
    processed: results.processed,
    failed: results.failed,
    processingTimeMs: Date.now() - batch.startTime,
  });

  return results;
}
```

## React Component Comments

### Component Documentation

````typescript
/**
 * Real-time weather data visualization dashboard
 *
 * Displays live weather data from multiple stations with interactive charts,
 * map visualization, and configurable alerts. Optimized for high-frequency
 * updates with automatic data aggregation and efficient re-rendering.
 *
 * @example Basic Usage
 * ```tsx
 * <WeatherDashboard
 *   stationIds={['tokyo-001', 'osaka-002']}
 *   updateInterval={30000}
 *   onAlertTriggered={(alert) => console.log('Alert:', alert)}
 * />
 * ```
 *
 * @example Advanced Configuration
 * ```tsx
 * <WeatherDashboard
 *   stationIds={stationList}
 *   updateInterval={10000}
 *   displayOptions={{
 *     showMap: true,
 *     chartTypes: ['temperature', 'humidity'],
 *     aggregation: 'hourly'
 *   }}
 *   alertThresholds={{
 *     temperature: { min: -10, max: 40 },
 *     humidity: { min: 10, max: 90 }
 *   }}
 *   onAlertTriggered={handleAlert}
 *   onDataUpdate={handleDataUpdate}
 * />
 * ```
 */
interface WeatherDashboardProps {
  /** List of weather station IDs to display */
  stationIds: string[];

  /** Data refresh interval in milliseconds (minimum: 1000) */
  updateInterval?: number;

  /** Visual display configuration options */
  displayOptions?: {
    /** Whether to show interactive map view */
    showMap?: boolean;

    /** Types of charts to display */
    chartTypes?: ('temperature' | 'humidity' | 'pressure' | 'wind')[];

    /** Data aggregation period for chart display */
    aggregation?: 'minute' | 'hour' | 'day';
  };

  /** Alert threshold configuration */
  alertThresholds?: {
    temperature?: { min: number; max: number };
    humidity?: { min: number; max: number };
    pressure?: { min: number; max: number };
  };

  /** Callback fired when alert threshold is exceeded */
  onAlertTriggered?: (alert: WeatherAlert) => void;

  /** Callback fired when new data is received */
  onDataUpdate?: (data: WeatherData[]) => void;
}

const WeatherDashboard: React.FC<WeatherDashboardProps> = ({
  stationIds,
  updateInterval = 30000,
  displayOptions = {},
  alertThresholds,
  onAlertTriggered,
  onDataUpdate
}) => {
  // PERFORMANCE: Memoize expensive calculations to prevent unnecessary re-renders
  const chartConfig = useMemo(() => {
    // Chart configuration is computationally expensive (>10ms for large datasets)
    // Memoization reduces CPU usage and improves UI responsiveness
    return generateChartConfiguration(displayOptions.chartTypes || ['temperature']);
  }, [displayOptions.chartTypes]);

  // REAL-TIME: WebSocket connection for live data updates
  const { data, connectionStatus, error } = useWebSocket(`/ws/weather/stations`, {
    // OPTIMIZATION: Only subscribe to specified stations to reduce bandwidth
    filter: { stationIds },

    // RESILIENCE: Automatic reconnection with exponential backoff
    reconnect: {
      maxAttempts: 5,
      baseDelay: 1000,
      maxDelay: 30000
    },

    // DATA QUALITY: Validate incoming data structure
    validator: WeatherDataSchema,

    onMessage: useCallback((newData: WeatherData[]) => {
      // PERFORMANCE: Batch data updates to prevent excessive re-renders
      // Update UI at most every 100ms even with high-frequency data
      batchDataUpdate(newData);

      // BUSINESS LOGIC: Check alert thresholds on new data
      if (alertThresholds) {
        const alerts = checkAlertThresholds(newData, alertThresholds);
        alerts.forEach(alert => onAlertTriggered?.(alert));
      }

      // CALLBACK: Notify parent component of data updates
      onDataUpdate?.(newData);
    }, [alertThresholds, onAlertTriggered, onDataUpdate])
  });

  // ACCESSIBILITY: Announce important changes to screen readers
  const [announcement, setAnnouncement] = useState<string>('');

  useEffect(() => {
    if (error) {
      // ACCESSIBILITY: Announce connection errors to assistive technology
      setAnnouncement(`Weather data connection error: ${error.message}`);
    } else if (connectionStatus === 'connected') {
      setAnnouncement('Weather data connection established');
    }
  }, [error, connectionStatus]);

  // UI STATE: Loading state while establishing connection
  if (connectionStatus === 'connecting') {
    return (
      <div className="weather-dashboard loading" role="status" aria-label="Loading weather data">
        <LoadingSpinner />
        <span className="sr-only">Connecting to weather stations...</span>
      </div>
    );
  }

  // ERROR STATE: Connection failed or data unavailable
  if (error || connectionStatus === 'disconnected') {
    return (
      <ErrorBoundary
        error={error}
        onRetry={() => window.location.reload()}
        fallback={
          <div className="weather-dashboard error" role="alert">
            <h2>Weather Data Unavailable</h2>
            <p>Unable to connect to weather stations. Please check your connection.</p>
            <button onClick={() => window.location.reload()}>
              Retry Connection
            </button>
          </div>
        }
      />
    );
  }

  return (
    <div
      className="weather-dashboard"
      role="main"
      aria-label={`Weather dashboard for ${stationIds.length} stations`}
    >
      {/* ACCESSIBILITY: Screen reader announcements */}
      <div aria-live="polite" aria-atomic="true" className="sr-only">
        {announcement}
      </div>

      {/* FEATURE: Interactive map view (optional) */}
      {displayOptions.showMap && (
        <section className="dashboard-section map-section" aria-labelledby="map-heading">
          <h2 id="map-heading">Weather Station Map</h2>
          <WeatherMap
            stations={data}
            onStationSelect={handleStationSelect}
            accessibilityLabel="Interactive weather station map"
          />
        </section>
      )}

      {/* FEATURE: Real-time charts */}
      <section className="dashboard-section charts-section" aria-labelledby="charts-heading">
        <h2 id="charts-heading">Real-Time Weather Data</h2>
        <div className="charts-grid">
          {chartConfig.map((config) => (
            <WeatherChart
              key={config.type}
              type={config.type}
              data={data}
              config={config}
              updateInterval={updateInterval}
              aria-label={`${config.type} chart for weather stations`}
            />
          ))}
        </div>
      </section>

      {/* FEATURE: Alert status display */}
      {alertThresholds && (
        <section className="dashboard-section alerts-section" aria-labelledby="alerts-heading">
          <h2 id="alerts-heading">Alert Status</h2>
          <AlertPanel
            thresholds={alertThresholds}
            currentData={data}
            onAlertAcknowledge={handleAlertAcknowledge}
          />
        </section>
      )}
    </div>
  );
};

export default WeatherDashboard;
````

## Comment Validation Rules

### Automated Checks

The following rules are enforced by our linting tools:

```typescript
// ✅ GOOD: Explains WHY, not WHAT
// Business requirement: ISO 14064 compliance requires hourly data aggregation
const aggregatedData = aggregateHourly(rawData);

// ❌ BAD: States the obvious
// Aggregate the data hourly
const aggregatedData = aggregateHourly(rawData);

// ✅ GOOD: Provides context and reasoning
/**
 * Calculates weather forecast accuracy using RMSE methodology
 *
 * Uses Root Mean Square Error as defined in WMO-No. 1199 for
 * standardized forecast verification. Results are used for
 * model performance ranking and automatic model selection.
 */
function calculateForecastAccuracy(forecast: number[], actual: number[]): number {
  // Implementation...
}

// ❌ BAD: Missing context and explanation
/**
 * Calculates accuracy
 */
function calculateForecastAccuracy(forecast: number[], actual: number[]): number {
  // Implementation...
}
```

### Required Comments

**Must Have Comments:**

- All exported functions and classes
- Complex algorithms (>10 lines of logic)
- Security-sensitive operations
- Performance-critical code
- Business rule implementations
- Error handling strategies
- Configuration and setup code

**Should Have Comments:**

- Non-obvious variable names or magic numbers
- Temporary workarounds or TODOs
- Integration points with external systems
- Data transformation logic
- Validation and sanitization

**May Have Comments:**

- Implementation details for complex logic
- Historical context for decisions
- Links to relevant documentation
- Examples of usage patterns

---

**Next**: [Rust Commenting Standards](./rust-standards.md)

**Last Updated**: January 2025  
**Version**: 1.0.0
