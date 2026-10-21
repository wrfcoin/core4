const express = require('express');
const cors = require('cors');
const axios = require('axios');
const path = require('path');
const { createServer } = require('http');
const { Server } = require('socket.io');

const app = express();
const server = createServer(app);
const io = new Server(server, {
  cors: {
    origin: '*',
    methods: ['GET', 'POST'],
  },
});

const PORT = process.env.PORT || 4000;

// Middleware
app.use(cors());
app.use(express.json());
app.use(express.static(path.join(__dirname, 'public')));

// Service configurations
const services = {
  api: {
    name: 'API Server',
    url: process.env.API_URL || 'http://api-server-offline:4001',
    healthPath: '/health',
    status: 'unknown',
  },
  frontend: {
    name: 'Frontend',
    url: process.env.FRONTEND_URL || 'http://frontend-offline:3000',
    healthPath: '/',
    status: 'unknown',
  },
  postgres: {
    name: 'PostgreSQL',
    url:
      process.env.POSTGRES_URL ||
      'postgresql://postgres:devpass@postgres-offline:5432/wrfcoin_dev',
    healthPath: null, // Custom check
    status: 'unknown',
  },
  redis: {
    name: 'Redis',
    url: process.env.REDIS_URL || 'redis://redis-offline:6379',
    healthPath: null, // Custom check
    status: 'unknown',
  },
  blockchain: {
    name: 'Blockchain RPC',
    url: process.env.BLOCKCHAIN_RPC_URL || 'http://blockchain-rpc-offline:8545',
    healthPath: '/health',
    status: 'unknown',
  },
  weather: {
    name: 'Weather Oracle',
    url: process.env.WEATHER_API_URL || 'http://weather-oracle-offline:3010',
    healthPath: '/health',
    status: 'unknown',
  },
  carbon: {
    name: 'Carbon Oracle',
    url: process.env.CARBON_API_URL || 'http://carbon-oracle-offline:3020',
    healthPath: '/health',
    status: 'unknown',
  },
  rabbitmq: {
    name: 'RabbitMQ',
    url:
      process.env.RABBITMQ_URL || 'http://admin:devpass@rabbitmq-offline:15672',
    healthPath: '/api/health/checks/virtual-hosts',
    status: 'unknown',
  },
  elasticsearch: {
    name: 'Elasticsearch',
    url: process.env.ELASTICSEARCH_URL || 'http://elasticsearch-offline:9200',
    healthPath: '/_cluster/health',
    status: 'unknown',
  },
};

// System metrics storage
let systemMetrics = {
  serviceStatuses: {},
  performance: [],
  errors: [],
  requests: 0,
  uptime: Date.now(),
};

// Service health checker
async function checkServiceHealth(serviceName, serviceConfig) {
  try {
    let isHealthy = false;
    let responseTime = Date.now();
    let details = {};

    if (serviceName === 'postgres') {
      // PostgreSQL health check (simplified)
      const response = await axios.get(`${services.api.url}/db-health`, {
        timeout: 5000,
      });
      isHealthy = response.status === 200;
      details = { connection: 'active', database: 'wrfcoin_dev' };
    } else if (serviceName === 'redis') {
      // Redis health check (simplified)
      const response = await axios.get(`${services.api.url}/cache-health`, {
        timeout: 5000,
      });
      isHealthy = response.status === 200;
      details = { connection: 'active', memory_usage: 'normal' };
    } else if (serviceConfig.healthPath) {
      // Standard HTTP health check
      const response = await axios.get(
        `${serviceConfig.url}${serviceConfig.healthPath}`,
        {
          timeout: 5000,
          validateStatus: status => status < 500,
        }
      );
      isHealthy = response.status >= 200 && response.status < 400;
      details = response.data || {};
    }

    responseTime = Date.now() - responseTime;

    const status = {
      name: serviceConfig.name,
      healthy: isHealthy,
      responseTime,
      lastCheck: new Date().toISOString(),
      details,
    };

    services[serviceName].status = status;
    systemMetrics.serviceStatuses[serviceName] = status;

    return status;
  } catch (error) {
    const status = {
      name: serviceConfig.name,
      healthy: false,
      responseTime: null,
      lastCheck: new Date().toISOString(),
      error: error.message,
      details: {},
    };

    services[serviceName].status = status;
    systemMetrics.serviceStatuses[serviceName] = status;

    return status;
  }
}

// Check all services
async function checkAllServices() {
  const checks = Object.entries(services).map(([name, config]) =>
    checkServiceHealth(name, config)
  );

  const results = await Promise.all(checks);

  // Emit to connected clients
  io.emit('serviceUpdate', systemMetrics.serviceStatuses);

  return results;
}

