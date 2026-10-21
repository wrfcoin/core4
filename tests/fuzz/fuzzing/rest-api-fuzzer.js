/**
 * REST API Fuzzer for Wrfcoin Platform
 * Uses JSON schema-based fuzzing with security-focused mutations
 */

const axios = require('axios');
const Ajv = require('ajv');
const addFormats = require('ajv-formats');
const { faker } = require('@faker-js/faker');
const winston = require('winston');
const config = require('../config/fuzzing-config');
const FuzzReporter = require('../reporting/fuzz-reporter');

class RestApiFuzzer {
  constructor(options = {}) {
    this.config = { ...config, ...options };
    this.ajv = new Ajv({ allErrors: true });
    addFormats(this.ajv);
    this.reporter = new FuzzReporter('rest-api');
    this.setupLogger();
    this.setupAxios();
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
          filename: 'fuzz-testing/logs/rest-api-fuzzer.log',
        }),
      ],
    });
  }

  setupAxios() {
    // Configure axios with timeouts and error handling
    axios.defaults.timeout = this.config.fuzzing.timeouts.request;
    axios.defaults.validateStatus = () => true; // Don't throw on HTTP errors
  }

  /**
   * Generate fuzzed data based on JSON schema
   */
  generateFuzzedData(schema, mutationType = 'random') {
    switch (mutationType) {
      case 'valid':
        return this.generateValidData(schema);
      case 'boundary':
        return this.generateBoundaryData(schema);
      case 'invalid':
        return this.generateInvalidData(schema);
      case 'security':
        return this.generateSecurityTestData(schema);
      case 'malformed':
        return this.generateMalformedData(schema);
      default:
        return this.generateRandomMutationData(schema);
    }
  }

  generateValidData(schema) {
    const data = {};

    if (schema.properties) {
      Object.keys(schema.properties).forEach(key => {
        const prop = schema.properties[key];
        data[key] = this.generateValidValue(prop);
      });
    }

    return data;
  }

  generateValidValue(property) {
    switch (property.type) {
      case 'string':
        if (property.format === 'date-time') {
          return faker.date.recent().toISOString();
        }
        if (property.pattern) {
          return this.generatePatternString(property.pattern);
        }
        if (property.enum) {
          return faker.helpers.arrayElement(property.enum);
        }
        const maxLen = property.maxLength || 50;
        return faker.lorem.words(Math.floor(maxLen / 10));

      case 'number':
        const min = property.minimum || 0;
        const max = property.maximum || 1000;
        return faker.number.float({ min, max, precision: 0.01 });

      case 'integer':
        return faker.number.int({
          min: property.minimum || 0,
          max: property.maximum || 1000,
        });

      case 'boolean':
        return faker.datatype.boolean();

      case 'array':
        return [this.generateValidValue(property.items)];

      default:
        return faker.lorem.word();
    }
  }

  generatePatternString(pattern) {
    // Simple pattern generators for common cases
    if (pattern.includes('Wrf')) {
      return 'Wrf' + faker.string.alphanumeric(32);
    }
    return faker.string.alphanumeric(20);
  }

  generateBoundaryData(schema) {
    const data = this.generateValidData(schema);

    // Apply boundary mutations
    Object.keys(data).forEach(key => {
      const prop = schema.properties[key];
      if (prop.type === 'string' && prop.maxLength) {
        // Test exactly at boundary
        data[key] = 'A'.repeat(prop.maxLength);
      } else if (prop.type === 'number') {
        // Test at min/max boundaries
        data[key] =
          Math.random() > 0.5
            ? prop.minimum || 0
            : prop.maximum || Number.MAX_SAFE_INTEGER;
      }
    });

    return data;
  }

  generateInvalidData(schema) {
    const data = this.generateValidData(schema);

    // Apply invalid mutations
    Object.keys(data).forEach(key => {
      const prop = schema.properties[key];
      switch (prop.type) {
        case 'string':
          if (prop.maxLength) {
            // Exceed max length
            data[key] = 'A'.repeat(prop.maxLength * 2);
          }
          break;
        case 'number':
          if (prop.minimum !== undefined) {
            // Go below minimum
            data[key] = prop.minimum - 1000;
          }
          break;
        case 'integer':
          // Send float instead of integer
          data[key] = 3.14159;
          break;
      }
    });

    return data;
  }

  generateSecurityTestData(schema) {
    const data = this.generateValidData(schema);
    const { sqlInjectionPatterns, xssPatterns, bufferOverflowPatterns } =
      this.config.security;

    // Apply security-focused mutations
    Object.keys(data).forEach(key => {
      const prop = schema.properties[key];
      if (prop.type === 'string') {
        const patterns = [
          ...sqlInjectionPatterns,
          ...xssPatterns,
          ...bufferOverflowPatterns,
        ];
        data[key] = faker.helpers.arrayElement(patterns);
      }
    });

    return data;
  }

  generateMalformedData(schema) {
    const mutations = [
      null,
      undefined,
      {},
      [],
      '',
      'not_json',
      { malformed: true, extra: 'field' },
      Buffer.alloc(10000, 'X').toString(),
    ];

    return faker.helpers.arrayElement(mutations);
  }

  generateRandomMutationData(schema) {
    const mutationTypes = ['valid', 'boundary', 'invalid', 'security'];
    const randomType = faker.helpers.arrayElement(mutationTypes);
    return this.generateFuzzedData(schema, randomType);
  }

  /**
   * Fuzz a specific endpoint
   */
  async fuzzEndpoint(server, endpoint, iterations = 100) {
    const results = {
      endpoint: `${server.name}: ${endpoint.method} ${endpoint.path}`,
      totalRequests: 0,
      errors: [],
      responses: {
        success: 0,
        clientError: 0,
        serverError: 0,
        timeout: 0,
      },
      vulnerabilities: [],
    };

    this.logger.info(
      `Starting to fuzz ${results.endpoint} with ${iterations} iterations`
    );

    for (let i = 0; i < iterations; i++) {
      try {
        const mutationType = this.selectMutationType();
        const payload = this.generateTestPayload(endpoint, mutationType);

        const startTime = Date.now();
        const response = await this.makeRequest(
          server.baseUrl,
          endpoint,
          payload
        );
        const duration = Date.now() - startTime;

        results.totalRequests++;
        this.categorizeResponse(response, results);

        // Check for potential vulnerabilities
        const vulnerability = this.analyzeResponseForVulnerabilities(
          endpoint,
          payload,
          response,
          duration
        );

        if (vulnerability) {
          results.vulnerabilities.push(vulnerability);
          this.logger.warn('Potential vulnerability found', vulnerability);
        }

        // Log interesting responses
        if (response.status >= 500 || duration > 10000) {
          results.errors.push({
            iteration: i,
            payload,
            response: {
              status: response.status,
              statusText: response.statusText,
              duration,
            },
          });
        }

        // Rate limiting
        await this.sleep(1000 / this.config.fuzzing.requestsPerSecond.medium);
      } catch (error) {
        results.errors.push({
          iteration: i,
          error: error.message,
          type: 'request_failed',
        });
        this.logger.error('Request failed', {
          iteration: i,
          error: error.message,
        });
      }
    }

    this.logger.info(`Completed fuzzing ${results.endpoint}`, {
      totalRequests: results.totalRequests,
      errorCount: results.errors.length,
      vulnerabilityCount: results.vulnerabilities.length,
    });

    return results;
  }

  selectMutationType() {
    const types = ['valid', 'boundary', 'invalid', 'security', 'malformed'];
    const weights = [30, 20, 20, 20, 10]; // Percentage weights

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

  generateTestPayload(endpoint, mutationType) {
    // Determine schema based on endpoint
    let schema;

    if (endpoint.path.includes('wallet')) {
      schema = this.config.schemas.wallet;
    } else if (endpoint.path.includes('transaction')) {
      schema = this.config.schemas.transaction;
    } else if (endpoint.path.includes('bridge')) {
      schema = this.config.schemas.bridgeTransfer;
    } else if (endpoint.path.includes('weather')) {
      schema = this.config.schemas.weatherData;
    } else {
      // Generic schema
      schema = {
        type: 'object',
        properties: {
          data: { type: 'string' },
          id: { type: 'string' },
        },
      };
    }

    return this.generateFuzzedData(schema, mutationType);
  }

  async makeRequest(baseUrl, endpoint, payload) {
    const url = `${baseUrl}${endpoint.path}`;
    const method = endpoint.method.toLowerCase();

    const config = {
      method,
      url,
      timeout: this.config.fuzzing.timeouts.request,
      headers: {
        'Content-Type': 'application/json',
        'User-Agent': 'Wrfcoin-Fuzzer/1.0',
      },
    };

    if (method !== 'get') {
      config.data = payload;
    }

    return await axios(config);
  }

  categorizeResponse(response, results) {
    if (response.status >= 200 && response.status < 300) {
      results.responses.success++;
    } else if (response.status >= 400 && response.status < 500) {
      results.responses.clientError++;
    } else if (response.status >= 500) {
      results.responses.serverError++;
    }
  }

  analyzeResponseForVulnerabilities(endpoint, payload, response, duration) {
    const vulnerabilities = [];

    // Check for potential SQL injection
    if (response.data && typeof response.data === 'string') {
      if (
        response.data.includes('SQL syntax') ||
        response.data.includes('mysql_') ||
        response.data.includes('ORA-')
      ) {
        vulnerabilities.push({
          type: 'sql_injection',
          endpoint: endpoint.path,
          payload,
          evidence: 'Database error exposed in response',
        });
      }
    }

    // Check for stack traces
    if (
      response.data &&
      response.data.includes &&
      response.data.includes('Error:')
    ) {
      vulnerabilities.push({
        type: 'information_disclosure',
        endpoint: endpoint.path,
        payload,
        evidence: 'Stack trace exposed in response',
      });
    }

    // Check for timing attacks (very slow responses)
    if (duration > 30000) {
      vulnerabilities.push({
        type: 'denial_of_service',
        endpoint: endpoint.path,
        payload,
        evidence: `Request took ${duration}ms to complete`,
      });
    }

    // Check for server errors with security payloads
    if (
      response.status >= 500 &&
      payload &&
      typeof payload === 'object' &&
      JSON.stringify(payload).includes('<script>')
    ) {
      vulnerabilities.push({
        type: 'xss_potential',
        endpoint: endpoint.path,
        payload,
        evidence: 'Server error with XSS payload',
      });
    }

    return vulnerabilities.length > 0 ? vulnerabilities[0] : null;
  }

  async sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  /**
   * Run comprehensive fuzzing across all configured API servers
   */
  async runComprehensiveFuzzing(
    duration = this.config.fuzzing.duration.standard
  ) {
    const startTime = Date.now();
    const allResults = [];

    this.logger.info('Starting comprehensive REST API fuzzing', {
      duration: duration,
      servers: Object.keys(this.config.apiServers).length,
    });

    for (const [serverName, server] of Object.entries(this.config.apiServers)) {
      this.logger.info(`Fuzzing server: ${server.name}`);

      for (const endpoint of server.endpoints) {
        if (Date.now() - startTime >= duration && duration > 0) {
          this.logger.info('Fuzzing duration reached, stopping');
          break;
        }

        const iterations = Math.floor(
          duration / (server.endpoints.length * 1000)
        );
        const result = await this.fuzzEndpoint(
          server,
          endpoint,
          Math.max(10, iterations)
        );
        allResults.push(result);
      }
    }

    // Generate comprehensive report
    const report = this.generateComprehensiveReport(allResults);
    await this.reporter.saveReport(report, 'rest-api-fuzz-report');

    this.logger.info('Comprehensive REST API fuzzing completed', {
      totalEndpoints: allResults.length,
      totalVulnerabilities: allResults.reduce(
        (sum, r) => sum + r.vulnerabilities.length,
        0
      ),
    });

    return report;
  }

  generateComprehensiveReport(results) {
    const totalRequests = results.reduce((sum, r) => sum + r.totalRequests, 0);
    const totalVulnerabilities = results.reduce(
      (sum, r) => sum + r.vulnerabilities.length,
      0
    );
    const totalErrors = results.reduce((sum, r) => sum + r.errors.length, 0);

    return {
      summary: {
        timestamp: new Date().toISOString(),
        totalEndpoints: results.length,
        totalRequests,
        totalErrors,
        totalVulnerabilities,
        successRate: (
          ((totalRequests - totalErrors) / totalRequests) *
          100
        ).toFixed(2),
      },
      vulnerabilities: results.flatMap(r => r.vulnerabilities),
      errors: results.flatMap(r => r.errors),
      endpointResults: results,
      recommendations: this.generateRecommendations(results),
    };
  }

  generateRecommendations(results) {
    const recommendations = [];

    // Check for endpoints with high error rates
    results.forEach(result => {
      const errorRate = result.errors.length / result.totalRequests;
      if (errorRate > 0.1) {
        recommendations.push({
          type: 'high_error_rate',
          endpoint: result.endpoint,
          message: `High error rate detected (${(errorRate * 100).toFixed(1)}%). Consider improving input validation.`,
        });
      }
    });

    // Check for security vulnerabilities
    const vulnTypes = results.flatMap(r => r.vulnerabilities).map(v => v.type);
    const uniqueVulnTypes = [...new Set(vulnTypes)];

    uniqueVulnTypes.forEach(type => {
      recommendations.push({
        type: 'security_vulnerability',
        vulnType: type,
        message: `${type} vulnerability detected. Implement appropriate security controls.`,
      });
    });

    return recommendations;
  }
}

module.exports = RestApiFuzzer;

// Run if called directly
if (require.main === module) {
  const fuzzer = new RestApiFuzzer();
  fuzzer
    .runComprehensiveFuzzing()
    .then(report => {
      console.log('Fuzzing completed. Report generated.');
      console.log(
        `Total vulnerabilities found: ${report.summary.totalVulnerabilities}`
      );
      process.exit(0);
    })
    .catch(error => {
      console.error('Fuzzing failed:', error);
      process.exit(1);
    });
}
