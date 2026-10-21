#!/usr/bin/env node

/**
 * Health Dashboard Service
 * Provides comprehensive health monitoring and status dashboard
 * for all mock services in the test environment
 */

const express = require('express');
const cors = require('cors');

const app = express();
const PORT = process.env.PORT || 4000;

// Service configuration from environment
const services = {
  postgres: {
    name: 'PostgreSQL Primary',
    url:
      process.env.POSTGRES_URL ||
      'postgresql://postgres:testpass@test-postgres-primary:5432/wrfcoin_test',
    type: 'database',
    critical: true,
  },
  blockchain: {
    name: 'Blockchain RPC',
    url:
      process.env.BLOCKCHAIN_RPC_URL ||
      'http://blockchain-rpc-mock-enhanced:8545',
    healthEndpoint: '/health',
    type: 'api',
    critical: true,
  },
  weather: {
    name: 'Weather Oracle',
    url:
      process.env.WEATHER_API_URL || 'http://weather-oracle-mock-enhanced:3010',
    healthEndpoint: '/health',
    type: 'api',
    critical: true,
  },
  carbon: {
    name: 'Carbon Oracle',
    url:
      process.env.CARBON_ORACLE_URL ||
      'http://carbon-credit-oracle-enhanced:3020',
    healthEndpoint: '/health',
    type: 'api',
    critical: true,
  },
  redis: {
    name: 'Redis Cache',
    url: process.env.REDIS_URL || 'redis://redis-mock:6379',
    type: 'cache',
    critical: false,
  },
  mockserver: {
    name: 'MockServer',
    url: process.env.MOCKSERVER_URL || 'http://mockserver:1080',
    healthEndpoint: '/status',
    type: 'tool',
    critical: false,
  },
};

// Service health status
const serviceStatus = new Map();
const healthHistory = new Map();

// Middleware
app.use(
  cors({
    origin: '*',
    methods: ['GET', 'POST', 'OPTIONS'],
    allowedHeaders: ['Content-Type', 'Authorization'],
  })
);
app.use(express.json());

// Health check functions
async function checkHttpService(service) {
  try {
    const url = service.url + (service.healthEndpoint || '/health');
    const response = await fetch(url, { timeout: 5000 });

    const status = {
      name: service.name,
      status: response.ok ? 'healthy' : 'unhealthy',
      responseTime: Date.now() - startTime,
      statusCode: response.status,
      url: url,
      timestamp: new Date().toISOString(),
    };

    if (response.ok) {
      try {
        const data = await response.json();
        status.details = data;
      } catch (e) {
        status.details = {
          message: 'Health check passed but response not JSON',
        };
      }
    } else {
      status.details = { error: `HTTP ${response.status}` };
    }

    return status;
  } catch (error) {
    return {
      name: service.name,
      status: 'unhealthy',
      responseTime: null,
      error: error.message,
      url: service.url,
      timestamp: new Date().toISOString(),
    };
  }
}

async function checkPostgresService(service) {
  // For PostgreSQL, we'll do a simple TCP connection check
  // In a real implementation, you'd use a proper PostgreSQL client
  return {
    name: service.name,
    status: 'healthy', // Simulate healthy for now
    responseTime: Math.floor(Math.random() * 50) + 10,
    details: {
      database: 'wrfcoin_test',
      connections: Math.floor(Math.random() * 20) + 5,
      version: '15.0',
    },
    timestamp: new Date().toISOString(),
  };
}

async function checkRedisService(service) {
  // For Redis, we'll simulate a connection check
  return {
    name: service.name,
    status: Math.random() > 0.1 ? 'healthy' : 'unhealthy', // 90% uptime simulation
    responseTime: Math.floor(Math.random() * 20) + 5,
    details: {
      memory: {
        used: Math.floor(Math.random() * 100) + 'MB',
        total: '256MB',
      },
      connections: Math.floor(Math.random() * 50) + 10,
      version: '7.0',
    },
    timestamp: new Date().toISOString(),
  };
}

// Perform health checks on all services
async function performHealthChecks() {
  const results = {};
  let startTime;

  for (const [key, service] of Object.entries(services)) {
    startTime = Date.now();

    try {
      let healthResult;

      switch (service.type) {
        case 'database':
          healthResult = await checkPostgresService(service);
          break;
        case 'cache':
          healthResult = await checkRedisService(service);
          break;
        case 'api':
        case 'tool':
        default:
          healthResult = await checkHttpService(service);
          break;
      }

      results[key] = healthResult;
      serviceStatus.set(key, healthResult);

      // Store in history
      if (!healthHistory.has(key)) {
        healthHistory.set(key, []);
      }
      const history = healthHistory.get(key);
      history.push(healthResult);

      // Keep only last 100 entries
      if (history.length > 100) {
        history.shift();
      }
    } catch (error) {
      results[key] = {
        name: service.name,
        status: 'error',
        error: error.message,
        timestamp: new Date().toISOString(),
      };
    }
  }

  return results;
}

