#!/usr/bin/env node

/**
 * Integration Test Runner
 * Orchestrates comprehensive integration tests across all enhanced mock services
 * Supports various test scenarios, failure simulation, and performance testing
 */

const express = require('express');
const cors = require('cors');
const axios = require('axios');
const WebSocket = require('ws');
const { spawn, exec } = require('child_process');
const fs = require('fs').promises;
const path = require('path');

const app = express();
const PORT = process.env.PORT || 5000;

// Test configuration
const config = {
  services: {
    postgres: {
      primary: { host: 'test-postgres-primary', port: 5432 },
      replica: { host: 'test-postgres-replica', port: 5432 },
    },
    blockchain: {
      host: 'blockchain-rpc-mock-enhanced',
      port: 8545,
      wsPort: 8546,
      healthUrl: 'http://blockchain-rpc-mock-enhanced:8545/health',
    },
    weather: {
      host: 'weather-oracle-mock-enhanced',
      port: 3010,
      wsPort: 3011,
      healthUrl: 'http://weather-oracle-mock-enhanced:3010/health',
    },
    carbon: {
      host: 'carbon-credit-oracle-enhanced',
      port: 3020,
      trackingPort: 3021,
      healthUrl: 'http://carbon-credit-oracle-enhanced:3020/health',
    },
    redis: { host: 'redis-mock', port: 6379 },
    mockserver: { host: 'mockserver', port: 1080 },
    healthDashboard: {
      host: 'health-dashboard',
      port: 4000,
      healthUrl: 'http://health-dashboard:4000/health',
    },
  },
  testSuites: {
    smoke: {
      name: 'Smoke Tests',
      description: 'Basic connectivity and health checks',
      timeout: 30000,
    },
    integration: {
      name: 'Integration Tests',
      description: 'Cross-service communication and data flow',
      timeout: 60000,
    },
    load: {
      name: 'Load Tests',
      description: 'Performance and scalability testing',
      timeout: 300000,
    },
    chaos: {
      name: 'Chaos Engineering',
      description: 'Failure simulation and resilience testing',
      timeout: 180000,
    },
    e2e: {
      name: 'End-to-End Tests',
      description: 'Complete user journey simulation',
      timeout: 120000,
    },
  },
};

// Test results storage
const testResults = new Map();
const testHistory = [];

// Middleware
app.use(cors());
app.use(express.json());

// Test execution state
let currentTestRun = null;
let testRunCounter = 0;

/**
 * Health Check for Test Runner
 */
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: 'integration-test-runner',
    timestamp: new Date().toISOString(),
    config: {
      servicesConfigured: Object.keys(config.services).length,
      testSuitesAvailable: Object.keys(config.testSuites).length,
    },
    currentTestRun: currentTestRun
      ? {
          id: currentTestRun.id,
          suite: currentTestRun.suite,
          status: currentTestRun.status,
          startTime: currentTestRun.startTime,
        }
      : null,
  });
});

/**
 * Service Health Checks
 */
async function checkServiceHealth(serviceName) {
  const service = config.services[serviceName];
  if (!service || !service.healthUrl) {
    return {
      service: serviceName,
      status: 'unknown',
      error: 'No health endpoint configured',
    };
  }

  try {
    const response = await axios.get(service.healthUrl, { timeout: 5000 });
    return {
      service: serviceName,
      status: 'healthy',
      responseTime: Date.now() - startTime,
      data: response.data,
    };
  } catch (error) {
    return {
      service: serviceName,
      status: 'unhealthy',
      error: error.message,
      code: error.code,
    };
  }
}

app.get('/api/services/health', async (req, res) => {
  const healthChecks = await Promise.all(
    Object.keys(config.services).map(checkServiceHealth)
  );

  const overallHealth = {
    status: healthChecks.every(h => h.status === 'healthy')
      ? 'healthy'
      : 'degraded',
    timestamp: new Date().toISOString(),
    services: healthChecks.reduce((acc, check) => {
      acc[check.service] = check;
      return acc;
    }, {}),
  };

  res.json(overallHealth);
});

/**
 * Smoke Tests - Basic connectivity and health
 */
