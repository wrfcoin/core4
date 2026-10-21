> **NOT the testnet API.** This client is generated from the backend developer-portal OpenAPI spec, not from the routes the testnet node (`launch_api`) serves, and is not published or kept in sync with the node. Do not assume its operations exist on the testnet. The hand-derived contract for the node is `docs/api/launch-api.openapi.yaml` (core4#3245); inventory and status: `docs/api/generated-code-inventory.md` (core4#2995).

# WRFCoin Enterprise Java SDK

An enterprise-grade Java SDK for the WRFCoin weather forecasting blockchain platform with advanced
features including connection pooling, retry logic, circuit breakers, and comprehensive error
handling.

- API version: 1.0.0
- Build date: 2025-07-22T06:55:31.471663500-07:00[America/Los_Angeles]
- Generator version: 7.14.0

Comprehensive API for the WRFCoin climate-focused blockchain platform with enterprise Java patterns.
impact tracking, and advanced enterprise features.

For more information, please visit [https://docs.wrfcoin.com](https://docs.wrfcoin.com)

_Enhanced with enterprise patterns on top of OpenAPI Generator_

## Requirements

Building the API client library requires:

1. Java 1.8+
2. Maven/Gradle

## Installation

To install the API client library to your local Maven repository, simply execute:

```shell
mvn clean install
```

To deploy it to a remote Maven repository instead, configure the settings of the repository and
execute:

```shell
mvn clean deploy
```

Refer to the [OSSRH Guide](http://central.sonatype.org/pages/ossrh-guide.html) for more information.

### Maven users

Add this dependency to your project's POM:

```xml
<dependency>
  <groupId>com.wrfcoin</groupId>
  <artifactId>wrfcoin-sdk</artifactId>
  <version>1.0.0</version>
  <scope>compile</scope>
</dependency>
```

### Gradle users

Add this dependency to your project's build file:

```groovy
compile "com.wrfcoin:wrfcoin-sdk:1.0.0"
```

### Others

At first generate the JAR by executing:

```shell
mvn clean package
```

Then manually install the following JARs:

- `target/wrfcoin-sdk-1.0.0.jar`
- `target/lib/*.jar`

## Getting Started

Please follow the [installation](#installation) instruction and execute the following Java code:

```java

import com.wrfcoin.sdk.*;
import com.wrfcoin.sdk.auth.*;
import com.wrfcoin.sdk.model.*;
import com.wrfcoin.sdk.api.AiMlApi;

public class AiMlApiExample {

    public static void main(String[] args) {
        ApiClient defaultClient = Configuration.getDefaultApiClient();
        defaultClient.setBasePath("https://api.wrfcoin.com/v1");

        // Configure API key authorization: ApiKeyAuth
        ApiKeyAuth ApiKeyAuth = (ApiKeyAuth) defaultClient.getAuthentication("ApiKeyAuth");
        ApiKeyAuth.setApiKey("YOUR API KEY");
        // Uncomment the following line to set a prefix for the API key, e.g. "Token" (defaults to null)
        //ApiKeyAuth.setApiKeyPrefix("Token");

        // Configure HTTP bearer authorization: BearerAuth
        HttpBearerAuth BearerAuth = (HttpBearerAuth) defaultClient.getAuthentication("BearerAuth");
        BearerAuth.setBearerToken("BEARER TOKEN");

        AiMlApi apiInstance = new AiMlApi(defaultClient);
        ApiAimlInferencePostRequest apiAimlInferencePostRequest = new ApiAimlInferencePostRequest(); // ApiAimlInferencePostRequest |
        try {
            ApiAimlInferencePost200Response result = apiInstance.apiAimlInferencePost(apiAimlInferencePostRequest);
            System.out.println(result);
        } catch (ApiException e) {
            System.err.println("Exception when calling AiMlApi#apiAimlInferencePost");
            System.err.println("Status code: " + e.getCode());
            System.err.println("Reason: " + e.getResponseBody());
            System.err.println("Response headers: " + e.getResponseHeaders());
            e.printStackTrace();
        }
    }
}

```

## Documentation for API Endpoints

All URIs are relative to *https://api.wrfcoin.com/v1*

| Class                 | Method                                                                                                         | HTTP request                                         | Description                           |
| --------------------- | -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------- |
| _AiMlApi_             | [**apiAimlInferencePost**](docs/AiMlApi.md#apiAimlInferencePost)                                               | **POST** /api/aiml/inference                         | Run AI/ML inference                   |
| _AiMlApi_             | [**apiAimlModelsGet**](docs/AiMlApi.md#apiAimlModelsGet)                                                       | **GET** /api/aiml/models                             | List available AI/ML models           |
| _AnalyticsApi_        | [**apiAnalyticsGet**](docs/AnalyticsApi.md#apiAnalyticsGet)                                                    | **GET** /api/analytics                               | Get network analytics                 |
| _BlockchainApi_       | [**apiBlockchainBlocksBlockHashGet**](docs/BlockchainApi.md#apiBlockchainBlocksBlockHashGet)                   | **GET** /api/blockchain/blocks/{blockHash}           | Get block by hash                     |
| _BlockchainApi_       | [**apiBlockchainBlocksGet**](docs/BlockchainApi.md#apiBlockchainBlocksGet)                                     | **GET** /api/blockchain/blocks                       | Get blockchain blocks                 |
| _BlockchainApi_       | [**apiBlockchainStatusGet**](docs/BlockchainApi.md#apiBlockchainStatusGet)                                     | **GET** /api/blockchain/status                       | Get blockchain status                 |
| _BlockchainApi_       | [**apiBlockchainTransactionsTxHashGet**](docs/BlockchainApi.md#apiBlockchainTransactionsTxHashGet)             | **GET** /api/blockchain/transactions/{txHash}        | Get transaction by hash               |
| _ComplianceApi_       | [**apiComplianceStatusGet**](docs/ComplianceApi.md#apiComplianceStatusGet)                                     | **GET** /api/compliance/status                       | Get compliance status                 |
| _ComputeApi_          | [**apiComputeProvidersGet**](docs/ComputeApi.md#apiComputeProvidersGet)                                        | **GET** /api/compute/providers                       | List compute providers                |
| _ComputeApi_          | [**apiComputeTasksPost**](docs/ComputeApi.md#apiComputeTasksPost)                                              | **POST** /api/compute/tasks                          | Submit compute task                   |
| _ComputeApi_          | [**apiComputeTasksTaskIdStatusGet**](docs/ComputeApi.md#apiComputeTasksTaskIdStatusGet)                        | **GET** /api/compute/tasks/{taskId}/status           | Get compute task status               |
| _GovernanceApi_       | [**apiGovernanceProposalsGet**](docs/GovernanceApi.md#apiGovernanceProposalsGet)                               | **GET** /api/governance/proposals                    | List governance proposals             |
| _GovernanceApi_       | [**apiGovernanceProposalsPost**](docs/GovernanceApi.md#apiGovernanceProposalsPost)                             | **POST** /api/governance/proposals                   | Create new proposal                   |
| _GovernanceApi_       | [**apiGovernanceProposalsProposalIdVotePost**](docs/GovernanceApi.md#apiGovernanceProposalsProposalIdVotePost) | **POST** /api/governance/proposals/{proposalId}/vote | Vote on proposal                      |
| _IntegrationsApi_     | [**apiIntegrationsLangchainEnablePost**](docs/IntegrationsApi.md#apiIntegrationsLangchainEnablePost)           | **POST** /api/integrations/langchain/enable          | Enable LangChain integration          |
| _IntegrationsApi_     | [**apiIntegrationsLangchainInsightsGet**](docs/IntegrationsApi.md#apiIntegrationsLangchainInsightsGet)         | **GET** /api/integrations/langchain/insights         | Get LangChain insights                |
| _IntegrationsApi_     | [**apiIntegrationsLangchainStatusGet**](docs/IntegrationsApi.md#apiIntegrationsLangchainStatusGet)             | **GET** /api/integrations/langchain/status           | Get LangChain integration status      |
| _MiningApi_           | [**apiMiningRewardsGet**](docs/MiningApi.md#apiMiningRewardsGet)                                               | **GET** /api/mining/rewards                          | Get mining rewards                    |
| _PerformanceApi_      | [**apiPerformanceMetricsGet**](docs/PerformanceApi.md#apiPerformanceMetricsGet)                                | **GET** /api/performance/metrics                     | Get performance metrics               |
| _PerformanceApi_      | [**apiPerformanceOptimizationPost**](docs/PerformanceApi.md#apiPerformanceOptimizationPost)                    | **POST** /api/performance/optimization               | Trigger performance optimization      |
| _SecurityApi_         | [**apiSecurityStatsGet**](docs/SecurityApi.md#apiSecurityStatsGet)                                             | **GET** /api/security/stats                          | Get security statistics               |
| _SmartContractsApi_   | [**apiContractsCallPost**](docs/SmartContractsApi.md#apiContractsCallPost)                                     | **POST** /api/contracts/call                         | Call contract function                |
| _SmartContractsApi_   | [**apiContractsDeployPost**](docs/SmartContractsApi.md#apiContractsDeployPost)                                 | **POST** /api/contracts/deploy                       | Deploy smart contract                 |
| _SmartContractsApi_   | [**apiContractsEstimateGasPost**](docs/SmartContractsApi.md#apiContractsEstimateGasPost)                       | **POST** /api/contracts/estimate-gas                 | Estimate gas for contract call        |
| _SmartContractsApi_   | [**apiContractsQueryPost**](docs/SmartContractsApi.md#apiContractsQueryPost)                                   | **POST** /api/contracts/query                        | Query contract state                  |
| _SystemApi_           | [**apiHealthGet**](docs/SystemApi.md#apiHealthGet)                                                             | **GET** /api/health                                  | API health check                      |
| _SystemApi_           | [**healthGet**](docs/SystemApi.md#healthGet)                                                                   | **GET** /health                                      | System health check                   |
| _WalletApi_           | [**apiWalletGenerateMnemonicPost**](docs/WalletApi.md#apiWalletGenerateMnemonicPost)                           | **POST** /api/wallet/generate-mnemonic               | Generate mnemonic phrase              |
| _WalletApi_           | [**apiWalletHardwareDevicesGet**](docs/WalletApi.md#apiWalletHardwareDevicesGet)                               | **GET** /api/wallet/hardware/devices                 | List connected hardware wallets       |
| _WalletApi_           | [**apiWalletHardwareSignPost**](docs/WalletApi.md#apiWalletHardwareSignPost)                                   | **POST** /api/wallet/hardware/sign                   | Sign transaction with hardware wallet |
| _WalletApi_           | [**apiWalletValidateMnemonicPost**](docs/WalletApi.md#apiWalletValidateMnemonicPost)                           | **POST** /api/wallet/validate-mnemonic               | Validate mnemonic phrase              |
| _WeatherDataApi_      | [**apiWeatherDataGet**](docs/WeatherDataApi.md#apiWeatherDataGet)                                              | **GET** /api/weather/data                            | Get weather data                      |
| _WeatherDataApi_      | [**apiWeatherSecurityStatusGet**](docs/WeatherDataApi.md#apiWeatherSecurityStatusGet)                          | **GET** /api/weather/security-status                 | Get weather security status           |
| _WeatherDataApi_      | [**apiWeatherSubmitPost**](docs/WeatherDataApi.md#apiWeatherSubmitPost)                                        | **POST** /api/weather/submit                         | Submit weather data                   |

## Documentation for Models

- [ApiAimlInferencePost200Response](docs/ApiAimlInferencePost200Response.md)
- [ApiAimlInferencePostRequest](docs/ApiAimlInferencePostRequest.md)
- [ApiAimlModelsGet200Response](docs/ApiAimlModelsGet200Response.md)
- [ApiAimlModelsGet200ResponseModelsInner](docs/ApiAimlModelsGet200ResponseModelsInner.md)
- [ApiAimlModelsGet200ResponseModelsInnerPricing](docs/ApiAimlModelsGet200ResponseModelsInnerPricing.md)
- [ApiAimlModelsGet200ResponseModelsInnerRequirements](docs/ApiAimlModelsGet200ResponseModelsInnerRequirements.md)
- [ApiAnalyticsGet200Response](docs/ApiAnalyticsGet200Response.md)
- [ApiBlockchainBlocksGet200Response](docs/ApiBlockchainBlocksGet200Response.md)
- [ApiComplianceStatusGet200Response](docs/ApiComplianceStatusGet200Response.md)
- [ApiComputeProvidersGet200Response](docs/ApiComputeProvidersGet200Response.md)
- [ApiComputeProvidersGet200ResponseProvidersInner](docs/ApiComputeProvidersGet200ResponseProvidersInner.md)
- [ApiComputeProvidersGet200ResponseProvidersInnerResources](docs/ApiComputeProvidersGet200ResponseProvidersInnerResources.md)
- [ApiComputeTasksPost200Response](docs/ApiComputeTasksPost200Response.md)
- [ApiComputeTasksPostRequest](docs/ApiComputeTasksPostRequest.md)
- [ApiComputeTasksPostRequestResourceRequirements](docs/ApiComputeTasksPostRequestResourceRequirements.md)
- [ApiComputeTasksTaskIdStatusGet200Response](docs/ApiComputeTasksTaskIdStatusGet200Response.md)
- [ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage](docs/ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.md)
- [ApiContractsEstimateGasPost200Response](docs/ApiContractsEstimateGasPost200Response.md)
- [ApiIntegrationsLangchainEnablePost200Response](docs/ApiIntegrationsLangchainEnablePost200Response.md)
- [ApiIntegrationsLangchainEnablePost200ResponseData](docs/ApiIntegrationsLangchainEnablePost200ResponseData.md)
- [ApiIntegrationsLangchainInsightsGet200Response](docs/ApiIntegrationsLangchainInsightsGet200Response.md)
- [ApiIntegrationsLangchainInsightsGet200ResponseData](docs/ApiIntegrationsLangchainInsightsGet200ResponseData.md)
- [ApiIntegrationsLangchainStatusGet200Response](docs/ApiIntegrationsLangchainStatusGet200Response.md)
- [ApiMiningRewardsGet200Response](docs/ApiMiningRewardsGet200Response.md)
- [ApiPerformanceMetricsGet200Response](docs/ApiPerformanceMetricsGet200Response.md)
- [ApiPerformanceMetricsGet200ResponseMetrics](docs/ApiPerformanceMetricsGet200ResponseMetrics.md)
- [ApiPerformanceMetricsGet200ResponseMetricsLatency](docs/ApiPerformanceMetricsGet200ResponseMetricsLatency.md)
- [ApiPerformanceMetricsGet200ResponseMetricsThroughput](docs/ApiPerformanceMetricsGet200ResponseMetricsThroughput.md)
- [ApiPerformanceOptimizationPost200Response](docs/ApiPerformanceOptimizationPost200Response.md)
- [ApiPerformanceOptimizationPost200ResponseExpectedImprovements](docs/ApiPerformanceOptimizationPost200ResponseExpectedImprovements.md)
- [ApiPerformanceOptimizationPostRequest](docs/ApiPerformanceOptimizationPostRequest.md)
- [ApiSecurityStatsGet200Response](docs/ApiSecurityStatsGet200Response.md)
- [ApiWalletGenerateMnemonicPost200Response](docs/ApiWalletGenerateMnemonicPost200Response.md)
- [ApiWalletValidateMnemonicPost200Response](docs/ApiWalletValidateMnemonicPost200Response.md)
- [ApiWalletValidateMnemonicPostRequest](docs/ApiWalletValidateMnemonicPostRequest.md)
- [Block](docs/Block.md)
- [BlockchainStatus](docs/BlockchainStatus.md)
- [ComplianceStatus](docs/ComplianceStatus.md)
- [ComplianceStatusCertificationsInner](docs/ComplianceStatusCertificationsInner.md)
- [ComplianceStatusDataProtection](docs/ComplianceStatusDataProtection.md)
- [ContractCallRequest](docs/ContractCallRequest.md)
- [ContractCallResponse](docs/ContractCallResponse.md)
- [ContractDeployRequest](docs/ContractDeployRequest.md)
- [ContractDeployResponse](docs/ContractDeployResponse.md)
- [ContractQueryRequest](docs/ContractQueryRequest.md)
- [ContractQueryResponse](docs/ContractQueryResponse.md)
- [CreateProposalRequest](docs/CreateProposalRequest.md)
- [Error](docs/Error.md)
- [HardwareDevice](docs/HardwareDevice.md)
- [HardwareSignRequest](docs/HardwareSignRequest.md)
- [HardwareSignResponse](docs/HardwareSignResponse.md)
- [HealthGet200Response](docs/HealthGet200Response.md)
- [HealthStatus](docs/HealthStatus.md)
- [HealthStatusBlockchain](docs/HealthStatusBlockchain.md)
- [HealthStatusDatabase](docs/HealthStatusDatabase.md)
- [HealthStatusDatabaseConnectionPool](docs/HealthStatusDatabaseConnectionPool.md)
- [HealthStatusDatabaseQueryPerformance](docs/HealthStatusDatabaseQueryPerformance.md)
- [HealthStatusDatabaseStorage](docs/HealthStatusDatabaseStorage.md)
- [HealthStatusPerformance](docs/HealthStatusPerformance.md)
- [HealthStatusPerformanceCpu](docs/HealthStatusPerformanceCpu.md)
- [HealthStatusPerformanceNetwork](docs/HealthStatusPerformanceNetwork.md)
- [HealthStatusSecurity](docs/HealthStatusSecurity.md)
- [HealthStatusSecurityCertificates](docs/HealthStatusSecurityCertificates.md)
- [HealthStatusSecurityFirewall](docs/HealthStatusSecurityFirewall.md)
- [HealthStatusSecurityVulnerabilities](docs/HealthStatusSecurityVulnerabilities.md)
- [HealthStatusServicesInner](docs/HealthStatusServicesInner.md)
- [LangChainInsight](docs/LangChainInsight.md)
- [LangChainStatus](docs/LangChainStatus.md)
- [LangChainStatusAnalytics](docs/LangChainStatusAnalytics.md)
- [Location](docs/Location.md)
- [MiningReward](docs/MiningReward.md)
- [MiningRewardBlockchain](docs/MiningRewardBlockchain.md)
- [MiningRewardMultipliers](docs/MiningRewardMultipliers.md)
- [MiningRewardPerformance](docs/MiningRewardPerformance.md)
- [MiningRewardSource](docs/MiningRewardSource.md)
- [NetworkAnalytics](docs/NetworkAnalytics.md)
- [NetworkAnalyticsCrossChain](docs/NetworkAnalyticsCrossChain.md)
- [NetworkAnalyticsOverview](docs/NetworkAnalyticsOverview.md)
- [NetworkAnalyticsPerformance](docs/NetworkAnalyticsPerformance.md)
- [NetworkAnalyticsSecurity](docs/NetworkAnalyticsSecurity.md)
- [NetworkAnalyticsWeather](docs/NetworkAnalyticsWeather.md)
- [Pagination](docs/Pagination.md)
- [Proposal](docs/Proposal.md)
- [SecurityStats](docs/SecurityStats.md)
- [Transaction](docs/Transaction.md)
- [VoteRequest](docs/VoteRequest.md)
- [VoteResponse](docs/VoteResponse.md)
- [WeatherData](docs/WeatherData.md)
- [WeatherDataSubmission](docs/WeatherDataSubmission.md)
- [WeatherDataSubmissionQuality](docs/WeatherDataSubmissionQuality.md)
- [WeatherDataSubmissionSensorDataInner](docs/WeatherDataSubmissionSensorDataInner.md)
- [WeatherMeasurement](docs/WeatherMeasurement.md)
- [WeatherSecurityStatus](docs/WeatherSecurityStatus.md)
- [WeatherSecurityStatusApiSecurity](docs/WeatherSecurityStatusApiSecurity.md)
- [WeatherSecurityStatusDataValidation](docs/WeatherSecurityStatusDataValidation.md)
- [WeatherSubmissionResponse](docs/WeatherSubmissionResponse.md)
- [WeatherSubmissionResponseData](docs/WeatherSubmissionResponseData.md)
- [WeatherSubmissionResponseDataBlockchain](docs/WeatherSubmissionResponseDataBlockchain.md)
- [WeatherSubmissionResponseDataQuality](docs/WeatherSubmissionResponseDataQuality.md)
- [WeatherSubmissionResponseDataRewards](docs/WeatherSubmissionResponseDataRewards.md)
- [WeatherSubmissionResponseDataSensorsInner](docs/WeatherSubmissionResponseDataSensorsInner.md)

<a id="documentation-for-authorization"></a>

## Documentation for Authorization

Authentication schemes defined for the API: <a id="ApiKeyAuth"></a>

### ApiKeyAuth

- **Type**: API key
- **API key parameter name**: X-API-Key
- **Location**: HTTP header

<a id="BearerAuth"></a>

### BearerAuth

- **Type**: HTTP Bearer Token authentication (JWT)

## Recommendation

It's recommended to create an instance of `ApiClient` per thread in a multithreaded environment to
avoid any potential issues.

## Enterprise Features

### Enterprise Patterns

- **Connection Pooling**: Apache HttpClient 5 with configurable pool sizes
- **Retry Logic**: Resilience4j-powered automatic retry with exponential backoff
- **Circuit Breaker**: Fault tolerance with configurable failure thresholds
- **Comprehensive Logging**: SLF4J-based logging with enterprise monitoring hooks
- **Metrics Collection**: Built-in support for Micrometer metrics (optional)
- **Bean Validation**: Jakarta Validation for request/response validation

### Security & Authentication

- **API Key Authentication**: Header-based API key support (`X-API-Key`)
- **Bearer Token Authentication**: JWT token support with token suppliers
- **Secure Credential Management**: Optional integration patterns
- **Custom Headers**: Support for custom authentication and tracking headers

### Configuration Management

- **Environment-Based Configuration**: Production, staging, development presets
- **Builder Pattern**: Fluent configuration with validation
- **Multiple Authentication**: Support for both API keys and Bearer tokens
- **Custom Timeouts**: Configurable connection and read timeouts

## Enterprise Quick Start

### Development Setup

```java
import com.wrfcoin.sdk.WRFCoinSDK;

// Simple development setup with debug enabled
WRFCoinSDK sdk = WRFCoinSDK.forDevelopment("your-dev-api-key");

// Get weather data using enterprise features
var weather = sdk.getWeatherApi().apiWeatherDataGet();
System.out.println("Weather data retrieved with enterprise patterns");
```

### Production Setup

```java
import com.wrfcoin.sdk.WRFCoinSDK;

// Production setup with all enterprise features
WRFCoinSDK sdk = WRFCoinSDK.forProduction("your-prod-api-key");

// The SDK automatically enables:
// - Connection pooling for high performance
// - Circuit breaker for fault tolerance
// - Retry logic for transient failures
// - Compression for reduced bandwidth
// - Metrics collection for monitoring

var analytics = sdk.getAnalyticsApi().apiAnalyticsGet();
```

### Custom Enterprise Configuration

```java
import com.wrfcoin.sdk.WRFCoinConfiguration;
import com.wrfcoin.sdk.WRFCoinSDK;
import java.time.Duration;

WRFCoinConfiguration config = WRFCoinConfiguration.builder()
    .environment(WRFCoinConfiguration.Environment.PRODUCTION)
    .apiKey("your-api-key")
    .connectionTimeout(15000) // 15 second timeout
    .readTimeout(30000) // 30 second read timeout
    .maxConnectionsPerRoute(50) // High concurrency
    .maxTotalConnections(200)
    .connectionPoolingEnabled(true)
    .maxRetries(5) // Aggressive retry policy
    .retryDelay(Duration.ofSeconds(2))
    .circuitBreakerEnabled(true)
    .metricsEnabled(true)
    .customHeader("X-Client-Version", "1.0.0")
    .userAgent("MyApp/1.0 WRFCoin-SDK/1.0")
    .build();

WRFCoinSDK sdk = WRFCoinSDK.create(config);
```

## Enterprise Configuration Examples

### Connection Pooling

```java
WRFCoinConfiguration config = WRFCoinConfiguration.builder()
    .apiKey("your-api-key")
    .connectionPoolingEnabled(true)
    .maxConnectionsPerRoute(25) // Connections per endpoint
    .maxTotalConnections(100) // Total connection pool size
    .connectionTimeout(10000) // 10 second connection timeout
    .readTimeout(30000) // 30 second read timeout
    .keepAliveEnabled(true)
    .compressionEnabled(true) // Enable GZIP compression
    .build();
```

### Retry Logic

```java
WRFCoinConfiguration config = WRFCoinConfiguration.builder()
    .apiKey("your-api-key")
    .maxRetries(3) // Retry up to 3 times
    .retryDelay(Duration.ofSeconds(1)) // 1 second between retries
    .build();

// The SDK will automatically retry on:
// - IOException (network issues)
// - ApiException with retryable status codes (500, 502, 503, 504)
// - Connection timeouts and temporary failures
```

### Circuit Breaker

```java
WRFCoinConfiguration config = WRFCoinConfiguration.builder()
    .apiKey("your-api-key")
    .circuitBreakerEnabled(true)
    .build();

// Circuit breaker configuration:
// - 50% failure rate threshold
// - 30 second wait duration in open state
// - 10 call sliding window
// - Minimum 5 calls before circuit breaker activates
```

### Error Handling

```java
try {
    var weatherData = sdk.getWeatherDataApi().apiWeatherDataGet();

} catch (ApiException e) {
    // Handle API errors with enterprise context
    System.err.println("API Error: " + e.getMessage());
    System.err.println("Status Code: " + e.getCode());
    System.err.println("Response Body: " + e.getResponseBody());

    // Check enterprise features status
    sdk.getApiClient().getCircuitBreaker().ifPresent(cb -> {
        System.out.println("Circuit breaker state: " + cb.getState());
    });

} catch (Exception e) {
    System.err.println("Unexpected error: " + e.getMessage());
}
```

## Migration from Standard Client

```java
// Before (standard generated client)
ApiClient apiClient = Configuration.getDefaultApiClient();
apiClient.setBasePath("https://api.wrfcoin.com/v1");
ApiKeyAuth apiKey = (ApiKeyAuth) apiClient.getAuthentication("ApiKeyAuth");
apiKey.setApiKey("YOUR API KEY");
WeatherDataApi apiInstance = new WeatherDataApi(apiClient);

// After (enterprise SDK)
WRFCoinSDK sdk = WRFCoinSDK.forProduction("YOUR API KEY");
WeatherDataApi apiInstance = sdk.getWeatherDataApi();
// Enterprise features are automatically enabled
```

## Enterprise Dependencies

The enhanced SDK includes these additional enterprise dependencies:

```xml
<!-- Enterprise Dependencies -->
<dependency>
    <groupId>io.github.resilience4j</groupId>
    <artifactId>resilience4j-retry</artifactId>
    <version>2.1.0</version>
</dependency>
<dependency>
    <groupId>io.github.resilience4j</groupId>
    <artifactId>resilience4j-circuitbreaker</artifactId>
    <version>2.1.0</version>
</dependency>

<!-- Logging -->
<dependency>
    <groupId>org.slf4j</groupId>
    <artifactId>slf4j-api</artifactId>
    <version>2.0.9</version>
</dependency>

<!-- Validation -->
<dependency>
    <groupId>jakarta.validation</groupId>
    <artifactId>jakarta.validation-api</artifactId>
    <version>3.0.2</version>
</dependency>
```

## Performance Considerations

### Connection Pooling Best Practices

- **maxTotalConnections**: Set based on expected concurrent API calls (recommended: 100-200)
- **maxConnectionsPerRoute**: Set based on API endpoint distribution (recommended: 20-50)
- **Connection timeouts**: Balance responsiveness vs reliability (recommended: 10-30 seconds)
- **Keep-alive**: Always enable for better performance with connection reuse

### Retry Policy Best Practices

- **maxRetries**: Keep moderate to avoid cascading delays (recommended: 3-5)
- **retryDelay**: Use exponential backoff starting with 1-2 seconds
- **Exception handling**: Only retry on transient failures (network issues, 5xx errors)

### Circuit Breaker Best Practices

- **Failure threshold**: 50% is typically appropriate for most APIs
- **Wait duration**: 30-60 seconds allows time for service recovery
- **Sliding window**: 10-20 calls provides good statistical basis

## Author

developers@wrfcoin.com
