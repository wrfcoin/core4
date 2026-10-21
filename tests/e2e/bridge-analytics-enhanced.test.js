/**
 * Enhanced Bridge Analytics Test with MCP Integration
 * Demonstrates comprehensive testing of bridge analytics and performance monitoring
 */

const { test, expect } = require('@playwright/test');

test.describe('Enhanced Bridge Analytics Verification', () => {
  let testStartTime;
  let analyticsData = {};

  test.beforeEach(async ({ page }) => {
    testStartTime = Date.now();

    // Navigate to the main application
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Collect initial analytics data from the cross-chain server
    const metricsResponse = await page.request.get(
      'http://localhost:3000/api/crosschain/metrics'
    );
    const bridgeStatusResponse = await page.request.get(
      'http://localhost:3000/api/crosschain/bridge-status'
    );

    analyticsData.metrics = await metricsResponse.json();
    analyticsData.bridgeStatus = await bridgeStatusResponse.json();

    console.log(
      'Test environment prepared with',
      analyticsData.metrics.metrics.total_protocols,
      'protocols'
    );
  });

  test('Comprehensive Bridge Analytics Data Validation', async ({ page }) => {
    // Test 1: Validate cross-chain server metrics are accurate
    const serverMetrics = analyticsData.metrics.metrics;

    expect(serverMetrics.total_protocols).toBeGreaterThan(0);
    expect(serverMetrics.active_protocols).toBeGreaterThan(0);
    expect(serverMetrics.supported_chains).toBeGreaterThan(0);
    expect(serverMetrics.success_rate).toMatch(/\d+\.?\d*%/);

    // Test 2: Validate bridge status data
    const bridgeStatus = analyticsData.bridgeStatus.bridges;
    expect(bridgeStatus.length).toBeGreaterThan(0);
    expect(analyticsData.bridgeStatus.overall_status).toBe('operational');

    // Validate each bridge has required fields
    for (const bridge of bridgeStatus) {
      expect(bridge.protocol).toBeTruthy();
      expect(bridge.status).toBeTruthy();
      expect(bridge.uptime).toMatch(/\d+\.?\d*%/);
      expect(bridge.avg_processing_time).toBeTruthy();
    }

    console.log('✅ Server analytics validation completed');
    console.log('- Total protocols:', serverMetrics.total_protocols);
    console.log('- Success rate:', serverMetrics.success_rate);
    console.log('- Bridge count:', bridgeStatus.length);
  });

  test('Real-time Analytics Performance Testing', async ({ page }) => {
    // Test performance of analytics endpoints under load
    const testDuration = 10000; // 10 seconds
    const requestInterval = 1000; // 1 second
    const performanceData = [];

    const startTime = Date.now();

    while (Date.now() - startTime < testDuration) {
      const requestStart = Date.now();

      try {
        // Make concurrent requests to multiple endpoints
        const [metricsRes, statusRes, protocolsRes] = await Promise.all([
          page.request.get('http://localhost:3000/api/crosschain/metrics'),
          page.request.get(
            'http://localhost:3000/api/crosschain/bridge-status'
          ),
          page.request.get('http://localhost:3000/api/crosschain/protocols'),
        ]);

        const requestEnd = Date.now();
        const responseTime = requestEnd - requestStart;

        performanceData.push({
          timestamp: requestEnd,
          responseTime,
          success: metricsRes.ok() && statusRes.ok() && protocolsRes.ok(),
        });

        // Verify response quality
        const metricsData = await metricsRes.json();
        expect(metricsData.success).toBe(true);
      } catch (error) {
        performanceData.push({
          timestamp: Date.now(),
          responseTime: -1,
          success: false,
          error: error.message,
        });
      }

      await page.waitForTimeout(requestInterval);
    }

    // Analyze performance data
    const successfulRequests = performanceData.filter(d => d.success);
    const avgResponseTime =
      successfulRequests.reduce((sum, d) => sum + d.responseTime, 0) /
      successfulRequests.length;
    const successRate =
      (successfulRequests.length / performanceData.length) * 100;

    console.log('📊 Performance Test Results:');
    console.log('- Total requests:', performanceData.length);
    console.log('- Success rate:', successRate.toFixed(2) + '%');
    console.log('- Average response time:', avgResponseTime.toFixed(2) + 'ms');

    // Performance assertions
    expect(successRate).toBeGreaterThan(95); // 95% success rate minimum
    expect(avgResponseTime).toBeLessThan(1000); // Under 1 second average
  });

  test('Cross-Chain Message Processing Analytics', async ({ page }) => {
    // Test the message processing capabilities
    const testMessage = {
      protocol: 'axelar',
      source_chain: 'ethereum',
      destination_chain: 'polygon',
      message_type: 'token_transfer',
      payload: { amount: '1.0', token: 'USDC' },
      recipient: '0x742d35Cc6635C0532925a3b8Bc9c45Ad0523c4B1',
    };

    // Send a test cross-chain message
    const sendResponse = await page.request.post(
      'http://localhost:3000/api/crosschain/send',
      {
        data: testMessage,
      }
    );

    expect(sendResponse.ok()).toBeTruthy();
    const sendData = await sendResponse.json();
    expect(sendData.success).toBe(true);
    expect(sendData.message_id).toBeTruthy();

    const messageId = sendData.message_id;

    // Track message status over time
    const statusUpdates = [];
    const trackingDuration = 15000; // 15 seconds
    const checkInterval = 2000; // 2 seconds

    const startTracking = Date.now();

    while (Date.now() - startTracking < trackingDuration) {
      const statusResponse = await page.request.get(
        `http://localhost:3000/api/crosschain/messages/${messageId}`
      );

      if (statusResponse.ok()) {
        const statusData = await statusResponse.json();
        statusUpdates.push({
          timestamp: Date.now(),
          status: statusData.message.status,
          message: statusData.message,
        });

        console.log(`Message ${messageId}: ${statusData.message.status}`);

        // If completed, break early
        if (statusData.message.status === 'completed') {
          break;
        }
      }

      await page.waitForTimeout(checkInterval);
    }

    // Analyze message progression
    expect(statusUpdates.length).toBeGreaterThan(0);
    expect(statusUpdates[0].status).toBe('pending');

    // Check if message progressed through expected states
    const finalStatus = statusUpdates[statusUpdates.length - 1];
    expect(['pending', 'relaying', 'completed']).toContain(finalStatus.status);

    console.log('🔄 Message Processing Analytics:');
    console.log('- Message ID:', messageId);
    console.log('- Status updates:', statusUpdates.length);
    console.log('- Final status:', finalStatus.status);
    console.log('- Protocol used:', testMessage.protocol);
  });

  test('Bridge Analytics Data Consistency Check', async ({ page }) => {
    // Collect data from multiple endpoints and verify consistency
    const endpoints = [
      '/api/crosschain/protocols',
      '/api/crosschain/metrics',
      '/api/crosschain/bridge-status',
      '/api/crosschain/messages',
    ];

    const responses = {};

    for (const endpoint of endpoints) {
      const response = await page.request.get(
        `http://localhost:3000${endpoint}`
      );
      expect(response.ok()).toBeTruthy();
      responses[endpoint] = await response.json();
    }

    // Verify data consistency
    const protocolsData = responses['/api/crosschain/protocols'];
    const metricsData = responses['/api/crosschain/metrics'];
    const statusData = responses['/api/crosschain/bridge-status'];

    // Check protocol count consistency
    const protocolCount = Object.keys(protocolsData.protocols).length;
    const metricsProtocolCount = metricsData.metrics.total_protocols;
    const statusBridgeCount = statusData.bridges.length;

    expect(protocolCount).toBe(metricsProtocolCount);
    expect(protocolCount).toBe(statusBridgeCount);

    // Verify protocol names match across endpoints
    const protocolNames = Object.keys(protocolsData.protocols);
    const statusProtocolNames = statusData.bridges.map(b => b.protocol);

    for (const protocolName of protocolNames) {
      const protocol = protocolsData.protocols[protocolName];
      const statusBridge = statusData.bridges.find(
        b =>
          b.protocol.toLowerCase().includes(protocolName) ||
          protocolName.includes(b.protocol.toLowerCase().replace(/[^a-z]/g, ''))
      );

      if (statusBridge) {
        expect(protocol.status).toBe('active'); // All should be active
        expect(statusBridge.status).toBe('active');
      }
    }

    console.log('🔄 Data Consistency Check Results:');
    console.log('- Protocol consistency: ✅');
    console.log('- Count validation: ✅');
    console.log('- Status alignment: ✅');
  });

  test('Error Handling and Resilience Testing', async ({ page }) => {
    // Test various error scenarios and recovery
    const errorTests = [
      {
        name: 'Invalid Protocol Request',
        endpoint: '/api/crosschain/protocols/nonexistent',
        expectedStatus: 404,
      },
      {
        name: 'Invalid Message ID',
        endpoint: '/api/crosschain/messages/invalid-id',
        expectedStatus: 404,
      },
      {
        name: 'Malformed Send Request',
        endpoint: '/api/crosschain/send',
        method: 'POST',
        data: { invalid: 'data' },
        expectedStatus: 400,
      },
    ];

    for (const errorTest of errorTests) {
      try {
        let response;

        if (errorTest.method === 'POST') {
          response = await page.request.post(
            `http://localhost:3000${errorTest.endpoint}`,
            {
              data: errorTest.data,
            }
          );
        } else {
          response = await page.request.get(
            `http://localhost:3000${errorTest.endpoint}`
          );
        }

        expect(response.status()).toBe(errorTest.expectedStatus);

        const responseData = await response.json();
        expect(responseData.success).toBe(false);
        expect(responseData.error).toBeTruthy();

        console.log(
          `✅ ${errorTest.name}: Handled correctly (${response.status()})`
        );
      } catch (error) {
        console.error(`❌ ${errorTest.name}: Test failed -`, error.message);
        throw error;
      }
    }

    // Test server recovery after errors
    const healthResponse = await page.request.get(
      'http://localhost:3000/health'
    );
    expect(healthResponse.ok()).toBeTruthy();

    const healthData = await healthResponse.json();
    expect(healthData.status).toBe('healthy');

    console.log('🛡️ Error handling tests completed successfully');
  });

  test.afterEach(async ({ page }) => {
    const testEndTime = Date.now();
    const testDuration = testEndTime - testStartTime;

    console.log(`⏱️ Test completed in ${testDuration}ms`);

    // Cleanup: Get final server state
    try {
      const finalMetrics = await page.request.get(
        'http://localhost:3000/api/crosschain/metrics'
      );
      const finalData = await finalMetrics.json();

      console.log('📊 Final Server State:');
      console.log('- Active protocols:', finalData.metrics.active_protocols);
      console.log('- Success rate:', finalData.metrics.success_rate);
    } catch (error) {
      console.log('Could not fetch final metrics:', error.message);
    }
  });
});
