# Weather App Example - Android SDK

This example demonstrates building a complete weather application using the WRFCoin Android SDK with
Material Design 3, Jetpack Compose, and modern Android architecture.

## Overview

The Weather App showcases:

- Real-time weather data display
- 7-day weather forecast
- Location-based services
- Offline data caching
- Pull-to-refresh functionality
- Dark/Light theme support
- Weather alerts and notifications

## Project Structure

```
weather-app/
├── app/
│   ├── src/main/java/com/example/weatherapp/
│   │   ├── data/
│   │   │   ├── repository/
│   │   │   ├── local/
│   │   │   └── remote/
│   │   ├── domain/
│   │   │   ├── model/
│   │   │   ├── repository/
│   │   │   └── usecase/
│   │   ├── presentation/
│   │   │   ├── ui/
│   │   │   ├── viewmodel/
│   │   │   └── navigation/
│   │   └── di/
│   └── build.gradle.kts
├── gradle/
└── build.gradle.kts
```

## Dependencies

Add to your `build.gradle.kts` (Module: app):

```kotlin
dependencies {
    // WRFCoin SDK
    implementation("com.wrfcoin:sdk-android:1.0.0")

    // Android Core
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.7.0")
    implementation("androidx.activity:activity-compose:1.8.2")

    // Compose
    implementation(platform("androidx.compose:compose-bom:2024.02.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")

    // ViewModel
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.7.0")

    // Navigation
    implementation("androidx.navigation:navigation-compose:2.7.6")

    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")

    // Dependency Injection
    implementation("com.google.dagger:hilt-android:2.48")
    kapt("com.google.dagger:hilt-compiler:2.48")
    implementation("androidx.hilt:hilt-navigation-compose:1.1.0")

    // Location Services
    implementation("com.google.android.gms:play-services-location:21.0.1")
    implementation("com.google.android.gms:play-services-maps:18.2.0")

    // Image Loading
    implementation("io.coil-kt:coil-compose:2.5.0")

    // Local Database
    implementation("androidx.room:room-runtime:2.6.1")
    implementation("androidx.room:room-ktx:2.6.1")
    kapt("androidx.room:room-compiler:2.6.1")

    // Work Manager
    implementation("androidx.work:work-runtime-ktx:2.9.0")

    // Testing
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.mockito.kotlin:mockito-kotlin:5.2.1")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.7.3")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
}
```

## Application Class

```kotlin
@HiltAndroidApp
class WeatherApplication : Application() {
    override fun onCreate() {
        super.onCreate()

        initializeWRFCoinSDK()
    }

    private fun initializeWRFCoinSDK() {
        val config = WRFCoinConfig.Builder()
            .apiKey(BuildConfig.WRFCOIN_API_KEY)
            .environment(if (BuildConfig.DEBUG) WRFCoinEnvironment.SANDBOX else WRFCoinEnvironment.PRODUCTION)
            .enableLogging(BuildConfig.DEBUG)
            .enableOfflineSupport(true)
            .cacheSize(50 * 1024 * 1024) // 50MB cache
            .requestTimeout(30) // 30 seconds
            .build()

        WRFCoinSDK.initialize(this, config)
    }
}
```

## Data Layer

### Weather Repository Interface

```kotlin
interface WeatherRepository {
    suspend fun getCurrentWeather(lat: Double, lng: Double): Result<WeatherData>
    suspend fun getWeatherForecast(lat: Double, lng: Double, days: Int): Result<List<WeatherForecast>>
    suspend fun getHistoricalWeather(lat: Double, lng: Double, startDate: LocalDate, endDate: LocalDate): Result<List<WeatherData>>
    fun subscribeToWeatherUpdates(lat: Double, lng: Double): Flow<WeatherData>
    suspend fun searchLocations(query: String): Result<List<Location>>
}
```

### Weather Repository Implementation

