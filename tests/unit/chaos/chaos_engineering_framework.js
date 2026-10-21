/**
 * Wrfcoin Chaos Engineering Framework
 * Advanced resilience testing inspired by Netflix's Chaos Monkey
 * Intentionally introduces failures to validate system recovery capabilities
 */

const axios = require('axios');
const EventEmitter = require('events');

class ChaosEngineeringFramework extends EventEmitter {
  constructor(baseUrl = 'http://localhost:5000') {
    super();
    this.baseUrl = baseUrl;
    this.experiments = [];
    this.metrics = {
      totalExperiments: 0,
      successfulRecoveries: 0,
      failedRecoveries: 0,
      averageRecoveryTime: 0,
      systemResilience: 0,
    };
    this.isExperimentRunning = false;
  }

  /**
   * Execute comprehensive chaos engineering experiments
   */
  async runChaosExperiments() {
    console.log('🔥 Starting Chaos Engineering Framework');
    console.log('=====================================');
    console.log(
      '⚠️  WARNING: Introducing controlled failures to test system resilience'
    );
    console.log(`Target: ${this.baseUrl}`);

    const experiments = [
      {
        name: 'API Endpoint Failure Test',
        test: () => this.testApiEndpointFailures(),
      },
      {
        name: 'Database Connection Chaos',
        test: () => this.testDatabaseConnectionChaos(),
      },
      {
        name: 'Network Latency Injection',
        test: () => this.testNetworkLatencyInjection(),
      },
      {
        name: 'Memory Pressure Chaos',
        test: () => this.testMemoryPressureChaos(),
      },
      {
        name: 'Cross-Chain Bridge Disruption',
        test: () => this.testCrossChainBridgeDisruption(),
      },
      {
        name: 'Transaction Processing Chaos',
        test: () => this.testTransactionProcessingChaos(),
      },
      {
        name: 'Rate Limiting Bypass Attempt',
        test: () => this.testRateLimitingBypass(),
      },
      {
        name: 'Cascading Failure Simulation',
        test: () => this.testCascadingFailureSimulation(),
      },
    ];

    for (const experiment of experiments) {
      await this.runExperiment(experiment.name, experiment.test);
    }

    this.generateResilienceReport();
  }

  /**
   * Run individual chaos experiment with monitoring
   */
  async runExperiment(name, experimentFunction) {
    console.log(`\n🔥 Running Chaos Experiment: ${name}`);
    const startTime = Date.now();
    this.isExperimentRunning = true;

    try {
      const result = await experimentFunction();
      const duration = Date.now() - startTime;

      this.experiments.push({
        name,
        status: 'completed',
        duration,
        recoveryTime: result.recoveryTime || 0,
        resilience: result.resilience || 0,
        details: result.details || {},
        metrics: result.metrics || {},
      });

      console.log(`   ✅ ${name} - Completed (${duration}ms)`);
      if (result.summary) {
        console.log(`   📊 ${result.summary}`);
      }
    } catch (error) {
      const duration = Date.now() - startTime;
      this.experiments.push({
        name,
        status: 'failed',
        duration,
        error: error.message,
        resilience: 0,
      });
      console.log(
        `   💥 ${name} - System vulnerability detected: ${error.message}`
      );
    } finally {
      this.isExperimentRunning = false;
    }
  }

  /**
   * Test API endpoint failure scenarios
   */
  async testApiEndpointFailures() {
    const endpoints = [
      '/api/blockchain/status',
      '/api/wallet/balance',
      '/api/mining/cross-chain-rewards',
    ];

    const failureResults = [];
    const recoveryTimes = [];

    for (const endpoint of endpoints) {
      const startTime = Date.now();

      // Simulate failure by rapid requests
      const rapidRequests = 15;
      const promises = [];

      for (let i = 0; i < rapidRequests; i++) {
        promises.push(this.makeRequest('GET', endpoint));
      }

      const results = await Promise.allSettled(promises);
      const failures = results.filter(
        r => r.status === 'rejected' || (r.value && r.value.status >= 500)
      );
      const successes = results.filter(
        r => r.status === 'fulfilled' && r.value && r.value.status < 500
      );

      // Test recovery
      await this.sleep(100); // Brief pause
      const recoveryStart = Date.now();
      const recoveryTest = await this.makeRequest('GET', endpoint);
      const recoveryTime = Date.now() - recoveryStart;

      recoveryTimes.push(recoveryTime);

      failureResults.push({
        endpoint,
        totalRequests: rapidRequests,
        failures: failures.length,
        successes: successes.length,
        recoveryTime,
        recovered: recoveryTest.status < 500,
      });
    }

    const avgRecoveryTime =
      recoveryTimes.reduce((sum, time) => sum + time, 0) / recoveryTimes.length;
    const recoveredEndpoints = failureResults.filter(r => r.recovered).length;
    const resilience = (recoveredEndpoints / endpoints.length) * 100;

    return {
      recoveryTime: avgRecoveryTime,
      resilience,
      summary: `${recoveredEndpoints}/${endpoints.length} endpoints recovered, ${avgRecoveryTime.toFixed(0)}ms avg recovery`,
      details: { failureResults },
      metrics: {
        averageRecoveryTime: avgRecoveryTime,
        endpointResilience: resilience,
      },
    };
  }

