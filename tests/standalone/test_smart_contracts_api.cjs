#!/usr/bin/env node

/**
 * Comprehensive Smart Contract Graph API Test Suite
 * Tests all backend API endpoints and validates data structure
 */

const axios = require('axios').default;

const BASE_URL = 'http://localhost:3001';

// Test configuration
const EXPECTED_CONTRACTS = 96;
const EXPECTED_RELATIONSHIPS = 153;
const EXPECTED_CATEGORIES = [
  'Core Tokens',
  'DeFi',
  'Governance',
  'Security',
  'Data/Oracles',
  'NFT/Marketplace',
  'Carbon/Climate',
  'Cross-Chain',
  'Infrastructure',
  'Other',
];

class APITester {
  constructor() {
    this.results = {
      passed: 0,
      failed: 0,
      tests: [],
    };
  }

  async test(name, testFn) {
    console.log(`🧪 Testing: ${name}`);
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

  async run() {
    console.log('🚀 Starting Smart Contract Graph API Test Suite\n');

    // Test 1: Main Graph Endpoint
    await this.test('Main Graph API (/api/smart-contracts/graph)', async () => {
      const response = await axios.get(`${BASE_URL}/api/smart-contracts/graph`);

      if (!response.data.success) {
        throw new Error('API returned success: false');
      }

      const data = response.data.data;

      // Validate structure
      if (!data.nodes || !Array.isArray(data.nodes)) {
        throw new Error('Missing or invalid nodes array');
      }

      if (!data.links || !Array.isArray(data.links)) {
        throw new Error('Missing or invalid links array');
      }

      if (!data.categories || typeof data.categories !== 'object') {
        throw new Error('Missing or invalid categories object');
      }

      if (!data.metadata || typeof data.metadata !== 'object') {
        throw new Error('Missing or invalid metadata object');
      }

      // Validate counts
      if (data.nodes.length !== EXPECTED_CONTRACTS) {
        throw new Error(
          `Expected ${EXPECTED_CONTRACTS} contracts, got ${data.nodes.length}`
        );
      }

      if (data.links.length !== EXPECTED_RELATIONSHIPS) {
        throw new Error(
          `Expected ${EXPECTED_RELATIONSHIPS} relationships, got ${data.links.length}`
        );
      }

      return {
        nodeCount: data.nodes.length,
        linkCount: data.links.length,
        categories: Object.keys(data.categories).length,
        cached: response.data.cached,
      };
    });

    // Test 2: Categories Endpoint
    await this.test(
      'Categories API (/api/smart-contracts/categories)',
      async () => {
        const response = await axios.get(
          `${BASE_URL}/api/smart-contracts/categories`
        );

        if (!response.data.success) {
          throw new Error('API returned success: false');
        }

        const categories = response.data.data.categories;
        const categoryNames = Object.keys(categories);

        // Validate all expected categories exist
        for (const expectedCategory of EXPECTED_CATEGORIES) {
          if (!categoryNames.includes(expectedCategory)) {
            throw new Error(`Missing expected category: ${expectedCategory}`);
          }
        }

        // Validate category structure
        for (const [name, config] of Object.entries(categories)) {
          if (!config.color || !config.description) {
            throw new Error(`Category ${name} missing color or description`);
          }

          // Validate color format (hex)
          if (!/^#[0-9A-Fa-f]{6}$/.test(config.color)) {
            throw new Error(
              `Category ${name} has invalid color format: ${config.color}`
            );
          }
        }

        return {
          totalCategories: categoryNames.length,
          categories: categoryNames,
        };
      }
    );

    // Test 3: Stats Endpoint (Fixed URL)
    await this.test('Stats API (/api/smart-contracts/stats)', async () => {
      try {
        const response = await axios.get(
          `${BASE_URL}/api/smart-contracts/stats`
        );

        if (!response.data.success) {
          throw new Error('API returned success: false');
        }

        const stats = response.data.data;

        // Validate required stats fields
        if (typeof stats.totalContracts !== 'number') {
          throw new Error('Missing totalContracts in stats');
        }

        if (typeof stats.totalRelationships !== 'number') {
          throw new Error('Missing totalRelationships in stats');
        }

        if (!stats.categories || typeof stats.categories !== 'object') {
          throw new Error('Missing categories breakdown in stats');
        }

        return {
          totalContracts: stats.totalContracts,
          totalRelationships: stats.totalRelationships,
          hubContracts: stats.hubContracts || 0,
        };
      } catch (error) {
        if (error.response && error.response.status === 404) {
          throw new Error(
            'ROUTING ISSUE: Stats endpoint being caught by :contractName route'
          );
        }
        throw error;
      }
    });

    // Test 4: Search Endpoint (Test routing issue)
    await this.test('Search API (/api/smart-contracts/search)', async () => {
      try {
        const response = await axios.get(
          `${BASE_URL}/api/smart-contracts/search?q=WRF`
        );

        if (!response.data.success) {
          throw new Error('API returned success: false');
        }

        const searchData = response.data.data;

        if (!searchData.contracts || !Array.isArray(searchData.contracts)) {
          throw new Error('Missing contracts array in search results');
        }

        if (typeof searchData.total !== 'number') {
          throw new Error('Missing total count in search results');
        }

        return {
          resultsFound: searchData.total,
          query: searchData.query,
        };
      } catch (error) {
        if (
          error.response &&
          error.response.data &&
          error.response.data.error === "Contract 'search' not found"
        ) {
          throw new Error(
            'ROUTING ISSUE: Search endpoint being caught by :contractName route'
          );
        }
        throw error;
      }
    });

    // Test 5: Individual Contract Endpoint
    await this.test(
      'Individual Contract API (/api/smart-contracts/WRFToken)',
      async () => {
        const response = await axios.get(
          `${BASE_URL}/api/smart-contracts/WRFToken`
        );

        if (!response.data.success) {
          throw new Error('API returned success: false');
        }

        const data = response.data.data;

        if (!data.contract) {
          throw new Error('Missing contract data');
        }

        if (!data.relationships || !Array.isArray(data.relationships)) {
          throw new Error('Missing relationships array');
        }

        if (!data.relatedContracts || !Array.isArray(data.relatedContracts)) {
          throw new Error('Missing relatedContracts array');
        }

        return {
          contractName: data.contract.name,
          relationshipCount: data.relationships.length,
          relatedContractCount: data.relatedContracts.length,
        };
      }
    );

    // Test 6: Cache Management
    await this.test(
      'Cache Refresh API (/api/smart-contracts/refresh-cache)',
      async () => {
        const response = await axios.post(
          `${BASE_URL}/api/smart-contracts/refresh-cache`
        );

        if (!response.data.success) {
          throw new Error('API returned success: false');
        }

        const data = response.data.data;

        if (typeof data.totalContracts !== 'number') {
          throw new Error('Missing totalContracts in refresh response');
        }

        if (typeof data.totalRelationships !== 'number') {
          throw new Error('Missing totalRelationships in refresh response');
        }

        return {
          refreshed: true,
          totalContracts: data.totalContracts,
          totalRelationships: data.totalRelationships,
        };
      }
    );

    // Test 7: Data Structure Validation
    await this.test('Node Data Structure Validation', async () => {
      const response = await axios.get(`${BASE_URL}/api/smart-contracts/graph`);
      const nodes = response.data.data.nodes;

      // Test first few nodes for required fields
      const requiredNodeFields = [
        'id',
        'name',
        'category',
        'color',
        'type',
        'size',
        'complexity',
      ];
      const sampleNodes = nodes.slice(0, 5);

      for (const node of sampleNodes) {
        for (const field of requiredNodeFields) {
          if (!(field in node)) {
            throw new Error(
              `Node ${node.id || 'unknown'} missing required field: ${field}`
            );
          }
        }

        // Validate field types
        if (typeof node.size !== 'number') {
          throw new Error(`Node ${node.id} has invalid size type`);
        }

        if (typeof node.complexity !== 'number') {
          throw new Error(`Node ${node.id} has invalid complexity type`);
        }
      }

      return {
        nodesValidated: sampleNodes.length,
        requiredFields: requiredNodeFields,
      };
    });

    // Test 8: Link Data Structure Validation
    await this.test('Link Data Structure Validation', async () => {
      const response = await axios.get(`${BASE_URL}/api/smart-contracts/graph`);
      const links = response.data.data.links;

      // Test first few links for required fields
      const requiredLinkFields = [
        'source',
        'target',
        'type',
        'strength',
        'value',
      ];
      const sampleLinks = links.slice(0, 5);

      for (const link of sampleLinks) {
        for (const field of requiredLinkFields) {
          if (!(field in link)) {
            throw new Error(`Link missing required field: ${field}`);
          }
        }

        // Validate relationship types
        const validTypes = [
          'import',
          'inheritance',
          'interface',
          'composition',
          'dependency',
        ];
        if (!validTypes.includes(link.type)) {
          throw new Error(`Link has invalid type: ${link.type}`);
        }
      }

      return {
        linksValidated: sampleLinks.length,
        requiredFields: requiredLinkFields,
      };
    });

    // Test 9: Performance Test
    await this.test('API Performance Test', async () => {
      const start = Date.now();
      const response = await axios.get(`${BASE_URL}/api/smart-contracts/graph`);
      const duration = Date.now() - start;

      if (!response.data.success) {
        throw new Error('API call failed');
      }

      // Should be fast due to caching
      if (duration > 5000) {
        // 5 seconds
        console.warn(`⚠️  API response time: ${duration}ms (possibly slow)`);
      }

      return {
        responseTime: duration,
        cached: response.data.cached,
        dataSize: JSON.stringify(response.data).length,
      };
    });

    // Print summary
    this.printSummary();
  }

  printSummary() {
    console.log('\n📊 Test Summary');
    console.log('================');
    console.log(`✅ Passed: ${this.results.passed}`);
    console.log(`❌ Failed: ${this.results.failed}`);
    console.log(
      `📈 Success Rate: ${Math.round((this.results.passed / (this.results.passed + this.results.failed)) * 100)}%\n`
    );

    // Print failed tests details
    const failedTests = this.results.tests.filter(t => t.status === 'FAILED');
    if (failedTests.length > 0) {
      console.log('🚨 Failed Tests Details:');
      console.log('=======================');
      failedTests.forEach(test => {
        console.log(`❌ ${test.name}: ${test.error}`);
      });
      console.log();
    }

    // Print specific routing issues found
    const routingIssues = failedTests.filter(t =>
      t.error.includes('ROUTING ISSUE')
    );
    if (routingIssues.length > 0) {
      console.log('🔧 Routing Issues Detected:');
      console.log('===========================');
      console.log(
        'The :contractName route is catching specific endpoints before they can be processed.'
      );
      console.log(
        'Fix: Move specific routes (search, stats) BEFORE the parameterized route in the server file.'
      );
      console.log();
    }
  }
}

// Run tests
if (require.main === module) {
  const tester = new APITester();
  tester.run().catch(error => {
    console.error('❌ Test suite failed:', error);
    process.exit(1);
  });
}

module.exports = APITester;
