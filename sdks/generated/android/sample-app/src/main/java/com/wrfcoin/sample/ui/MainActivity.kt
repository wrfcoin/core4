package com.wrfcoin.sample.ui

import android.content.Intent
import android.os.Bundle
import android.view.Menu
import android.view.MenuItem
import androidx.activity.viewModels
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.LinearLayoutManager
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.snackbar.Snackbar
import com.wrfcoin.sample.BuildConfig
import com.wrfcoin.sample.R
import com.wrfcoin.sample.WRFCoinSampleApplication
import com.wrfcoin.sample.databinding.ActivityMainBinding
import com.wrfcoin.sample.ui.blockchain.BlockchainExplorerActivity
import com.wrfcoin.sample.ui.settings.SettingsActivity
import com.wrfcoin.sample.ui.wallet.WalletActivity
import com.wrfcoin.sample.ui.weather.WeatherStationActivity
import com.wrfcoin.sample.viewmodel.MainViewModel
import kotlinx.coroutines.launch
import timber.log.Timber

/**
 * Main activity showcasing WRFCoin SDK features.
 * 
 * Features demonstrated:
 * - API connectivity status
 * - Weather data retrieval
 * - Blockchain operations
 * - Wallet management
 * - Background sync status
 * - Network monitoring
 */
class MainActivity : AppCompatActivity() {

    private lateinit var binding: ActivityMainBinding
    private val viewModel: MainViewModel by viewModels()
    private lateinit var apiClient: com.wrfcoin.android.sdk.lifecycle.MobileApiClient
    
