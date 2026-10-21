# Carbon Tracker App Example - iOS SDK

This example demonstrates building a comprehensive carbon footprint tracking application using the
WRFCoin iOS SDK with SwiftUI, Combine, and modern iOS development patterns.

## Overview

The Carbon Tracker App showcases:

- Personal carbon footprint calculation
- Activity tracking and categorization
- Carbon offset recommendations
- Progress visualization with charts
- Social sharing and challenges
- Real-time carbon credit marketplace
- Integration with health and location data

## Project Structure

```
CarbonTracker/
├── CarbonTracker/
│   ├── App/
│   │   ├── CarbonTrackerApp.swift
│   │   └── ContentView.swift
│   ├── Models/
│   │   ├── CarbonActivity.swift
│   │   ├── CarbonFootprint.swift
│   │   └── CarbonCredit.swift
│   ├── ViewModels/
│   │   ├── CarbonTrackingViewModel.swift
│   │   ├── OffsetViewModel.swift
│   │   └── MarketplaceViewModel.swift
│   ├── Views/
│   │   ├── Dashboard/
│   │   ├── Tracking/
│   │   ├── Marketplace/
│   │   └── Profile/
│   ├── Services/
│   │   ├── CarbonCalculationService.swift
│   │   ├── HealthKitService.swift
│   │   └── LocationService.swift
│   └── Utils/
│       ├── Extensions/
│       └── Helpers/
├── CarbonTrackerWidget/
└── CarbonTrackerTests/
```

## Dependencies

Add to your `Package.swift` or use Xcode's Package Manager:

```swift
dependencies: [
    .package(url: "https://github.com/wrfcoin/ios-sdk", from: "1.0.0"),
    .package(url: "https://github.com/danielgindi/Charts", from: "4.1.0"),
    .package(url: "https://github.com/apple/swift-collections", from: "1.0.0")
]
```

## App Configuration

### CarbonTrackerApp.swift

```swift
import SwiftUI
import WRFCoinSDK
import HealthKit
import CoreLocation

@main
struct CarbonTrackerApp: App {
    @StateObject private var appState = AppState()

    init() {
        configureWRFCoinSDK()
        requestPermissions()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(appState)
                .onAppear {
                    Task {
                        await appState.initialize()
                    }
                }
        }
    }

    private func configureWRFCoinSDK() {
        let config = WRFCoinConfiguration(
            apiKey: "your-api-key",
            environment: .production,
            enableLogging: true,
            enableOfflineSupport: true,
            enableBiometricAuth: true
        )

        WRFCoinSDK.configure(with: config)
    }

    private func requestPermissions() {
        // Request location permission
        LocationService.shared.requestPermission()

        // Request HealthKit permission
        HealthKitService.shared.requestAuthorization()
    }
}

@MainActor
class AppState: ObservableObject {
    @Published var isAuthenticated = false
    @Published var currentUser: User?
    @Published var hasCompletedOnboarding = false

    private let sdk = WRFCoinSDK.shared

    func initialize() async {
        do {
            // Check if user is already authenticated
            if let token = try? KeychainManager.shared.retrieve(key: "auth_token"),
               let tokenString = String(data: token, encoding: .utf8) {

                sdk.setAuthToken(tokenString)
                let user = try await sdk.userService.getCurrentUser()

                self.currentUser = user
                self.isAuthenticated = true
                self.hasCompletedOnboarding = user.hasCompletedOnboarding
            }
        } catch {
            print("Failed to initialize app state: \(error)")
        }
    }
}
```

## Data Models

### CarbonActivity.swift