// Calculate overall system health
function calculateOverallHealth() {
  const statuses = Array.from(serviceStatus.values());

  if (statuses.length === 0) {
    return { status: 'unknown', score: 0 };
  }

  const healthyCount = statuses.filter(s => s.status === 'healthy').length;
  const unhealthyCount = statuses.filter(s => s.status === 'unhealthy').length;
  const errorCount = statuses.filter(s => s.status === 'error').length;

  // Calculate critical service status
  const criticalServices = Object.entries(services)
    .filter(([key, service]) => service.critical)
    .map(([key, service]) => serviceStatus.get(key))
    .filter(status => status);

  const criticalHealthy = criticalServices.filter(
    s => s.status === 'healthy'
  ).length;
  const criticalTotal = criticalServices.length;

  let overallStatus;
  if (criticalTotal > 0 && criticalHealthy < criticalTotal) {
    overallStatus = 'critical';
  } else if (unhealthyCount > 0 || errorCount > 0) {
    overallStatus = 'degraded';
  } else {
    overallStatus = 'healthy';
  }

  const score = Math.round((healthyCount / statuses.length) * 100);

  return {
    status: overallStatus,
    score,
    total: statuses.length,
    healthy: healthyCount,
    unhealthy: unhealthyCount,
    errors: errorCount,
    critical: {
      total: criticalTotal,
      healthy: criticalHealthy,
    },
  };
}

// Dashboard HTML
function generateDashboardHTML(healthData, overallHealth) {
  return `
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WRFCoin Test Environment Health Dashboard</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }
        
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #f5f7fa;
            color: #2d3748;
            line-height: 1.6;
        }
        
        .header {
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 2rem;
            text-align: center;
        }
        
        .header h1 {
            font-size: 2rem;
            margin-bottom: 0.5rem;
        }
        
        .header p {
            opacity: 0.9;
        }
        
        .container {
            max-width: 1200px;
            margin: 0 auto;
            padding: 2rem;
        }
        
        .overall-status {
            background: white;
            border-radius: 12px;
            padding: 2rem;
            margin-bottom: 2rem;
            box-shadow: 0 4px 6px rgba(0,0,0,0.1);
            text-align: center;
        }
        
        .status-indicator {
            display: inline-block;
            padding: 0.5rem 1rem;
            border-radius: 20px;
            font-weight: bold;
            text-transform: uppercase;
            letter-spacing: 0.5px;
            margin-bottom: 1rem;
        }
        
        .status-healthy { background: #48bb78; color: white; }
        .status-degraded { background: #ed8936; color: white; }
        .status-critical { background: #f56565; color: white; }
        .status-unknown { background: #a0aec0; color: white; }
        
        .status-score {
            font-size: 3rem;
            font-weight: bold;
            color: #2d3748;
        }
        
        .status-details {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 1rem;
            margin-top: 2rem;
        }
        
        .status-detail {
            text-align: center;
        }
        
        .status-detail .number {
            font-size: 2rem;
            font-weight: bold;
            color: #4299e1;
        }
        
        .status-detail .label {
            color: #718096;
            font-size: 0.9rem;
        }
        
        .services-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
            gap: 2rem;
        }
        
        .service-card {
            background: white;
            border-radius: 12px;
            padding: 1.5rem;
            box-shadow: 0 4px 6px rgba(0,0,0,0.1);
            border-left: 4px solid #e2e8f0;
        }
        
        .service-card.healthy {
            border-left-color: #48bb78;
        }
        
        .service-card.unhealthy {
            border-left-color: #f56565;
        }
        
        .service-card.error {
            border-left-color: #ed8936;
        }
        
        .service-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 1rem;
        }
        
        .service-name {
            font-size: 1.1rem;
            font-weight: bold;
        }
        
        .service-status {
            padding: 0.25rem 0.75rem;
            border-radius: 12px;
            font-size: 0.8rem;
            font-weight: bold;
            text-transform: uppercase;
        }
        
        .service-details {
            color: #718096;
            font-size: 0.9rem;
        }
        
        .service-detail-item {
            margin-bottom: 0.5rem;
        }
        
        .service-url {
            font-family: monospace;
            background: #f7fafc;
            padding: 0.25rem 0.5rem;
            border-radius: 4px;
            word-break: break-all;
        }
        
        .refresh-info {
            text-align: center;
            margin-top: 2rem;
            color: #718096;
            font-size: 0.9rem;
        }
        
        .auto-refresh {
            position: fixed;
            top: 20px;
            right: 20px;
            background: white;
            padding: 0.5rem 1rem;
            border-radius: 20px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
            font-size: 0.8rem;
            color: #718096;
        }
        
        @media (max-width: 768px) {
            .container {
                padding: 1rem;
            }
            
            .services-grid {
                grid-template-columns: 1fr;
            }
        }
    </style>
    <script>
        // Auto-refresh every 30 seconds
        setTimeout(() => {
            window.location.reload();
        }, 30000);
        
        // Update countdown
        let countdown = 30;
        setInterval(() => {
            countdown--;
            const element = document.getElementById('countdown');
            if (element) {
                element.textContent = countdown;
            }
            if (countdown <= 0) {
                countdown = 30;
            }
        }, 1000);
    </script>
</head>
<body>
    <div class="header">
        <h1>WRFCoin Test Environment</h1>
        <p>Service Health Dashboard</p>
    </div>
    
    <div class="auto-refresh">
        Auto-refresh in <span id="countdown">30</span>s
    </div>
    
    <div class="container">
        <div class="overall-status">
            <div class="status-indicator status-${overallHealth.status}">
                ${overallHealth.status}
            </div>
            <div class="status-score">${overallHealth.score}%</div>
            <p>Overall System Health</p>
            
            <div class="status-details">
                <div class="status-detail">
                    <div class="number">${overallHealth.total}</div>
                    <div class="label">Total Services</div>
                </div>
                <div class="status-detail">
                    <div class="number">${overallHealth.healthy}</div>
                    <div class="label">Healthy</div>
                </div>
                <div class="status-detail">
                    <div class="number">${overallHealth.unhealthy}</div>
                    <div class="label">Unhealthy</div>
                </div>
                <div class="status-detail">
                    <div class="number">${overallHealth.errors}</div>
                    <div class="label">Errors</div>
                </div>
            </div>
        </div>
        
        <div class="services-grid">
            ${Object.entries(healthData)
              .map(
                ([key, service]) => `
                <div class="service-card ${service.status}">
                    <div class="service-header">
                        <div class="service-name">${service.name}</div>
                        <div class="service-status status-${service.status}">${service.status}</div>
                    </div>
                    
                    <div class="service-details">
                        ${
                          service.url
                            ? `<div class="service-detail-item">
                            <strong>URL:</strong> <span class="service-url">${service.url}</span>
                        </div>`
                            : ''
                        }
                        
                        ${
                          service.responseTime
                            ? `<div class="service-detail-item">
                            <strong>Response Time:</strong> ${service.responseTime}ms
                        </div>`
                            : ''
                        }
                        
                        ${
                          service.statusCode
                            ? `<div class="service-detail-item">
                            <strong>Status Code:</strong> ${service.statusCode}
                        </div>`
                            : ''
                        }
                        
                        ${
                          service.error
                            ? `<div class="service-detail-item">
                            <strong>Error:</strong> ${service.error}
                        </div>`
                            : ''
                        }
                        
                        ${
                          service.details && typeof service.details === 'object'
                            ? Object.entries(service.details)
                                .map(([key, value]) =>
                                  typeof value === 'object'
                                    ? ''
                                    : `<div class="service-detail-item">
                              <strong>${key}:</strong> ${value}
                            </div>`
                                )
                                .join('')
                            : ''
                        }
                        
                        <div class="service-detail-item">
                            <strong>Last Check:</strong> ${new Date(service.timestamp).toLocaleString()}
                        </div>
                    </div>
                </div>
            `
              )
              .join('')}
        </div>
        
        <div class="refresh-info">
            <p>Dashboard auto-refreshes every 30 seconds</p>
            <p>Last updated: ${new Date().toLocaleString()}</p>
        </div>
    </div>
</body>
</html>`;
}

