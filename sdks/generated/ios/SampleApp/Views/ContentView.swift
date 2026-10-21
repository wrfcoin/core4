import SwiftUI
import WRFCoinSDK

/**
 * Main content view with tab navigation.
 *
 * Demonstrates the core features of the WRFCoin SDK through multiple tabs:
 * - Dashboard: Overview and quick actions
 * - Weather: Weather station management and data visualization
 * - Blockchain: Blockchain explorer and transaction history
 * - Wallet: Wallet management and DeFi features
 * - Settings: App settings and SDK configuration
 */
struct ContentView: View {
    @EnvironmentObject var appState: AppState
    @EnvironmentObject var apiClient: MobileAPIClient
    
    var body: some View {
        TabView(selection: $appState.selectedTab) {
            DashboardView()
                .tabItem {
                    Image(systemName: "house.fill")
                    Text("Dashboard")
                }
                .tag(0)
            
            WeatherView()
                .tabItem {
                    Image(systemName: "cloud.sun.fill")
                    Text("Weather")
                }
                .tag(1)
            
            BlockchainView()
                .tabItem {
                    Image(systemName: "link")
                    Text("Blockchain")
                }
                .tag(2)
            
            WalletView()
                .tabItem {
                    Image(systemName: "creditcard.fill")
                    Text("Wallet")
                }
                .tag(3)
            
            SettingsView()
                .tabItem {
                    Image(systemName: "gear")
                    Text("Settings")
                }
                .tag(4)
        }
        .alert("Error", isPresented: .constant(appState.errorMessage != nil)) {
            Button("OK") {
                appState.clearError()
            }
        } message: {
            Text(appState.errorMessage ?? "")
        }
        .overlay {
            if appState.isLoading {
                LoadingOverlay()
            }
        }
    }
}

/**
 * Loading overlay shown during async operations.
 */
struct LoadingOverlay: View {
    var body: some View {
        ZStack {
            Color.black.opacity(0.3)
                .ignoresSafeArea()
            
            VStack(spacing: 16) {
                ProgressView()
                    .scaleEffect(1.5)
                    .tint(.white)
                
                Text("Loading...")
                    .foregroundColor(.white)
                    .font(.headline)
            }
            .padding(32)
            .background(Color.black.opacity(0.7))
            .cornerRadius(12)
        }
    }
}

/**
 * Dashboard view showing overview and quick actions.
 */
struct DashboardView: View {
    @EnvironmentObject var apiClient: MobileAPIClient
    @State private var systemStatus: SystemStatus?
    @State private var refreshing = false
    
    var body: some View {
        NavigationView {
            ScrollView {
                LazyVStack(spacing: 16) {
                    // Network Status Card
                    NetworkStatusCard()
                    
                    // API Status Card
                    APIStatusCard(systemStatus: systemStatus)
                    
                    // Quick Actions
                    QuickActionsCard()
                    
                    // Cache Statistics
                    CacheStatsCard()
                    
                    // SDK Information
                    SDKInfoCard()
                }
                .padding()
            }
            .navigationTitle("WRFCoin Dashboard")
            .refreshable {
                await refreshData()
            }
            .task {
                await loadData()
            }
        }
    }
    
    private func loadData() async {
        // Load system status and other dashboard data
        // This would use the SDK to fetch real data
        print("Loading dashboard data...")
    }
    
    private func refreshData() async {
        refreshing = true
        await loadData()
        refreshing = false
    }
}

/**
 * Network status monitoring card.
 */
struct NetworkStatusCard: View {
    @EnvironmentObject var apiClient: MobileAPIClient
    
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: apiClient.isOnline ? "wifi" : "wifi.slash")
                    .foregroundColor(apiClient.isOnline ? .green : .red)
                
                Text("Network Status")
                    .font(.headline)
                
                Spacer()
                
                Text(apiClient.isOnline ? "Online" : "Offline")
                    .font(.caption)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(apiClient.isOnline ? Color.green.opacity(0.2) : Color.red.opacity(0.2))
                    .cornerRadius(8)
            }
            
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("Quality:")
                    Spacer()
                    Text(networkQualityText)
                        .foregroundColor(networkQualityColor)
                }
                
                if apiClient.isBackgroundSyncActive {
                    HStack {
                        Text("Background Sync:")
                        Spacer()
                        Text("Active")
                            .foregroundColor(.blue)
                    }
                }
            }
            .font(.caption)
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
    }
    
    private var networkQualityText: String {
        switch apiClient.networkQuality {
        case .poor: return "Poor"
        case .fair: return "Fair"
        case .good: return "Good"
        case .excellent: return "Excellent"
        }
    }
    
    private var networkQualityColor: Color {
        switch apiClient.networkQuality {
        case .poor: return .red
        case .fair: return .orange
        case .good: return .yellow
        case .excellent: return .green
        }
    }
}

/**
 * API connection status card.
 */
