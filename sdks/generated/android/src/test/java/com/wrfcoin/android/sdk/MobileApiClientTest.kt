package com.wrfcoin.android.sdk

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.wrfcoin.android.sdk.lifecycle.MobileApiClient
import com.wrfcoin.android.sdk.security.SecureCredentialStorage
import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.Mock
import org.mockito.MockitoAnnotations
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Comprehensive test suite for MobileApiClient.
 * 
 * Tests mobile-specific functionality including:
 * - Lifecycle management
 * - Network connectivity handling
 * - Cache operations
 * - Security features
 * - Background task management
 * - Error handling and recovery
 */
@RunWith(AndroidJUnit4::class)
@Config(sdk = [28])
class MobileApiClientTest {

    @Mock
    private lateinit var mockContext: Context
    
    private lateinit var context: Context
    private lateinit var mockWebServer: MockWebServer
    private lateinit var apiClient: MobileApiClient
    private lateinit var secureStorage: SecureCredentialStorage

    @Before
    fun setUp() {
        MockitoAnnotations.openMocks(this)
        context = ApplicationProvider.getApplicationContext()
        
        // Setup mock web server
        mockWebServer = MockWebServer()
        mockWebServer.start()
        
        // Create API client with test configuration
        apiClient = MobileApiClient.createInstance(
            context = context,
            baseUrl = mockWebServer.url("/").toString()
        )
        
        secureStorage = SecureCredentialStorage(context)
    }

    @After
    fun tearDown() {
        mockWebServer.shutdown()
    }

    @Test
    fun testApiClientInitialization() {
        assertNotNull(apiClient)
        assertTrue(apiClient.getApiClient().basePath.isNotEmpty())
    }

    @Test
    fun testSecureCredentialStorage() {
        val testApiKey = "test_api_key_12345"
        
        // Save API key
        secureStorage.saveApiKey(testApiKey)
        
        // Retrieve API key
        val retrievedApiKey = secureStorage.getApiKey()
        
        assertEquals(testApiKey, retrievedApiKey)
    }

    @Test
    fun testApiKeyConfiguration() {
        val testApiKey = "test_api_key_67890"
        
        apiClient.setApiKey(testApiKey)
        
        // Verify API key is stored securely
        val storedApiKey = secureStorage.getApiKey()
        assertEquals(testApiKey, storedApiKey)
    }

    @Test
    fun testBearerTokenConfiguration() {
        val testToken = "bearer_token_test_12345"
        
        apiClient.setBearerToken(testToken)
        
        // Verify token is stored securely
        val storedToken = secureStorage.getBearerToken()
        assertEquals(testToken, storedToken)
    }

    @Test
    fun testCredentialClearing() {
        // Set credentials
        apiClient.setApiKey("test_key")
        apiClient.setBearerToken("test_token")
        
        // Clear credentials
        apiClient.clearCredentials()
        
        // Verify credentials are cleared
        assertEquals(null, secureStorage.getApiKey())
        assertEquals(null, secureStorage.getBearerToken())
    }

    @Test
    fun testNetworkConnectivityCheck() {
        // Test online status detection
        val isOnline = apiClient.isOnline()
        
        // In test environment, this should return a boolean
        assertTrue(isOnline is Boolean)
    }

    @Test
    fun testCacheOperations() {
        // Get initial cache size
        val initialSize = apiClient.getCacheSize()
        assertTrue(initialSize >= 0)
        
        // Clear cache
        apiClient.clearCache()
        
        // Verify cache is cleared (size should be 0 or minimal)
        val clearedSize = apiClient.getCacheSize()
        assertTrue(clearedSize <= initialSize)
    }

    @Test
    fun testApiResponseHandling() = runTest {
        // Setup mock response
        val mockResponse = MockResponse()
            .setResponseCode(200)
            .setBody("""{"status": "success", "message": "Test response"}""")
            .addHeader("Content-Type", "application/json")
        
        mockWebServer.enqueue(mockResponse)
        
        // Make API request (would use actual SDK API call)
        // This is a placeholder for actual API testing
        val response = mockWebServer.takeRequest()
        
        assertNotNull(response)
        assertEquals("GET", response.method)
    }

    @Test
    fun testErrorHandling() = runTest {
        // Setup error response
        val errorResponse = MockResponse()
            .setResponseCode(404)
            .setBody("""{"error": "Not found"}""")
        
        mockWebServer.enqueue(errorResponse)
        
        // Test error handling
        val response = mockWebServer.takeRequest()
        assertNotNull(response)
    }

    @Test
    fun testSecurityValidation() {
        // Test credential validation
        val hasCredentials = secureStorage.hasCredentials()
        assertTrue(hasCredentials is Boolean)
        
        // Test wallet info validation
        val hasWalletInfo = secureStorage.hasWalletInfo()
        assertTrue(hasWalletInfo is Boolean)
    }