```swift
import Foundation
import WRFCoinSDK

struct CarbonActivity: Identifiable, Codable {
    let id = UUID()
    let type: ActivityType
    let amount: Double
    let unit: String
    let co2Emission: Double
    let timestamp: Date
    let location: CLLocation?
    let metadata: [String: String]

    enum ActivityType: String, CaseIterable, Codable {
        case transportation = "transportation"
        case energy = "energy"
        case food = "food"
        case waste = "waste"
        case shopping = "shopping"
        case travel = "travel"

        var icon: String {
            switch self {
            case .transportation: return "car.fill"
            case .energy: return "bolt.fill"
            case .food: return "fork.knife"
            case .waste: return "trash.fill"
            case .shopping: return "bag.fill"
            case .travel: return "airplane"
            }
        }

        var color: Color {
            switch self {
            case .transportation: return .blue
            case .energy: return .yellow
            case .food: return .green
            case .waste: return .red
            case .shopping: return .purple
            case .travel: return .orange
            }
        }
    }

    static func fromTransportation(distance: Double, mode: TransportMode) -> CarbonActivity {
        let co2Factor: Double
        switch mode {
        case .car: co2Factor = 0.21 // kg CO2 per km
        case .bus: co2Factor = 0.089
        case .train: co2Factor = 0.041
        case .plane: co2Factor = 0.255
        case .bike: co2Factor = 0.0
        case .walking: co2Factor = 0.0
        }

        return CarbonActivity(
            type: .transportation,
            amount: distance,
            unit: "km",
            co2Emission: distance * co2Factor,
            timestamp: Date(),
            location: nil,
            metadata: ["mode": mode.rawValue]
        )
    }

    static func fromEnergyUsage(kwh: Double, energySource: EnergySource) -> CarbonActivity {
        let co2Factor: Double
        switch energySource {
        case .coal: co2Factor = 0.820 // kg CO2 per kWh
        case .naturalGas: co2Factor = 0.490
        case .renewable: co2Factor = 0.050
        case .nuclear: co2Factor = 0.012
        case .mixed: co2Factor = 0.500 // average grid mix
        }

        return CarbonActivity(
            type: .energy,
            amount: kwh,
            unit: "kWh",
            co2Emission: kwh * co2Factor,
            timestamp: Date(),
            location: nil,
            metadata: ["source": energySource.rawValue]
        )
    }
}

enum TransportMode: String, CaseIterable {
    case car = "car"
    case bus = "bus"
    case train = "train"
    case plane = "plane"
    case bike = "bike"
    case walking = "walking"
}

enum EnergySource: String, CaseIterable {
    case coal = "coal"
    case naturalGas = "natural_gas"
    case renewable = "renewable"
    case nuclear = "nuclear"
    case mixed = "mixed"
}
```

### CarbonFootprint.swift

```swift
import Foundation

struct CarbonFootprint: Codable {
    let id: String
    let userId: String
    let period: Period
    let totalEmissions: Double // kg CO2
    let activitiesByCategory: [ActivityType: Double]
    let comparisonToAverage: Double // percentage
    let offsetCredits: Double
    let netEmissions: Double
    let createdAt: Date
    let updatedAt: Date

    enum Period: String, CaseIterable, Codable {
        case daily = "daily"
        case weekly = "weekly"
        case monthly = "monthly"
        case yearly = "yearly"

        var displayName: String {
            switch self {
            case .daily: return "Today"
            case .weekly: return "This Week"
            case .monthly: return "This Month"
            case .yearly: return "This Year"
            }
        }
    }

    var offsetPercentage: Double {
        guard totalEmissions > 0 else { return 0 }
        return (offsetCredits / totalEmissions) * 100
    }

    var isNeutral: Bool {
        return netEmissions <= 0
    }

    var topCategories: [(ActivityType, Double)] {
        return activitiesByCategory
            .sorted { $0.value > $1.value }
            .prefix(3)
            .map { ($0.key, $0.value) }
    }
}
```

## ViewModels

### CarbonTrackingViewModel.swift

