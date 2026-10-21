/**
 * K6 Progressive Load Test
 * Gradually increases load to find system breaking points
 *
 * This test identifies:
 * - Maximum sustainable load
 * - Performance degradation patterns
 * - Resource utilization thresholds
 * - Recovery capabilities after load spikes
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Counter, Rate, Trend, Gauge } from 'k6/metrics';
import {
  randomIntBetween,
  randomItem,
} from 'https://jslib.k6.io/k6-utils/1.2.0/index.js';
import { SERVICE_REGISTRY } from '../k6.config.js';

// Progressive Load Metrics
const systemLoadLevel = new Gauge('system_load_level');
const responseTimeProgression = new Trend('response_time_progression');
const errorRateProgression = new Rate('error_rate_progression');
const resourceUtilization = new Gauge('resource_utilization');
const systemBreakingPoint = new Gauge('system_breaking_point');
const recoveryTime = new Trend('system_recovery_time');
const sustainableLoad = new Gauge('maximum_sustainable_load');

export let options = {
  scenarios: {
    // Main progressive load test
    progressive_load: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        // Phase 1: Baseline establishment
        { duration: '2m', target: 10 }, // Establish baseline
        { duration: '2m', target: 10 }, // Maintain baseline

        // Phase 2: Light load progression
        { duration: '2m', target: 25 }, // Light load
        { duration: '2m', target: 25 }, // Sustain light

        // Phase 3: Medium load progression
        { duration: '2m', target: 50 }, // Medium load
        { duration: '3m', target: 50 }, // Sustain medium

        // Phase 4: High load progression
        { duration: '2m', target: 100 }, // High load
        { duration: '3m', target: 100 }, // Sustain high

        // Phase 5: Stress load progression
        { duration: '2m', target: 200 }, // Stress load
        { duration: '3m', target: 200 }, // Sustain stress

        // Phase 6: Breaking point test
        { duration: '1m', target: 300 }, // Approach breaking point
        { duration: '2m', target: 300 }, // Test breaking point
        { duration: '1m', target: 400 }, // Push to failure
        { duration: '2m', target: 400 }, // Maintain failure load

        // Phase 7: Recovery testing
        { duration: '1m', target: 200 }, // Quick recovery
        { duration: '2m', target: 100 }, // Stabilize
        { duration: '2m', target: 50 }, // Return to sustainable
        { duration: '2m', target: 0 }, // Complete recovery
      ],
      tags: { test_type: 'progressive_load' },
    },

    // Concurrent monitoring during progressive load
    system_monitoring: {
      executor: 'constant-vus',
      vus: 5,
      duration: '35m', // Same duration as main test
      tags: { test_type: 'system_monitoring' },
    },
  },

  thresholds: {
    http_req_duration: [
      'p(50)<200', // Baseline performance
      'p(95)<1000', // Acceptable degradation
      'p(99)<5000', // Maximum acceptable
    ],
    http_req_failed: ['rate<0.05'], // 5% error rate threshold
    response_time_progression: ['p(95)<2000'], // Progressive degradation
    error_rate_progression: ['rate<0.10'], // 10% error rate at peak
    system_load_level: ['value<=400'], // Maximum load level
    resource_utilization: ['value<=0.95'], // 95% resource limit
    system_recovery_time: ['p(95)<30000'], // 30s recovery time
  },
};

// Load phases for tracking progression
const LOAD_PHASES = [
  { name: 'baseline', vus: 10, duration: 240, expectedRT: 150 },
  { name: 'light', vus: 25, duration: 240, expectedRT: 200 },
  { name: 'medium', vus: 50, duration: 300, expectedRT: 300 },
  { name: 'high', vus: 100, duration: 300, expectedRT: 500 },
  { name: 'stress', vus: 200, duration: 300, expectedRT: 800 },
  { name: 'breaking', vus: 300, duration: 180, expectedRT: 1500 },
  { name: 'failure', vus: 400, duration: 180, expectedRT: 3000 },
];

// System endpoints for comprehensive testing
const SYSTEM_ENDPOINTS = [
  {
    name: 'iot_sensor_post',
    url: `${SERVICE_REGISTRY.iot.host}:${SERVICE_REGISTRY.iot.port}/api/sensor/readings`,
    method: 'POST',
    weight: 40, // Primary load target
    payload: () => generateSensorData(),
  },
  {
    name: 'weather_data_query',
    url: `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/weather/recent`,
    method: 'GET',
    weight: 25,
    payload: null,
  },
  {
    name: 'wallet_balance',
    url: `${SERVICE_REGISTRY.wallet.host}:${SERVICE_REGISTRY.wallet.port}/api/wallet/balance`,
    method: 'GET',
    weight: 15,
    payload: null,
  },
  {
    name: 'cross_chain_status',
    url: `${SERVICE_REGISTRY.crossChain.host}:${SERVICE_REGISTRY.crossChain.port}/api/bridge/status`,
    method: 'GET',
    weight: 10,
    payload: null,
  },
  {
    name: 'governance_proposals',
    url: `${SERVICE_REGISTRY.governance.host}:${SERVICE_REGISTRY.governance.port}/api/proposals`,
    method: 'GET',
    weight: 10,
    payload: null,
  },
];

let currentPhase = 'baseline';
let phaseStartTime = Date.now();
let systemFailureDetected = false;
let maxSustainableLoadFound = false;

export default function () {
  // Determine current load phase based on VU count
  const currentVUs = __VU;
  updateCurrentPhase(currentVUs);

  // Record current load level
  systemLoadLevel.add(currentVUs);

  group('Progressive Load Testing', function () {
    // Execute weighted endpoint testing
    const endpoint = selectEndpointByWeight();
    const startTime = Date.now();

    let response;

    if (endpoint.method === 'POST' && endpoint.payload) {
      response = http.post(endpoint.url, JSON.stringify(endpoint.payload()), {
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${__ENV.API_TOKEN || 'test-token-001'}`,
        },
        tags: {
          endpoint: endpoint.name,
          phase: currentPhase,
          load_level: currentVUs,
        },
      });
    } else {
      response = http.get(endpoint.url, {
        headers: {
          Authorization: `Bearer ${__ENV.API_TOKEN || 'test-token-001'}`,
        },
        tags: {
          endpoint: endpoint.name,
          phase: currentPhase,
          load_level: currentVUs,
        },
      });
    }

    const responseTime = Date.now() - startTime;
    responseTimeProgression.add(responseTime);

    // Check for system degradation indicators
    const checks = check(response, {
      'Status is successful': r => r.status >= 200 && r.status < 300,
      'Response time reasonable': r =>
        responseTime < getCurrentPhaseExpectedRT(),
      'Response contains data': r => r.body && r.body.length > 0,
      'No server errors': r =>
        r.status !== 500 && r.status !== 502 && r.status !== 503,
    });

    // Track error progression
    const hasErrors = !checks;
    errorRateProgression.add(hasErrors ? 1 : 0);

    // Detect system failure patterns
    if (hasErrors || responseTime > getCurrentPhaseExpectedRT() * 3) {
      detectSystemFailure(currentVUs, responseTime);
    }

    // Track resource utilization if available in response headers
    if (response.headers['X-System-CPU-Usage']) {
      resourceUtilization.add(
        parseFloat(response.headers['X-System-CPU-Usage']) / 100
      );
    }
  });

  // System monitoring scenario
  if (__SCENARIO === 'system_monitoring') {
    performSystemMonitoring();
  }

  // Adaptive sleep based on current load level
  sleep(adaptiveSleep(currentVUs));
}

function updateCurrentPhase(vus) {
  const phase =
    LOAD_PHASES.find(p => vus <= p.vus) || LOAD_PHASES[LOAD_PHASES.length - 1];

  if (phase.name !== currentPhase) {
    console.log(
      `📊 Phase transition: ${currentPhase} -> ${phase.name} (${vus} VUs)`
    );
    currentPhase = phase.name;
    phaseStartTime = Date.now();
  }
}

function selectEndpointByWeight() {
  const totalWeight = SYSTEM_ENDPOINTS.reduce((sum, ep) => sum + ep.weight, 0);
  const random = Math.random() * totalWeight;

  let weightSum = 0;
  for (const endpoint of SYSTEM_ENDPOINTS) {
    weightSum += endpoint.weight;
    if (random <= weightSum) {
      return endpoint;
    }
  }

  return SYSTEM_ENDPOINTS[0]; // Fallback
}

function getCurrentPhaseExpectedRT() {
  const phase = LOAD_PHASES.find(p => p.name === currentPhase);
  return phase ? phase.expectedRT : 1000;
}

function detectSystemFailure(vus, responseTime) {
  const phase = LOAD_PHASES.find(p => p.name === currentPhase);
  const expectedRT = phase ? phase.expectedRT : 1000;

  // System failure indicators
  const isFailure =
    responseTime > expectedRT * 5 || // 5x expected response time
    responseTime > 10000; // 10 second timeout

  if (isFailure && !systemFailureDetected) {
    systemFailureDetected = true;
    systemBreakingPoint.add(vus);
    console.log(
      `❌ System failure detected at ${vus} VUs (${responseTime}ms response time)`
    );
  }

  // Track maximum sustainable load
  if (
    !maxSustainableLoadFound &&
    currentPhase !== 'baseline' &&
    currentPhase !== 'light'
  ) {
    const errorRate = errorRateProgression.rate || 0;

    if (errorRate < 0.01 && responseTime < expectedRT * 2) {
      sustainableLoad.add(vus);
      console.log(`✅ Sustainable load confirmed: ${vus} VUs`);
    } else if (errorRate >= 0.05) {
      maxSustainableLoadFound = true;
      console.log(`⚠️ Maximum sustainable load reached at ${vus - 50} VUs`);
    }
  }
}

function performSystemMonitoring() {
  group('System Health Monitoring', function () {
    // Monitor core services health
    const services = ['mainApi', 'iot', 'wallet', 'crossChain'];
    services.forEach(serviceName => {
      const service = SERVICE_REGISTRY[serviceName];

      if (service.port) {
        const healthResponse = http.get(
          `${service.host}:${service.port}/health`,
          {
            timeout: '5s',
            tags: {
              service: serviceName,
              monitoring: 'health_check',
            },
          }
        );

        check(healthResponse, {
          [`${service.name} health check passes`]: r => r.status === 200,
          [`${service.name} responds quickly`]: r => r.timings.duration < 1000,
        });

        // Extract system metrics if available
        if (healthResponse.headers['X-System-Metrics']) {
          try {
            const metrics = JSON.parse(
              healthResponse.headers['X-System-Metrics']
            );
            if (metrics.cpu) resourceUtilization.add(metrics.cpu / 100);
          } catch (e) {
            // Ignore parsing errors
          }
        }
      }
    });

    // Check system recovery if we've detected failure
    if (systemFailureDetected) {
      const recoveryStartTime = Date.now();

      const recoveryResponse = http.get(
        `${SERVICE_REGISTRY.mainApi.host}:${SERVICE_REGISTRY.mainApi.port}/api/health/detailed`,
        {
          tags: { monitoring: 'recovery_check' },
        }
      );

      const isRecovering = check(recoveryResponse, {
        'System is recovering': r => r.status === 200,
        'Recovery response time acceptable': r => r.timings.duration < 2000,
      });

      if (isRecovering) {
        const recoveryTimeMs = Date.now() - recoveryStartTime;
        recoveryTime.add(recoveryTimeMs);
        console.log(`🔄 System recovery detected in ${recoveryTimeMs}ms`);
      }
    }
  });

  sleep(5); // Monitor every 5 seconds
}

function generateSensorData() {
  return {
    stationId: `WS_${randomIntBetween(1, 100)}`,
    timestamp: new Date().toISOString(),
    readings: {
      temperature: randomIntBetween(-10, 40),
      humidity: randomIntBetween(20, 90),
      pressure: randomIntBetween(980, 1040),
      windSpeed: randomIntBetween(0, 50),
    },
    quality: randomItem(['excellent', 'good', 'fair']),
  };
}

function adaptiveSleep(vus) {
  // Reduce sleep time as load increases to maintain pressure
  if (vus < 50) return randomIntBetween(1, 3);
  if (vus < 100) return randomIntBetween(0.5, 2);
  if (vus < 200) return randomIntBetween(0.2, 1);
  return randomIntBetween(0.1, 0.5); // High load = minimal sleep
}

export function setup() {
  console.log('📈 Starting Progressive Load Test');
  console.log('🎯 Objective: Find system breaking points and sustainable load');
  console.log(`🔄 Testing ${LOAD_PHASES.length} load phases over 35 minutes`);

  return {
    startTime: Date.now(),
    phases: LOAD_PHASES,
  };
}

export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;

  console.log(`✅ Progressive load test completed after ${duration} seconds`);
  console.log(
    `📊 Breaking point: ${systemFailureDetected ? 'Detected' : 'Not reached'}`
  );

  if (systemBreakingPoint.value) {
    console.log(`💥 System breaking point: ${systemBreakingPoint.value} VUs`);
  }

  if (sustainableLoad.value) {
    console.log(`✅ Maximum sustainable load: ${sustainableLoad.value} VUs`);
  }
}

export {
  systemLoadLevel,
  responseTimeProgression,
  errorRateProgression,
  resourceUtilization,
  systemBreakingPoint,
  recoveryTime,
  sustainableLoad,
};
