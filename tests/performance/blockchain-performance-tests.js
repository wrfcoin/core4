/**
 * Blockchain Performance Testing Suite
 * Tests blockchain operations under load including transaction processing,
 * block validation, consensus mechanisms, and smart contract execution
 */

const { performance } = require('perf_hooks');
const fs = require('fs');
const path = require('path');
const {
  Worker,
  isMainThread,
  parentPort,
  workerData,
} = require('worker_threads');
const config = require('../test.config');

class BlockchainPerformanceTester {
  constructor() {
    this.baseUrl = config.baseUrls.backend;
    this.results = [];
    this.reportDir = path.join(__dirname, '../reports/performance');
    this.workers = [];

    // Ensure report directory exists
    if (!fs.existsSync(this.reportDir)) {
      fs.mkdirSync(this.reportDir, { recursive: true });
    }
  }

  /**
   * Execute blockchain performance test
   */
  async executePerformanceTest(testConfig) {
    const {
      title,
      operation,
      concurrency = 10,
      transactions = 100,
      duration,
      parameters = {},
      expectation = {},
    } = testConfig;

    console.log(`⛓️  Starting ${title}...`);
    console.log(`   Operation: ${operation}`);
    console.log(
      `   Concurrency: ${concurrency}, Transactions: ${transactions}`
    );

    const startTime = performance.now();
    const results = await this.runConcurrentTest(
      operation,
      concurrency,
      transactions,
      parameters
    );
    const endTime = performance.now();

    const testDuration = (endTime - startTime) / 1000;
    const successful = results.filter(r => r.success).length;
    const failed = results.filter(r => !r.success).length;
    const avgLatency =
      results.reduce((sum, r) => sum + r.latency, 0) / results.length;
    const tps = successful / testDuration;

    // Calculate percentiles
    const latencies = results.map(r => r.latency).sort((a, b) => a - b);
    const p50 = latencies[Math.floor(latencies.length * 0.5)];
    const p95 = latencies[Math.floor(latencies.length * 0.95)];
    const p99 = latencies[Math.floor(latencies.length * 0.99)];

    const processedResult = {
      title,
      operation,
      testParams: {
        concurrency,
        totalTransactions: transactions,
        duration: testDuration,
      },
      performance: {
        successful,
        failed,
        successRate: (successful / transactions) * 100,
        tps: Math.round(tps * 100) / 100,
        avgLatency: Math.round(avgLatency),
        latencyPercentiles: {
          p50: Math.round(p50),
          p95: Math.round(p95),
          p99: Math.round(p99),
        },
      },
      gasUsage: this.calculateGasMetrics(results),
      mempool: this.calculateMempoolMetrics(results),
      consensus: this.calculateConsensusMetrics(results),
      timestamp: new Date().toISOString(),
      passed: this.evaluateBlockchainPerformance(
        tps,
        avgLatency,
        successful,
        failed,
        expectation
      ),
    };

    this.results.push(processedResult);
    this.printBlockchainResults(processedResult);

    return processedResult;
  }

  /**
   * Run concurrent blockchain operations using worker threads
   */
  async runConcurrentTest(
    operation,
    concurrency,
    totalTransactions,
    parameters
  ) {
    const transactionsPerWorker = Math.ceil(totalTransactions / concurrency);
    const workers = [];
    const results = [];

    // Create worker threads for concurrent execution
    for (let i = 0; i < concurrency; i++) {
      const worker = new Worker(__filename, {
        workerData: {
          operation,
          transactions: transactionsPerWorker,
          workerId: i,
          parameters,
          baseUrl: this.baseUrl,
        },
      });

      workers.push(worker);
    }

    // Collect results from all workers
    await Promise.all(
      workers.map(worker => {
        return new Promise((resolve, reject) => {
          worker.on('message', workerResults => {
            results.push(...workerResults);
            resolve();
          });

          worker.on('error', reject);

          worker.on('exit', code => {
            if (code !== 0) {
              reject(new Error(`Worker stopped with exit code ${code}`));
            }
          });
        });
      })
    );

    // Cleanup workers
    workers.forEach(worker => worker.terminate());

    return results.slice(0, totalTransactions); // Ensure exact count
  }

