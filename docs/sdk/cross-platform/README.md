# Cross-Platform Development with WRFCoin SDKs

This guide demonstrates how to build applications that work across multiple platforms using
WRFCoin's comprehensive SDK ecosystem. Learn how to share business logic, maintain consistent user
experiences, and leverage platform-specific features.

## Platform Support Matrix

| Feature            | Java | C#  | Android | iOS | Web API |
| ------------------ | ---- | --- | ------- | --- | ------- |
| Weather Data       | ✅   | ✅  | ✅      | ✅  | ✅      |
| Carbon Trading     | ✅   | ✅  | ✅      | ✅  | ✅      |
| DeFi Operations    | ✅   | ✅  | ✅      | ✅  | ✅      |
| IoT Integration    | ✅   | ✅  | ✅      | ✅  | ✅      |
| Real-time Updates  | ✅   | ✅  | ✅      | ✅  | ✅      |
| Offline Support    | ⚠️   | ⚠️  | ✅      | ✅  | ❌      |
| Biometric Auth     | ❌   | ❌  | ✅      | ✅  | ❌      |
| Push Notifications | ❌   | ❌  | ✅      | ✅  | ❌      |
| Background Sync    | ❌   | ❌  | ✅      | ✅  | ❌      |

- ✅ Full Support
- ⚠️ Limited Support (cache only)
- ❌ Not Applicable

## Architecture Patterns

### 1. Shared Business Logic

Create a common interface that can be implemented across all platforms:

#### Common Interface Definition

```typescript
// TypeScript interface for cross-platform compatibility
interface WRFCoinService {
  // Authentication
  authenticate(email: string, password: string): Promise<AuthResult>;
  logout(): Promise<void>;

  // Weather Services
  getCurrentWeather(lat: number, lng: number): Promise<WeatherData>;
  getForecast(lat: number, lng: number, days: number): Promise<WeatherForecast[]>;

  // Carbon Services
  getAvailableCredits(): Promise<CarbonCredit[]>;
  purchaseCredits(creditId: string, quantity: number): Promise<Transaction>;
  calculateFootprint(activities: CarbonActivity[]): Promise<CarbonFootprint>;

  // DeFi Services
  getLiquidityPools(): Promise<LiquidityPool[]>;
  addLiquidity(poolId: string, tokenA: number, tokenB: number): Promise<Transaction>;
  stakeTokens(amount: number, duration: StakingDuration): Promise<StakingResult>;
}
```

#### Java Implementation

```java
public class WRFCoinServiceImpl implements WRFCoinService {
    private final WRFCoinSDK sdk;

    public WRFCoinServiceImpl(String apiKey) {
        this.sdk = new WRFCoinSDK.Builder()
            .apiKey(apiKey)
            .environment(Environment.PRODUCTION)
            .build();
    }

    @Override
    public CompletableFuture<AuthResult> authenticate(String email, String password) {
        return sdk.getAuthService().authenticate(email, password);
    }

    @Override
    public CompletableFuture<WeatherData> getCurrentWeather(double lat, double lng) {
        return sdk.getWeatherService().getCurrentWeather(lat, lng);
    }

    @Override
    public CompletableFuture<List<CarbonCredit>> getAvailableCredits() {
        return sdk.getCarbonService().getAvailableCredits();
    }

    // ... other implementations
}
```

#### C# Implementation

```csharp
public class WRFCoinServiceImpl : IWRFCoinService
{
    private readonly WRFCoinSDK _sdk;

    public WRFCoinServiceImpl(string apiKey)
    {
        _sdk = new WRFCoinSDK.Builder()
            .WithApiKey(apiKey)
            .WithEnvironment(Environment.Production)
            .Build();
    }

    public async Task<AuthResult> AuthenticateAsync(string email, string password)
    {
        return await _sdk.AuthService.AuthenticateAsync(email, password);
    }

    public async Task<WeatherData> GetCurrentWeatherAsync(double lat, double lng)
    {
        return await _sdk.WeatherService.GetCurrentWeatherAsync(lat, lng);
    }

    public async Task<List<CarbonCredit>> GetAvailableCreditsAsync()
    {
        return await _sdk.CarbonService.GetAvailableCreditsAsync();
    }

    // ... other implementations
}
```

#### Android Implementation

```kotlin
class WRFCoinServiceImpl(apiKey: String) : WRFCoinService {
    private val sdk = WRFCoinSDK.getInstance()

    init {
        val config = WRFCoinConfig.Builder()
            .apiKey(apiKey)
            .environment(WRFCoinEnvironment.PRODUCTION)
            .build()
        WRFCoinSDK.configure(config)
    }

    override suspend fun authenticate(email: String, password: String): AuthResult {
        return sdk.authService.authenticate(email, password)
    }

    override suspend fun getCurrentWeather(lat: Double, lng: Double): WeatherData {
        return sdk.weatherService.getCurrentWeather(lat, lng)
    }

    override suspend fun getAvailableCredits(): List<CarbonCredit> {
        return sdk.carbonService.getAvailableCredits()
    }

    // ... other implementations
}
```

#### iOS Implementation

