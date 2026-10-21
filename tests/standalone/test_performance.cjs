#!/usr/bin/env node

/**
 * Performance Test Suite for Smart Contract Graph Visualization
 * Tests memory usage, rendering performance, and scalability with 96 nodes and 153 edges
 */

const axios = require('axios');
const { performance } = require('perf_hooks');

class PerformanceTester {
  constructor() {
    this.results = {
      passed: 0,
      failed: 0,
      tests: [],
    };
    this.apiBaseUrl = 'http://localhost:3001';
  }

  async test(name, testFn) {
    console.log(`⚡ Performance Testing: ${name}`);
    try {
      const result = await testFn();
      this.results.passed++;
      this.results.tests.push({ name, status: 'PASSED', result });
      console.log(`✅ PASSED: ${name}`);
      return result;
    } catch (error) {
      this.results.failed++;
      this.results.tests.push({ name, status: 'FAILED', error: error.message });
      console.log(`❌ FAILED: ${name} - ${error.message}`);
      return null;
    }
  }

  async measureExecutionTime(fn) {
    const start = performance.now();
    const result = await fn();
    const end = performance.now();
    return {
      result,
      duration: end - start,
      durationMs: Math.round((end - start) * 100) / 100,
    };
  }

  async run() {
    console.log(
      '⚡ Starting Performance Test Suite for Smart Contract Graph\n'
    );

    // Test 1: API Response Time with Large Dataset
    await this.test('API Response Time (96 nodes, 153 edges)', async () => {
      const measurement = await this.measureExecutionTime(async () => {
        const response = await axios.get(
          `${this.apiBaseUrl}/api/smart-contracts/graph`
        );
        return response.data;
      });

      if (measurement.duration > 2000) {
        // 2 seconds
        throw new Error(
          `API response too slow: ${measurement.durationMs}ms (expected <2000ms)`
        );
      }

      const data = measurement.result;
      if (!data.success) {
        throw new Error('API returned unsuccessful response');
      }

      return {
        responseTime: measurement.durationMs,
        nodeCount: data.data.nodes.length,
        linkCount: data.data.links.length,
        dataSize: JSON.stringify(data).length,
        cached: data.cached || false,
      };
    });

    // Test 2: JSON Parsing Performance
    await this.test('JSON Data Parsing Performance', async () => {
      const response = await axios.get(
        `${this.apiBaseUrl}/api/smart-contracts/graph`
      );
      const jsonString = JSON.stringify(response.data);

      const measurement = await this.measureExecutionTime(async () => {
        // Simulate multiple parsing operations (as might happen in React components)
        const iterations = 100;
        for (let i = 0; i < iterations; i++) {
          JSON.parse(jsonString);
        }
        return iterations;
      });

      if (measurement.duration > 1000) {
        // 1 second for 100 iterations
        throw new Error(
          `JSON parsing too slow: ${measurement.durationMs}ms for 100 iterations`
        );
      }

      return {
        iterations: measurement.result,
        totalTime: measurement.durationMs,
        avgPerIteration: measurement.durationMs / measurement.result,
        jsonSize: jsonString.length,
      };
    });

    // Test 3: Memory Usage Simulation
    await this.test('Memory Usage with Large Graph Data', async () => {
      const response = await axios.get(
        `${this.apiBaseUrl}/api/smart-contracts/graph`
      );
      const graphData = response.data.data;

      // Simulate memory usage patterns similar to D3.js operations
      const measurement = await this.measureExecutionTime(async () => {
        const memoryUsage = [];

        // Create node lookup map (common D3 pattern)
        const nodeMap = new Map();
        graphData.nodes.forEach(node => {
          nodeMap.set(node.id, node);
        });

        // Create adjacency list (common graph algorithm)
        const adjacencyList = new Map();
        graphData.links.forEach(link => {
          if (!adjacencyList.has(link.source)) {
            adjacencyList.set(link.source, []);
          }
          if (!adjacencyList.has(link.target)) {
            adjacencyList.set(link.target, []);
          }
          adjacencyList.get(link.source).push(link.target);
          adjacencyList.get(link.target).push(link.source);
        });

        // Simulate force simulation data structures
        const positions = graphData.nodes.map(node => ({
          id: node.id,
          x: Math.random() * 800,
          y: Math.random() * 600,
          vx: 0,
          vy: 0,
        }));

        // Memory usage check
        if (process.memoryUsage) {
          const memBefore = process.memoryUsage();

          // Simulate some graph operations
          for (let i = 0; i < 1000; i++) {
            const randomNode =
              graphData.nodes[
                Math.floor(Math.random() * graphData.nodes.length)
              ];
            const neighbors = adjacencyList.get(randomNode.id) || [];
            const position = positions.find(p => p.id === randomNode.id);
          }

          const memAfter = process.memoryUsage();

          return {
            nodeMapSize: nodeMap.size,
            adjacencyListSize: adjacencyList.size,
            positionsCount: positions.length,
            memoryUsed: memAfter.heapUsed - memBefore.heapUsed,
          };
        }

        return {
          nodeMapSize: nodeMap.size,
          adjacencyListSize: adjacencyList.size,
          positionsCount: positions.length,
        };
      });

      return {
        executionTime: measurement.durationMs,
        dataStructures: measurement.result,
        nodeCount: graphData.nodes.length,
        linkCount: graphData.links.length,
      };
    });

    // Test 4: Search Performance
    await this.test('Search Performance with Large Dataset', async () => {
      const searchTerms = ['WRF', 'Token', 'Contract', 'Staking', 'Bridge'];
      const results = [];

      for (const term of searchTerms) {
        const measurement = await this.measureExecutionTime(async () => {
          const response = await axios.get(
            `${this.apiBaseUrl}/api/smart-contracts/search?q=${term}`
          );
          return response.data;
        });

        if (measurement.duration > 500) {
          // 500ms per search
          throw new Error(
            `Search too slow for "${term}": ${measurement.durationMs}ms`
          );
        }

        results.push({
          searchTerm: term,
          responseTime: measurement.durationMs,
          resultsCount: measurement.result.data.total,
          found: measurement.result.success,
        });
      }

      const avgResponseTime =
        results.reduce((sum, r) => sum + r.responseTime, 0) / results.length;
      const totalResults = results.reduce((sum, r) => sum + r.resultsCount, 0);

      return {
        searchTests: results.length,
        avgResponseTime: Math.round(avgResponseTime * 100) / 100,
        totalResultsFound: totalResults,
        allSearchesSuccessful: results.every(r => r.found),
      };
    });

    // Test 5: Concurrent Request Performance
    await this.test('Concurrent Request Handling', async () => {
      const concurrentRequests = 10;
      const endpoints = [
        '/api/smart-contracts/graph',
        '/api/smart-contracts/categories',
        '/api/smart-contracts/stats',
      ];

      const measurement = await this.measureExecutionTime(async () => {
        const promises = [];

        for (let i = 0; i < concurrentRequests; i++) {
          const endpoint = endpoints[i % endpoints.length];
          promises.push(axios.get(`${this.apiBaseUrl}${endpoint}`));
        }

        const results = await Promise.all(promises);
        return results;
      });

      if (measurement.duration > 3000) {
        // 3 seconds for 10 concurrent requests
        throw new Error(
          `Concurrent requests too slow: ${measurement.durationMs}ms`
        );
      }

      const allSuccessful = measurement.result.every(r => r.data.success);
      if (!allSuccessful) {
        throw new Error('Some concurrent requests failed');
      }

      return {
        concurrentRequests,
        totalTime: measurement.durationMs,
        avgTimePerRequest: measurement.durationMs / concurrentRequests,
        allRequestsSuccessful: allSuccessful,
      };
    });

    // Test 6: Cache Performance Impact
    await this.test('Cache Performance Impact', async () => {
      // First request (cache miss)
      const firstRequest = await this.measureExecutionTime(async () => {
        const response = await axios.post(
          `${this.apiBaseUrl}/api/smart-contracts/refresh-cache`
        );
        return response.data;
      });

      // Second request (cache hit)
      const secondRequest = await this.measureExecutionTime(async () => {
        const response = await axios.get(
          `${this.apiBaseUrl}/api/smart-contracts/graph`
        );
        return response.data;
      });

      const cacheSpeedup = firstRequest.duration / secondRequest.duration;

      if (cacheSpeedup < 1.5) {
        console.warn(
          `⚠️  Cache speedup lower than expected: ${cacheSpeedup.toFixed(2)}x`
        );
      }

      return {
        cacheRefreshTime: firstRequest.durationMs,
        cachedResponseTime: secondRequest.durationMs,
        speedupFactor: Math.round(cacheSpeedup * 100) / 100,
        cached: secondRequest.result.cached,
      };
    });

    // Test 7: Simulated D3.js Layout Performance
    await this.test('Simulated D3.js Force Layout Performance', async () => {
      const response = await axios.get(
        `${this.apiBaseUrl}/api/smart-contracts/graph`
      );
      const graphData = response.data.data;

      const measurement = await this.measureExecutionTime(async () => {
        // Simulate force-directed layout calculations
        const nodes = graphData.nodes.map(node => ({
          id: node.id,
          x: Math.random() * 800,
          y: Math.random() * 600,
          vx: 0,
          vy: 0,
          size: node.size || 10,
        }));

        const links = graphData.links.map(link => ({
          source: link.source,
          target: link.target,
          strength: link.strength || 1,
        }));

        // Simulate force simulation iterations
        const iterations = 300; // Typical for D3 force simulation
        for (let i = 0; i < iterations; i++) {
          // Simplified force calculations
          nodes.forEach(node => {
            let fx = 0,
              fy = 0;

            // Repulsion forces (simplified)
            nodes.forEach(other => {
              if (node.id !== other.id) {
                const dx = node.x - other.x;
                const dy = node.y - other.y;
                const distance = Math.sqrt(dx * dx + dy * dy) || 1;
                const force = 30 / (distance * distance);
                fx += (dx / distance) * force;
                fy += (dy / distance) * force;
              }
            });

            // Apply forces
            node.vx += fx * 0.1;
            node.vy += fy * 0.1;
            node.x += node.vx * 0.1;
            node.y += node.vy * 0.1;

            // Damping
            node.vx *= 0.9;
            node.vy *= 0.9;
          });
        }

        return { nodes, links, iterations };
      });

      // 60 FPS target means each frame should be < 16.67ms
      // For 300 iterations, that's about 5 seconds maximum acceptable
      if (measurement.duration > 5000) {
        throw new Error(
          `Force layout simulation too slow: ${measurement.durationMs}ms (expected <5000ms)`
        );
      }

      const fps = (measurement.result.iterations / measurement.duration) * 1000;

      return {
        simulationTime: measurement.durationMs,
        iterations: measurement.result.iterations,
        nodesProcessed: measurement.result.nodes.length,
        linksProcessed: measurement.result.links.length,
        estimatedFPS: Math.round(fps * 100) / 100,
        performanceRating: fps > 10 ? 'Good' : fps > 5 ? 'Acceptable' : 'Poor',
      };
    });

    // Print summary
    this.printSummary();
  }

