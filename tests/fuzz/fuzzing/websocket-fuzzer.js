/**
 * WebSocket Fuzzer for Wrfcoin Platform
 * Tests WebSocket connections, message handling, and protocol compliance
 */

const WebSocket = require('ws');
const { faker } = require('@faker-js/faker');
const winston = require('winston');
const config = require('../config/fuzzing-config');
const FuzzReporter = require('../reporting/fuzz-reporter');

class WebSocketFuzzer {
  constructor(options = {}) {
    this.config = { ...config, ...options };
    this.reporter = new FuzzReporter('websocket');
    this.setupLogger();
    this.connections = [];
    this.testResults = [];
  }

  setupLogger() {
    this.logger = winston.createLogger({
      level: 'info',
      format: winston.format.combine(
        winston.format.timestamp(),
        winston.format.json()
      ),
      transports: [
        new winston.transports.Console(),
        new winston.transports.File({
          filename: 'fuzz-testing/logs/websocket-fuzzer.log',
        }),
      ],
    });
  }

  /**
   * Generate fuzzed WebSocket messages
   */
  generateFuzzedMessage(type = 'random') {
    switch (type) {
      case 'valid':
        return this.generateValidMessage();
      case 'malformed_json':
        return this.generateMalformedJsonMessage();
      case 'oversized':
        return this.generateOversizedMessage();
      case 'protocol_violation':
        return this.generateProtocolViolationMessage();
      case 'injection_attack':
        return this.generateInjectionAttackMessage();
      case 'binary':
        return this.generateBinaryMessage();
      default:
        return this.generateRandomMessage();
    }
  }

  generateValidMessage() {
    const actions = ['subscribe', 'unsubscribe', 'ping'];
    const channels = this.config.websocket.channels;

    const message = {
      action: faker.helpers.arrayElement(actions),
      timestamp: new Date().toISOString(),
    };

    if (message.action === 'subscribe' || message.action === 'unsubscribe') {
      message.channels = faker.helpers.arrayElements(channels, {
        min: 1,
        max: 3,
      });
    }

    return JSON.stringify(message);
  }

  generateMalformedJsonMessage() {
    const malformedOptions = [
      '{"action": "subscribe", "channels": [}', // Missing closing bracket
      '{"action": "subscribe" "channels": []}', // Missing comma
      '{action: "subscribe", "channels": []}', // Unquoted key
      '{"action": "subscribe", "channels": []]', // Extra bracket
      'not_json_at_all',
      '',
      null,
      undefined,
    ];

    return faker.helpers.arrayElement(malformedOptions);
  }

  generateOversizedMessage() {
    const baseMessage = {
      action: 'subscribe',
      channels: ['global'],
      data: 'A'.repeat(1024 * 1024), // 1MB of data
    };

    return JSON.stringify(baseMessage);
  }

  generateProtocolViolationMessage() {
    const violations = [
      { action: 'invalid_action', channels: ['global'] },
      { action: 'subscribe', channels: ['invalid_channel'] },
      { action: 'subscribe', channels: null },
      { action: 'subscribe', channels: 'not_array' },
      { malicious: 'payload', script: '<script>alert("xss")</script>' },
    ];

    return JSON.stringify(faker.helpers.arrayElement(violations));
  }

  generateInjectionAttackMessage() {
    const attacks = [
      {
        action: 'subscribe',
        channels: ["'; DROP TABLE users; --"],
        injection: '<script>alert("XSS")</script>',
      },
      {
        action: 'ping',
        data: '{{constructor.constructor("return process")().exit()}}',
      },
      {
        action: 'subscribe',
        channels: ['global'],
        __proto__: { polluted: true },
      },
    ];

    return JSON.stringify(faker.helpers.arrayElement(attacks));
  }

  generateBinaryMessage() {
    // Generate random binary data
    const buffer = Buffer.alloc(faker.number.int({ min: 1, max: 1000 }));
    for (let i = 0; i < buffer.length; i++) {
      buffer[i] = faker.number.int({ min: 0, max: 255 });
    }
    return buffer;
  }

  generateRandomMessage() {
    const types = [
      'valid',
      'malformed_json',
      'oversized',
      'protocol_violation',
      'injection_attack',
      'binary',
    ];

    const randomType = faker.helpers.arrayElement(types);
    return this.generateFuzzedMessage(randomType);
  }

