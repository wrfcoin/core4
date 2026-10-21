# WRFCoin SDK Best Practices Guide

This comprehensive guide outlines best practices for developing applications with WRFCoin SDKs
across all supported platforms. Follow these guidelines to build secure, performant, and
maintainable applications.

## Table of Contents

- [Authentication & Security](#authentication--security)
- [API Usage Patterns](#api-usage-patterns)
- [Error Handling](#error-handling)
- [Performance Optimization](#performance-optimization)
- [Data Management](#data-management)
- [Real-time Features](#real-time-features)
- [Testing Strategies](#testing-strategies)
- [Platform-Specific Guidelines](#platform-specific-guidelines)
- [Production Deployment](#production-deployment)
- [Monitoring & Analytics](#monitoring--analytics)

## Authentication & Security

### 1. Secure API Key Management

**❌ Don't: Hardcode API Keys**

```java
// BAD - Never do this
WRFCoinSDK sdk = new WRFCoinSDK.Builder()
    .apiKey("your-actual-api-key-here")  // Exposed in source code
    .build();
```

**✅ Do: Use Environment Variables and Secure Storage**

```java
// GOOD - Java/Spring Boot
@Value("${wrfcoin.api.key}")
private String apiKey;

// GOOD - Android
String apiKey = BuildConfig.WRFCOIN_API_KEY; // From build.gradle

// GOOD - iOS
let apiKey = Bundle.main.object(forInfoDictionaryKey: "WRFCOIN_API_KEY") as? String
```

### 2. Implement Proper Authentication Flow

**✅ Recommended Authentication Pattern**

```kotlin
// Android Kotlin
class AuthenticationManager {
    private val biometricPrompt: BiometricPrompt by lazy { createBiometricPrompt() }

    suspend fun authenticateUser(email: String, password: String): AuthResult {
        return try {
            // 1. Check if biometric auth is available and user has enabled it
            if (shouldUseBiometricAuth()) {
                authenticateWithBiometrics()
            }

            // 2. Authenticate with WRFCoin
            val authResult = WRFCoinSDK.getInstance()
                .authService
                .authenticate(email, password)

            // 3. Store tokens securely
            storeTokensSecurely(authResult.accessToken, authResult.refreshToken)

            // 4. Setup automatic token refresh
            scheduleTokenRefresh(authResult.expiresIn)

            authResult
        } catch (e: Exception) {
            handleAuthenticationError(e)
            throw e
        }
    }

    private suspend fun storeTokensSecurely(accessToken: String, refreshToken: String) {
        // Use Android Keystore for encryption
        val encryptedPrefs = EncryptedSharedPreferences.create(
            "wrfcoin_tokens",
            masterKeyAlias,
            context,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )

        encryptedPrefs.edit()
            .putString("access_token", accessToken)
            .putString("refresh_token", refreshToken)
            .apply()
    }
}
```

### 3. Token Management Best Practices

**✅ Automatic Token Refresh Implementation**

```swift
// iOS Swift
class TokenManager: ObservableObject {
    @Published var isAuthenticated = false
    private var refreshTimer: Timer?

    func setupTokenRefresh(expiresIn: TimeInterval) {
        // Refresh token 5 minutes before expiry
        let refreshTime = expiresIn - 300

        refreshTimer?.invalidate()
        refreshTimer = Timer.scheduledTimer(withTimeInterval: refreshTime, repeats: false) { _ in
            Task {
                await self.refreshTokenIfNeeded()
            }
        }
    }

    private func refreshTokenIfNeeded() async {
        do {
            let newTokens = try await WRFCoinSDK.shared.authService.refreshToken()
            try storeTokensInKeychain(newTokens)
            setupTokenRefresh(expiresIn: newTokens.expiresIn)
        } catch {
            // Handle refresh failure - might need to re-authenticate
            await signOut()
        }
    }

    private func storeTokensInKeychain(_ tokens: TokenResponse) throws {
        let accessTokenData = tokens.accessToken.data(using: .utf8)!
        let refreshTokenData = tokens.refreshToken.data(using: .utf8)!

        try KeychainManager.shared.store(key: "access_token", data: accessTokenData)
        try KeychainManager.shared.store(key: "refresh_token", data: refreshTokenData)
    }
}
```

### 4. Rate Limiting and Request Management

**✅ Implement Intelligent Rate Limiting**

```csharp
// C# Implementation
public class RateLimitedApiClient
{
    private readonly SemaphoreSlim _semaphore;
    private readonly Dictionary<string, DateTime> _lastRequestTimes;
    private readonly TimeSpan _minRequestInterval = TimeSpan.FromMilliseconds(100);

    public RateLimitedApiClient()
    {
        _semaphore = new SemaphoreSlim(5, 5); // Max 5 concurrent requests
        _lastRequestTimes = new Dictionary<string, DateTime>();
    }

    public async Task<T> ExecuteRequestAsync<T>(Func<Task<T>> request, string endpoint)
    {
        await _semaphore.WaitAsync();

        try
        {
            // Enforce minimum interval between requests to same endpoint
            if (_lastRequestTimes.TryGetValue(endpoint, out DateTime lastRequest))
            {
                var elapsed = DateTime.UtcNow - lastRequest;
                if (elapsed < _minRequestInterval)
                {
                    await Task.Delay(_minRequestInterval - elapsed);
                }
            }

            _lastRequestTimes[endpoint] = DateTime.UtcNow;

            return await request();
        }
        finally
        {
            _semaphore.Release();
        }
    }
}
```

## API Usage Patterns

### 1. Efficient Data Fetching

**✅ Implement Smart Caching Strategy**

```java
// Java implementation with caching
@Service
public class WeatherService {
    private final WRFCoinSDK sdk;
    private final Cache<String, WeatherData> cache;
    private final Duration cacheExpiry = Duration.ofMinutes(15);

    public CompletableFuture<WeatherData> getCurrentWeather(double lat, double lng) {
        String cacheKey = String.format("weather:%.4f:%.4f", lat, lng);

        // Check cache first
        WeatherData cached = cache.getIfPresent(cacheKey);
        if (cached != null && isCacheValid(cached)) {
            return CompletableFuture.completedFuture(cached);
        }

        // Fetch from API
        return sdk.getWeatherService()
            .getCurrentWeather(lat, lng)
            .thenApply(weather -> {
                // Cache the result
                cache.put(cacheKey, weather);
                return weather;
            });
    }

    private boolean isCacheValid(WeatherData weather) {
        return weather.getTimestamp()
            .isAfter(Instant.now().minus(cacheExpiry));
    }
}
```

### 2. Batch Operations

**✅ Use Batch APIs When Available**

```kotlin
// Android Kotlin - Batch carbon credit purchases
class CarbonTradingRepository {

    suspend fun purchaseMultipleCredits(
        purchases: List<CreditPurchaseRequest>
    ): List<Transaction> {
        // Use batch API instead of individual requests
        return if (purchases.size > 1) {
            carbonService.purchaseCreditsInBatch(purchases)
        } else {
            listOf(carbonService.purchaseCredit(purchases.first()))
        }
    }

    suspend fun getMarketDataBatch(
        creditIds: List<String>
    ): Map<String, CarbonCredit> {
        // Chunk large requests to avoid timeouts
        return creditIds.chunked(50).map { chunk ->
            carbonService.getCreditsBatch(chunk)
        }.fold(emptyMap()) { acc, batch ->
            acc + batch.associateBy { it.id }
        }
    }
}
```

### 3. Progressive Data Loading

**✅ Implement Pagination and Progressive Loading**

```swift
// iOS Swift - Progressive loading with pagination
class CarbonCreditsViewModel: ObservableObject {
    @Published var credits: [CarbonCredit] = []
    @Published var isLoading = false
    @Published var hasMoreData = true

    private var currentPage = 0
    private let pageSize = 20

    func loadMoreCreditsIfNeeded(currentCredit: CarbonCredit) async {
        // Load more when user scrolls near the end
        guard let index = credits.firstIndex(where: { $0.id == currentCredit.id }),
              index >= credits.count - 5,
              !isLoading,
              hasMoreData else { return }

        await loadMoreCredits()
    }

    private func loadMoreCredits() async {
        isLoading = true

        do {
            let newCredits = try await WRFCoinSDK.shared.carbonService.getAvailableCredits(
                page: currentPage,
                pageSize: pageSize
            )

            self.credits.append(contentsOf: newCredits)
            self.currentPage += 1
            self.hasMoreData = newCredits.count == pageSize

        } catch {
            // Handle error
            print("Failed to load more credits: \(error)")
        }

        isLoading = false
    }
}
```

## Error Handling

### 1. Comprehensive Error Classification

**✅ Define Clear Error Hierarchies**

```java
// Java - Comprehensive error handling
public abstract class WRFCoinException extends Exception {
    private final String errorCode;
    private final Map<String, Object> metadata;

    protected WRFCoinException(String message, String errorCode, Throwable cause) {
        super(message, cause);
        this.errorCode = errorCode;
        this.metadata = new HashMap<>();
    }

    public abstract boolean isRetryable();
    public abstract Duration getRetryDelay();
}

public class NetworkException extends WRFCoinException {
    public NetworkException(String message, Throwable cause) {
        super(message, "NETWORK_ERROR", cause);
    }

    @Override
    public boolean isRetryable() {
        return true;
    }

    @Override
    public Duration getRetryDelay() {
        return Duration.ofSeconds(2);
    }
}

public class AuthenticationException extends WRFCoinException {
    public AuthenticationException(String message) {
        super(message, "AUTH_ERROR", null);
    }

    @Override
    public boolean isRetryable() {
        return false; // Don't retry auth errors
    }

    @Override
    public Duration getRetryDelay() {
        return Duration.ZERO;
    }
}
```

### 2. Retry Logic with Exponential Backoff

**✅ Implement Smart Retry Mechanisms**

```csharp
// C# - Retry with exponential backoff
public class RetryPolicy
{
    private readonly int _maxRetries;
    private readonly TimeSpan _baseDelay;
    private readonly double _backoffMultiplier;

    public RetryPolicy(int maxRetries = 3, TimeSpan? baseDelay = null, double backoffMultiplier = 2.0)
    {
        _maxRetries = maxRetries;
        _baseDelay = baseDelay ?? TimeSpan.FromSeconds(1);
        _backoffMultiplier = backoffMultiplier;
    }

    public async Task<T> ExecuteAsync<T>(Func<Task<T>> operation)
    {
        var attempt = 0;

        while (true)
        {
            try
            {
                return await operation();
            }
            catch (Exception ex) when (ShouldRetry(ex, attempt))
            {
                attempt++;
                var delay = CalculateDelay(attempt);

                // Add jitter to prevent thundering herd
                var jitter = TimeSpan.FromMilliseconds(Random.Shared.Next(0, 1000));
                await Task.Delay(delay + jitter);
            }
        }
    }

    private bool ShouldRetry(Exception exception, int attempt)
    {
        if (attempt >= _maxRetries) return false;

        return exception switch
        {
            NetworkException => true,
            TimeoutException => true,
            HttpRequestException httpEx when IsRetryableHttpStatus(httpEx) => true,
            _ => false
        };
    }

    private TimeSpan CalculateDelay(int attempt)
    {
        return TimeSpan.FromTicks((long)(_baseDelay.Ticks * Math.Pow(_backoffMultiplier, attempt - 1)));
    }
}
```

### 3. User-Friendly Error Messages

**✅ Provide Contextual Error Information**

```kotlin
// Android Kotlin - User-friendly error handling
class ErrorHandler {

    fun handleError(error: Throwable, context: Context): ErrorInfo {
        return when (error) {
            is NetworkException -> ErrorInfo(
                title = context.getString(R.string.network_error_title),
                message = context.getString(R.string.network_error_message),
                action = ErrorAction.RETRY,
                icon = R.drawable.ic_wifi_off
            )

            is AuthenticationException -> ErrorInfo(
                title = context.getString(R.string.auth_error_title),
                message = context.getString(R.string.auth_error_message),
                action = ErrorAction.LOGIN,
                icon = R.drawable.ic_lock
            )

            is RateLimitException -> ErrorInfo(
                title = context.getString(R.string.rate_limit_title),
                message = context.getString(
                    R.string.rate_limit_message,
                    formatDuration(error.retryAfter)
                ),
                action = ErrorAction.WAIT,
                icon = R.drawable.ic_hourglass
            )

            else -> ErrorInfo(
                title = context.getString(R.string.generic_error_title),
                message = error.localizedMessage ?: context.getString(R.string.generic_error_message),
                action = ErrorAction.DISMISS,
                icon = R.drawable.ic_error
            )
        }
    }
}

data class ErrorInfo(
    val title: String,
    val message: String,
    val action: ErrorAction,
    val icon: Int
)

enum class ErrorAction {
    RETRY, LOGIN, WAIT, DISMISS, CONTACT_SUPPORT
}
```

## Performance Optimization

### 1. Memory Management

**✅ Implement Proper Resource Management**

```swift
// iOS Swift - Memory-efficient image caching
class ImageCacheManager {
    private let cache = NSCache<NSString, UIImage>()
    private let diskCache: URL

    init() {
        // Configure memory cache
        cache.countLimit = 100
        cache.totalCostLimit = 50 * 1024 * 1024 // 50MB

        // Setup disk cache
        diskCache = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("ImageCache")

        try? FileManager.default.createDirectory(at: diskCache, withIntermediateDirectories: true)

        // Clean up on memory warning
        NotificationCenter.default.addObserver(
            forName: UIApplication.didReceiveMemoryWarningNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.clearMemoryCache()
        }
    }

    func loadImage(from url: URL) async -> UIImage? {
        let key = url.absoluteString as NSString

        // Check memory cache
        if let cachedImage = cache.object(forKey: key) {
            return cachedImage
        }

        // Check disk cache
        let diskPath = diskCache.appendingPathComponent(key.hash.description)
        if let diskImage = UIImage(contentsOfFile: diskPath.path) {
            cache.setObject(diskImage, forKey: key)
            return diskImage
        }

        // Download and cache
        do {
            let (data, _) = try await URLSession.shared.data(from: url)
            guard let image = UIImage(data: data) else { return nil }

            // Cache in memory and disk
            cache.setObject(image, forKey: key)
            try? data.write(to: diskPath)

            return image
        } catch {
            return nil
        }
    }

    private func clearMemoryCache() {
        cache.removeAllObjects()
    }
}
```

### 2. Network Optimization

**✅ Optimize Network Requests**

```java
// Java - Network optimization with connection pooling
@Configuration
public class NetworkConfig {

    @Bean
    public OkHttpClient okHttpClient() {
        ConnectionPool connectionPool = new ConnectionPool(10, 5, TimeUnit.MINUTES);

        return new OkHttpClient.Builder()
            .connectionPool(connectionPool)
            .connectTimeout(30, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .writeTimeout(30, TimeUnit.SECONDS)
            .addInterceptor(new HttpLoggingInterceptor()
                .setLevel(HttpLoggingInterceptor.Level.BODY))
            .addInterceptor(new GzipRequestInterceptor())
            .addNetworkInterceptor(new CacheInterceptor())
            .cache(new Cache(new File("http-cache"), 50 * 1024 * 1024)) // 50MB
            .build();
    }

    private static class GzipRequestInterceptor implements Interceptor {
        @Override
        public Response intercept(Chain chain) throws IOException {
            Request originalRequest = chain.request();

            if (originalRequest.body() == null || originalRequest.header("Content-Encoding") != null) {
                return chain.proceed(originalRequest);
            }

            Request compressedRequest = originalRequest.newBuilder()
                .header("Content-Encoding", "gzip")
                .method(originalRequest.method(), gzip(originalRequest.body()))
                .build();

            return chain.proceed(compressedRequest);
        }

        private RequestBody gzip(final RequestBody body) {
            return new RequestBody() {
                @Override
                public MediaType contentType() {
                    return body.contentType();
                }

                @Override
                public long contentLength() {
                    return -1; // We don't know the compressed length in advance
                }

                @Override
                public void writeTo(BufferedSink sink) throws IOException {
                    BufferedSink gzipSink = Okio.buffer(new GzipSink(sink));
                    body.writeTo(gzipSink);
                    gzipSink.close();
                }
            };
        }
    }
}
```

### 3. Background Processing

**✅ Efficient Background Task Management**

```kotlin
// Android Kotlin - Background sync with WorkManager
class WeatherSyncWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        return try {
            val locationService = LocationService()
            val weatherService = WRFCoinSDK.getInstance().weatherService

            // Get user's locations
            val locations = locationService.getSavedLocations()

            // Batch fetch weather data
            val weatherUpdates = locations.chunked(10).flatMap { chunk ->
                chunk.map { location ->
                    async(Dispatchers.IO) {
                        weatherService.getCurrentWeather(location.lat, location.lng)
                    }
                }.awaitAll()
            }

            // Cache updated data
            val cacheManager = CacheManager(applicationContext)
            weatherUpdates.forEach { weather ->
                cacheManager.cacheWeatherData(weather)
            }

            // Send notifications for weather alerts
            checkForWeatherAlerts(weatherUpdates)

            Result.success()

        } catch (e: Exception) {
            if (runAttemptCount < 3) {
                Result.retry()
            } else {
                Result.failure()
            }
        }
    }

    private suspend fun checkForWeatherAlerts(weatherData: List<WeatherData>) {
        val alertService = WeatherAlertService()

        weatherData.forEach { weather ->
            val alerts = alertService.checkAlerts(weather)
            alerts.forEach { alert ->
                NotificationHelper.showWeatherAlert(applicationContext, alert)
            }
        }
    }
}

// Schedule periodic sync
class WeatherSyncScheduler {
    fun schedulePeriodicSync(context: Context) {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .setRequiresBatteryNotLow(true)
            .build()

        val periodicWork = PeriodicWorkRequestBuilder<WeatherSyncWorker>(15, TimeUnit.MINUTES)
            .setConstraints(constraints)
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()

        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            "weather_sync",
            ExistingPeriodicWorkPolicy.KEEP,
            periodicWork
        )
    }
}
```

## Data Management

### 1. Offline-First Architecture

**✅ Implement Robust Offline Support**

```swift
// iOS Swift - Offline-first data management
class CarbonDataManager: ObservableObject {
    private let coreDataManager = CoreDataManager.shared
    private let apiClient = WRFCoinSDK.shared.carbonService
    private let connectivityMonitor = NetworkConnectivityMonitor()

    @Published var carbonCredits: [CarbonCredit] = []
    @Published var syncStatus: SyncStatus = .synced

    enum SyncStatus {
        case synced
        case syncing
        case offline
        case error(String)
    }

    init() {
        loadCachedData()
        setupConnectivityMonitoring()
    }

    // Load data - cache first, then sync
    func loadCarbonCredits() async {
        // Always load cached data first for immediate UI update
        loadCachedData()

        // Sync with server if online
        if connectivityMonitor.isConnected {
            await syncWithServer()
        } else {
            syncStatus = .offline
        }
    }

    private func loadCachedData() {
        do {
            let cached = try coreDataManager.fetchCarbonCredits()
            self.carbonCredits = cached
        } catch {
            print("Failed to load cached data: \(error)")
        }
    }

    private func syncWithServer() async {
        syncStatus = .syncing

        do {
            let serverCredits = try await apiClient.getAvailableCredits()

            // Update cache
            try coreDataManager.updateCarbonCredits(serverCredits)

            // Update UI
            self.carbonCredits = serverCredits
            self.syncStatus = .synced

        } catch {
            syncStatus = .error(error.localizedDescription)
        }
    }

    // Purchase with offline queue
    func purchaseCredit(_ credit: CarbonCredit, quantity: Double) async throws {
        let purchase = CarbonCreditPurchase(
            creditId: credit.id,
            quantity: quantity,
            price: credit.price,
            timestamp: Date()
        )

        if connectivityMonitor.isConnected {
            // Execute immediately
            let transaction = try await apiClient.purchaseCredit(purchase)
            try coreDataManager.savePurchase(purchase, transaction: transaction)
        } else {
            // Queue for later
            try coreDataManager.queuePurchase(purchase)
        }
    }

    private func setupConnectivityMonitoring() {
        connectivityMonitor.onConnectivityChanged = { [weak self] isConnected in
            if isConnected {
                Task {
                    await self?.processPendingOperations()
                    await self?.syncWithServer()
                }
            } else {
                self?.syncStatus = .offline
            }
        }
    }

    private func processPendingOperations() async {
        do {
            let pendingPurchases = try coreDataManager.fetchPendingPurchases()

            for purchase in pendingPurchases {
                do {
                    let transaction = try await apiClient.purchaseCredit(purchase)
                    try coreDataManager.completePurchase(purchase, transaction: transaction)
                } catch {
                    // Mark as failed, will retry later
                    print("Failed to process pending purchase: \(error)")
                }
            }
        } catch {
            print("Failed to process pending operations: \(error)")
        }
    }
}
```

### 2. Data Validation and Integrity

**✅ Implement Comprehensive Data Validation**

```java
// Java - Data validation with constraints
@Entity
@Table(name = "weather_data")
public class WeatherData {
    @Id
    private String id;

    @NotNull
    @DecimalMin(value = "-100.0", message = "Temperature cannot be below -100°C")
    @DecimalMax(value = "100.0", message = "Temperature cannot exceed 100°C")
    private Double temperature;

    @NotNull
    @Min(value = 0, message = "Humidity cannot be negative")
    @Max(value = 100, message = "Humidity cannot exceed 100%")
    private Integer humidity;

    @NotNull
    @DecimalMin(value = "-90.0")
    @DecimalMax(value = "90.0")
    private Double latitude;

    @NotNull
    @DecimalMin(value = "-180.0")
    @DecimalMax(value = "180.0")
    private Double longitude;

    @PastOrPresent(message = "Timestamp cannot be in the future")
    private Instant timestamp;

    // Custom validation
    @AssertTrue(message = "Weather data integrity check failed")
    public boolean isDataIntegrityValid() {
        // Custom business logic validation
        if (temperature != null && humidity != null) {
            // Example: Very low temperatures should have low humidity
            if (temperature < -20 && humidity > 80) {
                return false;
            }
        }
        return true;
    }
}

@Service
public class WeatherDataValidator {
    private final Validator validator;

    public void validateWeatherData(WeatherData data) throws ValidationException {
        Set<ConstraintViolation<WeatherData>> violations = validator.validate(data);

        if (!violations.isEmpty()) {
            String errors = violations.stream()
                .map(ConstraintViolation::getMessage)
                .collect(Collectors.joining(", "));
            throw new ValidationException("Weather data validation failed: " + errors);
        }

        // Additional business logic validation
        validateBusinessRules(data);
    }

    private void validateBusinessRules(WeatherData data) throws ValidationException {
        // Check for suspicious data patterns
        if (isAnomalousReading(data)) {
            throw new ValidationException("Weather data appears anomalous");
        }

        // Validate against historical data
        if (!isWithinExpectedRange(data)) {
            throw new ValidationException("Weather data outside expected range for location");
        }
    }
}
```

## Real-time Features

### 1. WebSocket Connection Management

**✅ Robust WebSocket Implementation**

```csharp
// C# - Robust WebSocket connection
public class RealtimeConnectionManager : IDisposable
{
    private ClientWebSocket _webSocket;
    private CancellationTokenSource _cancellationTokenSource;
    private readonly SemaphoreSlim _connectionSemaphore = new(1, 1);
    private readonly Timer _heartbeatTimer;
    private readonly Subject<string> _messageSubject = new();

    public IObservable<string> Messages => _messageSubject.AsObservable();
    public bool IsConnected => _webSocket?.State == WebSocketState.Open;

    public RealtimeConnectionManager()
    {
        _heartbeatTimer = new Timer(SendHeartbeat, null, TimeSpan.Zero, TimeSpan.FromSeconds(30));
    }

    public async Task ConnectAsync(Uri uri)
    {
        await _connectionSemaphore.WaitAsync();

        try
        {
            if (IsConnected) return;

            _cancellationTokenSource?.Cancel();
            _cancellationTokenSource = new CancellationTokenSource();

            _webSocket = new ClientWebSocket();
            await _webSocket.ConnectAsync(uri, _cancellationTokenSource.Token);

            // Start receiving messages
            _ = Task.Run(() => ReceiveLoop(_cancellationTokenSource.Token));

            // Send authentication
            await SendAuthenticationAsync();
        }
        finally
        {
            _connectionSemaphore.Release();
        }
    }

    private async Task ReceiveLoop(CancellationToken cancellationToken)
    {
        var buffer = new byte[4096];

        while (!cancellationToken.IsCancellationRequested && IsConnected)
        {
            try
            {
                var result = await _webSocket.ReceiveAsync(new ArraySegment<byte>(buffer), cancellationToken);

                if (result.MessageType == WebSocketMessageType.Text)
                {
                    var message = Encoding.UTF8.GetString(buffer, 0, result.Count);
                    _messageSubject.OnNext(message);
                }
                else if (result.MessageType == WebSocketMessageType.Close)
                {
                    await HandleDisconnection();
                    break;
                }
            }
            catch (WebSocketException ex)
            {
                // Handle connection errors and attempt reconnection
                await HandleConnectionError(ex);
                break;
            }
        }
    }

    private async Task HandleDisconnection()
    {
        // Implement exponential backoff reconnection
        var delay = TimeSpan.FromSeconds(1);
        var maxDelay = TimeSpan.FromMinutes(5);
        var attempt = 0;

        while (attempt < 10)
        {
            try
            {
                await Task.Delay(delay);
                await ConnectAsync(_lastConnectedUri);
                return; // Successfully reconnected
            }
            catch
            {
                attempt++;
                delay = TimeSpan.FromMilliseconds(Math.Min(delay.TotalMilliseconds * 2, maxDelay.TotalMilliseconds));
            }
        }
    }

    public async Task SendAsync(string message)
    {
        if (!IsConnected) return;

        var buffer = Encoding.UTF8.GetBytes(message);
        await _webSocket.SendAsync(
            new ArraySegment<byte>(buffer),
            WebSocketMessageType.Text,
            true,
            _cancellationTokenSource.Token
        );
    }

    private async void SendHeartbeat(object state)
    {
        if (IsConnected)
        {
            await SendAsync(JsonSerializer.Serialize(new { type = "heartbeat", timestamp = DateTimeOffset.UtcNow }));
        }
    }
}
```

### 2. Event-Driven Updates

**✅ Implement Event-Driven Architecture**

```kotlin
// Android Kotlin - Event-driven weather updates
class WeatherRealtimeManager {
    private val _weatherUpdates = MutableSharedFlow<WeatherUpdate>()
    val weatherUpdates: SharedFlow<WeatherUpdate> = _weatherUpdates.asSharedFlow()

    private val _alertUpdates = MutableSharedFlow<WeatherAlert>()
    val alertUpdates: SharedFlow<WeatherAlert> = _alertUpdates.asSharedFlow()

    private var websocketConnection: WebSocketConnection? = null
    private val subscribedLocations = mutableSetOf<Location>()

    suspend fun subscribeToLocation(location: Location) {
        subscribedLocations.add(location)

        if (websocketConnection == null) {
            initializeConnection()
        }

        sendSubscription(location)
    }

    suspend fun unsubscribeFromLocation(location: Location) {
        subscribedLocations.remove(location)
        sendUnsubscription(location)

        if (subscribedLocations.isEmpty()) {
            websocketConnection?.close()
            websocketConnection = null
        }
    }

    private suspend fun initializeConnection() {
        websocketConnection = WebSocketConnection(
            url = "wss://realtime.wrfcoin.com/weather",
            onMessage = { message -> handleMessage(message) },
            onError = { error -> handleError(error) },
            onReconnect = { reconnectSubscriptions() }
        )

        websocketConnection?.connect()
    }

    private suspend fun handleMessage(message: String) {
        try {
            val jsonObject = JSONObject(message)
            val type = jsonObject.getString("type")

            when (type) {
                "weather_update" -> {
                    val update = WeatherUpdate.fromJson(jsonObject.getJSONObject("data"))
                    _weatherUpdates.emit(update)
                }

                "weather_alert" -> {
                    val alert = WeatherAlert.fromJson(jsonObject.getJSONObject("data"))
                    _alertUpdates.emit(alert)

                    // Show notification for severe alerts
                    if (alert.severity >= AlertSeverity.SEVERE) {
                        NotificationHelper.showWeatherAlert(alert)
                    }
                }

                "subscription_confirmed" -> {
                    Log.d("WeatherRealtime", "Subscription confirmed for location")
                }
            }
        } catch (e: Exception) {
            Log.e("WeatherRealtime", "Failed to parse message: $message", e)
        }
    }

    private suspend fun reconnectSubscriptions() {
        // Re-subscribe to all locations after reconnection
        subscribedLocations.forEach { location ->
            sendSubscription(location)
        }
    }

    private suspend fun sendSubscription(location: Location) {
        val subscription = JSONObject().apply {
            put("type", "subscribe")
            put("data", JSONObject().apply {
                put("latitude", location.latitude)
                put("longitude", location.longitude)
                put("alerts", true)
                put("updates", true)
            })
        }

        websocketConnection?.send(subscription.toString())
    }
}

// Usage in ViewModel
class WeatherViewModel : ViewModel() {
    private val realtimeManager = WeatherRealtimeManager()

    private val _currentWeather = MutableLiveData<WeatherData>()
    val currentWeather: LiveData<WeatherData> = _currentWeather

    private val _weatherAlerts = MutableLiveData<List<WeatherAlert>>()
    val weatherAlerts: LiveData<List<WeatherAlert>> = _weatherAlerts

    init {
        // Subscribe to real-time updates
        viewModelScope.launch {
            realtimeManager.weatherUpdates.collect { update ->
                _currentWeather.value = update.weatherData
            }
        }

        viewModelScope.launch {
            realtimeManager.alertUpdates.collect { alert ->
                val currentAlerts = _weatherAlerts.value?.toMutableList() ?: mutableListOf()
                currentAlerts.add(alert)
                _weatherAlerts.value = currentAlerts
            }
        }
    }

    fun subscribeToLocation(location: Location) {
        viewModelScope.launch {
            realtimeManager.subscribeToLocation(location)
        }
    }
}
```

## Testing Strategies

### 1. Comprehensive Test Coverage

**✅ Implement Multi-Layer Testing**

```swift
// iOS Swift - Comprehensive testing approach
import XCTest
@testable import WRFCoinApp

class WeatherServiceTests: XCTestCase {
    var weatherService: WeatherService!
    var mockNetworkManager: MockNetworkManager!
    var mockCacheManager: MockCacheManager!

    override func setUp() {
        super.setUp()
        mockNetworkManager = MockNetworkManager()
        mockCacheManager = MockCacheManager()
        weatherService = WeatherService(
            networkManager: mockNetworkManager,
            cacheManager: mockCacheManager
        )
    }

    // Unit Tests
    func testGetCurrentWeather_WithValidLocation_ReturnsWeatherData() async throws {
        // Given
        let expectedWeather = WeatherData.mock()
        mockNetworkManager.mockResponse = expectedWeather

        // When
        let result = try await weatherService.getCurrentWeather(
            latitude: 40.7128,
            longitude: -74.0060
        )

        // Then
        XCTAssertEqual(result.temperature, expectedWeather.temperature)
        XCTAssertEqual(result.location.latitude, 40.7128, accuracy: 0.001)
    }

    func testGetCurrentWeather_WithCachedData_ReturnsCachedWeather() async throws {
        // Given
        let cachedWeather = WeatherData.mock()
        mockCacheManager.cachedWeather = cachedWeather

        // When
        let result = try await weatherService.getCurrentWeather(
            latitude: 40.7128,
            longitude: -74.0060
        )

        // Then
        XCTAssertEqual(result, cachedWeather)
        XCTAssertFalse(mockNetworkManager.wasNetworkRequestMade)
    }

    func testGetCurrentWeather_WithNetworkError_ThrowsNetworkException() async {
        // Given
        mockNetworkManager.shouldFailRequest = true

        // When/Then
        do {
            _ = try await weatherService.getCurrentWeather(latitude: 40.7128, longitude: -74.0060)
            XCTFail("Expected network error")
        } catch {
            XCTAssertTrue(error is NetworkException)
        }
    }
}

// Integration Tests
class WeatherIntegrationTests: XCTestCase {
    var app: XCUIApplication!

    override func setUp() {
        super.setUp()
        app = XCUIApplication()
        app.launchArguments = ["--uitesting"]
        app.launch()
    }

    func testWeatherFlow_EndToEnd() {
        // Test complete user flow
        app.buttons["Allow Location"].tap()

        let weatherCard = app.otherElements["WeatherCard"]
        XCTAssertTrue(weatherCard.waitForExistence(timeout: 10))

        let temperature = app.staticTexts.matching(NSPredicate(format: "label CONTAINS '°'")).firstMatch
        XCTAssertTrue(temperature.exists)

        // Test refresh functionality
        app.scrollViews.firstMatch.swipeDown()

        let loadingIndicator = app.activityIndicators.firstMatch
        XCTAssertTrue(loadingIndicator.waitForExistence(timeout: 2))
        XCTAssertTrue(weatherCard.waitForExistence(timeout: 10))
    }
}

// Performance Tests
class WeatherPerformanceTests: XCTestCase {
    func testWeatherAPIPerformance() {
        let weatherService = WeatherService()

        measure {
            let expectation = XCTestExpectation(description: "Weather API call")

            Task {
                do {
                    _ = try await weatherService.getCurrentWeather(
                        latitude: 40.7128,
                        longitude: -74.0060
                    )
                    expectation.fulfill()
                } catch {
                    XCTFail("Weather API call failed: \(error)")
                }
            }

            wait(for: [expectation], timeout: 5.0)
        }
    }
}

// Mock Objects
class MockNetworkManager: NetworkManagerProtocol {
    var mockResponse: Any?
    var shouldFailRequest = false
    var wasNetworkRequestMade = false

    func request<T: Decodable>(_ endpoint: APIEndpoint) async throws -> T {
        wasNetworkRequestMade = true

        if shouldFailRequest {
            throw NetworkException.requestFailed
        }

        guard let response = mockResponse as? T else {
            throw NetworkException.decodingFailed
        }

        return response
    }
}

// Test Data Extensions
extension WeatherData {
    static func mock(
        temperature: Double = 22.5,
        condition: String = "Sunny",
        location: Location = Location(latitude: 40.7128, longitude: -74.0060, name: "New York")
    ) -> WeatherData {
        return WeatherData(
            id: UUID().uuidString,
            temperature: temperature,
            condition: condition,
            humidity: 65,
            pressure: 1013.25,
            windSpeed: 15.0,
            windDirection: 180,
            visibility: 10.0,
            uvIndex: 6,
            location: location,
            timestamp: Date()
        )
    }
}
```

### 2. Automated Testing Pipeline

**✅ CI/CD Integration**

```yaml
# .github/workflows/test.yml
name: Cross-Platform Tests

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

jobs:
  test-java:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Set up JDK 17
        uses: actions/setup-java@v3
        with:
          java-version: '17'
          distribution: 'temurin'

      - name: Cache Maven packages
        uses: actions/cache@v3
        with:
          path: ~/.m2
          key: ${{ runner.os }}-m2-${{ hashFiles('**/pom.xml') }}

      - name: Run tests
        run: mvn clean test

      - name: Generate test report
        uses: dorny/test-reporter@v1
        if: success() || failure()
        with:
          name: Java Tests
          path: target/surefire-reports/*.xml
          reporter: java-junit

  test-csharp:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Setup .NET
        uses: actions/setup-dotnet@v3
        with:
          dotnet-version: '7.0.x'

      - name: Restore dependencies
        run: dotnet restore

      - name: Build
        run: dotnet build --no-restore

      - name: Test
        run: dotnet test --no-build --verbosity normal --collect:"XPlat Code Coverage"

      - name: Upload coverage to Codecov
        uses: codecov/codecov-action@v3

  test-android:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3
      - name: Set up JDK 17
        uses: actions/setup-java@v3
        with:
          java-version: '17'
          distribution: 'temurin'

      - name: Setup Android SDK
        uses: android-actions/setup-android@v2

      - name: Cache Gradle packages
        uses: actions/cache@v3
        with:
          path: |
            ~/.gradle/caches
            ~/.gradle/wrapper
          key:
            ${{ runner.os }}-gradle-${{ hashFiles('**/*.gradle*', '**/gradle-wrapper.properties') }}

      - name: Run unit tests
        run: ./gradlew testDebugUnitTest

      - name: Run instrumented tests
        uses: reactivecircus/android-emulator-runner@v2
        with:
          api-level: 29
          script: ./gradlew connectedDebugAndroidTest

  test-ios:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3
      - name: Select Xcode version
        run: sudo xcode-select -s /Applications/Xcode_14.2.app/Contents/Developer

      - name: Cache derived data
        uses: actions/cache@v3
        with:
          path: ~/Library/Developer/Xcode/DerivedData
          key: ${{ runner.os }}-derived-data-${{ hashFiles('**/*.xcodeproj') }}

      - name: Build and test
        run: |
          xcodebuild \
            -project WRFCoinApp.xcodeproj \
            -scheme WRFCoinApp \
            -destination 'platform=iOS Simulator,name=iPhone 14,OS=16.2' \
            clean test

  security-scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Run security scan
        uses: securecodewarrior/github-action-add-sarif@v1
        with:
          sarif-file: security-scan-results.sarif
```

This comprehensive best practices guide provides the foundation for building robust, secure, and
performant applications with the WRFCoin SDK ecosystem. Following these guidelines will ensure your
applications are production-ready and maintainable across all supported platforms.
