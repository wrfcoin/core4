const express = require('express');
const cors = require('cors');
const winston = require('winston');
const cron = require('node-cron');
const fs = require('fs');
const path = require('path');

/**
 * Enhanced Carbon Oracle Mock with Real-Time Data Simulation
 *
 * This enhanced version supports:
 * - Real-time carbon price updates with realistic market patterns
 * - Multiple carbon credit types and markets
 * - Historical price data with trends
 * - Market volatility simulation
 * - Multiple test scenarios and failure modes
 * - Comprehensive logging and metrics
 */

const app = express();
const PORT = process.env.PORT || 3020;

// Configuration from environment variables
const config = {
  scenario: process.env.MOCK_SCENARIO || 'default',
  failureRate: parseFloat(process.env.FAILURE_RATE || 0),
  latencyMs: parseInt(process.env.LATENCY_MS || 0),
  chaosEnabled: process.env.CHAOS_ENABLED === 'true',
  updateInterval: parseInt(process.env.UPDATE_INTERVAL || 30000), // 30 seconds
  priceVolatility: parseFloat(process.env.PRICE_VOLATILITY || 0.05), // 5% volatility
};

// Setup logging
const logger = winston.createLogger({
  level: 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: '/app/logs/carbon-mock.log' }),
  ],
});

// Load scenario configurations
let scenarios = {};
try {
  if (fs.existsSync('./scenarios/carbon-scenarios.json')) {
    scenarios = JSON.parse(
      fs.readFileSync('./scenarios/carbon-scenarios.json', 'utf8')
    );
  }
} catch (error) {
  logger.warn('Failed to load carbon scenarios:', error.message);
}

// Carbon market data structure
const carbonMarkets = new Map();
const priceHistory = new Map();
const marketStats = {
  totalVolume24h: 0,
  totalTransactions: 0,
  activeMarkets: 0,
  lastUpdate: new Date().toISOString(),
};

// Metrics tracking
const metrics = {
  requests: 0,
  errors: 0,
  responseTime: [],
  priceUpdates: 0,
  startTime: Date.now(),
};

// Middleware
app.use(cors());
app.use(express.json());

// Request logging middleware
app.use((req, res, next) => {
  logger.info(`Carbon API Request: ${req.method} ${req.path}`, {
    ip: req.ip,
    userAgent: req.get('User-Agent'),
    scenario: config.scenario,
  });
  next();
});

// Failure simulation middleware
app.use((req, res, next) => {
  // Simulate random failures
  if (config.failureRate > 0 && Math.random() < config.failureRate) {
    const errorTypes = [
      {
        status: 500,
        message: 'Oracle service temporarily unavailable',
        code: 'ORACLE_UNAVAILABLE',
      },
      {
        status: 503,
        message: 'Carbon market data feed disrupted',
        code: 'DATA_FEED_ERROR',
      },
      {
        status: 429,
        message: 'Rate limit exceeded',
        code: 'RATE_LIMIT_EXCEEDED',
      },
      {
        status: 502,
        message: 'Upstream carbon registry error',
        code: 'REGISTRY_ERROR',
      },
    ];
    const error = errorTypes[Math.floor(Math.random() * errorTypes.length)];
    logger.warn('Simulating carbon oracle failure', { error, path: req.path });

    return res.status(error.status).json({
      success: false,
      error: error.message,
      code: error.code,
      timestamp: new Date().toISOString(),
    });
  }

  // Simulate latency
  if (config.latencyMs > 0) {
    setTimeout(next, config.latencyMs);
  } else {
    next();
  }
});

// Metrics middleware
app.use((req, res, next) => {
  const start = Date.now();
  metrics.requests++;

  res.on('finish', () => {
    const duration = Date.now() - start;
    metrics.responseTime.push(duration);

    // Keep only last 1000 response times
    if (metrics.responseTime.length > 1000) {
      metrics.responseTime = metrics.responseTime.slice(-1000);
    }

    if (res.statusCode >= 400) {
      metrics.errors++;
    }
  });

  next();
});

