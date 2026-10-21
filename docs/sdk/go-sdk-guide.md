# Go SDK for Wrfcoin - Enterprise Microservices

## 🚀 **Go SDK Installation & Setup**

### **Module Installation**

```bash
# Initialize Go module
go mod init your-climate-app

# Add Wrfcoin Go SDK
go get github.com/wrfcoin/go-sdk@latest

# Install common dependencies
go get github.com/gin-gonic/gin
go get github.com/gorilla/websocket
go get go.uber.org/zap
```

### **Basic Setup**

```go
package main

import (
    "context"
    "log"

    "github.com/wrfcoin/go-sdk/client"
    "github.com/wrfcoin/go-sdk/config"
)

func main() {
    // Initialize Wrfcoin client
    cfg := config.Config{
        Network: "testnet",
        APIKey:  "your_api_key_here",
        Timeout: 30,
    }

    wrfClient, err := client.NewWrfcoinClient(cfg)
    if err != nil {
        log.Fatal("Failed to create Wrfcoin client:", err)
    }

    // Verify connection
    info, err := wrfClient.GetNetworkInfo(context.Background())
    if err != nil {
        log.Fatal("Failed to get network info:", err)
    }

    log.Printf("Connected to Wrfcoin %s", info.NetworkName)
}
```

## 🏗️ **Enterprise Service Integration**

### **Climate Data Microservice**

```go
package services

import (
    "context"
    "encoding/json"
    "net/http"
    "time"

    "github.com/gin-gonic/gin"
    "github.com/wrfcoin/go-sdk/climate"
    "go.uber.org/zap"
)

type ClimateService struct {
    client *client.WrfcoinClient
    logger *zap.Logger
    cache  *redis.Client
}

type ClimateDataRequest struct {
    Location  string    `json:"location" binding:"required"`
    StartDate time.Time `json:"start_date" binding:"required"`
    EndDate   time.Time `json:"end_date" binding:"required"`
    Metrics   []string  `json:"metrics" binding:"required"`
}

type ClimateDataResponse struct {
    Data        []climate.DataPoint `json:"data"`
    Summary     climate.Summary     `json:"summary"`
    CacheHit    bool               `json:"cache_hit"`
    ProcessTime time.Duration      `json:"process_time_ms"`
}

func NewClimateService(client *client.WrfcoinClient, logger *zap.Logger) *ClimateService {
    return &ClimateService{
        client: client,
        logger: logger,
        cache:  redis.NewClient(&redis.Options{Addr: "localhost:6379"}),
    }
}

func (s *ClimateService) GetClimateData(c *gin.Context) {
    startTime := time.Now()

    var req ClimateDataRequest
    if err := c.ShouldBindJSON(&req); err != nil {
        c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
        return
    }

    // Check cache first
    cacheKey := s.generateCacheKey(req)
    if cachedData, err := s.getFromCache(cacheKey); err == nil {
        c.JSON(http.StatusOK, ClimateDataResponse{
            Data:        cachedData.Data,
            Summary:     cachedData.Summary,
            CacheHit:    true,
            ProcessTime: time.Since(startTime),
        })
        return
    }

    // Fetch from Wrfcoin network
    ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
    defer cancel()

    data, err := s.client.GetHistoricalClimateData(ctx, climate.HistoricalRequest{
        Location:  req.Location,
        StartDate: req.StartDate,
        EndDate:   req.EndDate,
        Metrics:   req.Metrics,
    })
    if err != nil {
        s.logger.Error("Failed to fetch climate data", zap.Error(err))
        c.JSON(http.StatusInternalServerError, gin.H{"error": "Failed to fetch data"})
        return
    }

    // Generate summary
    summary := s.generateSummary(data)

    // Cache the result
    response := ClimateDataResponse{
        Data:        data,
        Summary:     summary,
        CacheHit:    false,
        ProcessTime: time.Since(startTime),
    }

    go s.cacheResponse(cacheKey, response)

    c.JSON(http.StatusOK, response)
}

func (s *ClimateService) generateSummary(data []climate.DataPoint) climate.Summary {
    if len(data) == 0 {
        return climate.Summary{}
    }

    var totalTemp, totalHumidity float64
    var minTemp, maxTemp float64 = data[0].Temperature, data[0].Temperature

    for _, point := range data {
        totalTemp += point.Temperature
        totalHumidity += point.Humidity

        if point.Temperature < minTemp {
            minTemp = point.Temperature
        }
        if point.Temperature > maxTemp {
            maxTemp = point.Temperature
        }
    }

    return climate.Summary{
        AvgTemperature: totalTemp / float64(len(data)),
        AvgHumidity:    totalHumidity / float64(len(data)),
        MinTemperature: minTemp,
        MaxTemperature: maxTemp,
        DataPoints:     len(data),
    }
}
```

### **Real-Time Weather Alert System**

