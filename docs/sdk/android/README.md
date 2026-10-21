# WRFCoin Android SDK

The WRFCoin Android SDK provides native Android integration for the WRFCoin weather forecasting
blockchain platform. Built with modern Android development practices, it offers seamless access to
weather data, carbon credits, DeFi operations, and IoT sensor management.

## Features

- **🌡️ Weather Data Access** - Real-time and historical weather data from 20+ sensor platforms
- **💚 Carbon Credit Trading** - Trade verified carbon credits with blockchain transparency
- **💰 DeFi Integration** - Access AMM, yield farming, and governance features
- **📱 Native Android UI** - Material Design 3 components and patterns
- **🔐 Secure Authentication** - Biometric authentication and hardware security
- **⚡ High Performance** - Coroutines-based async operations with caching
- **🌐 Offline Support** - Local data caching and offline transaction queuing
- **📊 Real-time Updates** - WebSocket connections for live data streams

## Quick Start

### Installation

Add the WRFCoin SDK to your `build.gradle.kts` (Module: app):

```kotlin
dependencies {
    implementation("com.wrfcoin:sdk-android:1.0.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")
    implementation("com.squareup.retrofit2:retrofit:2.9.0")
    implementation("com.squareup.retrofit2:converter-gson:2.9.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.7.0")
    implementation("androidx.work:work-runtime-ktx:2.9.0")
}
```

### Permissions

Add required permissions to your `AndroidManifest.xml`:

```xml
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
<uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" />
<uses-permission android:name="android.permission.ACCESS_COARSE_LOCATION" />
<uses-permission android:name="android.permission.USE_BIOMETRIC" />
<uses-permission android:name="android.permission.USE_FINGERPRINT" />
```

### Initialize SDK

Initialize the SDK in your `Application` class:

```kotlin
class WRFCoinApplication : Application() {
    override fun onCreate() {
        super.onCreate()

        val config = WRFCoinConfig.Builder()
            .apiKey("your-api-key")
            .environment(WRFCoinEnvironment.PRODUCTION) // or SANDBOX
            .enableLogging(BuildConfig.DEBUG)
            .enableOfflineSupport(true)
            .enableBiometricAuth(true)
            .build()

        WRFCoinSDK.initialize(this, config)
    }
}
```

### Basic Usage

```kotlin
class MainActivity : ComponentActivity() {
    private val wrfCoinSDK = WRFCoinSDK.getInstance()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Authenticate user
        lifecycleScope.launch {
            try {
                val authResult = wrfCoinSDK.authenticateUser("user@example.com", "password")
                Log.d("WRFCoin", "Authentication successful: ${authResult.token}")

                // Fetch weather data
                fetchWeatherData()

            } catch (e: WRFCoinException) {
                Log.e("WRFCoin", "Authentication failed", e)
            }
        }
    }

    private suspend fun fetchWeatherData() {
        try {
            val weatherData = wrfCoinSDK.weatherService.getCurrentWeather(
                latitude = 40.7128,
                longitude = -74.0060
            )
            Log.d("WRFCoin", "Temperature: ${weatherData.temperature}°C")
        } catch (e: Exception) {
            Log.e("WRFCoin", "Failed to fetch weather data", e)
        }
    }
}
```

## Table of Contents

- [Installation & Setup](installation.md)
- [Authentication](authentication.md)
- [Weather Services](weather-services.md)
- [Carbon Credit Trading](carbon-trading.md)
- [DeFi Operations](defi-operations.md)
- [IoT Integration](iot-integration.md)
- [UI Components](ui-components.md)
- [Real-time Features](realtime-features.md)
- [Offline Support](offline-support.md)
- [Security](security.md)
- [Testing](testing.md)
- [Migration Guide](migration.md)
- [API Reference](api-reference.md)
- [Examples](examples/)

## Architecture Overview

The WRFCoin Android SDK follows clean architecture principles:

```
┌─────────────────────────────────────┐
│           Presentation Layer        │
│   (Activities, Fragments, ViewModels) │
├─────────────────────────────────────┤
│            Domain Layer             │
│      (Use Cases, Repositories)      │
├─────────────────────────────────────┤
│             Data Layer              │
│  (API, Database, Preferences)       │
└─────────────────────────────────────┘
```

