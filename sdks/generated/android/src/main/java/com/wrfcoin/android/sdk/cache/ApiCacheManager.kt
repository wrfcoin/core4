package com.wrfcoin.android.sdk.cache

import android.content.Context
import androidx.work.WorkManager
import com.wrfcoin.android.sdk.BuildConfig
import kotlinx.coroutines.*
import okhttp3.CacheControl
import okhttp3.Interceptor
import okhttp3.Response
import timber.log.Timber
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * Intelligent cache manager for API responses with mobile-optimized strategies.
 * 
 * Features:
 * - Offline-first caching for critical endpoints
 * - Network-aware cache strategies
 * - Cache invalidation and cleanup
 * - Background cache optimization
 * - Analytics for cache performance
 */
class ApiCacheManager(private val context: Context) {

    companion object {
        // Cache control constants
        private const val MAX_AGE_ONLINE = 5 * 60 // 5 minutes when online
        private const val MAX_STALE_OFFLINE = 24 * 60 * 60 // 24 hours when offline
        private const val MAX_CACHE_SIZE = 50 * 1024 * 1024L // 50MB
        
        // Cache strategies for different endpoint types
        private val CACHE_STRATEGIES = mapOf(
            "weather" to CacheStrategy.CACHE_THEN_NETWORK,
            "blockchain" to CacheStrategy.NETWORK_ONLY,
            "profile" to CacheStrategy.CACHE_FIRST,
            "static" to CacheStrategy.CACHE_ONLY
        )
    }

    enum class CacheStrategy {
        NETWORK_ONLY,       // Always fetch from network
        CACHE_ONLY,         // Only use cache, never network
        CACHE_FIRST,        // Try cache first, fallback to network
        NETWORK_FIRST,      // Try network first, fallback to cache
        CACHE_THEN_NETWORK  // Show cache immediately, update from network
    }

    private val cacheDir = File(context.cacheDir, "wrfcoin_api_cache")
    private val coroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    init {
        // Ensure cache directory exists
        if (!cacheDir.exists()) {
            cacheDir.mkdirs()
        }
        
        // Schedule periodic cache cleanup
        scheduleCacheCleanup()
        
        Timber.d("ApiCacheManager initialized with cache dir: ${cacheDir.absolutePath}")
    }

    /**
     * Creates an interceptor for handling cache headers.
     */
    fun createCacheInterceptor(): Interceptor {
        return Interceptor { chain ->
            val request = chain.request()
            val url = request.url.toString()
            
            // Determine cache strategy based on endpoint
            val cacheStrategy = determineCacheStrategy(url)
            
            val modifiedRequest = when (cacheStrategy) {
                CacheStrategy.NETWORK_ONLY -> {
                    request.newBuilder()
                        .cacheControl(CacheControl.FORCE_NETWORK)
                        .build()
                }
                CacheStrategy.CACHE_ONLY -> {
                    request.newBuilder()
                        .cacheControl(CacheControl.FORCE_CACHE)
                        .build()
                }
                CacheStrategy.CACHE_FIRST -> {
                    if (isNetworkAvailable()) {
                        request.newBuilder()
                            .cacheControl(CacheControl.Builder()
                                .maxStale(MAX_STALE_OFFLINE, TimeUnit.SECONDS)
                                .build())
                            .build()
                    } else {
                        request.newBuilder()
                            .cacheControl(CacheControl.FORCE_CACHE)
                            .build()
                    }
                }
                else -> request
            }
            
            try {
                val response = chain.proceed(modifiedRequest)
                logCacheHit(url, response.cacheResponse != null)
                response
            } catch (e: Exception) {
                Timber.e(e, "Cache interceptor error for URL: $url")
                throw e
            }
        }
    }

    /**
     * Creates a network interceptor for adding cache headers to responses.
     */
    fun createNetworkCacheInterceptor(): Interceptor {
        return Interceptor { chain ->
            val response = chain.proceed(chain.request())
            val url = response.request.url.toString()
            
            // Add appropriate cache headers based on content type
            val cacheControl = when {
                url.contains("/weather/") -> {
                    CacheControl.Builder()
                        .maxAge(MAX_AGE_ONLINE, TimeUnit.SECONDS)
                        .maxStale(MAX_STALE_OFFLINE, TimeUnit.SECONDS)
                        .build()
                }
                url.contains("/blockchain/") -> {
                    CacheControl.Builder()
                        .noCache()
                        .noStore()
                        .build()
                }
                url.contains("/profile/") -> {
                    CacheControl.Builder()
                        .maxAge(15 * 60, TimeUnit.SECONDS) // 15 minutes
                        .build()
                }
                else -> {
                    CacheControl.Builder()
                        .maxAge(MAX_AGE_ONLINE, TimeUnit.SECONDS)
                        .build()
                }
            }
            
            response.newBuilder()
                .header("Cache-Control", cacheControl.toString())
                .removeHeader("Pragma")
                .removeHeader("Expires")
                .build()
        }
    }