// Performance monitoring
function recordPerformance() {
  const metrics = {
    timestamp: Date.now(),
    servicesHealthy: Object.values(systemMetrics.serviceStatuses).filter(
      s => s.healthy
    ).length,
    totalServices: Object.keys(services).length,
    averageResponseTime: Object.values(systemMetrics.serviceStatuses)
      .filter(s => s.responseTime)
      .reduce((acc, s, _, arr) => acc + s.responseTime / arr.length, 0),
    uptime: Date.now() - systemMetrics.uptime,
  };

  systemMetrics.performance.push(metrics);

  // Keep only last 100 metrics
  if (systemMetrics.performance.length > 100) {
    systemMetrics.performance = systemMetrics.performance.slice(-100);
  }

  return metrics;
}

// Routes
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    timestamp: new Date().toISOString(),
    uptime: Date.now() - systemMetrics.uptime,
  });
});

app.get('/api/services', (req, res) => {
  res.json(systemMetrics.serviceStatuses);
});

app.get('/api/metrics', (req, res) => {
  res.json({
    performance: systemMetrics.performance.slice(-20),
    summary: {
      totalServices: Object.keys(services).length,
      healthyServices: Object.values(systemMetrics.serviceStatuses).filter(
        s => s.healthy
      ).length,
      totalRequests: systemMetrics.requests,
      uptime: Date.now() - systemMetrics.uptime,
      lastUpdate: new Date().toISOString(),
    },
  });
});

app.get('/api/logs', (req, res) => {
  res.json({
    errors: systemMetrics.errors.slice(-50),
    requests: systemMetrics.requests,
  });
});

// Environment configuration endpoint
app.get('/api/environment', (req, res) => {
  res.json({
    mode: 'offline',
    services: Object.keys(services),
    configuration: {
      database: 'PostgreSQL (Offline)',
      cache: 'Redis (Offline)',
      blockchain: 'Mock RPC (Offline)',
      weather: 'Mock Oracle (Offline)',
      carbon: 'Mock Oracle (Offline)',
      messaging: 'RabbitMQ (Offline)',
      search: 'Elasticsearch (Offline)',
    },
    features: {
      hotReload: true,
      debugging: true,
      apiExplorer: true,
      dataSeeding: true,
      realTimeUpdates: true,
    },
  });
});

// Control endpoints
app.post('/api/services/:service/restart', async (req, res) => {
  const serviceName = req.params.service;

  if (!services[serviceName]) {
    return res.status(404).json({ error: 'Service not found' });
  }

  try {
    // Simulate service restart (in real implementation, would use Docker API)
    systemMetrics.errors.push({
      timestamp: new Date().toISOString(),
      service: serviceName,
      action: 'restart',
      message: `Service ${serviceName} restart requested`,
    });

    res.json({
      message: `Service ${serviceName} restart initiated`,
      timestamp: new Date().toISOString(),
    });

    // Force health check after restart
    setTimeout(
      () => checkServiceHealth(serviceName, services[serviceName]),
      2000
    );
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.post('/api/data/seed', async (req, res) => {
  try {
    const { dataset } = req.body;

    // Simulate data seeding
    systemMetrics.errors.push({
      timestamp: new Date().toISOString(),
      service: 'data-seeder',
      action: 'seed',
      message: `Data seeding initiated for ${dataset || 'all datasets'}`,
    });

    res.json({
      message: 'Data seeding initiated',
      dataset: dataset || 'all',
      timestamp: new Date().toISOString(),
    });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

// Socket.IO connection handling
io.on('connection', socket => {
  console.log('Client connected to dashboard');

  // Send current status immediately
  socket.emit('serviceUpdate', systemMetrics.serviceStatuses);
  socket.emit('metricsUpdate', {
    performance: systemMetrics.performance.slice(-20),
    summary: {
      totalServices: Object.keys(services).length,
      healthyServices: Object.values(systemMetrics.serviceStatuses).filter(
        s => s.healthy
      ).length,
      uptime: Date.now() - systemMetrics.uptime,
    },
  });

  socket.on('disconnect', () => {
    console.log('Client disconnected from dashboard');
  });

  socket.on('requestServiceCheck', async serviceName => {
    if (services[serviceName]) {
      const status = await checkServiceHealth(
        serviceName,
        services[serviceName]
      );
      socket.emit('serviceUpdate', { [serviceName]: status });
    }
  });
});

// Serve dashboard HTML
app.get('/', (req, res) => {
  res.sendFile(path.join(__dirname, 'public', 'index.html'));
});

// Start periodic health checks
setInterval(checkAllServices, 30000); // Every 30 seconds
setInterval(recordPerformance, 60000); // Every minute

// Initial health check
setTimeout(checkAllServices, 5000); // Wait 5 seconds for services to start

server.listen(PORT, () => {
  console.log(`WRFCoin Offline Development Dashboard running on port ${PORT}`);
  console.log(`Dashboard: http://localhost:${PORT}`);
  console.log(`API: http://localhost:${PORT}/api/services`);
});

// Graceful shutdown
process.on('SIGTERM', () => {
  console.log('Dashboard shutting down gracefully...');
  server.close();
});

module.exports = app;
