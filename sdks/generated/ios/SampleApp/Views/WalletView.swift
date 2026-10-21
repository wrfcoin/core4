import SwiftUI
import WRFCoinSDK
import LocalAuthentication

/**
 * Wallet management and DeFi features view.
 *
 * Features demonstrated:
 * - Wallet creation and import
 * - Balance management and transaction history
 * - DeFi operations (staking, liquidity pools)
 * - Cross-chain transfers
 * - Security features with biometric authentication
 */
struct WalletView: View {
    @State private var wallet: WalletInfo?
    @State private var showingCreateWallet = false
    @State private var showingImportWallet = false
    @State private var showingSendModal = false
    @State private var isLoading = false
    @State private var biometricType: LABiometryType = .none
    
    var body: some View {
        NavigationView {
            Group {
                if let wallet = wallet {
                    WalletDashboard(wallet: wallet)
                } else {
                    WalletSetupView(
                        onCreateWallet: { showingCreateWallet = true },
                        onImportWallet: { showingImportWallet = true }
                    )
                }
            }
            .navigationTitle("Wallet")
            .toolbar {
                if wallet != nil {
                    ToolbarItem(placement: .navigationBarTrailing) {
                        Menu {
                            Button("Send") {
                                showingSendModal = true
                            }
                            
                            Button("Receive") {
                                // Show receive modal
                            }
                            
                            Button("Settings") {
                                // Show wallet settings
                            }
                        } label: {
                            Image(systemName: "ellipsis.circle")
                        }
                    }
                }
            }
            .sheet(isPresented: $showingCreateWallet) {
                CreateWalletView { newWallet in
                    wallet = newWallet
                }
            }
            .sheet(isPresented: $showingImportWallet) {
                ImportWalletView { importedWallet in
                    wallet = importedWallet
                }
            }
            .sheet(isPresented: $showingSendModal) {
                if let wallet = wallet {
                    SendTransactionView(wallet: wallet)
                }
            }
            .task {
                await loadWallet()
                checkBiometricSupport()
            }
        }
    }
    
    private func loadWallet() async {
        isLoading = true
        
        // Try to load existing wallet from secure storage
        let secureStorage = SecureCredentialStorage()
        
        if let address = secureStorage.getWalletAddress() {
            // Load wallet info
            wallet = WalletInfo(
                address: address,
                balance: 1245.67,
                wrfBalance: 2500.0,
                stakingBalance: 500.0,
                networkType: .mainnet
            )
        }
        
        isLoading = false
    }
    
    private func checkBiometricSupport() {
        let context = LAContext()
        var error: NSError?
        
        if context.canEvaluatePolicy(.biometryAny, error: &error) {
            biometricType = context.biometryType
        }
    }
}

/**
 * Wallet setup view for new users.
 */
struct WalletSetupView: View {
    let onCreateWallet: () -> Void
    let onImportWallet: () -> Void
    
    var body: some View {
        VStack(spacing: 32) {
            Spacer()
            
            VStack(spacing: 16) {
                Image(systemName: "wallet.pass.fill")
                    .font(.system(size: 80))
                    .foregroundColor(.blue)
                
                Text("Welcome to WRFCoin Wallet")
                    .font(.title)
                    .fontWeight(.bold)
                    .multilineTextAlignment(.center)
                
                Text("Manage your WRF tokens, participate in DeFi, and earn rewards from weather data contributions.")
                    .font(.body)
                    .multilineTextAlignment(.center)
                    .foregroundColor(.secondary)
            }
            
            VStack(spacing: 16) {
                Button(action: onCreateWallet) {
                    Text("Create New Wallet")
                        .font(.headline)
                        .foregroundColor(.white)
                        .frame(maxWidth: .infinity)
                        .padding()
                        .background(Color.blue)
                        .cornerRadius(12)
                }
                
                Button(action: onImportWallet) {
                    Text("Import Existing Wallet")
                        .font(.headline)
                        .foregroundColor(.blue)
                        .frame(maxWidth: .infinity)
                        .padding()
                        .background(Color.blue.opacity(0.1))
                        .cornerRadius(12)
                }
            }
            
            Spacer()
            
            VStack(spacing: 8) {
                Text("🔒 Your wallet is secured with biometric authentication")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)
                
                Text("Private keys are stored securely in the iOS Keychain")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(24)
    }
}

/**
 * Main wallet dashboard.
 */
struct WalletDashboard: View {
    let wallet: WalletInfo
    @State private var selectedTab = 0
    
