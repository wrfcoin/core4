# WRFCoin iOS SDK

The WRFCoin iOS SDK provides native iOS integration for the WRFCoin weather forecasting blockchain
platform. Built with modern iOS development practices using Swift and SwiftUI, it offers seamless
access to weather data, carbon credits, DeFi operations, and IoT sensor management.

## Features

- **🌡️ Weather Data Access** - Real-time and historical weather data from 20+ sensor platforms
- **💚 Carbon Credit Trading** - Trade verified carbon credits with blockchain transparency
- **💰 DeFi Integration** - Access AMM, yield farming, and governance features
- **📱 Native iOS UI** - SwiftUI components following Human Interface Guidelines
- **🔐 Secure Authentication** - Face ID, Touch ID, and Keychain integration
- **⚡ High Performance** - Async/await with Combine publishers for reactive programming
- **🌐 Offline Support** - Core Data caching and background sync
- **📊 Real-time Updates** - WebSocket connections for live data streams
- **♿ Accessibility** - Full VoiceOver and accessibility support

## Requirements

- iOS 15.0+
- Xcode 14.0+
- Swift 5.7+

## Quick Start

### Installation

#### Swift Package Manager (Recommended)

Add the WRFCoin SDK to your project using Xcode:

1. In Xcode, go to **File → Add Package Dependencies**
2. Enter the repository URL: `https://github.com/wrfcoin/ios-sdk`
3. Select the version range and add to your target

Or add to your `Package.swift`:

```swift
dependencies: [
    .package(url: "https://github.com/wrfcoin/ios-sdk", from: "1.0.0")
]
```

#### CocoaPods

Add to your `Podfile`:

```ruby
pod 'WRFCoinSDK', '~> 1.0'
```

Then run:

```bash
pod install
```

### Configure App

Add required permissions to your `Info.plist`:

```xml
<key>NSLocationWhenInUseUsageDescription</key>
<string>WRFCoin needs location access to provide accurate weather data</string>
<key>NSLocationAlwaysAndWhenInUseUsageDescription</key>
<string>WRFCoin needs location access to provide accurate weather data</string>
<key>NSFaceIDUsageDescription</key>
<string>Use Face ID to securely access your WRFCoin account</string>
```

### Initialize SDK

Configure the SDK in your `App.swift`:

```swift
import SwiftUI
import WRFCoinSDK

@main
struct WRFCoinApp: App {
    init() {
        configureWRFCoinSDK()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .onAppear {
                    Task {
                        await WRFCoinSDK.shared.initializeServices()
                    }
                }
        }
    }

    private func configureWRFCoinSDK() {
        let config = WRFCoinConfiguration(
            apiKey: "your-api-key",
            environment: .production, // or .sandbox
            enableLogging: true,
            enableOfflineSupport: true,
            enableBiometricAuth: true,
            cacheSize: 50 * 1024 * 1024 // 50MB
        )

        WRFCoinSDK.configure(with: config)
    }
}
```

### Basic Usage

