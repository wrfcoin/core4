# WRFCoin Java SDK - Quick Start Guide

Get up and running with the WRFCoin Java SDK in minutes. This guide covers installation,
configuration, and your first API call.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Basic Configuration](#basic-configuration)
- [First API Call](#first-api-call)
- [Authentication](#authentication)
- [Error Handling](#error-handling)
- [Spring Boot Integration](#spring-boot-integration)
- [Next Steps](#next-steps)

## Prerequisites

- **Java 8+** (Java 11+ recommended for production)
- **Maven 3.6+** or **Gradle 7.0+**
- **WRFCoin API Key** (get one at [WRFCoin Developer Portal](https://dev.wrfcoin.com))

## Installation

### Maven

Add the dependency to your `pom.xml`:

```xml
<dependency>
    <groupId>com.wrfcoin</groupId>
    <artifactId>wrfcoin-sdk</artifactId>
    <version>1.0.0</version>
</dependency>
```

### Gradle

Add the dependency to your `build.gradle`:

```groovy
implementation 'com.wrfcoin:wrfcoin-sdk:1.0.0'
```

### Manual Installation

Download the JAR from [releases](https://github.com/wrfcoin/java-sdk/releases) and add to your
classpath:

```bash
# Download and install
mvn install:install-file \
  -Dfile=wrfcoin-sdk-1.0.0.jar \
  -DgroupId=com.wrfcoin \
  -DartifactId=wrfcoin-sdk \
  -Dversion=1.0.0 \
  -Dpackaging=jar
```

## Basic Configuration

### Development Setup (Simplified)

```java
import com.wrfcoin.sdk.WRFCoinSDK;

public class QuickStartExample {
    public static void main(String[] args) {
        // Initialize SDK for development
        WRFCoinSDK sdk = WRFCoinSDK.forDevelopment("your-api-key-here");

        // SDK is now ready with default settings
        System.out.println("WRFCoin SDK initialized successfully!");
    }
}
```

### Production Setup (Recommended)

```java
import com.wrfcoin.sdk.WRFCoinSDK;

public class ProductionExample {
    public static void main(String[] args) {
        // Initialize SDK for production with enterprise features
        WRFCoinSDK sdk = WRFCoinSDK.forProduction("your-production-api-key");

        // Production features automatically enabled:
        // - Connection pooling
        // - Retry logic with exponential backoff
        // - Circuit breaker for fault tolerance
        // - Request/response compression

        System.out.println("Production SDK ready!");
    }
}
```

## First API Call

### Get System Health

```java
import com.wrfcoin.sdk.WRFCoinSDK;
import com.wrfcoin.sdk.model.HealthGet200Response;

public class HealthCheckExample {
    public static void main(String[] args) {
        try {
            // Initialize SDK
            WRFCoinSDK sdk = WRFCoinSDK.forDevelopment("your-api-key");

            // Make your first API call
            HealthGet200Response health = sdk.getSystemApi().healthGet();

            System.out.println("System Status: " + health.getData().getStatus());
            System.out.println("Uptime: " + health.getData().getUptime());

        } catch (Exception e) {
            System.err.println("Error: " + e.getMessage());
        }
    }
}
```

### Submit Weather Data

```java
import com.wrfcoin.sdk.WRFCoinSDK;
import com.wrfcoin.sdk.model.*;
import java.math.BigDecimal;
import java.util.Arrays;

public class WeatherSubmissionExample {
    public static void main(String[] args) {
        try {
            WRFCoinSDK sdk = WRFCoinSDK.forDevelopment("your-api-key");

            // Create location
            Location location = new Location()
                .latitude(new BigDecimal("40.7128"))  // New York City
                .longitude(new BigDecimal("-74.0060"))
                .altitude(new BigDecimal("10"));

            // Create sensor data
            WeatherDataSubmissionSensorDataInner temperatureSensor =
                new WeatherDataSubmissionSensorDataInner()
                    .type("temperature")
                    .value(new BigDecimal("23.5"))
                    .unit("celsius")
                    .status("active");

            WeatherDataSubmissionSensorDataInner humiditySensor =
                new WeatherDataSubmissionSensorDataInner()
                    .type("humidity")
                    .value(new BigDecimal("65.2"))
                    .unit("percent")
                    .status("active");

            // Create quality info
            WeatherDataSubmissionQuality quality =
                new WeatherDataSubmissionQuality()
                    .score(new BigDecimal("95"))
                    .issues(Arrays.asList());

            // Create weather submission
            WeatherDataSubmission submission = new WeatherDataSubmission()
                .stationId("my-weather-station-001")
                .location(location)
                .sensorData(Arrays.asList(temperatureSensor, humiditySensor))
                .quality(quality);

            // Submit weather data
            WeatherSubmissionResponse response = sdk.getWeatherDataApi()
                .apiWeatherSubmitPost(submission);

            if (response.getSuccess()) {
                System.out.println("Weather data submitted successfully!");
                System.out.println("Reward amount: " +
                    response.getData().getRewards().getTotalAmount());
                System.out.println("Transaction hash: " +
                    response.getData().getBlockchain().getTransactionHash());
            }

        } catch (Exception e) {
            System.err.println("Submission failed: " + e.getMessage());
        }
    }
}
```

### Query Network Analytics

```java
import com.wrfcoin.sdk.WRFCoinSDK;
import com.wrfcoin.sdk.model.ApiAnalyticsGet200Response;

public class AnalyticsExample {
    public static void main(String[] args) {
        try {
            WRFCoinSDK sdk = WRFCoinSDK.forDevelopment("your-api-key");

            // Get comprehensive network analytics
            ApiAnalyticsGet200Response analytics = sdk.getAnalyticsApi()
                .apiAnalyticsGet();

            var data = analytics.getData();

            System.out.println("Network Overview:");
            System.out.println("- Total Users: " + data.getOverview().getTotalUsers());
            System.out.println("- Active Users (24h): " + data.getOverview().getActiveUsers24h());
            System.out.println("- Network Utilization: " + data.getOverview().getNetworkUtilization() + "%");

            System.out.println("\nPerformance Metrics:");
            System.out.println("- Average Response Time: " + data.getPerformance().getAverageResponseTime() + "ms");
            System.out.println("- Throughput: " + data.getPerformance().getThroughput() + " req/sec");
            System.out.println("- Uptime: " + data.getPerformance().getUptime() + "%");

            System.out.println("\nWeather Network:");
            System.out.println("- Active Stations: " + data.getWeather().getActiveStations());
            System.out.println("- Data Submissions (24h): " + data.getWeather().getDataSubmissions24h());
            System.out.println("- Average Quality: " + data.getWeather().getAverageQuality());

        } catch (Exception e) {
            System.err.println("Analytics query failed: " + e.getMessage());
        }
    }
}
```

## Authentication

### API Key Authentication

```java
// Method 1: SDK builder (recommended)
WRFCoinSDK sdk = WRFCoinSDK.forProduction("your-api-key");

// Method 2: Configuration builder
WRFCoinConfiguration config = WRFCoinConfiguration.builder()
    .apiKey("your-api-key")
    .environment(WRFCoinConfiguration.Environment.PRODUCTION)
    .build();
WRFCoinSDK sdk = WRFCoinSDK.create(config);
```

### Bearer Token Authentication

```java
// For JWT tokens
WRFCoinConfiguration config = WRFCoinConfiguration.builder()
    .bearerToken("your-jwt-token")
    .environment(WRFCoinConfiguration.Environment.PRODUCTION)
    .build();
WRFCoinSDK sdk = WRFCoinSDK.create(config);
```

### Environment Variables

```bash
# Set environment variables
export WRFCOIN_API_KEY=your-api-key
export WRFCOIN_ENVIRONMENT=production
```

```java
// SDK will automatically pick up environment variables
WRFCoinSDK sdk = WRFCoinSDK.fromEnvironment();
```

### Credential Management

```java
// Token supplier for dynamic token refresh
WRFCoinConfiguration config = WRFCoinConfiguration.builder()
    .bearerTokenSupplier(() -> {
        // Your token refresh logic here
        return refreshAndGetNewToken();
    })
    .build();
```

## Error Handling

### Basic Error Handling

```java
import com.wrfcoin.sdk.ApiException;

try {
    var response = sdk.getSystemApi().healthGet();
    // Process successful response

} catch (ApiException e) {
    // Handle API-specific errors
    System.err.println("API Error: " + e.getMessage());
    System.err.println("Status Code: " + e.getCode());
    System.err.println("Response Body: " + e.getResponseBody());

    // Check specific error codes
    switch (e.getCode()) {
        case 401:
            System.err.println("Authentication failed - check your API key");
            break;
        case 429:
            System.err.println("Rate limit exceeded - please retry later");
            break;
        case 500:
            System.err.println("Server error - will retry automatically");
            break;
        default:
            System.err.println("Unexpected error code: " + e.getCode());
    }

} catch (Exception e) {
    // Handle network and other errors
    System.err.println("Network error: " + e.getMessage());
}
```

### Enterprise Error Handling

```java
import io.github.resilience4j.circuitbreaker.CircuitBreakerOpenException;

try {
    var response = sdk.getAnalyticsApi().apiAnalyticsGet();

} catch (CircuitBreakerOpenException e) {
    System.err.println("Circuit breaker is open - service temporarily unavailable");

    // Check circuit breaker state
    sdk.getApiClient().getCircuitBreaker().ifPresent(cb -> {
        System.out.println("Circuit breaker state: " + cb.getState());
        System.out.println("Failure rate: " + cb.getMetrics().getFailureRate());
    });

} catch (ApiException e) {
    System.err.println("API call failed after retries: " + e.getMessage());

} catch (Exception e) {
    System.err.println("Unexpected error: " + e.getMessage());
}
```

## Spring Boot Integration

### Maven Dependencies

```xml
<dependencies>
    <dependency>
        <groupId>com.wrfcoin</groupId>
        <artifactId>wrfcoin-sdk</artifactId>
        <version>1.0.0</version>
    </dependency>
    <dependency>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-starter</artifactId>
    </dependency>
</dependencies>
```

### Configuration Class

```java
import com.wrfcoin.sdk.WRFCoinSDK;
import com.wrfcoin.sdk.WRFCoinConfiguration;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;

@Configuration
public class WRFCoinConfig {

    @Value("${wrfcoin.api-key}")
    private String apiKey;

    @Value("${wrfcoin.environment:production}")
    private String environment;

    @Bean
    public WRFCoinSDK wrfCoinSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(apiKey)
            .environment(WRFCoinConfiguration.Environment.valueOf(environment.toUpperCase()))
            .connectionPoolingEnabled(true)
            .maxConnectionsPerRoute(50)
            .maxTotalConnections(200)
            .circuitBreakerEnabled(true)
            .maxRetries(3)
            .metricsEnabled(true)
            .build();

        return WRFCoinSDK.create(config);
    }
}
```

### Service Implementation

```java
import com.wrfcoin.sdk.WRFCoinSDK;
import com.wrfcoin.sdk.model.WeatherDataSubmission;
import com.wrfcoin.sdk.model.WeatherSubmissionResponse;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Service;

@Service
public class WeatherService {

    @Autowired
    private WRFCoinSDK wrfCoinSDK;

    public WeatherSubmissionResponse submitWeatherData(WeatherDataSubmission data) {
        try {
            return wrfCoinSDK.getWeatherDataApi().apiWeatherSubmitPost(data);
        } catch (Exception e) {
            throw new RuntimeException("Failed to submit weather data", e);
        }
    }

    public boolean isSystemHealthy() {
        try {
            var health = wrfCoinSDK.getSystemApi().healthGet();
            return "healthy".equals(health.getData().getStatus());
        } catch (Exception e) {
            return false;
        }
    }
}
```

### Application Properties

```properties
# application.properties
wrfcoin.api-key=${WRFCOIN_API_KEY:your-default-api-key}
wrfcoin.environment=production

# Optional: Configure HTTP client
wrfcoin.connection-timeout=15000
wrfcoin.read-timeout=30000
wrfcoin.max-connections=200
```

## Next Steps

### Essential Reading

1. **[Enterprise Integration Guide](ENTERPRISE.md)** - Advanced configuration, connection pooling,
   and production patterns
2. **[API Reference](docs/)** - Complete API documentation with examples
3. **[Security Best Practices](../docs/SECURITY.md)** - Credential management and secure usage
4. **[Performance Optimization](../docs/PERFORMANCE.md)** - Tuning for high-throughput applications

### Common Use Cases

1. **Weather Station Integration** - [See examples](src/main/java/com/wrfcoin/sdk/examples/)
2. **Blockchain Transactions** - Smart contract deployment and execution
3. **Cross-Chain Operations** - Multi-blockchain asset transfers
4. **DeFi Integration** - Yield farming and governance participation

### Advanced Features

- **Connection Pooling** - High-performance concurrent API calls
- **Circuit Breaker** - Fault tolerance and graceful degradation
- **Retry Logic** - Automatic recovery from transient failures
- **Metrics Collection** - Performance monitoring and alerting
- **Custom Headers** - Advanced authentication and tracking

### Community and Support

- **Documentation**: [docs.wrfcoin.com](https://docs.wrfcoin.com)
- **Developer Portal**: [dev.wrfcoin.com](https://dev.wrfcoin.com)
- **GitHub Issues**:
  [github.com/wrfcoin/java-sdk/issues](https://github.com/wrfcoin/java-sdk/issues)
- **Discord Community**: [discord.gg/wrfcoin](https://discord.gg/wrfcoin)
- **Email Support**: [developers@wrfcoin.com](mailto:developers@wrfcoin.com)

---

**Ready to build with WRFCoin?** Start with the examples above and check out our
[Enterprise Integration Guide](ENTERPRISE.md) for production-ready patterns.