async function runSmokeTests() {
  const tests = [
    {
      name: 'Service Health Checks',
      test: async () => {
        const healthChecks = await Promise.all(
          Object.keys(config.services).map(checkServiceHealth)
        );
        const unhealthyServices = healthChecks.filter(
          h => h.status !== 'healthy'
        );
        if (unhealthyServices.length > 0) {
          throw new Error(
            `Unhealthy services: ${unhealthyServices.map(s => s.service).join(', ')}`
          );
        }
        return { healthyServices: healthChecks.length };
      },
    },
    {
      name: 'Blockchain RPC Connectivity',
      test: async () => {
        const response = await axios.post(
          `http://${config.services.blockchain.host}:${config.services.blockchain.port}`,
          {
            jsonrpc: '2.0',
            method: 'eth_blockNumber',
            params: [],
            id: 1,
          }
        );
        if (!response.data.result) {
          throw new Error('Invalid blockchain response');
        }
        return { blockNumber: response.data.result };
      },
    },
    {
      name: 'Weather API Connectivity',
      test: async () => {
        const response = await axios.get(
          `http://${config.services.weather.host}:${config.services.weather.port}/api/weather/stations`
        );
        if (!response.data.stations || response.data.stations.length === 0) {
          throw new Error('No weather stations available');
        }
        return { stationCount: response.data.stations.length };
      },
    },
    {
      name: 'Carbon Oracle Connectivity',
      test: async () => {
        const response = await axios.get(
          `http://${config.services.carbon.host}:${config.services.carbon.port}/api/carbon/projects`
        );
        if (!response.data.projects || response.data.projects.length === 0) {
          throw new Error('No carbon projects available');
        }
        return { projectCount: response.data.projects.length };
      },
    },
  ];

  return executeTestSuite('smoke', tests);
}

/**
 * Integration Tests - Cross-service communication
 */
async function runIntegrationTests() {
  const tests = [
    {
      name: 'Weather Data to Blockchain Integration',
      test: async () => {
        // Get weather data
        const weatherResponse = await axios.get(
          `http://${config.services.weather.host}:${config.services.weather.port}/api/weather/stations/WS001/current`
        );
        const weatherData = weatherResponse.data;

        // Simulate storing on blockchain
        const blockchainResponse = await axios.post(
          `http://${config.services.blockchain.host}:${config.services.blockchain.port}`,
          {
            jsonrpc: '2.0',
            method: 'eth_sendTransaction',
            params: [
              {
                from: '0x1234567890123456789012345678901234567890',
                to: '0x0987654321098765432109876543210987654321',
                data: JSON.stringify(weatherData),
              },
            ],
            id: 1,
          }
        );

        if (!blockchainResponse.data.result) {
          throw new Error('Failed to store weather data on blockchain');
        }

        return {
          weatherStationId: weatherData.stationId,
          transactionHash: blockchainResponse.data.result,
        };
      },
    },
    {
      name: 'Carbon Credit Purchase Flow',
      test: async () => {
        // Get available projects
        const projectsResponse = await axios.get(
          `http://${config.services.carbon.host}:${config.services.carbon.port}/api/carbon/projects?limit=1`
        );
        const project = projectsResponse.data.projects[0];

        // Purchase carbon credits
        const purchaseResponse = await axios.post(
          `http://${config.services.carbon.host}:${config.services.carbon.port}/api/carbon/credits/purchase`,
          {
            projectId: project.id,
            quantity: 1,
            buyerInfo: { name: 'Test Buyer', email: 'test@wrfcoin.com' },
          }
        );

        if (!purchaseResponse.data.success) {
          throw new Error('Carbon credit purchase failed');
        }

        return {
          projectId: project.id,
          transactionId: purchaseResponse.data.transaction.id,
          quantity: 1,
        };
      },
    },
    {
      name: 'Cross-Service Data Consistency',
      test: async () => {
        // Test that all services report consistent timestamps
        const timestamps = await Promise.all([
          axios.get(
            `http://${config.services.blockchain.host}:${config.services.blockchain.port}/health`
          ),
          axios.get(
            `http://${config.services.weather.host}:${config.services.weather.port}/health`
          ),
          axios.get(
            `http://${config.services.carbon.host}:${config.services.carbon.port}/health`
          ),
        ]);

        const timeDiffs = timestamps.map(response => {
          const serviceTime = new Date(response.data.timestamp);
          const currentTime = new Date();
          return Math.abs(currentTime - serviceTime);
        });

        // Check that all timestamps are within 5 seconds of current time
        const maxDiff = Math.max(...timeDiffs);
        if (maxDiff > 5000) {
          throw new Error(`Time inconsistency detected: ${maxDiff}ms`);
        }

        return { maxTimeDifference: maxDiff };
      },
    },
  ];

  return executeTestSuite('integration', tests);
}