```swift
import Foundation
import SwiftUI
import Combine
import WRFCoinSDK

@MainActor
class CarbonTrackingViewModel: ObservableObject {
    @Published var currentFootprint: CarbonFootprint?
    @Published var recentActivities: [CarbonActivity] = []
    @Published var isLoading = false
    @Published var error: String?
    @Published var selectedPeriod: CarbonFootprint.Period = .weekly

    private let carbonService = WRFCoinSDK.shared.carbonService
    private let calculationService = CarbonCalculationService()
    private let healthKitService = HealthKitService.shared
    private let locationService = LocationService.shared

    private var cancellables = Set<AnyCancellable>()

    init() {
        setupSubscriptions()
        loadInitialData()
    }

    private func setupSubscriptions() {
        // Listen for period changes
        $selectedPeriod
            .removeDuplicates()
            .sink { [weak self] _ in
                Task {
                    await self?.loadFootprint()
                }
            }
            .store(in: &cancellables)

        // Listen for location updates for automatic tracking
        locationService.significantLocationChanges
            .sink { [weak self] location in
                Task {
                    await self?.trackLocationBasedActivity(location)
                }
            }
            .store(in: &cancellables)

        // Listen for HealthKit step updates
        healthKitService.stepsPublisher
            .sink { [weak self] steps in
                Task {
                    await self?.updateWalkingActivity(steps: steps)
                }
            }
            .store(in: &cancellables)
    }

    private func loadInitialData() {
        Task {
            await loadFootprint()
            await loadRecentActivities()
        }
    }

    func loadFootprint() async {
        isLoading = true
        error = nil

        do {
            let footprint = try await carbonService.getFootprint(
                period: selectedPeriod,
                userId: WRFCoinSDK.shared.userService.currentUserId
            )

            self.currentFootprint = footprint
        } catch {
            self.error = "Failed to load carbon footprint: \(error.localizedDescription)"
        }

        isLoading = false
    }

    func loadRecentActivities() async {
        do {
            let activities = try await carbonService.getRecentActivities(limit: 20)
            self.recentActivities = activities
        } catch {
            print("Failed to load recent activities: \(error)")
        }
    }

    func addManualActivity(_ activity: CarbonActivity) async {
        do {
            let savedActivity = try await carbonService.logActivity(activity)
            self.recentActivities.insert(savedActivity, at: 0)

            // Recalculate footprint
            await loadFootprint()

        } catch {
            self.error = "Failed to add activity: \(error.localizedDescription)"
        }
    }

    func deleteActivity(_ activity: CarbonActivity) async {
        do {
            try await carbonService.deleteActivity(activity.id.uuidString)
            self.recentActivities.removeAll { $0.id == activity.id }

            // Recalculate footprint
            await loadFootprint()

        } catch {
            self.error = "Failed to delete activity: \(error.localizedDescription)"
        }
    }

    private func trackLocationBasedActivity(_ location: CLLocation) async {
        // Implement automatic activity detection based on location changes
        // This could include detecting travel patterns, commute routes, etc.

        let distance = locationService.calculateTravelDistance()
        let mode = await locationService.inferTransportMode()

        if distance > 0.5 { // Only track trips longer than 0.5km
            let activity = CarbonActivity.fromTransportation(
                distance: distance,
                mode: mode
            )

            await addManualActivity(activity)
        }
    }

    private func updateWalkingActivity(steps: Int) async {
        // Convert steps to distance (average step length ~0.762m)
        let distance = Double(steps) * 0.000762 // km

        let today = Calendar.current.startOfDay(for: Date())

        // Check if we already have a walking activity for today
        if let existingActivity = recentActivities.first(where: {
            $0.type == .transportation &&
            $0.metadata["mode"] == "walking" &&
            Calendar.current.isDate($0.timestamp, inSameDayAs: today)
        }) {
            // Update existing activity
            let updatedActivity = CarbonActivity(
                type: .transportation,
                amount: distance,
                unit: "km",
                co2Emission: 0, // Walking has no CO2 emissions
                timestamp: Date(),
                location: nil,
                metadata: ["mode": "walking", "steps": String(steps)]
            )

            await addManualActivity(updatedActivity)
        } else if steps > 1000 { // Only track if significant walking
            let activity = CarbonActivity.fromTransportation(
                distance: distance,
                mode: .walking
            )

            await addManualActivity(activity)
        }
    }

    func exportData() async -> URL? {
        do {
            let exportData = CarbonDataExport(
                footprint: currentFootprint,
                activities: recentActivities,
                exportDate: Date()
            )

            let jsonData = try JSONEncoder().encode(exportData)

            let documentsPath = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            let exportURL = documentsPath.appendingPathComponent("carbon_data_export.json")

            try jsonData.write(to: exportURL)
            return exportURL

        } catch {
            self.error = "Failed to export data: \(error.localizedDescription)"
            return nil
        }
    }
}

struct CarbonDataExport: Codable {
    let footprint: CarbonFootprint?
    let activities: [CarbonActivity]
    let exportDate: Date
}
```

## Views

### Dashboard View