```kotlin
@Singleton
class WeatherRepositoryImpl @Inject constructor(
    private val weatherService: WeatherService,
    private val weatherDao: WeatherDao,
    private val locationDao: LocationDao,
    private val connectivityManager: ConnectivityManager
) : WeatherRepository {

    override suspend fun getCurrentWeather(lat: Double, lng: Double): Result<WeatherData> {
        return try {
            if (isNetworkAvailable()) {
                val weather = weatherService.getCurrentWeather(lat, lng)
                // Cache the result
                weatherDao.insertWeather(weather.toEntity())
                Result.success(weather)
            } else {
                // Return cached data if available
                val cached = weatherDao.getCurrentWeather(lat, lng)
                if (cached != null) {
                    Result.success(cached.toDomain())
                } else {
                    Result.failure(NetworkException("No network connection and no cached data available"))
                }
            }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    override suspend fun getWeatherForecast(lat: Double, lng: Double, days: Int): Result<List<WeatherForecast>> {
        return try {
            if (isNetworkAvailable()) {
                val forecast = weatherService.getForecast(lat, lng, days)
                // Cache forecast data
                weatherDao.insertForecast(forecast.map { it.toEntity() })
                Result.success(forecast)
            } else {
                val cached = weatherDao.getForecast(lat, lng, days)
                Result.success(cached.map { it.toDomain() })
            }
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    override fun subscribeToWeatherUpdates(lat: Double, lng: Double): Flow<WeatherData> {
        return weatherService.subscribeToWeatherUpdates(lat, lng, Duration.ofMinutes(5))
            .onEach { weather ->
                // Cache real-time updates
                weatherDao.insertWeather(weather.toEntity())
            }
    }

    override suspend fun searchLocations(query: String): Result<List<Location>> {
        return try {
            val locations = weatherService.searchLocations(query)
            // Cache frequently searched locations
            locationDao.insertLocations(locations.map { it.toEntity() })
            Result.success(locations)
        } catch (e: Exception) {
            // Return cached locations if search fails
            val cached = locationDao.searchCachedLocations(query)
            if (cached.isNotEmpty()) {
                Result.success(cached.map { it.toDomain() })
            } else {
                Result.failure(e)
            }
        }
    }

    private fun isNetworkAvailable(): Boolean {
        val network = connectivityManager.activeNetwork ?: return false
        val capabilities = connectivityManager.getNetworkCapabilities(network) ?: return false
        return capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
    }
}
```

## Domain Layer

### Weather Use Cases

```kotlin
class GetCurrentWeatherUseCase @Inject constructor(
    private val weatherRepository: WeatherRepository
) {
    suspend operator fun invoke(lat: Double, lng: Double): Result<WeatherData> {
        return weatherRepository.getCurrentWeather(lat, lng)
    }
}

class GetWeatherForecastUseCase @Inject constructor(
    private val weatherRepository: WeatherRepository
) {
    suspend operator fun invoke(lat: Double, lng: Double, days: Int = 7): Result<List<WeatherForecast>> {
        return weatherRepository.getWeatherForecast(lat, lng, days)
    }
}

class SearchLocationsUseCase @Inject constructor(
    private val weatherRepository: WeatherRepository
) {
    suspend operator fun invoke(query: String): Result<List<Location>> {
        return if (query.length >= 3) {
            weatherRepository.searchLocations(query)
        } else {
            Result.success(emptyList())
        }
    }
}
```

## Presentation Layer

### Weather ViewModel