/**
 * Load Tests - Performance and scalability
 */
async function runLoadTests() {
  const tests = [
    {
      name: 'Weather API Load Test',
      test: async () => {
        const concurrentRequests = 50;
        const requests = Array.from({ length: concurrentRequests }, () =>
          axios.get(
            `http://${config.services.weather.host}:${config.services.weather.port}/api/weather/stations`
          )
        );

        const startTime = Date.now();
        const responses = await Promise.allSettled(requests);
        const duration = Date.now() - startTime;

        const successful = responses.filter(
          r => r.status === 'fulfilled'
        ).length;
        const failed = responses.filter(r => r.status === 'rejected').length;

        if (successful < concurrentRequests * 0.95) {
          // 95% success rate required
          throw new Error(
            `Load test failed: ${successful}/${concurrentRequests} requests successful`
          );
        }

        return {
          totalRequests: concurrentRequests,
          successful,
          failed,
          duration,
          requestsPerSecond: Math.round((concurrentRequests / duration) * 1000),
        };
      },
    },
    {
      name: 'Blockchain RPC Load Test',
      test: async () => {
        const concurrentRequests = 30;
        const requests = Array.from({ length: concurrentRequests }, () =>
          axios.post(
            `http://${config.services.blockchain.host}:${config.services.blockchain.port}`,
            {
              jsonrpc: '2.0',
              method: 'eth_blockNumber',
              params: [],
              id: Math.floor(Math.random() * 1000),
            }
          )
        );

        const startTime = Date.now();
        const responses = await Promise.allSettled(requests);
        const duration = Date.now() - startTime;

        const successful = responses.filter(
          r => r.status === 'fulfilled' && r.value.data.result
        ).length;

        if (successful < concurrentRequests * 0.9) {
          // 90% success rate for blockchain
          throw new Error(
            `Blockchain load test failed: ${successful}/${concurrentRequests} requests successful`
          );
        }

        return {
          totalRequests: concurrentRequests,
          successful,
          duration,
          requestsPerSecond: Math.round((concurrentRequests / duration) * 1000),
        };
      },
    },
  ];

  return executeTestSuite('load', tests);
}

/**
 * Chaos Tests - Failure simulation and resilience
 */