```swift
class WRFCoinServiceImpl: WRFCoinService {
    private let sdk = WRFCoinSDK.shared

    init(apiKey: String) {
        let config = WRFCoinConfiguration(
            apiKey: apiKey,
            environment: .production
        )
        WRFCoinSDK.configure(with: config)
    }

    func authenticate(email: String, password: String) async throws -> AuthResult {
        return try await sdk.authService.authenticate(email: email, password: password)
    }

    func getCurrentWeather(lat: Double, lng: Double) async throws -> WeatherData {
        return try await sdk.weatherService.getCurrentWeather(latitude: lat, longitude: lng)
    }

    func getAvailableCredits() async throws -> [CarbonCredit] {
        return try await sdk.carbonService.getAvailableCredits()
    }

    // ... other implementations
}
```

### 2. Data Models Synchronization

Ensure consistent data models across all platforms:

#### Shared Model Definitions

```json
{
  "WeatherData": {
    "id": "string",
    "temperature": "number",
    "humidity": "number",
    "pressure": "number",
    "windSpeed": "number",
    "windDirection": "number",
    "condition": "string",
    "iconUrl": "string",
    "visibility": "number",
    "uvIndex": "number",
    "location": "Location",
    "timestamp": "ISO8601"
  },
  "CarbonCredit": {
    "id": "string",
    "name": "string",
    "description": "string",
    "price": "number",
    "co2Amount": "number",
    "vintage": "number",
    "standard": "string",
    "projectType": "string",
    "location": "string",
    "available": "boolean",
    "verified": "boolean"
  },
  "Transaction": {
    "id": "string",
    "type": "string",
    "amount": "number",
    "currency": "string",
    "status": "string",
    "timestamp": "ISO8601",
    "blockHash": "string",
    "gasUsed": "number"
  }
}
```

#### Model Generation Script