// Carbon market initialization
const initializeCarbonMarkets = () => {
  const markets = [
    {
      id: 'VCS',
      name: 'Verified Carbon Standard',
      type: 'voluntary',
      basePrice: 15.5,
      currency: 'USD',
      unit: 'tCO2e',
      description: 'Voluntary carbon credits from VCS registry',
    },
    {
      id: 'GS',
      name: 'Gold Standard',
      type: 'voluntary',
      basePrice: 22.3,
      currency: 'USD',
      unit: 'tCO2e',
      description: 'Premium voluntary carbon credits',
    },
    {
      id: 'CAR',
      name: 'Climate Action Reserve',
      type: 'voluntary',
      basePrice: 18.75,
      currency: 'USD',
      unit: 'tCO2e',
      description: 'North American carbon offset credits',
    },
    {
      id: 'EU-ETS',
      name: 'EU Emissions Trading System',
      type: 'compliance',
      basePrice: 85.2,
      currency: 'EUR',
      unit: 'tCO2e',
      description: 'European Union carbon allowances',
    },
    {
      id: 'WCI',
      name: 'Western Climate Initiative',
      type: 'compliance',
      basePrice: 28.45,
      currency: 'USD',
      unit: 'tCO2e',
      description: 'California-Quebec cap-and-trade',
    },
    {
      id: 'NATURE',
      name: 'Nature-Based Solutions',
      type: 'premium',
      basePrice: 45.8,
      currency: 'USD',
      unit: 'tCO2e',
      description: 'High-quality nature-based carbon credits',
    },
  ];

  const scenarioConfig = scenarios[config.scenario] || {};
  const selectedMarkets = markets.slice(
    0,
    scenarioConfig.marketCount || markets.length
  );

  selectedMarkets.forEach(market => {
    const marketData = {
      ...market,
      currentPrice: market.basePrice * (1 + (Math.random() - 0.5) * 0.1), // ±5% variation
      dailyChange: (Math.random() - 0.5) * 0.1, // ±5% daily change
      volume24h: Math.floor(Math.random() * 100000),
      bid: 0,
      ask: 0,
      spread: 0,
      lastUpdate: new Date().toISOString(),
      isActive: Math.random() > 0.05, // 95% uptime
    };

    // Calculate bid/ask spread
    marketData.spread = marketData.currentPrice * 0.02; // 2% spread
    marketData.bid = marketData.currentPrice - marketData.spread / 2;
    marketData.ask = marketData.currentPrice + marketData.spread / 2;

    carbonMarkets.set(market.id, marketData);

    // Initialize price history
    const history = [];
    const now = new Date();
    for (let i = 168; i >= 0; i--) {
      // 7 days of hourly data
      const timestamp = new Date(now.getTime() - i * 3600000);
      const price =
        market.basePrice *
        (1 + Math.sin(i * 0.1) * 0.1 + (Math.random() - 0.5) * 0.05);
      history.push({
        timestamp: timestamp.toISOString(),
        price: Math.round(price * 100) / 100,
        volume: Math.floor(Math.random() * 10000),
      });
    }
    priceHistory.set(market.id, history);
  });

  // Update market stats
  marketStats.activeMarkets = selectedMarkets.length;
  marketStats.totalVolume24h = Array.from(carbonMarkets.values()).reduce(
    (sum, market) => sum + market.volume24h,
    0
  );
  marketStats.lastUpdate = new Date().toISOString();

  logger.info('Carbon markets initialized', {
    scenario: config.scenario,
    marketCount: selectedMarkets.length,
    totalVolume24h: marketStats.totalVolume24h,
  });
};