```swift
import SwiftUI
import Charts
import WRFCoinSDK

struct DashboardView: View {
    @StateObject private var viewModel = CarbonTrackingViewModel()
    @State private var showingAddActivity = false
    @State private var selectedCategory: ActivityType?

    var body: some View {
        NavigationView {
            ScrollView {
                LazyVStack(spacing: 20) {
                    // Period Selector
                    PeriodSelectorView(selectedPeriod: $viewModel.selectedPeriod)

                    // Main Carbon Footprint Card
                    if let footprint = viewModel.currentFootprint {
                        CarbonFootprintCardView(footprint: footprint)
                    }

                    // Emissions Breakdown Chart
                    EmissionsBreakdownChartView(
                        activities: viewModel.currentFootprint?.activitiesByCategory ?? [:],
                        selectedCategory: $selectedCategory
                    )

                    // Quick Actions
                    QuickActionsView {
                        showingAddActivity = true
                    }

                    // Recent Activities
                    RecentActivitiesView(activities: viewModel.recentActivities) { activity in
                        Task {
                            await viewModel.deleteActivity(activity)
                        }
                    }

                    // Offset Recommendations
                    if let footprint = viewModel.currentFootprint,
                       footprint.netEmissions > 0 {
                        OffsetRecommendationsView(emissions: footprint.netEmissions)
                    }
                }
                .padding()
            }
            .navigationTitle("Carbon Tracker")
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Add") {
                        showingAddActivity = true
                    }
                }
            }
            .refreshable {
                await viewModel.loadFootprint()
                await viewModel.loadRecentActivities()
            }
            .sheet(isPresented: $showingAddActivity) {
                AddActivityView { activity in
                    Task {
                        await viewModel.addManualActivity(activity)
                    }
                }
            }
            .alert("Error", isPresented: .constant(viewModel.error != nil)) {
                Button("OK") {
                    viewModel.error = nil
                }
            } message: {
                Text(viewModel.error ?? "")
            }
            .overlay {
                if viewModel.isLoading {
                    ProgressView("Loading...")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(Color.black.opacity(0.3))
                }
            }
        }
    }
}

struct CarbonFootprintCardView: View {
    let footprint: CarbonFootprint

    var body: some View {
        VStack(spacing: 16) {
            HStack {
                VStack(alignment: .leading) {
                    Text("Total Emissions")
                        .font(.headline)
                        .foregroundColor(.primary)

                    HStack(alignment: .lastTextBaseline, spacing: 4) {
                        Text("\(footprint.totalEmissions, specifier: "%.1f")")
                            .font(.system(size: 36, weight: .bold, design: .rounded))

                        Text("kg CO₂")
                            .font(.title3)
                            .foregroundColor(.secondary)
                    }

                    HStack {
                        Image(systemName: footprint.comparisonToAverage > 0 ? "arrow.up.right" : "arrow.down.right")
                            .foregroundColor(footprint.comparisonToAverage > 0 ? .red : .green)

                        Text("\(abs(footprint.comparisonToAverage), specifier: "%.0f")% vs average")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }

                Spacer()

                // Circular progress indicator
                ZStack {
                    Circle()
                        .stroke(Color.gray.opacity(0.3), lineWidth: 8)

                    Circle()
                        .trim(from: 0, to: min(footprint.offsetPercentage / 100, 1.0))
                        .stroke(
                            footprint.isNeutral ? Color.green : Color.orange,
                            style: StrokeStyle(lineWidth: 8, lineCap: .round)
                        )
                        .rotationEffect(.degrees(-90))

                    VStack {
                        Text("\(footprint.offsetPercentage, specifier: "%.0f")%")
                            .font(.title2)
                            .fontWeight(.bold)

                        Text("Offset")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
                .frame(width: 80, height: 80)
            }

            if footprint.isNeutral {
                HStack {
                    Image(systemName: "leaf.fill")
                        .foregroundColor(.green)

                    Text("Carbon Neutral!")
                        .font(.headline)
                        .foregroundColor(.green)

                    Spacer()
                }
                .padding()
                .background(Color.green.opacity(0.1))
                .cornerRadius(8)
            }
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(radius: 2)
    }
}

struct EmissionsBreakdownChartView: View {
    let activities: [ActivityType: Double]
    @Binding var selectedCategory: ActivityType?

    var chartData: [(category: String, value: Double, color: Color)] {
        activities.map { (key, value) in
            (
                category: key.rawValue.capitalized,
                value: value,
                color: key.color
            )
        }.sorted { $0.value > $1.value }
    }

    var body: some View {
        VStack(alignment: .leading) {
            Text("Emissions Breakdown")
                .font(.headline)
                .padding(.horizontal)

            if !chartData.isEmpty {
                Chart(chartData, id: \.category) { item in
                    SectorMark(
                        angle: .value("Emissions", item.value),
                        innerRadius: .ratio(0.5),
                        angularInset: 2
                    )
                    .foregroundStyle(item.color)
                    .opacity(selectedCategory?.rawValue.capitalized == item.category ? 1.0 : 0.7)
                }
                .frame(height: 200)
                .chartLegend(position: .bottom, alignment: .center)
                .padding()
            } else {
                Text("No data available")
                    .foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding()
            }
        }
        .background(Color(.systemBackground))
        .cornerRadius(12)
        .shadow(radius: 2)
    }
}

struct QuickActionsView: View {
    let onAddActivity: () -> Void

    var body: some View {
        VStack(alignment: .leading) {
            Text("Quick Actions")
                .font(.headline)
                .padding(.horizontal)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 12) {
                    QuickActionButton(
                        icon: "car.fill",
                        title: "Car Trip",
                        color: .blue,
                        action: onAddActivity
                    )

                    QuickActionButton(
                        icon: "airplane",
                        title: "Flight",
                        color: .orange,
                        action: onAddActivity
                    )

                    QuickActionButton(
                        icon: "bolt.fill",
                        title: "Energy",
                        color: .yellow,
                        action: onAddActivity
                    )

                    QuickActionButton(
                        icon: "fork.knife",
                        title: "Meal",
                        color: .green,
                        action: onAddActivity
                    )

                    QuickActionButton(
                        icon: "plus",
                        title: "Custom",
                        color: .gray,
                        action: onAddActivity
                    )
                }
                .padding(.horizontal)
            }
        }
    }
}

struct QuickActionButton: View {
    let icon: String
    let title: String
    let color: Color
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.title2)
                    .foregroundColor(.white)
                    .frame(width: 50, height: 50)
                    .background(color)
                    .clipShape(Circle())

                Text(title)
                    .font(.caption)
                    .foregroundColor(.primary)
            }
        }
        .buttonStyle(PlainButtonStyle())
    }
}
```