```swift
import SwiftUI
import WRFCoinSDK

struct ContentView: View {
    @StateObject private var weatherViewModel = WeatherViewModel()

    var body: some View {
        NavigationView {
            VStack {
                if let weather = weatherViewModel.currentWeather {
                    WeatherCardView(weather: weather)
                } else if weatherViewModel.isLoading {
                    ProgressView("Loading weather data...")
                } else if let error = weatherViewModel.error {
                    ErrorView(error: error) {
                        Task {
                            await weatherViewModel.fetchCurrentLocation()
                        }
                    }
                }
            }
            .navigationTitle("Weather")
            .task {
                await weatherViewModel.authenticateAndFetchWeather()
            }
        }
    }
}

@MainActor
class WeatherViewModel: ObservableObject {
    @Published var currentWeather: WeatherData?
    @Published var isLoading = false
    @Published var error: String?

    private let sdk = WRFCoinSDK.shared

    func authenticateAndFetchWeather() async {
        isLoading = true

        do {
            // Authenticate user
            let authResult = try await sdk.authenticate(
                email: "user@example.com",
                password: "password"
            )

            print("Authentication successful: \(authResult.token)")

            // Fetch weather for current location
            await fetchCurrentLocation()

        } catch {
            self.error = "Authentication failed: \(error.localizedDescription)"
            isLoading = false
        }
    }

    func fetchCurrentLocation() async {
        do {
            let location = try await sdk.locationService.getCurrentLocation()
            let weather = try await sdk.weatherService.getCurrentWeather(
                latitude: location.coordinate.latitude,
                longitude: location.coordinate.longitude
            )

            self.currentWeather = weather
            self.error = nil

        } catch {
            self.error = "Failed to fetch weather: \(error.localizedDescription)"
        }

        isLoading = false
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
- [SwiftUI Components](swiftui-components.md)
- [Real-time Features](realtime-features.md)
- [Offline Support](offline-support.md)
- [Security](security.md)
- [Testing](testing.md)
- [Migration Guide](migration.md)
- [API Reference](api-reference.md)
- [Examples](examples/)

## Architecture Overview

The WRFCoin iOS SDK follows clean architecture principles with clear separation of concerns:

```
┌─────────────────────────────────────┐
│           Presentation Layer        │
│      (SwiftUI Views, ViewModels)    │
├─────────────────────────────────────┤
│            Domain Layer             │
│      (Use Cases, Repositories)      │
├─────────────────────────────────────┤
│             Data Layer              │
│   (Network, Core Data, Keychain)    │
└─────────────────────────────────────┘
```

### Key Components

- **WRFCoinSDK** - Main SDK singleton instance
- **ServiceManager** - Manages all API services and dependencies
- **AuthenticationService** - Handles user authentication and biometrics
- **CacheManager** - Core Data stack for offline storage
- **NetworkManager** - URLSession-based HTTP client with retry logic
- **SecurityManager** - Keychain integration and biometric authentication
- **RealtimeManager** - WebSocket connections using URLSessionWebSocketTask

## Core Services

### Weather Service

Access comprehensive weather data from multiple sources:

```swift
let weatherService = WRFCoinSDK.shared.weatherService

// Current weather
let current = try await weatherService.getCurrentWeather(
    latitude: 40.7128,
    longitude: -74.0060
)

// 7-day forecast
let forecast = try await weatherService.getForecast(
    latitude: 40.7128,
    longitude: -74.0060,
    days: 7
)

// Historical data
let historical = try await weatherService.getHistoricalWeather(
    latitude: 40.7128,
    longitude: -74.0060,
    startDate: Calendar.current.date(byAdding: .day, value: -30, to: Date())!,
    endDate: Date()
)

// Subscribe to real-time updates
for await weather in weatherService.weatherUpdates(
    latitude: 40.7128,
    longitude: -74.0060,
    interval: .minutes(5)
) {
    print("Updated weather: \(weather.temperature)°C")
}
```

### Carbon Credit Service

Trade and manage carbon credits:

```swift
let carbonService = WRFCoinSDK.shared.carbonService

// Browse available credits
let credits = try await carbonService.getAvailableCredits()

// Purchase credits
let purchase = try await carbonService.purchaseCredits(
    creditId: "carbon-123",
    quantity: 10.0,
    price: 25.50
)

// Calculate carbon footprint
let activities: [CarbonActivity] = [
    .flight(distance: 1000.0),
    .carTravel(distance: 500.0, fuelType: .gasoline)
]

let footprint = try await carbonService.calculateFootprint(activities: activities)
print("Your carbon footprint: \(footprint.totalCO2) kg CO2")
```

### DeFi Service

Access decentralized finance features:

```swift
let defiService = WRFCoinSDK.shared.defiService

// Get available liquidity pools
let pools = try await defiService.getLiquidityPools()

// Add liquidity to a pool
let result = try await defiService.addLiquidity(
    poolId: "WRF-USDC",
    tokenAAmount: 1000.0,
    tokenBAmount: 500.0
)

// Stake tokens for rewards
let staking = try await defiService.stakeTokens(
    amount: 5000.0,
    duration: .months(6)
)

// Get staking rewards
let rewards = try await defiService.getStakingRewards()
print("Pending rewards: \(rewards.pendingAmount) WRF")
```

## SwiftUI Components

The SDK includes pre-built SwiftUI components following iOS design guidelines:

### Weather Views

```swift
import SwiftUI
import WRFCoinSDK