// Realistic price update simulation
const updateMarketPrices = () => {
  if (config.scenario === 'offline') return;

  const now = new Date().toISOString();
  let totalVolumeUpdate = 0;

  for (const [marketId, market] of carbonMarkets.entries()) {
    if (!market.isActive) continue;

    // Simulate market movements with realistic patterns
    const volatility =
      config.priceVolatility *
      (scenarios[config.scenario]?.volatilityMultiplier || 1);
    const randomWalk = (Math.random() - 0.5) * volatility;
    const trendFactor = Math.sin(Date.now() / 86400000) * 0.001; // Daily trend cycle
    const priceChange = randomWalk + trendFactor;

    // Update price with bounds
    const minPrice = market.basePrice * 0.5;
    const maxPrice = market.basePrice * 2.0;
    market.currentPrice = Math.max(
      minPrice,
      Math.min(maxPrice, market.currentPrice * (1 + priceChange))
    );
    market.currentPrice = Math.round(market.currentPrice * 100) / 100;

    // Update daily change
    const previousPrice =
      priceHistory.get(marketId)?.slice(-1)[0]?.price || market.basePrice;
    market.dailyChange = (market.currentPrice - previousPrice) / previousPrice;

    // Update bid/ask
    market.spread = market.currentPrice * (0.01 + Math.random() * 0.02); // 1-3% spread
    market.bid =
      Math.round((market.currentPrice - market.spread / 2) * 100) / 100;
    market.ask =
      Math.round((market.currentPrice + market.spread / 2) * 100) / 100;

    // Simulate trading volume
    const baseVolume = market.volume24h || 1000;
    const volumeVariation = 0.8 + Math.random() * 0.4; // 80-120% of base
    market.volume24h = Math.floor(baseVolume * volumeVariation);
    totalVolumeUpdate += market.volume24h;

    market.lastUpdate = now;

    // Add to price history
    const history = priceHistory.get(marketId);
    history.push({
      timestamp: now,
      price: market.currentPrice,
      volume: Math.floor(market.volume24h / 24), // Hourly volume estimate
    });

    // Keep last 168 hours (7 days)
    if (history.length > 168) {
      history.shift();
    }

    metrics.priceUpdates++;
  }

  // Update market stats
  marketStats.totalVolume24h = totalVolumeUpdate;
  marketStats.totalTransactions += Math.floor(Math.random() * 50);
  marketStats.lastUpdate = now;

  logger.debug('Carbon market prices updated', {
    activeMarkets: Array.from(carbonMarkets.values()).filter(m => m.isActive)
      .length,
    totalVolume24h: marketStats.totalVolume24h,
    avgPriceChange:
      Array.from(carbonMarkets.values()).reduce(
        (sum, m) => sum + Math.abs(m.dailyChange),
        0
      ) / carbonMarkets.size,
  });
};

// Health check endpoint
app.get('/health', (req, res) => {
  const uptime = Date.now() - metrics.startTime;
  const avgResponseTime =
    metrics.responseTime.length > 0
      ? metrics.responseTime.reduce((a, b) => a + b, 0) /
        metrics.responseTime.length
      : 0;

  const health = {
    status: 'healthy',
    service: 'carbon-oracle-mock',
    version: '2.0.0',
    scenario: config.scenario,
    uptime: Math.floor(uptime / 1000),
    markets: {
      total: carbonMarkets.size,
      active: Array.from(carbonMarkets.values()).filter(m => m.isActive).length,
      totalVolume24h: marketStats.totalVolume24h,
      lastPriceUpdate: marketStats.lastUpdate,
    },
    metrics: {
      totalRequests: metrics.requests,
      totalErrors: metrics.errors,
      errorRate:
        metrics.requests > 0
          ? ((metrics.errors / metrics.requests) * 100).toFixed(2) + '%'
          : '0%',
      averageResponseTime: Math.round(avgResponseTime) + 'ms',
      priceUpdates: metrics.priceUpdates,
    },
    configuration: {
      failureRate: config.failureRate,
      latencyMs: config.latencyMs,
      chaosEnabled: config.chaosEnabled,
      updateInterval: config.updateInterval,
      priceVolatility: config.priceVolatility,
    },
    timestamp: new Date().toISOString(),
  };

  res.json(health);
});