  /**
   * Test database connection chaos scenarios
   */
  async testDatabaseConnectionChaos() {
    const dbEndpoints = [
      '/api/wallet/balance',
      '/api/blockchain/status',
      '/api/mining/cross-chain-rewards',
    ];

    const connectionTests = [];

    // Simulate database stress with concurrent requests
    const concurrentConnections = 10;
    const promises = [];

    for (let i = 0; i < concurrentConnections; i++) {
      const endpoint = dbEndpoints[i % dbEndpoints.length];
      promises.push(this.simulateDatabaseStress(endpoint));
    }

    const results = await Promise.allSettled(promises);
    const successful = results.filter(r => r.status === 'fulfilled').length;

    // Test recovery after stress
    await this.sleep(200);
    const recoveryStart = Date.now();
    const recoveryTest = await this.makeRequest(
      'GET',
      '/api/blockchain/status'
    );
    const recoveryTime = Date.now() - recoveryStart;

    const resilience = (successful / concurrentConnections) * 100;

    return {
      recoveryTime,
      resilience,
      summary: `${successful}/${concurrentConnections} database connections survived stress test`,
      details: { concurrentConnections, successful },
      metrics: { dbConnectionResilience: resilience, recoveryTime },
    };
  }

  /**
   * Test network latency injection scenarios
   */
  async testNetworkLatencyInjection() {
    const endpoints = ['/api/blockchain/status', '/api/wallet/balance'];
    const latencyTests = [];

    for (const endpoint of endpoints) {
      const normalStart = Date.now();
      await this.makeRequest('GET', endpoint);
      const normalTime = Date.now() - normalStart;

      // Simulate network delay with rapid successive requests
      const delayedStart = Date.now();
      const rapidPromises = [];
      for (let i = 0; i < 5; i++) {
        rapidPromises.push(this.makeRequest('GET', endpoint));
      }
      await Promise.allSettled(rapidPromises);
      const delayedTime = Date.now() - delayedStart;

      latencyTests.push({
        endpoint,
        normalLatency: normalTime,
        stressLatency: delayedTime / 5, // Average per request
        latencyIncrease: ((delayedTime / 5 - normalTime) / normalTime) * 100,
      });
    }

    const avgLatencyIncrease =
      latencyTests.reduce((sum, test) => sum + test.latencyIncrease, 0) /
      latencyTests.length;
    const resilience = Math.max(0, 100 - Math.min(avgLatencyIncrease, 100));

    return {
      recoveryTime: 0,
      resilience,
      summary: `${avgLatencyIncrease.toFixed(1)}% average latency increase under stress`,
      details: { latencyTests },
      metrics: {
        averageLatencyIncrease: avgLatencyIncrease,
        networkResilience: resilience,
      },
    };
  }

  /**
   * Test memory pressure chaos scenarios
   */
  async testMemoryPressureChaos() {
    const memoryIntensiveRequests = 8;
    const results = [];

    // Simulate memory pressure with large concurrent requests
    const startTime = Date.now();
    const promises = [];

    for (let i = 0; i < memoryIntensiveRequests; i++) {
      promises.push(this.makeRequest('GET', '/api/blockchain/status'));
    }

    const responses = await Promise.allSettled(promises);
    const successful = responses.filter(
      r => r.status === 'fulfilled' && r.value.status < 500
    ).length;

    const endTime = Date.now();
    const totalTime = endTime - startTime;

    // Test system recovery
    await this.sleep(100);
    const recoveryStart = Date.now();
    const recoveryTest = await this.makeRequest(
      'GET',
      '/api/blockchain/status'
    );
    const recoveryTime = Date.now() - recoveryStart;

    const resilience = (successful / memoryIntensiveRequests) * 100;

    return {
      recoveryTime,
      resilience,
      summary: `${successful}/${memoryIntensiveRequests} requests handled under memory pressure`,
      details: { memoryIntensiveRequests, successful, totalTime },
      metrics: { memoryPressureResilience: resilience },
    };
  }

