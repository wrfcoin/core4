package com.wrfcoin.android.sdk.background

import android.content.Context
import androidx.work.*
import com.wrfcoin.android.sdk.BuildConfig
import kotlinx.coroutines.*
import timber.log.Timber
import java.util.concurrent.TimeUnit

/**
 * Background task manager for handling API operations when the app is not in foreground.
 * 
 * Features:
 * - WorkManager integration for reliable background execution
 * - Battery-optimized scheduling
 * - Network-aware task execution
 * - Retry strategies with exponential backoff
 * - Priority-based task queuing
 */
class BackgroundTaskManager(private val context: Context) {

    companion object {
        // Work tags for different types of background tasks
        const val TAG_SYNC_WEATHER = "sync_weather"
        const val TAG_SYNC_BLOCKCHAIN = "sync_blockchain"
        const val TAG_UPLOAD_ANALYTICS = "upload_analytics"
        const val TAG_CACHE_CLEANUP = "cache_cleanup"
        const val TAG_HEALTH_CHECK = "health_check"
        
        // Work names for unique one-time tasks
        const val WORK_PERIODIC_SYNC = "periodic_sync"
        const val WORK_CACHE_MAINTENANCE = "cache_maintenance"
        
        // Scheduling intervals
        private const val SYNC_INTERVAL_HOURS = 6L
        private const val CACHE_CLEANUP_INTERVAL_HOURS = 24L
        private const val HEALTH_CHECK_INTERVAL_HOURS = 12L
    }

    enum class TaskPriority {
        LOW,
        NORMAL,
        HIGH,
        CRITICAL
    }

    private val workManager = WorkManager.getInstance(context)
    private val coroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    init {
        setupPeriodicTasks()
        Timber.d("BackgroundTaskManager initialized")
    }

    /**
     * Sets up periodic background tasks.
     */
    private fun setupPeriodicTasks() {
        try {
            // Schedule periodic weather data sync
            schedulePeriodicWeatherSync()
            
            // Schedule cache maintenance
            scheduleCacheMaintenance()
            
            // Schedule health checks
            scheduleHealthCheck()
            
            Timber.d("Periodic background tasks scheduled")
        } catch (e: Exception) {
            Timber.e(e, "Failed to setup periodic tasks")
        }
    }

    /**
     * Schedules periodic weather data synchronization.
     */
    private fun schedulePeriodicWeatherSync() {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .setRequiresBatteryNotLow(true)
            .setRequiresDeviceIdle(false)
            .build()

        val weatherSyncWork = PeriodicWorkRequestBuilder<WeatherSyncWorker>(
            SYNC_INTERVAL_HOURS, TimeUnit.HOURS,
            15, TimeUnit.MINUTES // Flex interval
        )
            .setConstraints(constraints)
            .setBackoffCriteria(
                BackoffPolicy.EXPONENTIAL,
                WorkRequest.MIN_BACKOFF_MILLIS,
                TimeUnit.MILLISECONDS
            )
            .addTag(TAG_SYNC_WEATHER)
            .build()

        workManager.enqueueUniquePeriodicWork(
            WORK_PERIODIC_SYNC,
            ExistingPeriodicWorkPolicy.KEEP,
            weatherSyncWork
        )
    }

    /**
     * Schedules periodic cache maintenance.
     */
    private fun scheduleCacheMaintenance() {
        val constraints = Constraints.Builder()
            .setRequiresBatteryNotLow(true)
            .setRequiresDeviceIdle(true)
            .setRequiresCharging(false)
            .build()

        val cacheMaintenanceWork = PeriodicWorkRequestBuilder<CacheMaintenanceWorker>(
            CACHE_CLEANUP_INTERVAL_HOURS, TimeUnit.HOURS
        )
            .setConstraints(constraints)
            .addTag(TAG_CACHE_CLEANUP)
            .build()

        workManager.enqueueUniquePeriodicWork(
            WORK_CACHE_MAINTENANCE,
            ExistingPeriodicWorkPolicy.KEEP,
            cacheMaintenanceWork
        )
    }