// Routes
app.get('/', async (req, res) => {
  try {
    const healthData = await performHealthChecks();
    const overallHealth = calculateOverallHealth();

    const html = generateDashboardHTML(healthData, overallHealth);
    res.setHeader('Content-Type', 'text/html');
    res.send(html);
  } catch (error) {
    res.status(500).json({
      error: 'Failed to generate dashboard',
      message: error.message,
    });
  }
});

app.get('/api/health', async (req, res) => {
  try {
    const healthData = await performHealthChecks();
    const overallHealth = calculateOverallHealth();

    res.json({
      timestamp: new Date().toISOString(),
      overall: overallHealth,
      services: healthData,
    });
  } catch (error) {
    res.status(500).json({
      error: 'Health check failed',
      message: error.message,
    });
  }
});

app.get('/api/health/:serviceName', (req, res) => {
  const { serviceName } = req.params;
  const serviceHealth = serviceStatus.get(serviceName);

  if (!serviceHealth) {
    return res.status(404).json({
      error: 'Service not found',
      serviceName,
      availableServices: Object.keys(services),
    });
  }

  res.json(serviceHealth);
});

app.get('/api/health/:serviceName/history', (req, res) => {
  const { serviceName } = req.params;
  const { limit = 50 } = req.query;

  const history = healthHistory.get(serviceName);
  if (!history) {
    return res.status(404).json({
      error: 'Service not found or no history available',
      serviceName,
    });
  }

  const limitedHistory = history.slice(-parseInt(limit));

  res.json({
    serviceName,
    history: limitedHistory,
    count: limitedHistory.length,
    totalRecords: history.length,
  });
});

// Health endpoint for this service
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: 'health-dashboard',
    timestamp: new Date().toISOString(),
    uptime: process.uptime(),
    monitoredServices: Object.keys(services).length,
  });
});

// Start server
app.listen(PORT, '0.0.0.0', () => {
  console.log(`Health Dashboard running on port ${PORT}`);
  console.log(`Monitoring ${Object.keys(services).length} services`);
  console.log(`Dashboard available at: http://localhost:${PORT}`);

  // Perform initial health check
  performHealthChecks();
});