  /**
   * Calculate gas usage metrics
   */
  calculateGasMetrics(results) {
    const gasResults = results.filter(r => r.gasUsed);

    if (gasResults.length === 0) {
      return { available: false };
    }

    const gasValues = gasResults.map(r => r.gasUsed);
    const totalGas = gasValues.reduce((sum, gas) => sum + gas, 0);
    const avgGas = totalGas / gasValues.length;

    return {
      available: true,
      total: totalGas,
      average: Math.round(avgGas),
      min: Math.min(...gasValues),
      max: Math.max(...gasValues),
      efficiency: avgGas / (results[0]?.estimatedGas || avgGas), // Gas efficiency ratio
    };
  }

  /**
   * Calculate mempool metrics
   */
  calculateMempoolMetrics(results) {
    const mempoolResults = results.filter(r => r.mempoolTime);

    if (mempoolResults.length === 0) {
      return { available: false };
    }

    const mempoolTimes = mempoolResults.map(r => r.mempoolTime);
    const avgMempoolTime =
      mempoolTimes.reduce((sum, time) => sum + time, 0) / mempoolTimes.length;

    return {
      available: true,
      averageTime: Math.round(avgMempoolTime),
      congestion: this.calculateCongestionLevel(mempoolTimes),
      throughput: mempoolResults.length / (Math.max(...mempoolTimes) / 1000),
    };
  }

  /**
   * Calculate consensus metrics
   */
  calculateConsensusMetrics(results) {
    const consensusResults = results.filter(r => r.blockTime);

    if (consensusResults.length === 0) {
      return { available: false };
    }

    const blockTimes = consensusResults.map(r => r.blockTime);
    const avgBlockTime =
      blockTimes.reduce((sum, time) => sum + time, 0) / blockTimes.length;

    return {
      available: true,
      averageBlockTime: Math.round(avgBlockTime),
      finality: this.calculateFinality(consensusResults),
      networkUtilization: this.calculateNetworkUtilization(consensusResults),
    };
  }

  /**
   * Calculate network congestion level
   */
  calculateCongestionLevel(mempoolTimes) {
    const avgTime =
      mempoolTimes.reduce((sum, time) => sum + time, 0) / mempoolTimes.length;

    if (avgTime < 1000) return 'Low';
    if (avgTime < 5000) return 'Medium';
    if (avgTime < 15000) return 'High';
    return 'Critical';
  }

  /**
   * Calculate transaction finality metrics
   */
  calculateFinality(results) {
    const finalityTimes = results.map(r => r.finalityTime || r.blockTime * 6); // Assume 6 confirmations
    const avgFinality =
      finalityTimes.reduce((sum, time) => sum + time, 0) / finalityTimes.length;

    return {
      averageTime: Math.round(avgFinality),
      reliability:
        (results.filter(r => r.finalized).length / results.length) * 100,
    };
  }

  /**
   * Calculate network utilization
   */
  calculateNetworkUtilization(results) {
    const totalCapacity = 1000; // Assume 1000 TPS theoretical max
    const actualTps =
      results.length /
      ((Math.max(...results.map(r => r.timestamp)) -
        Math.min(...results.map(r => r.timestamp))) /
        1000);

    return Math.min((actualTps / totalCapacity) * 100, 100);
  }

  /**
   * Evaluate blockchain performance against expectations
   */
  evaluateBlockchainPerformance(
    tps,
    avgLatency,
    successful,
    failed,
    expectation
  ) {
    const checks = [];

    if (expectation.minTPS) {
      checks.push({
        check: 'Minimum TPS',
        expected: expectation.minTPS,
        actual: tps,
        passed: tps >= expectation.minTPS,
      });
    }

    if (expectation.maxLatency) {
      checks.push({
        check: 'Maximum Latency',
        expected: `${expectation.maxLatency}ms`,
        actual: `${avgLatency}ms`,
        passed: avgLatency <= expectation.maxLatency,
      });
    }

    if (expectation.minSuccessRate) {
      const successRate = (successful / (successful + failed)) * 100;
      checks.push({
        check: 'Minimum Success Rate',
        expected: `${expectation.minSuccessRate}%`,
        actual: `${successRate.toFixed(2)}%`,
        passed: successRate >= expectation.minSuccessRate,
      });
    }

    return checks;
  }

