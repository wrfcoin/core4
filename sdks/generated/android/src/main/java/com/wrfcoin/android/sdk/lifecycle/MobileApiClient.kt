package com.wrfcoin.android.sdk.lifecycle

import android.content.Context
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.wrfcoin.android.sdk.ApiClient
import com.wrfcoin.android.sdk.BuildConfig
import com.wrfcoin.android.sdk.cache.ApiCacheManager
import com.wrfcoin.android.sdk.security.CertificatePinningInterceptor
import com.wrfcoin.android.sdk.security.SecureCredentialStorage
import com.wrfcoin.android.sdk.utils.NetworkMonitor
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import okhttp3.Cache
import okhttp3.ConnectionPool
import okhttp3.OkHttpClient
import okhttp3.logging.HttpLoggingInterceptor
import timber.log.Timber
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * Mobile-optimized API client with Android lifecycle awareness, caching, and security features.
 * 
 * This class extends the generated ApiClient with mobile-specific optimizations:
 * - Lifecycle-aware connection management
 * - Certificate pinning for security
 * - Intelligent caching strategies
 * - Network connectivity monitoring
 * - Background request handling
 * - Secure credential storage
 */
class MobileApiClient private constructor(
    private val context: Context,
    private val baseUrl: String
) : DefaultLifecycleObserver {

    private val apiClient: ApiClient
    private val cacheManager: ApiCacheManager
    private val credentialStorage: SecureCredentialStorage
    private val networkMonitor: NetworkMonitor
    private val coroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    
    private var isInitialized = false
    private var lifecycleJob: Job? = null

    init {
        cacheManager = ApiCacheManager(context)
        credentialStorage = SecureCredentialStorage(context)
        networkMonitor = NetworkMonitor(context)
        
        apiClient = createOptimizedApiClient()
        
        // Register for process lifecycle events
        ProcessLifecycleOwner.get().lifecycle.addObserver(this)
        
        isInitialized = true
        Timber.d("MobileApiClient initialized with base URL: $baseUrl")
    }

    /**
     * Creates an optimized OkHttpClient with mobile-specific configurations.
     */
    private fun createOptimizedApiClient(): ApiClient {
        val cacheDir = File(context.cacheDir, "api_cache")
        val cache = Cache(cacheDir, BuildConfig.CACHE_SIZE_BYTES)
        
        val httpClient = OkHttpClient.Builder()
            .apply {
                // Connection pooling optimized for mobile
                connectionPool(ConnectionPool(
                    maxIdleConnections = 5,
                    keepAliveDuration = 30,
                    TimeUnit.SECONDS
                ))
                
                // Timeout configurations for mobile networks
                connectTimeout(30, TimeUnit.SECONDS)
                readTimeout(60, TimeUnit.SECONDS)
                writeTimeout(60, TimeUnit.SECONDS)
                callTimeout(120, TimeUnit.SECONDS)
                
                // Add cache for offline support
                cache(cache)
                addInterceptor(cacheManager.createCacheInterceptor())
                addNetworkInterceptor(cacheManager.createNetworkCacheInterceptor())
                
                // Add certificate pinning for production
                if (BuildConfig.CERTIFICATE_PINNING_ENABLED) {
                    addInterceptor(CertificatePinningInterceptor())
                }
                
                // Add logging in debug builds
                if (BuildConfig.DEBUG_ENABLED) {
                    val loggingInterceptor = HttpLoggingInterceptor { message ->
                        Timber.tag("WRFCoin-API").d(message)
                    }.apply {
                        level = HttpLoggingInterceptor.Level.BODY
                    }
                    addInterceptor(loggingInterceptor)
                }
                
                // Network monitoring interceptor
                addInterceptor(networkMonitor.createNetworkInterceptor())
                
                // Retry on connection failure
                retryOnConnectionFailure(true)
            }
            .build()

        return ApiClient(httpClient).apply {
            setBasePath(baseUrl)
            
            // Set default headers
            addDefaultHeader("User-Agent", getUserAgent())
            addDefaultHeader("X-SDK-Version", BuildConfig.SDK_VERSION)
            addDefaultHeader("X-Platform", "android")
            
            // Configure authentication if available
            configureAuthentication()
        }
    }

    /**
     * Configures authentication using securely stored credentials.
     */
    private fun ApiClient.configureAuthentication() {
        credentialStorage.getApiKey()?.let { apiKey ->
            setApiKey(apiKey)
        }
        
        credentialStorage.getBearerToken()?.let { token ->
            setBearerToken(token)
        }
    }

    /**
     * Generates a user agent string with device and app information.
     */
    private fun getUserAgent(): String {
        val packageInfo = context.packageManager.getPackageInfo(context.packageName, 0)
        return "WRFCoin-Android-SDK/${BuildConfig.SDK_VERSION} " +
                "(${packageInfo.packageName}/${packageInfo.versionName}; " +
                "Android ${android.os.Build.VERSION.RELEASE}; " +
                "${android.os.Build.MANUFACTURER} ${android.os.Build.MODEL})"
    }

    /**
     * Sets the API key for authentication.
     */
    fun setApiKey(apiKey: String) {
        credentialStorage.saveApiKey(apiKey)
        apiClient.setApiKey(apiKey)
    }

    /**
     * Sets the Bearer token for authentication.
     */
    fun setBearerToken(token: String) {
        credentialStorage.saveBearerToken(token)
        apiClient.setBearerToken(token)
    }

    /**
     * Clears all stored credentials.
     */
    fun clearCredentials() {
        credentialStorage.clearAll()
        apiClient.setApiKey(null)
        apiClient.setBearerToken(null)
    }

    /**
     * Returns the underlying ApiClient for advanced usage.
     */
    fun getApiClient(): ApiClient = apiClient

    /**
     * Checks if the device is currently online.
     */
    fun isOnline(): Boolean = networkMonitor.isConnected()

    /**
     * Clears the API response cache.
     */
    fun clearCache() {
        cacheManager.clearCache()
    }

    /**
     * Gets the current cache size in bytes.
     */
    fun getCacheSize(): Long = cacheManager.getCacheSize()

    // Lifecycle callbacks
    
    override fun onStart(owner: LifecycleOwner) {
        super.onStart(owner)
        Timber.d("App foregrounded - resuming network operations")
        networkMonitor.startMonitoring()
    }

    override fun onStop(owner: LifecycleOwner) {
        super.onStop(owner)
        Timber.d("App backgrounded - optimizing network usage")
        
        // Clear idle connections to save battery
        apiClient.httpClient.connectionPool.evictAll()
        
        // Optionally pause non-critical network operations
        networkMonitor.stopMonitoring()
    }

    override fun onDestroy(owner: LifecycleOwner) {
        super.onDestroy(owner)
        cleanup()
    }

    /**
     * Cleanup resources when the client is no longer needed.
     */
    private fun cleanup() {
        lifecycleJob?.cancel()
        apiClient.httpClient.cache?.close()
        networkMonitor.cleanup()
        ProcessLifecycleOwner.get().lifecycle.removeObserver(this)
        Timber.d("MobileApiClient cleaned up")
    }

    companion object {
        @Volatile
        private var INSTANCE: MobileApiClient? = null

        /**
         * Gets or creates a singleton instance of MobileApiClient.
         */
        fun getInstance(context: Context, baseUrl: String? = null): MobileApiClient {
            return INSTANCE ?: synchronized(this) {
                INSTANCE ?: MobileApiClient(
                    context.applicationContext,
                    baseUrl ?: getDefaultBaseUrl()
                ).also { INSTANCE = it }
            }
        }

        /**
         * Gets the default base URL based on build configuration.
         */
        private fun getDefaultBaseUrl(): String {
            return when {
                BuildConfig.DEBUG -> BuildConfig.BASE_URL_LOCAL
                BuildConfig.BUILD_TYPE == "staging" -> BuildConfig.BASE_URL_STAGING
                else -> BuildConfig.BASE_URL_PRODUCTION
            }
        }

        /**
         * Creates a new instance (useful for testing or multiple configurations).
         */
        fun createInstance(context: Context, baseUrl: String): MobileApiClient {
            return MobileApiClient(context.applicationContext, baseUrl)
        }
    }
}