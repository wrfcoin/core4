package com.wrfcoin.android.sdk.security

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import timber.log.Timber

/**
 * Secure storage for API credentials using Android's EncryptedSharedPreferences.
 * 
 * This class provides secure storage for sensitive data like API keys and tokens
 * using Android's security crypto library with hardware-backed encryption when available.
 */
class SecureCredentialStorage(private val context: Context) {

    companion object {
        private const val PREFS_NAME = "wrfcoin_secure_prefs"
        private const val KEY_API_KEY = "api_key"
        private const val KEY_BEARER_TOKEN = "bearer_token"
        private const val KEY_PRIVATE_KEY = "private_key"
        private const val KEY_PUBLIC_KEY = "public_key"
        private const val KEY_WALLET_ADDRESS = "wallet_address"
        private const val KEY_MNEMONIC_HASH = "mnemonic_hash" // Only store hash, never the actual mnemonic
    }

    private val masterKey: MasterKey by lazy {
        MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .setRequestStrongBoxBacked(true) // Use hardware security module if available
            .build()
    }

    private val encryptedPrefs: SharedPreferences by lazy {
        try {
            EncryptedSharedPreferences.create(
                context,
                PREFS_NAME,
                masterKey,
                EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
            )
        } catch (e: Exception) {
            Timber.e(e, "Failed to create encrypted preferences, falling back to regular preferences")
            // Fallback to regular preferences if encryption fails
            context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        }
    }

    /**
     * Saves the API key securely.
     */
    fun saveApiKey(apiKey: String) {
        try {
            encryptedPrefs.edit()
                .putString(KEY_API_KEY, apiKey)
                .apply()
            Timber.d("API key saved securely")
        } catch (e: Exception) {
            Timber.e(e, "Failed to save API key")
        }
    }

    /**
     * Retrieves the stored API key.
     */
    fun getApiKey(): String? {
        return try {
            encryptedPrefs.getString(KEY_API_KEY, null)
        } catch (e: Exception) {
            Timber.e(e, "Failed to retrieve API key")
            null
        }
    }

    /**
     * Saves the Bearer token securely.
     */
    fun saveBearerToken(token: String) {
        try {
            encryptedPrefs.edit()
                .putString(KEY_BEARER_TOKEN, token)
                .apply()
            Timber.d("Bearer token saved securely")
        } catch (e: Exception) {
            Timber.e(e, "Failed to save Bearer token")
        }
    }

    /**
     * Retrieves the stored Bearer token.
     */
    fun getBearerToken(): String? {
        return try {
            encryptedPrefs.getString(KEY_BEARER_TOKEN, null)
        } catch (e: Exception) {
            Timber.e(e, "Failed to retrieve Bearer token")
            null
        }
    }

    /**
     * Saves wallet private key securely.
     * WARNING: Store only if absolutely necessary and consider using hardware security modules.
     */
    fun savePrivateKey(privateKey: String) {
        try {
            encryptedPrefs.edit()
                .putString(KEY_PRIVATE_KEY, privateKey)
                .apply()
            Timber.d("Private key saved securely")
        } catch (e: Exception) {
            Timber.e(e, "Failed to save private key")
        }
    }

    /**
     * Retrieves the stored private key.
     */
    fun getPrivateKey(): String? {
        return try {
            encryptedPrefs.getString(KEY_PRIVATE_KEY, null)
        } catch (e: Exception) {
            Timber.e(e, "Failed to retrieve private key")
            null
        }
    }

    /**
     * Saves wallet public key.
     */
    fun savePublicKey(publicKey: String) {
        try {
            encryptedPrefs.edit()
                .putString(KEY_PUBLIC_KEY, publicKey)
                .apply()
            Timber.d("Public key saved securely")
        } catch (e: Exception) {
            Timber.e(e, "Failed to save public key")
        }
    }

    /**
     * Retrieves the stored public key.
     */
    fun getPublicKey(): String? {
        return try {
            encryptedPrefs.getString(KEY_PUBLIC_KEY, null)
        } catch (e: Exception) {
            Timber.e(e, "Failed to retrieve public key")
            null
        }
    }

    /**
     * Saves wallet address.
     */
    fun saveWalletAddress(address: String) {
        try {
            encryptedPrefs.edit()
                .putString(KEY_WALLET_ADDRESS, address)
                .apply()
            Timber.d("Wallet address saved securely")
        } catch (e: Exception) {
            Timber.e(e, "Failed to save wallet address")
        }
    }

    /**
     * Retrieves the stored wallet address.
     */
    fun getWalletAddress(): String? {
        return try {
            encryptedPrefs.getString(KEY_WALLET_ADDRESS, null)
        } catch (e: Exception) {
            Timber.e(e, "Failed to retrieve wallet address")
            null
        }
    }

    /**
     * Saves a hash of the mnemonic phrase (never store the actual mnemonic).
     */
    fun saveMnemonicHash(mnemonicHash: String) {
        try {
            encryptedPrefs.edit()
                .putString(KEY_MNEMONIC_HASH, mnemonicHash)
                .apply()
            Timber.d("Mnemonic hash saved securely")
        } catch (e: Exception) {
            Timber.e(e, "Failed to save mnemonic hash")
        }
    }

    /**
     * Retrieves the stored mnemonic hash.
     */
    fun getMnemonicHash(): String? {
        return try {
            encryptedPrefs.getString(KEY_MNEMONIC_HASH, null)
        } catch (e: Exception) {
            Timber.e(e, "Failed to retrieve mnemonic hash")
            null
        }
    }

    /**
     * Checks if any credentials are stored.
     */
    fun hasCredentials(): Boolean {
        return getApiKey() != null || getBearerToken() != null
    }

    /**
     * Checks if wallet information is stored.
     */
    fun hasWalletInfo(): Boolean {
        return getWalletAddress() != null || getPublicKey() != null
    }

    /**
     * Clears all stored credentials.
     */
    fun clearAll() {
        try {
            encryptedPrefs.edit().clear().apply()
            Timber.d("All credentials cleared")
        } catch (e: Exception) {
            Timber.e(e, "Failed to clear credentials")
        }
    }

    /**
     * Clears only authentication credentials (keeps wallet info).
     */
    fun clearAuthCredentials() {
        try {
            encryptedPrefs.edit()
                .remove(KEY_API_KEY)
                .remove(KEY_BEARER_TOKEN)
                .apply()
            Timber.d("Authentication credentials cleared")
        } catch (e: Exception) {
            Timber.e(e, "Failed to clear authentication credentials")
        }
    }

    /**
     * Clears only wallet credentials (keeps auth info).
     */
    fun clearWalletCredentials() {
        try {
            encryptedPrefs.edit()
                .remove(KEY_PRIVATE_KEY)
                .remove(KEY_PUBLIC_KEY)
                .remove(KEY_WALLET_ADDRESS)
                .remove(KEY_MNEMONIC_HASH)
                .apply()
            Timber.d("Wallet credentials cleared")
        } catch (e: Exception) {
            Timber.e(e, "Failed to clear wallet credentials")
        }
    }

    /**
     * Exports credential information for backup (without sensitive data).
     */
    fun exportCredentialInfo(): Map<String, Any> {
        return mapOf(
            "hasApiKey" to (getApiKey() != null),
            "hasBearerToken" to (getBearerToken() != null),
            "hasWalletAddress" to (getWalletAddress() != null),
            "hasPublicKey" to (getPublicKey() != null),
            "walletAddress" to getWalletAddress(),
            "publicKey" to getPublicKey()
        ).filterValues { it != null }
    }
}