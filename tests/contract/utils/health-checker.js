const express = require('express');
const axios = require('axios');
const winston = require('winston');
const { Pool } = require('pg');
const Redis = require('ioredis');
const { corsMiddleware } = require('../../../backend/shared/middleware/cors-config.cjs');

/**
 * Comprehensive Health Checker Utility
 *
 * Monitors the health of all mock services and external dependencies
 * Provides detailed health status, dependency mapping, and alerting
 * Supports integration test coordination and service readiness detection
 */

const app = express();
const PORT = process.env.PORT || 3100;

// Configuration
const config = {
  checkInterval: parseInt(process.env.HEALTH_CHECK_INTERVAL || 30000), // 30 seconds
  timeout: parseInt(process.env.HEALTH_CHECK_TIMEOUT || 5000), // 5 seconds
  retries: parseInt(process.env.HEALTH_CHECK_RETRIES || 3),
  alertThreshold: parseInt(process.env.ALERT_THRESHOLD || 3), // failures before alert
  scenario: process.env.MOCK_SCENARIO || 'default',
};

// Service registry
const services = {
  // Core Infrastructure
  postgres: {
    name: 'PostgreSQL Database',
    type: 'database',
    url:
      process.env.POSTGRES_URL ||
      'postgresql://wrfuser:wrfpass@localhost:5432/wrftest',
    healthEndpoint: null,
    critical: true,
    dependencies: [],
  },
  redis: {
    name: 'Redis Cache',
    type: 'cache',
    url: process.env.REDIS_URL || 'redis://localhost:6379',
    healthEndpoint: null,
    critical: true,
    dependencies: [],
  },

  // Mock Services
  'weather-mock': {
    name: 'Weather Oracle Mock',
    type: 'mock',
    url: 'http://localhost:3010',
    healthEndpoint: '/health',
    critical: true,
    dependencies: [],
  },
  'blockchain-mock': {
    name: 'Blockchain RPC Mock',
    type: 'mock',
    url: 'http://localhost:8545',
    healthEndpoint: '/health',
    critical: true,
    dependencies: [],
  },
  'carbon-mock': {
    name: 'Carbon Oracle Mock',
    type: 'mock',
    url: 'http://localhost:3020',
    healthEndpoint: '/health',
    critical: false,
    dependencies: [],
  },

  // Advanced Services
  mockserver: {
    name: 'MockServer HTTP',
    type: 'utility',
    url: 'http://localhost:1080',
    healthEndpoint: '/mockserver/status',
    critical: false,
    dependencies: [],
  },
  consul: {
    name: 'Service Discovery',
    type: 'utility',
    url: 'http://localhost:8500',
    healthEndpoint: '/v1/status/leader',
    critical: false,
    dependencies: [],
  },
  rabbitmq: {
    name: 'Message Queue',
    type: 'messaging',
    url: 'http://localhost:15672',
    healthEndpoint: '/api/aliveness-test/%2F',
    critical: false,
    dependencies: [],
    auth: { username: 'admin', password: 'admin' },
  },

  // Monitoring Stack
  prometheus: {
    name: 'Prometheus Metrics',
    type: 'monitoring',
    url: 'http://localhost:9090',
    healthEndpoint: '/-/healthy',
    critical: false,
    dependencies: [],
  },
  grafana: {
    name: 'Grafana Dashboard',
    type: 'monitoring',
    url: 'http://localhost:3000',
    healthEndpoint: '/api/health',
    critical: false,
    dependencies: ['prometheus'],
  },
  elasticsearch: {
    name: 'Elasticsearch Logs',
    type: 'logging',
    url: 'http://localhost:9200',
    healthEndpoint: '/_cluster/health',
    critical: false,
    dependencies: [],
  },
  kibana: {
    name: 'Kibana Logs UI',
    type: 'logging',
    url: 'http://localhost:5601',
    healthEndpoint: '/api/status',
    critical: false,
    dependencies: ['elasticsearch'],
  },
};

// Health status tracking
const healthStatus = new Map();
const healthHistory = new Map();
const alertCounts = new Map();

// Initialize health status for all services
Object.keys(services).forEach(serviceId => {
  healthStatus.set(serviceId, {
    status: 'unknown',
    lastCheck: null,
    lastSuccess: null,
    lastError: null,
    consecutiveFailures: 0,
    responseTime: null,
    details: {},
  });
  healthHistory.set(serviceId, []);
  alertCounts.set(serviceId, 0);
});

// Logging setup
const logger = winston.createLogger({
  level: 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: '/app/logs/health-checker.log' }),
  ],
});

// Middleware
app.use(express.json());
app.use(corsMiddleware);

// Database connections
let pgPool = null;
let redisClient = null;