```javascript
// generate-models.js - Generates platform-specific models from JSON schema
const fs = require('fs');
const path = require('path');

const schema = JSON.parse(fs.readFileSync('models.json', 'utf8'));

// Generate Java models
function generateJavaModel(name, properties) {
  const className = name;
  let javaCode = `public class ${className} {\n`;

  // Fields
  for (const [fieldName, fieldType] of Object.entries(properties)) {
    const javaType = mapToJavaType(fieldType);
    javaCode += `    private ${javaType} ${fieldName};\n`;
  }

  javaCode += '\n';

  // Constructor
  javaCode += `    public ${className}() {}\n\n`;

  // Getters and setters
  for (const [fieldName, fieldType] of Object.entries(properties)) {
    const javaType = mapToJavaType(fieldType);
    const capitalizedField = fieldName.charAt(0).toUpperCase() + fieldName.slice(1);

    javaCode += `    public ${javaType} get${capitalizedField}() {\n`;
    javaCode += `        return ${fieldName};\n`;
    javaCode += `    }\n\n`;

    javaCode += `    public void set${capitalizedField}(${javaType} ${fieldName}) {\n`;
    javaCode += `        this.${fieldName} = ${fieldName};\n`;
    javaCode += `    }\n\n`;
  }

  javaCode += '}';
  return javaCode;
}

// Generate C# models
function generateCSharpModel(name, properties) {
  const className = name;
  let csharpCode = `public class ${className}\n{\n`;

  for (const [fieldName, fieldType] of Object.entries(properties)) {
    const csharpType = mapToCSharpType(fieldType);
    const propertyName = fieldName.charAt(0).toUpperCase() + fieldName.slice(1);
    csharpCode += `    public ${csharpType} ${propertyName} { get; set; }\n`;
  }

  csharpCode += '}';
  return csharpCode;
}

// Generate Kotlin models
function generateKotlinModel(name, properties) {
  const className = name;
  let kotlinCode = `data class ${className}(\n`;

  const fields = Object.entries(properties).map(([fieldName, fieldType]) => {
    const kotlinType = mapToKotlinType(fieldType);
    return `    val ${fieldName}: ${kotlinType}`;
  });

  kotlinCode += fields.join(',\n');
  kotlinCode += '\n)';
  return kotlinCode;
}

// Generate Swift models
function generateSwiftModel(name, properties) {
  const structName = name;
  let swiftCode = `struct ${structName}: Codable {\n`;

  for (const [fieldName, fieldType] of Object.entries(properties)) {
    const swiftType = mapToSwiftType(fieldType);
    swiftCode += `    let ${fieldName}: ${swiftType}\n`;
  }

  swiftCode += '}';
  return swiftCode;
}

// Type mapping functions
function mapToJavaType(type) {
  switch (type) {
    case 'string':
      return 'String';
    case 'number':
      return 'Double';
    case 'boolean':
      return 'Boolean';
    case 'ISO8601':
      return 'Instant';
    default:
      return type; // Assume it's a custom type
  }
}

function mapToCSharpType(type) {
  switch (type) {
    case 'string':
      return 'string';
    case 'number':
      return 'double';
    case 'boolean':
      return 'bool';
    case 'ISO8601':
      return 'DateTime';
    default:
      return type;
  }
}

function mapToKotlinType(type) {
  switch (type) {
    case 'string':
      return 'String';
    case 'number':
      return 'Double';
    case 'boolean':
      return 'Boolean';
    case 'ISO8601':
      return 'Instant';
    default:
      return type;
  }
}

function mapToSwiftType(type) {
  switch (type) {
    case 'string':
      return 'String';
    case 'number':
      return 'Double';
    case 'boolean':
      return 'Bool';
    case 'ISO8601':
      return 'Date';
    default:
      return type;
  }
}

// Generate all models
for (const [modelName, properties] of Object.entries(schema)) {
  // Java
  fs.writeFileSync(`generated/java/${modelName}.java`, generateJavaModel(modelName, properties));

  // C#
  fs.writeFileSync(`generated/csharp/${modelName}.cs`, generateCSharpModel(modelName, properties));

  // Kotlin
  fs.writeFileSync(`generated/kotlin/${modelName}.kt`, generateKotlinModel(modelName, properties));

  // Swift
  fs.writeFileSync(`generated/swift/${modelName}.swift`, generateSwiftModel(modelName, properties));
}
```

## Cross-Platform Examples

### 1. Weather Dashboard

Create a weather dashboard that works on multiple platforms with shared business logic:

#### Shared Weather Service

```typescript
// weather-service.interface.ts
export interface WeatherService {
  getCurrentWeather(location: Location): Promise<WeatherData>;
  getForecast(location: Location, days: number): Promise<WeatherForecast[]>;
  subscribeToUpdates(location: Location, callback: (weather: WeatherData) => void): void;
  unsubscribeFromUpdates(): void;
}

export interface Location {
  latitude: number;
  longitude: number;
  name?: string;
}

export interface WeatherData {
  temperature: number;
  humidity: number;
  pressure: number;
  windSpeed: number;
  condition: string;
  iconUrl: string;
  location: Location;
  timestamp: Date;
}
```

#### Platform-Specific Implementations

**Java (Spring Boot Web App)**

```java
@RestController
@RequestMapping("/api/weather")
public class WeatherController {

    @Autowired
    private WeatherService weatherService;

    @GetMapping("/current")
    public ResponseEntity<WeatherData> getCurrentWeather(
            @RequestParam double lat,
            @RequestParam double lng) {

        Location location = new Location(lat, lng);
        WeatherData weather = weatherService.getCurrentWeather(location).join();

        return ResponseEntity.ok(weather);
    }

    @GetMapping("/forecast")
    public ResponseEntity<List<WeatherForecast>> getForecast(
            @RequestParam double lat,
            @RequestParam double lng,
            @RequestParam(defaultValue = "7") int days) {

        Location location = new Location(lat, lng);
        List<WeatherForecast> forecast = weatherService.getForecast(location, days).join();

        return ResponseEntity.ok(forecast);
    }
}
```

**C# (ASP.NET Core API)**

```csharp
[ApiController]
[Route("api/[controller]")]
public class WeatherController : ControllerBase
{
    private readonly IWeatherService _weatherService;

    public WeatherController(IWeatherService weatherService)
    {
        _weatherService = weatherService;
    }

    [HttpGet("current")]
    public async Task<ActionResult<WeatherData>> GetCurrentWeather(
        [FromQuery] double lat,
        [FromQuery] double lng)
    {
        var location = new Location { Latitude = lat, Longitude = lng };
        var weather = await _weatherService.GetCurrentWeatherAsync(location);

        return Ok(weather);
    }

    [HttpGet("forecast")]
    public async Task<ActionResult<List<WeatherForecast>>> GetForecast(
        [FromQuery] double lat,
        [FromQuery] double lng,
        [FromQuery] int days = 7)
    {
        var location = new Location { Latitude = lat, Longitude = lng };
        var forecast = await _weatherService.GetForecastAsync(location, days);

        return Ok(forecast);
    }
}
```

**Android (Kotlin)**

```kotlin
class WeatherFragment : Fragment() {
    private var _binding: FragmentWeatherBinding? = null
    private val binding get() = _binding!!

    private val viewModel: WeatherViewModel by viewModels()

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View {
        _binding = FragmentWeatherBinding.inflate(inflater, container, false)
        return binding.root
    }

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)

        setupObservers()
        viewModel.loadCurrentLocation()
    }

    private fun setupObservers() {
        viewModel.weatherData.observe(viewLifecycleOwner) { weather ->
            updateWeatherUI(weather)
        }

        viewModel.forecast.observe(viewLifecycleOwner) { forecast ->
            updateForecastUI(forecast)
        }

        viewModel.isLoading.observe(viewLifecycleOwner) { isLoading ->
            binding.progressBar.isVisible = isLoading
        }
    }

    private fun updateWeatherUI(weather: WeatherData) {
        binding.temperatureText.text = "${weather.temperature.roundToInt()}°C"
        binding.conditionText.text = weather.condition
        binding.humidityText.text = "Humidity: ${weather.humidity}%"
        binding.windSpeedText.text = "Wind: ${weather.windSpeed} km/h"

        Glide.with(this)
            .load(weather.iconUrl)
            .into(binding.weatherIcon)
    }
}
```

**iOS (SwiftUI)**

```swift
struct WeatherView: View {
    @StateObject private var viewModel = WeatherViewModel()
    @State private var showingLocationPicker = false

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(spacing: 20) {
                    if let weather = viewModel.currentWeather {
                        CurrentWeatherCardView(weather: weather)
                    }

                    if !viewModel.forecast.isEmpty {
                        ForecastListView(forecast: viewModel.forecast)
                    }

                    if viewModel.isLoading {
                        ProgressView("Loading weather data...")
                    }
                }
                .padding()
            }
            .navigationTitle("Weather")
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Location") {
                        showingLocationPicker = true
                    }
                }
            }
            .sheet(isPresented: $showingLocationPicker) {
                LocationPickerView { location in
                    Task {
                        await viewModel.fetchWeather(for: location)
                    }
                }
            }
            .task {
                await viewModel.loadCurrentLocation()
            }
        }
    }
}

struct CurrentWeatherCardView: View {
    let weather: WeatherData

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                AsyncImage(url: URL(string: weather.iconUrl)) { image in
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                } placeholder: {
                    ProgressView()
                }
                .frame(width: 64, height: 64)

                VStack(alignment: .leading) {
                    Text("\(Int(weather.temperature))°C")
                        .font(.largeTitle)
                        .fontWeight(.bold)

                    Text(weather.condition)
                        .font(.headline)
                        .foregroundColor(.secondary)
                }

                Spacer()
            }

            HStack {
                WeatherStatView(
                    label: "Humidity",
                    value: "\(weather.humidity)%"
                )

                Spacer()

                WeatherStatView(
                    label: "Wind",
                    value: "\(Int(weather.windSpeed)) km/h"
                )

                Spacer()

                WeatherStatView(
                    label: "Pressure",
                    value: "\(Int(weather.pressure)) hPa"
                )
            }
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(radius: 4)
    }
}
```

### 2. Carbon Trading Platform

Implement a cross-platform carbon trading system:

#### Shared Trading Logic

```typescript
// trading-service.interface.ts
export interface TradingService {
  getMarketData(): Promise<MarketData>;
  getAvailableCredits(filters?: CreditFilters): Promise<CarbonCredit[]>;
  placeBuyOrder(order: BuyOrder): Promise<Transaction>;
  placeSellOrder(order: SellOrder): Promise<Transaction>;
  getOrderHistory(userId: string): Promise<Order[]>;
  getPortfolio(userId: string): Promise<Portfolio>;
}

export interface MarketData {
  totalVolume24h: number;
  avgPrice: number;
  priceChange24h: number;
  activeCredits: number;
  trending: CarbonCredit[];
}

export interface CreditFilters {
  projectType?: string[];
  vintage?: number[];
  standard?: string[];
  location?: string[];
  priceRange?: [number, number];
}
```

#### Multi-Platform Trading Interface

**Java (Desktop Swing Application)**

```java
public class TradingPanel extends JPanel {
    private final TradingService tradingService;
    private final DefaultTableModel creditTableModel;
    private final JTable creditTable;
    private final JLabel marketStatsLabel;

    public TradingPanel(TradingService tradingService) {
        this.tradingService = tradingService;
        this.creditTableModel = new DefaultTableModel(
            new String[]{"Name", "Type", "Price", "CO₂", "Vintage", "Action"}, 0
        );
        this.creditTable = new JTable(creditTableModel);

        initializeUI();
        loadMarketData();
    }

    private void initializeUI() {
        setLayout(new BorderLayout());

        // Market stats panel
        JPanel statsPanel = createMarketStatsPanel();
        add(statsPanel, BorderLayout.NORTH);

        // Credits table
        JScrollPane tableScrollPane = new JScrollPane(creditTable);
        add(tableScrollPane, BorderLayout.CENTER);

        // Trading controls
        JPanel controlsPanel = createTradingControlsPanel();
        add(controlsPanel, BorderLayout.SOUTH);

        // Configure table
        creditTable.setSelectionMode(ListSelectionModel.SINGLE_SELECTION);
        creditTable.getColumn("Action").setCellRenderer(new ButtonRenderer());
        creditTable.getColumn("Action").setCellEditor(new ButtonEditor());
    }

    private void loadMarketData() {
        SwingUtilities.invokeLater(() -> {
            try {
                MarketData marketData = tradingService.getMarketData().join();
                updateMarketStats(marketData);

                List<CarbonCredit> credits = tradingService.getAvailableCredits().join();
                updateCreditsTable(credits);

            } catch (Exception e) {
                JOptionPane.showMessageDialog(this,
                    "Failed to load market data: " + e.getMessage(),
                    "Error", JOptionPane.ERROR_MESSAGE);
            }
        });
    }

    private void purchaseCredit(CarbonCredit credit) {
        String quantityStr = JOptionPane.showInputDialog(
            this,
            "Enter quantity to purchase:",
            "Purchase " + credit.getName(),
            JOptionPane.QUESTION_MESSAGE
        );

        if (quantityStr != null && !quantityStr.trim().isEmpty()) {
            try {
                double quantity = Double.parseDouble(quantityStr);
                BuyOrder order = new BuyOrder(credit.getId(), quantity, credit.getPrice());

                tradingService.placeBuyOrder(order)
                    .thenAccept(transaction -> {
                        SwingUtilities.invokeLater(() -> {
                            JOptionPane.showMessageDialog(this,
                                "Purchase successful! Transaction ID: " + transaction.getId(),
                                "Success", JOptionPane.INFORMATION_MESSAGE);
                            loadMarketData(); // Refresh data
                        });
                    })
                    .exceptionally(throwable -> {
                        SwingUtilities.invokeLater(() -> {
                            JOptionPane.showMessageDialog(this,
                                "Purchase failed: " + throwable.getMessage(),
                                "Error", JOptionPane.ERROR_MESSAGE);
                        });
                        return null;
                    });

            } catch (NumberFormatException e) {
                JOptionPane.showMessageDialog(this,
                    "Please enter a valid number",
                    "Invalid Input", JOptionPane.WARNING_MESSAGE);
            }
        }
    }
}
```

**C# (WPF Desktop Application)**

```csharp
public partial class TradingWindow : Window
{
    private readonly ITradingService _tradingService;
    private readonly ObservableCollection<CarbonCredit> _credits;

    public TradingWindow(ITradingService tradingService)
    {
        InitializeComponent();
        _tradingService = tradingService;
        _credits = new ObservableCollection<CarbonCredit>();

        CreditsDataGrid.ItemsSource = _credits;

        LoadMarketDataAsync();
    }

    private async void LoadMarketDataAsync()
    {
        try {
            LoadingProgressBar.Visibility = Visibility.Visible;

            var marketData = await _tradingService.GetMarketDataAsync();
            UpdateMarketStats(marketData);

            var credits = await _tradingService.GetAvailableCreditsAsync();

            _credits.Clear();
            foreach (var credit in credits)
            {
                _credits.Add(credit);
            }
        }
        catch (Exception ex)
        {
            MessageBox.Show($"Failed to load market data: {ex.Message}",
                          "Error", MessageBoxButton.OK, MessageBoxImage.Error);
        }
        finally
        {
            LoadingProgressBar.Visibility = Visibility.Collapsed;
        }
    }

    private void UpdateMarketStats(MarketData marketData)
    {
        TotalVolumeLabel.Content = $"${marketData.TotalVolume24h:N0}";
        AvgPriceLabel.Content = $"${marketData.AvgPrice:F2}";
        PriceChangeLabel.Content = $"{marketData.PriceChange24h:+0.00;-0.00}%";

        PriceChangeLabel.Foreground = marketData.PriceChange24h >= 0
            ? Brushes.Green
            : Brushes.Red;
    }

    private async void PurchaseButton_Click(object sender, RoutedEventArgs e)
    {
        if (CreditsDataGrid.SelectedItem is CarbonCredit selectedCredit)
        {
            var dialog = new PurchaseDialog(selectedCredit);
            if (dialog.ShowDialog() == true)
            {
                try
                {
                    var order = new BuyOrder
                    {
                        CreditId = selectedCredit.Id,
                        Quantity = dialog.Quantity,
                        Price = selectedCredit.Price
                    };

                    var transaction = await _tradingService.PlaceBuyOrderAsync(order);

                    MessageBox.Show($"Purchase successful! Transaction ID: {transaction.Id}",
                                  "Success", MessageBoxButton.OK, MessageBoxImage.Information);

                    await LoadMarketDataAsync(); // Refresh data
                }
                catch (Exception ex)
                {
                    MessageBox.Show($"Purchase failed: {ex.Message}",
                                  "Error", MessageBoxButton.OK, MessageBoxImage.Error);
                }
            }
        }
    }
}
```

**Android (Kotlin)**

```kotlin
class TradingActivity : AppCompatActivity() {
    private lateinit var binding: ActivityTradingBinding
    private lateinit var tradingService: TradingService
    private lateinit var creditsAdapter: CarbonCreditsAdapter

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityTradingBinding.inflate(layoutInflater)
        setContentView(binding.root)

        tradingService = WRFCoinSDK.getInstance().tradingService
        setupRecyclerView()
        setupRefreshLayout()
        loadMarketData()
    }

    private fun setupRecyclerView() {
        creditsAdapter = CarbonCreditsAdapter { credit ->
            showPurchaseDialog(credit)
        }

        binding.creditsRecyclerView.apply {
            adapter = creditsAdapter
            layoutManager = LinearLayoutManager(this@TradingActivity)
            addItemDecoration(DividerItemDecoration(context, DividerItemDecoration.VERTICAL))
        }
    }

    private fun setupRefreshLayout() {
        binding.swipeRefreshLayout.setOnRefreshListener {
            loadMarketData()
        }
    }

    private fun loadMarketData() {
        lifecycleScope.launch {
            try {
                binding.swipeRefreshLayout.isRefreshing = true

                val marketData = tradingService.getMarketData()
                updateMarketStats(marketData)

                val credits = tradingService.getAvailableCredits()
                creditsAdapter.submitList(credits)

            } catch (e: Exception) {
                Toast.makeText(this@TradingActivity,
                    "Failed to load market data: ${e.message}",
                    Toast.LENGTH_LONG).show()
            } finally {
                binding.swipeRefreshLayout.isRefreshing = false
            }
        }
    }

    private fun updateMarketStats(marketData: MarketData) {
        binding.apply {
            totalVolumeText.text = "$${marketData.totalVolume24h.toLong()}"
            avgPriceText.text = "$%.2f".format(marketData.avgPrice)
            priceChangeText.text = "%+.2f%%".format(marketData.priceChange24h)

            priceChangeText.setTextColor(
                ContextCompat.getColor(
                    this@TradingActivity,
                    if (marketData.priceChange24h >= 0) R.color.green else R.color.red
                )
            )
        }
    }

    private fun showPurchaseDialog(credit: CarbonCredit) {
        val dialogBinding = DialogPurchaseCreditBinding.inflate(layoutInflater)

        dialogBinding.apply {
            creditNameText.text = credit.name
            creditPriceText.text = "$%.2f per ton".format(credit.price)
            quantityInput.setText("1.0")
        }

        AlertDialog.Builder(this)
            .setTitle("Purchase Carbon Credit")
            .setView(dialogBinding.root)
            .setPositiveButton("Purchase") { _, _ ->
                val quantity = dialogBinding.quantityInput.text.toString().toDoubleOrNull()
                if (quantity != null && quantity > 0) {
                    purchaseCredit(credit, quantity)
                } else {
                    Toast.makeText(this, "Please enter a valid quantity", Toast.LENGTH_SHORT).show()
                }
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun purchaseCredit(credit: CarbonCredit, quantity: Double) {
        lifecycleScope.launch {
            try {
                val order = BuyOrder(credit.id, quantity, credit.price)
                val transaction = tradingService.placeBuyOrder(order)

                Toast.makeText(this@TradingActivity,
                    "Purchase successful! TX: ${transaction.id}",
                    Toast.LENGTH_LONG).show()

                loadMarketData() // Refresh data

            } catch (e: Exception) {
                Toast.makeText(this@TradingActivity,
                    "Purchase failed: ${e.message}",
                    Toast.LENGTH_LONG).show()
            }
        }
    }
}
```

**iOS (SwiftUI)**

```swift
struct TradingView: View {
    @StateObject private var viewModel = TradingViewModel()
    @State private var showingPurchaseSheet = false
    @State private var selectedCredit: CarbonCredit?

    var body: some View {
        NavigationView {
            VStack(spacing: 0) {
                // Market Stats Header
                MarketStatsView(marketData: viewModel.marketData)
                    .padding()
                    .background(Color(.systemGray6))

                // Credits List
                List(viewModel.availableCredits) { credit in
                    CreditRowView(credit: credit) {
                        selectedCredit = credit
                        showingPurchaseSheet = true
                    }
                }
                .refreshable {
                    await viewModel.loadMarketData()
                }
            }
            .navigationTitle("Carbon Trading")
            .sheet(isPresented: $showingPurchaseSheet) {
                if let credit = selectedCredit {
                    PurchaseCreditView(credit: credit) { quantity in
                        Task {
                            await viewModel.purchaseCredit(credit, quantity: quantity)
                        }
                    }
                }
            }
            .task {
                await viewModel.loadMarketData()
            }
            .alert("Error", isPresented: .constant(viewModel.error != nil)) {
                Button("OK") {
                    viewModel.error = nil
                }
            } message: {
                Text(viewModel.error ?? "")
            }
        }
    }
}

struct MarketStatsView: View {
    let marketData: MarketData?

    var body: some View {
        HStack(spacing: 20) {
            if let data = marketData {
                StatView(
                    title: "24h Volume",
                    value: "$\(data.totalVolume24h, specifier: "%.0f")"
                )

                StatView(
                    title: "Avg Price",
                    value: "$\(data.avgPrice, specifier: "%.2f")"
                )

                StatView(
                    title: "24h Change",
                    value: "\(data.priceChange24h, specifier: "%+.2f")%",
                    color: data.priceChange24h >= 0 ? .green : .red
                )
            } else {
                ProgressView("Loading market data...")
            }
        }
    }
}

struct StatView: View {
    let title: String
    let value: String
    let color: Color

    init(title: String, value: String, color: Color = .primary) {
        self.title = title
        self.value = value
        self.color = color
    }

    var body: some View {
        VStack(spacing: 4) {
            Text(title)
                .font(.caption)
                .foregroundColor(.secondary)

            Text(value)
                .font(.headline)
                .fontWeight(.semibold)
                .foregroundColor(color)
        }
    }
}

struct CreditRowView: View {
    let credit: CarbonCredit
    let onPurchase: () -> Void

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text(credit.name)
                    .font(.headline)
                    .lineLimit(1)

                Text(credit.projectType)
                    .font(.caption)
                    .foregroundColor(.secondary)

                HStack {
                    Label("\(credit.co2Amount, specifier: "%.1f") tons CO₂",
                          systemImage: "leaf.fill")
                        .font(.caption)
                        .foregroundColor(.green)

                    Spacer()

                    Text("Vintage \(credit.vintage)")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
            }

            Spacer()

            VStack(alignment: .trailing, spacing: 4) {
                Text("$\(credit.price, specifier: "%.2f")")
                    .font(.title3)
                    .fontWeight(.bold)

                Button("Purchase", action: onPurchase)
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
            }
        }
        .padding(.vertical, 4)
    }
}
```

## Configuration Management

### Environment-Specific Configuration

Create environment-specific configurations that work across all platforms:

#### Configuration Schema

```json
{
  "development": {
    "apiBaseUrl": "https://dev-api.wrfcoin.com",
    "wsBaseUrl": "wss://dev-ws.wrfcoin.com",
    "apiKey": "dev-api-key",
    "enableLogging": true,
    "enableOfflineSupport": false,
    "cacheSize": 10485760
  },
  "staging": {
    "apiBaseUrl": "https://staging-api.wrfcoin.com",
    "wsBaseUrl": "wss://staging-ws.wrfcoin.com",
    "apiKey": "staging-api-key",
    "enableLogging": true,
    "enableOfflineSupport": true,
    "cacheSize": 52428800
  },
  "production": {
    "apiBaseUrl": "https://api.wrfcoin.com",
    "wsBaseUrl": "wss://ws.wrfcoin.com",
    "apiKey": "production-api-key",
    "enableLogging": false,
    "enableOfflineSupport": true,
    "cacheSize": 104857600
  }
}
```

#### Platform-Specific Configuration Loaders

**Java Configuration**

```java
@Configuration
@ConfigurationProperties(prefix = "wrfcoin")
public class WRFCoinConfig {
    private String apiBaseUrl;
    private String wsBaseUrl;
    private String apiKey;
    private boolean enableLogging;
    private boolean enableOfflineSupport;
    private long cacheSize;

    // Getters and setters...

    @Bean
    public WRFCoinSDK wrfCoinSDK() {
        return new WRFCoinSDK.Builder()
            .apiBaseUrl(apiBaseUrl)
            .wsBaseUrl(wsBaseUrl)
            .apiKey(apiKey)
            .enableLogging(enableLogging)
            .cacheSize(cacheSize)
            .build();
    }
}
```

**C# Configuration**

```csharp
public class WRFCoinConfig
{
    public string ApiBaseUrl { get; set; }
    public string WsBaseUrl { get; set; }
    public string ApiKey { get; set; }
    public bool EnableLogging { get; set; }
    public bool EnableOfflineSupport { get; set; }
    public long CacheSize { get; set; }
}

// In Startup.cs or Program.cs
services.Configure<WRFCoinConfig>(configuration.GetSection("WRFCoin"));
services.AddSingleton<IWRFCoinSDK>(provider =>
{
    var config = provider.GetRequiredService<IOptions<WRFCoinConfig>>().Value;
    return new WRFCoinSDK.Builder()
        .WithApiBaseUrl(config.ApiBaseUrl)
        .WithWsBaseUrl(config.WsBaseUrl)
        .WithApiKey(config.ApiKey)
        .WithLogging(config.EnableLogging)
        .WithCacheSize(config.CacheSize)
        .Build();
});
```

**Android Configuration**

```kotlin
// build.gradle.kts
android {
    buildTypes {
        debug {
            buildConfigField("String", "API_BASE_URL", "\"https://dev-api.wrfcoin.com\"")
            buildConfigField("String", "WS_BASE_URL", "\"wss://dev-ws.wrfcoin.com\"")
            buildConfigField("String", "API_KEY", "\"dev-api-key\"")
            buildConfigField("boolean", "ENABLE_LOGGING", "true")
        }

        release {
            buildConfigField("String", "API_BASE_URL", "\"https://api.wrfcoin.com\"")
            buildConfigField("String", "WS_BASE_URL", "\"wss://ws.wrfcoin.com\"")
            buildConfigField("String", "API_KEY", "\"production-api-key\"")
            buildConfigField("boolean", "ENABLE_LOGGING", "false")
        }
    }
}

// Application class
class WRFCoinApplication : Application() {
    override fun onCreate() {
        super.onCreate()

        val config = WRFCoinConfig.Builder()
            .apiBaseUrl(BuildConfig.API_BASE_URL)
            .wsBaseUrl(BuildConfig.WS_BASE_URL)
            .apiKey(BuildConfig.API_KEY)
            .enableLogging(BuildConfig.ENABLE_LOGGING)
            .enableOfflineSupport(true)
            .cacheSize(50 * 1024 * 1024)
            .build()

        WRFCoinSDK.initialize(this, config)
    }
}
```

**iOS Configuration**

```swift
// Config.xcconfig files for different schemes
// Debug.xcconfig
API_BASE_URL = https://dev-api.wrfcoin.com
WS_BASE_URL = wss://dev-ws.wrfcoin.com
API_KEY = dev-api-key
ENABLE_LOGGING = true

// Release.xcconfig
API_BASE_URL = https://api.wrfcoin.com
WS_BASE_URL = wss://ws.wrfcoin.com
API_KEY = production-api-key
ENABLE_LOGGING = false

// ConfigurationManager.swift
class ConfigurationManager {
    static let shared = ConfigurationManager()

    private init() {}

    var apiBaseURL: String {
        return Bundle.main.object(forInfoDictionaryKey: "API_BASE_URL") as? String ?? ""
    }

    var wsBaseURL: String {
        return Bundle.main.object(forInfoDictionaryKey: "WS_BASE_URL") as? String ?? ""
    }

    var apiKey: String {
        return Bundle.main.object(forInfoDictionaryKey: "API_KEY") as? String ?? ""
    }

    var enableLogging: Bool {
        return (Bundle.main.object(forInfoDictionaryKey: "ENABLE_LOGGING") as? String)?.lowercased() == "true"
    }
}

// App initialization
@main
struct WRFCoinApp: App {
    init() {
        let config = ConfigurationManager.shared

        let sdkConfig = WRFCoinConfiguration(
            apiBaseURL: config.apiBaseURL,
            wsBaseURL: config.wsBaseURL,
            apiKey: config.apiKey,
            enableLogging: config.enableLogging,
            enableOfflineSupport: true,
            cacheSize: 50 * 1024 * 1024
        )

        WRFCoinSDK.configure(with: sdkConfig)
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
```

## Testing Strategies

### Cross-Platform Test Suite

Create comprehensive tests that validate functionality across all platforms:

#### Shared Test Cases

```typescript
// test-cases.ts - Shared test specifications
export interface TestCase {
  name: string;
  description: string;
  setup: () => Promise<void>;
  execute: () => Promise<TestResult>;
  teardown: () => Promise<void>;
}

export interface TestResult {
  passed: boolean;
  duration: number;
  error?: string;
  data?: any;
}

export const weatherServiceTests: TestCase[] = [
  {
    name: 'getCurrentWeather_ValidLocation_ReturnsWeatherData',
    description: 'Should return weather data for valid coordinates',
    setup: async () => {
      // Setup test environment
    },
    execute: async () => {
      const start = Date.now();
      try {
        const weather = await weatherService.getCurrentWeather({
          latitude: 40.7128,
          longitude: -74.006,
        });

        const passed =
          weather !== null && weather.temperature !== undefined && weather.condition !== undefined;

        return {
          passed,
          duration: Date.now() - start,
          data: weather,
        };
      } catch (error) {
        return {
          passed: false,
          duration: Date.now() - start,
          error: error.message,
        };
      }
    },
    teardown: async () => {
      // Cleanup
    },
  },
  // More test cases...
];
```

#### Platform-Specific Test Runners

**Java (JUnit)**

```java
@RunWith(Parameterized.class)
public class WeatherServiceCrossPlatformTest {

    @Parameterized.Parameters(name = "{0}")
    public static Collection<Object[]> testCases() {
        return TestCaseLoader.loadWeatherServiceTests();
    }

    private final TestCase testCase;
    private WeatherService weatherService;

    public WeatherServiceCrossPlatformTest(TestCase testCase) {
        this.testCase = testCase;
    }

    @Before
    public void setUp() throws Exception {
        weatherService = new WRFCoinSDK.Builder()
            .apiKey("test-api-key")
            .environment(Environment.SANDBOX)
            .build()
            .getWeatherService();

        testCase.setup();
    }

    @Test
    public void executeTestCase() throws Exception {
        TestResult result = testCase.execute();

        assertTrue(testCase.getName() + " failed: " + result.getError(), result.isPassed());

        // Log performance metrics
        System.out.println(String.format("%s completed in %d ms",
            testCase.getName(), result.getDuration()));
    }

    @After
    public void tearDown() throws Exception {
        testCase.teardown();
    }
}
```

**C# (xUnit)**

```csharp
public class WeatherServiceCrossPlatformTests : IClassFixture<TestFixture>
{
    private readonly IWeatherService _weatherService;
    private readonly ITestOutputHelper _output;

    public WeatherServiceCrossPlatformTests(TestFixture fixture, ITestOutputHelper output)
    {
        _weatherService = fixture.WeatherService;
        _output = output;
    }

    [Theory]
    [MemberData(nameof(GetTestCases))]
    public async Task ExecuteTestCase(TestCase testCase)
    {
        // Setup
        await testCase.SetupAsync();

        try
        {
            // Execute
            var result = await testCase.ExecuteAsync();

            // Assert
            Assert.True(result.Passed, $"{testCase.Name} failed: {result.Error}");

            // Log performance
            _output.WriteLine($"{testCase.Name} completed in {result.Duration} ms");
        }
        finally
        {
            // Teardown
            await testCase.TeardownAsync();
        }
    }

    public static IEnumerable<object[]> GetTestCases()
    {
        return TestCaseLoader.LoadWeatherServiceTests()
            .Select(testCase => new object[] { testCase });
    }
}
```

**Android (Espresso + JUnit)**

```kotlin
@RunWith(AndroidJUnit4::class)
@LargeTest
class WeatherServiceCrossPlatformTest {

    @get:Rule
    val activityRule = ActivityScenarioRule(MainActivity::class.java)

    private lateinit var weatherService: WeatherService

    @Before
    fun setup() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val config = WRFCoinConfig.Builder()
            .apiKey("test-api-key")
            .environment(WRFCoinEnvironment.SANDBOX)
            .build()

        WRFCoinSDK.initialize(context, config)
        weatherService = WRFCoinSDK.getInstance().weatherService
    }

    @Test
    fun executeAllTestCases() = runTest {
        val testCases = TestCaseLoader.loadWeatherServiceTests()

        for (testCase in testCases) {
            try {
                testCase.setup()
                val result = testCase.execute()

                assertTrue("${testCase.name} failed: ${result.error}", result.passed)

                Log.d("CrossPlatformTest",
                    "${testCase.name} completed in ${result.duration} ms")

            } finally {
                testCase.teardown()
            }
        }
    }
}
```

**iOS (XCTest)**

```swift
class WeatherServiceCrossPlatformTests: XCTestCase {

    private var weatherService: WeatherService!

    override func setUp() {
        super.setUp()

        let config = WRFCoinConfiguration(
            apiKey: "test-api-key",
            environment: .sandbox
        )

        WRFCoinSDK.configure(with: config)
        weatherService = WRFCoinSDK.shared.weatherService
    }

    func testAllTestCases() async throws {
        let testCases = TestCaseLoader.loadWeatherServiceTests()

        for testCase in testCases {
            do {
                await testCase.setup()
                let result = await testCase.execute()

                XCTAssertTrue(result.passed, "\(testCase.name) failed: \(result.error ?? "Unknown error")")

                print("\(testCase.name) completed in \(result.duration) ms")

            } catch {
                XCTFail("\(testCase.name) threw error: \(error)")
            }

            await testCase.teardown()
        }
    }
}
```

## Best Practices

### 1. API Design Consistency

- Use consistent naming conventions across all platforms
- Maintain the same method signatures and return types
- Implement similar error handling patterns
- Provide uniform configuration options

### 2. Performance Optimization

- Implement platform-specific caching strategies
- Use appropriate concurrency patterns for each platform
- Optimize network requests for mobile networks
- Implement progressive loading for large datasets

### 3. Error Handling

- Define common error types and codes
- Provide meaningful error messages
- Implement retry mechanisms with exponential backoff
- Log errors consistently across platforms

### 4. Security

- Use platform-specific secure storage (Keychain, Android Keystore, etc.)
- Implement certificate pinning for network requests
- Validate all user inputs
- Use secure random number generators

### 5. Testing

- Maintain comprehensive test suites for all platforms
- Use platform-specific testing frameworks
- Implement integration tests for cross-platform scenarios
- Monitor performance metrics across platforms

This cross-platform development guide provides the foundation for building consistent, maintainable
applications across the entire WRFCoin SDK ecosystem while leveraging platform-specific capabilities
when appropriate.