struct APIStatusCard: View {
    let systemStatus: SystemStatus?
    
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "server.rack")
                    .foregroundColor(.blue)
                
                Text("API Status")
                    .font(.headline)
                
                Spacer()
                
                Button("Test") {
                    testConnection()
                }
                .font(.caption)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.blue.opacity(0.2))
                .cornerRadius(8)
            }
            
            if let status = systemStatus {
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text("Status:")
                        Spacer()
                        Text(status.overall)
                            .foregroundColor(status.overall == "healthy" ? .green : .red)
                    }
                    
                    HStack {
                        Text("Response Time:")
                        Spacer()
                        Text("\\(status.responseTime ?? 0)ms")
                    }
                    
                    HStack {
                        Text("Version:")
                        Spacer()
                        Text(status.version ?? "Unknown")
                    }
                }
                .font(.caption)
            } else {
                Text("Loading...")
                    .font(.caption)
                    .foregroundColor(.gray)
            }
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
    }
    
    private func testConnection() {
        // Test API connection
        print("Testing API connection...")
    }
}

/**
 * Quick actions card for common operations.
 */
struct QuickActionsCard: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Quick Actions")
                .font(.headline)
            
            LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 2), spacing: 12) {
                QuickActionButton(
                    title: "Sync Weather",
                    icon: "cloud.sun.fill",
                    color: .blue
                ) {
                    syncWeatherData()
                }
                
                QuickActionButton(
                    title: "Check Wallet",
                    icon: "creditcard.fill",
                    color: .green
                ) {
                    checkWalletBalance()
                }
                
                QuickActionButton(
                    title: "Clear Cache",
                    icon: "trash.fill",
                    color: .red
                ) {
                    clearCache()
                }
                
                QuickActionButton(
                    title: "View Analytics",
                    icon: "chart.bar.fill",
                    color: .purple
                ) {
                    viewAnalytics()
                }
            }
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
    }
    
    private func syncWeatherData() {
        print("Syncing weather data...")
    }
    
    private func checkWalletBalance() {
        print("Checking wallet balance...")
    }
    
    private func clearCache() {
        MobileAPIClient.shared.clearCache()
        print("Cache cleared")
    }
    
    private func viewAnalytics() {
        print("Viewing analytics...")
    }
}

/**
 * Quick action button component.
 */
struct QuickActionButton: View {
    let title: String
    let icon: String
    let color: Color
    let action: () -> Void
    
    var body: some View {
        Button(action: action) {
            VStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.title2)
                    .foregroundColor(color)
                
                Text(title)
                    .font(.caption)
                    .foregroundColor(.primary)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 16)
            .background(Color(.systemBackground))
            .cornerRadius(8)
            .shadow(radius: 2)
        }
        .buttonStyle(PlainButtonStyle())
    }
}

/**
 * Cache statistics card.
 */
struct CacheStatsCard: View {
    @State private var cacheSize: Int64 = 0
    
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "externaldrive.fill")
                    .foregroundColor(.orange)
                
                Text("Cache Statistics")
                    .font(.headline)
                
                Spacer()
                
                Button("Refresh") {
                    updateCacheSize()
                }
                .font(.caption)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.orange.opacity(0.2))
                .cornerRadius(8)
            }
            
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("Cache Size:")
                    Spacer()
                    Text(formatBytes(cacheSize))
                }
                
                HStack {
                    Text("Status:")
                    Spacer()
                    Text(cacheSize > 0 ? "Active" : "Empty")
                        .foregroundColor(cacheSize > 0 ? .green : .gray)
                }
            }
            .font(.caption)
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
        .onAppear {
            updateCacheSize()
        }
    }
    
    private func updateCacheSize() {
        cacheSize = MobileAPIClient.shared.getCacheSize()
    }
    
    private func formatBytes(_ bytes: Int64) -> String {
        let formatter = ByteCountFormatter()
        formatter.allowedUnits = [.useBytes, .useKB, .useMB]
        formatter.countStyle = .file
        return formatter.string(fromByteCount: bytes)
    }
}

/**
 * SDK information card.
 */
struct SDKInfoCard: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Image(systemName: "info.circle.fill")
                    .foregroundColor(.blue)
                
                Text("SDK Information")
                    .font(.headline)
            }
            
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("Version:")
                    Spacer()
                    Text("1.0.0")
                }
                
                HStack {
                    Text("Platform:")
                    Spacer()
                    Text("iOS \\(UIDevice.current.systemVersion)")
                }
                
                HStack {
                    Text("Device:")
                    Spacer()
                    Text(UIDevice.current.model)
                }
            }
            .font(.caption)
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
    }
}

// MARK: - Supporting Types

struct SystemStatus {
    let overall: String
    let responseTime: Int?
    let version: String?
}

#Preview {
    ContentView()
        .environmentObject(AppState())
        .environmentObject(MobileAPIClient.shared)
}