### Add Activity View

```swift
struct AddActivityView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var selectedType: ActivityType = .transportation
    @State private var amount: String = ""
    @State private var selectedMode: TransportMode = .car
    @State private var selectedEnergySource: EnergySource = .mixed
    @State private var notes: String = ""

    let onSave: (CarbonActivity) -> Void

    var body: some View {
        NavigationView {
            Form {
                Section("Activity Type") {
                    Picker("Type", selection: $selectedType) {
                        ForEach(ActivityType.allCases, id: \.self) { type in
                            HStack {
                                Image(systemName: type.icon)
                                    .foregroundColor(type.color)
                                Text(type.rawValue.capitalized)
                            }
                            .tag(type)
                        }
                    }
                    .pickerStyle(.menu)
                }

                Section("Details") {
                    HStack {
                        Text("Amount")
                        Spacer()
                        TextField("0", text: $amount)
                            .keyboardType(.decimalPad)
                            .multilineTextAlignment(.trailing)
                        Text(unitForType(selectedType))
                            .foregroundColor(.secondary)
                    }

                    if selectedType == .transportation {
                        Picker("Transport Mode", selection: $selectedMode) {
                            ForEach(TransportMode.allCases, id: \.self) { mode in
                                Text(mode.rawValue.capitalized)
                                    .tag(mode)
                            }
                        }
                    }

                    if selectedType == .energy {
                        Picker("Energy Source", selection: $selectedEnergySource) {
                            ForEach(EnergySource.allCases, id: \.self) { source in
                                Text(source.rawValue.replacingOccurrences(of: "_", with: " ").capitalized)
                                    .tag(source)
                            }
                        }
                    }

                    TextField("Notes (optional)", text: $notes, axis: .vertical)
                        .lineLimit(3...6)
                }

                if !amount.isEmpty, let amountValue = Double(amount) {
                    Section("Environmental Impact") {
                        let co2Emission = calculateCO2Emission(
                            type: selectedType,
                            amount: amountValue,
                            mode: selectedMode,
                            energySource: selectedEnergySource
                        )

                        HStack {
                            Image(systemName: "leaf.fill")
                                .foregroundColor(.green)

                            Text("CO₂ Emissions")

                            Spacer()

                            Text("\(co2Emission, specifier: "%.2f") kg")
                                .fontWeight(.semibold)
                        }

                        if co2Emission > 0 {
                            Text("Consider offsetting this emission with carbon credits")
                                .font(.caption)
                                .foregroundColor(.secondary)
                        }
                    }
                }
            }
            .navigationTitle("Add Activity")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarLeading) {
                    Button("Cancel") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Save") {
                        saveActivity()
                    }
                    .disabled(amount.isEmpty)
                }
            }
        }
    }

    private func unitForType(_ type: ActivityType) -> String {
        switch type {
        case .transportation: return "km"
        case .energy: return "kWh"
        case .food: return "meals"
        case .waste: return "kg"
        case .shopping: return "items"
        case .travel: return "km"
        }
    }

    private func calculateCO2Emission(
        type: ActivityType,
        amount: Double,
        mode: TransportMode,
        energySource: EnergySource
    ) -> Double {
        switch type {
        case .transportation:
            switch mode {
            case .car: return amount * 0.21
            case .bus: return amount * 0.089
            case .train: return amount * 0.041
            case .plane: return amount * 0.255
            case .bike, .walking: return 0.0
            }
        case .energy:
            switch energySource {
            case .coal: return amount * 0.820
            case .naturalGas: return amount * 0.490
            case .renewable: return amount * 0.050
            case .nuclear: return amount * 0.012
            case .mixed: return amount * 0.500
            }
        case .food: return amount * 2.5 // Average meal
        case .waste: return amount * 0.5
        case .shopping: return amount * 5.0 // Rough estimate per item
        case .travel: return amount * 0.15 // Average travel emissions
        }
    }

    private func saveActivity() {
        guard let amountValue = Double(amount) else { return }

        let activity: CarbonActivity

        switch selectedType {
        case .transportation:
            activity = CarbonActivity.fromTransportation(
                distance: amountValue,
                mode: selectedMode
            )
        case .energy:
            activity = CarbonActivity.fromEnergyUsage(
                kwh: amountValue,
                energySource: selectedEnergySource
            )
        default:
            let co2Emission = calculateCO2Emission(
                type: selectedType,
                amount: amountValue,
                mode: selectedMode,
                energySource: selectedEnergySource
            )

            activity = CarbonActivity(
                type: selectedType,
                amount: amountValue,
                unit: unitForType(selectedType),
                co2Emission: co2Emission,
                timestamp: Date(),
                location: nil,
                metadata: notes.isEmpty ? [:] : ["notes": notes]
            )
        }

        onSave(activity)
        dismiss()
    }
}
```