### Key Components

- **WRFCoinSDK** - Main SDK entry point
- **ServiceManager** - Manages all API services
- **AuthenticationManager** - Handles user authentication
- **CacheManager** - Local data caching with Room database
- **NetworkManager** - HTTP client with retry logic
- **SecurityManager** - Biometric authentication and encryption
- **RealtimeManager** - WebSocket connections and live updates

## Core Services

### Weather Service

Access comprehensive weather data from multiple sources:

```kotlin
val weatherService = WRFCoinSDK.getInstance().weatherService

// Current weather
val current = weatherService.getCurrentWeather(lat, lng)

// Forecast
val forecast = weatherService.getForecast(lat, lng, days = 7)

// Historical data
val historical = weatherService.getHistoricalWeather(
    lat, lng,
    startDate = LocalDate.now().minusDays(30),
    endDate = LocalDate.now()
)
```

### Carbon Credit Service

Trade and manage carbon credits:

```kotlin
val carbonService = WRFCoinSDK.getInstance().carbonService

// Browse available credits
val credits = carbonService.getAvailableCredits()

// Purchase credits
val purchase = carbonService.purchaseCredits(
    creditId = "carbon-123",
    quantity = 10.0,
    price = 25.50
)

// Track carbon footprint
val footprint = carbonService.calculateFootprint(
    activities = listOf(
        CarbonActivity.FLIGHT(distance = 1000.0),
        CarbonActivity.CAR_TRAVEL(distance = 500.0)
    )
)
```

### DeFi Service

Access decentralized finance features:

```kotlin
val defiService = WRFCoinSDK.getInstance().defiService

// Get liquidity pools
val pools = defiService.getLiquidityPools()

// Provide liquidity
val result = defiService.addLiquidity(
    poolId = "WRF-USDC",
    tokenAAmount = 1000.0,
    tokenBAmount = 500.0
)

// Stake tokens for rewards
val staking = defiService.stakeTokens(
    amount = 5000.0,
    duration = StakingDuration.MONTHS_6
)
```

## UI Components

The SDK includes pre-built Material Design 3 UI components:

### Weather Cards

```kotlin
@Composable
fun WeatherScreen() {
    Column {
        WeatherCurrentCard(
            weatherData = currentWeather,
            onRefresh = { viewModel.refreshWeather() }
        )

        WeatherForecastCard(
            forecastData = forecast,
            onDaySelected = { day -> viewModel.selectDay(day) }
        )

        WeatherMapView(
            location = userLocation,
            showPrecipitation = true,
            showTemperature = true
        )
    }
}
```

### Carbon Trading UI

```kotlin
@Composable
fun CarbonTradingScreen() {
    CarbonCreditList(
        credits = availableCredits,
        onCreditSelected = { credit ->
            navController.navigate("credit-details/${credit.id}")
        }
    )

    CarbonFootprintCard(
        footprint = userFootprint,
        onOffsetClicked = { amount ->
            viewModel.offsetCarbon(amount)
        }
    )
}
```

### DeFi Dashboard

```kotlin
@Composable
fun DeFiDashboard() {
    LazyColumn {
        item {
            WalletBalanceCard(
                balance = walletBalance,
                onSendClicked = { navController.navigate("send") },
                onReceiveClicked = { navController.navigate("receive") }
            )
        }

        item {
            LiquidityPoolsCard(
                pools = liquidityPools,
                onPoolSelected = { pool -> viewModel.selectPool(pool) }
            )
        }

        item {
            StakingRewardsCard(
                rewards = stakingRewards,
                onClaimClicked = { viewModel.claimRewards() }
            )
        }
    }
}
```

## Real-time Features

### Live Weather Updates

```kotlin
class WeatherViewModel : ViewModel() {
    private val weatherService = WRFCoinSDK.getInstance().weatherService

    val liveWeather = weatherService.subscribeToWeatherUpdates(
        latitude = 40.7128,
        longitude = -74.0060,
        updateInterval = Duration.ofMinutes(5)
    ).asLiveData()

    // Usage in Activity/Fragment
    weatherViewModel.liveWeather.observe(this) { weather ->
        updateUI(weather)
    }
}
```

