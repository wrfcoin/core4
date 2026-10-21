package com.wrfcoin.android.sdk.utils

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.lifecycle.LiveData
import androidx.lifecycle.MutableLiveData
import kotlinx.coroutines.*
import okhttp3.Interceptor
import okhttp3.Response
import timber.log.Timber
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Network connectivity monitor with real-time status tracking and quality assessment.
 * 
 * Features:
 * - Real-time connectivity monitoring
 * - Network quality assessment (wifi, cellular, etc.)
 * - Connection speed estimation
 * - Retry strategies based on network conditions
 * - Battery-optimized monitoring
 */
class NetworkMonitor(private val context: Context) {

    companion object {
        private const val PING_TIMEOUT_MS = 5000
        private const val PING_HOST = "8.8.8.8"
        private const val PING_PORT = 53
        private const val SPEED_TEST_INTERVAL_MS = 30000L // 30 seconds
    }

    enum class NetworkType {
        NONE,
        WIFI,
        CELLULAR,
        ETHERNET,
        VPN,
        UNKNOWN
    }

    enum class NetworkQuality {
        POOR,      // < 1 Mbps
        FAIR,      // 1-5 Mbps
        GOOD,      // 5-20 Mbps
        EXCELLENT  // > 20 Mbps
    }

    data class NetworkStatus(
        val isConnected: Boolean,
        val type: NetworkType,
        val quality: NetworkQuality,
        val isMetered: Boolean,
        val estimatedSpeedKbps: Long,
        val latencyMs: Long
    )

    private val connectivityManager = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    private val coroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    
    private val _networkStatus = MutableLiveData<NetworkStatus>()
    val networkStatus: LiveData<NetworkStatus> = _networkStatus
    
    private val isMonitoring = AtomicBoolean(false)
    private val lastSpeedTest = AtomicLong(0)
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private var speedTestJob: Job? = null

    init {
        // Initialize with current network status
        updateNetworkStatus()
    }

