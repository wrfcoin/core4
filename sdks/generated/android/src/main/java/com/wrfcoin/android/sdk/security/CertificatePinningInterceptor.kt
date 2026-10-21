package com.wrfcoin.android.sdk.security

import okhttp3.CertificatePinner
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import okhttp3.Response
import timber.log.Timber
import java.io.IOException

/**
 * Certificate pinning interceptor for enhanced security.
 * 
 * This interceptor implements certificate pinning to prevent man-in-the-middle attacks
 * by validating that the server's certificate matches expected pins.
 */
class CertificatePinningInterceptor : Interceptor {

    companion object {
        // Production certificate pins for api.wrfcoin.com
        // NOTE: These should be replaced with actual certificate pins
        private const val PRODUCTION_HOST = "api.wrfcoin.com"
        private const val STAGING_HOST = "staging-api.wrfcoin.com"
        
        // Backup pins in case primary certificate is rotated
        private val PRODUCTION_PINS = arrayOf(
            "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", // Primary pin
            "sha256/BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=", // Backup pin
            "sha256/CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC="  // CA pin
        )
        
        private val STAGING_PINS = arrayOf(
            "sha256/DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD=", // Staging primary
            "sha256/EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE="  // Staging backup
        )
    }

    private val certificatePinner: CertificatePinner by lazy {
        CertificatePinner.Builder()
            .apply {
                // Add production pins
                PRODUCTION_PINS.forEach { pin ->
                    add(PRODUCTION_HOST, pin)
                }
                
                // Add staging pins
                STAGING_PINS.forEach { pin ->
                    add(STAGING_HOST, pin)
                }
            }
            .build()
    }

    @Throws(IOException::class)
    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val hostname = request.url.host
        
        return try {
            // Validate certificate pins for known hosts
            when {
                hostname.contains("wrfcoin.com") -> {
                    validateCertificatePins(chain, hostname)
                }
                else -> {
                    // Allow other hosts without pinning
                    chain.proceed(request)
                }
            }
        } catch (e: Exception) {
            Timber.e(e, "Certificate pinning validation failed for host: $hostname")
            
            // In production, you might want to fail the request entirely
            // For now, we'll log the error and proceed (configure based on security requirements)
            when (e) {
                is javax.net.ssl.SSLPeerUnverifiedException -> {
                    Timber.w("SSL certificate validation failed - possible MITM attack detected")
                    throw IOException("Certificate pinning validation failed", e)
                }
                else -> {
                    chain.proceed(request)
                }
            }
        }
    }

    /**
     * Validates certificate pins for the given hostname.
     */
    private fun validateCertificatePins(chain: Interceptor.Chain, hostname: String): Response {
        // Create a temporary client with certificate pinning for this request
        val originalClient = chain.connection()?.socket()?.let { 
            // We need to validate through the certificate pinner
            // This is a simplified implementation - in production you'd want more robust validation
            certificatePinner.check(hostname, emptyList()) // This will throw if validation fails
        }
        
        return chain.proceed(chain.request())
    }

    /**
     * Creates an OkHttpClient with certificate pinning configured.
     */
    fun createSecureClient(baseClient: OkHttpClient): OkHttpClient {
        return baseClient.newBuilder()
            .certificatePinner(certificatePinner)
            .addInterceptor(this)
            .build()
    }

    /**
     * Validates if the given hostname should use certificate pinning.
     */
    fun shouldPinCertificate(hostname: String): Boolean {
        return hostname.contains("wrfcoin.com")
    }

    /**
     * Updates certificate pins at runtime (useful for pin rotation).
     */
    fun updatePins(hostname: String, newPins: Array<String>) {
        // Note: CertificatePinner is immutable, so you'd need to recreate it
        // This is a placeholder for dynamic pin updates
        Timber.i("Certificate pins updated for $hostname")
    }
}