```kotlin
@HiltViewModel
class WeatherViewModel @Inject constructor(
    private val getCurrentWeatherUseCase: GetCurrentWeatherUseCase,
    private val getWeatherForecastUseCase: GetWeatherForecastUseCase,
    private val searchLocationsUseCase: SearchLocationsUseCase,
    private val locationProvider: LocationProvider,
    savedStateHandle: SavedStateHandle
) : ViewModel() {

    private val _uiState = MutableStateFlow(WeatherUiState())
    val uiState: StateFlow<WeatherUiState> = _uiState.asStateFlow()

    private val _searchState = MutableStateFlow(SearchUiState())
    val searchState: StateFlow<SearchUiState> = _searchState.asStateFlow()

    init {
        getCurrentLocation()
    }

    fun getCurrentLocation() {
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true) }

            try {
                val location = locationProvider.getCurrentLocation()
                fetchWeatherData(location.latitude, location.longitude)
            } catch (e: Exception) {
                _uiState.update {
                    it.copy(
                        isLoading = false,
                        error = "Unable to get location: ${e.message}"
                    )
                }
            }
        }
    }

    fun fetchWeatherData(lat: Double, lng: Double) {
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, error = null) }

            // Fetch current weather
            val currentWeatherResult = getCurrentWeatherUseCase(lat, lng)

            // Fetch forecast
            val forecastResult = getWeatherForecastUseCase(lat, lng, 7)

            _uiState.update { currentState ->
                currentState.copy(
                    isLoading = false,
                    currentWeather = currentWeatherResult.getOrNull(),
                    forecast = forecastResult.getOrNull() ?: emptyList(),
                    error = when {
                        currentWeatherResult.isFailure -> currentWeatherResult.exceptionOrNull()?.message
                        forecastResult.isFailure -> forecastResult.exceptionOrNull()?.message
                        else -> null
                    }
                )
            }
        }
    }

    fun searchLocations(query: String) {
        viewModelScope.launch {
            _searchState.update { it.copy(isSearching = true) }

            val result = searchLocationsUseCase(query)

            _searchState.update {
                it.copy(
                    isSearching = false,
                    searchResults = result.getOrNull() ?: emptyList(),
                    searchError = result.exceptionOrNull()?.message
                )
            }
        }
    }

    fun selectLocation(location: Location) {
        fetchWeatherData(location.latitude, location.longitude)
        _searchState.update { it.copy(searchResults = emptyList()) }
    }

    fun refreshWeather() {
        val currentLocation = _uiState.value.currentWeather?.location
        if (currentLocation != null) {
            fetchWeatherData(currentLocation.latitude, currentLocation.longitude)
        } else {
            getCurrentLocation()
        }
    }
}

data class WeatherUiState(
    val isLoading: Boolean = false,
    val currentWeather: WeatherData? = null,
    val forecast: List<WeatherForecast> = emptyList(),
    val error: String? = null
)

data class SearchUiState(
    val isSearching: Boolean = false,
    val searchResults: List<Location> = emptyList(),
    val searchError: String? = null
)
```

### Weather Screen

```kotlin
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WeatherScreen(
    viewModel: WeatherViewModel = hiltViewModel()
) {
    val uiState by viewModel.uiState.collectAsState()
    val searchState by viewModel.searchState.collectAsState()
    val pullRefreshState = rememberPullRefreshState(
        refreshing = uiState.isLoading,
        onRefresh = { viewModel.refreshWeather() }
    )

    Box(
        modifier = Modifier
            .fillMaxSize()
            .pullRefresh(pullRefreshState)
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
        ) {
            // Search Bar
            SearchBar(
                onSearchClick = { viewModel.searchLocations(it) },
                searchResults = searchState.searchResults,
                onLocationSelected = { viewModel.selectLocation(it) }
            )

            when {
                uiState.error != null -> {
                    ErrorCard(
                        error = uiState.error,
                        onRetryClick = { viewModel.refreshWeather() }
                    )
                }
                uiState.currentWeather != null -> {
                    // Current Weather Card
                    CurrentWeatherCard(
                        weather = uiState.currentWeather,
                        modifier = Modifier.padding(16.dp)
                    )

                    // Hourly Forecast
                    HourlyForecastSection(
                        hourlyData = uiState.currentWeather.hourlyForecast,
                        modifier = Modifier.padding(horizontal = 16.dp)
                    )

                    // Daily Forecast
                    DailyForecastSection(
                        forecast = uiState.forecast,
                        modifier = Modifier.padding(16.dp)
                    )

                    // Weather Details
                    WeatherDetailsCard(
                        weather = uiState.currentWeather,
                        modifier = Modifier.padding(16.dp)
                    )

                    // Weather Map
                    WeatherMapCard(
                        location = uiState.currentWeather.location,
                        modifier = Modifier.padding(16.dp)
                    )
                }
            }
        }

        PullRefreshIndicator(
            refreshing = uiState.isLoading,
            state = pullRefreshState,
            modifier = Modifier.align(Alignment.TopCenter)
        )
    }
}
```

### UI Components

#### Current Weather Card

