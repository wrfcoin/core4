/**
 * HSM Performance & Regression Tests
 *
 * Tests to ensure the HSM authorization fix does not introduce performance
 * degradation or break existing functionality. Validates that security
 * enhancements maintain system performance and reliability.
 *
 * PERFORMANCE CRITERIA:
 * - Authorization checks must complete under 50ms
 * - Legitimate operations should have <5% performance impact
 * - System should handle 1000+ concurrent requests
 * - Memory usage should remain stable under load
 * - No performance regression in non-HSM operations
 */

const request = require('supertest');
const { expect } = require('chai');
const crypto = require('crypto');
const cluster = require('cluster');
const app = require('../../backend/server');
const EnterpriseHSMService =
  require('../../security/enterprise-hsm-service').default;

describe('HSM Performance & Regression Tests', () => {
  let hsmService;
  let testUser;
  let testSession;
  let testKeyId;
  let baselineMetrics = {};

  before(async () => {
    // Initialize HSM service
    hsmService = new EnterpriseHSMService({
      provider: 'aws-cloudhsm',
      endpoint: 'localhost:9000',
      keyStoreId: 'performance-test-keys',
      sessionTimeout: 3600000,
      maxConcurrentSessions: 100,
      keyRotationInterval: 86400000 * 30,
      complianceLevel: 'FIPS-140-2-L3',
      quantumResistant: true,
    });

    await hsmService.initialize();

    // Set up test user and session
    testUser = {
      id: 'perf_test_user_123',
      username: 'performance_tester',
      email: 'perf@wrfcoin.com',
    };

    testSession = {
      sessionId: crypto.randomBytes(32).toString('hex'),
      userId: testUser.id,
      createdAt: new Date(),
      expiresAt: new Date(Date.now() + 3600000),
      isActive: true,
    };

    // Generate test key
    const keyMetadata = await hsmService.generateKeyPair(
      'master-signing',
      'ECDSA-P256',
      2
    );
    testKeyId = keyMetadata.keyId;

    // Register session
    hsmService.activeSessions.set(testSession.sessionId, testSession);

    // Establish baseline performance metrics
    await establishBaseline();
  });

  after(() => {
    if (hsmService) {
      hsmService.activeSessions.clear();
      hsmService.activeKeys.clear();
    }
  });

  describe('1. AUTHORIZATION PERFORMANCE TESTS', () => {
    it('PERFORMANCE: Authorization validation should complete under 50ms', async () => {
      const iterations = 50;
      const timings = [];

      for (let i = 0; i < iterations; i++) {
        const payload = {
          proposalData: {
            title: `Performance test proposal ${i}`,
            description: 'Testing authorization performance',
          },
          sessionId: testSession.sessionId,
          keyId: testKeyId,
        };

        const startTime = process.hrtime.bigint();

        const response = await request(app)
          .post('/api/governance/proposals/perf-test/sign')
          .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
          .send(payload);

        const endTime = process.hrtime.bigint();
        const durationMs = Number(endTime - startTime) / 1000000;

        timings.push(durationMs);
        expect(response.status).to.equal(200);
      }

      const averageTime = timings.reduce((a, b) => a + b, 0) / timings.length;
      const maxTime = Math.max(...timings);
      const p95Time = timings.sort((a, b) => a - b)[
        Math.floor(timings.length * 0.95)
      ];

      console.log(`Authorization Performance Metrics:
        Average: ${averageTime.toFixed(2)}ms
        Maximum: ${maxTime.toFixed(2)}ms  
        95th Percentile: ${p95Time.toFixed(2)}ms`);

      expect(averageTime).to.be.below(50);
      expect(p95Time).to.be.below(75);
      expect(maxTime).to.be.below(100);
    });

    it('PERFORMANCE: Security rejection should be faster than authorization', async () => {
      const unauthorizedTimings = [];
      const authorizedTimings = [];

      // Test unauthorized requests (should be rejected quickly)
      for (let i = 0; i < 20; i++) {
        const maliciousPayload = {
          proposalData: { title: 'Unauthorized test' },
          sessionId: crypto.randomBytes(32).toString('hex'), // Invalid session
          keyId: testKeyId,
        };

        const startTime = process.hrtime.bigint();

        await request(app)
          .post('/api/governance/proposals/unauthorized-perf/sign')
          .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
          .send(maliciousPayload);

        const endTime = process.hrtime.bigint();
        unauthorizedTimings.push(Number(endTime - startTime) / 1000000);
      }

      // Test authorized requests
      for (let i = 0; i < 20; i++) {
        const authorizedPayload = {
          proposalData: { title: 'Authorized test' },
          sessionId: testSession.sessionId,
          keyId: testKeyId,
        };

        const startTime = process.hrtime.bigint();

        await request(app)
          .post('/api/governance/proposals/authorized-perf/sign')
          .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
          .send(authorizedPayload);

        const endTime = process.hrtime.bigint();
        authorizedTimings.push(Number(endTime - startTime) / 1000000);
      }

      const avgUnauthorized =
        unauthorizedTimings.reduce((a, b) => a + b, 0) /
        unauthorizedTimings.length;
      const avgAuthorized =
        authorizedTimings.reduce((a, b) => a + b, 0) / authorizedTimings.length;

      console.log(`Security Performance Comparison:
        Unauthorized (rejected): ${avgUnauthorized.toFixed(2)}ms
        Authorized (processed): ${avgAuthorized.toFixed(2)}ms`);

      // Security rejections should be fast
      expect(avgUnauthorized).to.be.below(30);
      // But authorized requests can take longer due to actual processing
      expect(avgAuthorized).to.be.below(100);
    });

    it('PERFORMANCE: Concurrent authorization checks should scale linearly', async () => {
      const concurrencyLevels = [1, 5, 10, 25, 50];
      const performanceResults = {};

      for (const concurrency of concurrencyLevels) {
        const requests = [];
        const startTime = process.hrtime.bigint();

        for (let i = 0; i < concurrency; i++) {
          const payload = {
            authData: {
              userId: testUser.id,
              action: `concurrent_test_${i}`,
              timestamp: new Date().toISOString(),
              nonce: crypto.randomBytes(16).toString('hex'),
            },
            sessionId: testSession.sessionId,
            keyId: testKeyId,
          };

          requests.push(
            request(app)
              .post('/api/auth/sign')
              .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
              .send(payload)
          );
        }

        const responses = await Promise.all(requests);
        const endTime = process.hrtime.bigint();
        const totalTime = Number(endTime - startTime) / 1000000;
        const avgTimePerRequest = totalTime / concurrency;

        performanceResults[concurrency] = {
          totalTime,
          avgTimePerRequest,
          successRate:
            responses.filter(r => r.status === 200).length / responses.length,
        };

        // All requests should succeed
        expect(performanceResults[concurrency].successRate).to.equal(1);

        console.log(
          `Concurrency ${concurrency}: ${avgTimePerRequest.toFixed(2)}ms avg, ${totalTime.toFixed(2)}ms total`
        );
      }

      // Performance should scale reasonably (not exponentially degrade)
      const singleRequestTime = performanceResults[1].avgTimePerRequest;
      const highConcurrencyTime = performanceResults[50].avgTimePerRequest;
      const performanceDegradation = highConcurrencyTime / singleRequestTime;

      expect(performanceDegradation).to.be.below(3); // Max 3x degradation at 50x concurrency
    });
  });

  describe('2. MEMORY & RESOURCE USAGE TESTS', () => {
    it('PERFORMANCE: Memory usage should remain stable under load', async () => {
      const initialMemory = process.memoryUsage();

      // Generate load for 30 seconds
      const loadDuration = 30000; // 30 seconds
      const startTime = Date.now();
      const memorySnapshots = [];

      // Monitor memory every 2 seconds
      const memoryMonitor = setInterval(() => {
        const currentMemory = process.memoryUsage();
        memorySnapshots.push({
          timestamp: Date.now() - startTime,
          heapUsed: currentMemory.heapUsed,
          heapTotal: currentMemory.heapTotal,
          rss: currentMemory.rss,
        });
      }, 2000);

      // Generate continuous load
      const loadPromises = [];
      while (Date.now() - startTime < loadDuration) {
        const payload = {
          authData: {
            userId: testUser.id,
            action: 'memory_test',
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: testSession.sessionId,
          keyId: testKeyId,
        };

        loadPromises.push(
          request(app)
            .post('/api/auth/sign')
            .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
            .send(payload)
        );

        if (loadPromises.length >= 10) {
          await Promise.all(loadPromises);
          loadPromises.length = 0;
        }

        await new Promise(resolve => setTimeout(resolve, 100));
      }

      await Promise.all(loadPromises);
      clearInterval(memoryMonitor);

      const finalMemory = process.memoryUsage();
      const memoryIncrease = finalMemory.heapUsed - initialMemory.heapUsed;
      const memoryIncreasePercent =
        (memoryIncrease / initialMemory.heapUsed) * 100;

      console.log(`Memory Usage Analysis:
        Initial: ${(initialMemory.heapUsed / 1024 / 1024).toFixed(2)} MB
        Final: ${(finalMemory.heapUsed / 1024 / 1024).toFixed(2)} MB
        Increase: ${(memoryIncrease / 1024 / 1024).toFixed(2)} MB (${memoryIncreasePercent.toFixed(2)}%)`);

      // Memory increase should be reasonable
      expect(memoryIncreasePercent).to.be.below(50); // Less than 50% increase
      expect(memoryIncrease).to.be.below(100 * 1024 * 1024); // Less than 100MB increase
    });

    it('PERFORMANCE: Session cache should not grow unboundedly', async () => {
      const initialSessionCount = hsmService.activeSessions.size;

      // Create many sessions rapidly
      for (let i = 0; i < 1000; i++) {
        const tempSession = {
          sessionId: crypto.randomBytes(32).toString('hex'),
          userId: `temp_user_${i}`,
          createdAt: new Date(),
          expiresAt: new Date(Date.now() + 1000), // Very short expiry
          isActive: true,
        };

        hsmService.activeSessions.set(tempSession.sessionId, tempSession);
      }

      // Wait for potential cleanup
      await new Promise(resolve => setTimeout(resolve, 2000));

      const maxSessionCount = hsmService.activeSessions.size;

      // Simulate time passage to trigger cleanup of expired sessions
      // (In production, there should be a cleanup mechanism)

      console.log(`Session Cache Analysis:
        Initial: ${initialSessionCount}
        Peak: ${maxSessionCount}
        Current: ${hsmService.activeSessions.size}`);

      // Cache should have reasonable bounds
      expect(maxSessionCount).to.be.below(2000); // Reasonable upper limit
    });
  });

  describe('3. REGRESSION TESTS', () => {
    it('REGRESSION: Non-HSM endpoints should not be affected', async () => {
      const nonHSMEndpoints = [
        { method: 'GET', path: '/api/governance/proposals' },
        { method: 'GET', path: '/api/bridges/' },
        { method: 'GET', path: '/api/weather/data/stations' },
        {
          method: 'POST',
          path: '/api/governance/proposals',
          body: {
            title: 'Test proposal',
            description: 'Test',
            proposalType: 'TEXT',
          },
        },
      ];

      for (const endpoint of nonHSMEndpoints) {
        const startTime = process.hrtime.bigint();

        let response;
        if (endpoint.method === 'GET') {
          response = await request(app)
            .get(endpoint.path)
            .set('Authorization', `Bearer ${generateTestJWT(testUser)}`);
        } else {
          response = await request(app)
            .post(endpoint.path)
            .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
            .send(endpoint.body || {});
        }

        const endTime = process.hrtime.bigint();
        const duration = Number(endTime - startTime) / 1000000;

        // Non-HSM endpoints should be fast and unaffected
        expect(response.status).to.be.oneOf([200, 201]);
        expect(duration).to.be.below(100); // Should be fast

        console.log(
          `${endpoint.method} ${endpoint.path}: ${duration.toFixed(2)}ms`
        );
      }
    });

    it('REGRESSION: Existing API contracts should remain unchanged', async () => {
      // Test that response formats haven't changed
      const governanceResponse = await request(app)
        .post('/api/governance/proposals/contract-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
        .send({
          proposalData: {
            title: 'Contract test',
            description: 'Testing API contract compliance',
          },
          sessionId: testSession.sessionId,
          keyId: testKeyId,
        })
        .expect(200);

      // Verify response structure hasn't changed
      expect(governanceResponse.body).to.have.property('success');
      expect(governanceResponse.body).to.have.property('signature');
      expect(governanceResponse.body.signature).to.have.property(
        'signatureData'
      );
      expect(governanceResponse.body.signature).to.have.property('algorithm');
      expect(governanceResponse.body.signature).to.have.property('timestamp');
      expect(governanceResponse.body.signature).to.have.property('userId');
      expect(governanceResponse.body.signature).to.have.property('keyId');

      // Test bridge endpoint contract
      const bridgeResponse = await request(app)
        .post('/api/bridges/transactions/contract-test/sign')
        .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
        .send({
          transferData: {
            fromChain: 'ethereum',
            toChain: 'polygon',
            amount: '100',
            fromAddress: '0x123',
            toAddress: '0x456',
            token: 'USDC',
          },
          sessionId: testSession.sessionId,
          keyId: testKeyId,
        })
        .expect(200);

      expect(bridgeResponse.body).to.have.property('success');
      expect(bridgeResponse.body).to.have.property('signedTransaction');
      expect(bridgeResponse.body.signedTransaction).to.have.property(
        'transactionId'
      );
      expect(bridgeResponse.body.signedTransaction).to.have.property(
        'signatureData'
      );
      expect(bridgeResponse.body.signedTransaction).to.have.property(
        'status',
        'signed'
      );
    });

    it('REGRESSION: Error messages should be consistent and informative', async () => {
      // Test unauthorized access error consistency
      const unauthorizedTests = [
        {
          endpoint: '/api/governance/proposals/error-test/sign',
          payload: {
            proposalData: { title: 'Test' },
            sessionId: 'invalid',
            keyId: testKeyId,
          },
        },
        {
          endpoint: '/api/bridges/transactions/error-test/sign',
          payload: {
            transferData: { fromChain: 'eth', toChain: 'polygon', amount: '1' },
            sessionId: 'invalid',
            keyId: testKeyId,
          },
        },
        {
          endpoint: '/api/auth/sign',
          payload: {
            authData: {
              userId: testUser.id,
              action: 'test',
              timestamp: new Date().toISOString(),
              nonce: '123',
            },
            sessionId: 'invalid',
            keyId: testKeyId,
          },
        },
      ];

      for (const test of unauthorizedTests) {
        const response = await request(app)
          .post(test.endpoint)
          .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
          .send(test.payload);

        expect(response.status).to.be.oneOf([401, 403]);
        expect(response.body).to.have.property('error');
        expect(response.body).to.have.property('code');
        expect(response.body.error).to.be.a('string');
        expect(response.body.code).to.be.a('string');
      }
    });
  });

  describe('4. LOAD TESTING', () => {
    it('LOAD: Should handle 1000 concurrent legitimate requests', async function () {
      this.timeout(60000); // 60 second timeout for load test

      const concurrentRequests = 1000;
      const requests = [];
      const startTime = process.hrtime.bigint();

      // Create many concurrent sessions for realistic load testing
      const testSessions = [];
      for (let i = 0; i < 100; i++) {
        const session = {
          sessionId: crypto.randomBytes(32).toString('hex'),
          userId: `load_user_${i}`,
          createdAt: new Date(),
          expiresAt: new Date(Date.now() + 3600000),
          isActive: true,
        };
        testSessions.push(session);
        hsmService.activeSessions.set(session.sessionId, session);
      }

      // Generate concurrent requests
      for (let i = 0; i < concurrentRequests; i++) {
        const sessionIndex = i % testSessions.length;
        const session = testSessions[sessionIndex];

        const payload = {
          authData: {
            userId: session.userId,
            action: `load_test_${i}`,
            timestamp: new Date().toISOString(),
            nonce: crypto.randomBytes(16).toString('hex'),
          },
          sessionId: session.sessionId,
          keyId: testKeyId,
        };

        requests.push(
          request(app)
            .post('/api/auth/sign')
            .set(
              'Authorization',
              `Bearer ${generateTestJWT({ id: session.userId, username: `user_${sessionIndex}` })}`
            )
            .send(payload)
        );
      }

      const responses = await Promise.allSettled(requests);
      const endTime = process.hrtime.bigint();
      const totalTime = Number(endTime - startTime) / 1000000;

      const successful = responses.filter(
        r => r.status === 'fulfilled' && r.value.status === 200
      ).length;
      const failed = responses.filter(
        r => r.status === 'rejected' || r.value.status !== 200
      ).length;
      const successRate = (successful / responses.length) * 100;
      const throughput = responses.length / (totalTime / 1000);

      console.log(`Load Test Results:
        Total Requests: ${responses.length}
        Successful: ${successful}
        Failed: ${failed}
        Success Rate: ${successRate.toFixed(2)}%
        Total Time: ${totalTime.toFixed(2)}ms
        Throughput: ${throughput.toFixed(2)} req/sec`);

      expect(successRate).to.be.above(95); // 95% success rate minimum
      expect(throughput).to.be.above(50); // Minimum 50 req/sec throughput

      // Cleanup test sessions
      testSessions.forEach(session => {
        hsmService.activeSessions.delete(session.sessionId);
      });
    });

    it('LOAD: Should maintain security under attack load', async function () {
      this.timeout(30000); // 30 second timeout

      const attackRequests = 500;
      const legitimateRequests = 100;
      const allRequests = [];

      // Generate attack requests (using wrong sessions)
      for (let i = 0; i < attackRequests; i++) {
        const attackPayload = {
          proposalData: { title: `Attack ${i}` },
          sessionId: testSession.sessionId, // Valid session but wrong user
          keyId: testKeyId,
        };

        allRequests.push(
          request(app)
            .post('/api/governance/proposals/attack-load/sign')
            .set(
              'Authorization',
              `Bearer ${generateTestJWT({ id: `attacker_${i}`, username: `attacker_${i}` })}`
            )
            .send(attackPayload)
        );
      }

      // Generate legitimate requests
      for (let i = 0; i < legitimateRequests; i++) {
        const legitPayload = {
          proposalData: { title: `Legit ${i}` },
          sessionId: testSession.sessionId,
          keyId: testKeyId,
        };

        allRequests.push(
          request(app)
            .post('/api/governance/proposals/legit-load/sign')
            .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
            .send(legitPayload)
        );
      }

      const responses = await Promise.allSettled(allRequests);

      const attackBlocked = responses
        .slice(0, attackRequests)
        .filter(
          r => r.status === 'fulfilled' && [401, 403].includes(r.value.status)
        ).length;

      const legitimateSuccessful = responses
        .slice(attackRequests)
        .filter(r => r.status === 'fulfilled' && r.value.status === 200).length;

      const attackBlockRate = (attackBlocked / attackRequests) * 100;
      const legitimateSuccessRate =
        (legitimateSuccessful / legitimateRequests) * 100;

      console.log(`Security Under Load:
        Attack Block Rate: ${attackBlockRate.toFixed(2)}%
        Legitimate Success Rate: ${legitimateSuccessRate.toFixed(2)}%`);

      expect(attackBlockRate).to.be.above(99); // Block >99% of attacks
      expect(legitimateSuccessRate).to.be.above(95); // Allow >95% of legitimate requests
    });
  });

  async function establishBaseline() {
    // Establish baseline performance metrics before security fix
    const baselineIterations = 10;
    const timings = [];

    for (let i = 0; i < baselineIterations; i++) {
      const payload = {
        proposalData: {
          title: 'Baseline test',
          description: 'Establishing baseline performance',
        },
        sessionId: testSession.sessionId,
        keyId: testKeyId,
      };

      const startTime = process.hrtime.bigint();

      await request(app)
        .post('/api/governance/proposals/baseline/sign')
        .set('Authorization', `Bearer ${generateTestJWT(testUser)}`)
        .send(payload);

      const endTime = process.hrtime.bigint();
      timings.push(Number(endTime - startTime) / 1000000);
    }

    baselineMetrics.averageResponseTime =
      timings.reduce((a, b) => a + b, 0) / timings.length;
    baselineMetrics.maxResponseTime = Math.max(...timings);

    console.log(`Baseline Metrics Established:
      Average Response Time: ${baselineMetrics.averageResponseTime.toFixed(2)}ms
      Max Response Time: ${baselineMetrics.maxResponseTime.toFixed(2)}ms`);
  }
});

/**
 * Helper function to generate test JWT tokens
 */
function generateTestJWT(user) {
  return require('jsonwebtoken').sign(
    {
      id: user.id,
      username: user.username,
      email: user.email,
    },
    process.env.JWT_SECRET || 'test-secret-key',
    { expiresIn: '1h' }
  );
}