## Services

### CarbonCalculationService.swift

```swift
import Foundation
import CoreLocation

class CarbonCalculationService {

    // Emission factors (kg CO2 per unit)
    private let emissionFactors: [String: [String: Double]] = [
        "transportation": [
            "car_gasoline": 0.21,      // per km
            "car_diesel": 0.19,
            "car_electric": 0.05,
            "bus": 0.089,
            "train_electric": 0.041,
            "train_diesel": 0.083,
            "plane_domestic": 0.255,
            "plane_international": 0.195,
            "motorcycle": 0.113,
            "bike": 0.0,
            "walking": 0.0
        ],
        "energy": [
            "coal": 0.820,              // per kWh
            "natural_gas": 0.490,
            "oil": 0.733,
            "renewable": 0.050,
            "nuclear": 0.012,
            "grid_average": 0.500
        ],
        "food": [
            "beef": 27.0,               // per kg
            "pork": 12.1,
            "chicken": 6.9,
            "fish": 6.1,
            "dairy": 3.2,
            "eggs": 4.8,
            "vegetables": 2.0,
            "fruits": 1.1,
            "grains": 2.7
        ]
    ]

    func calculateTransportationEmissions(
        distance: Double,
        mode: TransportMode,
        fuelType: String = "gasoline"
    ) -> Double {
        let key = "\(mode.rawValue)_\(fuelType)"
        let factor = emissionFactors["transportation"]?[key] ?? emissionFactors["transportation"]?[mode.rawValue] ?? 0
        return distance * factor
    }

    func calculateEnergyEmissions(
        consumption: Double,
        source: EnergySource
    ) -> Double {
        let factor = emissionFactors["energy"]?[source.rawValue] ?? 0
        return consumption * factor
    }

    func calculateFoodEmissions(
        weight: Double,
        foodType: String
    ) -> Double {
        let factor = emissionFactors["food"]?[foodType] ?? 2.0 // default factor
        return weight * factor
    }

    func calculateMonthlyFootprint(activities: [CarbonActivity]) -> CarbonFootprint {
        let calendar = Calendar.current
        let now = Date()
        let startOfMonth = calendar.dateInterval(of: .month, for: now)?.start ?? now

        let monthlyActivities = activities.filter { activity in
            activity.timestamp >= startOfMonth
        }

        var categorizedEmissions: [ActivityType: Double] = [:]

        for activity in monthlyActivities {
            categorizedEmissions[activity.type, default: 0] += activity.co2Emission
        }

        let totalEmissions = categorizedEmissions.values.reduce(0, +)

        // Compare to average (example: 1000 kg CO2 per month average)
        let averageMonthlyEmissions = 1000.0
        let comparisonToAverage = ((totalEmissions - averageMonthlyEmissions) / averageMonthlyEmissions) * 100

        return CarbonFootprint(
            id: UUID().uuidString,
            userId: "current_user",
            period: .monthly,
            totalEmissions: totalEmissions,
            activitiesByCategory: categorizedEmissions,
            comparisonToAverage: comparisonToAverage,
            offsetCredits: 0, // This would be fetched from the API
            netEmissions: totalEmissions,
            createdAt: now,
            updatedAt: now
        )
    }

    func getOffsetRecommendations(for emissions: Double) -> [OffsetRecommendation] {
        var recommendations: [OffsetRecommendation] = []

        // Solar panel offset
        let solarPanelsNeeded = ceil(emissions / 500) // 500 kg CO2 per panel per year
        recommendations.append(OffsetRecommendation(
            type: .renewableEnergy,
            title: "Solar Panel Installation",
            description: "Offset your emissions by installing \(Int(solarPanelsNeeded)) solar panels",
            co2Offset: solarPanelsNeeded * 500,
            cost: solarPanelsNeeded * 150,
            timeframe: "Annual"
        ))

        // Tree planting
        let treesNeeded = ceil(emissions / 22) // 22 kg CO2 per tree per year
        recommendations.append(OffsetRecommendation(
            type: .forestry,
            title: "Tree Planting",
            description: "Plant \(Int(treesNeeded)) trees to offset your carbon footprint",
            co2Offset: treesNeeded * 22,
            cost: treesNeeded * 5,
            timeframe: "Annual"
        ))

        // Carbon credits
        recommendations.append(OffsetRecommendation(
            type: .carbonCredits,
            title: "Verified Carbon Credits",
            description: "Purchase verified carbon credits to immediately offset emissions",
            co2Offset: emissions,
            cost: emissions * 25, // $25 per ton
            timeframe: "Immediate"
        ))

        return recommendations
    }
}

struct OffsetRecommendation {
    let type: OffsetType
    let title: String
    let description: String
    let co2Offset: Double
    let cost: Double
    let timeframe: String

    enum OffsetType {
        case renewableEnergy
        case forestry
        case carbonCredits
        case energyEfficiency
    }
}
```