  /**
   * Create a WebSocket connection for testing
   */
  async createTestConnection() {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(this.config.websocket.url);
      const connectionInfo = {
        id: faker.string.uuid(),
        createdAt: Date.now(),
        events: [],
        errors: [],
      };

      const timeout = setTimeout(() => {
        reject(new Error('Connection timeout'));
      }, this.config.fuzzing.timeouts.websocket);

      ws.on('open', () => {
        clearTimeout(timeout);
        connectionInfo.events.push({ type: 'open', timestamp: Date.now() });
        this.logger.info(
          `WebSocket connection established: ${connectionInfo.id}`
        );
        resolve({ ws, connectionInfo });
      });

      ws.on('message', data => {
        try {
          const parsed = JSON.parse(data);
          connectionInfo.events.push({
            type: 'message',
            timestamp: Date.now(),
            data: parsed,
          });
        } catch (error) {
          connectionInfo.events.push({
            type: 'message_unparseable',
            timestamp: Date.now(),
            raw: data.toString(),
            error: error.message,
          });
        }
      });

      ws.on('error', error => {
        connectionInfo.errors.push({
          timestamp: Date.now(),
          error: error.message,
          code: error.code,
        });
        this.logger.error(`WebSocket error: ${connectionInfo.id}`, error);
      });

      ws.on('close', (code, reason) => {
        connectionInfo.events.push({
          type: 'close',
          timestamp: Date.now(),
          code,
          reason: reason.toString(),
        });
        this.logger.info(`WebSocket connection closed: ${connectionInfo.id}`, {
          code,
          reason,
        });
      });
    });
  }

  /**
   * Test a single WebSocket connection with fuzzed messages
   */
  async testConnection(iterations = 50) {
    const testResult = {
      connectionId: faker.string.uuid(),
      startTime: Date.now(),
      iterations,
      messagesSent: 0,
      responses: [],
      errors: [],
      vulnerabilities: [],
    };

    try {
      const { ws, connectionInfo } = await this.createTestConnection();
      testResult.connectionId = connectionInfo.id;

      // Wait for initial connection message
      await this.sleep(1000);

      for (let i = 0; i < iterations; i++) {
        try {
          const messageType = this.selectMessageType();
          const fuzzedMessage = this.generateFuzzedMessage(messageType);

          const beforeEvents = connectionInfo.events.length;
          const beforeErrors = connectionInfo.errors.length;

          // Send the fuzzed message
          if (Buffer.isBuffer(fuzzedMessage)) {
            ws.send(fuzzedMessage);
          } else {
            ws.send(fuzzedMessage);
          }

          testResult.messagesSent++;

          // Wait for response
          await this.sleep(500);

          // Analyze response
          const newEvents = connectionInfo.events.slice(beforeEvents);
          const newErrors = connectionInfo.errors.slice(beforeErrors);

          const responseAnalysis = this.analyzeResponse(
            messageType,
            fuzzedMessage,
            newEvents,
            newErrors
          );

          testResult.responses.push({
            iteration: i,
            messageType,
            message: Buffer.isBuffer(fuzzedMessage)
              ? '[Binary Data]'
              : fuzzedMessage,
            events: newEvents,
            errors: newErrors,
            analysis: responseAnalysis,
          });

          // Check for vulnerabilities
          if (responseAnalysis.vulnerability) {
            testResult.vulnerabilities.push(responseAnalysis.vulnerability);
          }

          // Check if connection is still alive
          if (ws.readyState !== WebSocket.OPEN) {
            this.logger.warn(
              `Connection closed unexpectedly at iteration ${i}`
            );
            break;
          }
        } catch (error) {
          testResult.errors.push({
            iteration: i,
            error: error.message,
            type: 'send_failed',
          });
        }
      }

      // Clean up connection
      if (ws.readyState === WebSocket.OPEN) {
        ws.close();
      }

      testResult.endTime = Date.now();
      testResult.duration = testResult.endTime - testResult.startTime;
    } catch (error) {
      testResult.errors.push({
        error: error.message,
        type: 'connection_failed',
      });
    }

    return testResult;
  }

  selectMessageType() {
    const types = [
      'valid',
      'malformed_json',
      'oversized',
      'protocol_violation',
      'injection_attack',
      'binary',
    ];
    const weights = [40, 15, 10, 15, 15, 5]; // Percentage weights

    const random = Math.random() * 100;
    let cumulative = 0;

    for (let i = 0; i < types.length; i++) {
      cumulative += weights[i];
      if (random <= cumulative) {
        return types[i];
      }
    }

    return 'valid';
  }

  analyzeResponse(messageType, sentMessage, events, errors) {
    const analysis = {
      messageType,
      hasResponse: events.length > 0,
      hasError: errors.length > 0,
      vulnerability: null,
    };

    // Check for server crashes (connection closed unexpectedly)
    const closeEvent = events.find(e => e.type === 'close');
    if (closeEvent && messageType !== 'valid') {
      analysis.vulnerability = {
        type: 'denial_of_service',
        severity: 'high',
        description: 'Server closed connection after receiving fuzzed message',
        messageType,
        closeCode: closeEvent.code,
      };
    }

    // Check for information disclosure
    const messageEvents = events.filter(e => e.type === 'message');
    messageEvents.forEach(event => {
      if (event.data && typeof event.data === 'object') {
        if (
          event.data.error &&
          event.data.error.includes &&
          event.data.error.includes('Error:')
        ) {
          analysis.vulnerability = {
            type: 'information_disclosure',
            severity: 'medium',
            description: 'Server exposed error details in WebSocket response',
            errorDetails: event.data.error,
          };
        }
      }
    });

    // Check for potential injection vulnerabilities
    if (messageType === 'injection_attack' && events.length > 0) {
      analysis.vulnerability = {
        type: 'injection_potential',
        severity: 'high',
        description:
          'Server processed injection attack payload without proper validation',
        messageType,
      };
    }

    // Check for memory exhaustion (oversized messages accepted)
    if (messageType === 'oversized' && !errors.length && events.length > 0) {
      analysis.vulnerability = {
        type: 'resource_exhaustion',
        severity: 'medium',
        description:
          'Server accepted oversized WebSocket message without rejection',
        messageType,
      };
    }

    return analysis;
  }

  async sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  /**
   * Run comprehensive WebSocket fuzzing
   */
  async runComprehensiveFuzzing(
    duration = this.config.fuzzing.duration.standard
  ) {
    const startTime = Date.now();
    const results = [];

    this.logger.info('Starting comprehensive WebSocket fuzzing', {
      duration,
      url: this.config.websocket.url,
    });

    let connectionCount = 0;
    while (Date.now() - startTime < duration || duration === -1) {
      try {
        connectionCount++;
        this.logger.info(`Testing WebSocket connection #${connectionCount}`);

        const iterations = Math.min(
          50,
          Math.floor(duration / (connectionCount * 1000))
        );
        const result = await this.testConnection(Math.max(5, iterations));
        results.push(result);

        // Brief pause between connections
        await this.sleep(2000);

        // Break if duration is reached
        if (duration > 0 && Date.now() - startTime >= duration) {
          break;
        }
      } catch (error) {
        this.logger.error('WebSocket test failed', error);
      }
    }

    // Generate comprehensive report
    const report = this.generateComprehensiveReport(results);
    await this.reporter.saveReport(report, 'websocket-fuzz-report');

    this.logger.info('Comprehensive WebSocket fuzzing completed', {
      totalConnections: results.length,
      totalVulnerabilities: results.reduce(
        (sum, r) => sum + r.vulnerabilities.length,
        0
      ),
    });

    return report;
  }

  generateComprehensiveReport(results) {
    const totalConnections = results.length;
    const totalMessages = results.reduce((sum, r) => sum + r.messagesSent, 0);
    const totalVulnerabilities = results.reduce(
      (sum, r) => sum + r.vulnerabilities.length,
      0
    );
    const totalErrors = results.reduce((sum, r) => sum + r.errors.length, 0);

    return {
      summary: {
        timestamp: new Date().toISOString(),
        totalConnections,
        totalMessages,
        totalErrors,
        totalVulnerabilities,
        averageMessagesPerConnection: (
          totalMessages / totalConnections
        ).toFixed(2),
      },
      vulnerabilities: results.flatMap(r => r.vulnerabilities),
      connectionResults: results,
      recommendations: this.generateRecommendations(results),
    };
  }

  generateRecommendations(results) {
    const recommendations = [];

    // Check for high error rates
    const errorRate =
      results.reduce((sum, r) => sum + r.errors.length, 0) / results.length;
    if (errorRate > 2) {
      recommendations.push({
        type: 'high_error_rate',
        message: `High WebSocket error rate detected (${errorRate.toFixed(1)} errors per connection). Improve error handling.`,
      });
    }

    // Check for connection instability
    const unexpectedCloses = results.filter(r =>
      r.responses.some(resp =>
        resp.events.some(e => e.type === 'close' && e.code !== 1000)
      )
    ).length;

    if (unexpectedCloses > results.length * 0.1) {
      recommendations.push({
        type: 'connection_instability',
        message:
          'High rate of unexpected connection closures. Review WebSocket message handling.',
      });
    }

    // Security recommendations based on vulnerabilities
    const vulnTypes = results.flatMap(r => r.vulnerabilities).map(v => v.type);
    const uniqueVulnTypes = [...new Set(vulnTypes)];

    uniqueVulnTypes.forEach(type => {
      switch (type) {
        case 'denial_of_service':
          recommendations.push({
            type: 'security',
            message:
              'DoS vulnerability found. Implement proper input validation and rate limiting for WebSocket messages.',
          });
          break;
        case 'information_disclosure':
          recommendations.push({
            type: 'security',
            message:
              'Information disclosure detected. Avoid exposing error details in WebSocket responses.',
          });
          break;
        case 'injection_potential':
          recommendations.push({
            type: 'security',
            message:
              'Injection vulnerability potential. Implement proper input sanitization for WebSocket messages.',
          });
          break;
        case 'resource_exhaustion':
          recommendations.push({
            type: 'security',
            message:
              'Resource exhaustion risk. Implement message size limits for WebSocket connections.',
          });
          break;
      }
    });

    return recommendations;
  }
}

module.exports = WebSocketFuzzer;

// Run if called directly
if (require.main === module) {
  const fuzzer = new WebSocketFuzzer();
  fuzzer
    .runComprehensiveFuzzing()
    .then(report => {
      console.log('WebSocket fuzzing completed. Report generated.');
      console.log(
        `Total vulnerabilities found: ${report.summary.totalVulnerabilities}`
      );
      process.exit(0);
    })
    .catch(error => {
      console.error('WebSocket fuzzing failed:', error);
      process.exit(1);
    });
}