async function runChaosTests() {
  const tests = [
    {
      name: 'Service Recovery Test',
      test: async () => {
        // Test that services can handle temporary failures gracefully
        // This is simulated by testing error handling endpoints

        const errorTests = await Promise.allSettled([
          // Test 404 handling
          axios
            .get(
              `http://${config.services.weather.host}:${config.services.weather.port}/api/weather/stations/NONEXISTENT`
            )
            .catch(err => ({
              status: err.response?.status,
              handled: err.response?.status === 404,
            })),

          // Test invalid blockchain request
          axios
            .post(
              `http://${config.services.blockchain.host}:${config.services.blockchain.port}`,
              {
                jsonrpc: '2.0',
                method: 'invalid_method',
                params: [],
                id: 1,
              }
            )
            .catch(err => ({ status: err.response?.status, handled: true })),

          // Test carbon project validation
          axios
            .post(
              `http://${config.services.carbon.host}:${config.services.carbon.port}/api/carbon/credits/purchase`,
              {
                projectId: 'INVALID',
                quantity: -1,
              }
            )
            .catch(err => ({
              status: err.response?.status,
              handled: err.response?.status === 400,
            })),
        ]);

        const properlyHandled = errorTests.filter(
          result =>
            result.status === 'fulfilled' ||
            (result.status === 'rejected' && result.reason?.handled)
        ).length;

        if (properlyHandled < errorTests.length) {
          throw new Error(
            `Error handling test failed: ${properlyHandled}/${errorTests.length} properly handled`
          );
        }

        return { errorTestsPassed: properlyHandled };
      },
    },
    {
      name: 'Data Validation Resilience',
      test: async () => {
        // Test that services properly validate and reject invalid data
        const validationTests = [
          // Weather API validation
          axios
            .post(
              `http://${config.services.weather.host}:${config.services.weather.port}/api/weather/data`,
              {
                stationId: '', // Invalid empty station ID
                temperature: 'not_a_number', // Invalid temperature
                timestamp: 'invalid_date', // Invalid timestamp
              }
            )
            .catch(err => err.response?.status === 400),

          // Carbon footprint calculation validation
          axios
            .post(
              `http://${config.services.carbon.host}:${config.services.carbon.port}/api/carbon/calculate`,
              {
                activities: 'not_an_array', // Invalid activities format
              }
            )
            .catch(err => err.response?.status === 400),
        ];

        const results = await Promise.allSettled(validationTests);
        const validationsPassed = results.filter(
          r => r.status === 'fulfilled' && r.value === true
        ).length;

        if (validationsPassed < validationTests.length) {
          throw new Error(
            `Validation resilience test failed: ${validationsPassed}/${validationTests.length} validations passed`
          );
        }

        return { validationTestsPassed: validationsPassed };
      },
    },
  ];

  return executeTestSuite('chaos', tests);
}

/**
 * End-to-End Tests - Complete user journey simulation
 */
async function runE2ETests() {
  const tests = [
    {
      name: 'Complete Weather-to-Carbon Journey',
      test: async () => {
        // 1. Get weather station data
        const stationsResponse = await axios.get(
          `http://${config.services.weather.host}:${config.services.weather.port}/api/weather/stations`
        );
        const station = stationsResponse.data.stations[0];

        // 2. Get current weather data
        const weatherResponse = await axios.get(
          `http://${config.services.weather.host}:${config.services.weather.port}/api/weather/stations/${station.stationId}/current`
        );

        // 3. Calculate carbon footprint (simulate based on weather data)
        const carbonResponse = await axios.post(
          `http://${config.services.carbon.host}:${config.services.carbon.port}/api/carbon/calculate`,
          {
            activities: [
              {
                type: 'electricity',
                fuel: 'grid_average_us',
                amount: 100,
                unit: 'kWh',
              },
              {
                type: 'transportation',
                fuel: 'car_gasoline',
                amount: 50,
                unit: 'km',
              },
            ],
          }
        );

        // 4. Get offset recommendations
        const offsetRecommendations = carbonResponse.data.recommendations;

        // 5. Purchase carbon credits
        const purchaseResponse = await axios.post(
          `http://${config.services.carbon.host}:${config.services.carbon.port}/api/carbon/credits/purchase`,
          {
            projectId: offsetRecommendations.recommendedProjects[0].projectId,
            quantity: Math.ceil(offsetRecommendations.offsetAmount),
            buyerInfo: { name: 'E2E Test User', email: 'e2e@wrfcoin.com' },
          }
        );

        // 6. Record transaction on blockchain
        const blockchainResponse = await axios.post(
          `http://${config.services.blockchain.host}:${config.services.blockchain.port}`,
          {
            jsonrpc: '2.0',
            method: 'eth_sendTransaction',
            params: [
              {
                from: '0x1234567890123456789012345678901234567890',
                to: '0x0987654321098765432109876543210987654321',
                data: JSON.stringify({
                  type: 'carbon_offset',
                  transactionId: purchaseResponse.data.transaction.id,
                  emissions: carbonResponse.data.totalEmissions,
                  offset: offsetRecommendations.offsetAmount,
                }),
              },
            ],
            id: 1,
          }
        );

        return {
          stationId: station.stationId,
          emissions: carbonResponse.data.totalEmissions,
          offsetAmount: offsetRecommendations.offsetAmount,
          transactionId: purchaseResponse.data.transaction.id,
          blockchainTx: blockchainResponse.data.result,
          journeyComplete: true,
        };
      },
    },
  ];

  return executeTestSuite('e2e', tests);
}

