/**
 * K6 Cross-Service Integration Load Test
 * Tests multi-service workflows under load for WRFCoin platform
 *
 * Validates interaction between all 15+ microservices:
 * - Weather data flow through the system
 * - Wallet and transaction processing
 * - Cross-chain bridge operations
 * - Real-time reward calculations
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Counter, Rate, Trend, Gauge } from 'k6/metrics';
import {
  randomIntBetween,
  randomItem,
  randomString,
} from 'https://jslib.k6.io/k6-utils/1.2.0/index.js';
import { SERVICE_REGISTRY, TEST_DATA } from '../k6.config.js';

// Custom Metrics for Cross-Service Testing
const crossServiceTransactionTime = new Trend('cross_service_transaction_time');
const serviceInteractionRate = new Rate('service_interaction_success_rate');
const rewardCalculationTime = new Trend('reward_calculation_time');
const bridgeOperationTime = new Trend('bridge_operation_time');
const serviceHealthGauge = new Gauge('service_health_gauge');
const workflowCompletionRate = new Rate('workflow_completion_rate');

export let options = {
  scenarios: {
    // Multi-service workflow testing
    integration_flow: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '2m', target: 20 }, // Ramp-up
        { duration: '5m', target: 20 }, // Steady state
        { duration: '2m', target: 50 }, // Increase load
        { duration: '8m', target: 50 }, // Sustained load
        { duration: '3m', target: 0 }, // Ramp-down
      ],
      tags: { test_type: 'cross_service_integration' },
    },

    // Real-time processing validation
    realtime_processing: {
      executor: 'constant-vus',
      vus: 30,
      duration: '10m',
      tags: { test_type: 'realtime_processing' },
    },
  },

  thresholds: {
    http_req_duration: ['p(95)<2000'], // Overall response time
    cross_service_transaction_time: ['p(95)<5000'], // End-to-end workflow
    service_interaction_success_rate: ['rate>=0.95'], // 95% success rate
    reward_calculation_time: ['p(95)<1000'], // Reward processing
    bridge_operation_time: ['p(95)<3000'], // Cross-chain ops
    workflow_completion_rate: ['rate>=0.90'], // Workflow completion
    service_health_gauge: ['value>=0.8'], // Service health
  },
};

// Test users with different roles
const TEST_USERS = [
  {
    id: 'user-001',
    role: 'weather_station_owner',
    wallet: '0x1234...',
    token: 'test-token-001',
  },
  {
    id: 'user-002',
    role: 'compute_provider',
    wallet: '0x2345...',
    token: 'test-token-002',
  },
  {
    id: 'user-003',
    role: 'carbon_trader',
    wallet: '0x3456...',
    token: 'test-token-003',
  },
  {
    id: 'user-004',
    role: 'nft_creator',
    wallet: '0x4567...',
    token: 'test-token-004',
  },
  {
    id: 'user-005',
    role: 'governance_voter',
    wallet: '0x5678...',
    token: 'test-token-005',
  },
];

// Simulate comprehensive cross-service workflow
export default function () {
  const user = randomItem(TEST_USERS);
  const workflowStartTime = Date.now();
  let workflowSuccess = true;

  group('Cross-Service Integration Workflow', function () {
    // STEP 1: Weather Data Submission (IoT Service -> Database)
    group('Weather Data Ingestion Flow', function () {
      const weatherData = generateWeatherData(user);

      const iotResponse = http.post(
        `${SERVICE_REGISTRY.iot.host}:${SERVICE_REGISTRY.iot.port}/api/sensor/readings`,
        JSON.stringify(weatherData),
        {
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${user.token}`,
          },
          tags: { service: 'iot_sensors', workflow_step: '1_data_ingestion' },
        }
      );

      const iotSuccess = check(iotResponse, {
        'Weather data submitted': r => r.status === 200 || r.status === 201,
        'Data validated': r => {
          try {
            const body = JSON.parse(r.body);
            return body.status === 'processed' || body.status === 'accepted';
          } catch (e) {
            return false;
          }
        },
      });

      if (!iotSuccess) workflowSuccess = false;
      serviceInteractionRate.add(iotSuccess ? 1 : 0);
    });

    sleep(0.1);

    // STEP 2: Real-time Reward Calculation (Weather Earnings Service)
    group('Reward Calculation Flow', function () {
      const rewardStartTime = Date.now();

      const rewardResponse = http.post(
        `${SERVICE_REGISTRY.weatherEarnings.host}:${SERVICE_REGISTRY.weatherEarnings.port}/api/rewards/calculate`,
        JSON.stringify({
          userId: user.id,
          dataQuality: randomItem(['excellent', 'good', 'fair']),
          dataPoints: randomIntBetween(10, 100),
          timeRange: '1h',
        }),
        {
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${user.token}`,
          },
          tags: {
            service: 'weather_earnings',
            workflow_step: '2_reward_calculation',
          },
        }
      );

      const rewardTime = Date.now() - rewardStartTime;
      rewardCalculationTime.add(rewardTime);

      const rewardSuccess = check(rewardResponse, {
        'Rewards calculated': r => r.status === 200,
        'Reward amount present': r => {
          try {
            const body = JSON.parse(r.body);
            return body.rewardAmount !== undefined;
          } catch (e) {
            return false;
          }
        },
      });

      if (!rewardSuccess) workflowSuccess = false;
      serviceInteractionRate.add(rewardSuccess ? 1 : 0);
    });

    sleep(0.1);

    // STEP 3: Wallet Transaction (Main API -> Wallet Service)
    group('Wallet Transaction Flow', function () {
      const transactionResponse = http.post(
        `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/wallet/transaction`,
        JSON.stringify({
          from: user.wallet,
          to: randomItem(TEST_USERS).wallet,
          amount: randomIntBetween(1, 100).toString(),
          token: 'WRF',
          type: 'reward_payout',
        }),
        {
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${user.token}`,
          },
          tags: { service: 'wallet', workflow_step: '3_wallet_transaction' },
        }
      );

      const txSuccess = check(transactionResponse, {
        'Transaction created': r => r.status === 200 || r.status === 201,
        'Transaction hash present': r => {
          try {
            const body = JSON.parse(r.body);
            return body.transactionHash !== undefined;
          } catch (e) {
            return false;
          }
        },
      });

      if (!txSuccess) workflowSuccess = false;
      serviceInteractionRate.add(txSuccess ? 1 : 0);
    });

    sleep(0.1);

    // STEP 4: Cross-Chain Bridge Operation (if applicable)
    if (Math.random() > 0.7) {
      // 30% chance for cross-chain operation
      group('Cross-Chain Bridge Flow', function () {
        const bridgeStartTime = Date.now();

        const bridgeResponse = http.post(
          `${SERVICE_REGISTRY.crossChain.host}:${SERVICE_REGISTRY.crossChain.port}/api/bridge/initiate`,
          JSON.stringify({
            sourceChain: 'ethereum',
            targetChain: randomItem(['polygon', 'bsc', 'avalanche']),
            amount: randomIntBetween(10, 1000),
            token: 'WRF',
            userAddress: user.wallet,
          }),
          {
            headers: {
              'Content-Type': 'application/json',
              Authorization: `Bearer ${user.token}`,
            },
            tags: {
              service: 'cross_chain',
              workflow_step: '4_bridge_operation',
            },
          }
        );

        const bridgeTime = Date.now() - bridgeStartTime;
        bridgeOperationTime.add(bridgeTime);

        const bridgeSuccess = check(bridgeResponse, {
          'Bridge operation initiated': r =>
            r.status === 200 || r.status === 202,
          'Bridge transaction ID present': r => {
            try {
              const body = JSON.parse(r.body);
              return body.bridgeTransactionId !== undefined;
            } catch (e) {
              return false;
            }
          },
        });

        if (!bridgeSuccess) workflowSuccess = false;
        serviceInteractionRate.add(bridgeSuccess ? 1 : 0);
      });
    }

    sleep(0.1);

    // STEP 5: Carbon Credit Processing (if user is carbon trader)
    if (user.role === 'carbon_trader') {
      group('Carbon Credit Trading Flow', function () {
        const carbonResponse = http.post(
          `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/carbon-credits/purchase`,
          JSON.stringify({
            projectId: randomItem(TEST_DATA.carbonProjects),
            quantity: randomIntBetween(10, 100),
            maxPrice: randomIntBetween(15, 25),
          }),
          {
            headers: {
              'Content-Type': 'application/json',
              Authorization: `Bearer ${user.token}`,
            },
            tags: {
              service: 'carbon_credits',
              workflow_step: '5_carbon_trading',
            },
          }
        );

        const carbonSuccess = check(carbonResponse, {
          'Carbon credits purchased': r => r.status === 200 || r.status === 201,
          'Purchase confirmed': r => {
            try {
              const body = JSON.parse(r.body);
              return body.purchaseId !== undefined;
            } catch (e) {
              return false;
            }
          },
        });

        if (!carbonSuccess) workflowSuccess = false;
        serviceInteractionRate.add(carbonSuccess ? 1 : 0);
      });
    }

    sleep(0.1);

    // STEP 6: Distributed Computing Job (if user is compute provider)
    if (user.role === 'compute_provider') {
      group('Distributed Computing Flow', function () {
        const computeResponse = http.post(
          `${SERVICE_REGISTRY.compute.host}:${SERVICE_REGISTRY.compute.port}/api/jobs/claim`,
          JSON.stringify({
            providerId: user.id,
            capabilities: ['weather_modeling', 'data_processing'],
            maxDuration: 3600, // 1 hour
          }),
          {
            headers: {
              'Content-Type': 'application/json',
              Authorization: `Bearer ${user.token}`,
            },
            tags: {
              service: 'compute_networks',
              workflow_step: '6_compute_job',
            },
          }
        );

        const computeSuccess = check(computeResponse, {
          'Compute job claimed': r => r.status === 200 || r.status === 202,
          'Job assignment received': r => {
            try {
              const body = JSON.parse(r.body);
              return (
                body.jobId !== undefined || body.message === 'no_jobs_available'
              );
            } catch (e) {
              return false;
            }
          },
        });

        if (!computeSuccess) workflowSuccess = false;
        serviceInteractionRate.add(computeSuccess ? 1 : 0);
      });
    }

    // STEP 7: Final Service Health Check
    group('Service Health Validation', function () {
      const healthPromises = [];
      let healthyServices = 0;
      const totalServices = Object.keys(SERVICE_REGISTRY).length;

      // Check core services health
      const coreServices = ['mainApi', 'iot', 'wallet', 'crossChain'];
      coreServices.forEach(serviceName => {
        const service = SERVICE_REGISTRY[serviceName];
        if (service.port) {
          const healthResponse = http.get(
            `${service.host}:${service.port}/health`,
            {
              timeout: '5s',
              tags: { service: serviceName, workflow_step: '7_health_check' },
            }
          );

          const isHealthy = check(healthResponse, {
            [`${service.name} is healthy`]: r => r.status === 200,
          });

          if (isHealthy) healthyServices++;
        }
      });

      const healthRatio = healthyServices / coreServices.length;
      serviceHealthGauge.add(healthRatio);

      if (healthRatio < 0.75) workflowSuccess = false;
    });
  });

  // Record overall workflow metrics
  const totalWorkflowTime = Date.now() - workflowStartTime;
  crossServiceTransactionTime.add(totalWorkflowTime);
  workflowCompletionRate.add(workflowSuccess ? 1 : 0);

  // Adaptive sleep based on system load
  sleep(randomIntBetween(1, 3));
}

// Generate realistic weather data for testing
function generateWeatherData(user) {
  return {
    stationId: `${user.id}_station`,
    timestamp: new Date().toISOString(),
    readings: {
      temperature: randomIntBetween(-10, 40),
      humidity: randomIntBetween(20, 90),
      pressure: randomIntBetween(980, 1040),
      windSpeed: randomIntBetween(0, 50),
      windDirection: randomIntBetween(0, 360),
      rainfall: Math.random() > 0.8 ? randomIntBetween(0, 20) : 0,
    },
    quality: randomItem(['excellent', 'good', 'fair']),
    metadata: {
      firmware: '1.2.3',
      batteryLevel: randomIntBetween(50, 100),
    },
  };
}

export function setup() {
  console.log('🔗 Starting Cross-Service Integration Load Test');
  console.log(`👥 Testing with ${TEST_USERS.length} different user roles`);
  console.log(
    `🏗️ Testing ${Object.keys(SERVICE_REGISTRY).length} microservices`
  );

  return {
    startTime: Date.now(),
    users: TEST_USERS,
  };
}

export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(
    `✅ Cross-service integration test completed after ${duration} seconds`
  );
}

export {
  crossServiceTransactionTime,
  serviceInteractionRate,
  rewardCalculationTime,
  bridgeOperationTime,
  serviceHealthGauge,
  workflowCompletionRate,
};
