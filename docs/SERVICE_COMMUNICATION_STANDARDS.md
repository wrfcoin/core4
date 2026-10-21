# WRFCoin Service Communication Standards and Protocols

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Current Architecture Analysis](#current-architecture-analysis)
3. [Protocol Selection Guidelines](#protocol-selection-guidelines)
4. [REST API Standards](#rest-api-standards)
5. [gRPC Standards](#grpc-standards)
6. [Authentication & Authorization](#authentication--authorization)
7. [Error Handling Standards](#error-handling-standards)
8. [API Versioning Strategy](#api-versioning-strategy)
9. [Data Format Standards](#data-format-standards)
10. [Service Discovery & Health Checks](#service-discovery--health-checks)
11. [Request Tracing & Monitoring](#request-tracing--monitoring)
12. [Performance & Caching](#performance--caching)
13. [Security Standards](#security-standards)
14. [Implementation Guidelines](#implementation-guidelines)
15. [Compliance Checklist](#compliance-checklist)

## Executive Summary

This document establishes comprehensive communication standards and protocols for all microservices
within the WRFCoin platform. Based on analysis of existing patterns and modern API best practices,
these standards ensure consistency, reliability, and scalability across the 15+ microservices
ecosystem.

### Key Principles

- **Consistency**: Uniform patterns across all services
- **Type Safety**: Strong typing with TypeScript throughout
- **Observability**: Request tracing and comprehensive monitoring
- **Security**: Authentication, authorization, and data protection
- **Performance**: Efficient communication and caching strategies

## Current Architecture Analysis

The WRFCoin platform currently employs:

- **REST APIs** with consistent response patterns (ApiResponse/ApiError)
- **TypeScript type safety** across service boundaries
- **Structured error handling** with standardized error codes
- **Health check endpoints** for service monitoring
- **Transaction ID propagation** for request tracing
- **Input validation middleware** for data integrity

### Existing Services Portfolio

```
Port 4001 - Main API Server           # Primary application endpoints
Port 3000 - Cross-Chain Protocol      # Multi-blockchain bridges
Port 4000 - Wallet Server            # BIP-39/44 wallet operations
Port 6000 - Compute Networks Server   # Distributed computing and AI/ML
Port 6001 - AI/ML Networks Server     # Machine learning model deployment
Port 7000 - Storage Networks Server   # IPFS, Filecoin, Arweave integration
Port 3002 - IoT Sensor Networks       # Environmental sensor management
Port 8080 - Weather Earnings          # Weather station reward distribution
Port 9001 - Node Discovery           # P2P network coordination
Port 8000 - Governance Server         # DAO proposals and voting
```

## Protocol Selection Guidelines

### When to Use REST

**Recommended for:**

- Public APIs and client-facing endpoints
- CRUD operations on resources
- Simple request-response patterns
- HTTP caching benefits needed
- Wide client compatibility required
- Web dashboard and frontend integration

**Example Use Cases:**

- `/api/weather/submit` - Weather data submission
- `/api/nft/marketplace` - NFT marketplace data
- `/api/governance/proposals` - DAO proposals
- `/api/wallet/:address` - Wallet balance queries

### When to Use gRPC

**Recommended for:**

- High-performance inter-service communication
- Strong typing requirements
- Streaming data scenarios
- Internal service-to-service calls
- Complex data transformations
- Real-time bidirectional communication

**Example Use Cases:**

- Cross-chain bridge protocols
- AI/ML model inference
- Real-time sensor data streaming
- Internal service coordination

## REST API Standards

### 1. URL Structure

```
https://api.wrfcoin.com/v1/{resource}/{id?}/{action?}
```

**Examples:**

```
GET  /api/v1/weather/stations          # List weather stations
GET  /api/v1/weather/stations/123      # Get specific station
POST /api/v1/weather/stations/123/readings # Submit readings
PUT  /api/v1/weather/stations/123      # Update station
DELETE /api/v1/weather/stations/123    # Delete station
```

### 2. HTTP Methods

| Method | Purpose                 | Idempotent | Safe |
| ------ | ----------------------- | ---------- | ---- |
| GET    | Retrieve data           | ✓          | ✓    |
| POST   | Create new resource     | ✗          | ✗    |
| PUT    | Update/replace resource | ✓          | ✗    |
| PATCH  | Partial update          | ✗          | ✗    |
| DELETE | Remove resource         | ✓          | ✗    |

### 3. Response Format

**Success Response:**

```typescript
interface ApiResponse<T> {
  success: true;
  data: T;
  timestamp: string;
  meta?: {
    pagination?: PaginationMeta;
    cache?: CacheMeta;
    [key: string]: any;
  };
}
```

**Error Response:**

```typescript
interface ApiError {
  success: false;
  error: {
    code: string; // Machine-readable error code
    message: string; // Human-readable message
    details?: unknown; // Additional error context
    field?: string; // Field causing validation error
    stack?: string; // Stack trace (development only)
  };
  timestamp: string;
  requestId?: string; // For request tracking
}
```

### 4. Status Codes

| Code | Meaning               | Usage                          |
| ---- | --------------------- | ------------------------------ |
| 200  | OK                    | Successful GET, PUT, PATCH     |
| 201  | Created               | Successful POST                |
| 204  | No Content            | Successful DELETE              |
| 400  | Bad Request           | Invalid request data           |
| 401  | Unauthorized          | Missing/invalid authentication |
| 403  | Forbidden             | Insufficient permissions       |
| 404  | Not Found             | Resource doesn't exist         |
| 409  | Conflict              | Resource conflict              |
| 422  | Unprocessable Entity  | Validation errors              |
| 429  | Too Many Requests     | Rate limit exceeded            |
| 500  | Internal Server Error | Server error                   |
| 502  | Bad Gateway           | Upstream service error         |
| 503  | Service Unavailable   | Service temporarily down       |

### 5. Pagination

```typescript
interface PaginationMeta {
  page: number; // Current page (1-based)
  limit: number; // Items per page
  total: number; // Total items
  totalPages: number; // Total pages
  hasNext: boolean; // Has next page
  hasPrev: boolean; // Has previous page
}

interface PaginatedResponse<T> {
  data: T[];
  pagination: PaginationMeta;
}
```

**Query Parameters:**

```
GET /api/v1/transactions?page=1&limit=20&sortBy=timestamp&sortOrder=desc
```

## gRPC Standards

### 1. Service Definition

```protobuf
syntax = "proto3";

package wrfcoin.v1;

import "google/protobuf/timestamp.proto";
import "google/api/annotations.proto";

service WeatherService {
  rpc SubmitWeatherData(SubmitWeatherDataRequest) returns (SubmitWeatherDataResponse);
  rpc GetWeatherHistory(GetWeatherHistoryRequest) returns (stream WeatherReading);
  rpc SubscribeToUpdates(SubscribeRequest) returns (stream WeatherUpdate);
}

message WeatherReading {
  string station_id = 1;
  double temperature = 2;
  double humidity = 3;
  double pressure = 4;
  google.protobuf.Timestamp timestamp = 5;
  map<string, double> additional_sensors = 6;
}
```

### 2. Error Handling

```protobuf
message ErrorDetails {
  string code = 1;
  string message = 2;
  map<string, string> metadata = 3;
}

message ApiStatus {
  int32 code = 1;
  string message = 2;
  repeated ErrorDetails details = 3;
}
```

### 3. Service Implementation

```typescript
import { Status } from '@grpc/grpc-js/build/src/constants';

class WeatherServiceImplementation {
  async submitWeatherData(
    call: ServerUnaryCall<SubmitWeatherDataRequest, SubmitWeatherDataResponse>,
    callback: sendUnaryData<SubmitWeatherDataResponse>
  ) {
    try {
      const result = await this.processWeatherData(call.request);
      callback(null, result);
    } catch (error) {
      callback({
        code: Status.INTERNAL,
        message: 'Failed to process weather data',
        details: error.message,
      });
    }
  }
}
```

## Authentication & Authorization

### 1. Authentication Methods

**JWT Bearer Tokens (REST):**

```typescript
// Header
Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...

// Middleware
app.use(jwt({
  secret: process.env.JWT_SECRET,
  algorithms: ['HS256'],
  isRevoked: blacklist.isRevoked
}));
```

**mTLS (gRPC):**

```typescript
const credentials = grpc.credentials.createSsl(
  fs.readFileSync('ca-cert.pem'),
  fs.readFileSync('client-key.pem'),
  fs.readFileSync('client-cert.pem')
);
```

### 2. Authorization Patterns

**Role-Based Access Control (RBAC):**

```typescript
interface UserPermissions {
  roles: string[];
  permissions: string[];
  scopes: string[];
}

// Middleware
const authorize = (requiredPermission: string) => {
  return (req: AuthenticatedRequest, res: Response, next: NextFunction) => {
    if (!req.user.permissions.includes(requiredPermission)) {
      return res.status(403).json(createApiError('INSUFFICIENT_PERMISSIONS', 'Access denied'));
    }
    next();
  };
};
```

### 3. API Key Management

```typescript
interface ApiKeyValidation {
  keyId: string;
  scopes: string[];
  rateLimit: number;
  expiresAt?: Date;
}

// Validation
const validateApiKey = async (key: string): Promise<ApiKeyValidation> => {
  // Implementation details
};
```

## Error Handling Standards

### 1. Error Code Taxonomy

**Format:** `{DOMAIN}_{ERROR_TYPE}_{SPECIFIC_ERROR}`

**Examples:**

```typescript
const ERROR_CODES = {
  // Validation Errors
  VALIDATION_MISSING_FIELD: 'VALIDATION_MISSING_FIELD',
  VALIDATION_INVALID_FORMAT: 'VALIDATION_INVALID_FORMAT',

  // Authentication Errors
  AUTH_TOKEN_EXPIRED: 'AUTH_TOKEN_EXPIRED',
  AUTH_INVALID_CREDENTIALS: 'AUTH_INVALID_CREDENTIALS',

  // Business Logic Errors
  WEATHER_STATION_NOT_FOUND: 'WEATHER_STATION_NOT_FOUND',
  INSUFFICIENT_BALANCE: 'INSUFFICIENT_BALANCE',

  // System Errors
  DATABASE_CONNECTION_FAILED: 'DATABASE_CONNECTION_FAILED',
  EXTERNAL_SERVICE_UNAVAILABLE: 'EXTERNAL_SERVICE_UNAVAILABLE',
} as const;
```

### 2. Error Response Examples

```typescript
// Validation Error
{
  "success": false,
  "error": {
    "code": "VALIDATION_MISSING_FIELD",
    "message": "Required field 'stationId' is missing",
    "details": {
      "field": "stationId",
      "required": true
    }
  },
  "timestamp": "2025-01-20T10:30:00Z",
  "requestId": "req_123456789"
}

// Business Logic Error
{
  "success": false,
  "error": {
    "code": "INSUFFICIENT_BALANCE",
    "message": "Insufficient WRF balance for transaction",
    "details": {
      "required": 100,
      "available": 75,
      "shortfall": 25
    }
  },
  "timestamp": "2025-01-20T10:30:00Z",
  "requestId": "req_123456789"
}
```

### 3. Centralized Error Handler

```typescript
class ErrorHandler {
  public async handleError(error: Error, responseStream?: Response): Promise<void> {
    await this.logError(error);
    await this.fireMonitoringMetric(error);

    if (responseStream) {
      await this.sendErrorResponse(error, responseStream);
    }

    if (!this.isTrustedError(error)) {
      process.exit(1); // Crash for untrusted errors
    }
  }

  private isTrustedError(error: Error): boolean {
    return error instanceof ValidationError || error instanceof BusinessLogicError;
  }
}
```

## API Versioning Strategy

### 1. Versioning Scheme

**URL-based versioning (Recommended):**

```
/api/v1/weather/stations
/api/v2/weather/stations
```

**Header-based versioning (Alternative):**

```
Accept: application/vnd.wrfcoin.v1+json
API-Version: v2
```

### 2. Version Lifecycle

```typescript
interface ApiVersion {
  version: string;
  status: 'beta' | 'stable' | 'deprecated' | 'retired';
  releaseDate: Date;
  deprecationDate?: Date;
  retirementDate?: Date;
  supportedUntil: Date;
}

// Version management
const API_VERSIONS: ApiVersion[] = [
  {
    version: 'v1',
    status: 'stable',
    releaseDate: new Date('2024-01-01'),
    supportedUntil: new Date('2026-01-01'),
  },
  {
    version: 'v2',
    status: 'beta',
    releaseDate: new Date('2025-01-01'),
    supportedUntil: new Date('2027-01-01'),
  },
];
```

### 3. Breaking vs Non-Breaking Changes

**Non-Breaking Changes (Minor versions):**

- Adding new endpoints
- Adding optional request fields
- Adding response fields
- Bug fixes

**Breaking Changes (Major versions):**

- Removing endpoints
- Removing request/response fields
- Changing field types
- Changing URL structure

## Data Format Standards

### 1. JSON Schema Validation

```typescript
const WeatherDataSchema = {
  type: 'object',
  required: ['stationId', 'temperature', 'humidity', 'timestamp'],
  properties: {
    stationId: { type: 'string', pattern: '^[A-Z0-9_]+$' },
    temperature: { type: 'number', minimum: -100, maximum: 100 },
    humidity: { type: 'number', minimum: 0, maximum: 100 },
    pressure: { type: 'number', minimum: 800, maximum: 1200 },
    timestamp: { type: 'string', format: 'date-time' },
  },
};
```

### 2. Date and Time Formats

**ISO 8601 Format:**

```typescript
// Always use UTC timestamps
timestamp: '2025-01-20T10:30:00.000Z';

// Date validation
const isValidDate = (dateString: string): boolean => {
  const date = new Date(dateString);
  return date instanceof Date && !isNaN(date.getTime());
};
```

### 3. Numeric Precision

```typescript
// Financial amounts - use string representation for precision
interface TokenAmount {
  amount: string; // "123.456789" (avoid floating point)
  decimals: number; // 18
  symbol: string; // "WRF"
}

// Scientific measurements - number with specified precision
interface SensorReading {
  value: number; // 23.45
  precision: number; // 2 decimal places
  unit: string; // "°C"
}
```

## Service Discovery & Health Checks

### 1. Health Check Endpoints

**Standard Health Check:**

```typescript
app.get('/health', async (req, res) => {
  const health = {
    status: 'healthy',
    timestamp: new Date().toISOString(),
    version: process.env.APP_VERSION,
    services: {
      database: await checkDatabaseConnection(),
      redis: await checkRedisConnection(),
      externalApis: await checkExternalApis(),
    },
    metrics: {
      uptime: process.uptime(),
      memory: process.memoryUsage(),
      cpu: await getCpuUsage(),
    },
  };

  const isHealthy = Object.values(health.services).every(s => s.status === 'up');
  res.status(isHealthy ? 200 : 503).json(createApiResponse(health));
});
```

### 2. Readiness and Liveness Probes

```typescript
// Liveness - is the service running?
app.get('/health/live', (req, res) => {
  res.status(200).json({ status: 'alive' });
});

// Readiness - is the service ready to handle requests?
app.get('/health/ready', async (req, res) => {
  const checks = await Promise.allSettled([
    checkDatabaseConnection(),
    checkRequiredServices(),
    checkResourceAvailability(),
  ]);

  const isReady = checks.every(check => check.status === 'fulfilled');
  res.status(isReady ? 200 : 503).json({ status: isReady ? 'ready' : 'not-ready' });
});
```

### 3. Service Registry

```typescript
interface ServiceRegistration {
  serviceId: string;
  serviceName: string;
  version: string;
  host: string;
  port: number;
  protocol: 'http' | 'https' | 'grpc';
  healthCheckUrl?: string;
  tags: string[];
  metadata: Record<string, string>;
}

const SERVICE_REGISTRY: ServiceRegistration[] = [
  {
    serviceId: 'main-api-v1',
    serviceName: 'main-api',
    version: '1.0.0',
    host: 'localhost',
    port: 4001,
    protocol: 'https',
    healthCheckUrl: '/health',
    tags: ['api', 'core'],
    metadata: { region: 'us-west-2' },
  },
];
```

## Request Tracing & Monitoring

### 1. Transaction ID Propagation

**AsyncLocalStorage Implementation:**

```typescript
const { AsyncLocalStorage } = require('async_hooks');
const asyncLocalStorage = new AsyncLocalStorage();

const transactionIdMiddleware = (req: Request, res: Response, next: NextFunction) => {
  asyncLocalStorage.run(new Map(), () => {
    const transactionId = req.headers['x-transaction-id'] || generateTransactionId();
    asyncLocalStorage.getStore()?.set('transactionId', transactionId);

    // Add to response headers
    res.set('x-transaction-id', transactionId);
    next();
  });
};

// Usage in services
const getTransactionId = (): string => {
  return asyncLocalStorage.getStore()?.get('transactionId') || 'unknown';
};
```

### 2. Distributed Tracing

**OpenTelemetry Integration:**

```typescript
import { trace, context, SpanStatusCode } from '@opentelemetry/api';

const tracer = trace.getTracer('weather-service', '1.0.0');

const processWeatherData = async (data: WeatherData) => {
  const span = tracer.startSpan('process-weather-data');

  try {
    span.setAttributes({
      'weather.station_id': data.stationId,
      'weather.temperature': data.temperature,
    });

    const result = await weatherProcessor.process(data);
    span.setStatus({ code: SpanStatusCode.OK });
    return result;
  } catch (error) {
    span.setStatus({
      code: SpanStatusCode.ERROR,
      message: error.message,
    });
    throw error;
  } finally {
    span.end();
  }
};
```

### 3. Structured Logging

```typescript
import winston from 'winston';

const logger = winston.createLogger({
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.errors({ stack: true }),
    winston.format.json()
  ),
  defaultMeta: {
    service: 'weather-api',
    version: process.env.APP_VERSION,
  },
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: 'app.log' }),
  ],
});

// Usage with transaction ID
const logWithContext = (level: string, message: string, metadata?: object) => {
  logger.log(level, message, {
    ...metadata,
    transactionId: getTransactionId(),
    timestamp: new Date().toISOString(),
  });
};
```

## Performance & Caching

### 1. Response Caching

**HTTP Cache Headers:**

```typescript
// Cache static data
app.get('/api/v1/network/info', cache('5 minutes'), (req, res) => {
  res.set({
    'Cache-Control': 'public, max-age=300',
    ETag: generateETag(networkInfo),
  });
  res.json(createApiResponse(networkInfo));
});

// Cache validation
app.get('/api/v1/market/prices', (req, res) => {
  const etag = generateETag(currentPrices);

  if (req.headers['if-none-match'] === etag) {
    return res.status(304).end();
  }

  res.set('ETag', etag);
  res.json(createApiResponse(currentPrices));
});
```

### 2. Redis Caching Layer

```typescript
interface CacheConfig {
  ttl: number; // Time to live in seconds
  prefix: string; // Key prefix
  compress?: boolean; // Enable compression
}

class CacheService {
  async get<T>(key: string, config: CacheConfig): Promise<T | null> {
    const fullKey = `${config.prefix}:${key}`;
    const cached = await redis.get(fullKey);

    if (!cached) return null;

    return config.compress ? JSON.parse(await decompress(cached)) : JSON.parse(cached);
  }

  async set<T>(key: string, value: T, config: CacheConfig): Promise<void> {
    const fullKey = `${config.prefix}:${key}`;
    const serialized = JSON.stringify(value);
    const data = config.compress ? await compress(serialized) : serialized;

    await redis.setex(fullKey, config.ttl, data);
  }
}
```

### 3. Rate Limiting

```typescript
import rateLimit from 'express-rate-limit';

// Global rate limit
const globalLimiter = rateLimit({
  windowMs: 15 * 60 * 1000, // 15 minutes
  max: 1000, // Limit each IP to 1000 requests per windowMs
  message: 'Too many requests from this IP',
  standardHeaders: true,
  legacyHeaders: false,
});

// API-specific rate limit
const apiLimiter = rateLimit({
  windowMs: 60 * 1000, // 1 minute
  max: 60, // 60 requests per minute
  keyGenerator: req => req.user?.id || req.ip,
  skip: req => req.user?.role === 'premium',
});
```

## Security Standards

### 1. Input Validation

**Schema-based Validation:**

```typescript
import Joi from 'joi';

const weatherDataSchema = Joi.object({
  stationId: Joi.string()
    .pattern(/^[A-Z0-9_]+$/)
    .required(),
  temperature: Joi.number().min(-100).max(100).required(),
  humidity: Joi.number().min(0).max(100).required(),
  pressure: Joi.number().min(800).max(1200).optional(),
  timestamp: Joi.date().iso().required(),
});

const validateWeatherData = (req: Request, res: Response, next: NextFunction) => {
  const { error, value } = weatherDataSchema.validate(req.body);

  if (error) {
    return res
      .status(400)
      .json(
        createApiError('VALIDATION_ERROR', error.details[0].message, {
          field: error.details[0].path[0],
        })
      );
  }

  req.body = value; // Use sanitized data
  next();
};
```

### 2. Security Headers

```typescript
import helmet from 'helmet';

app.use(
  helmet({
    contentSecurityPolicy: {
      directives: {
        defaultSrc: ["'self'"],
        styleSrc: ["'self'", "'unsafe-inline'"],
        scriptSrc: ["'self'"],
        imgSrc: ["'self'", 'data:', 'https:'],
      },
    },
    hsts: {
      maxAge: 31536000,
      includeSubDomains: true,
      preload: true,
    },
  })
);

// Additional security headers
app.use((req, res, next) => {
  res.set({
    'X-API-Version': process.env.API_VERSION,
    'X-RateLimit-Remaining': res.get('X-RateLimit-Remaining'),
    'Strict-Transport-Security': 'max-age=31536000; includeSubDomains',
  });
  next();
});
```

### 3. Data Sanitization

```typescript
import DOMPurify from 'isomorphic-dompurify';
import validator from 'validator';

const sanitizeInput = (data: any): any => {
  if (typeof data === 'string') {
    return validator.escape(DOMPurify.sanitize(data));
  }

  if (Array.isArray(data)) {
    return data.map(sanitizeInput);
  }

  if (typeof data === 'object' && data !== null) {
    const sanitized: any = {};
    for (const [key, value] of Object.entries(data)) {
      sanitized[validator.escape(key)] = sanitizeInput(value);
    }
    return sanitized;
  }

  return data;
};
```

## Implementation Guidelines

### 1. Service Template Structure

```
service-template/
├── src/
│   ├── controllers/          # Request handlers
│   ├── services/            # Business logic
│   ├── models/              # Data models
│   ├── middleware/          # Custom middleware
│   ├── utils/               # Helper functions
│   └── types/               # TypeScript definitions
├── proto/                   # gRPC definitions (if applicable)
├── tests/
│   ├── unit/               # Unit tests
│   ├── integration/        # Integration tests
│   └── fixtures/           # Test data
├── docs/
│   ├── api/                # API documentation
│   └── deployment/         # Deployment guides
└── config/
    ├── development.json
    ├── production.json
    └── test.json
```

### 2. Service Implementation Checklist

- [ ] **API Design**

  - [ ] RESTful resource design
  - [ ] Consistent URL patterns
  - [ ] Proper HTTP methods and status codes
  - [ ] OpenAPI/Swagger documentation

- [ ] **Type Safety**

  - [ ] TypeScript throughout
  - [ ] Shared type definitions
  - [ ] Runtime validation
  - [ ] Schema definitions

- [ ] **Error Handling**

  - [ ] Centralized error handler
  - [ ] Consistent error format
  - [ ] Proper error codes
  - [ ] Error logging and monitoring

- [ ] **Security**

  - [ ] Authentication implementation
  - [ ] Authorization checks
  - [ ] Input validation and sanitization
  - [ ] Security headers

- [ ] **Monitoring**

  - [ ] Health check endpoints
  - [ ] Request tracing
  - [ ] Structured logging
  - [ ] Metrics collection

- [ ] **Performance**
  - [ ] Caching strategy
  - [ ] Rate limiting
  - [ ] Database optimization
  - [ ] Connection pooling

### 3. Development Workflow

```typescript
// 1. Define API contract (OpenAPI/Proto)
// 2. Generate types and client code
// 3. Implement service logic
// 4. Add tests
// 5. Document endpoints
// 6. Deploy and monitor

// Example service implementation
@Service()
export class WeatherService {
  constructor(
    private readonly weatherRepository: WeatherRepository,
    private readonly cache: CacheService,
    private readonly logger: Logger
  ) {}

  @ValidateInput(WeatherDataSchema)
  @Cached({ ttl: 300, key: 'weather:{{stationId}}' })
  async submitWeatherData(data: WeatherData): Promise<WeatherReward> {
    const span = tracer.startSpan('submit-weather-data');

    try {
      this.logger.info('Processing weather data', { stationId: data.stationId });

      const reward = await this.weatherRepository.processSubmission(data);

      span.setAttributes({
        'weather.station_id': data.stationId,
        'reward.amount': reward.amount,
      });

      return reward;
    } catch (error) {
      span.setStatus({ code: SpanStatusCode.ERROR, message: error.message });
      this.logger.error('Failed to process weather data', { error, data });
      throw error;
    } finally {
      span.end();
    }
  }
}
```

## Compliance Checklist

### REST API Compliance

- [ ] **URL Design**

  - [ ] RESTful resource naming
  - [ ] Consistent path structure
  - [ ] Proper use of HTTP methods
  - [ ] Query parameter conventions

- [ ] **Response Format**

  - [ ] Consistent success response structure
  - [ ] Standardized error format
  - [ ] Proper HTTP status codes
  - [ ] Timestamp inclusion

- [ ] **Content Types**
  - [ ] application/json for APIs
  - [ ] Proper Content-Type headers
  - [ ] Accept header handling
  - [ ] Character encoding (UTF-8)

### gRPC Compliance

- [ ] **Service Definition**

  - [ ] Proto3 syntax
  - [ ] Consistent naming conventions
  - [ ] Proper field numbering
  - [ ] Version compatibility

- [ ] **Error Handling**
  - [ ] Standard gRPC status codes
  - [ ] Error details structure
  - [ ] Timeout handling
  - [ ] Retry mechanisms

### Security Compliance

- [ ] **Authentication**

  - [ ] JWT token validation
  - [ ] Token expiration handling
  - [ ] Refresh token support
  - [ ] Blacklist implementation

- [ ] **Authorization**

  - [ ] Role-based access control
  - [ ] Permission validation
  - [ ] Resource-level authorization
  - [ ] Scope limitations

- [ ] **Data Protection**
  - [ ] Input validation
  - [ ] Output sanitization
  - [ ] SQL injection prevention
  - [ ] XSS prevention

### Monitoring Compliance

- [ ] **Health Checks**

  - [ ] /health endpoint
  - [ ] Dependency health checks
  - [ ] Performance metrics
  - [ ] Resource utilization

- [ ] **Logging**

  - [ ] Structured logging format
  - [ ] Transaction ID propagation
  - [ ] Error logging
  - [ ] Performance logging

- [ ] **Metrics**
  - [ ] Request/response metrics
  - [ ] Error rate tracking
  - [ ] Latency measurements
  - [ ] Business metrics

## Conclusion

These communication standards provide a comprehensive framework for building consistent, secure, and
scalable microservices within the WRFCoin platform. All services must adhere to these standards to
ensure platform reliability and maintainability.

For questions or clarifications, please consult the platform architecture team or refer to the
implementation examples provided in this document.

---

**Document Version:** 1.0  
**Last Updated:** 2025-01-20  
**Next Review:** 2025-04-20  
**Owned by:** Platform Architecture Team
