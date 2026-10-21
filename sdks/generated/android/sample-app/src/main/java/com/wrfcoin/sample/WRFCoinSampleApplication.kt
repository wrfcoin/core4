package com.wrfcoin.sample

import android.app.Application
import androidx.work.Configuration
import androidx.work.WorkManager
import com.wrfcoin.android.sdk.lifecycle.MobileApiClient
import com.wrfcoin.sample.background.WRFCoinWorkerFactory
import timber.log.Timber

/**
 * Application class for WRFCoin Sample App.
 * 
 * Initializes the WRFCoin SDK and configures app-wide components.
 */
class WRFCoinSampleApplication : Application(), Configuration.Provider {

    lateinit var apiClient: MobileApiClient
        private set

    override fun onCreate() {
        super.onCreate()
        
        // Initialize logging
        if (BuildConfig.DEBUG) {
            Timber.plant(Timber.DebugTree())
        }
        
        Timber.d("WRFCoin Sample App starting...")
        
        // Initialize WRFCoin SDK
        initializeWRFCoinSDK()
        
        // Initialize WorkManager
        initializeWorkManager()
        
        Timber.d("WRFCoin Sample App initialized successfully")
    }

    /**
     * Initializes the WRFCoin SDK with configuration.
     */
    private fun initializeWRFCoinSDK() {
        try {
            // Create MobileApiClient instance
            apiClient = MobileApiClient.getInstance(
                context = this,
                baseUrl = BuildConfig.API_BASE_URL
            )
            
            // Configure API key if available
            if (BuildConfig.API_KEY.isNotEmpty() && BuildConfig.API_KEY != "your_api_key_here") {
                apiClient.setApiKey(BuildConfig.API_KEY)
            }
            
            Timber.d("WRFCoin SDK initialized with base URL: ${BuildConfig.API_BASE_URL}")
        } catch (e: Exception) {
            Timber.e(e, "Failed to initialize WRFCoin SDK")
        }
    }

    /**
     * Initializes WorkManager with custom configuration.
     */
    private fun initializeWorkManager() {
        try {
            val workManagerConfig = Configuration.Builder()
                .setWorkerFactory(WRFCoinWorkerFactory(apiClient))
                .setMinimumLoggingLevel(if (BuildConfig.DEBUG) android.util.Log.DEBUG else android.util.Log.INFO)
                .build()
            
            WorkManager.initialize(this, workManagerConfig)
            Timber.d("WorkManager initialized with custom configuration")
        } catch (e: Exception) {
            Timber.e(e, "Failed to initialize WorkManager")
        }
    }

    override fun getWorkManagerConfiguration(): Configuration {
        return Configuration.Builder()
            .setWorkerFactory(WRFCoinWorkerFactory(apiClient))
            .setMinimumLoggingLevel(if (BuildConfig.DEBUG) android.util.Log.DEBUG else android.util.Log.INFO)
            .build()
    }

    /**
     * Gets the global API client instance.
     */
    fun getApiClient(): MobileApiClient = apiClient

    companion object {
        /**
         * Gets the application instance from any context.
         */
        fun getInstance(context: android.content.Context): WRFCoinSampleApplication {
            return context.applicationContext as WRFCoinSampleApplication
        }
    }
}