```go
package alerts

import (
    "context"
    "encoding/json"
    "log"
    "sync"

    "github.com/gorilla/websocket"
    "github.com/wrfcoin/go-sdk/streaming"
    "go.uber.org/zap"
)

type AlertSystem struct {
    client      *client.WrfcoinClient
    logger      *zap.Logger
    subscribers map[string]*websocket.Conn
    mu          sync.RWMutex

    // Alert thresholds
    extremeHeatThreshold      float64
    extremeColdThreshold      float64
    highWindSpeedThreshold    float64
    lowVisibilityThreshold    float64
}

type WeatherAlert struct {
    ID          string                 `json:"id"`
    Type        string                 `json:"type"`
    Severity    string                 `json:"severity"`
    Location    string                 `json:"location"`
    Message     string                 `json:"message"`
    Timestamp   time.Time              `json:"timestamp"`
    Data        map[string]interface{} `json:"data"`
    ExpiresAt   time.Time              `json:"expires_at"`
}

func NewAlertSystem(client *client.WrfcoinClient, logger *zap.Logger) *AlertSystem {
    return &AlertSystem{
        client:                    client,
        logger:                    logger,
        subscribers:               make(map[string]*websocket.Conn),
        extremeHeatThreshold:      40.0, // 40°C
        extremeColdThreshold:      -20.0, // -20°C
        highWindSpeedThreshold:    120.0, // 120 km/h
        lowVisibilityThreshold:    0.5,   // 0.5 km
    }
}

func (as *AlertSystem) StartMonitoring(ctx context.Context) error {
    // Subscribe to real-time weather updates
    stream, err := as.client.SubscribeToWeatherUpdates(ctx)
    if err != nil {
        return err
    }

    go as.processWeatherStream(ctx, stream)
    return nil
}

func (as *AlertSystem) processWeatherStream(ctx context.Context, stream *streaming.WeatherStream) {
    for {
        select {
        case <-ctx.Done():
            return
        case update := <-stream.Updates():
            as.processWeatherUpdate(update)
        case err := <-stream.Errors():
            as.logger.Error("Weather stream error", zap.Error(err))
        }
    }
}

func (as *AlertSystem) processWeatherUpdate(update streaming.WeatherUpdate) {
    alerts := as.checkForAlerts(update)

    for _, alert := range alerts {
        as.broadcastAlert(alert)

        // Store alert on blockchain for historical record
        go as.storeAlertOnChain(alert)
    }
}

func (as *AlertSystem) checkForAlerts(update streaming.WeatherUpdate) []WeatherAlert {
    var alerts []WeatherAlert

    // Check for extreme temperature
    if update.Temperature > as.extremeHeatThreshold {
        alerts = append(alerts, WeatherAlert{
            ID:        generateAlertID(),
            Type:      "extreme_heat",
            Severity:  "high",
            Location:  update.Location,
            Message:   fmt.Sprintf("Extreme heat warning: %.1f°C", update.Temperature),
            Timestamp: time.Now(),
            Data: map[string]interface{}{
                "temperature": update.Temperature,
                "threshold":   as.extremeHeatThreshold,
            },
            ExpiresAt: time.Now().Add(6 * time.Hour),
        })
    }

    if update.Temperature < as.extremeColdThreshold {
        alerts = append(alerts, WeatherAlert{
            ID:        generateAlertID(),
            Type:      "extreme_cold",
            Severity:  "high",
            Location:  update.Location,
            Message:   fmt.Sprintf("Extreme cold warning: %.1f°C", update.Temperature),
            Timestamp: time.Now(),
            Data: map[string]interface{}{
                "temperature": update.Temperature,
                "threshold":   as.extremeColdThreshold,
            },
            ExpiresAt: time.Now().Add(6 * time.Hour),
        })
    }

    // Check for high wind speeds
    if update.WindSpeed > as.highWindSpeedThreshold {
        severity := "medium"
        if update.WindSpeed > 150 { // Hurricane force
            severity = "critical"
        }

        alerts = append(alerts, WeatherAlert{
            ID:        generateAlertID(),
            Type:      "high_winds",
            Severity:  severity,
            Location:  update.Location,
            Message:   fmt.Sprintf("High wind warning: %.1f km/h", update.WindSpeed),
            Timestamp: time.Now(),
            Data: map[string]interface{}{
                "wind_speed": update.WindSpeed,
                "threshold":  as.highWindSpeedThreshold,
            },
            ExpiresAt: time.Now().Add(3 * time.Hour),
        })
    }

    return alerts
}

func (as *AlertSystem) broadcastAlert(alert WeatherAlert) {
    as.mu.RLock()
    defer as.mu.RUnlock()

    alertJSON, err := json.Marshal(alert)
    if err != nil {
        as.logger.Error("Failed to marshal alert", zap.Error(err))
        return
    }

    // Broadcast to all connected clients
    for clientID, conn := range as.subscribers {
        if err := conn.WriteMessage(websocket.TextMessage, alertJSON); err != nil {
            as.logger.Warn("Failed to send alert to client",
                zap.String("client_id", clientID),
                zap.Error(err))

            // Remove disconnected client
            delete(as.subscribers, clientID)
            conn.Close()
        }
    }
}

func (as *AlertSystem) AddSubscriber(clientID string, conn *websocket.Conn) {
    as.mu.Lock()
    defer as.mu.Unlock()

    as.subscribers[clientID] = conn
    as.logger.Info("Client subscribed to alerts", zap.String("client_id", clientID))
}

func (as *AlertSystem) RemoveSubscriber(clientID string) {
    as.mu.Lock()
    defer as.mu.Unlock()

    if conn, exists := as.subscribers[clientID]; exists {
        conn.Close()
        delete(as.subscribers, clientID)
        as.logger.Info("Client unsubscribed from alerts", zap.String("client_id", clientID))
    }
}
```