// Metrics endpoint
app.get('/metrics', (req, res) => {
  const uptime = Date.now() - metrics.startTime;
  const avgResponseTime =
    metrics.responseTime.length > 0
      ? metrics.responseTime.reduce((a, b) => a + b, 0) /
        metrics.responseTime.length
      : 0;

  const prometheusMetrics = `
# HELP carbon_mock_requests_total Total number of requests
# TYPE carbon_mock_requests_total counter
carbon_mock_requests_total ${metrics.requests}

# HELP carbon_mock_errors_total Total number of errors
# TYPE carbon_mock_errors_total counter
carbon_mock_errors_total ${metrics.errors}

# HELP carbon_mock_response_time_ms Average response time in milliseconds
# TYPE carbon_mock_response_time_ms gauge
carbon_mock_response_time_ms ${Math.round(avgResponseTime)}

# HELP carbon_mock_uptime_seconds Service uptime in seconds
# TYPE carbon_mock_uptime_seconds counter
carbon_mock_uptime_seconds ${Math.floor(uptime / 1000)}

# HELP carbon_mock_markets_total Total number of carbon markets
# TYPE carbon_mock_markets_total gauge
carbon_mock_markets_total ${carbonMarkets.size}

# HELP carbon_mock_volume_24h Total 24h trading volume
# TYPE carbon_mock_volume_24h gauge
carbon_mock_volume_24h ${marketStats.totalVolume24h}

# HELP carbon_mock_price_updates_total Total price updates
# TYPE carbon_mock_price_updates_total counter
carbon_mock_price_updates_total ${metrics.priceUpdates}
`;

  res.set('Content-Type', 'text/plain');
  res.send(prometheusMetrics.trim());
});

// Get all carbon markets
app.get('/api/carbon/markets', (req, res) => {
  const { type, active_only = 'false' } = req.query;

  let markets = Array.from(carbonMarkets.values());

  if (type) {
    markets = markets.filter(m => m.type === type);
  }

  if (active_only === 'true') {
    markets = markets.filter(m => m.isActive);
  }

  res.json({
    success: true,
    data: markets,
    count: markets.length,
    stats: marketStats,
    timestamp: new Date().toISOString(),
  });
});

// Get specific market data
app.get('/api/carbon/markets/:marketId', (req, res) => {
  const { marketId } = req.params;
  const market = carbonMarkets.get(marketId.toUpperCase());

  if (!market) {
    return res.status(404).json({
      success: false,
      error: 'Carbon market not found',
      code: 'MARKET_NOT_FOUND',
      availableMarkets: Array.from(carbonMarkets.keys()),
      timestamp: new Date().toISOString(),
    });
  }

  res.json({
    success: true,
    data: market,
    timestamp: new Date().toISOString(),
  });
});

// Get current carbon price
app.get('/api/carbon/price/:marketId?', (req, res) => {
  const { marketId } = req.params;

  if (marketId) {
    const market = carbonMarkets.get(marketId.toUpperCase());
    if (!market) {
      return res.status(404).json({
        success: false,
        error: 'Carbon market not found',
        code: 'MARKET_NOT_FOUND',
        timestamp: new Date().toISOString(),
      });
    }

    res.json({
      success: true,
      data: {
        marketId: market.id,
        price: market.currentPrice,
        currency: market.currency,
        unit: market.unit,
        bid: market.bid,
        ask: market.ask,
        spread: market.spread,
        dailyChange: market.dailyChange,
        volume24h: market.volume24h,
        lastUpdate: market.lastUpdate,
      },
      timestamp: new Date().toISOString(),
    });
  } else {
    // Return all market prices
    const prices = Array.from(carbonMarkets.values()).map(market => ({
      marketId: market.id,
      price: market.currentPrice,
      currency: market.currency,
      unit: market.unit,
      dailyChange: market.dailyChange,
      lastUpdate: market.lastUpdate,
    }));

    res.json({
      success: true,
      data: prices,
      count: prices.length,
      timestamp: new Date().toISOString(),
    });
  }
});