### Price Notifications

```kotlin
class PriceAlertManager {
    fun setupPriceAlerts() {
        val carbonService = WRFCoinSDK.getInstance().carbonService

        carbonService.subscribeToPriceAlerts(
            creditType = CreditType.RENEWABLE_ENERGY,
            thresholdPrice = 20.0,
            direction = AlertDirection.BELOW
        ).onEach { alert ->
            showNotification(alert)
        }.launchIn(viewLifecycleOwner.lifecycleScope)
    }
}
```

## Security Features

### Biometric Authentication

```kotlin
class AuthenticationManager {
    suspend fun authenticateWithBiometrics(
        context: Context,
        title: String = "Authenticate with WRFCoin"
    ): AuthResult {
        return withContext(Dispatchers.Main) {
            val biometricPrompt = BiometricPrompt(
                context as FragmentActivity,
                ContextCompat.getMainExecutor(context),
                object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        // Handle success
                    }

                    override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                        // Handle error
                    }
                }
            )

            val promptInfo = BiometricPrompt.PromptInfo.Builder()
                .setTitle(title)
                .setSubtitle("Use your fingerprint or face to authenticate")
                .setNegativeButtonText("Cancel")
                .build()

            biometricPrompt.authenticate(promptInfo)
        }
    }
}
```

### Secure Storage

```kotlin
class SecurePreferences {
    private val encryptedPrefs = EncryptedSharedPreferences.create(
        "wrfcoin_secure_prefs",
        masterKeyAlias,
        context,
        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
    )

    fun storeApiKey(apiKey: String) {
        encryptedPrefs.edit().putString("api_key", apiKey).apply()
    }

    fun getApiKey(): String? {
        return encryptedPrefs.getString("api_key", null)
    }
}
```

## Offline Support

### Data Caching

```kotlin
@Database(
    entities = [WeatherData::class, CarbonCredit::class, Transaction::class],
    version = 1
)
abstract class WRFCoinDatabase : RoomDatabase() {
    abstract fun weatherDao(): WeatherDao
    abstract fun carbonDao(): CarbonDao
    abstract fun transactionDao(): TransactionDao
}

class OfflineManager {
    suspend fun cacheWeatherData(weatherData: WeatherData) {
        database.weatherDao().insertWeatherData(weatherData)
    }

    suspend fun getCachedWeatherData(lat: Double, lng: Double): WeatherData? {
        return database.weatherDao().getWeatherData(lat, lng)
    }
}
```

### Offline Transaction Queue

```kotlin
class TransactionQueue {
    fun queueTransaction(transaction: Transaction) {
        WorkManager.getInstance(context)
            .enqueueUniqueWork(
                "sync_transaction_${transaction.id}",
                ExistingWorkPolicy.KEEP,
                OneTimeWorkRequestBuilder<TransactionSyncWorker>()
                    .setInputData(workDataOf("transaction_id" to transaction.id))
                    .setConstraints(
                        Constraints.Builder()
                            .setRequiredNetworkType(NetworkType.CONNECTED)
                            .build()
                    )
                    .build()
            )
    }
}
```

## Testing

### Unit Testing with MockK

```kotlin
class WeatherServiceTest {
    @MockK
    private lateinit var weatherApi: WeatherApi

    @MockK
    private lateinit var cacheManager: CacheManager

    private lateinit var weatherService: WeatherService

    @Before
    fun setup() {
        MockKAnnotations.init(this)
        weatherService = WeatherService(weatherApi, cacheManager)
    }

    @Test
    fun `getCurrentWeather should return cached data when available`() = runTest {
        // Given
        val cachedWeather = WeatherData(temperature = 25.0, humidity = 60)
        coEvery { cacheManager.getCachedWeather(any(), any()) } returns cachedWeather

        // When
        val result = weatherService.getCurrentWeather(40.7128, -74.0060)

        // Then
        assertEquals(cachedWeather, result)
        coVerify(exactly = 0) { weatherApi.getCurrentWeather(any(), any()) }
    }
}
```