// Current Weather Card
struct WeatherCardView: View {
    let weather: WeatherData

    var body: some View {
        VStack(spacing: 16) {
            HStack {
                VStack(alignment: .leading) {
                    Text(weather.location.name)
                        .font(.title2)
                        .fontWeight(.semibold)

                    Text(weather.location.country)
                        .font(.caption)
                        .foregroundColor(.secondary)
                }

                Spacer()

                Text("\(Int(weather.temperature))°")
                    .font(.system(size: 48, weight: .thin))
            }

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
                    Text(weather.condition)
                        .font(.headline)

                    Text("Feels like \(Int(weather.feelsLike))°")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }

                Spacer()
            }

            // Weather stats
            HStack(spacing: 32) {
                WeatherStatView(
                    icon: "humidity",
                    label: "Humidity",
                    value: "\(weather.humidity)%"
                )

                WeatherStatView(
                    icon: "wind",
                    label: "Wind",
                    value: "\(Int(weather.windSpeed)) km/h"
                )

                WeatherStatView(
                    icon: "eye",
                    label: "Visibility",
                    value: "\(Int(weather.visibility)) km"
                )
            }
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(radius: 2)
    }
}

struct WeatherStatView: View {
    let icon: String
    let label: String
    let value: String

    var body: some View {
        VStack(spacing: 4) {
            Image(systemName: icon)
                .foregroundColor(.blue)
                .font(.title2)

            Text(value)
                .font(.caption)
                .fontWeight(.semibold)

            Text(label)
                .font(.caption2)
                .foregroundColor(.secondary)
        }
    }
}
```

### Carbon Trading Views

```swift
struct CarbonTradingView: View {
    @StateObject private var viewModel = CarbonTradingViewModel()

    var body: some View {
        NavigationView {
            List {
                Section("Your Carbon Footprint") {
                    CarbonFootprintCardView(footprint: viewModel.footprint)
                }

                Section("Available Credits") {
                    ForEach(viewModel.availableCredits) { credit in
                        CarbonCreditRowView(credit: credit) {
                            Task {
                                await viewModel.purchaseCredit(credit)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Carbon Credits")
            .refreshable {
                await viewModel.refresh()
            }
            .task {
                await viewModel.loadData()
            }
        }
    }
}

struct CarbonCreditRowView: View {
    let credit: CarbonCredit
    let onPurchase: () -> Void

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text(credit.name)
                    .font(.headline)

                Text(credit.description)
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .lineLimit(2)

                HStack {
                    Label("\(credit.co2Amount, specifier: "%.1f") kg CO2", systemImage: "leaf")
                        .font(.caption)
                        .foregroundColor(.green)

                    Spacer()

                    Text("$\(credit.price, specifier: "%.2f")")
                        .font(.callout)
                        .fontWeight(.semibold)
                }
            }

            Spacer()

            Button("Purchase", action: onPurchase)
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
        }
        .padding(.vertical, 4)
    }
}
```

### DeFi Dashboard

```swift
struct DeFiDashboardView: View {
    @StateObject private var viewModel = DeFiViewModel()

    var body: some View {
        ScrollView {
            LazyVStack(spacing: 16) {
                // Wallet Balance
                WalletBalanceCardView(balance: viewModel.walletBalance)

                // Liquidity Pools
                LiquidityPoolsCardView(pools: viewModel.liquidityPools) { pool in
                    Task {
                        await viewModel.selectPool(pool)
                    }
                }

                // Staking Rewards
                StakingRewardsCardView(rewards: viewModel.stakingRewards) {
                    Task {
                        await viewModel.claimRewards()
                    }
                }

                // Recent Transactions
                TransactionHistoryCardView(transactions: viewModel.recentTransactions)
            }
            .padding()
        }
        .navigationTitle("DeFi Dashboard")
        .refreshable {
            await viewModel.refresh()
        }
        .task {
            await viewModel.loadData()
        }
    }
}

struct WalletBalanceCardView: View {
    let balance: WalletBalance

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Text("Wallet Balance")
                    .font(.headline)
                Spacer()
                Menu("Actions") {
                    Button("Send", action: {})
                    Button("Receive", action: {})
                    Button("Swap", action: {})
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
            }

            HStack {
                VStack(alignment: .leading) {
                    Text("\(balance.wrfAmount, specifier: "%.2f") WRF")
                        .font(.title)
                        .fontWeight(.bold)

                    Text("$\(balance.usdValue, specifier: "%.2f") USD")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }

                Spacer()

                VStack(alignment: .trailing) {
                    HStack {
                        Image(systemName: balance.change24h >= 0 ? "arrow.up.right" : "arrow.down.right")
                            .foregroundColor(balance.change24h >= 0 ? .green : .red)

                        Text("\(abs(balance.change24h), specifier: "%.2f")%")
                            .foregroundColor(balance.change24h >= 0 ? .green : .red)
                    }
                    .font(.caption)

                    Text("24h change")
                        .font(.caption2)
                        .foregroundColor(.secondary)
                }
            }
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(radius: 2)
    }
}
```

## Real-time Features

### Live Weather Updates

```swift
@MainActor
class WeatherViewModel: ObservableObject {
    @Published var currentWeather: WeatherData?
    @Published var isLoading = false