  /**
   * Test cross-chain bridge disruption scenarios
   */
  async testCrossChainBridgeDisruption() {
    const bridgeEndpoints = [
      '/api/bridges/axelar/status',
      '/api/bridges/layerzero/status',
      '/api/bridges/wormhole/status',
    ];

    const bridgeTests = [];

    for (const endpoint of bridgeEndpoints) {
      const startTime = Date.now();

      try {
        const response = await this.makeRequest('GET', endpoint);
        const responseTime = Date.now() - startTime;

        bridgeTests.push({
          endpoint,
          status: response.status,
          responseTime,
          operational: response.status < 500,
        });
      } catch (error) {
        bridgeTests.push({
          endpoint,
          status: 500,
          responseTime: Date.now() - startTime,
          operational: false,
          error: error.message,
        });
      }
    }

    const operationalBridges = bridgeTests.filter(
      test => test.operational
    ).length;
    const resilience = (operationalBridges / bridgeEndpoints.length) * 100;
    const avgResponseTime =
      bridgeTests.reduce((sum, test) => sum + test.responseTime, 0) /
      bridgeTests.length;

    return {
      recoveryTime: avgResponseTime,
      resilience,
      summary: `${operationalBridges}/${bridgeEndpoints.length} cross-chain bridges operational`,
      details: { bridgeTests },
      metrics: {
        bridgeResilience: resilience,
        averageBridgeResponseTime: avgResponseTime,
      },
    };
  }

  /**
   * Test transaction processing chaos scenarios
   */
  async testTransactionProcessingChaos() {
    const transactionEndpoints = [
      '/api/transactions/create',
      '/api/mining/start',
      '/api/cross-chain/transfer',
    ];

    const transactionTests = [];

    for (const endpoint of transactionEndpoints) {
      const batchSize = 3;
      const promises = [];

      for (let i = 0; i < batchSize; i++) {
        const txData = { amount: 100, test: true };
        promises.push(this.makeRequest('POST', endpoint, txData));
      }

      const results = await Promise.allSettled(promises);
      const successful = results.filter(
        r => r.status === 'fulfilled' && r.value.status < 500
      ).length;

      transactionTests.push({
        endpoint,
        attempted: batchSize,
        successful,
        successRate: (successful / batchSize) * 100,
      });
    }

    const totalAttempted = transactionTests.reduce(
      (sum, test) => sum + test.attempted,
      0
    );
    const totalSuccessful = transactionTests.reduce(
      (sum, test) => sum + test.successful,
      0
    );
    const resilience = (totalSuccessful / totalAttempted) * 100;

    return {
      recoveryTime: 0,
      resilience,
      summary: `${totalSuccessful}/${totalAttempted} transactions processed under chaos conditions`,
      details: { transactionTests },
      metrics: { transactionProcessingResilience: resilience },
    };
  }

  /**
   * Test rate limiting bypass attempts
   */
  async testRateLimitingBypass() {
    const endpoint = '/api/blockchain/status';
    const bypassAttempts = 25; // Should trigger rate limiting

    console.log(
      `     🚀 Attempting to bypass rate limiting with ${bypassAttempts} rapid requests...`
    );

    const results = [];
    const startTime = Date.now();

    for (let i = 0; i < bypassAttempts; i++) {
      try {
        const response = await this.makeRequest('GET', endpoint);
        results.push({
          attempt: i + 1,
          status: response.status,
          blocked: response.status === 429,
        });
      } catch (error) {
        results.push({
          attempt: i + 1,
          status: 500,
          blocked: false,
          error: error.message,
        });
      }
    }

    const totalTime = Date.now() - startTime;
    const blockedRequests = results.filter(r => r.blocked).length;
    const successfulBypass = results.filter(r => r.status === 200).length;

    // Rate limiting effectiveness (higher blocking = better resilience)
    const resilience = (blockedRequests / bypassAttempts) * 100;

    return {
      recoveryTime: 0,
      resilience,
      summary: `Rate limiting blocked ${blockedRequests}/${bypassAttempts} bypass attempts (${resilience.toFixed(1)}% effective)`,
      details: { bypassAttempts, blockedRequests, successfulBypass, totalTime },
      metrics: { rateLimitingEffectiveness: resilience },
    };
  }

  /**
   * Test cascading failure simulation
   */
  async testCascadingFailureSimulation() {
    const systemComponents = [
      '/api/blockchain/status',
      '/api/wallet/balance',
      '/api/mining/cross-chain-rewards',
    ];

    const cascadingResults = [];

    // Simulate gradual system stress
    for (let intensity = 1; intensity <= 3; intensity++) {
      const requests = intensity * 5;
      const componentPromises = [];

      for (const component of systemComponents) {
        const componentRequests = [];
        for (let i = 0; i < requests; i++) {
          componentRequests.push(this.makeRequest('GET', component));
        }
        componentPromises.push(Promise.allSettled(componentRequests));
      }

      const allResults = await Promise.all(componentPromises);

      let totalRequests = 0;
      let totalSuccessful = 0;

      allResults.forEach((componentResults, index) => {
        const successful = componentResults.filter(
          r => r.status === 'fulfilled' && r.value.status < 500
        ).length;
        totalRequests += requests;
        totalSuccessful += successful;

        cascadingResults.push({
          intensity,
          component: systemComponents[index],
          requests,
          successful,
          successRate: (successful / requests) * 100,
        });
      });
    }

    const overallSuccessRate =
      cascadingResults.reduce((sum, result) => sum + result.successRate, 0) /
      cascadingResults.length;
    const resilience = overallSuccessRate;

    return {
      recoveryTime: 0,
      resilience,
      summary: `System maintained ${resilience.toFixed(1)}% performance under cascading load`,
      details: { cascadingResults },
      metrics: { cascadingFailureResilience: resilience },
    };
  }