// Initialize database connections
const initializeConnections = async () => {
  try {
    // PostgreSQL connection
    pgPool = new Pool({
      connectionString: services.postgres.url,
      max: 5,
      idleTimeoutMillis: 30000,
      connectionTimeoutMillis: 5000,
    });

    // Redis connection
    redisClient = new Redis(services.redis.url, {
      retryDelayOnFailover: 100,
      maxRetriesPerRequest: 3,
      lazyConnect: true,
    });

    logger.info('Database connections initialized');
  } catch (error) {
    logger.error('Failed to initialize database connections:', error);
  }
};

// Health check functions
const checkHttpService = async (serviceId, service) => {
  const startTime = Date.now();

  try {
    const requestConfig = {
      timeout: config.timeout,
      validateStatus: status => status < 500, // Accept 2xx, 3xx, 4xx
    };

    // Add authentication if required
    if (service.auth) {
      requestConfig.auth = service.auth;
    }

    const response = await axios.get(
      `${service.url}${service.healthEndpoint}`,
      requestConfig
    );

    const responseTime = Date.now() - startTime;

    return {
      status: 'healthy',
      responseTime,
      httpStatus: response.status,
      details: {
        url: `${service.url}${service.healthEndpoint}`,
        responseData:
          typeof response.data === 'object'
            ? response.data
            : { message: response.data },
      },
    };
  } catch (error) {
    const responseTime = Date.now() - startTime;

    return {
      status: 'unhealthy',
      responseTime,
      details: {
        url: `${service.url}${service.healthEndpoint}`,
        error: error.message,
        code: error.code,
        httpStatus: error.response?.status,
      },
    };
  }
};

const checkPostgres = async () => {
  const startTime = Date.now();

  try {
    if (!pgPool) {
      throw new Error('PostgreSQL pool not initialized');
    }

    const client = await pgPool.connect();
    try {
      const result = await client.query(
        'SELECT NOW() as current_time, version() as version'
      );
      const responseTime = Date.now() - startTime;

      return {
        status: 'healthy',
        responseTime,
        details: {
          currentTime: result.rows[0].current_time,
          version: result.rows[0].version.split(' ')[0], // First part of version
          totalConnections: pgPool.totalCount,
          idleConnections: pgPool.idleCount,
          waitingConnections: pgPool.waitingCount,
        },
      };
    } finally {
      client.release();
    }
  } catch (error) {
    const responseTime = Date.now() - startTime;

    return {
      status: 'unhealthy',
      responseTime,
      details: {
        error: error.message,
        code: error.code,
      },
    };
  }
};

const checkRedis = async () => {
  const startTime = Date.now();

  try {
    if (!redisClient) {
      throw new Error('Redis client not initialized');
    }

    const pingResult = await redisClient.ping();
    const info = await redisClient.info('server');
    const responseTime = Date.now() - startTime;

    // Parse Redis info
    const infoLines = info.split('\r\n');
    const redisVersion = infoLines
      .find(line => line.startsWith('redis_version:'))
      ?.split(':')[1];
    const uptimeSeconds = infoLines
      .find(line => line.startsWith('uptime_in_seconds:'))
      ?.split(':')[1];

    return {
      status: 'healthy',
      responseTime,
      details: {
        ping: pingResult,
        version: redisVersion,
        uptimeSeconds: parseInt(uptimeSeconds || 0),
        status: redisClient.status,
      },
    };
  } catch (error) {
    const responseTime = Date.now() - startTime;

    return {
      status: 'unhealthy',
      responseTime,
      details: {
        error: error.message,
        redisStatus: redisClient?.status,
      },
    };
  }
};

// Main health check function
const checkServiceHealth = async serviceId => {
  const service = services[serviceId];
  if (!service) return null;

  let result;

  try {
    switch (service.type) {
      case 'database':
        if (serviceId === 'postgres') {
          result = await checkPostgres();
        }
        break;

      case 'cache':
        if (serviceId === 'redis') {
          result = await checkRedis();
        }
        break;

      default:
        result = await checkHttpService(serviceId, service);
    }

    // Update health status
    const currentStatus = healthStatus.get(serviceId);
    const now = new Date().toISOString();

    const updatedStatus = {
      ...currentStatus,
      status: result.status,
      lastCheck: now,
      responseTime: result.responseTime,
      details: result.details,
    };

    if (result.status === 'healthy') {
      updatedStatus.lastSuccess = now;
      updatedStatus.consecutiveFailures = 0;
      alertCounts.set(serviceId, 0);
    } else {
      updatedStatus.lastError = now;
      updatedStatus.consecutiveFailures = currentStatus.consecutiveFailures + 1;

      // Alert threshold check
      if (updatedStatus.consecutiveFailures >= config.alertThreshold) {
        const alertCount = alertCounts.get(serviceId) + 1;
        alertCounts.set(serviceId, alertCount);

        logger.error('Service health alert', {
          serviceId,
          serviceName: service.name,
          consecutiveFailures: updatedStatus.consecutiveFailures,
          alertCount,
          error: result.details.error,
        });
      }
    }

    healthStatus.set(serviceId, updatedStatus);

    // Update history (keep last 100 checks)
    const history = healthHistory.get(serviceId);
    history.push({
      timestamp: now,
      status: result.status,
      responseTime: result.responseTime,
      details: result.details,
    });

    if (history.length > 100) {
      history.shift();
    }

    return updatedStatus;
  } catch (error) {
    logger.error(`Health check failed for ${serviceId}:`, error);
    return null;
  }
};