    private var weatherTask: Task<Void, Never>?

    func startLiveUpdates(latitude: Double, longitude: Double) {
        weatherTask = Task {
            do {
                for try await weather in WRFCoinSDK.shared.weatherService.weatherUpdates(
                    latitude: latitude,
                    longitude: longitude,
                    interval: .minutes(5)
                ) {
                    self.currentWeather = weather
                }
            } catch {
                print("Weather updates error: \(error)")
            }
        }
    }

    func stopLiveUpdates() {
        weatherTask?.cancel()
        weatherTask = nil
    }

    deinit {
        stopLiveUpdates()
    }
}
```

### Price Notifications

```swift
class PriceAlertManager: ObservableObject {
    @Published var alerts: [PriceAlert] = []

    private let carbonService = WRFCoinSDK.shared.carbonService
    private var alertTasks: [Task<Void, Never>] = []

    func setupPriceAlert(for creditType: CreditType, threshold: Double, direction: AlertDirection) {
        let task = Task {
            do {
                for try await alert in carbonService.priceAlerts(
                    creditType: creditType,
                    threshold: threshold,
                    direction: direction
                ) {
                    await MainActor.run {
                        self.alerts.append(alert)
                        self.showNotification(for: alert)
                    }
                }
            } catch {
                print("Price alert error: \(error)")
            }
        }

        alertTasks.append(task)
    }

    private func showNotification(for alert: PriceAlert) {
        let content = UNMutableNotificationContent()
        content.title = "Price Alert"
        content.body = "\(alert.creditType.rawValue) is now \(alert.direction.rawValue) $\(alert.threshold)"
        content.sound = .default

        let request = UNNotificationRequest(
            identifier: UUID().uuidString,
            content: content,
            trigger: nil
        )

        UNUserNotificationCenter.current().add(request)
    }
}
```

## Security Features

### Biometric Authentication

```swift
import LocalAuthentication

class BiometricAuthManager: ObservableObject {
    private let context = LAContext()

    func authenticateWithBiometrics() async throws -> Bool {
        var error: NSError?

        guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
            throw AuthenticationError.biometricNotAvailable
        }

        let reason = "Authenticate with WRFCoin to access your account"

        do {
            let success = try await context.evaluatePolicy(
                .deviceOwnerAuthenticationWithBiometrics,
                localizedReason: reason
            )
            return success
        } catch {
            throw AuthenticationError.biometricFailed(error.localizedDescription)
        }
    }

    func getBiometricType() -> LABiometryType {
        return context.biometryType
    }
}

enum AuthenticationError: LocalizedError {
    case biometricNotAvailable
    case biometricFailed(String)

    var errorDescription: String? {
        switch self {
        case .biometricNotAvailable:
            return "Biometric authentication is not available on this device"
        case .biometricFailed(let message):
            return "Biometric authentication failed: \(message)"
        }
    }
}
```

### Keychain Integration

```swift
import Security

class KeychainManager {
    static let shared = KeychainManager()
    private init() {}