```kotlin
@Composable
fun CurrentWeatherCard(
    weather: WeatherData,
    modifier: Modifier = Modifier
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        elevation = CardDefaults.cardElevation(defaultElevation = 8.dp)
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            // Location
            Text(
                text = weather.location.name,
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold
            )

            Text(
                text = weather.location.country,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Weather Icon and Temperature
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(16.dp)
            ) {
                AsyncImage(
                    model = weather.iconUrl,
                    contentDescription = weather.condition,
                    modifier = Modifier.size(80.dp)
                )

                Text(
                    text = "${weather.temperature.roundToInt()}°",
                    style = MaterialTheme.typography.displayLarge,
                    fontWeight = FontWeight.Light
                )
            }

            // Weather Condition
            Text(
                text = weather.condition,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            // Feels Like
            Text(
                text = "Feels like ${weather.feelsLike.roundToInt()}°",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Weather Stats
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly
            ) {
                WeatherStat(
                    icon = Icons.Default.Water,
                    label = "Humidity",
                    value = "${weather.humidity}%"
                )

                WeatherStat(
                    icon = Icons.Default.Air,
                    label = "Wind",
                    value = "${weather.windSpeed} km/h"
                )

                WeatherStat(
                    icon = Icons.Default.Visibility,
                    label = "Visibility",
                    value = "${weather.visibility} km"
                )
            }
        }
    }
}

@Composable
fun WeatherStat(
    icon: ImageVector,
    label: String,
    value: String
) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(
            imageVector = icon,
            contentDescription = label,
            tint = MaterialTheme.colorScheme.primary,
            modifier = Modifier.size(24.dp)
        )

        Spacer(modifier = Modifier.height(4.dp))

        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Bold
        )

        Text(
            text = label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
    }
}
```

#### Daily Forecast Section

```kotlin
@Composable
fun DailyForecastSection(
    forecast: List<WeatherForecast>,
    modifier: Modifier = Modifier
) {
    Card(
        modifier = modifier.fillMaxWidth()
    ) {
        Column(
            modifier = Modifier.padding(16.dp)
        ) {
            Text(
                text = "7-Day Forecast",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(bottom = 16.dp)
            )

            forecast.forEach { day ->
                DailyForecastItem(
                    forecast = day,
                    modifier = Modifier.fillMaxWidth()
                )

                if (day != forecast.last()) {
                    HorizontalDivider(
                        modifier = Modifier.padding(vertical = 8.dp)
                    )
                }
            }
        }
    }
}

@Composable
fun DailyForecastItem(
    forecast: WeatherForecast,
    modifier: Modifier = Modifier
) {
    Row(
        modifier = modifier.padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        // Day
        Text(
            text = if (forecast.date.isEqual(LocalDate.now())) {
                "Today"
            } else {
                forecast.date.dayOfWeek.getDisplayName(TextStyle.SHORT, Locale.getDefault())
            },
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.width(60.dp)
        )

        // Weather Icon and Condition
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.weight(1f)
        ) {
            AsyncImage(
                model = forecast.iconUrl,
                contentDescription = forecast.condition,
                modifier = Modifier.size(32.dp)
            )

            Text(
                text = forecast.condition,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }

        // Temperature Range
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = "${forecast.maxTemperature.roundToInt()}°",
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Bold
            )

            Text(
                text = "${forecast.minTemperature.roundToInt()}°",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}
```

#### Search Bar with Location Results