### HealthKitService.swift

```swift
import HealthKit
import Combine

class HealthKitService: ObservableObject {
    static let shared = HealthKitService()

    private let healthStore = HKHealthStore()
    private var stepsQuery: HKObserverQuery?

    @Published var stepsPublisher = PassthroughSubject<Int, Never>()
    @Published var isAuthorized = false

    private init() {}

    func requestAuthorization() {
        guard HKHealthStore.isHealthDataAvailable() else {
            print("HealthKit is not available on this device")
            return
        }

        let stepCount = HKQuantityType.quantityType(forIdentifier: .stepCount)!
        let distanceWalkingRunning = HKQuantityType.quantityType(forIdentifier: .distanceWalkingRunning)!

        let typesToRead: Set<HKObjectType> = [stepCount, distanceWalkingRunning]

        healthStore.requestAuthorization(toShare: nil, read: typesToRead) { [weak self] success, error in
            DispatchQueue.main.async {
                self?.isAuthorized = success
                if success {
                    self?.startObservingSteps()
                }
            }
        }
    }

    private func startObservingSteps() {
        guard let stepCountType = HKQuantityType.quantityType(forIdentifier: .stepCount) else {
            return
        }

        stepsQuery = HKObserverQuery(sampleType: stepCountType, predicate: nil) { [weak self] _, _, error in
            if error == nil {
                self?.fetchTodaySteps()
            }
        }

        if let query = stepsQuery {
            healthStore.execute(query)
        }

        // Initial fetch
        fetchTodaySteps()
    }

    private func fetchTodaySteps() {
        guard let stepCountType = HKQuantityType.quantityType(forIdentifier: .stepCount) else {
            return
        }

        let calendar = Calendar.current
        let startDate = calendar.startOfDay(for: Date())
        let endDate = Date()

        let predicate = HKQuery.predicateForSamples(withStart: startDate, end: endDate, options: .strictStartDate)

        let query = HKStatisticsQuery(
            quantityType: stepCountType,
            quantitySamplePredicate: predicate,
            options: .cumulativeSum
        ) { [weak self] _, result, _ in

            guard let result = result,
                  let sum = result.sumQuantity() else {
                return
            }

            let steps = Int(sum.doubleValue(for: HKUnit.count()))

            DispatchQueue.main.async {
                self?.stepsPublisher.send(steps)
            }
        }

        healthStore.execute(query)
    }

    func fetchWalkingDistance(completion: @escaping (Double) -> Void) {
        guard let distanceType = HKQuantityType.quantityType(forIdentifier: .distanceWalkingRunning) else {
            completion(0)
            return
        }

        let calendar = Calendar.current
        let startDate = calendar.startOfDay(for: Date())
        let endDate = Date()

        let predicate = HKQuery.predicateForSamples(withStart: startDate, end: endDate, options: .strictStartDate)

        let query = HKStatisticsQuery(
            quantityType: distanceType,
            quantitySamplePredicate: predicate,
            options: .cumulativeSum
        ) { _, result, _ in

            guard let result = result,
                  let sum = result.sumQuantity() else {
                completion(0)
                return
            }

            let distance = sum.doubleValue(for: HKUnit.meterUnit(with: .kilo))

            DispatchQueue.main.async {
                completion(distance)
            }
        }

        healthStore.execute(query)
    }

    deinit {
        if let query = stepsQuery {
            healthStore.stop(query)
        }
    }
}
```

## Widget Extension

### CarbonTrackerWidget.swift