    /**
     * Schedules periodic health checks.
     */
    private fun scheduleHealthCheck() {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .build()

        val healthCheckWork = PeriodicWorkRequestBuilder<HealthCheckWorker>(
            HEALTH_CHECK_INTERVAL_HOURS, TimeUnit.HOURS
        )
            .setConstraints(constraints)
            .addTag(TAG_HEALTH_CHECK)
            .build()

        workManager.enqueueUniquePeriodicWork(
            "health_check_periodic",
            ExistingPeriodicWorkPolicy.KEEP,
            healthCheckWork
        )
    }

    /**
     * Schedules a one-time background sync task.
     */
    fun scheduleImmediateSync(priority: TaskPriority = TaskPriority.NORMAL) {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .build()

        val syncWork = OneTimeWorkRequestBuilder<WeatherSyncWorker>()
            .setConstraints(constraints)
            .setExpedited(priority == TaskPriority.CRITICAL)
            .addTag(TAG_SYNC_WEATHER)
            .build()

        workManager.enqueue(syncWork)
        Timber.d("Immediate sync scheduled with priority: $priority")
    }

    /**
     * Schedules blockchain data synchronization.
     */
    fun scheduleBlockchainSync(walletAddress: String, priority: TaskPriority = TaskPriority.NORMAL) {
        val inputData = Data.Builder()
            .putString("wallet_address", walletAddress)
            .build()

        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.UNMETERED) // Use unmetered for blockchain
            .setRequiresBatteryNotLow(true)
            .build()

        val blockchainWork = OneTimeWorkRequestBuilder<BlockchainSyncWorker>()
            .setInputData(inputData)
            .setConstraints(constraints)
            .setExpedited(priority == TaskPriority.CRITICAL)
            .setBackoffCriteria(
                BackoffPolicy.EXPONENTIAL,
                WorkRequest.MIN_BACKOFF_MILLIS,
                TimeUnit.MILLISECONDS
            )
            .addTag(TAG_SYNC_BLOCKCHAIN)
            .build()

        workManager.enqueue(blockchainWork)
        Timber.d("Blockchain sync scheduled for wallet: $walletAddress")
    }

    /**
     * Schedules analytics data upload.
     */
    fun scheduleAnalyticsUpload(priority: TaskPriority = TaskPriority.LOW) {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.UNMETERED)
            .setRequiresBatteryNotLow(true)
            .setRequiresDeviceIdle(true)
            .build()

        val analyticsWork = OneTimeWorkRequestBuilder<AnalyticsUploadWorker>()
            .setConstraints(constraints)
            .addTag(TAG_UPLOAD_ANALYTICS)
            .build()

        workManager.enqueue(analyticsWork)
        Timber.d("Analytics upload scheduled")
    }

    /**
     * Cancels all background tasks with the specified tag.
     */
    fun cancelTasksByTag(tag: String) {
        workManager.cancelAllWorkByTag(tag)
        Timber.d("Cancelled all work with tag: $tag")
    }

    /**
     * Cancels all background tasks.
     */
    fun cancelAllTasks() {
        workManager.cancelAllWork()
        Timber.d("Cancelled all background work")
    }

    /**
     * Gets the status of work with the specified tag.
     */
    fun getWorkStatus(tag: String): LiveData<List<WorkInfo>> {
        return workManager.getWorkInfosByTagLiveData(tag)
    }

    /**
     * Checks if any work is currently running.
     */
    suspend fun isAnyWorkRunning(): Boolean {
        return try {
            val workInfos = workManager.getWorkInfosByTag(TAG_SYNC_WEATHER).await() +
                           workManager.getWorkInfosByTag(TAG_SYNC_BLOCKCHAIN).await() +
                           workManager.getWorkInfosByTag(TAG_UPLOAD_ANALYTICS).await()
            
            workInfos.any { it.state == WorkInfo.State.RUNNING }
        } catch (e: Exception) {
            Timber.e(e, "Failed to check work status")
            false
        }
    }

    /**
     * Gets background task statistics.
     */
    suspend fun getTaskStatistics(): Map<String, Any> {
        return try {
            val allWorkInfos = workManager.getWorkInfosByTag("*").await()
            
            mapOf(
                "totalTasks" to allWorkInfos.size,
                "runningTasks" to allWorkInfos.count { it.state == WorkInfo.State.RUNNING },
                "enqueuedTasks" to allWorkInfos.count { it.state == WorkInfo.State.ENQUEUED },
                "succeededTasks" to allWorkInfos.count { it.state == WorkInfo.State.SUCCEEDED },
                "failedTasks" to allWorkInfos.count { it.state == WorkInfo.State.FAILED },
                "cancelledTasks" to allWorkInfos.count { it.state == WorkInfo.State.CANCELLED }
            )
        } catch (e: Exception) {
            Timber.e(e, "Failed to get task statistics")
            emptyMap()
        }
    }

    /**
     * Optimizes background task scheduling based on battery and network conditions.
     */
    fun optimizeScheduling() {
        coroutineScope.launch {
            try {
                // Check device conditions and adjust task scheduling
                val isLowBattery = android.os.BatteryManager().let { false } // Placeholder
                val isUnmeteredNetwork = true // Placeholder - check actual network type
                
                if (isLowBattery) {
                    // Reduce background task frequency
                    workManager.cancelAllWorkByTag(TAG_UPLOAD_ANALYTICS)
                    Timber.d("Reduced background tasks due to low battery")
                }
                
                if (!isUnmeteredNetwork) {
                    // Defer large data transfers
                    workManager.cancelAllWorkByTag(TAG_SYNC_BLOCKCHAIN)
                    Timber.d("Deferred blockchain sync due to metered network")
                }
            } catch (e: Exception) {
                Timber.e(e, "Failed to optimize scheduling")
            }
        }
    }

    /**
     * Cleanup resources.
     */
    fun cleanup() {
        coroutineScope.cancel()
        Timber.d("BackgroundTaskManager cleaned up")
    }
}