    /**
     * Starts monitoring network connectivity changes.
     */
    fun startMonitoring() {
        if (isMonitoring.compareAndSet(false, true)) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                registerNetworkCallback()
            }
            startSpeedTesting()
            Timber.d("Network monitoring started")
        }
    }

    /**
     * Stops monitoring network connectivity changes.
     */
    fun stopMonitoring() {
        if (isMonitoring.compareAndSet(true, false)) {
            unregisterNetworkCallback()
            stopSpeedTesting()
            Timber.d("Network monitoring stopped")
        }
    }

    /**
     * Registers network callback for API 24+ devices.
     */
    @RequiresApi(Build.VERSION_CODES.N)
    private fun registerNetworkCallback() {
        val networkRequest = NetworkRequest.Builder()
            .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .addCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
            .build()

        networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                super.onAvailable(network)
                Timber.d("Network available: $network")
                updateNetworkStatus()
            }

            override fun onLost(network: Network) {
                super.onLost(network)
                Timber.d("Network lost: $network")
                updateNetworkStatus()
            }

            override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) {
                super.onCapabilitiesChanged(network, networkCapabilities)
                Timber.d("Network capabilities changed: $network")
                updateNetworkStatus()
            }
        }

        try {
            connectivityManager.registerNetworkCallback(networkRequest, networkCallback!!)
        } catch (e: Exception) {
            Timber.e(e, "Failed to register network callback")
        }
    }

    /**
     * Unregisters network callback.
     */
    private fun unregisterNetworkCallback() {
        networkCallback?.let { callback ->
            try {
                connectivityManager.unregisterNetworkCallback(callback)
            } catch (e: Exception) {
                Timber.e(e, "Failed to unregister network callback")
            }
        }
        networkCallback = null
    }

    /**
     * Starts periodic speed testing.
     */
    private fun startSpeedTesting() {
        speedTestJob = coroutineScope.launch {
            while (isActive && isMonitoring.get()) {
                try {
                    if (isConnected()) {
                        performSpeedTest()
                    }
                    delay(SPEED_TEST_INTERVAL_MS)
                } catch (e: CancellationException) {
                    break
                } catch (e: Exception) {
                    Timber.e(e, "Speed test error")
                    delay(SPEED_TEST_INTERVAL_MS)
                }
            }
        }
    }

    /**
     * Stops speed testing.
     */
    private fun stopSpeedTesting() {
        speedTestJob?.cancel()
        speedTestJob = null
    }

    /**
     * Updates the current network status.
     */
    private fun updateNetworkStatus() {
        coroutineScope.launch {
            try {
                val isConnected = isConnected()
                val type = getCurrentNetworkType()
                val isMetered = isConnectionMetered()
                val latency = if (isConnected) measureLatency() else 0L
                val speed = getEstimatedSpeed()
                val quality = assessNetworkQuality(speed)

                val status = NetworkStatus(
                    isConnected = isConnected,
                    type = type,
                    quality = quality,
                    isMetered = isMetered,
                    estimatedSpeedKbps = speed,
                    latencyMs = latency
                )

                _networkStatus.postValue(status)
                Timber.d("Network status updated: $status")
            } catch (e: Exception) {
                Timber.e(e, "Failed to update network status")
            }
        }
    }

    /**
     * Checks if device is currently connected to the internet.
     */
    fun isConnected(): Boolean {
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                val network = connectivityManager.activeNetwork ?: return false
                val capabilities = connectivityManager.getNetworkCapabilities(network) ?: return false
                capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
            } else {
                @Suppress("DEPRECATION")
                val networkInfo = connectivityManager.activeNetworkInfo
                networkInfo?.isConnected == true
            }
        } catch (e: Exception) {
            Timber.e(e, "Failed to check network connectivity")
            false
        }
    }

    /**
     * Gets the current network type.
     */
    private fun getCurrentNetworkType(): NetworkType {
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                val network = connectivityManager.activeNetwork ?: return NetworkType.NONE
                val capabilities = connectivityManager.getNetworkCapabilities(network) ?: return NetworkType.NONE
                
                when {
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> NetworkType.WIFI
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> NetworkType.CELLULAR
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> NetworkType.ETHERNET
                    capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> NetworkType.VPN
                    else -> NetworkType.UNKNOWN
                }
            } else {
                @Suppress("DEPRECATION")
                val networkInfo = connectivityManager.activeNetworkInfo ?: return NetworkType.NONE
                when (networkInfo.type) {
                    ConnectivityManager.TYPE_WIFI -> NetworkType.WIFI
                    ConnectivityManager.TYPE_MOBILE -> NetworkType.CELLULAR
                    ConnectivityManager.TYPE_ETHERNET -> NetworkType.ETHERNET
                    ConnectivityManager.TYPE_VPN -> NetworkType.VPN
                    else -> NetworkType.UNKNOWN
                }
            }
        } catch (e: Exception) {
            Timber.e(e, "Failed to get network type")
            NetworkType.UNKNOWN
        }
    }

    /**
     * Checks if the current connection is metered.
     */
    private fun isConnectionMetered(): Boolean {
        return try {
            connectivityManager.isActiveNetworkMetered
        } catch (e: Exception) {
            Timber.e(e, "Failed to check if connection is metered")
            false
        }
    }

    /**
     * Measures network latency by pinging a reliable server.
     */
    private suspend fun measureLatency(): Long = withContext(Dispatchers.IO) {
        try {
            val startTime = System.currentTimeMillis()
            Socket().use { socket ->
                socket.connect(InetSocketAddress(PING_HOST, PING_PORT), PING_TIMEOUT_MS)
            }
            System.currentTimeMillis() - startTime
        } catch (e: Exception) {
            Timber.w(e, "Failed to measure latency")
            0L
        }
    }

    /**
     * Performs a speed test to estimate connection speed.
     */
    private suspend fun performSpeedTest() {
        if (System.currentTimeMillis() - lastSpeedTest.get() < SPEED_TEST_INTERVAL_MS) {
            return
        }
        
        try {
            // Simplified speed test - in production, this would download a small file
            // and measure the transfer speed
            lastSpeedTest.set(System.currentTimeMillis())
            Timber.d("Speed test performed")
        } catch (e: Exception) {
            Timber.e(e, "Speed test failed")
        }
    }

    /**
     * Gets estimated connection speed in Kbps.
     */
    private fun getEstimatedSpeed(): Long {
        // Placeholder implementation - real implementation would use actual speed test results
        return when (getCurrentNetworkType()) {
            NetworkType.WIFI -> 50000L // 50 Mbps
            NetworkType.CELLULAR -> 10000L // 10 Mbps
            NetworkType.ETHERNET -> 100000L // 100 Mbps
            else -> 1000L // 1 Mbps fallback
        }
    }

    /**
     * Assesses network quality based on speed.
     */
    private fun assessNetworkQuality(speedKbps: Long): NetworkQuality {
        return when {
            speedKbps < 1000 -> NetworkQuality.POOR // < 1 Mbps
            speedKbps < 5000 -> NetworkQuality.FAIR // 1-5 Mbps
            speedKbps < 20000 -> NetworkQuality.GOOD // 5-20 Mbps
            else -> NetworkQuality.EXCELLENT // > 20 Mbps
        }
    }

    /**
     * Creates an interceptor for network monitoring.
     */
    fun createNetworkInterceptor(): Interceptor {
        return Interceptor { chain ->
            val request = chain.request()
            
            if (!isConnected()) {
                throw IOException("No internet connection available")
            }
            
            val startTime = System.currentTimeMillis()
            
            try {
                val response = chain.proceed(request)
                val duration = System.currentTimeMillis() - startTime
                
                // Log network performance metrics
                if (duration > 5000) { // Log slow requests
                    Timber.w("Slow network request: ${request.url} took ${duration}ms")
                }
                
                response
            } catch (e: IOException) {
                val duration = System.currentTimeMillis() - startTime
                Timber.e(e, "Network error for ${request.url} after ${duration}ms")
                throw e
            }
        }
    }

    /**
     * Gets the current network status.
     */
    fun getCurrentStatus(): NetworkStatus? {
        return _networkStatus.value
    }

    /**
     * Checks if the current network is suitable for large downloads.
     */
    fun isSuitableForLargeDownloads(): Boolean {
        val status = getCurrentStatus() ?: return false
        return status.isConnected && 
               !status.isMetered && 
               status.quality in listOf(NetworkQuality.GOOD, NetworkQuality.EXCELLENT)
    }

    /**
     * Gets recommended retry delay based on network quality.
     */
    fun getRecommendedRetryDelay(): Long {
        val status = getCurrentStatus()
        return when (status?.quality) {
            NetworkQuality.POOR -> 10000L // 10 seconds
            NetworkQuality.FAIR -> 5000L  // 5 seconds
            NetworkQuality.GOOD -> 2000L  // 2 seconds
            NetworkQuality.EXCELLENT -> 1000L // 1 second
            null -> 5000L // Default
        }
    }

    /**
     * Cleanup resources.
     */
    fun cleanup() {
        stopMonitoring()
        coroutineScope.cancel()
        Timber.d("NetworkMonitor cleaned up")
    }
}