// Get price history
app.get('/api/carbon/history/:marketId', (req, res) => {
  const { marketId } = req.params;
  const { period = '24h', limit = 100 } = req.query;

  if (!carbonMarkets.has(marketId.toUpperCase())) {
    return res.status(404).json({
      success: false,
      error: 'Carbon market not found',
      code: 'MARKET_NOT_FOUND',
      timestamp: new Date().toISOString(),
    });
  }

  let history = priceHistory.get(marketId.toUpperCase()) || [];

  // Filter by period
  const now = new Date();
  let cutoffTime;
  switch (period) {
    case '1h':
      cutoffTime = new Date(now.getTime() - 3600000);
      break;
    case '24h':
      cutoffTime = new Date(now.getTime() - 86400000);
      break;
    case '7d':
      cutoffTime = new Date(now.getTime() - 604800000);
      break;
    case '30d':
      cutoffTime = new Date(now.getTime() - 2592000000);
      break;
    default:
      cutoffTime = new Date(now.getTime() - 86400000);
  }

  history = history.filter(h => new Date(h.timestamp) >= cutoffTime);

  // Apply limit
  history = history.slice(-parseInt(limit));

  res.json({
    success: true,
    data: {
      marketId: marketId.toUpperCase(),
      period,
      history,
      count: history.length,
    },
    timestamp: new Date().toISOString(),
  });
});

// Calculate carbon offset requirements
app.post('/api/carbon/calculate', (req, res) => {
  const { activity, amount, unit, region = 'global' } = req.body;

  if (!activity || !amount) {
    return res.status(400).json({
      success: false,
      error: 'Missing required fields: activity and amount',
      code: 'MISSING_REQUIRED_FIELDS',
      timestamp: new Date().toISOString(),
    });
  }

  // Emission factors (tCO2e per unit)
  const emissionFactors = {
    electricity: { kwh: 0.0005, mwh: 0.5 }, // per kWh/MWh
    transport: { km: 0.0002, mile: 0.0003 }, // per km/mile
    fuel: { liter: 0.0024, gallon: 0.009 }, // per liter/gallon
    gas: { m3: 0.002, cf: 0.0001 }, // per cubic meter/cubic feet
    flight: { km: 0.0001, mile: 0.0002 }, // per passenger-km/mile
  };

  const factor = emissionFactors[activity]?.[unit];
  if (!factor) {
    return res.status(400).json({
      success: false,
      error: `Unsupported activity/unit combination: ${activity}/${unit}`,
      supportedCombinations: emissionFactors,
      timestamp: new Date().toISOString(),
    });
  }

  const carbonEmissions = amount * factor;
  const offsetCost = carbonEmissions * 20; // Assume $20/tCO2e average

  // Recommend markets based on emissions amount
  let recommendedMarkets;
  if (carbonEmissions < 1) {
    recommendedMarkets = ['VCS', 'GS'];
  } else if (carbonEmissions < 10) {
    recommendedMarkets = ['VCS', 'GS', 'CAR'];
  } else {
    recommendedMarkets = ['VCS', 'GS', 'CAR', 'NATURE'];
  }

  const recommendations = recommendedMarkets
    .map(id => carbonMarkets.get(id))
    .filter(Boolean)
    .map(market => ({
      marketId: market.id,
      name: market.name,
      price: market.currentPrice,
      cost: Math.round(carbonEmissions * market.currentPrice * 100) / 100,
      currency: market.currency,
    }));

  res.json({
    success: true,
    data: {
      input: { activity, amount, unit, region },
      emissions: {
        amount: Math.round(carbonEmissions * 1000) / 1000,
        unit: 'tCO2e',
      },
      offset: {
        estimatedCost: Math.round(offsetCost * 100) / 100,
        currency: 'USD',
        recommendations,
      },
      calculatedAt: new Date().toISOString(),
    },
    timestamp: new Date().toISOString(),
  });
});

// Scenario management endpoints
app.get('/api/mock/scenario', (req, res) => {
  res.json({
    success: true,
    data: {
      currentScenario: config.scenario,
      availableScenarios: Object.keys(scenarios),
      configuration: config,
      marketStats,
    },
    timestamp: new Date().toISOString(),
  });
});