```kotlin
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SearchBar(
    onSearchClick: (String) -> Unit,
    searchResults: List<Location>,
    onLocationSelected: (Location) -> Unit,
    modifier: Modifier = Modifier
) {
    var query by remember { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }

    ExposedDropdownMenuBox(
        expanded = expanded && searchResults.isNotEmpty(),
        onExpandedChange = { expanded = it },
        modifier = modifier.padding(16.dp)
    ) {
        OutlinedTextField(
            value = query,
            onValueChange = {
                query = it
                if (it.length >= 3) {
                    onSearchClick(it)
                    expanded = true
                }
            },
            label = { Text("Search location") },
            leadingIcon = {
                Icon(
                    imageVector = Icons.Default.Search,
                    contentDescription = "Search"
                )
            },
            trailingIcon = {
                if (query.isNotEmpty()) {
                    IconButton(onClick = {
                        query = ""
                        expanded = false
                    }) {
                        Icon(
                            imageVector = Icons.Default.Clear,
                            contentDescription = "Clear"
                        )
                    }
                }
            },
            modifier = Modifier
                .fillMaxWidth()
                .menuAnchor()
        )

        ExposedDropdownMenu(
            expanded = expanded && searchResults.isNotEmpty(),
            onDismissRequest = { expanded = false }
        ) {
            searchResults.forEach { location ->
                DropdownMenuItem(
                    text = {
                        Column {
                            Text(
                                text = location.name,
                                style = MaterialTheme.typography.bodyMedium
                            )
                            Text(
                                text = "${location.region}, ${location.country}",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    },
                    onClick = {
                        onLocationSelected(location)
                        query = location.name
                        expanded = false
                    }
                )
            }
        }
    }
}
```

## Navigation Setup

```kotlin
@Composable
fun WeatherNavigation() {
    val navController = rememberNavController()

    NavHost(
        navController = navController,
        startDestination = "weather"
    ) {
        composable("weather") {
            WeatherScreen()
        }

        composable("settings") {
            SettingsScreen(navController)
        }

        composable("about") {
            AboutScreen(navController)
        }
    }
}
```

## Location Services

```kotlin
@Singleton
class LocationProvider @Inject constructor(
    @ApplicationContext private val context: Context
) {
    private val fusedLocationClient = LocationServices.getFusedLocationProviderClient(context)

    @SuppressLint("MissingPermission")
    suspend fun getCurrentLocation(): Location = suspendCoroutine { continuation ->
        fusedLocationClient.lastLocation
            .addOnSuccessListener { location ->
                if (location != null) {
                    continuation.resume(
                        Location(
                            latitude = location.latitude,
                            longitude = location.longitude,
                            name = "Current Location",
                            region = "",
                            country = ""
                        )
                    )
                } else {
                    // Request fresh location
                    requestNewLocationData(continuation)
                }
            }
            .addOnFailureListener { exception ->
                continuation.resumeWithException(exception)
            }
    }

    @SuppressLint("MissingPermission")
    private fun requestNewLocationData(continuation: Continuation<Location>) {
        val locationRequest = LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, 10000)
            .setWaitForAccurateLocation(false)
            .setMinUpdateIntervalMillis(5000)
            .setMaxUpdateDelayMillis(15000)
            .build()

        val locationCallback = object : LocationCallback() {
            override fun onLocationResult(locationResult: LocationResult) {
                val lastLocation = locationResult.lastLocation
                if (lastLocation != null) {
                    continuation.resume(
                        Location(
                            latitude = lastLocation.latitude,
                            longitude = lastLocation.longitude,
                            name = "Current Location",
                            region = "",
                            country = ""
                        )
                    )
                    fusedLocationClient.removeLocationUpdates(this)
                }
            }
        }

        fusedLocationClient.requestLocationUpdates(
            locationRequest,
            locationCallback,
            Looper.getMainLooper()
        )
    }
}
```

## Dependency Injection

```kotlin
@Module
@InstallIn(SingletonComponent::class)
object AppModule {

    @Provides
    @Singleton
    fun provideWRFCoinSDK(): WRFCoinSDK = WRFCoinSDK.getInstance()

    @Provides
    @Singleton
    fun provideWeatherService(sdk: WRFCoinSDK): WeatherService = sdk.weatherService

    @Provides
    @Singleton
    fun provideConnectivityManager(
        @ApplicationContext context: Context
    ): ConnectivityManager {
        return context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    }
}

@Module
@InstallIn(SingletonComponent::class)
object DatabaseModule {

    @Provides
    @Singleton
    fun provideWeatherDatabase(
        @ApplicationContext context: Context
    ): WeatherDatabase {
        return Room.databaseBuilder(
            context,
            WeatherDatabase::class.java,
            "weather_database"
        ).build()
    }

    @Provides
    fun provideWeatherDao(database: WeatherDatabase): WeatherDao = database.weatherDao()

    @Provides
    fun provideLocationDao(database: WeatherDatabase): LocationDao = database.locationDao()
}
```