// Check all services
const checkAllServices = async () => {
  const checks = Object.keys(services).map(serviceId =>
    checkServiceHealth(serviceId)
  );

  await Promise.all(checks);

  logger.debug('Health check cycle completed', {
    totalServices: Object.keys(services).length,
    healthyServices: Array.from(healthStatus.values()).filter(
      s => s.status === 'healthy'
    ).length,
    scenario: config.scenario,
  });
};

// API Endpoints

// Overall health status
app.get('/health', (req, res) => {
  const allStatuses = Array.from(healthStatus.entries());
  const criticalServices = allStatuses.filter(([id]) => services[id].critical);

  const healthyCount = allStatuses.filter(
    ([, status]) => status.status === 'healthy'
  ).length;
  const criticalHealthy = criticalServices.filter(
    ([, status]) => status.status === 'healthy'
  ).length;

  const overallStatus =
    criticalServices.length === criticalHealthy ? 'healthy' : 'unhealthy';

  res.json({
    status: overallStatus,
    services: {
      total: allStatuses.length,
      healthy: healthyCount,
      unhealthy: allStatuses.length - healthyCount,
      critical: {
        total: criticalServices.length,
        healthy: criticalHealthy,
      },
    },
    scenario: config.scenario,
    lastCheck: Math.max(
      ...allStatuses.map(([, status]) =>
        status.lastCheck ? new Date(status.lastCheck).getTime() : 0
      )
    ),
    timestamp: new Date().toISOString(),
  });
});

// Detailed service status
app.get('/health/services', (req, res) => {
  const { type, critical, status } = req.query;

  let filteredServices = Object.entries(services);

  if (type) {
    filteredServices = filteredServices.filter(
      ([, service]) => service.type === type
    );
  }

  if (critical !== undefined) {
    const isCritical = critical === 'true';
    filteredServices = filteredServices.filter(
      ([, service]) => service.critical === isCritical
    );
  }

  const serviceStatuses = filteredServices.map(([serviceId, service]) => {
    const healthData = healthStatus.get(serviceId);
    return {
      id: serviceId,
      name: service.name,
      type: service.type,
      url: service.url,
      critical: service.critical,
      dependencies: service.dependencies,
      health: healthData,
    };
  });

  if (status) {
    const filteredByStatus = serviceStatuses.filter(
      s => s.health.status === status
    );
    return res.json({
      services: filteredByStatus,
      count: filteredByStatus.length,
      filter: { type, critical, status },
      timestamp: new Date().toISOString(),
    });
  }

  res.json({
    services: serviceStatuses,
    count: serviceStatuses.length,
    filter: { type, critical, status },
    timestamp: new Date().toISOString(),
  });
});

// Individual service health
app.get('/health/services/:serviceId', (req, res) => {
  const { serviceId } = req.params;
  const service = services[serviceId];

  if (!service) {
    return res.status(404).json({
      error: 'Service not found',
      serviceId,
      availableServices: Object.keys(services),
      timestamp: new Date().toISOString(),
    });
  }

  const healthData = healthStatus.get(serviceId);
  const history = healthHistory.get(serviceId) || [];

  res.json({
    id: serviceId,
    name: service.name,
    type: service.type,
    url: service.url,
    critical: service.critical,
    dependencies: service.dependencies,
    health: healthData,
    history: history.slice(-20), // Last 20 checks
    alertCount: alertCounts.get(serviceId),
    timestamp: new Date().toISOString(),
  });
});

// Service dependencies graph
app.get('/health/dependencies', (req, res) => {
  const dependencyGraph = {};

  Object.entries(services).forEach(([serviceId, service]) => {
    dependencyGraph[serviceId] = {
      name: service.name,
      type: service.type,
      critical: service.critical,
      status: healthStatus.get(serviceId)?.status || 'unknown',
      dependencies: service.dependencies,
      dependents: [],
    };
  });

  // Calculate dependents (reverse dependencies)
  Object.entries(services).forEach(([serviceId, service]) => {
    service.dependencies.forEach(depId => {
      if (dependencyGraph[depId]) {
        dependencyGraph[depId].dependents.push(serviceId);
      }
    });
  });

  res.json({
    dependencyGraph,
    serviceCount: Object.keys(dependencyGraph).length,
    timestamp: new Date().toISOString(),
  });
});