/**
 * Execute a test suite
 */
async function executeTestSuite(suiteName, tests) {
  const suite = config.testSuites[suiteName];
  const startTime = Date.now();

  console.log(`Starting ${suite.name}...`);

  const results = {
    suite: suiteName,
    name: suite.name,
    description: suite.description,
    startTime: new Date(startTime).toISOString(),
    tests: [],
    summary: {
      total: tests.length,
      passed: 0,
      failed: 0,
      duration: 0,
    },
  };

  for (const test of tests) {
    const testStartTime = Date.now();
    console.log(`  Running: ${test.name}`);

    try {
      const testResult = await Promise.race([
        test.test(),
        new Promise((_, reject) =>
          setTimeout(() => reject(new Error('Test timeout')), suite.timeout)
        ),
      ]);

      const testDuration = Date.now() - testStartTime;
      results.tests.push({
        name: test.name,
        status: 'passed',
        duration: testDuration,
        result: testResult,
      });
      results.summary.passed++;
      console.log(`  ✅ ${test.name} (${testDuration}ms)`);
    } catch (error) {
      const testDuration = Date.now() - testStartTime;
      results.tests.push({
        name: test.name,
        status: 'failed',
        duration: testDuration,
        error: error.message,
        stack: error.stack,
      });
      results.summary.failed++;
      console.log(`  ❌ ${test.name} (${testDuration}ms): ${error.message}`);
    }
  }

  results.summary.duration = Date.now() - startTime;
  results.endTime = new Date().toISOString();

  console.log(
    `${suite.name} completed: ${results.summary.passed}/${results.summary.total} passed (${results.summary.duration}ms)`
  );

  return results;
}

/**
 * Run specific test suite
 */
app.post('/api/test/run/:suite', async (req, res) => {
  const { suite } = req.params;

  if (currentTestRun) {
    return res.status(409).json({
      error: 'Test already running',
      currentTestRun: {
        id: currentTestRun.id,
        suite: currentTestRun.suite,
        status: currentTestRun.status,
      },
    });
  }

  if (!config.testSuites[suite]) {
    return res.status(404).json({
      error: 'Test suite not found',
      availableSuites: Object.keys(config.testSuites),
    });
  }

  const testRunId = `run-${++testRunCounter}-${Date.now()}`;
  currentTestRun = {
    id: testRunId,
    suite,
    status: 'running',
    startTime: new Date().toISOString(),
  };

  res.json({
    testRunId,
    suite,
    status: 'started',
    message: `Test suite '${suite}' started`,
    estimatedDuration: config.testSuites[suite].timeout,
  });

  // Run tests asynchronously
  try {
    let results;
    switch (suite) {
      case 'smoke':
        results = await runSmokeTests();
        break;
      case 'integration':
        results = await runIntegrationTests();
        break;
      case 'load':
        results = await runLoadTests();
        break;
      case 'chaos':
        results = await runChaosTests();
        break;
      case 'e2e':
        results = await runE2ETests();
        break;
      default:
        throw new Error('Unknown test suite');
    }

    results.testRunId = testRunId;
    currentTestRun.status = 'completed';
    currentTestRun.endTime = new Date().toISOString();
    currentTestRun.results = results;

    testResults.set(testRunId, results);
    testHistory.push({
      id: testRunId,
      suite,
      status: 'completed',
      timestamp: new Date().toISOString(),
      summary: results.summary,
    });
  } catch (error) {
    currentTestRun.status = 'failed';
    currentTestRun.error = error.message;
    currentTestRun.endTime = new Date().toISOString();

    testHistory.push({
      id: testRunId,
      suite,
      status: 'failed',
      timestamp: new Date().toISOString(),
      error: error.message,
    });
  } finally {
    // Clear current test run after a delay to allow status checks
    setTimeout(() => {
      if (currentTestRun && currentTestRun.id === testRunId) {
        currentTestRun = null;
      }
    }, 60000);
  }
});

/**
 * Get test results
 */