## 📊 **Production Deployment Guide**

### **Kubernetes Deployment Configuration**

```yaml
# wrfcoin-climate-service.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: wrfcoin-climate-service
  labels:
    app: wrfcoin-climate-service
spec:
  replicas: 3
  selector:
    matchLabels:
      app: wrfcoin-climate-service
  template:
    metadata:
      labels:
        app: wrfcoin-climate-service
    spec:
      containers:
        - name: climate-service
          image: your-registry/wrfcoin-climate-service:latest
          ports:
            - containerPort: 8080
          env:
            - name: WRFCOIN_API_KEY
              valueFrom:
                secretKeyRef:
                  name: wrfcoin-secrets
                  key: api-key
            - name: WRFCOIN_NETWORK
              value: 'mainnet'
            - name: REDIS_URL
              value: 'redis-service:6379'
          resources:
            requests:
              memory: '256Mi'
              cpu: '250m'
            limits:
              memory: '512Mi'
              cpu: '500m'
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
            initialDelaySeconds: 30
            periodSeconds: 10
          readinessProbe:
            httpGet:
              path: /ready
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 5
---
apiVersion: v1
kind: Service
metadata:
  name: wrfcoin-climate-service
spec:
  selector:
    app: wrfcoin-climate-service
  ports:
    - protocol: TCP
      port: 80
      targetPort: 8080
  type: LoadBalancer
```

### **Docker Configuration**

```dockerfile
# Dockerfile
FROM golang:1.21-alpine AS builder

WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o climate-service ./cmd/server

FROM alpine:latest
RUN apk --no-cache add ca-certificates
WORKDIR /root/

COPY --from=builder /app/climate-service .

EXPOSE 8080
CMD ["./climate-service"]
```

### **Health Check Implementation**

```go
package health

import (
    "context"
    "net/http"
    "time"

    "github.com/gin-gonic/gin"
    "github.com/wrfcoin/go-sdk/client"
)

type HealthChecker struct {
    client *client.WrfcoinClient
}

type HealthStatus struct {
    Status      string            `json:"status"`
    Timestamp   time.Time         `json:"timestamp"`
    Version     string            `json:"version"`
    Checks      map[string]Check  `json:"checks"`
}

type Check struct {
    Status   string        `json:"status"`
    Duration time.Duration `json:"duration"`
    Error    string        `json:"error,omitempty"`
}

func NewHealthChecker(client *client.WrfcoinClient) *HealthChecker {
    return &HealthChecker{client: client}
}

func (h *HealthChecker) HealthCheck(c *gin.Context) {
    status := h.performHealthChecks()

    httpStatus := http.StatusOK
    if status.Status != "healthy" {
        httpStatus = http.StatusServiceUnavailable
    }

    c.JSON(httpStatus, status)
}

func (h *HealthChecker) ReadinessCheck(c *gin.Context) {
    // Quick readiness check
    ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
    defer cancel()

    start := time.Now()
    _, err := h.client.GetNetworkInfo(ctx)
    duration := time.Since(start)

    if err != nil {
        c.JSON(http.StatusServiceUnavailable, gin.H{
            "status": "not_ready",
            "error":  err.Error(),
        })
        return
    }

    c.JSON(http.StatusOK, gin.H{
        "status":   "ready",
        "duration": duration,
    })
}

func (h *HealthChecker) performHealthChecks() HealthStatus {
    checks := make(map[string]Check)

    // Check Wrfcoin network connectivity
    checks["wrfcoin_network"] = h.checkWrfcoinNetwork()

    // Check database connectivity (if using one)
    checks["database"] = h.checkDatabase()

    // Check external dependencies
    checks["redis"] = h.checkRedis()

    // Determine overall status
    overallStatus := "healthy"
    for _, check := range checks {
        if check.Status != "healthy" {
            overallStatus = "unhealthy"
            break
        }
    }

    return HealthStatus{
        Status:    overallStatus,
        Timestamp: time.Now(),
        Version:   "1.0.0", // You should inject this during build
        Checks:    checks,
    }
}

func (h *HealthChecker) checkWrfcoinNetwork() Check {
    start := time.Now()
    ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
    defer cancel()

    _, err := h.client.GetNetworkInfo(ctx)
    duration := time.Since(start)

    if err != nil {
        return Check{
            Status:   "unhealthy",
            Duration: duration,
            Error:    err.Error(),
        }
    }

    return Check{
        Status:   "healthy",
        Duration: duration,
    }
}
```

---

**🚀 The Wrfcoin Go SDK enables high-performance, scalable enterprise microservices for climate
applications with robust monitoring, caching, and deployment capabilities.**