### UI Testing with Compose

```kotlin
class WeatherScreenTest {
    @get:Rule
    val composeTestRule = createComposeRule()

    @Test
    fun weatherScreen_displaysCurrentTemperature() {
        val mockWeatherData = WeatherData(
            temperature = 22.5,
            condition = "Sunny",
            humidity = 65
        )

        composeTestRule.setContent {
            WeatherCurrentCard(weatherData = mockWeatherData)
        }

        composeTestRule
            .onNodeWithText("22.5°C")
            .assertIsDisplayed()

        composeTestRule
            .onNodeWithText("Sunny")
            .assertIsDisplayed()
    }
}
```

## Performance Optimization

### Memory Management

```kotlin
class ImageCacheManager {
    private val imageCache = LruCache<String, Bitmap>(
        (Runtime.getRuntime().maxMemory() / 1024 / 8).toInt()
    )

    fun getBitmap(url: String): Bitmap? = imageCache.get(url)

    fun putBitmap(url: String, bitmap: Bitmap) {
        imageCache.put(url, bitmap)
    }
}
```

### Network Optimization

```kotlin
class NetworkOptimizer {
    fun createOptimizedOkHttpClient(): OkHttpClient {
        return OkHttpClient.Builder()
            .addInterceptor(HttpLoggingInterceptor().apply {
                level = if (BuildConfig.DEBUG)
                    HttpLoggingInterceptor.Level.BODY
                else
                    HttpLoggingInterceptor.Level.NONE
            })
            .addInterceptor(CacheInterceptor())
            .addNetworkInterceptor(NetworkCacheInterceptor())
            .cache(Cache(context.cacheDir, 10 * 1024 * 1024)) // 10MB cache
            .connectTimeout(30, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .build()
    }
}
```

## Error Handling

### Custom Exceptions

```kotlin
sealed class WRFCoinException(message: String, cause: Throwable? = null) : Exception(message, cause) {
    class NetworkException(message: String, cause: Throwable? = null) : WRFCoinException(message, cause)
    class AuthenticationException(message: String) : WRFCoinException(message)
    class ValidationException(message: String) : WRFCoinException(message)
    class ServerException(val code: Int, message: String) : WRFCoinException(message)
    class RateLimitException(val retryAfter: Long) : WRFCoinException("Rate limit exceeded")
}
```

### Global Error Handler

```kotlin
class GlobalErrorHandler {
    fun handleError(error: Throwable): ErrorResponse {
        return when (error) {
            is WRFCoinException.NetworkException -> {
                ErrorResponse(
                    title = "Network Error",
                    message = "Please check your internet connection",
                    action = ErrorAction.RETRY
                )
            }
            is WRFCoinException.AuthenticationException -> {
                ErrorResponse(
                    title = "Authentication Failed",
                    message = "Please log in again",
                    action = ErrorAction.LOGIN
                )
            }
            else -> {
                ErrorResponse(
                    title = "Unexpected Error",
                    message = "Something went wrong. Please try again.",
                    action = ErrorAction.RETRY
                )
            }
        }
    }
}
```

## Migration Guide

### From Version 0.x to 1.0

```kotlin
// Old way (v0.x)
WRFCoin.init(context, apiKey)
val weather = WRFCoin.getWeather(lat, lng)

// New way (v1.0)
val config = WRFCoinConfig.Builder()
    .apiKey(apiKey)
    .build()
WRFCoinSDK.initialize(context, config)
val weather = WRFCoinSDK.getInstance().weatherService.getCurrentWeather(lat, lng)
```

## Support

- **Documentation**: [https://docs.wrfcoin.com/android](https://docs.wrfcoin.com/android)
- **GitHub Issues**:
  [https://github.com/wrfcoin/android-sdk/issues](https://github.com/wrfcoin/android-sdk/issues)
- **Stack Overflow**: Tag your questions with `wrfcoin-android`
- **Discord**: [WRFCoin Developer Community](https://discord.gg/wrfcoin)
- **Email**: android-sdk@wrfcoin.com

## License

```
MIT License

Copyright (c) 2024 WRFCoin

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
