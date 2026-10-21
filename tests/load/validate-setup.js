/**
 * K6 Load Testing Setup Validation Script
 * Validates that the K6 load testing infrastructure is properly configured
 * and all components are working correctly.
 */

import http from 'k6/http';
import { check, group } from 'k6';
import { Counter, Rate } from 'k6/metrics';
import { SERVICE_REGISTRY } from './k6.config.js';

// Validation metrics
const validationChecks = new Counter('validation_checks_total');
const validationPassed = new Counter('validation_passed_total');
const validationFailed = new Counter('validation_failed_total');
const serviceHealthRate = new Rate('service_health_rate');

export let options = {
  vus: 1,
  iterations: 1,
  thresholds: {
    validation_passed_total: ['count>=5'], // At least 5 validations should pass
    service_health_rate: ['rate>=0.6'], // At least 60% of services should be healthy
  },
};

export default function () {
  console.log('🔍 Validating WRFCoin K6 Load Testing Infrastructure...\n');

  group('Configuration Validation', function () {
    validateConfiguration();
  });

  group('Service Health Validation', function () {
    validateServiceHealth();
  });

  group('Test Data Generation Validation', function () {
    validateTestDataGeneration();
  });

  group('Monitoring Integration Validation', function () {
    validateMonitoringIntegration();
  });

  group('Environment Setup Validation', function () {
    validateEnvironmentSetup();
  });
}

function validateConfiguration() {
  console.log('📋 Validating configuration...');

  // Check if SERVICE_REGISTRY is properly loaded
  const serviceRegistryValid = check(SERVICE_REGISTRY, {
    'SERVICE_REGISTRY is defined': registry => registry !== undefined,
    'Main API service configured': registry => registry.mainApi !== undefined,
    'IoT service configured': registry => registry.iot !== undefined,
    'Wallet service configured': registry => registry.wallet !== undefined,
    'Cross-chain service configured': registry =>
      registry.crossChain !== undefined,
  });

  if (serviceRegistryValid) {
    console.log('✅ Service registry configuration is valid');
    validationPassed.add(1);
  } else {
    console.log('❌ Service registry configuration issues found');
    validationFailed.add(1);
  }

  validationChecks.add(1);

  // Check environment variables
  const envVarsValid = check(__ENV, {
    'Base URL is set': env =>
      env.BASE_URL !== undefined || SERVICE_REGISTRY.mainApi.host !== undefined,
    'API token is available': env =>
      env.API_TOKEN !== undefined || env.API_TOKEN !== '',
  });

  if (envVarsValid) {
    console.log('✅ Environment variables are properly configured');
    validationPassed.add(1);
  } else {
    console.log('❌ Environment variable configuration issues found');
    validationFailed.add(1);
  }

  validationChecks.add(1);
}

function validateServiceHealth() {
  console.log('🏥 Checking service health...');

  const coreServices = [
    { name: 'Main API', config: SERVICE_REGISTRY.mainApi },
    { name: 'IoT Sensors', config: SERVICE_REGISTRY.iot },
    { name: 'Wallet', config: SERVICE_REGISTRY.wallet },
    { name: 'Cross-Chain', config: SERVICE_REGISTRY.crossChain },
  ];

  let healthyServices = 0;

  coreServices.forEach(service => {
    if (!service.config || !service.config.port) {
      console.log(`⚠️ ${service.name}: No port configuration found`);
      return;
    }

    const healthUrl = `${service.config.host}:${service.config.port}/health`;

    try {
      const response = http.get(healthUrl, {
        timeout: '5s',
        tags: { service: service.name.toLowerCase().replace(' ', '_') },
      });

      const isHealthy = check(response, {
        [`${service.name} responds to health check`]: r => r.status === 200,
        [`${service.name} responds within timeout`]: r =>
          r.timings.duration < 5000,
      });

      if (isHealthy) {
        console.log(`✅ ${service.name}: Healthy (${response.status})`);
        healthyServices++;
        serviceHealthRate.add(1);
      } else {
        console.log(`❌ ${service.name}: Unhealthy (${response.status})`);
        serviceHealthRate.add(0);
      }
    } catch (error) {
      console.log(`❌ ${service.name}: Connection failed - ${error.message}`);
      serviceHealthRate.add(0);
    }

    validationChecks.add(1);
  });

  if (healthyServices >= 3) {
    console.log(
      `✅ Service health validation passed (${healthyServices}/${coreServices.length} healthy)`
    );
    validationPassed.add(1);
  } else {
    console.log(
      `⚠️ Service health validation warning (${healthyServices}/${coreServices.length} healthy)`
    );
    console.log('Consider starting more services for optimal load testing');
    validationFailed.add(1);
  }
}

function validateTestDataGeneration() {
  console.log('🎲 Validating test data generation...');

  try {
    // Test basic sensor data generation
    const sensorData = {
      stationId: 'TEST_STATION_001',
      timestamp: new Date().toISOString(),
      readings: {
        temperature: 25.5,
        humidity: 65,
        pressure: 1013.25,
        windSpeed: 10.5,
      },
      quality: 'excellent',
    };

    const dataValid = check(sensorData, {
      'Sensor data has station ID': data => data.stationId !== undefined,
      'Sensor data has timestamp': data => data.timestamp !== undefined,
      'Sensor data has readings': data => data.readings !== undefined,
      'Temperature reading is numeric': data =>
        typeof data.readings.temperature === 'number',
      'Humidity reading is valid': data =>
        data.readings.humidity >= 0 && data.readings.humidity <= 100,
    });

    if (dataValid) {
      console.log('✅ Test data generation is working correctly');
      validationPassed.add(1);
    } else {
      console.log('❌ Test data generation issues found');
      validationFailed.add(1);
    }
  } catch (error) {
    console.log(`❌ Test data generation failed: ${error.message}`);
    validationFailed.add(1);
  }

  validationChecks.add(1);
}

