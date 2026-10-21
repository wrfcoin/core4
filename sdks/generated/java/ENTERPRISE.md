# WRFCoin Java SDK - Enterprise Integration Guide

This guide covers advanced enterprise patterns, production configurations, and best practices for
integrating the WRFCoin Java SDK into enterprise applications.

## Table of Contents

- [Enterprise Architecture](#enterprise-architecture)
- [Connection Pooling](#connection-pooling)
- [Retry and Circuit Breaker Patterns](#retry-and-circuit-breaker-patterns)
- [Logging and Monitoring](#logging-and-monitoring)
- [Security Best Practices](#security-best-practices)
- [Performance Optimization](#performance-optimization)
- [Spring Boot Integration](#spring-boot-integration)
- [Microservices Patterns](#microservices-patterns)
- [Testing Strategies](#testing-strategies)
- [Production Deployment](#production-deployment)

## Enterprise Architecture

### High-Availability Configuration

```java
import com.wrfcoin.sdk.WRFCoinConfiguration;
import com.wrfcoin.sdk.WRFCoinSDK;
import java.time.Duration;

public class EnterpriseConfiguration {

    public static WRFCoinSDK createHighAvailabilitySDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            // Environment and authentication
            .environment(WRFCoinConfiguration.Environment.PRODUCTION)
            .apiKey(System.getenv("WRFCOIN_API_KEY"))

            // High-performance connection pooling
            .connectionPoolingEnabled(true)
            .maxTotalConnections(500)
            .maxConnectionsPerRoute(100)
            .connectionTimeout(15000)
            .readTimeout(45000)
            .keepAliveEnabled(true)
            .compressionEnabled(true)

            // Aggressive retry policy for critical operations
            .maxRetries(5)
            .retryDelay(Duration.ofSeconds(2))
            .retryDelayMultiplier(2.0)
            .maxRetryDelay(Duration.ofMinutes(1))

            // Circuit breaker for fault tolerance
            .circuitBreakerEnabled(true)
            .circuitBreakerFailureThreshold(60) // 60% failure rate
            .circuitBreakerWaitDuration(Duration.ofSeconds(45))
            .circuitBreakerMinimumCalls(20)

            // Monitoring and observability
            .metricsEnabled(true)
            .detailedLoggingEnabled(true)
            .requestTrackingEnabled(true)

            // Custom headers for tracking
            .customHeader("X-Service-Name", "enterprise-weather-service")
            .customHeader("X-Service-Version", "1.2.0")
            .customHeader("X-Environment", "production")

            .build();

        return WRFCoinSDK.create(config);
    }
}
```

### Multi-Environment Configuration

```java
public class EnvironmentSpecificConfiguration {

    public enum DeploymentEnvironment {
        DEVELOPMENT, STAGING, PRODUCTION
    }

    public static WRFCoinSDK createForEnvironment(DeploymentEnvironment env) {
        WRFCoinConfiguration.Builder builder = WRFCoinConfiguration.builder();

        switch (env) {
            case DEVELOPMENT:
                return configureForDevelopment(builder);
            case STAGING:
                return configureForStaging(builder);
            case PRODUCTION:
                return configureForProduction(builder);
            default:
                throw new IllegalArgumentException("Unknown environment: " + env);
        }
    }

    private static WRFCoinSDK configureForDevelopment(WRFCoinConfiguration.Builder builder) {
        return WRFCoinSDK.create(builder
            .environment(WRFCoinConfiguration.Environment.DEVELOPMENT)
            .apiKey(System.getenv("WRFCOIN_DEV_API_KEY"))
            .baseUrl("https://dev-api.wrfcoin.com/v1")
            .connectionTimeout(30000) // Longer timeouts for debugging
            .readTimeout(60000)
            .maxRetries(1) // Minimal retries for faster feedback
            .detailedLoggingEnabled(true)
            .metricsEnabled(false)
            .build());
    }

    private static WRFCoinSDK configureForStaging(WRFCoinConfiguration.Builder builder) {
        return WRFCoinSDK.create(builder
            .environment(WRFCoinConfiguration.Environment.STAGING)
            .apiKey(System.getenv("WRFCOIN_STAGING_API_KEY"))
            .baseUrl("https://staging-api.wrfcoin.com/v1")
            .connectionPoolingEnabled(true)
            .maxTotalConnections(100)
            .maxConnectionsPerRoute(25)
            .maxRetries(3)
            .circuitBreakerEnabled(true)
            .metricsEnabled(true)
            .build());
    }

    private static WRFCoinSDK configureForProduction(WRFCoinConfiguration.Builder builder) {
        return WRFCoinSDK.create(builder
            .environment(WRFCoinConfiguration.Environment.PRODUCTION)
            .apiKey(System.getenv("WRFCOIN_PROD_API_KEY"))
            .baseUrl("https://api.wrfcoin.com/v1")
            .connectionPoolingEnabled(true)
            .maxTotalConnections(500)
            .maxConnectionsPerRoute(100)
            .keepAliveEnabled(true)
            .compressionEnabled(true)
            .maxRetries(5)
            .retryDelay(Duration.ofSeconds(2))
            .circuitBreakerEnabled(true)
            .metricsEnabled(true)
            .customHeader("X-Client-Type", "enterprise")
            .build());
    }
}
```

## Connection Pooling

### Optimal Pool Configuration

```java
public class ConnectionPoolOptimization {

    /**
     * Configure connection pooling for high-throughput applications
     */
    public static WRFCoinSDK createHighThroughputSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(System.getenv("WRFCOIN_API_KEY"))

            // Connection pool sizing based on concurrent users
            .connectionPoolingEnabled(true)
            .maxTotalConnections(1000) // Total pool size
            .maxConnectionsPerRoute(200) // Per-endpoint connections

            // Connection lifecycle management
            .connectionTimeout(10000) // 10 second connection timeout
            .readTimeout(30000) // 30 second read timeout
            .keepAliveEnabled(true) // Reuse connections
            .connectionIdleTimeout(Duration.ofMinutes(5))
            .connectionTimeToLive(Duration.ofMinutes(30))

            // Request optimization
            .compressionEnabled(true) // GZIP compression
            .requestBufferingEnabled(false) // Stream large requests
            .responseBufferingEnabled(false) // Stream large responses

            .build();

        return WRFCoinSDK.create(config);
    }

    /**
     * Monitor connection pool health
     */
    public static void monitorConnectionPool(WRFCoinSDK sdk) {
        // Get connection pool metrics (if available)
        sdk.getApiClient().getConnectionPoolMetrics().ifPresent(metrics -> {
            System.out.println("Connection Pool Metrics:");
            System.out.println("- Total Connections: " + metrics.getTotalConnections());
            System.out.println("- Active Connections: " + metrics.getActiveConnections());
            System.out.println("- Idle Connections: " + metrics.getIdleConnections());
            System.out.println("- Pending Requests: " + metrics.getPendingRequests());
        });
    }
}
```

### Connection Pool Monitoring

```java
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;

public class ConnectionPoolMonitor {

    private final WRFCoinSDK sdk;
    private final ScheduledExecutorService scheduler;

    public ConnectionPoolMonitor(WRFCoinSDK sdk) {
        this.sdk = sdk;
        this.scheduler = Executors.newScheduledThreadPool(1);
    }

    public void startMonitoring() {
        scheduler.scheduleAtFixedRate(this::logConnectionPoolStats, 0, 30, TimeUnit.SECONDS);
    }

    private void logConnectionPoolStats() {
        sdk.getApiClient().getConnectionPoolMetrics().ifPresent(metrics -> {
            if (metrics.getActiveConnections() > metrics.getTotalConnections() * 0.8) {
                System.warn("Connection pool utilization high: " +
                    metrics.getActiveConnections() + "/" + metrics.getTotalConnections());
            }

            if (metrics.getPendingRequests() > 10) {
                System.warn("High number of pending requests: " + metrics.getPendingRequests());
            }
        });
    }

    public void shutdown() {
        scheduler.shutdown();
    }
}
```

## Retry and Circuit Breaker Patterns

### Advanced Retry Configuration

```java
import io.github.resilience4j.retry.Retry;
import io.github.resilience4j.retry.RetryConfig;
import java.time.Duration;

public class AdvancedRetryPatterns {

    /**
     * Configure intelligent retry with exponential backoff
     */
    public static WRFCoinSDK createResilientSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(System.getenv("WRFCOIN_API_KEY"))

            // Retry configuration
            .maxRetries(5)
            .retryDelay(Duration.ofSeconds(1))
            .retryDelayMultiplier(2.0) // Exponential backoff
            .maxRetryDelay(Duration.ofMinutes(2))
            .retryJitterEnabled(true) // Add randomness to avoid thundering herd

            // Custom retry conditions
            .retryOnException(exception -> {
                // Retry on network errors
                if (exception instanceof java.net.SocketTimeoutException) return true;
                if (exception instanceof java.net.ConnectException) return true;

                // Retry on specific API errors
                if (exception instanceof com.wrfcoin.sdk.ApiException) {
                    com.wrfcoin.sdk.ApiException apiEx = (com.wrfcoin.sdk.ApiException) exception;
                    return apiEx.getCode() >= 500 || apiEx.getCode() == 429; // Server errors or rate limits
                }

                return false;
            })

            .build();

        return WRFCoinSDK.create(config);
    }

    /**
     * Custom retry listener for monitoring
     */
    public static void configureRetryMonitoring(WRFCoinSDK sdk) {
        sdk.getApiClient().getRetry().ifPresent(retry -> {
            retry.getEventPublisher().onRetry(event -> {
                System.out.println("Retry attempt " + event.getNumberOfRetryAttempts() +
                    " for " + event.getName() + ": " + event.getLastThrowable().getMessage());
            });

            retry.getEventPublisher().onSuccess(event -> {
                System.out.println("Operation succeeded after " +
                    event.getNumberOfRetryAttempts() + " retries");
            });

            retry.getEventPublisher().onError(event -> {
                System.err.println("Operation failed after " +
                    event.getNumberOfRetryAttempts() + " retries: " +
                    event.getLastThrowable().getMessage());
            });
        });
    }
}
```

### Circuit Breaker Implementation

```java
import io.github.resilience4j.circuitbreaker.CircuitBreaker;
import io.github.resilience4j.circuitbreaker.CircuitBreakerConfig;

public class CircuitBreakerPatterns {

    /**
     * Configure circuit breaker for service protection
     */
    public static WRFCoinSDK createProtectedSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(System.getenv("WRFCOIN_API_KEY"))

            // Circuit breaker configuration
            .circuitBreakerEnabled(true)
            .circuitBreakerFailureThreshold(50) // 50% failure rate
            .circuitBreakerSlowCallThreshold(60) // 60% slow call rate
            .circuitBreakerSlowCallDuration(Duration.ofSeconds(5))
            .circuitBreakerWaitDuration(Duration.ofSeconds(30))
            .circuitBreakerMinimumCalls(10)
            .circuitBreakerSlidingWindowSize(20)

            .build();

        return WRFCoinSDK.create(config);
    }

    /**
     * Monitor circuit breaker state
     */
    public static void monitorCircuitBreaker(WRFCoinSDK sdk) {
        sdk.getApiClient().getCircuitBreaker().ifPresent(cb -> {
            cb.getEventPublisher().onStateTransition(event -> {
                System.out.println("Circuit breaker state transition: " +
                    event.getStateTransition());
            });

            cb.getEventPublisher().onCallNotPermitted(event -> {
                System.warn("Circuit breaker blocked call to: " + event.getCreationTime());
            });

            cb.getEventPublisher().onError(event -> {
                System.err.println("Circuit breaker recorded error: " +
                    event.getThrowable().getMessage());
            });
        });
    }

    /**
     * Manual circuit breaker control
     */
    public static void manageCircuitBreaker(WRFCoinSDK sdk) {
        sdk.getApiClient().getCircuitBreaker().ifPresent(cb -> {
            // Check current state
            CircuitBreaker.State currentState = cb.getState();
            System.out.println("Current circuit breaker state: " + currentState);

            // Get metrics
            var metrics = cb.getMetrics();
            System.out.println("Failure rate: " + metrics.getFailureRate() + "%");
            System.out.println("Number of calls: " + metrics.getNumberOfBufferedCalls());
            System.out.println("Number of failed calls: " + metrics.getNumberOfFailedCalls());

            // Manual control (use with caution)
            if (currentState == CircuitBreaker.State.OPEN) {
                // Optionally transition to half-open for testing
                cb.transitionToHalfOpenState();
            }
        });
    }
}
```

## Logging and Monitoring

### Structured Logging Configuration

```java
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.slf4j.MDC;

public class EnterpriseLogging {

    private static final Logger logger = LoggerFactory.getLogger(EnterpriseLogging.class);

    /**
     * Configure SDK with enterprise logging
     */
    public static WRFCoinSDK createMonitoredSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(System.getenv("WRFCOIN_API_KEY"))

            // Enable detailed logging
            .detailedLoggingEnabled(true)
            .requestTrackingEnabled(true)
            .responseLoggingEnabled(false) // Disable in production for security

            // Custom request interceptor for structured logging
            .requestInterceptor((request) -> {
                String requestId = generateRequestId();
                MDC.put("requestId", requestId);
                MDC.put("apiEndpoint", request.getURI().getPath());
                MDC.put("httpMethod", request.getMethod());

                logger.info("Starting API request: {} {}",
                    request.getMethod(), request.getURI());

                return request;
            })

            // Custom response interceptor
            .responseInterceptor((response) -> {
                long duration = response.getExecutionTime();
                int statusCode = response.getStatusCode();

                MDC.put("statusCode", String.valueOf(statusCode));
                MDC.put("duration", String.valueOf(duration));

                if (statusCode >= 200 && statusCode < 300) {
                    logger.info("API request completed successfully in {}ms", duration);
                } else {
                    logger.warn("API request failed with status {} in {}ms", statusCode, duration);
                }

                // Clear MDC
                MDC.clear();

                return response;
            })

            .build();

        return WRFCoinSDK.create(config);
    }

    private static String generateRequestId() {
        return java.util.UUID.randomUUID().toString().substring(0, 8);
    }
}
```

### Metrics Collection Integration

```java
import io.micrometer.core.instrument.MeterRegistry;
import io.micrometer.core.instrument.Timer;
import io.micrometer.core.instrument.Counter;

public class MetricsIntegration {

    private final MeterRegistry meterRegistry;
    private final Timer apiCallTimer;
    private final Counter successCounter;
    private final Counter errorCounter;

    public MetricsIntegration(MeterRegistry meterRegistry) {
        this.meterRegistry = meterRegistry;
        this.apiCallTimer = Timer.builder("wrfcoin.api.calls")
            .description("Duration of WRFCoin API calls")
            .register(meterRegistry);
        this.successCounter = Counter.builder("wrfcoin.api.success")
            .description("Successful API calls")
            .register(meterRegistry);
        this.errorCounter = Counter.builder("wrfcoin.api.errors")
            .description("Failed API calls")
            .register(meterRegistry);
    }

    public WRFCoinSDK createInstrumentedSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(System.getenv("WRFCOIN_API_KEY"))
            .metricsEnabled(true)

            // Custom metrics collection
            .requestInterceptor(request -> {
                Timer.Sample sample = Timer.start(meterRegistry);
                request.setAttribute("timer.sample", sample);
                return request;
            })

            .responseInterceptor(response -> {
                Timer.Sample sample = (Timer.Sample) response.getRequest()
                    .getAttribute("timer.sample");

                if (sample != null) {
                    sample.stop(apiCallTimer);
                }

                if (response.getStatusCode() >= 200 && response.getStatusCode() < 300) {
                    successCounter.increment();
                } else {
                    errorCounter.increment(
                        "status", String.valueOf(response.getStatusCode())
                    );
                }

                return response;
            })

            .build();

        return WRFCoinSDK.create(config);
    }
}
```

## Security Best Practices

### Secure Credential Management

```java
import javax.crypto.Cipher;
import javax.crypto.spec.SecretKeySpec;
import java.util.Base64;

public class SecureCredentialManagement {

    /**
     * Production-safe SDK configuration with encrypted credentials
     */
    public static WRFCoinSDK createSecureSDK() {
        // Use environment variables or secure key management service
        String apiKey = getDecryptedApiKey();

        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(apiKey)

            // Security headers
            .customHeader("X-Client-Version", getApplicationVersion())
            .customHeader("X-Request-Source", "enterprise-application")

            // Enable security features
            .tlsVerificationEnabled(true)
            .certificatePinningEnabled(true) // If supported

            // Token refresh for long-running applications
            .bearerTokenSupplier(() -> {
                // Implement your token refresh logic
                return refreshJWTToken();
            })

            .build();

        return WRFCoinSDK.create(config);
    }

    private static String getDecryptedApiKey() {
        // Implement secure key retrieval
        String encryptedKey = System.getenv("WRFCOIN_ENCRYPTED_API_KEY");
        String decryptionKey = System.getenv("WRFCOIN_DECRYPTION_KEY");

        if (encryptedKey == null || decryptionKey == null) {
            throw new IllegalStateException("Missing encrypted credentials");
        }

        return decryptString(encryptedKey, decryptionKey);
    }

    private static String decryptString(String encryptedText, String key) {
        try {
            SecretKeySpec secretKey = new SecretKeySpec(key.getBytes(), "AES");
            Cipher cipher = Cipher.getInstance("AES");
            cipher.init(Cipher.DECRYPT_MODE, secretKey);
            byte[] decryptedBytes = cipher.doFinal(Base64.getDecoder().decode(encryptedText));
            return new String(decryptedBytes);
        } catch (Exception e) {
            throw new RuntimeException("Failed to decrypt API key", e);
        }
    }

    private static String refreshJWTToken() {
        // Implement JWT token refresh logic
        // This would typically call your authentication service
        return "refreshed-jwt-token";
    }

    private static String getApplicationVersion() {
        return "1.0.0"; // Read from manifest or properties
    }
}
```

### Request Signing and Authentication

```java
import java.security.MessageDigest;
import java.time.Instant;

public class RequestAuthentication {

    /**
     * Configure SDK with request signing for enhanced security
     */
    public static WRFCoinSDK createSignedRequestSDK() {
        String apiKey = System.getenv("WRFCOIN_API_KEY");
        String apiSecret = System.getenv("WRFCOIN_API_SECRET");

        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(apiKey)

            // Add request signing
            .requestInterceptor(request -> {
                long timestamp = Instant.now().getEpochSecond();
                String signature = generateSignature(request, timestamp, apiSecret);

                request.addHeader("X-Timestamp", String.valueOf(timestamp));
                request.addHeader("X-Signature", signature);

                return request;
            })

            .build();

        return WRFCoinSDK.create(config);
    }

    private static String generateSignature(Object request, long timestamp, String secret) {
        try {
            // Create signature payload
            String payload = request.getMethod() + request.getURI().getPath() + timestamp;

            // Create HMAC signature
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            byte[] hash = digest.digest((payload + secret).getBytes());

            return Base64.getEncoder().encodeToString(hash);
        } catch (Exception e) {
            throw new RuntimeException("Failed to generate request signature", e);
        }
    }
}
```

## Performance Optimization

### High-Performance Configuration

```java
public class PerformanceOptimization {

    /**
     * Configure SDK for maximum performance
     */
    public static WRFCoinSDK createHighPerformanceSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(System.getenv("WRFCOIN_API_KEY"))

            // Optimize connection pooling
            .connectionPoolingEnabled(true)
            .maxTotalConnections(1000)
            .maxConnectionsPerRoute(200)
            .connectionTimeout(5000) // Aggressive timeouts
            .readTimeout(15000)
            .keepAliveEnabled(true)

            // Enable compression
            .compressionEnabled(true)
            .requestCompressionThreshold(1024) // Compress requests > 1KB

            // Optimize serialization
            .serializationPoolingEnabled(true)
            .objectMapperShared(true)

            // Disable expensive operations in production
            .requestLoggingEnabled(false)
            .responseLoggingEnabled(false)
            .detailedLoggingEnabled(false)

            // Optimize retry behavior
            .maxRetries(2) // Quick retries only
            .retryDelay(Duration.ofMillis(500))

            .build();

        return WRFCoinSDK.create(config);
    }
}
```

### Async and Reactive Patterns

```java
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.Executor;
import java.util.concurrent.ForkJoinPool;

public class AsyncPatterns {

    private final WRFCoinSDK sdk;
    private final Executor asyncExecutor;

    public AsyncPatterns(WRFCoinSDK sdk) {
        this.sdk = sdk;
        this.asyncExecutor = ForkJoinPool.commonPool();
    }

    /**
     * Async weather data submission
     */
    public CompletableFuture<WeatherSubmissionResponse> submitWeatherDataAsync(
            WeatherDataSubmission data) {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return sdk.getWeatherDataApi().apiWeatherSubmitPost(data);
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        }, asyncExecutor);
    }

    /**
     * Parallel data processing
     */
    public CompletableFuture<List<WeatherSubmissionResponse>> submitBatchWeatherData(
            List<WeatherDataSubmission> batch) {

        List<CompletableFuture<WeatherSubmissionResponse>> futures = batch.stream()
            .map(this::submitWeatherDataAsync)
            .collect(Collectors.toList());

        return CompletableFuture.allOf(futures.toArray(new CompletableFuture[0]))
            .thenApply(v -> futures.stream()
                .map(CompletableFuture::join)
                .collect(Collectors.toList()));
    }

    /**
     * Async with timeout and fallback
     */
    public CompletableFuture<ApiAnalyticsGet200Response> getAnalyticsWithFallback() {
        return CompletableFuture.supplyAsync(() -> {
            try {
                return sdk.getAnalyticsApi().apiAnalyticsGet();
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        }, asyncExecutor)
        .orTimeout(30, TimeUnit.SECONDS)
        .exceptionally(throwable -> {
            // Return cached or default analytics data
            return getCachedAnalytics();
        });
    }

    private ApiAnalyticsGet200Response getCachedAnalytics() {
        // Return cached or default response
        return new ApiAnalyticsGet200Response(); // Implement caching logic
    }
}
```

## Spring Boot Integration

### Comprehensive Spring Configuration

```java
import com.wrfcoin.sdk.WRFCoinSDK;
import com.wrfcoin.sdk.WRFCoinConfiguration;
import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.context.annotation.Primary;

@Configuration
@ConfigurationProperties(prefix = "wrfcoin")
public class WRFCoinSpringConfiguration {

    private String apiKey;
    private String environment = "production";
    private int connectionTimeout = 15000;
    private int readTimeout = 30000;
    private int maxConnections = 200;
    private int maxConnectionsPerRoute = 50;
    private boolean metricsEnabled = true;
    private boolean circuitBreakerEnabled = true;
    private int maxRetries = 3;

    @Bean
    @Primary
    public WRFCoinSDK wrfCoinSDK() {
        WRFCoinConfiguration config = WRFCoinConfiguration.builder()
            .apiKey(apiKey)
            .environment(WRFCoinConfiguration.Environment.valueOf(environment.toUpperCase()))
            .connectionTimeout(connectionTimeout)
            .readTimeout(readTimeout)
            .connectionPoolingEnabled(true)
            .maxTotalConnections(maxConnections)
            .maxConnectionsPerRoute(maxConnectionsPerRoute)
            .keepAliveEnabled(true)
            .compressionEnabled(true)
            .maxRetries(maxRetries)
            .circuitBreakerEnabled(circuitBreakerEnabled)
            .metricsEnabled(metricsEnabled)
            .customHeader("X-Service-Name", "spring-weather-service")
            .build();

        return WRFCoinSDK.create(config);
    }

    // Health indicator for Spring Boot Actuator
    @Bean
    public WRFCoinHealthIndicator wrfCoinHealthIndicator(WRFCoinSDK sdk) {
        return new WRFCoinHealthIndicator(sdk);
    }

    // Getters and setters for configuration properties
    public String getApiKey() { return apiKey; }
    public void setApiKey(String apiKey) { this.apiKey = apiKey; }

    public String getEnvironment() { return environment; }
    public void setEnvironment(String environment) { this.environment = environment; }

    public int getConnectionTimeout() { return connectionTimeout; }
    public void setConnectionTimeout(int connectionTimeout) { this.connectionTimeout = connectionTimeout; }

    public int getReadTimeout() { return readTimeout; }
    public void setReadTimeout(int readTimeout) { this.readTimeout = readTimeout; }

    public int getMaxConnections() { return maxConnections; }
    public void setMaxConnections(int maxConnections) { this.maxConnections = maxConnections; }

    public int getMaxConnectionsPerRoute() { return maxConnectionsPerRoute; }
    public void setMaxConnectionsPerRoute(int maxConnectionsPerRoute) {
        this.maxConnectionsPerRoute = maxConnectionsPerRoute;
    }

    public boolean isMetricsEnabled() { return metricsEnabled; }
    public void setMetricsEnabled(boolean metricsEnabled) { this.metricsEnabled = metricsEnabled; }

    public boolean isCircuitBreakerEnabled() { return circuitBreakerEnabled; }
    public void setCircuitBreakerEnabled(boolean circuitBreakerEnabled) {
        this.circuitBreakerEnabled = circuitBreakerEnabled;
    }

    public int getMaxRetries() { return maxRetries; }
    public void setMaxRetries(int maxRetries) { this.maxRetries = maxRetries; }
}
```

### Spring Boot Health Indicator

```java
import org.springframework.boot.actuator.health.Health;
import org.springframework.boot.actuator.health.HealthIndicator;
import org.springframework.stereotype.Component;

@Component
public class WRFCoinHealthIndicator implements HealthIndicator {

    private final WRFCoinSDK sdk;

    public WRFCoinHealthIndicator(WRFCoinSDK sdk) {
        this.sdk = sdk;
    }

    @Override
    public Health health() {
        try {
            var healthResponse = sdk.getSystemApi().healthGet();

            if ("healthy".equals(healthResponse.getData().getStatus())) {
                return Health.up()
                    .withDetail("status", healthResponse.getData().getStatus())
                    .withDetail("uptime", healthResponse.getData().getUptime())
                    .withDetail("version", healthResponse.getData().getVersion())
                    .build();
            } else {
                return Health.down()
                    .withDetail("status", healthResponse.getData().getStatus())
                    .withDetail("reason", "WRFCoin API reported degraded status")
                    .build();
            }
        } catch (Exception e) {
            return Health.down()
                .withDetail("error", e.getMessage())
                .withDetail("reason", "Failed to connect to WRFCoin API")
                .build();
        }
    }
}
```

### Configuration Properties

```yaml
# application.yml
wrfcoin:
  api-key: ${WRFCOIN_API_KEY}
  environment: production
  connection-timeout: 15000
  read-timeout: 30000
  max-connections: 200
  max-connections-per-route: 50
  metrics-enabled: true
  circuit-breaker-enabled: true
  max-retries: 3

management:
  endpoints:
    web:
      exposure:
        include: health,metrics
  endpoint:
    health:
      show-details: always

logging:
  level:
    com.wrfcoin.sdk: INFO
    io.github.resilience4j: DEBUG
```

## Production Deployment

### Deployment Checklist

```java
/**
 * Production deployment validation
 */
public class ProductionDeploymentValidator {

    public static void validateConfiguration(WRFCoinSDK sdk) {
        System.out.println("=== WRFCoin SDK Production Validation ===");

        // 1. Validate API connectivity
        validateAPIConnectivity(sdk);

        // 2. Validate authentication
        validateAuthentication(sdk);

        // 3. Validate enterprise features
        validateEnterpriseFeatures(sdk);

        // 4. Performance baseline
        validatePerformance(sdk);

        System.out.println("=== Validation Complete ===");
    }

    private static void validateAPIConnectivity(WRFCoinSDK sdk) {
        try {
            var health = sdk.getSystemApi().healthGet();
            if ("healthy".equals(health.getData().getStatus())) {
                System.out.println("✓ API connectivity verified");
            } else {
                System.err.println("✗ API health check failed: " + health.getData().getStatus());
            }
        } catch (Exception e) {
            System.err.println("✗ Failed to connect to API: " + e.getMessage());
        }
    }

    private static void validateAuthentication(WRFCoinSDK sdk) {
        try {
            // Test authenticated endpoint
            sdk.getAnalyticsApi().apiAnalyticsGet();
            System.out.println("✓ Authentication verified");
        } catch (Exception e) {
            System.err.println("✗ Authentication failed: " + e.getMessage());
        }
    }

    private static void validateEnterpriseFeatures(WRFCoinSDK sdk) {
        // Validate connection pooling
        if (sdk.getApiClient().isConnectionPoolingEnabled()) {
            System.out.println("✓ Connection pooling enabled");
        } else {
            System.out.println("⚠ Connection pooling disabled");
        }

        // Validate circuit breaker
        if (sdk.getApiClient().getCircuitBreaker().isPresent()) {
            System.out.println("✓ Circuit breaker enabled");
        } else {
            System.out.println("⚠ Circuit breaker disabled");
        }

        // Validate retry configuration
        if (sdk.getApiClient().getRetry().isPresent()) {
            System.out.println("✓ Retry logic enabled");
        } else {
            System.out.println("⚠ Retry logic disabled");
        }
    }

    private static void validatePerformance(WRFCoinSDK sdk) {
        long startTime = System.currentTimeMillis();
        try {
            sdk.getSystemApi().healthGet();
            long responseTime = System.currentTimeMillis() - startTime;

            if (responseTime < 1000) {
                System.out.println("✓ Performance baseline met: " + responseTime + "ms");
            } else {
                System.out.println("⚠ High response time: " + responseTime + "ms");
            }
        } catch (Exception e) {
            System.err.println("✗ Performance test failed: " + e.getMessage());
        }
    }
}
```

### Resource Management

```java
/**
 * Proper resource management for enterprise applications
 */
public class ResourceManagement {

    private final WRFCoinSDK sdk;
    private final ScheduledExecutorService healthCheckExecutor;

    public ResourceManagement() {
        this.sdk = createManagedSDK();
        this.healthCheckExecutor = Executors.newScheduledThreadPool(1);

        // Schedule regular health checks
        scheduleHealthChecks();

        // Register shutdown hook
        Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown));
    }

    private WRFCoinSDK createManagedSDK() {
        return WRFCoinSDK.create(WRFCoinConfiguration.builder()
            .apiKey(System.getenv("WRFCOIN_API_KEY"))
            .connectionPoolingEnabled(true)
            .maxTotalConnections(100)
            .maxConnectionsPerRoute(25)
            .connectionTimeout(15000)
            .readTimeout(30000)
            .circuitBreakerEnabled(true)
            .maxRetries(3)
            .build());
    }

    private void scheduleHealthChecks() {
        healthCheckExecutor.scheduleAtFixedRate(() -> {
            try {
                var health = sdk.getSystemApi().healthGet();
                if (!"healthy".equals(health.getData().getStatus())) {
                    System.warn("WRFCoin API health degraded: " + health.getData().getStatus());
                }
            } catch (Exception e) {
                System.err.println("Health check failed: " + e.getMessage());
            }
        }, 0, 30, TimeUnit.SECONDS);
    }

    public void shutdown() {
        System.out.println("Shutting down WRFCoin SDK resources...");

        // Shutdown health check executor
        healthCheckExecutor.shutdown();
        try {
            if (!healthCheckExecutor.awaitTermination(5, TimeUnit.SECONDS)) {
                healthCheckExecutor.shutdownNow();
            }
        } catch (InterruptedException e) {
            healthCheckExecutor.shutdownNow();
        }

        // Close SDK resources
        sdk.close();

        System.out.println("WRFCoin SDK resources cleaned up successfully");
    }

    public WRFCoinSDK getSDK() {
        return sdk;
    }
}
```

---

This enterprise integration guide provides comprehensive patterns for production-ready WRFCoin Java
SDK usage. For additional examples and advanced patterns, see the
[Performance Guide](../docs/PERFORMANCE.md) and [Security Best Practices](../docs/SECURITY.md).