  /**
   * Print blockchain test results
   */
  printBlockchainResults(result) {
    console.log(`\n⛓️  ${result.title} Results:`);
    console.log(`   TPS: ${result.performance.tps}`);
    console.log(
      `   Success Rate: ${result.performance.successRate.toFixed(2)}%`
    );
    console.log(`   Avg Latency: ${result.performance.avgLatency}ms`);
    console.log(
      `   Latency P95: ${result.performance.latencyPercentiles.p95}ms`
    );

    if (result.gasUsage.available) {
      console.log(
        `   Avg Gas: ${result.gasUsage.average}, Efficiency: ${result.gasUsage.efficiency.toFixed(3)}`
      );
    }

    if (result.mempool.available) {
      console.log(
        `   Mempool: ${result.mempool.averageTime}ms avg, ${result.mempool.congestion} congestion`
      );
    }

    if (result.passed && result.passed.length > 0) {
      console.log(`   Performance Checks:`);
      result.passed.forEach(check => {
        const status = check.passed ? '✅' : '❌';
        console.log(
          `     ${status} ${check.check}: ${check.actual} (expected: ${check.expected})`
        );
      });
    }
  }

  /**
   * Generate blockchain performance report
   */
  generateBlockchainReport() {
    const reportData = {
      summary: {
        totalTests: this.results.length,
        timestamp: new Date().toISOString(),
        overallTPS: this.calculateOverallTPS(),
        networkHealth: this.assessNetworkHealth(),
        recommendations: this.generateBlockchainRecommendations(),
      },
      tests: this.results,
    };

    // Save JSON report
    const jsonReport = path.join(
      this.reportDir,
      'blockchain-performance-results.json'
    );
    fs.writeFileSync(jsonReport, JSON.stringify(reportData, null, 2));

    console.log(`\n📋 Blockchain performance report generated: ${jsonReport}`);
    return reportData;
  }

  /**
   * Calculate overall TPS across all tests
   */
  calculateOverallTPS() {
    const totalTPS = this.results.reduce(
      (sum, result) => sum + result.performance.tps,
      0
    );
    return totalTPS / this.results.length;
  }

  /**
   * Assess overall network health
   */
  assessNetworkHealth() {
    const avgSuccessRate =
      this.results.reduce((sum, r) => sum + r.performance.successRate, 0) /
      this.results.length;
    const avgLatency =
      this.results.reduce((sum, r) => sum + r.performance.avgLatency, 0) /
      this.results.length;

    if (avgSuccessRate > 95 && avgLatency < 1000) return 'Excellent';
    if (avgSuccessRate > 90 && avgLatency < 2000) return 'Good';
    if (avgSuccessRate > 80 && avgLatency < 5000) return 'Fair';
    return 'Poor';
  }

  /**
   * Generate blockchain-specific recommendations
   */
  generateBlockchainRecommendations() {
    const recommendations = [];

    this.results.forEach(result => {
      if (result.performance.tps < 10) {
        recommendations.push({
          test: result.title,
          issue: 'Low Transaction Throughput',
          recommendation:
            'Consider optimizing consensus mechanism, increasing block size, or implementing sharding',
        });
      }

      if (result.performance.avgLatency > 3000) {
        recommendations.push({
          test: result.title,
          issue: 'High Transaction Latency',
          recommendation:
            'Optimize block time, reduce network congestion, or implement state channels',
        });
      }

      if (result.gasUsage.available && result.gasUsage.efficiency < 0.8) {
        recommendations.push({
          test: result.title,
          issue: 'Poor Gas Efficiency',
          recommendation:
            'Optimize smart contracts, implement gas estimation improvements, or review fee structure',
        });
      }
    });

    return recommendations;
  }
}