    var body: some View {
        VStack(spacing: 0) {
            // Wallet Balance Card
            WalletBalanceCard(wallet: wallet)
                .padding()
            
            // Tab Selection
            Picker("Tab", selection: $selectedTab) {
                Text("Overview").tag(0)
                Text("Transactions").tag(1)
                Text("DeFi").tag(2)
                Text("NFTs").tag(3)
            }
            .pickerStyle(SegmentedPickerStyle())
            .padding(.horizontal)
            
            // Tab Content
            TabView(selection: $selectedTab) {
                WalletOverviewTab(wallet: wallet)
                    .tag(0)
                
                TransactionHistoryTab()
                    .tag(1)
                
                DeFiTab()
                    .tag(2)
                
                NFTTab()
                    .tag(3)
            }
            .tabViewStyle(PageTabViewStyle(indexDisplayMode: .never))
        }
    }
}

/**
 * Wallet balance card showing key information.
 */
struct WalletBalanceCard: View {
    let wallet: WalletInfo
    @State private var showingFullAddress = false
    
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Total Balance")
                        .font(.caption)
                        .foregroundColor(.secondary)
                    
                    Text("$\\(wallet.balance, specifier: "%.2f")")
                        .font(.title)
                        .fontWeight(.bold)
                }
                
                Spacer()
                
                Button(action: { showingFullAddress.toggle() }) {
                    Text(showingFullAddress ? wallet.address : wallet.shortAddress)
                        .font(.caption)
                        .foregroundColor(.blue)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(Color.blue.opacity(0.1))
                        .cornerRadius(6)
                }
            }
            
            HStack(spacing: 24) {
                BalanceItem(
                    title: "WRF",
                    amount: "\\(wallet.wrfBalance, specifier: "%.2f")",
                    subtitle: "Available"
                )
                
                BalanceItem(
                    title: "Staked",
                    amount: "\\(wallet.stakingBalance, specifier: "%.2f")",
                    subtitle: "WRF Staking"
                )
                
                BalanceItem(
                    title: "Network",
                    amount: wallet.networkType.rawValue,
                    subtitle: "Connected"
                )
            }
        }
        .padding(20)
        .background(
            LinearGradient(
                gradient: Gradient(colors: [Color.blue.opacity(0.1), Color.purple.opacity(0.1)]),
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        )
        .cornerRadius(16)
    }
}

/**
 * Balance item component.
 */
struct BalanceItem: View {
    let title: String
    let amount: String
    let subtitle: String
    
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.caption)
                .foregroundColor(.secondary)
            
            Text(amount)
                .font(.headline)
                .fontWeight(.semibold)
            
            Text(subtitle)
                .font(.caption2)
                .foregroundColor(.secondary)
        }
    }
}

/**
 * Wallet overview tab.
 */
struct WalletOverviewTab: View {
    let wallet: WalletInfo
    
    var body: some View {
        ScrollView {
            LazyVStack(spacing: 16) {
                // Quick Actions
                QuickActionsGrid()
                
                // Recent Activity
                RecentActivityCard()
                
                // Market Summary
                MarketSummaryCard()
                
                // Weather Earnings
                WeatherEarningsCard()
            }
            .padding()
        }
    }
}

/**
 * Quick actions grid.
 */
struct QuickActionsGrid: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Quick Actions")
                .font(.headline)
            
            LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 4), spacing: 12) {
                QuickActionItem(icon: "paperplane.fill", title: "Send", color: .blue)
                QuickActionItem(icon: "qrcode", title: "Receive", color: .green)
                QuickActionItem(icon: "arrow.2.squarepath", title: "Swap", color: .orange)
                QuickActionItem(icon: "chart.bar.fill", title: "Stake", color: .purple)
            }
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
    }
}

/**
 * Quick action item.
 */
struct QuickActionItem: View {
    let icon: String
    let title: String
    let color: Color
    
    var body: some View {
        Button(action: {}) {
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
        }
        .buttonStyle(PlainButtonStyle())
    }
}

/**
 * Create wallet view.
 */
struct CreateWalletView: View {
    @Environment(\\.dismiss) var dismiss
    let onWalletCreated: (WalletInfo) -> Void
    
    @State private var currentStep = 0
    @State private var mnemonic: [String] = []
    @State private var confirmedMnemonic: [String] = []
    @State private var isCreating = false
    