app.get('/api/test/results/:testRunId', (req, res) => {
  const { testRunId } = req.params;
  const results = testResults.get(testRunId);

  if (!results) {
    return res.status(404).json({
      error: 'Test results not found',
      testRunId,
    });
  }

  res.json(results);
});

/**
 * Get test history
 */
app.get('/api/test/history', (req, res) => {
  const { limit = 20, offset = 0 } = req.query;

  const history = testHistory
    .slice(-parseInt(limit) - parseInt(offset))
    .slice(-parseInt(limit));

  res.json({
    history: history.reverse(),
    total: testHistory.length,
    hasMore: testHistory.length > parseInt(offset) + parseInt(limit),
  });
});

/**
 * Get available test suites
 */
app.get('/api/test/suites', (req, res) => {
  res.json({
    suites: Object.entries(config.testSuites).map(([key, suite]) => ({
      id: key,
      name: suite.name,
      description: suite.description,
      timeout: suite.timeout,
    })),
  });
});

/**
 * Run all test suites
 */
app.post('/api/test/run-all', async (req, res) => {
  if (currentTestRun) {
    return res.status(409).json({
      error: 'Test already running',
    });
  }

  const testRunId = `full-run-${++testRunCounter}-${Date.now()}`;
  const suites = ['smoke', 'integration', 'load', 'chaos', 'e2e'];

  currentTestRun = {
    id: testRunId,
    suite: 'all',
    suites,
    status: 'running',
    startTime: new Date().toISOString(),
    progress: { completed: 0, total: suites.length },
  };

  res.json({
    testRunId,
    suites,
    status: 'started',
    message: 'Full test suite started',
    estimatedDuration: Object.values(config.testSuites).reduce(
      (sum, suite) => sum + suite.timeout,
      0
    ),
  });

  // Run all test suites
  const allResults = {
    testRunId,
    suites: [],
    summary: {
      totalSuites: suites.length,
      passedSuites: 0,
      failedSuites: 0,
      totalTests: 0,
      passedTests: 0,
      failedTests: 0,
      duration: 0,
    },
  };

  const fullStartTime = Date.now();

  for (const suite of suites) {
    try {
      let results;
      switch (suite) {
        case 'smoke':
          results = await runSmokeTests();
          break;
        case 'integration':
          results = await runIntegrationTests();
          break;
        case 'load':
          results = await runLoadTests();
          break;
        case 'chaos':
          results = await runChaosTests();
          break;
        case 'e2e':
          results = await runE2ETests();
          break;
      }

      allResults.suites.push(results);
      allResults.summary.totalTests += results.summary.total;
      allResults.summary.passedTests += results.summary.passed;
      allResults.summary.failedTests += results.summary.failed;

      if (results.summary.failed === 0) {
        allResults.summary.passedSuites++;
      } else {
        allResults.summary.failedSuites++;
      }

      currentTestRun.progress.completed++;
    } catch (error) {
      allResults.suites.push({
        suite,
        error: error.message,
        status: 'failed',
      });
      allResults.summary.failedSuites++;
    }
  }

  allResults.summary.duration = Date.now() - fullStartTime;

  currentTestRun.status = 'completed';
  currentTestRun.endTime = new Date().toISOString();
  currentTestRun.results = allResults;

  testResults.set(testRunId, allResults);
  testHistory.push({
    id: testRunId,
    suite: 'all',
    status: 'completed',
    timestamp: new Date().toISOString(),
    summary: allResults.summary,
  });

  setTimeout(() => {
    if (currentTestRun && currentTestRun.id === testRunId) {
      currentTestRun = null;
    }
  }, 60000);
});

/**
 * Get current test status
 */
app.get('/api/test/status', (req, res) => {
  if (!currentTestRun) {
    return res.json({
      status: 'idle',
      message: 'No tests currently running',
    });
  }

  res.json(currentTestRun);
});

// Start server
app.listen(PORT, '0.0.0.0', () => {
  console.log(`Integration Test Runner running on port ${PORT}`);
  console.log(
    `Configured services: ${Object.keys(config.services).join(', ')}`
  );
  console.log(
    `Available test suites: ${Object.keys(config.testSuites).join(', ')}`
  );
  console.log(`Dashboard: http://localhost:${PORT}/api/test/suites`);
});