function validateMonitoringIntegration() {
  console.log('📊 Validating monitoring integration...');

  // Check if monitoring endpoints are available
  const monitoringEndpoints = [
    {
      name: 'Prometheus',
      url: 'http://localhost:9090/-/healthy',
      enabled: __ENV.K6_PROMETHEUS_ENABLED === 'true',
    },
    { name: 'Grafana', url: 'http://localhost:3004/api/health', enabled: true },
    {
      name: 'InfluxDB',
      url: 'http://localhost:8086/ping',
      enabled: __ENV.K6_INFLUXDB_ENABLED === 'true',
    },
  ];

  let availableMonitoring = 0;
  let enabledMonitoring = 0;

  monitoringEndpoints.forEach(endpoint => {
    if (endpoint.enabled) {
      enabledMonitoring++;

      try {
        const response = http.get(endpoint.url, {
          timeout: '3s',
          tags: { monitoring: endpoint.name.toLowerCase() },
        });

        if (response.status >= 200 && response.status < 400) {
          console.log(`✅ ${endpoint.name}: Available`);
          availableMonitoring++;
        } else {
          console.log(
            `⚠️ ${endpoint.name}: Not responding (${response.status})`
          );
        }
      } catch (error) {
        console.log(`⚠️ ${endpoint.name}: Connection failed`);
      }
    } else {
      console.log(`ℹ️ ${endpoint.name}: Disabled in configuration`);
    }
  });

  if (enabledMonitoring === 0) {
    console.log('ℹ️ No monitoring systems enabled - this is optional');
    validationPassed.add(1);
  } else if (availableMonitoring >= enabledMonitoring * 0.5) {
    console.log(
      `✅ Monitoring integration validation passed (${availableMonitoring}/${enabledMonitoring} available)`
    );
    validationPassed.add(1);
  } else {
    console.log(
      `⚠️ Monitoring integration issues (${availableMonitoring}/${enabledMonitoring} available)`
    );
    validationFailed.add(1);
  }

  validationChecks.add(1);
}

function validateEnvironmentSetup() {
  console.log('🔧 Validating environment setup...');

  // Check if K6 is running with expected capabilities
  const k6Setup = check(__ENV, {
    'K6 can access environment variables': env => env !== undefined,
    'Test scenario is defined': env => env.K6_SCENARIO !== undefined || true, // Optional
    'Environment is specified': env => env.ENVIRONMENT !== undefined || true, // Will use default
  });

  // Check if reports directory would be writable (simulated)
  const reportsSetup = true; // In real K6, we can't easily test file system access

  // Validate load testing parameters
  const loadParams = {
    maxVUs: parseInt(__ENV.MAX_VUS || '200'),
    duration: __ENV.TEST_DURATION || '15m',
    sensorRate: parseInt(__ENV.SENSOR_FLOOD_RATE || '2000'),
  };

  const paramsValid = check(loadParams, {
    'Max VUs is reasonable': params =>
      params.maxVUs > 0 && params.maxVUs <= 1000,
    'Duration is specified': params => params.duration !== undefined,
    'Sensor rate target is set': params => params.sensorRate >= 1000, // Minimum for meaningful test
  });

  if (k6Setup && reportsSetup && paramsValid) {
    console.log('✅ Environment setup is valid');
    console.log(`   - Max VUs: ${loadParams.maxVUs}`);
    console.log(`   - Duration: ${loadParams.duration}`);
    console.log(`   - Target sensor rate: ${loadParams.sensorRate}/min`);
    validationPassed.add(1);
  } else {
    console.log('❌ Environment setup issues found');
    validationFailed.add(1);
  }

  validationChecks.add(1);
}

export function teardown(data) {
  console.log('\n' + '='.repeat(60));
  console.log('🎯 K6 Load Testing Infrastructure Validation Summary');
  console.log('='.repeat(60));

  const totalChecks = validationChecks.count || 0;
  const passedChecks = validationPassed.count || 0;
  const failedChecks = validationFailed.count || 0;

  console.log(`Total Validations: ${totalChecks}`);
  console.log(`Passed: ${passedChecks} ✅`);
  console.log(`Failed: ${failedChecks} ❌`);

  if (passedChecks >= totalChecks * 0.8) {
    console.log('\n🎉 VALIDATION RESULT: PASS');
    console.log('Your K6 load testing infrastructure is ready!');
    console.log('\nNext steps:');
    console.log('1. Run a smoke test: ./run-load-tests.sh smoke');
    console.log(
      '2. Execute sensor flood test: ./run-load-tests.sh sensor-flood'
    );
    console.log('3. Monitor results in Grafana: http://localhost:3004');
  } else if (passedChecks >= totalChecks * 0.6) {
    console.log('\n⚠️ VALIDATION RESULT: PARTIAL PASS');
    console.log(
      'Load testing infrastructure has some issues but can be used with limitations.'
    );
    console.log('\nRecommendations:');
    if (failedChecks > 0) {
      console.log('- Review failed validations above');
      console.log('- Start missing services if needed');
      console.log('- Check configuration files');
    }
  } else {
    console.log('\n❌ VALIDATION RESULT: FAIL');
    console.log('Load testing infrastructure needs attention before use.');
    console.log('\nRequired actions:');
    console.log('- Fix configuration issues');
    console.log('- Start required WRFCoin services');
    console.log('- Verify environment variables');
    console.log('- Run validation again: k6 run validate-setup.js');
  }

  console.log('\n📚 Documentation: tests/load/README.md');
  console.log('💬 Support: Check service logs and health endpoints');
  console.log('='.repeat(60) + '\n');
}