app.post('/api/mock/scenario/:scenarioName', (req, res) => {
  const { scenarioName } = req.params;

  if (!scenarios[scenarioName]) {
    return res.status(400).json({
      success: false,
      error: `Unknown scenario: ${scenarioName}`,
      availableScenarios: Object.keys(scenarios),
      timestamp: new Date().toISOString(),
    });
  }

  config.scenario = scenarioName;

  // Reinitialize markets for new scenario
  carbonMarkets.clear();
  priceHistory.clear();
  initializeCarbonMarkets();

  logger.info('Scenario changed', { newScenario: scenarioName });

  res.json({
    success: true,
    message: `Scenario changed to: ${scenarioName}`,
    data: {
      scenario: config.scenario,
      markets: carbonMarkets.size,
      totalVolume24h: marketStats.totalVolume24h,
    },
    timestamp: new Date().toISOString(),
  });
});

// Chaos engineering endpoint
app.post('/api/mock/chaos', (req, res) => {
  const { action, duration = 30 } = req.body;

  switch (action) {
    case 'market_crash':
      // Crash all market prices by 50%
      for (const market of carbonMarkets.values()) {
        market.currentPrice *= 0.5;
        market.dailyChange = -0.5;
      }
      break;

    case 'high_volatility':
      config.priceVolatility = 0.2; // 20% volatility
      setTimeout(() => {
        config.priceVolatility = 0.05;
      }, duration * 1000);
      break;

    case 'market_closure':
      // Deactivate random markets
      const markets = Array.from(carbonMarkets.values());
      const marketsToClose = Math.floor(markets.length * 0.5);
      for (let i = 0; i < marketsToClose; i++) {
        markets[i].isActive = false;
      }
      setTimeout(() => {
        for (const market of carbonMarkets.values()) {
          market.isActive = true;
        }
      }, duration * 1000);
      break;

    case 'data_corruption':
      // Corrupt price data temporarily
      for (const market of carbonMarkets.values()) {
        market.currentPrice = Math.random() * 1000;
        market.bid = market.currentPrice * 2;
        market.ask = market.currentPrice * 0.5;
      }
      setTimeout(() => {
        initializeCarbonMarkets();
      }, duration * 1000);
      break;

    default:
      return res.status(400).json({
        success: false,
        error: 'Invalid chaos action',
        supportedActions: [
          'market_crash',
          'high_volatility',
          'market_closure',
          'data_corruption',
        ],
        timestamp: new Date().toISOString(),
      });
  }

  logger.warn('Chaos engineering activated', { action, duration });

  res.json({
    success: true,
    message: `Chaos action '${action}' activated for ${duration} seconds`,
    timestamp: new Date().toISOString(),
  });
});

// Error handling
app.use((req, res) => {
  res.status(404).json({
    success: false,
    error: `Endpoint ${req.method} ${req.path} not found`,
    code: 'ENDPOINT_NOT_FOUND',
    timestamp: new Date().toISOString(),
  });
});

app.use((error, req, res, next) => {
  logger.error('Carbon Oracle Mock Error:', error);
  res.status(500).json({
    success: false,
    error: 'Internal server error',
    code: 'INTERNAL_SERVER_ERROR',
    timestamp: new Date().toISOString(),
  });
});

// Scheduled price updates
if (config.updateInterval > 0 && config.scenario !== 'offline') {
  setInterval(updateMarketPrices, config.updateInterval);

  // Also update prices every minute using cron for more regular updates
  cron.schedule('* * * * *', updateMarketPrices);
}

// Initialize and start server
initializeCarbonMarkets();

app.listen(PORT, '0.0.0.0', () => {
  logger.info('Enhanced Carbon Oracle Mock started', {
    port: PORT,
    scenario: config.scenario,
    markets: carbonMarkets.size,
    updateInterval: config.updateInterval,
    config,
  });
});

module.exports = app;
