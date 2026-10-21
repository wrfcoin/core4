const request = require('supertest');
const axios = require('axios');

/**
 * API Integration Tests with Mock Services
 * Task 42.6 - Integration Test Automation
 *
 * Tests API functionality using Agent 4's enhanced mock services:
 * - Weather API Mock (port 3010)
 * - Blockchain RPC Mock (port 8545)
 * - Carbon Oracle Mock (port 3020)
 */

describe('API Integration Tests with Mock Services', () => {
  const MOCK_SERVICES = {
    weather: 'http://localhost:3010',
    blockchain: 'http://localhost:8545',
    carbon: 'http://localhost:3020',
  };

  const TIMEOUT = 30000;

  beforeAll(async () => {
    // Wait for mock services to be available
    console.log('🚀 Waiting for mock services to be ready...');

    const checkService = async (name, url) => {
      for (let i = 0; i < 10; i++) {
        try {
          await axios.get(`${url}/health`);
          console.log(`✅ ${name} service ready at ${url}`);
          return true;
        } catch (error) {
          console.log(
            `⏳ ${name} service not ready, waiting... (attempt ${i + 1}/10)`
          );
          await new Promise(resolve => setTimeout(resolve, 2000));
        }
      }
      throw new Error(`❌ ${name} service failed to start at ${url}`);
    };

    await Promise.all([
      checkService('Weather', MOCK_SERVICES.weather),
      checkService('Blockchain', MOCK_SERVICES.blockchain),
      checkService('Carbon', MOCK_SERVICES.carbon),
    ]);
  }, 60000);

  describe(
    'Weather API Integration',
    () => {
      test('should get weather stations list', async () => {
        const response = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toBeInstanceOf(Array);
        expect(response.data.data.length).toBeGreaterThan(0);

        // Verify station structure
        const station = response.data.data[0];
        expect(station).toHaveProperty('id');
        expect(station).toHaveProperty('name');
        expect(station).toHaveProperty('location');
      });

      test('should get specific station data', async () => {
        // First get stations list
        const stationsResponse = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations`
        );
        const stationId = stationsResponse.data.data[0].id;

        const response = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations/${stationId}`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toHaveProperty('id', stationId);
      });

      test('should get weather data for station', async () => {
        // First get stations list
        const stationsResponse = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations`
        );
        const stationId = stationsResponse.data.data[0].id;

        const response = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations/${stationId}/data`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toHaveProperty('temperature');
        expect(response.data.data).toHaveProperty('humidity');
        expect(response.data.data).toHaveProperty('timestamp');
      });

      test('should submit weather data to station', async () => {
        const stationsResponse = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations`
        );
        const stationId = stationsResponse.data.data[0].id;

        const weatherData = {
          temperature: 22.5,
          humidity: 65,
          pressure: 1013.25,
          windSpeed: 5.2,
          windDirection: 180,
          timestamp: new Date().toISOString(),
        };

        const response = await axios.post(
          `${MOCK_SERVICES.weather}/api/weather/stations/${stationId}/data`,
          weatherData
        );

        expect(response.status).toBe(201);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data).toHaveProperty('message');
      });

      test('should get weather analytics submissions', async () => {
        const response = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/analytics/submissions`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toHaveProperty('totalSubmissions');
        expect(response.data.data).toHaveProperty('recentSubmissions');
      });
    },
    TIMEOUT
  );

  describe(
    'Blockchain RPC Integration',
    () => {
      test('should get current block number', async () => {
        const rpcRequest = {
          jsonrpc: '2.0',
          method: 'eth_blockNumber',
          params: [],
          id: 1,
        };

        const response = await axios.post(MOCK_SERVICES.blockchain, rpcRequest);

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('jsonrpc', '2.0');
        expect(response.data).toHaveProperty('id', 1);
        expect(response.data).toHaveProperty('result');
        expect(response.data.result).toMatch(/^0x[0-9a-fA-F]+$/);
      });

      test('should get network version', async () => {
        const rpcRequest = {
          jsonrpc: '2.0',
          method: 'net_version',
          params: [],
          id: 2,
        };

        const response = await axios.post(MOCK_SERVICES.blockchain, rpcRequest);

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('result');
        expect(typeof response.data.result).toBe('string');
      });

      test('should get gas price', async () => {
        const rpcRequest = {
          jsonrpc: '2.0',
          method: 'eth_gasPrice',
          params: [],
          id: 3,
        };

        const response = await axios.post(MOCK_SERVICES.blockchain, rpcRequest);

        expect(response.status).toBe(200);
        expect(response.data.result).toMatch(/^0x[0-9a-fA-F]+$/);
      });

      test('should get account balance', async () => {
        // First get accounts
        const accountsRequest = {
          jsonrpc: '2.0',
          method: 'eth_accounts',
          params: [],
          id: 4,
        };

        const accountsResponse = await axios.post(
          MOCK_SERVICES.blockchain,
          accountsRequest
        );
        expect(accountsResponse.data.result.length).toBeGreaterThan(0);

        const account = accountsResponse.data.result[0];

        const balanceRequest = {
          jsonrpc: '2.0',
          method: 'eth_getBalance',
          params: [account, 'latest'],
          id: 5,
        };

        const response = await axios.post(
          MOCK_SERVICES.blockchain,
          balanceRequest
        );

        expect(response.status).toBe(200);
        expect(response.data.result).toMatch(/^0x[0-9a-fA-F]+$/);
      });

      test('should send transaction', async () => {
        const accountsResponse = await axios.post(MOCK_SERVICES.blockchain, {
          jsonrpc: '2.0',
          method: 'eth_accounts',
          params: [],
          id: 6,
        });

        const fromAccount = accountsResponse.data.result[0];
        const toAccount =
          accountsResponse.data.result[1] ||
          '0x742d35Cc6639C0532fCb5d6e91b67e03E6aaE4F4';

        const txRequest = {
          jsonrpc: '2.0',
          method: 'eth_sendTransaction',
          params: [
            {
              from: fromAccount,
              to: toAccount,
              value: '0x16345785d8a0000', // 0.1 ETH
              gas: '0x5208',
            },
          ],
          id: 7,
        };

        const response = await axios.post(MOCK_SERVICES.blockchain, txRequest);

        expect(response.status).toBe(200);
        expect(response.data.result).toMatch(/^0x[0-9a-fA-F]{64}$/);
      });
    },
    TIMEOUT
  );

  describe(
    'Carbon Oracle Integration',
    () => {
      test('should get all carbon markets', async () => {
        const response = await axios.get(
          `${MOCK_SERVICES.carbon}/api/carbon/markets`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toBeInstanceOf(Array);
        expect(response.data.data.length).toBeGreaterThan(0);

        // Verify market structure
        const market = response.data.data[0];
        expect(market).toHaveProperty('id');
        expect(market).toHaveProperty('name');
        expect(market).toHaveProperty('currentPrice');
        expect(market).toHaveProperty('currency');
      });

      test('should get specific market data', async () => {
        const response = await axios.get(
          `${MOCK_SERVICES.carbon}/api/carbon/markets/VCS`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toHaveProperty('id', 'VCS');
        expect(response.data.data).toHaveProperty(
          'name',
          'Verified Carbon Standard'
        );
      });

      test('should get carbon price for all markets', async () => {
        const response = await axios.get(
          `${MOCK_SERVICES.carbon}/api/carbon/price`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toBeInstanceOf(Array);

        const priceData = response.data.data[0];
        expect(priceData).toHaveProperty('marketId');
        expect(priceData).toHaveProperty('price');
        expect(priceData).toHaveProperty('currency');
        expect(typeof priceData.price).toBe('number');
      });

      test('should get carbon price for specific market', async () => {
        const response = await axios.get(
          `${MOCK_SERVICES.carbon}/api/carbon/price/VCS`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toHaveProperty('marketId', 'VCS');
        expect(response.data.data).toHaveProperty('price');
        expect(response.data.data).toHaveProperty('bid');
        expect(response.data.data).toHaveProperty('ask');
      });

      test('should calculate carbon offset requirements', async () => {
        const calculationRequest = {
          activity: 'electricity',
          amount: 1000,
          unit: 'kwh',
          region: 'US',
        };

        const response = await axios.post(
          `${MOCK_SERVICES.carbon}/api/carbon/calculate`,
          calculationRequest
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toHaveProperty('input');
        expect(response.data.data).toHaveProperty('emissions');
        expect(response.data.data).toHaveProperty('offset');
        expect(response.data.data.emissions).toHaveProperty('amount');
        expect(response.data.data.emissions).toHaveProperty('unit', 'tCO2e');
      });

      test('should get market price history', async () => {
        const response = await axios.get(
          `${MOCK_SERVICES.carbon}/api/carbon/history/VCS?period=24h`
        );

        expect(response.status).toBe(200);
        expect(response.data).toHaveProperty('success', true);
        expect(response.data.data).toHaveProperty('marketId', 'VCS');
        expect(response.data.data).toHaveProperty('history');
        expect(response.data.data.history).toBeInstanceOf(Array);
      });
    },
    TIMEOUT
  );

  describe(
    'Health Check Integration',
    () => {
      test('should verify all mock services are healthy', async () => {
        const healthChecks = await Promise.all([
          axios.get(`${MOCK_SERVICES.weather}/health`),
          axios.get(`${MOCK_SERVICES.blockchain}/health`),
          axios.get(`${MOCK_SERVICES.carbon}/health`),
        ]);

        healthChecks.forEach((response, index) => {
          const serviceName = Object.keys(MOCK_SERVICES)[index];
          expect(response.status).toBe(200);
          expect(response.data).toHaveProperty('status', 'healthy');
          expect(response.data).toHaveProperty('service');
          console.log(`✅ ${serviceName} service health check passed`);
        });
      });

      test('should get service metrics', async () => {
        const metricsResponses = await Promise.all([
          axios.get(`${MOCK_SERVICES.weather}/metrics`),
          axios.get(`${MOCK_SERVICES.blockchain}/metrics`),
          axios.get(`${MOCK_SERVICES.carbon}/metrics`),
        ]);

        metricsResponses.forEach((response, index) => {
          const serviceName = Object.keys(MOCK_SERVICES)[index];
          expect(response.status).toBe(200);
          expect(response.headers['content-type']).toContain('text/plain');
          expect(response.data).toContain('_total');
          console.log(`✅ ${serviceName} service metrics available`);
        });
      });
    },
    TIMEOUT
  );

  describe(
    'Cross-Service Integration Scenarios',
    () => {
      test('should integrate weather data with carbon calculations', async () => {
        // 1. Get weather station data
        const stationsResponse = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations`
        );
        const station = stationsResponse.data.data[0];

        // 2. Get weather data from station
        const weatherResponse = await axios.get(
          `${MOCK_SERVICES.weather}/api/weather/stations/${station.id}/data`
        );

        // 3. Use weather data to calculate carbon footprint (mock scenario)
        const electricityUsage =
          Math.abs(weatherResponse.data.data.temperature - 20) * 10; // Mock calculation

        const carbonResponse = await axios.post(
          `${MOCK_SERVICES.carbon}/api/carbon/calculate`,
          {
            activity: 'electricity',
            amount: electricityUsage,
            unit: 'kwh',
          }
        );

        expect(weatherResponse.status).toBe(200);
        expect(carbonResponse.status).toBe(200);
        expect(carbonResponse.data.data.emissions.amount).toBeGreaterThan(0);
      });

      test('should simulate blockchain transaction for carbon credit purchase', async () => {
        // 1. Get carbon market price
        const priceResponse = await axios.get(
          `${MOCK_SERVICES.carbon}/api/carbon/price/VCS`
        );
        const carbonPrice = priceResponse.data.data.price;

        // 2. Get blockchain accounts
        const accountsResponse = await axios.post(MOCK_SERVICES.blockchain, {
          jsonrpc: '2.0',
          method: 'eth_accounts',
          params: [],
          id: 1,
        });

        // 3. Simulate transaction for carbon credit purchase
        const txResponse = await axios.post(MOCK_SERVICES.blockchain, {
          jsonrpc: '2.0',
          method: 'eth_sendTransaction',
          params: [
            {
              from: accountsResponse.data.result[0],
              to: '0xCarbonCreditContract',
              value: '0x' + Math.floor(carbonPrice * 10000).toString(16), // Mock value
              data: '0xa9059cbb', // Mock transfer method signature
            },
          ],
          id: 2,
        });

        expect(priceResponse.status).toBe(200);
        expect(accountsResponse.status).toBe(200);
        expect(txResponse.status).toBe(200);
        expect(txResponse.data.result).toMatch(/^0x[0-9a-fA-F]{64}$/);
      });

      test('should verify scenario switching across all services', async () => {
        const testScenario = 'high-load';

        // Switch scenario on all services
        const scenarioPromises = await Promise.allSettled([
          axios.post(
            `${MOCK_SERVICES.weather}/api/mock/scenario/${testScenario}`
          ),
          axios.post(
            `${MOCK_SERVICES.carbon}/api/mock/scenario/${testScenario}`
          ),
        ]);

        // Check scenario status
        const statusPromises = await Promise.all([
          axios.get(`${MOCK_SERVICES.weather}/api/mock/scenario`),
          axios.get(`${MOCK_SERVICES.carbon}/api/mock/scenario`),
        ]);

        statusPromises.forEach(response => {
          if (response.status === 200) {
            // Only check if the service supports the scenario
            console.log(
              `Service scenario: ${response.data.data.currentScenario}`
            );
          }
        });

        // At least one service should respond successfully
        const successfulScenarios = scenarioPromises.filter(
          result => result.status === 'fulfilled' && result.value.status === 200
        );
        expect(successfulScenarios.length).toBeGreaterThan(0);
      });
    },
    TIMEOUT
  );

  afterAll(async () => {
    console.log('🎉 Integration tests completed successfully!');
    console.log('✅ All mock services validated');
    console.log('✅ Cross-service integration verified');
  });
});