    func store(key: String, data: Data) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]

        SecItemDelete(query as CFDictionary)

        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeychainError.storeFailed(status)
        }
    }

    func retrieve(key: String) throws -> Data {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]

        var dataTypeRef: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &dataTypeRef)

        guard status == errSecSuccess else {
            throw KeychainError.retrieveFailed(status)
        }

        guard let data = dataTypeRef as? Data else {
            throw KeychainError.invalidData
        }

        return data
    }

    func delete(key: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key
        ]

        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainError.deleteFailed(status)
        }
    }
}

enum KeychainError: LocalizedError {
    case storeFailed(OSStatus)
    case retrieveFailed(OSStatus)
    case deleteFailed(OSStatus)
    case invalidData

    var errorDescription: String? {
        switch self {
        case .storeFailed(let status):
            return "Failed to store data in Keychain: \(status)"
        case .retrieveFailed(let status):
            return "Failed to retrieve data from Keychain: \(status)"
        case .deleteFailed(let status):
            return "Failed to delete data from Keychain: \(status)"
        case .invalidData:
            return "Invalid data retrieved from Keychain"
        }
    }
}
```

## Offline Support

### Core Data Stack

```swift
import CoreData

class CoreDataManager: ObservableObject {
    static let shared = CoreDataManager()

    lazy var persistentContainer: NSPersistentContainer = {
        let container = NSPersistentContainer(name: "WRFCoinModel")
        container.loadPersistentStores { _, error in
            if let error = error {
                fatalError("Core Data error: \(error)")
            }
        }
        return container
    }()

    var context: NSManagedObjectContext {
        return persistentContainer.viewContext
    }

    func save() {
        if context.hasChanges {
            do {
                try context.save()
            } catch {
                print("Save error: \(error)")
            }
        }
    }
}

// Weather Data Entity
@objc(WeatherDataEntity)
class WeatherDataEntity: NSManagedObject {
    @NSManaged var id: String
    @NSManaged var latitude: Double
    @NSManaged var longitude: Double
    @NSManaged var temperature: Double
    @NSManaged var condition: String
    @NSManaged var humidity: Int16
    @NSManaged var timestamp: Date
}

extension WeatherDataEntity {
    static func create(from weather: WeatherData, in context: NSManagedObjectContext) -> WeatherDataEntity {
        let entity = WeatherDataEntity(context: context)
        entity.id = weather.id
        entity.latitude = weather.location.latitude
        entity.longitude = weather.location.longitude
        entity.temperature = weather.temperature
        entity.condition = weather.condition
        entity.humidity = Int16(weather.humidity)
        entity.timestamp = weather.timestamp
        return entity
    }

    func toDomain() -> WeatherData {
        return WeatherData(
            id: id,
            temperature: temperature,
            condition: condition,
            humidity: Int(humidity),
            location: Location(
                latitude: latitude,
                longitude: longitude,
                name: "",
                region: "",
                country: ""
            ),
            timestamp: timestamp
        )
    }
}
```

### Background Sync

```swift
import BackgroundTasks

class BackgroundSyncManager {
    static let shared = BackgroundSyncManager()
    private let backgroundTaskIdentifier = "com.wrfcoin.background-sync"