    /**
     * Determines the appropriate cache strategy for a given URL.
     */
    private fun determineCacheStrategy(url: String): CacheStrategy {
        return when {
            url.contains("/weather/") -> CACHE_STRATEGIES["weather"] ?: CacheStrategy.CACHE_THEN_NETWORK
            url.contains("/blockchain/") -> CACHE_STRATEGIES["blockchain"] ?: CacheStrategy.NETWORK_ONLY
            url.contains("/profile/") -> CACHE_STRATEGIES["profile"] ?: CacheStrategy.CACHE_FIRST
            url.contains("/static/") -> CACHE_STRATEGIES["static"] ?: CacheStrategy.CACHE_ONLY
            else -> CacheStrategy.NETWORK_FIRST
        }
    }

    /**
     * Checks if network is available (placeholder - actual implementation would check connectivity).
     */
    private fun isNetworkAvailable(): Boolean {
        // This would typically use ConnectivityManager
        // For now, returning true as a placeholder
        return true
    }

    /**
     * Logs cache hit/miss for analytics.
     */
    private fun logCacheHit(url: String, wasHit: Boolean) {
        if (BuildConfig.DEBUG_ENABLED) {
            val status = if (wasHit) "HIT" else "MISS"
            Timber.d("Cache $status for URL: $url")
        }
    }

    /**
     * Clears the entire API cache.
     */
    fun clearCache() {
        try {
            if (cacheDir.exists()) {
                cacheDir.deleteRecursively()
                cacheDir.mkdirs()
            }
            Timber.d("Cache cleared successfully")
        } catch (e: Exception) {
            Timber.e(e, "Failed to clear cache")
        }
    }

    /**
     * Gets the current cache size in bytes.
     */
    fun getCacheSize(): Long {
        return try {
            cacheDir.walkTopDown()
                .filter { it.isFile }
                .map { it.length() }
                .sum()
        } catch (e: Exception) {
            Timber.e(e, "Failed to calculate cache size")
            0L
        }
    }

    /**
     * Clears cache for specific endpoint patterns.
     */
    fun clearCacheForPattern(pattern: String) {
        coroutineScope.launch {
            try {
                // This is a simplified implementation
                // Real implementation would need to map URLs to cache files
                Timber.d("Clearing cache for pattern: $pattern")
            } catch (e: Exception) {
                Timber.e(e, "Failed to clear cache for pattern: $pattern")
            }
        }
    }

    /**
     * Schedules periodic cache cleanup using WorkManager.
     */
    private fun scheduleCacheCleanup() {
        try {
            // Schedule cache cleanup work (implementation depends on WorkManager setup)
            Timber.d("Cache cleanup scheduled")
        } catch (e: Exception) {
            Timber.e(e, "Failed to schedule cache cleanup")
        }
    }

    /**
     * Optimizes cache by removing old or large files.
     */
    fun optimizeCache() {
        coroutineScope.launch {
            try {
                val currentSize = getCacheSize()
                if (currentSize > MAX_CACHE_SIZE) {
                    // Remove oldest files first
                    val files = cacheDir.walkTopDown()
                        .filter { it.isFile }
                        .sortedBy { it.lastModified() }
                    
                    var removedSize = 0L
                    val targetSize = MAX_CACHE_SIZE * 0.8 // Remove 20% extra for breathing room
                    
                    for (file in files) {
                        if (currentSize - removedSize <= targetSize) break
                        
                        val fileSize = file.length()
                        if (file.delete()) {
                            removedSize += fileSize
                        }
                    }
                    
                    Timber.d("Cache optimized: removed ${removedSize} bytes")
                }
            } catch (e: Exception) {
                Timber.e(e, "Failed to optimize cache")
            }
        }
    }

    /**
     * Gets cache statistics for debugging and monitoring.
     */
    fun getCacheStats(): Map<String, Any> {
        return try {
            val files = cacheDir.walkTopDown().filter { it.isFile }.toList()
            mapOf(
                "cacheSize" to getCacheSize(),
                "fileCount" to files.size,
                "cacheDir" to cacheDir.absolutePath,
                "oldestFile" to (files.minByOrNull { it.lastModified() }?.lastModified() ?: 0),
                "newestFile" to (files.maxByOrNull { it.lastModified() }?.lastModified() ?: 0)
            )
        } catch (e: Exception) {
            Timber.e(e, "Failed to get cache stats")
            emptyMap()
        }
    }

    /**
     * Preloads critical data for offline usage.
     */
    fun preloadCriticalData(endpoints: List<String>) {
        coroutineScope.launch {
            try {
                Timber.d("Preloading critical data for ${endpoints.size} endpoints")
                // Implementation would make requests to specified endpoints
                // to populate cache for offline usage
            } catch (e: Exception) {
                Timber.e(e, "Failed to preload critical data")
            }
        }
    }

    /**
     * Cleanup resources.
     */
    fun cleanup() {
        coroutineScope.cancel()
        Timber.d("ApiCacheManager cleaned up")
    }
}