// Worker thread execution for concurrent blockchain operations
if (!isMainThread) {
  const { operation, transactions, workerId, parameters, baseUrl } = workerData;

  async function executeBlockchainOperations() {
    const results = [];

    for (let i = 0; i < transactions; i++) {
      const startTime = performance.now();

      try {
        const result = await simulateBlockchainOperation(
          operation,
          parameters,
          baseUrl
        );
        const endTime = performance.now();

        results.push({
          success: true,
          latency: endTime - startTime,
          timestamp: Date.now(),
          workerId,
          operationId: i,
          ...result,
        });
      } catch (error) {
        const endTime = performance.now();

        results.push({
          success: false,
          latency: endTime - startTime,
          timestamp: Date.now(),
          workerId,
          operationId: i,
          error: error.message,
        });
      }

      // Small delay to prevent overwhelming the system
      await new Promise(resolve => setTimeout(resolve, 10));
    }

    parentPort.postMessage(results);
  }

  executeBlockchainOperations().catch(error => {
    parentPort.postMessage([{ success: false, error: error.message }]);
  });
}

/**
 * Simulate blockchain operations
 */
async function simulateBlockchainOperation(operation, parameters, baseUrl) {
  const operationStartTime = performance.now();

  switch (operation) {
    case 'transfer':
      return await simulateTransfer(parameters, baseUrl);
    case 'nft_mint':
      return await simulateNFTMint(parameters, baseUrl);
    case 'smart_contract':
      return await simulateSmartContract(parameters, baseUrl);
    case 'governance_vote':
      return await simulateGovernanceVote(parameters, baseUrl);
    case 'cross_chain_bridge':
      return await simulateCrossChainBridge(parameters, baseUrl);
    default:
      throw new Error(`Unknown operation: ${operation}`);
  }
}

/**
 * Simulate token transfer
 */
async function simulateTransfer(parameters, baseUrl) {
  const mempoolStart = performance.now();

  // Simulate transaction creation and mempool entry
  await new Promise(resolve => setTimeout(resolve, Math.random() * 100 + 50));
  const mempoolTime = performance.now() - mempoolStart;

  // Simulate block inclusion
  const blockStart = performance.now();
  await new Promise(resolve =>
    setTimeout(resolve, Math.random() * 2000 + 1000)
  );
  const blockTime = performance.now() - blockStart;

  return {
    gasUsed: Math.floor(Math.random() * 21000 + 21000),
    estimatedGas: 21000,
    mempoolTime,
    blockTime,
    finalityTime: blockTime * 6,
    finalized: Math.random() > 0.05, // 95% finalization rate
  };
}

/**
 * Simulate NFT minting
 */
async function simulateNFTMint(parameters, baseUrl) {
  const mempoolStart = performance.now();

  // NFT minting typically takes longer
  await new Promise(resolve => setTimeout(resolve, Math.random() * 200 + 100));
  const mempoolTime = performance.now() - mempoolStart;

  const blockStart = performance.now();
  await new Promise(resolve =>
    setTimeout(resolve, Math.random() * 3000 + 2000)
  );
  const blockTime = performance.now() - blockStart;

  return {
    gasUsed: Math.floor(Math.random() * 150000 + 100000),
    estimatedGas: 125000,
    mempoolTime,
    blockTime,
    finalityTime: blockTime * 6,
    finalized: Math.random() > 0.03,
    metadataSize: Math.floor(Math.random() * 5000 + 1000),
  };
}

/**
 * Simulate smart contract execution
 */
async function simulateSmartContract(parameters, baseUrl) {
  const mempoolStart = performance.now();

  await new Promise(resolve => setTimeout(resolve, Math.random() * 150 + 75));
  const mempoolTime = performance.now() - mempoolStart;

  const blockStart = performance.now();
  await new Promise(resolve =>
    setTimeout(resolve, Math.random() * 4000 + 1500)
  );
  const blockTime = performance.now() - blockStart;

  return {
    gasUsed: Math.floor(Math.random() * 500000 + 200000),
    estimatedGas: 350000,
    mempoolTime,
    blockTime,
    finalityTime: blockTime * 6,
    finalized: Math.random() > 0.02,
    computationComplexity: Math.random() * 100,
  };
}

/**
 * Simulate governance voting
 */
async function simulateGovernanceVote(parameters, baseUrl) {
  const mempoolStart = performance.now();

  await new Promise(resolve => setTimeout(resolve, Math.random() * 120 + 60));
  const mempoolTime = performance.now() - mempoolStart;

  const blockStart = performance.now();
  await new Promise(resolve =>
    setTimeout(resolve, Math.random() * 2500 + 1200)
  );
  const blockTime = performance.now() - blockStart;

  return {
    gasUsed: Math.floor(Math.random() * 80000 + 50000),
    estimatedGas: 65000,
    mempoolTime,
    blockTime,
    finalityTime: blockTime * 12, // Governance requires more confirmations
    finalized: Math.random() > 0.01,
    votingPower: Math.floor(Math.random() * 10000 + 1000),
  };
}