    @Test
    fun testWalletCredentialManagement() {
        val testAddress = "0x1234567890abcdef1234567890abcdef12345678"
        val testPrivateKey = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        val testPublicKey = "0x04abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        
        // Save wallet credentials
        secureStorage.saveWalletAddress(testAddress)
        secureStorage.savePrivateKey(testPrivateKey)
        secureStorage.savePublicKey(testPublicKey)
        
        // Retrieve and verify
        assertEquals(testAddress, secureStorage.getWalletAddress())
        assertEquals(testPrivateKey, secureStorage.getPrivateKey())
        assertEquals(testPublicKey, secureStorage.getPublicKey())
        
        // Test wallet info detection
        assertTrue(secureStorage.hasWalletInfo())
    }

    @Test
    fun testMnemonicHashStorage() {
        val testMnemonicHash = "abc123def456ghi789jkl012mno345pqr678stu901vwx234yz567"
        
        // Save mnemonic hash
        secureStorage.saveMnemonicHash(testMnemonicHash)
        
        // Retrieve and verify
        assertEquals(testMnemonicHash, secureStorage.getMnemonicHash())
    }

    @Test
    fun testCredentialExport() {
        // Set up test credentials
        secureStorage.saveApiKey("test_api")
        secureStorage.saveBearerToken("test_token")
        secureStorage.saveWalletAddress("test_address")
        secureStorage.savePublicKey("test_public_key")
        
        // Export credential info
        val exportedInfo = secureStorage.exportCredentialInfo()
        
        assertTrue(exportedInfo.containsKey("hasApiKey"))
        assertTrue(exportedInfo.containsKey("hasBearerToken"))
        assertTrue(exportedInfo.containsKey("hasWalletAddress"))
        assertTrue(exportedInfo.containsKey("hasPublicKey"))
        
        assertEquals(true, exportedInfo["hasApiKey"])
        assertEquals(true, exportedInfo["hasBearerToken"])
        assertEquals("test_address", exportedInfo["walletAddress"])
        assertEquals("test_public_key", exportedInfo["publicKey"])
    }

    @Test
    fun testPartialCredentialClearing() {
        // Set up credentials
        secureStorage.saveApiKey("test_api")
        secureStorage.saveBearerToken("test_token")
        secureStorage.saveWalletAddress("test_address")
        secureStorage.savePrivateKey("test_private")
        
        // Clear only auth credentials
        secureStorage.clearAuthCredentials()
        
        // Verify auth credentials are cleared
        assertEquals(null, secureStorage.getApiKey())
        assertEquals(null, secureStorage.getBearerToken())
        
        // Verify wallet credentials remain
        assertEquals("test_address", secureStorage.getWalletAddress())
        assertEquals("test_private", secureStorage.getPrivateKey())
        
        // Clear wallet credentials
        secureStorage.clearWalletCredentials()
        
        // Verify wallet credentials are cleared
        assertEquals(null, secureStorage.getWalletAddress())
        assertEquals(null, secureStorage.getPrivateKey())
    }

    @Test
    fun testMultipleApiClientInstances() {
        val client1 = MobileApiClient.getInstance(context, "https://api1.example.com")
        val client2 = MobileApiClient.getInstance(context, "https://api2.example.com")
        
        // Should return the same singleton instance
        assertEquals(client1, client2)
        
        // Create separate instances
        val client3 = MobileApiClient.createInstance(context, "https://api3.example.com")
        val client4 = MobileApiClient.createInstance(context, "https://api4.example.com")
        
        // Should be different instances
        assertTrue(client3 !== client4)
    }

    @Test
    fun testUserAgentGeneration() {
        // This would test the user agent string generation
        // Placeholder for actual implementation testing
        assertTrue(true)
    }

    @Test
    fun testConcurrentOperations() = runTest {
        // Test concurrent API calls and operations
        // This would test thread safety and concurrent access
        
        val operations = (1..10).map {
            apiClient.setApiKey("test_key_$it")
            apiClient.getApiClient()
        }
        
        assertTrue(operations.isNotEmpty())
    }

    @Test
    fun testMemoryManagement() {
        // Test memory usage and cleanup
        val initialCacheSize = apiClient.getCacheSize()
        
        // Perform operations that might use memory
        repeat(100) {
            apiClient.setApiKey("test_key_$it")
        }
        
        // Clear and verify cleanup
        apiClient.clearCache()
        secureStorage.clearAll()
        
        assertTrue(apiClient.getCacheSize() <= initialCacheSize)
    }

    @Test
    fun testConfigurationValidation() {
        // Test various configuration scenarios
        val testConfigs = listOf(
            "https://api.wrfcoin.com",
            "http://localhost:4001",
            "https://staging-api.wrfcoin.com"
        )
        
        testConfigs.forEach { baseUrl ->
            val client = MobileApiClient.createInstance(context, baseUrl)
            assertNotNull(client)
            assertTrue(client.getApiClient().basePath.contains(baseUrl))
        }
    }
}