    func registerBackgroundTasks() {
        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: backgroundTaskIdentifier,
            using: nil
        ) { task in
            self.handleBackgroundSync(task: task as! BGAppRefreshTask)
        }
    }

    func scheduleBackgroundSync() {
        let request = BGAppRefreshTaskRequest(identifier: backgroundTaskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60) // 15 minutes

        do {
            try BGTaskScheduler.shared.submit(request)
        } catch {
            print("Could not schedule background sync: \(error)")
        }
    }

    private func handleBackgroundSync(task: BGAppRefreshTask) {
        scheduleBackgroundSync() // Schedule next sync

        let syncTask = Task {
            do {
                await WRFCoinSDK.shared.syncOfflineData()
                task.setTaskCompleted(success: true)
            } catch {
                task.setTaskCompleted(success: false)
            }
        }

        task.expirationHandler = {
            syncTask.cancel()
        }
    }
}
```

## Testing

### Unit Testing

```swift
import XCTest
@testable import WRFCoinSDK

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

    override func tearDown() {
        weatherService = nil
        mockNetworkManager = nil
        mockCacheManager = nil
        super.tearDown()
    }

    func testGetCurrentWeather_Success() async throws {
        // Given
        let expectedWeather = WeatherData(
            id: "test-id",
            temperature: 25.0,
            condition: "Sunny",
            humidity: 60,
            location: Location(latitude: 40.7128, longitude: -74.0060, name: "Test", region: "", country: ""),
            timestamp: Date()
        )
        mockNetworkManager.mockWeatherData = expectedWeather

        // When
        let result = try await weatherService.getCurrentWeather(
            latitude: 40.7128,
            longitude: -74.0060
        )

        // Then
        XCTAssertEqual(result.temperature, expectedWeather.temperature)
        XCTAssertEqual(result.condition, expectedWeather.condition)
        XCTAssertEqual(result.humidity, expectedWeather.humidity)
    }

    func testGetCurrentWeather_NetworkError() async {
        // Given
        mockNetworkManager.shouldFailRequest = true

        // When/Then
        do {
            _ = try await weatherService.getCurrentWeather(latitude: 40.7128, longitude: -74.0060)
            XCTFail("Expected error to be thrown")
        } catch {
            XCTAssertTrue(error is NetworkError)
        }
    }
}

// Mock classes
class MockNetworkManager: NetworkManagerProtocol {
    var mockWeatherData: WeatherData?
    var shouldFailRequest = false

    func request<T: Decodable>(_ endpoint: APIEndpoint) async throws -> T {
        if shouldFailRequest {
            throw NetworkError.requestFailed
        }

        if let weatherData = mockWeatherData as? T {
            return weatherData
        }

        throw NetworkError.decodingFailed
    }
}

class MockCacheManager: CacheManagerProtocol {
    private var cache: [String: Any] = [:]

    func store<T: Codable>(_ object: T, forKey key: String) {
        cache[key] = object
    }

    func retrieve<T: Codable>(_ type: T.Type, forKey key: String) -> T? {
        return cache[key] as? T
    }

    func remove(forKey key: String) {
        cache.removeValue(forKey: key)
    }
}
```

### UI Testing with SwiftUI

```swift
import XCTest
@testable import WRFCoinApp

class WeatherViewTests: XCTestCase {
    func testWeatherCardView_DisplaysTemperature() throws {
        let app = XCUIApplication()
        app.launch()

        // Wait for weather data to load
        let temperatureText = app.staticTexts.matching(NSPredicate(format: "label CONTAINS '°'")).firstMatch
        XCTAssertTrue(temperatureText.waitForExistence(timeout: 5))

        // Verify temperature is displayed
        XCTAssertTrue(temperatureText.exists)
    }

    func testWeatherView_RefreshFunctionality() throws {
        let app = XCUIApplication()
        app.launch()

        // Pull to refresh
        let scrollView = app.scrollViews.firstMatch
        scrollView.swipeDown()

        // Verify loading indicator appears
        let loadingIndicator = app.activityIndicators.firstMatch
        XCTAssertTrue(loadingIndicator.waitForExistence(timeout: 2))
    }
}
```

## Performance Optimization

### Image Caching

```swift
import UIKit

actor ImageCache {
    private var cache = NSCache<NSString, UIImage>()

    init() {
        cache.countLimit = 100
        cache.totalCostLimit = 50 * 1024 * 1024 // 50MB
    }

    func image(for url: String) -> UIImage? {
        return cache.object(forKey: url as NSString)
    }

    func setImage(_ image: UIImage, for url: String) {
        cache.setObject(image, forKey: url as NSString, cost: image.jpegData(compressionQuality: 0.8)?.count ?? 0)
    }

    func removeImage(for url: String) {
        cache.removeObject(forKey: url as NSString)
    }

    func clearCache() {
        cache.removeAllObjects()
    }
}
```

### Network Request Optimization

```swift
import Foundation

class NetworkOptimizer {
    private let urlSession: URLSession