// Readiness check for integration tests
app.get('/health/ready', (req, res) => {
  const requiredServices = req.query.services
    ? req.query.services.split(',')
    : Object.keys(services).filter(id => services[id].critical);

  const serviceStates = requiredServices.map(serviceId => {
    const service = services[serviceId];
    const health = healthStatus.get(serviceId);

    return {
      id: serviceId,
      name: service?.name || 'Unknown',
      required: requiredServices.includes(serviceId),
      ready: health?.status === 'healthy',
      status: health?.status || 'unknown',
      lastCheck: health?.lastCheck,
    };
  });

  const allReady = serviceStates.every(s => s.ready);
  const readyCount = serviceStates.filter(s => s.ready).length;

  res.status(allReady ? 200 : 503).json({
    ready: allReady,
    services: serviceStates,
    readyCount,
    totalRequired: requiredServices.length,
    message: allReady
      ? 'All required services are ready'
      : `${readyCount}/${requiredServices.length} services ready`,
    timestamp: new Date().toISOString(),
  });
});

// Trigger manual health check
app.post('/health/check', async (req, res) => {
  const { services: serviceIds } = req.body;

  const servicesToCheck = serviceIds || Object.keys(services);
  const invalidServices = serviceIds
    ? serviceIds.filter(id => !services[id])
    : [];

  if (invalidServices.length > 0) {
    return res.status(400).json({
      error: 'Invalid service IDs',
      invalidServices,
      availableServices: Object.keys(services),
      timestamp: new Date().toISOString(),
    });
  }

  const checks = servicesToCheck.map(serviceId =>
    checkServiceHealth(serviceId)
  );

  const results = await Promise.all(checks);

  res.json({
    message: 'Health checks completed',
    services: servicesToCheck.map((serviceId, index) => ({
      id: serviceId,
      name: services[serviceId].name,
      result: results[index],
    })),
    timestamp: new Date().toISOString(),
  });
});

// Health check metrics in Prometheus format
app.get('/metrics', (req, res) => {
  const allStatuses = Array.from(healthStatus.entries());

  let metrics = `# HELP health_checker_service_status Service health status (1=healthy, 0=unhealthy)
# TYPE health_checker_service_status gauge
`;

  allStatuses.forEach(([serviceId, status]) => {
    const service = services[serviceId];
    const healthValue = status.status === 'healthy' ? 1 : 0;
    metrics += `health_checker_service_status{service="${serviceId}",name="${service.name}",type="${service.type}",critical="${service.critical}"} ${healthValue}\n`;
  });

  metrics += `
# HELP health_checker_service_response_time_ms Service response time in milliseconds
# TYPE health_checker_service_response_time_ms gauge
`;

  allStatuses.forEach(([serviceId, status]) => {
    if (status.responseTime !== null) {
      metrics += `health_checker_service_response_time_ms{service="${serviceId}"} ${status.responseTime}\n`;
    }
  });

  metrics += `
# HELP health_checker_service_consecutive_failures Consecutive failure count
# TYPE health_checker_service_consecutive_failures gauge
`;

  allStatuses.forEach(([serviceId, status]) => {
    metrics += `health_checker_service_consecutive_failures{service="${serviceId}"} ${status.consecutiveFailures}\n`;
  });

  res.set('Content-Type', 'text/plain');
  res.send(metrics.trim());
});

// Error handling
app.use((req, res) => {
  res.status(404).json({
    error: 'Endpoint not found',
    path: req.path,
    method: req.method,
    timestamp: new Date().toISOString(),
  });
});

app.use((error, req, res, next) => {
  logger.error('Health Checker Error:', error);
  res.status(500).json({
    error: 'Internal server error',
    message: error.message,
    timestamp: new Date().toISOString(),
  });
});

// Initialize and start server
const startHealthChecker = async () => {
  try {
    await initializeConnections();

    // Initial health check
    await checkAllServices();

    // Start periodic health checks
    setInterval(checkAllServices, config.checkInterval);

    app.listen(PORT, '0.0.0.0', () => {
      logger.info('Health Checker started', {
        port: PORT,
        checkInterval: config.checkInterval,
        timeout: config.timeout,
        scenario: config.scenario,
        totalServices: Object.keys(services).length,
        criticalServices: Object.values(services).filter(s => s.critical)
          .length,
      });
    });
  } catch (error) {
    logger.error('Failed to start Health Checker:', error);
    process.exit(1);
  }
};

// Graceful shutdown
process.on('SIGTERM', async () => {
  logger.info('Health Checker shutting down...');

  if (pgPool) {
    await pgPool.end();
  }

  if (redisClient) {
    redisClient.disconnect();
  }

  process.exit(0);
});

startHealthChecker();

module.exports = app;