/**
 * Worker for syncing weather data in the background.
 */
class WeatherSyncWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        return try {
            Timber.d("Starting weather sync background task")
            
            // Simulate weather data sync
            delay(5000) // Placeholder for actual API calls
            
            Timber.d("Weather sync completed successfully")
            Result.success()
        } catch (e: Exception) {
            Timber.e(e, "Weather sync failed")
            Result.retry()
        }
    }
}

/**
 * Worker for syncing blockchain data in the background.
 */
class BlockchainSyncWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        return try {
            val walletAddress = inputData.getString("wallet_address")
            Timber.d("Starting blockchain sync for wallet: $walletAddress")
            
            // Simulate blockchain data sync
            delay(10000) // Placeholder for actual blockchain calls
            
            Timber.d("Blockchain sync completed successfully")
            Result.success()
        } catch (e: Exception) {
            Timber.e(e, "Blockchain sync failed")
            Result.retry()
        }
    }
}

/**
 * Worker for uploading analytics data in the background.
 */
class AnalyticsUploadWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        return try {
            Timber.d("Starting analytics upload")
            
            // Simulate analytics upload
            delay(3000) // Placeholder for actual upload
            
            Timber.d("Analytics upload completed successfully")
            Result.success()
        } catch (e: Exception) {
            Timber.e(e, "Analytics upload failed")
            Result.failure()
        }
    }
}

/**
 * Worker for cache maintenance in the background.
 */
class CacheMaintenanceWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        return try {
            Timber.d("Starting cache maintenance")
            
            // Simulate cache cleanup
            delay(2000) // Placeholder for actual cache operations
            
            Timber.d("Cache maintenance completed successfully")
            Result.success()
        } catch (e: Exception) {
            Timber.e(e, "Cache maintenance failed")
            Result.retry()
        }
    }
}

/**
 * Worker for periodic health checks.
 */
class HealthCheckWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        return try {
            Timber.d("Starting health check")
            
            // Simulate health check ping
            delay(1000) // Placeholder for actual health check
            
            Timber.d("Health check completed successfully")
            Result.success()
        } catch (e: Exception) {
            Timber.e(e, "Health check failed")
            Result.retry()
        }
    }
}