    init() {
        let configuration = URLSessionConfiguration.default
        configuration.timeoutIntervalForRequest = 30
        configuration.timeoutIntervalForResource = 60
        configuration.waitsForConnectivity = true
        configuration.httpMaximumConnectionsPerHost = 4

        // Enable HTTP/3 and HTTP/2
        configuration.httpShouldUsePipelining = true

        // Configure cache
        configuration.urlCache = URLCache(
            memoryCapacity: 20 * 1024 * 1024, // 20MB
            diskCapacity: 100 * 1024 * 1024,  // 100MB
            diskPath: "wrfcoin_cache"
        )
        configuration.requestCachePolicy = .useProtocolCachePolicy

        self.urlSession = URLSession(configuration: configuration)
    }

    func optimizedRequest(for url: URL) -> URLRequest {
        var request = URLRequest(url: url)
        request.setValue("gzip, deflate", forHTTPHeaderField: "Accept-Encoding")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("WRFCoin-iOS/1.0", forHTTPHeaderField: "User-Agent")
        return request
    }
}
```

## Error Handling

### Custom Error Types

```swift
public enum WRFCoinError: LocalizedError {
    case networkError(NetworkError)
    case authenticationError(AuthenticationError)
    case validationError(String)
    case serverError(Int, String)
    case rateLimitExceeded(retryAfter: TimeInterval)
    case unknown(Error)

    public var errorDescription: String? {
        switch self {
        case .networkError(let error):
            return "Network error: \(error.localizedDescription)"
        case .authenticationError(let error):
            return "Authentication error: \(error.localizedDescription)"
        case .validationError(let message):
            return "Validation error: \(message)"
        case .serverError(let code, let message):
            return "Server error (\(code)): \(message)"
        case .rateLimitExceeded(let retryAfter):
            return "Rate limit exceeded. Retry after \(Int(retryAfter)) seconds."
        case .unknown(let error):
            return "Unknown error: \(error.localizedDescription)"
        }
    }

    public var recoverySuggestion: String? {
        switch self {
        case .networkError:
            return "Please check your internet connection and try again."
        case .authenticationError:
            return "Please log in again."
        case .validationError:
            return "Please check your input and try again."
        case .serverError:
            return "Please try again later."
        case .rateLimitExceeded:
            return "Please wait before making another request."
        case .unknown:
            return "Please try again."
        }
    }
}
```

### Global Error Handler

```swift
@MainActor
class GlobalErrorHandler: ObservableObject {
    @Published var currentError: ErrorInfo?
    @Published var showingError = false

    func handle(_ error: Error) {
        let errorInfo = ErrorInfo(from: error)
        self.currentError = errorInfo
        self.showingError = true
    }

    func dismissError() {
        showingError = false
        currentError = nil
    }
}

struct ErrorInfo {
    let title: String
    let message: String
    let action: ErrorAction

    init(from error: Error) {
        switch error {
        case let wrfError as WRFCoinError:
            self.title = "WRFCoin Error"
            self.message = wrfError.localizedDescription
            self.action = .retry
        case is URLError:
            self.title = "Network Error"
            self.message = "Please check your internet connection"
            self.action = .retry
        default:
            self.title = "Unexpected Error"
            self.message = "Something went wrong. Please try again."
            self.action = .dismiss
        }
    }
}

enum ErrorAction {
    case retry
    case login
    case dismiss
}
```

## Migration Guide

### From Version 0.x to 1.0

```swift
// Old way (v0.x)
WRFCoin.initialize(apiKey: "your-api-key")
WRFCoin.shared.getWeather(lat: 40.7128, lng: -74.0060) { result in
    // Handle result
}

// New way (v1.0)
let config = WRFCoinConfiguration(apiKey: "your-api-key")
WRFCoinSDK.configure(with: config)

Task {
    do {
        let weather = try await WRFCoinSDK.shared.weatherService.getCurrentWeather(
            latitude: 40.7128,
            longitude: -74.0060
        )
        // Handle weather data
    } catch {
        // Handle error
    }
}
```

## Support

- **Documentation**: [https://docs.wrfcoin.com/ios](https://docs.wrfcoin.com/ios)
- **GitHub Issues**:
  [https://github.com/wrfcoin/ios-sdk/issues](https://github.com/wrfcoin/ios-sdk/issues)
- **Stack Overflow**: Tag your questions with `wrfcoin-ios`
- **Discord**: [WRFCoin Developer Community](https://discord.gg/wrfcoin)
- **Email**: ios-sdk@wrfcoin.com

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