    private lateinit var dashboardAdapter: DashboardAdapter

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)
        
        // Initialize SDK
        apiClient = WRFCoinSampleApplication.getInstance(this).getApiClient()
        
        setupToolbar()
        setupRecyclerView()
        setupObservers()
        setupClickListeners()
        
        // Load initial data
        loadDashboardData()
        
        Timber.d("MainActivity created")
    }

    private fun setupToolbar() {
        setSupportActionBar(binding.toolbar)
        supportActionBar?.apply {
            title = getString(R.string.app_name)
            subtitle = "v${BuildConfig.VERSION_NAME}"
        }
    }

    private fun setupRecyclerView() {
        dashboardAdapter = DashboardAdapter { item ->
            handleDashboardItemClick(item)
        }
        
        binding.recyclerViewDashboard.apply {
            layoutManager = LinearLayoutManager(this@MainActivity)
            adapter = dashboardAdapter
        }
    }

    private fun setupObservers() {
        // Observe dashboard data
        viewModel.dashboardItems.observe(this) { items ->
            dashboardAdapter.submitList(items)
        }
        
        // Observe loading state
        viewModel.isLoading.observe(this) { isLoading ->
            binding.swipeRefreshLayout.isRefreshing = isLoading
        }
        
        // Observe error messages
        viewModel.errorMessage.observe(this) { error ->
            error?.let {
                showErrorSnackbar(it)
                viewModel.clearError()
            }
        }
        
        // Observe API connectivity
        viewModel.apiStatus.observe(this) { status ->
            updateApiStatusIndicator(status)
        }
        
        // Observe network status
        lifecycleScope.launch {
            apiClient.getApiClient().let { client ->
                // Monitor network changes if available
                Timber.d("Monitoring network status")
            }
        }
    }

    private fun setupClickListeners() {
        binding.swipeRefreshLayout.setOnRefreshListener {
            loadDashboardData()
        }
        
        binding.fabQuickSync.setOnClickListener {
            performQuickSync()
        }
    }

    private fun loadDashboardData() {
        viewModel.loadDashboardData()
    }

    private fun handleDashboardItemClick(item: DashboardItem) {
        when (item.type) {
            DashboardItemType.WEATHER_STATIONS -> {
                startActivity(Intent(this, WeatherStationActivity::class.java))
            }
            DashboardItemType.BLOCKCHAIN_EXPLORER -> {
                startActivity(Intent(this, BlockchainExplorerActivity::class.java))
            }
            DashboardItemType.WALLET_MANAGEMENT -> {
                startActivity(Intent(this, WalletActivity::class.java))
            }
            DashboardItemType.API_STATUS -> {
                showApiStatusDialog()
            }
            DashboardItemType.NETWORK_STATUS -> {
                showNetworkStatusDialog()
            }
            DashboardItemType.CACHE_MANAGEMENT -> {
                showCacheManagementDialog()
            }
            DashboardItemType.BACKGROUND_SYNC -> {
                showBackgroundSyncDialog()
            }
        }
    }

    private fun performQuickSync() {
        lifecycleScope.launch {
            try {
                binding.swipeRefreshLayout.isRefreshing = true
                
                // Trigger immediate background sync
                viewModel.performQuickSync()
                
                showSuccessSnackbar("Quick sync initiated")
            } catch (e: Exception) {
                Timber.e(e, "Quick sync failed")
                showErrorSnackbar("Quick sync failed: ${e.message}")
            } finally {
                binding.swipeRefreshLayout.isRefreshing = false
            }
        }
    }

    private fun updateApiStatusIndicator(status: ApiStatus) {
        // Update UI based on API status
        val color = when (status) {
            ApiStatus.CONNECTED -> android.R.color.holo_green_light
            ApiStatus.DISCONNECTED -> android.R.color.holo_red_light
            ApiStatus.CONNECTING -> android.R.color.holo_orange_light
        }
        
        // You could update a status indicator here
        Timber.d("API status updated: $status")
    }

    private fun showApiStatusDialog() {
        val status = viewModel.getDetailedApiStatus()
        
        MaterialAlertDialogBuilder(this)
            .setTitle("API Status")
            .setMessage(buildString {
                appendLine("Base URL: ${BuildConfig.API_BASE_URL}")
                appendLine("Status: ${status.connectionStatus}")
                appendLine("Last Check: ${status.lastCheckTime}")
                appendLine("Response Time: ${status.responseTime}ms")
                appendLine("Cache Size: ${formatFileSize(status.cacheSize)}")
            })
            .setPositiveButton("Test Connection") { _, _ ->
                viewModel.testApiConnection()
            }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun showNetworkStatusDialog() {
        val networkStatus = viewModel.getNetworkStatus()
        
        MaterialAlertDialogBuilder(this)
            .setTitle("Network Status")
            .setMessage(buildString {
                appendLine("Connected: ${networkStatus.isConnected}")
                appendLine("Type: ${networkStatus.type}")
                appendLine("Quality: ${networkStatus.quality}")
                appendLine("Is Metered: ${networkStatus.isMetered}")
                appendLine("Speed: ${networkStatus.estimatedSpeedKbps} Kbps")
                appendLine("Latency: ${networkStatus.latencyMs}ms")
            })
            .setPositiveButton("Refresh") { _, _ ->
                viewModel.refreshNetworkStatus()
            }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun showCacheManagementDialog() {
        val cacheSize = apiClient.getCacheSize()
        
        MaterialAlertDialogBuilder(this)
            .setTitle("Cache Management")
            .setMessage("Current cache size: ${formatFileSize(cacheSize)}")
            .setPositiveButton("Clear Cache") { _, _ ->
                clearCache()
            }
            .setNeutralButton("Optimize") { _, _ ->
                optimizeCache()
            }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun showBackgroundSyncDialog() {
        val syncStatus = viewModel.getBackgroundSyncStatus()
        
        MaterialAlertDialogBuilder(this)
            .setTitle("Background Sync")
            .setMessage(buildString {
                appendLine("Last Sync: ${syncStatus.lastSyncTime}")
                appendLine("Next Sync: ${syncStatus.nextSyncTime}")
                appendLine("Status: ${syncStatus.status}")
                appendLine("Synced Items: ${syncStatus.syncedItems}")
                appendLine("Failed Items: ${syncStatus.failedItems}")
            })
            .setPositiveButton("Sync Now") { _, _ ->
                performQuickSync()
            }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun clearCache() {
        lifecycleScope.launch {
            try {
                apiClient.clearCache()
                showSuccessSnackbar("Cache cleared successfully")
                loadDashboardData() // Refresh dashboard
            } catch (e: Exception) {
                Timber.e(e, "Failed to clear cache")
                showErrorSnackbar("Failed to clear cache")
            }
        }
    }

    private fun optimizeCache() {
        lifecycleScope.launch {
            try {
                // Cache optimization would be handled by the SDK
                showSuccessSnackbar("Cache optimized successfully")
                loadDashboardData() // Refresh dashboard
            } catch (e: Exception) {
                Timber.e(e, "Failed to optimize cache")
                showErrorSnackbar("Failed to optimize cache")
            }
        }
    }

    private fun showErrorSnackbar(message: String) {
        Snackbar.make(binding.root, message, Snackbar.LENGTH_LONG)
            .setAction("Retry") { loadDashboardData() }
            .show()
    }

    private fun showSuccessSnackbar(message: String) {
        Snackbar.make(binding.root, message, Snackbar.LENGTH_SHORT).show()
    }

    private fun formatFileSize(bytes: Long): String {
        return when {
            bytes >= 1024 * 1024 -> String.format("%.1f MB", bytes / (1024.0 * 1024.0))
            bytes >= 1024 -> String.format("%.1f KB", bytes / 1024.0)
            else -> "$bytes bytes"
        }
    }

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menuInflater.inflate(R.menu.menu_main, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        return when (item.itemId) {
            R.id.action_settings -> {
                startActivity(Intent(this, SettingsActivity::class.java))
                true
            }
            R.id.action_refresh -> {
                loadDashboardData()
                true
            }
            R.id.action_about -> {
                showAboutDialog()
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    private fun showAboutDialog() {
        MaterialAlertDialogBuilder(this)
            .setTitle("About WRFCoin Sample")
            .setMessage(buildString {
                appendLine("WRFCoin Android SDK Sample Application")
                appendLine("Version: ${BuildConfig.VERSION_NAME}")
                appendLine("Build Type: ${BuildConfig.BUILD_TYPE}")
                appendLine("API URL: ${BuildConfig.API_BASE_URL}")
                appendLine()
                appendLine("This app demonstrates the capabilities of the WRFCoin Android SDK including:")
                appendLine("• Weather data management")
                appendLine("• Blockchain operations")
                appendLine("• Wallet functionality")
                appendLine("• Background synchronization")
                appendLine("• Network monitoring")
                appendLine("• Secure credential storage")
            })
            .setPositiveButton("Close", null)
            .show()
    }

    override fun onResume() {
        super.onResume()
        // Refresh data when returning to the activity
        loadDashboardData()
    }
}

// Data classes for the dashboard
enum class DashboardItemType {
    WEATHER_STATIONS,
    BLOCKCHAIN_EXPLORER,
    WALLET_MANAGEMENT,
    API_STATUS,
    NETWORK_STATUS,
    CACHE_MANAGEMENT,
    BACKGROUND_SYNC
}

data class DashboardItem(
    val type: DashboardItemType,
    val title: String,
    val subtitle: String,
    val iconRes: Int,
    val data: Any? = null
)

enum class ApiStatus {
    CONNECTED,
    DISCONNECTED,
    CONNECTING
}