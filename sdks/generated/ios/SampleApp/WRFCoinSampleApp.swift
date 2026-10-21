import SwiftUI
import WRFCoinSDK

/**
 * Main entry point for the WRFCoin iOS Sample Application.
 *
 * This app demonstrates the comprehensive capabilities of the WRFCoin iOS SDK including:
 * - Weather data management and visualization
 * - Blockchain operations and wallet functionality
 * - Real-time market data and trading features
 * - Secure credential management with biometric authentication
 * - Background synchronization and offline support
 * - Network monitoring and adaptive behavior
 * - Advanced caching strategies
 */
@main
struct WRFCoinSampleApp: App {
    
    @StateObject private var appState = AppState()
    @StateObject private var apiClient = MobileAPIClient.shared
    
    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(appState)
                .environmentObject(apiClient)
                .onAppear {
                    setupSDK()
                }
                .onReceive(NotificationCenter.default.publisher(for: UIApplication.didBecomeActiveNotification)) { _ in
                    appState.refreshData()
                }
        }
    }
    
    private func setupSDK() {
        // Configure SDK with API key (in production, load from secure storage)
        #if DEBUG
        // Use test API key for development
        apiClient.setAPIKey("test_api_key_development")
        #else
        // Load API key from Keychain in production
        if let apiKey = SecureCredentialStorage().getAPIKey() {
            apiClient.setAPIKey(apiKey)
        }
        #endif
        
        print("WRFCoin SDK initialized successfully")
    }
}

/**
 * Global application state management.
 */
class AppState: ObservableObject {
    @Published var selectedTab: Int = 0
    @Published var isLoading: Bool = false
    @Published var errorMessage: String?
    @Published var networkStatus: String = "Unknown"
    
    func refreshData() {
        Task {
            await MainActor.run {
                isLoading = true
            }
            
            // Perform data refresh operations
            try? await Task.sleep(nanoseconds: 1_000_000_000) // 1 second delay
            
            await MainActor.run {
                isLoading = false
            }
        }
    }
    
    func showError(_ message: String) {
        errorMessage = message
    }
    
    func clearError() {
        errorMessage = nil
    }
}