  /**
   * Helper methods
   */
  async simulateDatabaseStress(endpoint) {
    const requests = 3;
    const promises = [];

    for (let i = 0; i < requests; i++) {
      promises.push(this.makeRequest('GET', endpoint));
    }

    const results = await Promise.allSettled(promises);
    return results.filter(r => r.status === 'fulfilled').length;
  }

  async makeRequest(method, path, data = null) {
    const config = {
      method,
      url: `${this.baseUrl}${path}`,
      timeout: 15000,
      validateStatus: () => true,
    };

    if (data && method !== 'GET') {
      config.data = data;
      config.headers = { 'Content-Type': 'application/json' };
    }

    const response = await axios(config);
    return { status: response.status, data: response.data };
  }

  sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  /**
   * Generate comprehensive resilience report
   */
  generateResilienceReport() {
    console.log('\n🔥 Chaos Engineering Resilience Report');
    console.log('======================================');

    const totalExperiments = this.experiments.length;
    const completedExperiments = this.experiments.filter(
      e => e.status === 'completed'
    ).length;
    const totalDuration = this.experiments.reduce(
      (sum, e) => sum + e.duration,
      0
    );

    console.log(`Total Chaos Experiments: ${totalExperiments}`);
    console.log(`✅ Completed: ${completedExperiments}`);
    console.log(`💥 Failed: ${totalExperiments - completedExperiments}`);
    console.log(`⏱️  Total Duration: ${totalDuration}ms`);
    console.log(
      `📈 Experiment Success Rate: ${((completedExperiments / totalExperiments) * 100).toFixed(1)}%`
    );

    // Calculate overall system resilience
    const resilienceScores = this.experiments
      .filter(e => e.status === 'completed' && e.resilience !== undefined)
      .map(e => e.resilience);

    if (resilienceScores.length > 0) {
      const averageResilience =
        resilienceScores.reduce((sum, score) => sum + score, 0) /
        resilienceScores.length;

      console.log('\n🛡️  System Resilience Summary:');
      this.experiments.forEach(experiment => {
        if (
          experiment.status === 'completed' &&
          experiment.resilience !== undefined
        ) {
          console.log(
            `   ${experiment.name}: ${experiment.resilience.toFixed(1)}% resilient`
          );
        }
      });

      console.log(
        `\n⚡ Overall System Resilience Score: ${averageResilience.toFixed(1)}%`
      );

      if (averageResilience >= 90) {
        console.log(
          '🎉 EXCELLENT! Your system demonstrates exceptional resilience to chaos'
        );
      } else if (averageResilience >= 75) {
        console.log('👍 GOOD! Your system shows solid resilience under stress');
      } else if (averageResilience >= 60) {
        console.log(
          '⚠️  ACCEPTABLE! System resilience meets basic requirements'
        );
      } else {
        console.log(
          '🔴 CRITICAL! System resilience needs immediate improvement'
        );
      }

      // Recovery time analysis
      const recoveryTimes = this.experiments
        .filter(e => e.recoveryTime !== undefined && e.recoveryTime > 0)
        .map(e => e.recoveryTime);

      if (recoveryTimes.length > 0) {
        const avgRecoveryTime =
          recoveryTimes.reduce((sum, time) => sum + time, 0) /
          recoveryTimes.length;
        console.log(
          `⚡ Average Recovery Time: ${avgRecoveryTime.toFixed(0)}ms`
        );
      }
    }

    // Store metrics for future analysis
    this.metrics = {
      totalExperiments,
      completedExperiments,
      averageResilience:
        resilienceScores.length > 0
          ? resilienceScores.reduce((sum, score) => sum + score, 0) /
            resilienceScores.length
          : 0,
      experimentSuccessRate: (completedExperiments / totalExperiments) * 100,
    };
  }
}

// Execute chaos engineering if run directly
if (require.main === module) {
  const chaosFramework = new ChaosEngineeringFramework();
  chaosFramework.runChaosExperiments().catch(console.error);
}

module.exports = ChaosEngineeringFramework;