## Testing

### Unit Tests

```kotlin
@ExperimentalCoroutinesApi
class WeatherViewModelTest {
    @get:Rule
    val mainDispatcherRule = MainDispatcherRule()

    @MockK
    private lateinit var getCurrentWeatherUseCase: GetCurrentWeatherUseCase

    @MockK
    private lateinit var getWeatherForecastUseCase: GetWeatherForecastUseCase

    @MockK
    private lateinit var locationProvider: LocationProvider

    private lateinit var viewModel: WeatherViewModel

    @Before
    fun setup() {
        MockKAnnotations.init(this)
        viewModel = WeatherViewModel(
            getCurrentWeatherUseCase,
            getWeatherForecastUseCase,
            mockk(),
            locationProvider,
            SavedStateHandle()
        )
    }

    @Test
    fun `fetchWeatherData should update UI state with weather data`() = runTest {
        // Given
        val weatherData = WeatherData(
            temperature = 25.0,
            condition = "Sunny",
            humidity = 60,
            location = Location(40.7128, -74.0060, "New York", "NY", "US")
        )
        val forecast = listOf(
            WeatherForecast(
                date = LocalDate.now().plusDays(1),
                maxTemperature = 28.0,
                minTemperature = 18.0,
                condition = "Partly Cloudy"
            )
        )

        coEvery { getCurrentWeatherUseCase(any(), any()) } returns Result.success(weatherData)
        coEvery { getWeatherForecastUseCase(any(), any(), any()) } returns Result.success(forecast)

        // When
        viewModel.fetchWeatherData(40.7128, -74.0060)

        // Then
        val uiState = viewModel.uiState.value
        assertEquals(weatherData, uiState.currentWeather)
        assertEquals(forecast, uiState.forecast)
        assertFalse(uiState.isLoading)
        assertNull(uiState.error)
    }
}
```

### UI Tests

```kotlin
@HiltAndroidTest
class WeatherScreenTest {
    @get:Rule(order = 0)
    val hiltRule = HiltAndroidRule(this)

    @get:Rule(order = 1)
    val composeTestRule = createAndroidComposeRule<ComponentActivity>()

    @Before
    fun setup() {
        hiltRule.inject()
    }

    @Test
    fun weatherScreen_displaysWeatherData() {
        val mockWeatherData = WeatherData(
            temperature = 22.5,
            condition = "Sunny",
            humidity = 65,
            location = Location(40.7128, -74.0060, "New York", "NY", "US")
        )

        composeTestRule.setContent {
            MaterialTheme {
                CurrentWeatherCard(weather = mockWeatherData)
            }
        }

        composeTestRule
            .onNodeWithText("23°")
            .assertIsDisplayed()

        composeTestRule
            .onNodeWithText("Sunny")
            .assertIsDisplayed()

        composeTestRule
            .onNodeWithText("New York")
            .assertIsDisplayed()
    }
}
```

## Performance Considerations

### Memory Optimization

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

### Database Optimization

```kotlin
@Entity(tableName = "weather_data")
data class WeatherEntity(
    @PrimaryKey val id: String,
    val latitude: Double,
    val longitude: Double,
    val temperature: Double,
    val condition: String,
    val humidity: Int,
    val timestamp: Long = System.currentTimeMillis()
)

@Dao
interface WeatherDao {
    @Query("SELECT * FROM weather_data WHERE latitude = :lat AND longitude = :lng AND timestamp > :minTimestamp LIMIT 1")
    suspend fun getCurrentWeather(lat: Double, lng: Double, minTimestamp: Long = System.currentTimeMillis() - 3600000): WeatherEntity?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertWeather(weather: WeatherEntity)

    @Query("DELETE FROM weather_data WHERE timestamp < :cutoffTime")
    suspend fun cleanOldData(cutoffTime: Long)
}
```

This comprehensive Weather App example demonstrates how to build a production-ready Android
application using the WRFCoin SDK with modern Android development practices, including Jetpack
Compose, clean architecture, proper error handling, and offline support.