  printSummary() {
    console.log('\n⚡ Performance Test Summary');
    console.log('============================');
    console.log(`✅ Passed: ${this.results.passed}`);
    console.log(`❌ Failed: ${this.results.failed}`);
    console.log(
      `📈 Success Rate: ${Math.round((this.results.passed / (this.results.passed + this.results.failed)) * 100)}%\n`
    );

    // Print failed tests details
    const failedTests = this.results.tests.filter(t => t.status === 'FAILED');
    if (failedTests.length > 0) {
      console.log('🚨 Performance Issues Detected:');
      console.log('===============================');
      failedTests.forEach(test => {
        console.log(`❌ ${test.name}: ${test.error}`);
      });
      console.log();
    }

    // Print performance summary
    const passedTests = this.results.tests.filter(t => t.status === 'PASSED');
    if (passedTests.length > 0) {
      console.log('⚡ Performance Metrics Summary:');
      console.log('==============================');

      passedTests.forEach(test => {
        console.log(`✅ ${test.name}:`);
        if (test.result) {
          Object.entries(test.result).forEach(([key, value]) => {
            console.log(
              `   ${key}: ${typeof value === 'number' ? Math.round(value * 100) / 100 : value}`
            );
          });
        }
        console.log();
      });
    }

    // Print recommendations
    console.log('💡 Performance Recommendations:');
    console.log('==============================');

    const apiTest = passedTests.find(t => t.name.includes('API Response Time'));
    if (apiTest && apiTest.result.responseTime > 1000) {
      console.log('• Consider optimizing API response time (currently > 1s)');
    } else if (apiTest) {
      console.log('✓ API response time is good');
    }

    const searchTest = passedTests.find(t =>
      t.name.includes('Search Performance')
    );
    if (searchTest && searchTest.result.avgResponseTime > 300) {
      console.log('• Consider adding search result caching');
    } else if (searchTest) {
      console.log('✓ Search performance is good');
    }

    const layoutTest = passedTests.find(t => t.name.includes('Force Layout'));
    if (layoutTest && layoutTest.result.estimatedFPS < 15) {
      console.log(
        '• Consider optimizing D3.js force simulation or reducing node count for better FPS'
      );
    } else if (layoutTest) {
      console.log('✓ Simulated D3.js performance is acceptable');
    }

    if (this.results.passed >= 6) {
      console.log(
        '\n🎉 Overall performance is good for 96 nodes and 153 edges!'
      );
    } else {
      console.log(
        '\n⚠️  Performance optimization needed before production deployment'
      );
    }

    console.log();
  }
}

// Run tests
if (require.main === module) {
  const tester = new PerformanceTester();
  tester.run().catch(error => {
    console.error('❌ Performance test suite failed:', error);
    process.exit(1);
  });
}

module.exports = PerformanceTester;