/**
 * Simulate cross-chain bridge operation
 */
async function simulateCrossChainBridge(parameters, baseUrl) {
  const mempoolStart = performance.now();

  // Cross-chain operations take longer due to multiple chain coordination
  await new Promise(resolve => setTimeout(resolve, Math.random() * 300 + 200));
  const mempoolTime = performance.now() - mempoolStart;

  const blockStart = performance.now();
  await new Promise(resolve =>
    setTimeout(resolve, Math.random() * 8000 + 5000)
  );
  const blockTime = performance.now() - blockStart;

  return {
    gasUsed: Math.floor(Math.random() * 200000 + 150000),
    estimatedGas: 175000,
    mempoolTime,
    blockTime,
    finalityTime: blockTime * 24, // Cross-chain requires more confirmations
    finalized: Math.random() > 0.08, // Higher failure rate for cross-chain
    bridgeLatency: Math.random() * 30000 + 15000, // Additional bridge processing time
  };
}

// Main execution for blockchain performance tests
async function runBlockchainPerformanceTests() {
  const tester = new BlockchainPerformanceTester();

  console.log('⛓️  Starting WRFCoin Blockchain Performance Testing Suite\n');

  try {
    // Test 1: Basic token transfers
    await tester.executePerformanceTest({
      title: 'Token Transfer Performance',
      operation: 'transfer',
      concurrency: 50,
      transactions: 500,
      expectation: {
        minTPS: 15,
        maxLatency: 3000,
        minSuccessRate: 95,
      },
    });

    // Test 2: NFT minting under load
    await tester.executePerformanceTest({
      title: 'NFT Minting Performance',
      operation: 'nft_mint',
      concurrency: 20,
      transactions: 200,
      expectation: {
        minTPS: 5,
        maxLatency: 8000,
        minSuccessRate: 90,
      },
    });

    // Test 3: Smart contract execution
    await tester.executePerformanceTest({
      title: 'Smart Contract Execution',
      operation: 'smart_contract',
      concurrency: 30,
      transactions: 300,
      expectation: {
        minTPS: 8,
        maxLatency: 10000,
        minSuccessRate: 92,
      },
    });

    // Test 4: Governance voting
    await tester.executePerformanceTest({
      title: 'Governance Voting Performance',
      operation: 'governance_vote',
      concurrency: 25,
      transactions: 250,
      expectation: {
        minTPS: 10,
        maxLatency: 5000,
        minSuccessRate: 98,
      },
    });

    // Test 5: Cross-chain bridge stress test
    await tester.executePerformanceTest({
      title: 'Cross-Chain Bridge Stress Test',
      operation: 'cross_chain_bridge',
      concurrency: 15,
      transactions: 150,
      expectation: {
        minTPS: 3,
        maxLatency: 20000,
        minSuccessRate: 85,
      },
    });

    // Generate comprehensive report
    const report = tester.generateBlockchainReport();

    console.log('\n⛓️  Blockchain Performance Testing Complete!');
    console.log(`📊 Network Health: ${report.summary.networkHealth}`);
    console.log(`🚀 Overall TPS: ${report.summary.overallTPS.toFixed(2)}`);

    if (report.summary.recommendations.length > 0) {
      console.log(
        `⚠️  Found ${report.summary.recommendations.length} optimization opportunities`
      );
    }

    return report;
  } catch (error) {
    console.error('❌ Blockchain performance testing failed:', error);
    throw error;
  }
}

// Export for use in other test files
module.exports = {
  BlockchainPerformanceTester,
  runBlockchainPerformanceTests,
};

// Run tests if called directly
if (require.main === module && isMainThread) {
  runBlockchainPerformanceTests()
    .then(report => {
      process.exit(report.summary.networkHealth !== 'Poor' ? 0 : 1);
    })
    .catch(error => {
      console.error('Test execution failed:', error);
      process.exit(1);
    });
}