```swift
import WidgetKit
import SwiftUI

struct CarbonTrackerWidget: Widget {
    let kind: String = "CarbonTrackerWidget"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: kind, provider: CarbonWidgetProvider()) { entry in
            CarbonWidgetEntryView(entry: entry)
        }
        .configurationDisplayName("Carbon Tracker")
        .description("Track your daily carbon footprint")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}

struct CarbonWidgetEntry: TimelineEntry {
    let date: Date
    let todayEmissions: Double
    let weeklyEmissions: Double
    let offsetPercentage: Double
}

struct CarbonWidgetProvider: TimelineProvider {
    func placeholder(in context: Context) -> CarbonWidgetEntry {
        CarbonWidgetEntry(
            date: Date(),
            todayEmissions: 12.5,
            weeklyEmissions: 87.3,
            offsetPercentage: 35
        )
    }

    func getSnapshot(in context: Context, completion: @escaping (CarbonWidgetEntry) -> ()) {
        let entry = CarbonWidgetEntry(
            date: Date(),
            todayEmissions: 12.5,
            weeklyEmissions: 87.3,
            offsetPercentage: 35
        )
        completion(entry)
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<CarbonWidgetEntry>) -> ()) {
        Task {
            let entries = await generateEntries()
            let timeline = Timeline(entries: entries, policy: .atEnd)
            completion(timeline)
        }
    }

    private func generateEntries() async -> [CarbonWidgetEntry] {
        // In a real app, this would fetch data from the SDK
        var entries: [CarbonWidgetEntry] = []
        let currentDate = Date()

        for hourOffset in 0..<5 {
            let entryDate = Calendar.current.date(byAdding: .hour, value: hourOffset, to: currentDate)!
            let entry = CarbonWidgetEntry(
                date: entryDate,
                todayEmissions: Double.random(in: 10...20),
                weeklyEmissions: Double.random(in: 70...100),
                offsetPercentage: Double.random(in: 20...60)
            )
            entries.append(entry)
        }

        return entries
    }
}

struct CarbonWidgetEntryView: View {
    var entry: CarbonWidgetProvider.Entry
    @Environment(\.widgetFamily) var family

    var body: some View {
        switch family {
        case .systemSmall:
            SmallWidgetView(entry: entry)
        case .systemMedium:
            MediumWidgetView(entry: entry)
        default:
            SmallWidgetView(entry: entry)
        }
    }
}

struct SmallWidgetView: View {
    let entry: CarbonWidgetEntry

    var body: some View {
        VStack(spacing: 8) {
            HStack {
                Image(systemName: "leaf.fill")
                    .foregroundColor(.green)
                Text("Today")
                    .font(.caption)
                    .fontWeight(.medium)
                Spacer()
            }

            HStack {
                Text("\(entry.todayEmissions, specifier: "%.1f")")
                    .font(.title2)
                    .fontWeight(.bold)
                Text("kg CO₂")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
            }

            Spacer()

            ProgressView(value: entry.offsetPercentage / 100) {
                HStack {
                    Text("Offset")
                        .font(.caption2)
                    Spacer()
                    Text("\(entry.offsetPercentage, specifier: "%.0f")%")
                        .font(.caption2)
                        .fontWeight(.medium)
                }
            }
            .tint(.green)
        }
        .padding()
        .background(Color(.systemBackground))
    }
}

struct MediumWidgetView: View {
    let entry: CarbonWidgetEntry

    var body: some View {
        HStack(spacing: 16) {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Image(systemName: "leaf.fill")
                        .foregroundColor(.green)
                    Text("Carbon Tracker")
                        .font(.headline)
                        .fontWeight(.semibold)
                    Spacer()
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("Today")
                        .font(.caption)
                        .foregroundColor(.secondary)

                    HStack(alignment: .lastTextBaseline, spacing: 4) {
                        Text("\(entry.todayEmissions, specifier: "%.1f")")
                            .font(.title)
                            .fontWeight(.bold)
                        Text("kg CO₂")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }

                Spacer()
            }

            VStack(alignment: .trailing, spacing: 8) {
                VStack(alignment: .trailing, spacing: 2) {
                    Text("This Week")
                        .font(.caption)
                        .foregroundColor(.secondary)

                    Text("\(entry.weeklyEmissions, specifier: "%.0f") kg")
                        .font(.headline)
                        .fontWeight(.semibold)
                }

                Spacer()

                VStack(alignment: .trailing, spacing: 2) {
                    Text("Offset")
                        .font(.caption)
                        .foregroundColor(.secondary)

                    Text("\(entry.offsetPercentage, specifier: "%.0f")%")
                        .font(.headline)
                        .fontWeight(.semibold)
                        .foregroundColor(.green)
                }
            }
        }
        .padding()
        .background(Color(.systemBackground))
    }
}
```

This comprehensive Carbon Tracker App example demonstrates how to build a feature-rich iOS
application using the WRFCoin SDK with SwiftUI, HealthKit integration, real-time data updates, and
modern iOS development practices including widgets and background processing.