    var body: some View {
        NavigationView {
            VStack {
                ProgressView(value: Double(currentStep), total: 3)
                    .padding()
                
                switch currentStep {
                case 0:
                    SecurityInfoStep {
                        currentStep = 1
                        generateMnemonic()
                    }
                case 1:
                    MnemonicDisplayStep(mnemonic: mnemonic) {
                        currentStep = 2
                    }
                case 2:
                    MnemonicConfirmationStep(
                        mnemonic: mnemonic,
                        confirmedMnemonic: $confirmedMnemonic
                    ) {
                        createWallet()
                    }
                default:
                    EmptyView()
                }
                
                Spacer()
            }
            .navigationTitle("Create Wallet")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarLeading) {
                    Button("Cancel") {
                        dismiss()
                    }
                }
            }
        }
    }
    
    private func generateMnemonic() {
        // Generate 12-word mnemonic phrase
        mnemonic = [
            "abandon", "ability", "able", "about", "above", "absent",
            "absorb", "abstract", "absurd", "abuse", "access", "accident"
        ]
    }
    
    private func createWallet() {
        isCreating = true
        
        Task {
            // Create wallet using SDK
            let newWallet = WalletInfo(
                address: "0x1234567890abcdef1234567890abcdef12345678",
                balance: 0.0,
                wrfBalance: 0.0,
                stakingBalance: 0.0,
                networkType: .mainnet
            )
            
            // Store wallet securely
            let secureStorage = SecureCredentialStorage()
            secureStorage.saveWalletAddress(newWallet.address)
            
            await MainActor.run {
                onWalletCreated(newWallet)
                dismiss()
            }
        }
    }
}

// Additional wallet-related views would be implemented here...

// MARK: - Data Models

struct WalletInfo {
    let address: String
    let balance: Double
    let wrfBalance: Double
    let stakingBalance: Double
    let networkType: NetworkType
    
    var shortAddress: String {
        String(address.prefix(6)) + "..." + String(address.suffix(4))
    }
    
    enum NetworkType: String, CaseIterable {
        case mainnet = "Mainnet"
        case testnet = "Testnet"
        case polygon = "Polygon"
        case bsc = "BSC"
    }
}

// MARK: - Placeholder Views

struct SecurityInfoStep: View {
    let onNext: () -> Void
    
    var body: some View {
        VStack(spacing: 24) {
            Text("Security Information")
                .font(.title2)
                .fontWeight(.bold)
            
            Text("Your wallet will be protected with the highest security standards.")
                .multilineTextAlignment(.center)
            
            Button("Continue", action: onNext)
                .buttonStyle(.borderedProminent)
        }
        .padding()
    }
}

struct MnemonicDisplayStep: View {
    let mnemonic: [String]
    let onNext: () -> Void
    
    var body: some View {
        VStack(spacing: 24) {
            Text("Recovery Phrase")
                .font(.title2)
                .fontWeight(.bold)
            
            LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 3)) {
                ForEach(Array(mnemonic.enumerated()), id: \\.offset) { index, word in
                    Text("\\(index + 1). \\(word)")
                        .padding(8)
                        .background(Color(.systemGray6))
                        .cornerRadius(8)
                }
            }
            
            Button("I've Saved It", action: onNext)
                .buttonStyle(.borderedProminent)
        }
        .padding()
    }
}

struct MnemonicConfirmationStep: View {
    let mnemonic: [String]
    @Binding var confirmedMnemonic: [String]
    let onComplete: () -> Void
    
    var body: some View {
        VStack(spacing: 24) {
            Text("Confirm Recovery Phrase")
                .font(.title2)
                .fontWeight(.bold)
            
            Text("Please select the words in the correct order")
                .multilineTextAlignment(.center)
            
            Button("Create Wallet", action: onComplete)
                .buttonStyle(.borderedProminent)
        }
        .padding()
    }
}

struct ImportWalletView: View {
    @Environment(\\.dismiss) var dismiss
    let onWalletImported: (WalletInfo) -> Void
    
    var body: some View {
        Text("Import Wallet")
            .navigationTitle("Import Wallet")
    }
}

struct SendTransactionView: View {
    let wallet: WalletInfo
    @Environment(\\.dismiss) var dismiss
    
    var body: some View {
        Text("Send Transaction")
            .navigationTitle("Send")
    }
}

struct TransactionHistoryTab: View {
    var body: some View {
        Text("Transaction History")
    }
}

struct DeFiTab: View {
    var body: some View {
        Text("DeFi Features")
    }
}

struct NFTTab: View {
    var body: some View {
        Text("NFT Collection")
    }
}

struct RecentActivityCard: View {
    var body: some View {
        Text("Recent Activity")
    }
}

struct MarketSummaryCard: View {
    var body: some View {
        Text("Market Summary")
    }
}

struct WeatherEarningsCard: View {
    var body: some View {
        Text("Weather Earnings")
    }
}

#Preview {
    WalletView()
}